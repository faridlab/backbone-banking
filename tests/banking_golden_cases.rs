//! Golden oracle for the banking write path (import → match → clear → reconcile). Banking-only — the
//! clearing seam into the real ledger + payment is proven in `clearing_seam.rs`. Clearing here uses a
//! FAKE `GlPostSink`. Requires DATABASE_URL (:5433/backbone_banking).

use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_banking::application::service::banking_events::{BankingEvent, BankingEventSink};
use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope, GlPostAck, GlPostRejected, GlPostSink,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, MatchCandidate, NewBank, NewBankAccount, NewCharge, NewClearance,
    NewReconciliation, NewStatementImport, NewStatementLine,
};

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day(n: u32) -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, n).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}

#[derive(Default, Clone)]
struct FakeGl { seen: Arc<Mutex<Vec<AccountingPostEnvelope>>> }
impl FakeGl { fn last(&self) -> AccountingPostEnvelope { self.seen.lock().unwrap().last().unwrap().clone() } }
#[async_trait::async_trait]
impl GlPostSink for FakeGl {
    async fn post(&self, env: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        assert!(env.is_balanced(), "banking emitted an UNBALANCED post: {env:?}");
        self.seen.lock().unwrap().push(env.clone());
        Ok(GlPostAck { post_id: Uuid::new_v4(), journal_id: Uuid::new_v4(), idempotent_reuse: false })
    }
}
#[derive(Default, Clone)]
struct Recorder { events: Arc<Mutex<Vec<BankingEvent>>> }
impl BankingEventSink for Recorder { fn publish(&self, e: BankingEvent) { self.events.lock().unwrap().push(e); } }

async fn account(w: &BankingWriteService, company: Uuid, bank_gl: Uuid, clearing: Uuid) -> Uuid {
    let bank = w.create_bank(NewBank { company_id: company, name: uq("Bank"), swift_bic: None, country: None }).await.unwrap();
    w.create_bank_account(NewBankAccount {
        company_id: company, branch_id: None, bank_id: bank, account_name: "Ops".into(),
        account_number: uq("ACC"), gl_account_id: bank_gl, clearing_account_id: clearing,
        currency: None, account_type: None,
    }).await.unwrap()
}
fn line(dep: &str, wd: &str, refno: Option<&str>) -> NewStatementLine {
    NewStatementLine { txn_date: day(5), description: None, reference_no: refno.map(|s| s.to_string()), deposit: d(dep), withdrawal: d(wd) }
}

// BGC-1: import validates balance continuity — opening 1,000,000 + deposit 500,000 − withdrawal 200,000
// = closing 1,300,000; a wrong closing is rejected.
#[tokio::test]
async fn import_balance_continuity() {
    let pool = pool().await;
    let rec = Recorder::default();
    let w = BankingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let company = Uuid::new_v4();
    let acct = account(&w, company, Uuid::new_v4(), Uuid::new_v4()).await;

    let ok = NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: d("1000000"), closing_balance: d("1300000"), file_ref: None,
        lines: vec![line("500000", "0", None), line("0", "200000", None)],
    };
    let import_id = w.import_statement(ok).await.unwrap();
    let (rows, st): (i32, String) = sqlx::query_as("SELECT row_count, status::text FROM banking.bank_statement_imports WHERE id=$1")
        .bind(import_id).fetch_one(&pool).await.unwrap();
    assert_eq!(rows, 2);
    assert_eq!(st, "imported");
    assert!(rec.events.lock().unwrap().iter().any(|e| matches!(e, BankingEvent::BankStatementImported(s) if s.import_id == import_id)));

    // wrong closing → balance_mismatch
    let bad = NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: d("1000000"), closing_balance: d("9999999"), file_ref: None,
        lines: vec![line("500000", "0", None)],
    };
    assert!(matches!(w.import_statement(bad).await.unwrap_err(), BankingError::BalanceMismatch { .. }));
    // empty statement
    let empty = NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: Decimal::ZERO, file_ref: None, lines: vec![],
    };
    assert!(matches!(w.import_statement(empty).await.unwrap_err(), BankingError::EmptyStatement));
}

// BGC-2: propose_match prefers exact amount + reference, else exact amount, else none.
#[tokio::test]
async fn propose_match_amount_and_reference() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let acct = account(&w, company, Uuid::new_v4(), Uuid::new_v4()).await;
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("500000"), file_ref: None,
        lines: vec![line("500000", "0", Some("VA-12345"))],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();
    let pay_a = Uuid::new_v4(); let pay_b = Uuid::new_v4();
    let cands = vec![
        MatchCandidate { source_type: "payment".into(), source_id: pay_a, amount: d("500000"), reference: Some("OTHER".into()) },
        MatchCandidate { source_type: "payment".into(), source_id: pay_b, amount: d("500000"), reference: Some("VA-12345".into()) },
    ];
    let m = w.propose_match(txn, &cands).await.unwrap().unwrap();
    assert_eq!(m.source_id, pay_b, "reference match wins the tie on amount");
    // no candidate at the amount → None
    let none = w.propose_match(txn, &[MatchCandidate { source_type: "payment".into(), source_id: Uuid::new_v4(), amount: d("999"), reference: None }]).await.unwrap();
    assert!(none.is_none());
}

// BGC-3: clearing a received deposit posts Dr Bank · Cr Clearing and marks the line reconciled.
#[tokio::test]
async fn clear_deposit_posts_bank_over_clearing() {
    let pool = pool().await;
    let rec = Recorder::default();
    let w = BankingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let company = Uuid::new_v4();
    let (bank_gl, clearing) = (Uuid::new_v4(), Uuid::new_v4());
    let acct = account(&w, company, bank_gl, clearing).await;
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("750000"), file_ref: None,
        lines: vec![line("750000", "0", Some("VA-9"))],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();

    let gl = FakeGl::default();
    let out = w.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: Uuid::new_v4(),
        matched_source_amount: d("750000"), matched_amount: d("750000"), match_method: Some("exact".into()), clearance_date: day(6),
    }, &gl).await.unwrap();
    assert!(out.fully_reconciled);
    let env = gl.last();
    assert_eq!(env.totals(), (d("750000.00"), d("750000.00")));
    assert_eq!(env.lines.iter().find(|l| l.account_id == bank_gl).unwrap().debit, d("750000.00"));
    assert_eq!(env.lines.iter().find(|l| l.account_id == clearing).unwrap().credit, d("750000.00"));
    let st: String = sqlx::query_scalar("SELECT status::text FROM banking.bank_transactions WHERE id=$1").bind(txn).fetch_one(&pool).await.unwrap();
    assert_eq!(st, "reconciled");
    assert!(rec.events.lock().unwrap().iter().any(|e| matches!(e, BankingEvent::BankTransactionCleared(c) if c.bank_transaction_id == txn)));
}

// BGC-4: a paid withdrawal clears Dr Clearing · Cr Bank; a partial clear leaves the line partly_reconciled.
#[tokio::test]
async fn clear_withdrawal_and_partial() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let (bank_gl, clearing) = (Uuid::new_v4(), Uuid::new_v4());
    let acct = account(&w, company, bank_gl, clearing).await;
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: d("1000000"), closing_balance: d("600000"), file_ref: None,
        lines: vec![line("0", "400000", None)],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();
    let gl = FakeGl::default();
    // partial clear 250,000 of 400,000
    let out = w.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: Uuid::new_v4(),
        matched_source_amount: d("250000"), matched_amount: d("250000"), match_method: None, clearance_date: day(6),
    }, &gl).await.unwrap();
    assert!(!out.fully_reconciled);
    let env = gl.last();
    assert_eq!(env.lines.iter().find(|l| l.account_id == clearing).unwrap().debit, d("250000.00"));
    assert_eq!(env.lines.iter().find(|l| l.account_id == bank_gl).unwrap().credit, d("250000.00"));
    let (al, st): (Decimal, String) = sqlx::query_as("SELECT allocated_amount, status::text FROM banking.bank_transactions WHERE id=$1").bind(txn).fetch_one(&pool).await.unwrap();
    assert_eq!(al, d("250000.00"));
    assert_eq!(st, "partly_reconciled");
    // clear the remaining 150,000 → reconciled
    let out2 = w.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: Uuid::new_v4(),
        matched_source_amount: d("150000"), matched_amount: d("150000"), match_method: None, clearance_date: day(6),
    }, &gl).await.unwrap();
    assert!(out2.fully_reconciled);
}

// BGC-5: a bank charge posts Dr Bank Charges · Cr Bank.
#[tokio::test]
async fn bank_charge_posts_expense_over_bank() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let (bank_gl, clearing, charges) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let acct = account(&w, company, bank_gl, clearing).await;
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: d("100000"), closing_balance: d("85000"), file_ref: None,
        lines: vec![line("0", "15000", None)],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();
    let gl = FakeGl::default();
    w.recognize_bank_charge(NewCharge { bank_transaction_id: txn, charge_account_id: charges, amount: d("15000"), clearance_date: day(6) }, &gl).await.unwrap();
    let env = gl.last();
    assert_eq!(env.lines.iter().find(|l| l.account_id == charges).unwrap().debit, d("15000.00"));
    assert_eq!(env.lines.iter().find(|l| l.account_id == bank_gl).unwrap().credit, d("15000.00"));
}

// BGC-6: reconcile closes when the statement closing balance equals the ledger balance.
#[tokio::test]
async fn reconcile_closes_when_balanced() {
    let pool = pool().await;
    let rec = Recorder::default();
    let w = BankingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let company = Uuid::new_v4();
    let acct = account(&w, company, Uuid::new_v4(), Uuid::new_v4()).await;
    // No lines imported for this fresh account → zero exceptions, so diff-0 → closed.
    let out = w.reconcile(NewReconciliation {
        company_id: company, bank_account_id: acct, from_date: day(1), to_date: day(31),
        statement_closing_balance: d("1300000"), ledger_balance: d("1300000"),
    }).await.unwrap();
    assert_eq!(out.difference, d("0.00"));
    assert_eq!(out.status, "closed");
    assert_eq!(out.unreconciled_count, 0);
    assert!(rec.events.lock().unwrap().iter().any(|e| matches!(e, BankingEvent::BankReconciliationClosed(_))));
    // unbalanced stays open
    let out2 = w.reconcile(NewReconciliation {
        company_id: company, bank_account_id: acct, from_date: day(1), to_date: day(31),
        statement_closing_balance: d("1300000"), ledger_balance: d("1250000"),
    }).await.unwrap();
    assert_eq!(out2.difference, d("50000.00"));
    assert_eq!(out2.status, "open");
}

// BGC-7 (completeness council 2026-07-05): a session cannot close over unreconciled lines. Even when
// the two supplied balances agree (diff 0 — trivially arranged), open lines in the period force the
// session to `balanced` (NOT `closed`) and NO `BankReconciliationClosed` is emitted — so the control
// never falsely attests "the bank agrees with our books". Once the lines are reconciled, it closes.
#[tokio::test]
async fn reconcile_with_open_lines_stays_balanced_not_closed() {
    let pool = pool().await;
    let rec = Recorder::default();
    let w = BankingWriteService::with_sink(pool.clone(), Arc::new(rec.clone()));
    let company = Uuid::new_v4();
    let (bank_gl, clearing) = (Uuid::new_v4(), Uuid::new_v4());
    let acct = account(&w, company, bank_gl, clearing).await;
    // import a statement with one deposit line, left unreconciled.
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("500000"), file_ref: None,
        lines: vec![line("500000", "0", None)],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();

    // diff 0 but one line still unreconciled → balanced, NOT closed, no event, count 1.
    let out = w.reconcile(NewReconciliation {
        company_id: company, bank_account_id: acct, from_date: day(1), to_date: day(31),
        statement_closing_balance: d("500000"), ledger_balance: d("500000"),
    }).await.unwrap();
    assert_eq!(out.difference, d("0.00"));
    assert_eq!(out.status, "balanced", "cannot close with an open line");
    assert_eq!(out.unreconciled_count, 1);
    assert!(!rec.events.lock().unwrap().iter().any(|e| matches!(e, BankingEvent::BankReconciliationClosed(_))), "no false attestation");
    let persisted: (String, i32) = sqlx::query_as("SELECT status::text, unreconciled_count FROM banking.bank_reconciliations WHERE id=$1").bind(out.id).fetch_one(&pool).await.unwrap();
    assert_eq!(persisted, ("balanced".to_string(), 1), "exception snapshot is persisted");

    // reconcile the line, then the session closes.
    let gl = FakeGl::default();
    w.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: Uuid::new_v4(),
        matched_source_amount: d("500000"), matched_amount: d("500000"), match_method: None, clearance_date: day(6),
    }, &gl).await.unwrap();
    let out2 = w.reconcile(NewReconciliation {
        company_id: company, bank_account_id: acct, from_date: day(1), to_date: day(31),
        statement_closing_balance: d("500000"), ledger_balance: d("500000"),
    }).await.unwrap();
    assert_eq!(out2.status, "closed");
    assert_eq!(out2.unreconciled_count, 0);
    assert!(rec.events.lock().unwrap().iter().any(|e| matches!(e, BankingEvent::BankReconciliationClosed(_))));
}
