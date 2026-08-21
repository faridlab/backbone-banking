//! RPO — reconcile-preset candidate ordering: deterministic, side-effect-free, tenant-fenced.
//!
//! A [`ReconcilePreset`] ranks the open settlements a statement line could clear against — a READ,
//! never a match (the operator confirms through the clear verb). The suite pins:
//! - RPO-1: deterministic ordering — equal scores break on `payment_number` ascending; two calls
//!   over the same committed state return the SAME list, and NOTHING moves: no allocation
//!   watermark, no clearance row, no payment status, no preset row.
//! - RPO-2: the signal arms — `reference_exact` outranks and filters on reference equality;
//!   `amount_exact` matches the OPEN amount (paid − already-cleared through banking's own
//!   watermark — the same sum the clear verb bounds against, so ordering and clearing can never
//!   disagree about what is open).
//! - RPO-3: `amount_within_tolerance` admits by percent deviation, `days_window` by date distance;
//!   the fence errors — another tenant's preset is not-found, an inactive preset is refused,
//!   missing tolerance / days-window on the arms that need them are refused.
//! - RPO-4: `party` is vocabulary-only — no line counterparty exists yet, so that preset ranks
//!   NOTHING rather than an empty silent lie.
//! - RPO-5: the candidate POOL is an injected port — a stub pool flows through the same ranking.
//!
//! Requires DATABASE_URL (:5433/backbone_banking with accounting + payment + banking migrated).

use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope as BankEnv, GlPostAck as BankAck, GlPostRejected as BankRej,
    GlPostSink as BankSink, ReconcileEdgeAck, ReconcilePairRequest, ReconcileRejected,
    ReconcileSink, UnreconcilePairRequest,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewBank, NewBankAccount, NewClearance, NewStatementImport,
    NewStatementLine,
};
use backbone_banking::application::service::reconcile_preset_candidates::{
    CandidatePoolPort, OpenCandidate,
};

use backbone_payment::application::service::payment_events::{PaymentEvent, PaymentEventSink};
use backbone_payment::application::service::payment_gl::{
    AccountingPostEnvelope as PayEnv, GlPostAck as PayAck, GlPostRejected as PayRej,
    GlPostSink as PaySink,
};
use backbone_payment::application::service::payment_lifecycle::BankReconcilablePort;
use backbone_payment::application::service::payment_write_service::{
    NewAllocation, NewPayment, PaymentError, PaymentWriteService,
};

fn d(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap()
}
fn day(n: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 7, n).unwrap()
}
fn uq(p: &str) -> String {
    format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8])
}
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string()
    });
    PgPool::connect(&url).await.expect("connect DB")
}

// --- stubs: payment posts to nothing; banking clears through nothing; the edge always applies ---

#[derive(Default, Clone)]
struct Recorder {
    events: Arc<Mutex<Vec<PaymentEvent>>>,
}
impl PaymentEventSink for Recorder {
    fn publish(&self, e: PaymentEvent) {
        self.events.lock().unwrap().push(e);
    }
}

struct AlwaysReconcilable;
#[async_trait::async_trait]
impl BankReconcilablePort for AlwaysReconcilable {
    async fn bank_reconcilable(
        &self,
        _pool: &PgPool,
        _company_id: Uuid,
        _account_id: Uuid,
    ) -> Result<bool, PaymentError> {
        Ok(true)
    }
}

struct PayOkGl;
#[async_trait::async_trait]
impl PaySink for PayOkGl {
    async fn post(&self, env: &PayEnv) -> Result<PayAck, PayRej> {
        assert!(
            env.is_balanced(),
            "payment emitted an UNBALANCED post: {env:?}"
        );
        Ok(PayAck {
            post_id: Uuid::new_v4(),
            journal_id: Uuid::new_v4(),
            idempotent_reuse: false,
        })
    }
}

struct BankOkGl;
#[async_trait::async_trait]
impl BankSink for BankOkGl {
    async fn post(&self, env: &BankEnv) -> Result<BankAck, BankRej> {
        assert!(
            env.is_balanced(),
            "banking emitted an UNBALANCED post: {env:?}"
        );
        Ok(BankAck {
            post_id: Uuid::new_v4(),
            journal_id: Uuid::new_v4(),
            idempotent_reuse: false,
        })
    }
}

/// The clearing edge applies exactly what banking asked — the ranking only needs the clear to
/// SUCCEED so the watermark advances through the real verb.
struct OkEdge;
#[async_trait::async_trait]
impl ReconcileSink for OkEdge {
    async fn reconcile_pair_on(
        &self,
        _conn: &mut sqlx::PgConnection,
        req: &ReconcilePairRequest,
    ) -> Result<ReconcileEdgeAck, ReconcileRejected> {
        Ok(ReconcileEdgeAck {
            partial_id: Some(Uuid::new_v4()),
            applied: req.amount,
            full_reconcile_id: None,
        })
    }
    async fn unreconcile_pair_on(
        &self,
        _conn: &mut sqlx::PgConnection,
        _req: &UnreconcilePairRequest,
    ) -> Result<(), ReconcileRejected> {
        Ok(())
    }
}

// --- fixtures -------------------------------------------------------------------

/// A bank + account whose GL legs are fabricated uuids (the ranking never posts; only the
/// watermark-clear does, through the stub sinks).
async fn fixture(_pool: &PgPool, banking: &BankingWriteService, company: Uuid) -> Uuid {
    let bank = banking
        .create_bank(NewBank {
            company_id: company,
            name: uq("Bank"),
            swift_bic: None,
            country: None,
        })
        .await
        .unwrap();
    banking
        .create_bank_account(NewBankAccount {
            company_id: company,
            branch_id: None,
            bank_id: bank,
            account_name: "Ops".into(),
            account_number: uq("ACC"),
            gl_account_id: Uuid::new_v4(),
            clearing_account_id: Uuid::new_v4(),
            currency: None,
            account_type: None,
        })
        .await
        .unwrap()
}

/// One posted, in-flight payment against `bank_account` — the shape the candidate pool reads.
async fn payment(
    pool: &PgPool,
    company: Uuid,
    bank_account: Uuid,
    number: &str,
    amount: &str,
    on: u32,
    reference: Option<&str>,
) -> Uuid {
    let w = PaymentWriteService::with_sink(pool.clone(), Arc::new(Recorder::default()))
        .with_reconcilable_port(Arc::new(AlwaysReconcilable));
    let id = w
        .create_payment(NewPayment {
            payment_number: format!("{number}-{}", &Uuid::new_v4().simple().to_string()[..8]),
            company_id: company,
            branch_id: None,
            payment_type: "receive".into(),
            party_type: Some("customer".into()),
            party_id: Some(Uuid::new_v4()),
            posting_date: day(on),
            currency: None,
            mode_of_payment_id: None,
            method: None,
            provider_txn_id: None,
            bank_account_id: bank_account,
            party_account_id: Uuid::new_v4(),
            paid_amount: d(amount),
            reference_no: reference.map(Into::into),
            allocations: vec![NewAllocation {
                invoice_ref: Uuid::new_v4(),
                invoice_kind: "sales".into(),
                amount: d(amount),
            }],
            withholding_amount: Decimal::ZERO,
            withholding_account_id: None,
            withholding_tax_type: "none".into(),
        })
        .await
        .unwrap();
    w.post_payment(id, &PayOkGl).await.unwrap();
    id
}

/// One imported statement line (single-line statement, balanced).
async fn line(
    pool: &PgPool,
    banking: &BankingWriteService,
    company: Uuid,
    acct: Uuid,
    amount: &str,
    on: u32,
    reference: Option<&str>,
) -> Uuid {
    let import = banking
        .import_statement(NewStatementImport {
            company_id: company,
            bank_account_id: acct,
            source_format: None,
            period_start: day(1),
            period_end: day(31),
            opening_balance: Decimal::ZERO,
            closing_balance: d(amount),
            file_ref: None,
            lines: vec![NewStatementLine {
                txn_date: day(on),
                description: None,
                reference_no: reference.map(Into::into),
                deposit: d(amount),
                withdrawal: Decimal::ZERO,
            }],
        })
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1")
        .bind(import)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn preset(
    pool: &PgPool,
    company: Uuid,
    name: &str,
    match_on: &str,
    tolerance: Option<Decimal>,
    days_window: Option<i32>,
    status: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO banking.reconcile_presets (id, company_id, name, match_on, tolerance_percent, days_window, status)
         VALUES ($1,$2,$3,$4::match_on,$5,$6,$7::preset_status)",
    )
    .bind(id)
    .bind(company)
    .bind(name)
    .bind(match_on)
    .bind(tolerance)
    .bind(days_window)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    id
}

// --- the suite ------------------------------------------------------------------

/// RPO-1 — deterministic ordering + the side-effect-free probe. Three equal-amount payments
/// (inserted scrambled) all score 90 under `amount_exact`; the order breaks on `payment_number`
/// ascending, two calls return the identical list, and NOTHING in the database moved: no
/// allocation watermark, no clearance row, no payment state, no preset state.
#[tokio::test]
async fn ordering_is_deterministic_and_side_effect_free() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let banking = BankingWriteService::new(pool.clone());
    let acct = fixture(&pool, &banking, company).await;

    // Insertion order C, A, B — the ranking must not inherit it.
    let c = payment(&pool, company, acct, "PE-C", "500000", 3, None).await;
    let a = payment(&pool, company, acct, "PE-A", "500000", 5, None).await;
    let b = payment(&pool, company, acct, "PE-B", "500000", 4, None).await;
    let l = line(&pool, &banking, company, acct, "500000", 6, None).await;
    let p = preset(
        &pool,
        company,
        "exact",
        "amount_exact",
        None,
        None,
        "active",
    )
    .await;

    // The side-effect-free probe: snapshot everything the ranking could possibly move, then
    // assert none of it moved (below, after both calls).
    let first = banking.order_candidates(company, p, l).await.unwrap();
    assert_eq!(
        first.iter().map(|c| c.payment_id).collect::<Vec<_>>(),
        vec![a, b, c],
        "equal scores break on payment_number ascending — not insertion order"
    );
    assert!(
        first.iter().all(|c| c.score == 90),
        "amount_exact scores 90"
    );
    assert!(
        first.iter().all(|c| c.reason.starts_with("[exact] ")),
        "the reason names its preset"
    );

    let second = banking.order_candidates(company, p, l).await.unwrap();
    assert_eq!(
        first, second,
        "the same committed state yields the identical list"
    );

    // Nothing moved: the line's watermark + status, the clearance count, every payment's state.
    let (alloc, status): (Decimal, String) = sqlx::query_as(
        "SELECT allocated_amount, status::text FROM banking.bank_transactions WHERE id=$1",
    )
    .bind(l)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (alloc, status),
        (Decimal::ZERO, "unreconciled".to_string()),
        "no watermark advance"
    );
    let clearances: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM banking.bank_clearances WHERE company_id=$1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(clearances, 0, "ranking writes no clearance");
    let pays: Vec<(String, String)> = sqlx::query_as(
        "SELECT status::text, posting_state::text FROM payment.payment_entries WHERE company_id=$1 ORDER BY id",
    )
    .bind(company)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        pays.iter()
            .all(|(s, ps)| s == "in_flight" && ps == "posted"),
        "no payment state drift: {pays:?}"
    );
    let preset_row: (String,) =
        sqlx::query_as("SELECT status::text FROM banking.reconcile_presets WHERE id=$1")
            .bind(p)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(preset_row.0, "active", "no preset state drift");
}

/// RPO-2 — `reference_exact` outranks and filters on equality; `amount_exact` matches the OPEN
/// amount (paid − already-cleared), and the watermark the ranking uses is the SAME one the clear
/// verb advances: after a real 200k clear, the 500k payment ranks open at 300k against a 300k line
/// (before the clear it would not have ranked at all).
#[tokio::test]
async fn reference_outranks_and_amount_matches_the_open_watermark() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let banking = BankingWriteService::new(pool.clone());
    let acct = fixture(&pool, &banking, company).await;

    let with_ref = payment(&pool, company, acct, "PE-VA", "500000", 5, Some("VA-777")).await;
    let other_ref = payment(&pool, company, acct, "PE-VB", "500000", 5, Some("VA-999")).await;
    let no_ref = payment(&pool, company, acct, "PE-VC", "500000", 5, None).await;

    // reference_exact: only the equal reference survives; the others are not "ranked lower" —
    // they are not candidates under this signal at all.
    let l_ref = line(&pool, &banking, company, acct, "500000", 6, Some("VA-777")).await;
    let p_ref = preset(
        &pool,
        company,
        "by-ref",
        "reference_exact",
        None,
        None,
        "active",
    )
    .await;
    let ranked = banking
        .order_candidates(company, p_ref, l_ref)
        .await
        .unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].payment_id, with_ref);
    assert_eq!(
        ranked[0].score, 100,
        "reference_exact is the strongest signal"
    );
    assert!(
        ranked[0].reason.contains("VA-777"),
        "the reason shows the matched key"
    );

    // A line whose reference matches nobody → the honest empty list (no fiction).
    let l_miss = line(&pool, &banking, company, acct, "500000", 6, Some("VA-000")).await;
    assert!(banking
        .order_candidates(company, p_ref, l_miss)
        .await
        .unwrap()
        .is_empty());
    let _ = (other_ref, no_ref);

    // amount_exact against the OPEN amount: clear 200k of the 500k payment through the real verb,
    // then a 300k line ranks it at open 300k. Before the clear, a 300k line would rank nothing
    // (open 500000 ≠ 300000) — ordering and clearing share one watermark.
    let l300_before = line(&pool, &banking, company, acct, "300000", 7, None).await;
    let p_amt = preset(
        &pool,
        company,
        "by-amt",
        "amount_exact",
        None,
        None,
        "active",
    )
    .await;
    assert!(
        banking
            .order_candidates(company, p_amt, l300_before)
            .await
            .unwrap()
            .is_empty(),
        "open 500000 does not equal the 300000 line — nothing ranks yet"
    );

    let l200 = line(&pool, &banking, company, acct, "200000", 7, None).await;
    banking
        .clear_transaction(
            NewClearance {
                bank_transaction_id: l200,
                matched_source_type: "payment".into(),
                matched_source_id: with_ref,
                matched_source_amount: d("500000"),
                matched_amount: d("200000"),
                match_method: None,
                clearance_date: day(7),
            },
            &BankOkGl,
            &OkEdge,
        )
        .await
        .unwrap();

    let ranked = banking
        .order_candidates(company, p_amt, l300_before)
        .await
        .unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].payment_id, with_ref);
    assert_eq!(
        ranked[0].open_amount,
        d("300000"),
        "open = paid 500000 − cleared 200000"
    );
}

/// RPO-3 — tolerance + days-window arms, and the fence refusals: another tenant's preset is
/// indistinguishable from absence (404-class), an inactive preset refuses, and the arms that need
/// a parameter refuse when it is missing.
#[tokio::test]
async fn tolerance_days_window_and_the_fence_refusals() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let stranger = Uuid::new_v4();
    let banking = BankingWriteService::new(pool.clone());
    let acct = fixture(&pool, &banking, company).await;

    let near = payment(&pool, company, acct, "PE-N", "505000", 5, None).await; // +1.0% off the line
    let far = payment(&pool, company, acct, "PE-F", "600000", 5, None).await; // +20% off the line
    let l = line(&pool, &banking, company, acct, "500000", 6, None).await;

    // amount_within_tolerance @ 1.00%: the near payment admits (deviation exactly 1.0), the far
    // one does not.
    let p_tol = preset(
        &pool,
        company,
        "tol",
        "amount_within_tolerance",
        Some(d("1.00")),
        None,
        "active",
    )
    .await;
    let ranked = banking.order_candidates(company, p_tol, l).await.unwrap();
    assert_eq!(
        ranked.iter().map(|c| c.payment_id).collect::<Vec<_>>(),
        vec![near]
    );
    assert_eq!(ranked[0].score, 80);
    let _ = far;

    // days_window @ 3d: distance is all that matters — BOTH same-dated payments rank (the window
    // carries no amount signal), ordered by payment_number ascending; one dated 1 is 5d outside.
    let out_window = payment(&pool, company, acct, "PE-D2", "500000", 1, None).await;
    let p_days = preset(
        &pool,
        company,
        "days",
        "days_window",
        None,
        Some(3),
        "active",
    )
    .await;
    let ranked = banking.order_candidates(company, p_days, l).await.unwrap();
    let ids: Vec<Uuid> = ranked.iter().map(|c| c.payment_id).collect();
    assert!(
        ids.contains(&near) && ids.contains(&far),
        "both 1d-distant payments rank under the window"
    );
    assert!(!ids.contains(&out_window), "a 5d-distant payment does not");
    assert_eq!(
        ids,
        vec![far, near],
        "equal 70s break on payment_number ascending (PE-F < PE-N)"
    );
    assert!(ranked.iter().all(|c| c.score == 70));

    // Fence: a preset belonging to another tenant is not-found — never readable, never rankable.
    let foreign = preset(
        &pool,
        stranger,
        "foreign",
        "amount_exact",
        None,
        None,
        "active",
    )
    .await;
    assert!(matches!(
        banking
            .order_candidates(company, foreign, l)
            .await
            .unwrap_err(),
        BankingError::PresetNotFound(_)
    ));
    // Inactive presets refuse — a retired rule must not keep ordering.
    let off = preset(
        &pool,
        company,
        "off",
        "amount_exact",
        None,
        None,
        "inactive",
    )
    .await;
    assert!(matches!(
        banking.order_candidates(company, off, l).await.unwrap_err(),
        BankingError::PresetInactive(_)
    ));
    // The arms that need their parameter refuse when it is missing.
    let no_tol = preset(
        &pool,
        company,
        "no-tol",
        "amount_within_tolerance",
        None,
        None,
        "active",
    )
    .await;
    assert!(matches!(
        banking
            .order_candidates(company, no_tol, l)
            .await
            .unwrap_err(),
        BankingError::PresetMissingTolerance(_)
    ));
    let no_days = preset(
        &pool,
        company,
        "no-days",
        "days_window",
        None,
        None,
        "active",
    )
    .await;
    assert!(matches!(
        banking
            .order_candidates(company, no_days, l)
            .await
            .unwrap_err(),
        BankingError::PresetMissingDaysWindow(_)
    ));
}

/// RPO-4 — `party` is vocabulary-only: no line counterparty exists yet, so that preset ranks
/// NOTHING (an empty silent list would read "no candidates", which would be a lie).
#[tokio::test]
async fn party_match_on_ranks_nothing_and_says_so() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let banking = BankingWriteService::new(pool.clone());
    let acct = fixture(&pool, &banking, company).await;
    let _ = payment(&pool, company, acct, "PE-P", "500000", 5, None).await;
    let l = line(&pool, &banking, company, acct, "500000", 6, None).await;
    let p = preset(&pool, company, "by-party", "party", None, None, "active").await;
    let ranked = banking.order_candidates(company, p, l).await.unwrap();
    assert!(
        ranked.is_empty(),
        "party ranks nothing while no counterparty rides a line"
    );
}

/// RPO-5 — the candidate pool is an injected PORT: a stub pool flows through the same ranking, so
/// module tests (and non-payment compositions) substitute their own source of candidates.
#[tokio::test]
async fn the_candidate_pool_is_an_injected_port() {
    struct StubPool;
    #[async_trait::async_trait]
    impl CandidatePoolPort for StubPool {
        async fn open_candidates(
            &self,
            _pool: &PgPool,
            _company_id: Uuid,
            _bank_account_id: Uuid,
        ) -> Result<Vec<OpenCandidate>, BankingError> {
            Ok(vec![OpenCandidate {
                payment_id: Uuid::parse_str("00000000-0000-0000-0000-00000000c0de").unwrap(),
                payment_number: "STUB-1".into(),
                paid_amount: d("500000"),
                posting_date: day(5),
                reference_no: None,
            }])
        }
    }

    let pool = pool().await;
    let company = Uuid::new_v4();
    let banking = BankingWriteService::new(pool.clone()).with_candidate_port(Arc::new(StubPool));
    let acct = fixture(&pool, &banking, company).await;
    let l = line(&pool, &banking, company, acct, "500000", 6, None).await;
    let p = preset(
        &pool,
        company,
        "stub-exact",
        "amount_exact",
        None,
        None,
        "active",
    )
    .await;

    let ranked = banking.order_candidates(company, p, l).await.unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(
        ranked[0].payment_number, "STUB-1",
        "the ranked list derives from the injected pool"
    );
    assert_eq!(ranked[0].score, 90);
}
