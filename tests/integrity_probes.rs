//! Integrity probes for banking — invariants that must hold against a REAL Postgres beyond the golden
//! math. Requires DATABASE_URL (:5433/backbone_banking).

use std::sync::Arc;

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope, GlPostAck, GlPostRejected, GlPostSink,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewBank, NewBankAccount, NewClearance, NewStatementImport,
    NewStatementLine,
};

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day(n: u32) -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, n).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}

struct OkGl;
#[async_trait::async_trait]
impl GlPostSink for OkGl {
    async fn post(&self, _e: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        Ok(GlPostAck { post_id: Uuid::new_v4(), journal_id: Uuid::new_v4(), idempotent_reuse: false })
    }
}
struct RejectingGl;
#[async_trait::async_trait]
impl GlPostSink for RejectingGl {
    async fn post(&self, _e: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        Err(GlPostRejected { code: "period_closed".into(), message: "closed".into() })
    }
}

// IP-1: a clearance cannot exceed the line's un-allocated remainder — over-clearing is refused and the
// line's allocation is untouched.
#[tokio::test]
async fn over_clearing_is_refused() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let bank = w.create_bank(NewBank { company_id: company, name: uq("Bank"), swift_bic: None, country: None }).await.unwrap();
    let acct = w.create_bank_account(NewBankAccount {
        company_id: company, branch_id: None, bank_id: bank, account_name: "Ops".into(), account_number: uq("ACC"),
        gl_account_id: Uuid::new_v4(), clearing_account_id: Uuid::new_v4(), currency: None, account_type: None,
    }).await.unwrap();
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("500000"), file_ref: None,
        lines: vec![NewStatementLine { txn_date: day(5), description: None, reference_no: None, deposit: d("500000"), withdrawal: Decimal::ZERO }],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();

    let e = w.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: Uuid::new_v4(),
        matched_source_amount: d("600000"), matched_amount: d("600000"), match_method: None, clearance_date: day(6),
    }, &OkGl).await.unwrap_err();
    assert!(matches!(e, BankingError::OverAllocated { .. }));
    let al: Decimal = sqlx::query_scalar("SELECT allocated_amount FROM banking.bank_transactions WHERE id=$1").bind(txn).fetch_one(&pool).await.unwrap();
    assert_eq!(al, d("0.00"), "a refused clearance leaves the line untouched");
}

// IP-2: a rejected GL post surfaces as an error and writes no clearance / no allocation.
#[tokio::test]
async fn rejected_clear_writes_nothing() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let bank = w.create_bank(NewBank { company_id: company, name: uq("Bank"), swift_bic: None, country: None }).await.unwrap();
    let acct = w.create_bank_account(NewBankAccount {
        company_id: company, branch_id: None, bank_id: bank, account_name: "Ops".into(), account_number: uq("ACC"),
        gl_account_id: Uuid::new_v4(), clearing_account_id: Uuid::new_v4(), currency: None, account_type: None,
    }).await.unwrap();
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("300000"), file_ref: None,
        lines: vec![NewStatementLine { txn_date: day(5), description: None, reference_no: None, deposit: d("300000"), withdrawal: Decimal::ZERO }],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();
    let e = w.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: Uuid::new_v4(),
        matched_source_amount: d("300000"), matched_amount: d("300000"), match_method: None, clearance_date: day(6),
    }, &RejectingGl).await.unwrap_err();
    assert!(matches!(e, BankingError::GlRejected { .. }));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM banking.bank_clearances WHERE bank_transaction_id=$1").bind(txn).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0, "no clearance row on a rejected post");
    let al: Decimal = sqlx::query_scalar("SELECT allocated_amount FROM banking.bank_transactions WHERE id=$1").bind(txn).fetch_one(&pool).await.unwrap();
    assert_eq!(al, d("0.00"));
}

// IP-3: a non-IDR statement line is refused at clear time (no mis-valued clearing reaches the ledger).
#[tokio::test]
async fn non_idr_refused_at_clear() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let bank = w.create_bank(NewBank { company_id: company, name: uq("Bank"), swift_bic: None, country: None }).await.unwrap();
    let acct = w.create_bank_account(NewBankAccount {
        company_id: company, branch_id: None, bank_id: bank, account_name: "Ops".into(), account_number: uq("ACC"),
        gl_account_id: Uuid::new_v4(), clearing_account_id: Uuid::new_v4(), currency: None, account_type: None,
    }).await.unwrap();
    let import_id = w.import_statement(NewStatementImport {
        company_id: company, bank_account_id: acct, source_format: None, period_start: day(1), period_end: day(31),
        opening_balance: Decimal::ZERO, closing_balance: d("100000"), file_ref: None,
        lines: vec![NewStatementLine { txn_date: day(5), description: None, reference_no: None, deposit: d("100000"), withdrawal: Decimal::ZERO }],
    }).await.unwrap();
    let txn: Uuid = sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1").bind(import_id).fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE banking.bank_transactions SET currency='USD' WHERE id=$1").bind(txn).execute(&pool).await.unwrap();
    let e = w.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: "payment".into(), matched_source_id: Uuid::new_v4(),
        matched_source_amount: d("100000"), matched_amount: d("100000"), match_method: None, clearance_date: day(6),
    }, &OkGl).await.unwrap_err();
    assert!(matches!(e, BankingError::UnsupportedCurrency(c) if c == "USD"));
}
