//! The cash-management CLEARING seam, end-to-end across THREE modules: **payment → accounting →
//! banking → accounting** — the bank-side leg that closes the cash position. Zero normal Cargo edges
//! (payment + accounting are dev-deps only).
//!
//! Flow: a payment settles to a **clearing** account (`Dr Clearing · Cr A/R` into the real ledger) —
//! undeposited funds. Banking imports the bank statement, matches the deposit line to that payment,
//! and posts the **clearing leg** (`Dr Bank · Cr Clearing`). The clearing account nets to ZERO (the
//! money has now truly landed in the bank), A/R stays settled, and the bank GL holds the cash —
//! AND the clearing's reconciliation-graph edge lands in the same unit of work (through the in-test
//! `AccountingReconcileSink`, the composing host's shape), pairing the payment's clearing leg with
//! the clearance's so the undeposited-funds position provably closes on the ledger.
//! Requires DATABASE_URL (:5433/backbone_banking with accounting + payment + banking migrated).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_banking::application::service::banking_events::{BankingEvent, BankingEventSink};
use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope as BankEnv, GlPostAck as BankAck, GlPostRejected as BankRej, GlPostSink as BankSink,
    ReconcileEdgeAck, ReconcileLine, ReconcileOrigin, ReconcilePairRequest, ReconcileRejected,
    ReconcileSink, UnreconcilePairRequest,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, MatchCandidate, NewBank, NewBankAccount, NewClearance, NewStatementImport, NewStatementLine,
};

use backbone_payment::application::service::payment_gl::{
    AccountingPostEnvelope as PayEnv, GlPostAck as PayAck, GlPostRejected as PayRej, GlPostSink as PaySink,
};
use backbone_payment::application::service::payment_write_service::{NewPayment, PaymentWriteService};

use backbone_accounting::application::service::posting_service::{PostingLine, PostingRequest, PostingService};
use backbone_accounting::application::service::reconcile_write_service::ReconcileWriteService;
use backbone_accounting::domain::reconcile_graph::{LineLocator, PairRequest};
use backbone_accounting::infrastructure::persistence::{SqlxPostingRepository, SqlxReconcileGraphRepository};

/// ACL: either producer's serialized envelope → accounting's PostingRequest against the REAL ledger.
struct GlAdapter { svc: PostingService }
impl GlAdapter {
    async fn go(&self, company_id: Uuid, source_type: &str, source_id: Uuid, source_reference: Option<String>,
        posting_date: chrono::NaiveDate, posting_type: &str, lines: Vec<PostingLine>) -> Result<(Uuid, Uuid, bool), (String, String)> {
        let mut r = PostingRequest::original(company_id, source_type, source_id, posting_date);
        r.source_reference = source_reference;
        r.posting_type = posting_type.to_string();
        r.lines = lines;
        match self.svc.post(r, None).await {
            Ok(x) => Ok((x.post_id, x.journal_id, x.idempotent_reuse)),
            Err(x) => Err((x.code().to_string(), x.to_string())),
        }
    }
}
#[async_trait::async_trait]
impl PaySink for GlAdapter {
    async fn post(&self, e: &PayEnv) -> Result<PayAck, PayRej> {
        let lines = e.lines.iter().map(|l| PostingLine {
            account_id: l.account_id, debit: l.debit, credit: l.credit, party_type: l.party_type.clone(),
            party_id: l.party_id, cost_center_id: None, project_id: None, department_id: None, description: l.description.clone(),
        }).collect();
        match self.go(e.company_id, &e.source_type, e.source_id, e.source_reference.clone(), e.posting_date, &e.posting_type, lines).await {
            Ok((post_id, journal_id, idempotent_reuse)) => Ok(PayAck { post_id, journal_id, idempotent_reuse }),
            Err((code, message)) => Err(PayRej { code, message }),
        }
    }
}
#[async_trait::async_trait]
impl BankSink for GlAdapter {
    async fn post(&self, e: &BankEnv) -> Result<BankAck, BankRej> {
        let lines = e.lines.iter().map(|l| PostingLine {
            account_id: l.account_id, debit: l.debit, credit: l.credit, party_type: l.party_type.clone(),
            party_id: l.party_id, cost_center_id: None, project_id: None, department_id: None, description: l.description.clone(),
        }).collect();
        match self.go(e.company_id, &e.source_type, e.source_id, e.source_reference.clone(), e.posting_date, &e.posting_type, lines).await {
            Ok((post_id, journal_id, idempotent_reuse)) => Ok(BankAck { post_id, journal_id, idempotent_reuse }),
            Err((code, message)) => Err(BankRej { code, message }),
        }
    }
}

#[derive(Default, Clone)]
struct RecordingBankSink { events: Arc<Mutex<Vec<BankingEvent>>> }
impl BankingEventSink for RecordingBankSink { fn publish(&self, e: BankingEvent) { self.events.lock().unwrap().push(e); } }

/// ACL: the reconciliation port over accounting's write service — the composing host's shape,
/// implemented through BANKING's own `banking_gl` re-export of the shared contract.
struct AccountingReconcileSink { svc: ReconcileWriteService }
impl AccountingReconcileSink {
    fn new(pool: &PgPool) -> Self {
        Self {
            svc: ReconcileWriteService::new(
                Arc::new(SqlxReconcileGraphRepository::new()),
                Arc::new(SqlxPostingRepository::new(pool.clone())),
                pool.clone(),
                None,
            ),
        }
    }
}
#[async_trait::async_trait]
impl ReconcileSink for AccountingReconcileSink {
    async fn reconcile_pair_on(&self, conn: &mut sqlx::PgConnection, req: &ReconcilePairRequest) -> Result<ReconcileEdgeAck, ReconcileRejected> {
        let origin = match req.origin {
            ReconcileOrigin::Settlement => "settlement",
            ReconcileOrigin::Clearing => "clearing",
            ReconcileOrigin::Manual => "manual",
        };
        let to_loc = |l: &ReconcileLine| LineLocator {
            source_type: l.source_type.clone(), source_id: l.source_id,
            account_id: l.account_id, reversing: l.reversing,
        };
        match self.svc.reconcile_pair_on(conn, &PairRequest {
            company_id: req.company_id, debit: to_loc(&req.debit), credit: to_loc(&req.credit),
            amount: req.amount, origin: origin.to_string(), actor: None,
        }).await {
            Ok(o) => Ok(ReconcileEdgeAck { partial_id: o.partial_id, applied: o.applied, full_reconcile_id: o.full_reconcile_id }),
            Err(e) => Err(ReconcileRejected { code: e.code().to_string(), message: e.to_string() }),
        }
    }
    async fn unreconcile_pair_on(&self, conn: &mut sqlx::PgConnection, req: &UnreconcilePairRequest) -> Result<(), ReconcileRejected> {
        let to_loc = |l: &ReconcileLine| LineLocator {
            source_type: l.source_type.clone(), source_id: l.source_id,
            account_id: l.account_id, reversing: l.reversing,
        };
        self.svc.unreconcile_pair_on(conn, req.company_id, &to_loc(&req.debit), &to_loc(&req.credit)).await
            .map_err(|e| ReconcileRejected { code: e.code().to_string(), message: e.to_string() })
    }
}

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day(n: u32) -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, n).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}
/// A/R + clearing + bank chart. `clearing_reconcilable` flags the clearing account for the graph
/// (guard G3) — the happy paths set it true; the fail-closed probe seeds it false.
async fn seed(pool: &PgPool, clearing_reconcilable: bool) -> (Uuid, HashMap<&'static str, Uuid>) {
    let company = Uuid::new_v4();
    let coa: &[(&str, &str, &str, &str, &str, bool)] = &[
        ("1200", "Piutang Usaha", "asset", "accounts_receivable", "debit", true),
        ("2100", "Utang Usaha", "liability", "accounts_payable", "credit", true),
        ("1190", "Dana Belum Disetor", "asset", "current_asset", "debit", clearing_reconcilable),  // clearing / undeposited funds
        ("1110", "Bank BCA", "asset", "bank", "debit", false),
    ];
    let mut m = HashMap::new();
    for (code, name, at, st, nb, rec) in coa {
        let id = Uuid::new_v4();
        sqlx::query(r#"INSERT INTO accounting.accounts (id, company_id, account_number, account_code, name, account_type, account_subtype, normal_balance, is_header, is_detail, is_reconcilable, status)
            VALUES ($1,$2,$3,$4,$5,$6::account_type,$7::account_subtype,$8::normal_balance,false,true,$9,'active'::account_status)"#)
            .bind(id).bind(company).bind(code).bind(code).bind(name).bind(at).bind(st).bind(nb).bind(rec)
            .execute(pool).await.expect("seed acct");
        m.insert(*code, id);
    }
    (company, m)
}
async fn seed_coa(pool: &PgPool) -> (Uuid, HashMap<&'static str, Uuid>) {
    seed(pool, true).await
}
async fn balance(pool: &PgPool, account: Uuid) -> Decimal {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_amount),0) - COALESCE(SUM(credit_amount),0) FROM accounting.ledgers WHERE account_id=$1")
        .bind(account).fetch_one(pool).await.unwrap()
}

// --- reconciliation-graph reads (the ledger-side proof of every clearing) ---------------------

/// Σ clearing edges + their count for the company.
async fn clearing_edges(pool: &PgPool, company: Uuid) -> (Decimal, i64) {
    sqlx::query_as("SELECT COALESCE(SUM(amount),0), COUNT(*) FROM accounting.partial_reconciles WHERE company_id=$1 AND origin='clearing'")
        .bind(company).fetch_one(pool).await.unwrap()
}
/// A clearing-account line's residual — its signed amount minus every partial touching it. The
/// account pin matters: a source posts SEVERAL lines (the clearance posts Bank + Clearing), all
/// stamped with the same source identity; only the clearing leg carries the edge.
async fn residual(pool: &PgPool, company: Uuid, source_type: &str, source_id: Uuid, account: Uuid) -> Decimal {
    sqlx::query_scalar(
        r#"SELECT (CASE WHEN base_debit_amount > 0 THEN base_debit_amount ELSE base_credit_amount END)
                 - COALESCE((SELECT SUM(p.amount) FROM accounting.partial_reconciles p WHERE p.debit_move_id=jl.id),0)
                 - COALESCE((SELECT SUM(p.amount) FROM accounting.partial_reconciles p WHERE p.credit_move_id=jl.id),0)
             FROM accounting.journal_lines jl
            WHERE jl.company_id=$1 AND jl.source_type=$2 AND jl.source_id=$3 AND jl.account_id=$4 AND jl.is_posted"#,
    )
    .bind(company).bind(source_type).bind(source_id).bind(account)
    .fetch_one(pool).await.unwrap()
}
async fn full_groups(pool: &PgPool, company: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM accounting.full_reconciles WHERE company_id=$1")
        .bind(company).fetch_one(pool).await.unwrap()
}

/// CLSEAM-1: undeposited-funds clearing across payment, banking, and the real ledger.
#[tokio::test]
async fn clearing_nets_undeposited_funds_across_three_modules() {
    let pool = pool().await;
    let (company, coa) = seed_coa(&pool).await;
    let customer = Uuid::new_v4();

    let payment = PaymentWriteService::new(pool.clone());
    let recorder = RecordingBankSink::default();
    let banking = BankingWriteService::with_sink(pool.clone(), Arc::new(recorder.clone()));
    let gl = GlAdapter { svc: PostingService::new(Arc::new(SqlxPostingRepository::new(pool.clone()))) };
    let sink = AccountingReconcileSink::new(&pool);

    // 1) payment: a receive settles to the CLEARING account (undeposited funds) — Dr Clearing · Cr A/R.
    let va_ref = uq("VA");
    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "receive".into(),
        party_type: Some("customer".into()), party_id: Some(customer), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["1200"], paid_amount: d("750000"),
        reference_no: Some(va_ref.clone()), allocations: vec![], withholding_amount: Decimal::ZERO, withholding_account_id: None, withholding_tax_type: "none".into(),
    }).await.unwrap();
    let pp = payment.post_payment(pay, &gl).await.unwrap();
    assert_eq!(journal_totals(&pool, pp.journal_id).await, (d("750000"), d("750000")));
    assert_eq!(balance(&pool, coa["1190"]).await, d("750000.00"), "clearing holds undeposited funds");

    // 2) banking: bank + account (real Bank + Clearing), import the statement showing the deposit landed.
    let bank = banking.create_bank(NewBank { company_id: company, name: uq("BCA"), swift_bic: Some("CENAIDJA".into()), country: None }).await.unwrap();
    let acct = banking.create_bank_account(NewBankAccount {
        company_id: company, branch_id: None, bank_id: bank, account_name: "Ops".into(), account_number: uq("ACC"),
        gl_account_id: coa["1110"], clearing_account_id: coa["1190"], currency: None, account_type: Some("virtual_account".into()),
    }).await.unwrap();
    let import_id = banking.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: Some("csv".into()), period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("750000"), file_ref: None,
        lines: vec![NewStatementLine { txn_date: day(6), description: Some("Incoming VA".into()), reference_no: Some(va_ref.clone()), deposit: d("750000"), withdrawal: Decimal::ZERO }],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();

    // 3) match the statement line to the payment (candidate supplied by the composition, from payment).
    let cand = MatchCandidate { source_type: "payment".into(), source_id: pay, amount: d("750000"), reference: Some(va_ref) };
    let matched = banking.propose_match(txn, &[cand]).await.unwrap().expect("matched the payment");
    assert_eq!(matched.source_id, pay);

    // 4) clear it → Dr Bank · Cr Clearing into the REAL ledger + the clearing edge on the graph.
    let out = banking.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: matched.source_type, matched_source_id: matched.source_id,
        matched_source_amount: matched.amount, matched_amount: matched.amount, match_method: Some("exact".into()), clearance_date: day(6),
    }, &gl, &sink).await.unwrap();
    assert!(out.fully_reconciled);
    assert_eq!(journal_totals(&pool, out.journal_id).await, (d("750000"), d("750000")));

    // 5) the cash position is now correct:
    assert_eq!(balance(&pool, coa["1190"]).await, d("0.00"), "clearing nets to zero — funds no longer undeposited");
    assert_eq!(balance(&pool, coa["1110"]).await, d("750000.00"), "the bank GL holds the cash");
    assert_eq!(balance(&pool, coa["1200"]).await, d("-750000.00"), "A/R stays settled (customer paid)");
    // 6) the graph proves it: one clearing edge pairing the payment's Dr Clearing leg with the
    //    clearance's Cr Clearing leg; both residuals consumed; a full group stamps the pair.
    assert_eq!(clearing_edges(&pool, company).await, (d("750000.00"), 1), "one clearing edge of the matched amount");
    assert_eq!(residual(&pool, company, "payment", pay, coa["1190"]).await, Decimal::ZERO, "payment leg consumed");
    assert_eq!(residual(&pool, company, "settlement", out.clearance_id, coa["1190"]).await, Decimal::ZERO, "clearance leg consumed");
    assert_eq!(full_groups(&pool, company).await, 1, "the clearing pair closes into a full group");

    // event carries the matched payment for downstream (payment can mark itself bank-confirmed).
    let evts = recorder.events.lock().unwrap().clone();
    assert!(evts.iter().any(|e| matches!(e, BankingEvent::BankTransactionCleared(c) if c.matched_source_id == pay && c.amount == d("750000.00"))));
}

async fn journal_totals(pool: &PgPool, jid: Uuid) -> (Decimal, Decimal) {
    let r = sqlx::query("SELECT total_debit, total_credit FROM accounting.journals WHERE id=$1").bind(jid).fetch_one(pool).await.unwrap();
    (r.get("total_debit"), r.get("total_credit"))
}

async fn bank_account(banking: &BankingWriteService, company: Uuid, bank_gl: Uuid, clearing: Uuid) -> Uuid {
    let bank = banking.create_bank(NewBank { company_id: company, name: uq("Bank"), swift_bic: None, country: None }).await.unwrap();
    banking.create_bank_account(NewBankAccount {
        company_id: company, branch_id: None, bank_id: bank, account_name: "Ops".into(), account_number: uq("ACC"),
        gl_account_id: bank_gl, clearing_account_id: clearing, currency: None, account_type: None,
    }).await.unwrap()
}
async fn two_lines(pool: &PgPool, banking: &BankingWriteService, company: Uuid, acct: Uuid, a: &str, b: &str, closing: &str) -> (Uuid, Uuid) {
    let import_id = banking.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d(closing), file_ref: None,
        lines: vec![
            NewStatementLine { txn_date: day(6), description: None, reference_no: None, deposit: d(a), withdrawal: Decimal::ZERO },
            NewStatementLine { txn_date: day(6), description: None, reference_no: None, deposit: d(b), withdrawal: Decimal::ZERO },
        ],
    }).await.unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1 ORDER BY deposit DESC").bind(import_id).fetch_all(pool).await.unwrap();
    (ids[0], ids[1])
}

/// CLSEAM-2 (council 2026-07-05, skeptic): a settlement cannot be cleared twice. One payment settles
/// 500,000; a duplicate/re-imported statement shows the SAME deposit on two lines, both matching that
/// payment. Clearing the first is fine; the second is refused (`settlement_over_cleared`) — so the
/// clearing account stays at 0 and the bank GL is not overstated. Without the settlement bound the
/// second clear posts a phantom Cr Clearing 500,000 → clearing −500,000, bank overstated 500,000.
#[tokio::test]
async fn a_settlement_cannot_be_cleared_twice() {
    let pool = pool().await;
    let (company, coa) = seed_coa(&pool).await;
    let customer = Uuid::new_v4();
    let payment = PaymentWriteService::new(pool.clone());
    let banking = BankingWriteService::new(pool.clone());
    let gl = GlAdapter { svc: PostingService::new(Arc::new(SqlxPostingRepository::new(pool.clone()))) };
    let sink = AccountingReconcileSink::new(&pool);

    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "receive".into(),
        party_type: Some("customer".into()), party_id: Some(customer), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["1200"], paid_amount: d("500000"),
        reference_no: None, allocations: vec![], withholding_amount: Decimal::ZERO, withholding_account_id: None, withholding_tax_type: "none".into(),
    }).await.unwrap();
    payment.post_payment(pay, &gl).await.unwrap();

    let acct = bank_account(&banking, company, coa["1110"], coa["1190"]).await;
    let (l1, l2) = two_lines(&pool, &banking, company, acct, "500000", "500000", "1000000").await;

    // clear line 1 against the payment — fine.
    banking.clear_transaction(NewClearance {
        bank_transaction_id: l1, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("500000"), matched_amount: d("500000"), match_method: Some("exact".into()), clearance_date: day(6),
    }, &gl, &sink).await.unwrap();
    // clear line 2 against the SAME payment — refused; the settlement is already fully cleared.
    let e = banking.clear_transaction(NewClearance {
        bank_transaction_id: l2, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("500000"), matched_amount: d("500000"), match_method: Some("exact".into()), clearance_date: day(6),
    }, &gl, &sink).await.unwrap_err();
    assert!(matches!(e, BankingError::SettlementOverCleared { .. }));

    assert_eq!(balance(&pool, coa["1190"]).await, d("0.00"), "clearing stays at zero — no phantom credit");
    assert_eq!(balance(&pool, coa["1110"]).await, d("500000.00"), "bank GL not overstated");
    // The refused clear never touched the graph: exactly one clearing edge for the FIRST clear.
    assert_eq!(clearing_edges(&pool, company).await, (d("500000.00"), 1));
}

/// CLSEAM-3 (council 2026-07-05): the discriminator that proves the amount-bound is right, not a
/// unique constraint. One payment 750,000 legitimately lands as TWO bank deposits (500,000 + 250,000);
/// both clear against that payment (a unique constraint would reject the second), and the clearing
/// account still nets to zero.
#[tokio::test]
async fn one_settlement_splits_across_two_lines() {
    let pool = pool().await;
    let (company, coa) = seed_coa(&pool).await;
    let customer = Uuid::new_v4();
    let payment = PaymentWriteService::new(pool.clone());
    let banking = BankingWriteService::new(pool.clone());
    let gl = GlAdapter { svc: PostingService::new(Arc::new(SqlxPostingRepository::new(pool.clone()))) };
    let sink = AccountingReconcileSink::new(&pool);

    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "receive".into(),
        party_type: Some("customer".into()), party_id: Some(customer), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["1200"], paid_amount: d("750000"),
        reference_no: None, allocations: vec![], withholding_amount: Decimal::ZERO, withholding_account_id: None, withholding_tax_type: "none".into(),
    }).await.unwrap();
    payment.post_payment(pay, &gl).await.unwrap();

    let acct = bank_account(&banking, company, coa["1110"], coa["1190"]).await;
    let (l1, l2) = two_lines(&pool, &banking, company, acct, "500000", "250000", "750000").await;

    let c1 = banking.clear_transaction(NewClearance {
        bank_transaction_id: l1, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("750000"), matched_amount: d("500000"), match_method: None, clearance_date: day(6),
    }, &gl, &sink).await.unwrap();
    let c2 = banking.clear_transaction(NewClearance {
        bank_transaction_id: l2, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("750000"), matched_amount: d("250000"), match_method: None, clearance_date: day(6),
    }, &gl, &sink).await.unwrap();

    assert_eq!(balance(&pool, coa["1190"]).await, d("0.00"), "clearing nets to zero across the split");
    assert_eq!(balance(&pool, coa["1110"]).await, d("750000.00"), "bank GL holds the full payment");
    // The graph splits with the bank bookkeeping: one edge per deposit, each of its own matched
    // amount; the payment's leg is consumed by BOTH; the connected trio closes into one group.
    let (sum, n) = clearing_edges(&pool, company).await;
    assert_eq!((sum, n), (d("750000.00"), 2), "one clearing edge per deposit, totalling the payment");
    assert_eq!(residual(&pool, company, "payment", pay, coa["1190"]).await, Decimal::ZERO);
    assert_eq!(residual(&pool, company, "settlement", c1.clearance_id, coa["1190"]).await, Decimal::ZERO);
    assert_eq!(residual(&pool, company, "settlement", c2.clearance_id, coa["1190"]).await, Decimal::ZERO);
    assert_eq!(full_groups(&pool, company).await, 1, "the split trio closes into one group");
}

/// CLSEAM-4 (fail-closed): the clearing edge is a CONDITION of the clear, not decoration — when the
/// graph refuses the pair (here: the clearing account is not flagged reconcilable, a CoA config
/// error), the WHOLE clear rolls back: no `BankClearance` row, the statement line's allocation and
/// status not advanced. The operator fixes the account flag and re-clears; the settlement fence did
/// not advance, so nothing was consumed. (The clearing POST itself commits via its own sink before
/// the refusal — the stranded journal is the documented residue, caught by integrity probes.)
#[tokio::test]
async fn a_refused_clearing_edge_rolls_the_clear_back() {
    let pool = pool().await;
    let (company, coa) = seed(&pool, false).await; // clearing account NOT reconcilable
    let customer = Uuid::new_v4();
    let payment = PaymentWriteService::new(pool.clone());
    let banking = BankingWriteService::new(pool.clone());
    let gl = GlAdapter { svc: PostingService::new(Arc::new(SqlxPostingRepository::new(pool.clone()))) };
    let sink = AccountingReconcileSink::new(&pool);

    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "receive".into(),
        party_type: Some("customer".into()), party_id: Some(customer), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["1200"], paid_amount: d("400000"),
        reference_no: None, allocations: vec![], withholding_amount: Decimal::ZERO, withholding_account_id: None, withholding_tax_type: "none".into(),
    }).await.unwrap();
    payment.post_payment(pay, &gl).await.unwrap();

    let acct = bank_account(&banking, company, coa["1110"], coa["1190"]).await;
    let (l1, _l2) = two_lines(&pool, &banking, company, acct, "400000", "100000", "500000").await;

    let e = banking.clear_transaction(NewClearance {
        bank_transaction_id: l1, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("400000"), matched_amount: d("400000"), match_method: None, clearance_date: day(6),
    }, &gl, &sink).await.unwrap_err();
    assert!(matches!(&e, BankingError::ReconcileRefused { code, .. } if code == "account_not_reconcilable"),
        "the graph's refusal surfaces verbatim, got: {e:?}");

    // The clear rolled back: no clearance row, the line's allocation never advanced.
    let cleared: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM banking.bank_clearances WHERE company_id=$1")
        .bind(company).fetch_one(&pool).await.unwrap();
    assert_eq!(cleared, 0, "no clearance row committed");
    let (alloc, status): (Decimal, String) = sqlx::query_as(
        "SELECT allocated_amount, status::text FROM banking.bank_transactions WHERE id=$1")
        .bind(l1).fetch_one(&pool).await.unwrap();
    assert_eq!(alloc, Decimal::ZERO, "allocation not advanced");
    assert_eq!(status, "unreconciled");
    // No graph state was written either.
    assert_eq!(clearing_edges(&pool, company).await, (Decimal::ZERO, 0));
    assert_eq!(full_groups(&pool, company).await, 0);
}

/// CLSEAM-5 (direction): the paid-out flow posts the mirror — `Dr A/P · Cr Clearing` on the payment,
/// `Dr Clearing · Cr Bank` on the clearance — so the clearing edge's DEBIT is the clearance's leg and
/// its CREDIT the payment's. Proving the direction mapping matters: a flipped edge would be refused
/// by the opposite-directions guard, and silently skipping it would leave the clearing position open.
#[tokio::test]
async fn paid_out_clearing_edges_the_mirror_direction() {
    let pool = pool().await;
    let (company, coa) = seed_coa(&pool).await;
    let supplier = Uuid::new_v4();
    let payment = PaymentWriteService::new(pool.clone());
    let banking = BankingWriteService::new(pool.clone());
    let gl = GlAdapter { svc: PostingService::new(Arc::new(SqlxPostingRepository::new(pool.clone()))) };
    let sink = AccountingReconcileSink::new(&pool);

    // A pay settles through the clearing account — Dr A/P · Cr Clearing (undeposited outflow).
    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "pay".into(),
        party_type: Some("supplier".into()), party_id: Some(supplier), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["2100"], paid_amount: d("300000"),
        reference_no: None, allocations: vec![], withholding_amount: Decimal::ZERO, withholding_account_id: None, withholding_tax_type: "none".into(),
    }).await.unwrap();
    payment.post_payment(pay, &gl).await.unwrap();
    assert_eq!(balance(&pool, coa["1190"]).await, d("-300000.00"), "clearing holds the undeposited outflow");

    let acct = bank_account(&banking, company, coa["1110"], coa["1190"]).await;
    let import_id = banking.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("-300000"), file_ref: None,
        lines: vec![NewStatementLine { txn_date: day(6), description: None, reference_no: None, deposit: Decimal::ZERO, withdrawal: d("300000") }],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();

    let out = banking.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("300000"), matched_amount: d("300000"), match_method: None, clearance_date: day(6),
    }, &gl, &sink).await.unwrap();
    assert!(out.fully_reconciled);

    // The outflow landed; the clearing position closed.
    assert_eq!(balance(&pool, coa["1190"]).await, d("0.00"), "clearing nets to zero — the outflow left");
    assert_eq!(balance(&pool, coa["1110"]).await, d("-300000.00"), "the bank GL shows the withdrawal");
    // The graph edge landed with the MIRROR direction: debit = the clearance's leg (it debits
    // clearing), credit = the payment's (it credited clearing) — both consumed, one group.
    assert_eq!(clearing_edges(&pool, company).await, (d("300000.00"), 1));
    assert_eq!(residual(&pool, company, "payment", pay, coa["1190"]).await, Decimal::ZERO, "payment's credit leg consumed");
    assert_eq!(residual(&pool, company, "settlement", out.clearance_id, coa["1190"]).await, Decimal::ZERO, "clearance's debit leg consumed");
    assert_eq!(full_groups(&pool, company).await, 1);
}
