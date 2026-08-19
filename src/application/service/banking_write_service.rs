//! Validated write path + clearing engine for banking (hand-authored, user-owned).
//!
//! Banking ingests a statement, matches each line to a settled document (candidates supplied by a
//! composition layer — banking never reads payment/billing tables), and CLEARS it through the GL:
//!   - **received (deposit):** `Dr Bank · Cr Bank Clearing` — the bank-side leg payment left open.
//!   - **paid (withdrawal):** `Dr Bank Clearing · Cr Bank`.
//!   - **bank charge:** `Dr Bank Charges · Cr Bank`.
//! The clearing account nets to zero once a payment's settlement (which debited/credited clearing) is
//! confirmed by the statement. Balanced-or-refuse; IDR-only for now; clearing is bounded per line.
//!
//! **This file is the hub:** it holds the module's vocabulary (input structs, outcomes, errors) and
//! the masters path (`create_bank` / `create_bank_account`). The rest of the write surface is chunked
//! into focused siblings, each an `impl BankingWriteService` block over these same types:
//!
//! - [`super::banking_statement`] — `import_statement` (balance continuity + header + lines) and
//!   `propose_match` (exact/fuzzy match against supplied candidates).
//! - [`super::banking_clearance`] — the GL bank-side leg: `clear_transaction` (received/paid) and
//!   `recognize_bank_charge`, both bounded by the line and (for clear) the settlement.
//! - [`super::banking_reconciliation`] — the reconciliation session open/close with the
//!   line-completeness close-gate.

use backbone_orm::company_scope;
use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    BankAccountRepository, BankClearanceRepository, BankReconciliationRepository, BankRepository,
    BankStatementImportRepository, BankTransactionRepository, NewBankAccountRow, NewBankRow,
};

use super::banking_events::{BankingEventSink, LoggingSink};

pub(super) fn money(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

// --- input structs -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewBank {
    pub company_id: Uuid,
    pub name: String,
    pub swift_bic: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewBankAccount {
    pub company_id: Uuid,
    pub branch_id: Option<Uuid>,
    pub bank_id: Uuid,
    pub account_name: String,
    pub account_number: String,
    pub gl_account_id: Uuid,
    pub clearing_account_id: Uuid,
    pub currency: Option<String>,
    pub account_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewStatementLine {
    pub txn_date: chrono::NaiveDate,
    pub description: Option<String>,
    pub reference_no: Option<String>,
    pub deposit: Decimal,
    pub withdrawal: Decimal,
}

#[derive(Debug, Clone)]
pub struct NewStatementImport {
    pub company_id: Uuid,
    pub bank_account_id: Uuid,
    pub source_format: Option<String>,
    pub period_start: chrono::NaiveDate,
    pub period_end: chrono::NaiveDate,
    pub opening_balance: Decimal,
    pub closing_balance: Decimal,
    pub file_ref: Option<String>,
    pub lines: Vec<NewStatementLine>,
}

/// A candidate document a statement line might settle (supplied by the composition — banking does not
/// read payment/billing). `amount` is the settlement's cash amount; `reference` is a match key.
#[derive(Debug, Clone)]
pub struct MatchCandidate {
    pub source_type: String,
    pub source_id: Uuid,
    pub amount: Decimal,
    pub reference: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewClearance {
    pub bank_transaction_id: Uuid,
    pub matched_source_type: String,
    pub matched_source_id: Uuid,
    /// The settled document's TOTAL amount (from `MatchCandidate.amount`). Banking bounds the sum of
    /// all clearances against a settlement by this, so one payment can't be cleared twice.
    pub matched_source_amount: Decimal,
    pub matched_amount: Decimal,
    pub match_method: Option<String>,
    pub clearance_date: chrono::NaiveDate,
}

#[derive(Debug, Clone)]
pub struct NewCharge {
    pub bank_transaction_id: Uuid,
    pub charge_account_id: Uuid,
    pub amount: Decimal,
    pub clearance_date: chrono::NaiveDate,
}

#[derive(Debug, Clone)]
pub struct NewReconciliation {
    pub company_id: Uuid,
    pub bank_account_id: Uuid,
    pub from_date: chrono::NaiveDate,
    pub to_date: chrono::NaiveDate,
    pub statement_closing_balance: Decimal,
    pub ledger_balance: Decimal,
}

#[derive(Debug, Clone)]
pub struct ClearOutcome {
    pub clearance_id: Uuid,
    pub post_id: Uuid,
    pub journal_id: Uuid,
    pub fully_reconciled: bool,
}

#[derive(Debug, Clone)]
pub struct ReconcileOutcome {
    pub id: Uuid,
    pub difference: Decimal,
    /// Open lines (unreconciled/partly_reconciled) in the period — the exceptions blocking close.
    pub unreconciled_count: i64,
    /// "open" (numbers disagree) | "balanced" (agree, exceptions outstanding) | "closed" (agree + clean).
    pub status: String,
}

// --- errors ------------------------------------------------------------------

#[derive(Debug)]
pub enum BankingError {
    EmptyStatement,
    BalanceMismatch { expected: Decimal, computed: Decimal },
    NegativeAmount,
    NonPositiveAmount,
    OverAllocated { line_net: Decimal, already: Decimal, attempted: Decimal },
    SettlementOverCleared { settlement_amount: Decimal, already_cleared: Decimal, attempted: Decimal },
    UnbalancedPost,
    UnsupportedCurrency(String),
    TransactionNotFound(Uuid),
    AccountNotFound(Uuid),
    GlRejected { code: String, message: String },
    /// The reconciliation-graph sink refused the clearing's edge (unposted payment journal, a
    /// non-reconcilable clearing account, or a clamp disagreement). Clearing is fail-closed on
    /// this: the clearance row and the allocation advance roll back with the refused edge.
    ReconcileRefused { code: String, message: String },
    Db(sqlx::Error),
}

impl BankingError {
    pub fn code(&self) -> String {
        match self {
            BankingError::EmptyStatement => "empty_statement".into(),
            BankingError::BalanceMismatch { .. } => "balance_mismatch".into(),
            BankingError::NegativeAmount => "negative_amount".into(),
            BankingError::NonPositiveAmount => "non_positive_amount".into(),
            BankingError::OverAllocated { .. } => "over_allocated".into(),
            BankingError::SettlementOverCleared { .. } => "settlement_over_cleared".into(),
            BankingError::UnbalancedPost => "unbalanced_post".into(),
            BankingError::UnsupportedCurrency(_) => "unsupported_currency".into(),
            BankingError::TransactionNotFound(_) => "transaction_not_found".into(),
            BankingError::AccountNotFound(_) => "account_not_found".into(),
            BankingError::GlRejected { code, .. } => code.clone(),
            BankingError::ReconcileRefused { code, .. } => code.clone(),
            BankingError::Db(_) => "internal_error".into(),
        }
    }
    pub fn http_status(&self) -> u16 {
        match self {
            BankingError::TransactionNotFound(_) | BankingError::AccountNotFound(_) => 404,
            BankingError::Db(_) => 500,
            _ => 422,
        }
    }
}
impl std::fmt::Display for BankingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BankingError::GlRejected { code, message } => write!(f, "{code}: {message}"),
            BankingError::ReconcileRefused { code, message } => write!(f, "{code}: {message}"),
            BankingError::BalanceMismatch { expected, computed } => write!(f, "balance_mismatch: expected {expected}, computed {computed}"),
            other => write!(f, "{}", other.code()),
        }
    }
}
impl std::error::Error for BankingError {}
impl From<sqlx::Error> for BankingError {
    fn from(e: sqlx::Error) -> Self { BankingError::Db(e) }
}

/// The repositories this service orchestrates. Bundled behind one `Arc` so the service stays cheap
/// to `Clone` (it is cloned per request) without requiring the repository newtypes to be `Clone`.
/// Fields are `pub(super)` so the sibling `impl BankingWriteService` blocks (banking_statement /
/// banking_clearance / banking_reconciliation) can drive them — they share this service's vocabulary.
pub(super) struct Repos {
    pub(super) banks: BankRepository,
    pub(super) bank_accounts: BankAccountRepository,
    pub(super) imports: BankStatementImportRepository,
    pub(super) transactions: BankTransactionRepository,
    pub(super) clearances: BankClearanceRepository,
    pub(super) reconciliations: BankReconciliationRepository,
}

impl Repos {
    fn new(db_pool: &PgPool) -> Self {
        Self {
            banks: BankRepository::new(db_pool.clone()),
            bank_accounts: BankAccountRepository::new(db_pool.clone()),
            imports: BankStatementImportRepository::new(db_pool.clone()),
            transactions: BankTransactionRepository::new(db_pool.clone()),
            clearances: BankClearanceRepository::new(db_pool.clone()),
            reconciliations: BankReconciliationRepository::new(db_pool.clone()),
        }
    }
}

#[derive(Clone)]
pub struct BankingWriteService {
    pub(super) db_pool: PgPool,
    pub(super) repos: Arc<Repos>,
    pub(super) sink: Arc<dyn BankingEventSink>,
}

impl BankingWriteService {
    pub fn new(db_pool: PgPool) -> Self {
        Self::with_sink(db_pool, Arc::new(LoggingSink))
    }
    pub fn with_sink(db_pool: PgPool, sink: Arc<dyn BankingEventSink>) -> Self {
        let repos = Arc::new(Repos::new(&db_pool));
        Self { db_pool, repos, sink }
    }

    // ---- masters ------------------------------------------------------------

    pub async fn create_bank(&self, b: NewBank) -> Result<Uuid, BankingError> {
        // RLS scope (ADR-0008): company is on the DTO — bind it so the insert's WITH CHECK passes
        // under the non-superuser app role.
        let company = b.company_id;
        company_scope::with_company_scope(Some(company), async move {
            let id = Uuid::new_v4();
            let country = b.country.unwrap_or_else(|| "ID".into());
            self.repos.banks.insert_bank(&self.db_pool, &NewBankRow {
                id,
                company_id: b.company_id,
                name: &b.name,
                swift_bic: b.swift_bic.as_deref(),
                country: &country,
            }).await?;
            Ok(id)
        }).await
    }

    pub async fn create_bank_account(&self, a: NewBankAccount) -> Result<Uuid, BankingError> {
        // RLS scope (ADR-0008): company is on the DTO — same pattern as `create_bank`.
        let company = a.company_id;
        company_scope::with_company_scope(Some(company), async move {
            let id = Uuid::new_v4();
            let currency = a.currency.unwrap_or_else(|| "IDR".into());
            let account_type = a.account_type.unwrap_or_else(|| "checking".into());
            self.repos.bank_accounts.insert_bank_account(&self.db_pool, &NewBankAccountRow {
                id,
                company_id: a.company_id,
                branch_id: a.branch_id,
                bank_id: a.bank_id,
                account_name: &a.account_name,
                account_number: &a.account_number,
                gl_account_id: a.gl_account_id,
                clearing_account_id: a.clearing_account_id,
                currency: &currency,
                account_type: &account_type,
            }).await?;
            Ok(id)
        }).await
    }
}
