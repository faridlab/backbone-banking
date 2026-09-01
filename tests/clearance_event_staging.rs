//! CES — durable `BankClearanceRecorded` staging (the crash-safe half of the bank-confirmation
//! drift). When the write service carries an outbox schema, a payment-matched `clear_transaction`
//! stages the event into `<schema>.outbox_events` INSIDE the clearance transaction:
//! - CES-1: a payment-matched clear stages exactly one unpublished row — event type, aggregate,
//!   tenant, and payload (payment_id, amounts, journal/post ids) all present for the relay.
//! - CES-2: neither a non-payment clear nor a bank-charge recognition stages anything — the
//!   event's only consumer is payment's bank-confirmation drift, so nothing else emits it.
//! - CES-3: a REFUSED clear stages nothing — the bound refusals roll the whole unit back, event
//!   included; a crashed clear can never have emitted half a confirmation.
//!
//! Requires DATABASE_URL (:5433/backbone_banking with banking migrated).

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope as BankEnv, GlPostAck as BankAck, GlPostRejected as BankRej,
    GlPostSink as BankSink, ReconcileEdgeAck, ReconcilePairRequest, ReconcileRejected,
    ReconcileSink, UnreconcilePairRequest,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewBank, NewBankAccount, NewCharge, NewClearance,
    NewStatementImport, NewStatementLine,
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
#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string()
    });
    PgPool::connect(&url).await.expect("connect DB")
}

// Serialize the outbox bootstrap across this binary's concurrent tests: its policy stanza is
// DROP-then-CREATE, which two racing calls can interleave into "already exists".
static MIGRATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn migrate_outbox(pool: &PgPool) {
    let _guard = MIGRATE.lock().await;
    backbone_outbox::outbox::migrate(pool, "banking")
        .await
        .unwrap();
}

/// The GL sink always accepts (the staging is inside banking's own transaction — whether the
/// ledger accepted is the clearing suite's concern; here only the EVENT's fate is under test).
struct OkGl;
#[async_trait::async_trait]
impl BankSink for OkGl {
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
/// A GL sink that always refuses — for the refused-clear staging probe.
struct FailGl;
#[async_trait::async_trait]
impl BankSink for FailGl {
    async fn post(&self, _env: &BankEnv) -> Result<BankAck, BankRej> {
        Err(BankRej {
            code: "period_closed".into(),
            message: "the period is closed".into(),
        })
    }
}
/// The clearing edge applies exactly what banking asked — a payment-matched clear succeeds.
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

/// A bank account + one imported deposit line; returns (company, line). The outbox is enabled on
/// the service — every clear through it stages (or refuses to stage, which is the point).
async fn fixture(pool: &PgPool, amount: &str, on: u32) -> (Uuid, Uuid, BankingWriteService) {
    let banking = BankingWriteService::new(pool.clone()).with_outbox_schema("banking");
    let company = Uuid::new_v4();
    let bank = banking
        .create_bank(NewBank {
            company_id: company,
            name: uq("Bank"),
            swift_bic: None,
            country: None,
        })
        .await
        .unwrap();
    let acct = banking
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
        .unwrap();
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
                reference_no: None,
                deposit: d(amount),
                withdrawal: Decimal::ZERO,
            }],
        })
        .await
        .unwrap();
    let line: Uuid =
        sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1")
            .bind(import)
            .fetch_one(pool)
            .await
            .unwrap();
    (company, line, banking)
}

async fn staged(
    pool: &PgPool,
    company: Uuid,
) -> Vec<(
    String,
    String,
    String,
    Option<chrono::DateTime<chrono::Utc>>,
)> {
    sqlx::query_as(
        "SELECT event_type, aggregate_type, aggregate_id, published_at FROM banking.outbox_events WHERE company_id=$1",
    )
    .bind(company)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// CES-1 — a payment-matched clear stages exactly one unpublished `BankClearanceRecorded` carrying
/// the drift's full vocabulary: which payment, which tenant, how much, and which journal proves it.
#[tokio::test]
async fn a_payment_matched_clear_stages_the_confirmation_event() {
    let pool = pool().await;
    migrate_outbox(&pool).await;
    let (company, line, banking) = fixture(&pool, "500000", 6).await;
    let payment = Uuid::new_v4();

    let out = banking
        .clear_transaction(
            NewClearance {
                bank_transaction_id: line,
                matched_source_type: "payment".into(),
                matched_source_id: payment,
                matched_source_amount: d("500000"),
                matched_amount: d("500000"),
                match_method: Some("exact".into()),
                clearance_date: day(6),
            },
            &OkGl,
            &OkEdge,
        )
        .await
        .unwrap();

    let rows = staged(&pool, company).await;
    assert_eq!(rows.len(), 1, "exactly one staged event");
    let (ev, agg, agg_id, published) = &rows[0];
    assert_eq!(ev, "BankClearanceRecorded");
    assert_eq!(agg, "BankClearance");
    assert_eq!(
        agg_id,
        &out.clearance_id.to_string(),
        "the aggregate id is the clearance"
    );
    assert!(
        published.is_none(),
        "staged, not yet published — the relay's job"
    );

    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload FROM banking.outbox_events WHERE company_id=$1",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        payload["payment_id"].as_str(),
        Some(payment.to_string().as_str())
    );
    assert_eq!(
        payload["company_id"].as_str(),
        Some(company.to_string().as_str())
    );
    assert_eq!(
        payload["bank_transaction_id"].as_str(),
        Some(line.to_string().as_str())
    );
    assert_eq!(payload["amount"].as_str(), Some("500000"));
    assert_eq!(
        payload["journal_id"].as_str(),
        Some(out.journal_id.to_string().as_str())
    );
    assert_eq!(
        payload["post_id"].as_str(),
        Some(out.post_id.to_string().as_str())
    );
}

/// CES-2 — the event exists for exactly one reason: payment's bank-confirmation drift. A clear
/// matched to a non-payment document, and a bank-charge recognition, both commit their clearance
/// rows and stage NOTHING.
#[tokio::test]
async fn non_payment_clears_and_charges_stage_nothing() {
    let pool = pool().await;
    migrate_outbox(&pool).await;

    // A non-payment clear (an invoice): commits, stages nothing.
    let (company, line, banking) = fixture(&pool, "300000", 6).await;
    banking
        .clear_transaction(
            NewClearance {
                bank_transaction_id: line,
                matched_source_type: "invoice".into(),
                matched_source_id: Uuid::new_v4(),
                matched_source_amount: d("300000"),
                matched_amount: d("300000"),
                match_method: Some("exact".into()),
                clearance_date: day(6),
            },
            &OkGl,
            &OkEdge,
        )
        .await
        .unwrap();
    assert!(
        staged(&pool, company).await.is_empty(),
        "an invoice-matched clear stages no confirmation"
    );

    // A bank charge: commits, stages nothing.
    let (company2, line2, banking2) = fixture(&pool, "150000", 7).await;
    banking2
        .recognize_bank_charge(
            NewCharge {
                bank_transaction_id: line2,
                charge_account_id: Uuid::new_v4(),
                amount: d("150000"),
                clearance_date: day(7),
            },
            &OkGl,
        )
        .await
        .unwrap();
    assert!(
        staged(&pool, company2).await.is_empty(),
        "a bank charge stages no confirmation"
    );
}

/// CES-3 — a refused clear stages nothing. Both refusal classes: the settlement bound (the clear
/// never reaches the post) and the GL rejection (the post refuses; the whole unit rolls back with
/// the event it would have staged).
#[tokio::test]
async fn a_refused_clear_stages_nothing() {
    let pool = pool().await;
    migrate_outbox(&pool).await;

    // Bound refusal: attempt to clear MORE than the line holds.
    let (company, line, banking) = fixture(&pool, "100000", 6).await;
    let e = banking
        .clear_transaction(
            NewClearance {
                bank_transaction_id: line,
                matched_source_type: "payment".into(),
                matched_source_id: Uuid::new_v4(),
                matched_source_amount: d("999999"),
                matched_amount: d("200000"),
                match_method: None,
                clearance_date: day(6),
            },
            &OkGl,
            &OkEdge,
        )
        .await
        .unwrap_err();
    assert!(matches!(e, BankingError::OverAllocated { .. }));
    assert!(
        staged(&pool, company).await.is_empty(),
        "a bound-refused clear stages nothing"
    );

    // GL refusal: the sink rejects the post; the clearance, the allocation advance, and the event
    // all roll back together.
    let e = banking
        .clear_transaction(
            NewClearance {
                bank_transaction_id: line,
                matched_source_type: "payment".into(),
                matched_source_id: Uuid::new_v4(),
                matched_source_amount: d("100000"),
                matched_amount: d("100000"),
                match_method: None,
                clearance_date: day(6),
            },
            &FailGl,
            &OkEdge,
        )
        .await
        .unwrap_err();
    assert!(matches!(e, BankingError::GlRejected { .. }));
    assert!(
        staged(&pool, company).await.is_empty(),
        "a GL-refused clear stages nothing"
    );
    let clearances: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM banking.bank_clearances WHERE company_id=$1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(clearances, 0, "and no clearance row survives either");
    let (alloc, status): (Decimal, String) = sqlx::query_as(
        "SELECT allocated_amount, status::text FROM banking.bank_transactions WHERE id=$1",
    )
    .bind(line)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (alloc, status),
        (Decimal::ZERO, "unreconciled".to_string()),
        "the line is untouched"
    );
}
