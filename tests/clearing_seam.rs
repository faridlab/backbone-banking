//! The cash-management CLEARING seam, end-to-end across THREE modules: **payment → accounting →
//! banking → accounting** — the bank-side leg that closes the cash position. Zero normal Cargo edges
//! (payment + accounting are dev-deps only).
//!
//! Flow: a payment settles to a **clearing** account (`Dr Clearing · Cr A/R` into the real ledger) —
//! undeposited funds. Banking imports the bank statement, matches the deposit line to that payment,
//! and posts the **clearing leg** (`Dr Bank · Cr Clearing`). The clearing account nets to ZERO (the
//! money has now truly landed in the bank), A/R stays settled, and the bank GL holds the cash.
//! Requires DATABASE_URL (:5433/backbone_banking with accounting + payment + banking migrated).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_banking::application::service::banking_events::{BankingEvent, BankingEventSink};
use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope as BankEnv, GlPostAck as BankAck, GlPostRejected as BankRej, GlPostSink as BankSink,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, MatchCandidate, NewBank, NewBankAccount, NewClearance, NewStatementImport, NewStatementLine,
};

use backbone_payment::application::service::payment_gl::{
    AccountingPostEnvelope as PayEnv, GlPostAck as PayAck, GlPostRejected as PayRej, GlPostSink as PaySink,
};
use backbone_payment::application::service::payment_write_service::{NewPayment, PaymentWriteService};

use backbone_accounting::application::service::posting_service::{PostingLine, PostingRequest, PostingService};

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

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }
fn day(n: u32) -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 7, n).unwrap() }
fn uq(p: &str) -> String { format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8]) }
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}
async fn seed_coa(pool: &PgPool) -> (Uuid, HashMap<&'static str, Uuid>) {
    let company = Uuid::new_v4();
    let coa: &[(&str, &str, &str, &str, &str)] = &[
        ("1200", "Piutang Usaha", "asset", "accounts_receivable", "debit"),
        ("1190", "Dana Belum Disetor", "asset", "current_asset", "debit"),  // clearing / undeposited funds
        ("1110", "Bank BCA", "asset", "bank", "debit"),
    ];
    let mut m = HashMap::new();
    for (code, name, at, st, nb) in coa {
        let id = Uuid::new_v4();
        sqlx::query(r#"INSERT INTO accounting.accounts (id, company_id, account_number, account_code, name, account_type, account_subtype, normal_balance, is_header, is_detail, status)
            VALUES ($1,$2,$3,$4,$5,$6::account_type,$7::account_subtype,$8::normal_balance,false,true,'active'::account_status)"#)
            .bind(id).bind(company).bind(code).bind(code).bind(name).bind(at).bind(st).bind(nb)
            .execute(pool).await.expect("seed acct");
        m.insert(*code, id);
    }
    (company, m)
}
async fn balance(pool: &PgPool, account: Uuid) -> Decimal {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_amount),0) - COALESCE(SUM(credit_amount),0) FROM accounting.ledgers WHERE account_id=$1")
        .bind(account).fetch_one(pool).await.unwrap()
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
    let gl = GlAdapter { svc: PostingService::new(pool.clone()) };

    // 1) payment: a receive settles to the CLEARING account (undeposited funds) — Dr Clearing · Cr A/R.
    let va_ref = uq("VA");
    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "receive".into(),
        party_type: Some("customer".into()), party_id: Some(customer), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["1200"], paid_amount: d("750000"),
        reference_no: Some(va_ref.clone()), allocations: vec![],
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

    // 4) clear it → Dr Bank · Cr Clearing into the REAL ledger.
    let out = banking.clear_transaction(NewClearance {
        bank_transaction_id: txn, matched_source_type: matched.source_type, matched_source_id: matched.source_id,
        matched_source_amount: matched.amount, matched_amount: matched.amount, match_method: Some("exact".into()), clearance_date: day(6),
    }, &gl).await.unwrap();
    assert!(out.fully_reconciled);
    assert_eq!(journal_totals(&pool, out.journal_id).await, (d("750000"), d("750000")));

    // 5) the cash position is now correct:
    assert_eq!(balance(&pool, coa["1190"]).await, d("0.00"), "clearing nets to zero — funds no longer undeposited");
    assert_eq!(balance(&pool, coa["1110"]).await, d("750000.00"), "the bank GL holds the cash");
    assert_eq!(balance(&pool, coa["1200"]).await, d("-750000.00"), "A/R stays settled (customer paid)");

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
    let gl = GlAdapter { svc: PostingService::new(pool.clone()) };

    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "receive".into(),
        party_type: Some("customer".into()), party_id: Some(customer), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["1200"], paid_amount: d("500000"),
        reference_no: None, allocations: vec![],
    }).await.unwrap();
    payment.post_payment(pay, &gl).await.unwrap();

    let acct = bank_account(&banking, company, coa["1110"], coa["1190"]).await;
    let (l1, l2) = two_lines(&pool, &banking, company, acct, "500000", "500000", "1000000").await;

    // clear line 1 against the payment — fine.
    banking.clear_transaction(NewClearance {
        bank_transaction_id: l1, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("500000"), matched_amount: d("500000"), match_method: Some("exact".into()), clearance_date: day(6),
    }, &gl).await.unwrap();
    // clear line 2 against the SAME payment — refused; the settlement is already fully cleared.
    let e = banking.clear_transaction(NewClearance {
        bank_transaction_id: l2, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("500000"), matched_amount: d("500000"), match_method: Some("exact".into()), clearance_date: day(6),
    }, &gl).await.unwrap_err();
    assert!(matches!(e, BankingError::SettlementOverCleared { .. }));

    assert_eq!(balance(&pool, coa["1190"]).await, d("0.00"), "clearing stays at zero — no phantom credit");
    assert_eq!(balance(&pool, coa["1110"]).await, d("500000.00"), "bank GL not overstated");
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
    let gl = GlAdapter { svc: PostingService::new(pool.clone()) };

    let pay = payment.create_payment(NewPayment {
        payment_number: uq("PE"), company_id: company, branch_id: None, payment_type: "receive".into(),
        party_type: Some("customer".into()), party_id: Some(customer), posting_date: day(5), currency: None,
        mode_of_payment_id: None, bank_account_id: coa["1190"], party_account_id: coa["1200"], paid_amount: d("750000"),
        reference_no: None, allocations: vec![],
    }).await.unwrap();
    payment.post_payment(pay, &gl).await.unwrap();

    let acct = bank_account(&banking, company, coa["1110"], coa["1190"]).await;
    let (l1, l2) = two_lines(&pool, &banking, company, acct, "500000", "250000", "750000").await;

    banking.clear_transaction(NewClearance {
        bank_transaction_id: l1, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("750000"), matched_amount: d("500000"), match_method: None, clearance_date: day(6),
    }, &gl).await.unwrap();
    banking.clear_transaction(NewClearance {
        bank_transaction_id: l2, matched_source_type: "payment".into(), matched_source_id: pay,
        matched_source_amount: d("750000"), matched_amount: d("250000"), match_method: None, clearance_date: day(6),
    }, &gl).await.unwrap();

    assert_eq!(balance(&pool, coa["1190"]).await, d("0.00"), "clearing nets to zero across the split");
    assert_eq!(balance(&pool, coa["1110"]).await, d("750000.00"), "bank GL holds the full payment");
}
