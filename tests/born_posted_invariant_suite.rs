//! BPI — the statement-side "evidence, not accounting" invariant suite (ADR-0005).
//!
//! An imported bank statement line is evidence about the BANK's claim, never a move on OUR books:
//! - BPI-1: no draft lifecycle exists anywhere on the statement side — `txn_status` carries no
//!   draft/posted variant, `bank_transactions` carries no posting-state column, and an import
//!   births every line `unreconciled` at allocation 0. The import HEADER's `import_status` is
//!   ingest state (draft→imported), not a posting state: a line is never "waiting to post".
//! - BPI-2: an uncleared line is GL-invisible — import changes no account balance, no journal,
//!   no reconciliation-graph edge; only the cleared line's money reaches the ledger.
//! - BPI-3: exactly one deterministic clearance post per identity — the clearance id is the
//!   uuid5 of `bankclr:{txn}:{source}:{id}:{cumulative}`, one journal hangs off it, and a
//!   re-attempt is refused by the settlement fence rather than doubling the post.
//!
//! Requires DATABASE_URL (:5433/backbone_banking with accounting + banking migrated).

use std::collections::HashMap;
use std::sync::Arc;

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope as BankEnv, GlPostAck as BankAck, GlPostRejected as BankRej,
    GlPostSink as BankSink,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewBank, NewBankAccount, NewClearance, NewStatementImport,
    NewStatementLine,
};

use backbone_accounting::application::service::posting_service::{
    PostingLine, PostingRequest, PostingService,
};
use backbone_accounting::infrastructure::persistence::SqlxPostingRepository;

/// ACL: banking's serialized envelope → accounting's PostingRequest against the REAL ledger.
struct GlAdapter {
    svc: PostingService,
}
#[async_trait::async_trait]
impl BankSink for GlAdapter {
    async fn post(&self, e: &BankEnv) -> Result<BankAck, BankRej> {
        let lines = e
            .lines
            .iter()
            .map(|l| PostingLine {
                account_id: l.account_id,
                debit: l.debit,
                credit: l.credit,
                party_type: l.party_type.clone(),
                party_id: l.party_id,
                cost_center_id: None,
                project_id: None,
                department_id: None,
                description: l.description.clone(),
            })
            .collect();
        let mut r =
            PostingRequest::original(e.company_id, &e.source_type, e.source_id, e.posting_date);
        r.source_reference = e.source_reference.clone();
        r.idempotency_key = Some(e.idempotency_key.clone());
        r.posting_type = e.posting_type.clone();
        r.lines = lines;
        match self.svc.post(r, None).await {
            Ok(x) => Ok(BankAck {
                post_id: x.post_id,
                journal_id: x.journal_id,
                idempotent_reuse: x.idempotent_reuse,
            }),
            Err(x) => Err(BankRej {
                code: x.code().to_string(),
                message: x.to_string(),
            }),
        }
    }
}

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

/// Bank + clearing chart (the GL side of the invariant).
async fn seed(pool: &PgPool) -> (Uuid, HashMap<&'static str, Uuid>) {
    let company = Uuid::new_v4();
    let coa: &[(&str, &str, bool)] = &[
        ("1190", "Dana Belum Disetor", true),
        ("1110", "Bank BCA", false),
    ];
    let mut m = HashMap::new();
    for (code, name, rec) in coa {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO accounting.accounts (id, company_id, account_number, account_code, name, account_type, account_subtype, normal_balance, is_header, is_detail, is_reconcilable, status)
            VALUES ($1,$2,$3,$3,$4,'asset'::account_type,'current_asset'::account_subtype,'debit'::normal_balance,false,true,$5,'active'::account_status)"#,
        )
        .bind(id)
        .bind(company)
        .bind(code)
        .bind(name)
        .bind(rec)
        .execute(pool)
        .await
        .expect("seed acct");
        m.insert(*code, id);
    }
    (company, m)
}

async fn balance(pool: &PgPool, account: Uuid) -> Decimal {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(debit_amount),0) - COALESCE(SUM(credit_amount),0) FROM accounting.ledgers WHERE account_id=$1",
    )
    .bind(account)
    .fetch_one(pool)
    .await
    .unwrap()
}
async fn journals_for_company(pool: &PgPool, company: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM accounting.journals WHERE company_id=$1")
        .bind(company)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// BPI-1 — no draft lifecycle exists on the statement side: `txn_status` holds no draft/posted
/// variant (it is a RECONCILIATION lifecycle), the table carries no posting-state column, and an
/// import births every line unreconciled at allocation zero. The header's `import_status` advances
/// draft→imported as INGEST state while the lines stay put — it never means "waiting to post".
#[tokio::test]
async fn no_draft_lifecycle_exists_on_the_statement_side() {
    let pool = pool().await;
    let (company, coa) = seed(&pool).await;

    // The line lifecycle is reconciliation-only: unreconciled / partly_reconciled / reconciled /
    // ignored. A draft or posted variant would smuggle in a posting state this side never owns.
    let labels: Vec<String> = sqlx::query_scalar(
        "SELECT e.enumlabel FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid WHERE t.typname = 'txn_status' ORDER BY e.enumsortorder",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        !labels.iter().any(|l| l == "draft" || l == "posted"),
        "txn_status must carry no posting lifecycle, got {labels:?}"
    );

    // And no posting-state column hides on the table either.
    let postingish: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_schema='banking' AND table_name='bank_transactions' AND (column_name LIKE '%post%' OR column_name LIKE '%draft%' OR column_name LIKE '%gl%')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        postingish.is_empty(),
        "bank_transactions must carry no posting-state column, found {postingish:?}"
    );

    // An import births lines unreconciled at zero allocation, while the import header advances its
    // INGEST state — two different truths, one shape per side.
    let banking = BankingWriteService::new(pool.clone());
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
            gl_account_id: coa["1110"],
            clearing_account_id: coa["1190"],
            currency: None,
            account_type: None,
        })
        .await
        .unwrap();
    banking
        .import_statement(NewStatementImport {
            company_id: company,
            bank_account_id: acct,
            source_format: None,
            period_start: day(1),
            period_end: day(31),
            opening_balance: Decimal::ZERO,
            closing_balance: d("750000"),
            file_ref: None,
            lines: vec![
                NewStatementLine {
                    txn_date: day(6),
                    description: None,
                    reference_no: None,
                    deposit: d("600000"),
                    withdrawal: Decimal::ZERO,
                },
                NewStatementLine {
                    txn_date: day(7),
                    description: None,
                    reference_no: None,
                    deposit: d("150000"),
                    withdrawal: Decimal::ZERO,
                },
            ],
        })
        .await
        .unwrap();

    let lines: Vec<(Decimal, String)> = sqlx::query_as(
        "SELECT allocated_amount, status::text FROM banking.bank_transactions WHERE company_id=$1",
    )
    .bind(company)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(lines.len(), 2);
    for (alloc, status) in &lines {
        assert_eq!(*alloc, Decimal::ZERO, "born at zero allocation");
        assert_eq!(
            status, "unreconciled",
            "born unreconciled — never draft, never waiting to post"
        );
    }
    let header_status: String = sqlx::query_scalar(
        "SELECT status::text FROM banking.bank_statement_imports WHERE company_id=$1",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        header_status, "imported",
        "the header's import_status is ingest state — it advanced, the lines did not"
    );
}

/// BPI-2 + BPI-3 — evidence, not accounting: the import itself touches nothing in the ledger; the
/// first GL contact is the clear, and that clear is exactly ONE deterministic post per identity.
#[tokio::test]
async fn uncleared_lines_are_gl_invisible_and_the_clear_posts_once() {
    let pool = pool().await;
    let (company, coa) = seed(&pool).await;
    let banking = BankingWriteService::new(pool.clone());
    let gl = GlAdapter {
        svc: PostingService::new(Arc::new(SqlxPostingRepository::new(pool.clone()))),
    };

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
            gl_account_id: coa["1110"],
            clearing_account_id: coa["1190"],
            currency: None,
            account_type: None,
        })
        .await
        .unwrap();
    banking
        .import_statement(NewStatementImport {
            company_id: company,
            bank_account_id: acct,
            source_format: None,
            period_start: day(1),
            period_end: day(31),
            opening_balance: Decimal::ZERO,
            closing_balance: d("750000"),
            file_ref: None,
            lines: vec![
                NewStatementLine {
                    txn_date: day(6),
                    description: None,
                    reference_no: None,
                    deposit: d("600000"),
                    withdrawal: Decimal::ZERO,
                },
                NewStatementLine {
                    txn_date: day(7),
                    description: None,
                    reference_no: None,
                    deposit: d("150000"),
                    withdrawal: Decimal::ZERO,
                },
            ],
        })
        .await
        .unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM banking.bank_transactions WHERE company_id=$1 ORDER BY deposit DESC",
    )
    .bind(company)
    .fetch_all(&pool)
    .await
    .unwrap();
    let (big, small) = (ids[0], ids[1]);

    // BPI-2: the import committed two lines and changed NOTHING in the ledger — no journal, no
    // balance, no graph edge. An uncleared line is evidence, not a move.
    assert_eq!(
        journals_for_company(&pool, company).await,
        0,
        "import creates no journal"
    );
    assert_eq!(
        balance(&pool, coa["1110"]).await,
        Decimal::ZERO,
        "bank GL untouched by the import"
    );
    assert_eq!(
        balance(&pool, coa["1190"]).await,
        Decimal::ZERO,
        "clearing GL untouched by the import"
    );
    let edges: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM accounting.partial_reconciles WHERE company_id=$1",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(edges, 0, "import writes no reconciliation-graph edge");

    // The first GL contact is the clear — of ONE line, against a non-payment document (an invoice:
    // matched by the composition, no clearing edge — its convergence is the statement-side surface).
    let invoice = Uuid::new_v4();
    let out = banking
        .clear_transaction(
            NewClearance {
                bank_transaction_id: small,
                matched_source_type: "invoice".into(),
                matched_source_id: invoice,
                matched_source_amount: d("150000"),
                matched_amount: d("150000"),
                match_method: Some("exact".into()),
                clearance_date: day(7),
            },
            &gl,
            &NoEdge,
        )
        .await
        .unwrap();

    // BPI-3: the clearance identity is deterministic — uuid5 over (line, source kind, source id,
    // cumulative-cleared-at-zero). The same committed state always derives the same id.
    let identity = format!("bankclr:{}:{}:{}:{}", small, "invoice", invoice, "0");
    assert_eq!(
        out.clearance_id,
        Uuid::new_v5(&Uuid::NAMESPACE_URL, identity.as_bytes()),
        "clearance id is the deterministic uuid5 of its identity"
    );
    let journals: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT j.id) FROM accounting.journals j JOIN accounting.journal_lines jl ON jl.journal_id=j.id WHERE j.company_id=$1 AND jl.source_type='settlement' AND jl.source_id=$2",
    )
    .bind(company)
    .bind(out.clearance_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(journals, 1, "exactly one clearance post for the identity");

    // Only the CLEARED line's money reached the ledger; the uncleared 600k line stays invisible.
    assert_eq!(
        balance(&pool, coa["1110"]).await,
        d("150000.00"),
        "bank GL holds only the cleared line"
    );
    let uncleared: (Decimal, String) = sqlx::query_as(
        "SELECT allocated_amount, status::text FROM banking.bank_transactions WHERE id=$1",
    )
    .bind(big)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        uncleared,
        (Decimal::ZERO, "unreconciled".to_string()),
        "the uncleared line is untouched"
    );

    // BPI-3 (fence half): a re-attempt of the same clear — from a DIFFERENT line against the SAME
    // settlement, so it is the SETTLEMENT bound that fires (the same-line retry would merely hit
    // the line's own remainder) — is refused: the deterministic identity means a retry derives the
    // SAME id, and doubling the post is impossible.
    let e = banking
        .clear_transaction(
            NewClearance {
                bank_transaction_id: big,
                matched_source_type: "invoice".into(),
                matched_source_id: invoice,
                matched_source_amount: d("150000"),
                matched_amount: d("150000"),
                match_method: Some("exact".into()),
                clearance_date: day(8),
            },
            &gl,
            &NoEdge,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(e, BankingError::SettlementOverCleared { .. }),
        "re-attempt refused by the settlement fence, got {e:?}"
    );
    let clearances: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM banking.bank_clearances WHERE company_id=$1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(clearances, 1, "still exactly one clearance row");
    assert_eq!(
        balance(&pool, coa["1110"]).await,
        d("150000.00"),
        "and still exactly one post's worth of money"
    );
}

/// A no-op reconcile sink: a non-payment-matched clear carries no clearing edge — statement-side
/// convergence is the bank-rec surface, not the graph (the payment-matched path is the clearing
/// seam suite's territory).
struct NoEdge;
#[async_trait::async_trait]
impl backbone_banking::application::service::banking_gl::ReconcileSink for NoEdge {
    async fn reconcile_pair_on(
        &self,
        _conn: &mut sqlx::PgConnection,
        _req: &backbone_banking::application::service::banking_gl::ReconcilePairRequest,
    ) -> Result<
        backbone_banking::application::service::banking_gl::ReconcileEdgeAck,
        backbone_banking::application::service::banking_gl::ReconcileRejected,
    > {
        unreachable!("a non-payment clear must never touch the reconcile sink")
    }
    async fn unreconcile_pair_on(
        &self,
        _conn: &mut sqlx::PgConnection,
        _req: &backbone_banking::application::service::banking_gl::UnreconcilePairRequest,
    ) -> Result<(), backbone_banking::application::service::banking_gl::ReconcileRejected> {
        unreachable!("a non-payment clear must never touch the reconcile sink")
    }
}
