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
use super::iban_validation::{is_iban_shaped, normalize_iban, validate_iban_with, IbanError, IbanValidationPolicy};

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
    BalanceMismatch {
        expected: Decimal,
        computed: Decimal,
    },
    NegativeAmount,
    NonPositiveAmount,
    OverAllocated {
        line_net: Decimal,
        already: Decimal,
        attempted: Decimal,
    },
    SettlementOverCleared {
        settlement_amount: Decimal,
        already_cleared: Decimal,
        attempted: Decimal,
    },
    UnbalancedPost,
    UnsupportedCurrency(String),
    TransactionNotFound(Uuid),
    AccountNotFound(Uuid),
    GlRejected {
        code: String,
        message: String,
    },
    /// The reconciliation-graph sink refused the clearing's edge (unposted payment journal, a
    /// non-reconcilable clearing account, or a clamp disagreement). Clearing is fail-closed on
    /// this: the clearance row and the allocation advance roll back with the refused edge.
    ReconcileRefused {
        code: String,
        message: String,
    },
    PresetNotFound(Uuid),
    PresetInactive(Uuid),
    PresetMissingTolerance(Uuid),
    PresetMissingDaysWindow(Uuid),
    UnknownMatchOn(String),
    /// The candidate-pool read refused: the payment schema is absent (standalone module database,
    /// mis-composed host). Ordering ranks nothing rather than present fiction as candidates.
    CandidatePoolRefused(String),
    /// An IBAN-shaped account number whose country is KNOWN but whose length/structure or
    /// mod-97 checksum is wrong.
    InvalidIban(String),
    /// An IBAN-shaped account number claiming a country that is not in the reviewed IBAN
    /// registry — refused fail-closed (distinct from `InvalidIban` so the escape-able
    /// refusal is visible as its own error code).
    IbanUnknownCountry(String),
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
            BankingError::PresetNotFound(_) => "preset_not_found".into(),
            BankingError::PresetInactive(_) => "preset_inactive".into(),
            BankingError::PresetMissingTolerance(_) => "preset_missing_tolerance".into(),
            BankingError::PresetMissingDaysWindow(_) => "preset_missing_days_window".into(),
            BankingError::UnknownMatchOn(_) => "unknown_match_on".into(),
            BankingError::CandidatePoolRefused(_) => "candidate_pool_refused".into(),
            BankingError::InvalidIban(_) => "invalid_iban".into(),
            BankingError::IbanUnknownCountry(_) => "iban_unknown_country".into(),
            BankingError::Db(_) => "internal_error".into(),
        }
    }
    pub fn http_status(&self) -> u16 {
        match self {
            BankingError::TransactionNotFound(_)
            | BankingError::AccountNotFound(_)
            | BankingError::PresetNotFound(_) => 404,
            BankingError::CandidatePoolRefused(_) | BankingError::Db(_) => 500,
            _ => 422,
        }
    }
}
impl std::fmt::Display for BankingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BankingError::GlRejected { code, message } => write!(f, "{code}: {message}"),
            BankingError::ReconcileRefused { code, message } => write!(f, "{code}: {message}"),
            BankingError::BalanceMismatch { expected, computed } => write!(
                f,
                "balance_mismatch: expected {expected}, computed {computed}"
            ),
            other => write!(f, "{}", other.code()),
        }
    }
}
impl std::error::Error for BankingError {}
impl From<sqlx::Error> for BankingError {
    fn from(e: sqlx::Error) -> Self {
        BankingError::Db(e)
    }
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
    /// When set, a payment-matched `clear_transaction` stages `BankClearanceRecorded` into
    /// `<schema>.outbox_events` **inside the clearance transaction** (crash-safe emission — the
    /// payment module's bank-confirmation drift consumes it). When `None`, only the legacy in-proc
    /// sink fires (existing behaviour). The relay drains the outbox to the real bus.
    pub(super) outbox_schema: Option<String>,
    /// The candidate pool the reconcile-preset ordering ranks. Defaults to the guarded
    /// payment-header read; tests inject their own.
    pub(super) candidates: Arc<dyn super::reconcile_preset_candidates::CandidatePoolPort>,
    /// IBAN validation posture for `create_bank_account`. Default fail-closed for
    /// IBAN-shaped numbers claiming unregistered countries (see `iban_validation`).
    pub(super) iban_policy: IbanValidationPolicy,
}

impl BankingWriteService {
    pub fn new(db_pool: PgPool) -> Self {
        Self::with_sink(db_pool, Arc::new(LoggingSink))
    }
    pub fn with_sink(db_pool: PgPool, sink: Arc<dyn BankingEventSink>) -> Self {
        let repos = Arc::new(Repos::new(&db_pool));
        Self {
            db_pool,
            repos,
            sink,
            outbox_schema: None,
            candidates: Arc::new(super::reconcile_preset_candidates::PaymentPoolRead),
            iban_policy: IbanValidationPolicy::FAIL_CLOSED,
        }
    }
    /// Explicit IBAN validation posture for this service instance. Hosts wiring the named
    /// escape pass `IbanValidationPolicy::ALLOW_UNKNOWN_COUNTRIES` (or
    /// `IbanValidationPolicy::from_env()` to honor `BANKING_IBAN_ALLOW_UNKNOWN_COUNTRIES`).
    pub fn with_iban_policy(mut self, policy: IbanValidationPolicy) -> Self {
        self.iban_policy = policy;
        self
    }
    /// Enable crash-safe `BankClearanceRecorded` emission via the durable outbox in `schema`
    /// (e.g. `"banking"`). Requires `backbone_outbox::outbox::migrate` to have created
    /// `<schema>.outbox_events`.
    pub fn with_outbox_schema(mut self, schema: impl Into<String>) -> Self {
        self.outbox_schema = Some(schema.into());
        self
    }
    /// Inject the reconcile-preset candidate-pool read (module tests inject a stub; the default
    /// reads posted payment headers through a `to_regclass` guard).
    pub fn with_candidate_port(
        mut self,
        port: Arc<dyn super::reconcile_preset_candidates::CandidatePoolPort>,
    ) -> Self {
        self.candidates = port;
        self
    }

    // ---- masters ------------------------------------------------------------

    pub async fn create_bank(&self, b: NewBank) -> Result<Uuid, BankingError> {
        // RLS scope (ADR-0008): company is on the DTO — bind it so the insert's WITH CHECK passes
        // under the non-superuser app role.
        let company = b.company_id;
        company_scope::with_company_scope(Some(company), async move {
            let id = Uuid::new_v4();
            let country = b.country.unwrap_or_else(|| "ID".into());
            self.repos
                .banks
                .insert_bank(
                    &self.db_pool,
                    &NewBankRow {
                        id,
                        company_id: b.company_id,
                        name: &b.name,
                        swift_bic: b.swift_bic.as_deref(),
                        country: &country,
                    },
                )
                .await?;
            Ok(id)
        })
        .await
    }

    pub async fn create_bank_account(&self, a: NewBankAccount) -> Result<Uuid, BankingError> {
        // RLS scope (ADR-0008): company is on the DTO — same pattern as `create_bank`.
        let company = a.company_id;
        company_scope::with_company_scope(Some(company), async move {
            // IBAN validation, fail-closed: a value that CLAIMS to be an IBAN (two letters
            // + two digits) must carry a registered country, the right length/structure,
            // and a valid mod-97 checksum. Unknown countries are refused with the distinct
            // `iban_unknown_country` code (loudly logged; escape-able via the named config).
            // Non-IBAN-shaped values are LOCAL account numbers (the field's pre-existing
            // contract — Indonesian accounts are not IBAN-based) and pass through unchanged.
            let account_number = if is_iban_shaped(&normalize_iban(&a.account_number)) {
                match validate_iban_with(&self.iban_policy, &a.account_number) {
                    Ok(canonical) => canonical,
                    Err(IbanError::UnknownCountry(country)) => {
                        return Err(BankingError::IbanUnknownCountry(country));
                    }
                    Err(e) => {
                        return Err(BankingError::InvalidIban(format!("{} ({e})", a.account_number)));
                    }
                }
            } else {
                a.account_number.clone()
            };
            let id = Uuid::new_v4();
            let currency = a.currency.unwrap_or_else(|| "IDR".into());
            let account_type = a.account_type.unwrap_or_else(|| "checking".into());
            self.repos
                .bank_accounts
                .insert_bank_account(
                    &self.db_pool,
                    &NewBankAccountRow {
                        id,
                        company_id: a.company_id,
                        branch_id: a.branch_id,
                        bank_id: a.bank_id,
                        account_name: &a.account_name,
                        account_number: &account_number,
                        gl_account_id: a.gl_account_id,
                        clearing_account_id: a.clearing_account_id,
                        currency: &currency,
                        account_type: &account_type,
                    },
                )
                .await?;
            Ok(id)
        })
        .await
    }
}
