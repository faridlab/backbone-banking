//! Validated write path + clearing engine for banking (hand-authored, user-owned).
//!
//! Banking ingests a statement, matches each line to a settled document (candidates supplied by a
//! composition layer — banking never reads payment/billing tables), and CLEARS it through the GL:
//!   - **received (deposit):** `Dr Bank · Cr Bank Clearing` — the bank-side leg payment left open.
//!   - **paid (withdrawal):** `Dr Bank Clearing · Cr Bank`.
//!   - **bank charge:** `Dr Bank Charges · Cr Bank`.
//! The clearing account nets to zero once a payment's settlement (which debited/credited clearing) is
//! confirmed by the statement. Balanced-or-refuse; IDR-only for now; clearing is bounded per line.

use backbone_orm::company_scope;
use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    BankAccountRepository, BankClearanceRepository, BankReconciliationRepository, BankRepository,
    BankStatementImportRepository, BankTransactionRepository, NewBankAccountRow,
    NewBankTransactionRow, NewBankRow, NewChargeClearanceRow, NewClearanceRow,
    NewReconciliationRow, NewStatementImportRow,
};

use super::banking_events::{
    BankChargeRecognized, BankReconciliationClosed, BankStatementImported, BankTransactionCleared,
    BankTransactionMatched, BankingEvent, BankingEventSink, LoggingSink,
};
use super::banking_gl::{AccountingPostEnvelope, GlPostLine, GlPostSink};

fn money(v: Decimal) -> Decimal {
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
struct Repos {
    banks: BankRepository,
    bank_accounts: BankAccountRepository,
    imports: BankStatementImportRepository,
    transactions: BankTransactionRepository,
    clearances: BankClearanceRepository,
    reconciliations: BankReconciliationRepository,
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
    db_pool: PgPool,
    repos: Arc<Repos>,
    sink: Arc<dyn BankingEventSink>,
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

    // ---- import -------------------------------------------------------------

    /// Import a statement: validate balance continuity (`opening + Σdeposit − Σwithdrawal = closing`),
    /// persist the import + its lines, and emit `BankStatementImported`.
    pub async fn import_statement(&self, imp: NewStatementImport) -> Result<Uuid, BankingError> {
        if imp.lines.is_empty() {
            return Err(BankingError::EmptyStatement);
        }
        let mut net = Decimal::ZERO;
        for l in &imp.lines {
            if l.deposit < Decimal::ZERO || l.withdrawal < Decimal::ZERO {
                return Err(BankingError::NegativeAmount);
            }
            net += l.deposit - l.withdrawal;
        }
        let computed = money(imp.opening_balance + net);
        let closing = money(imp.closing_balance);
        if computed != closing {
            return Err(BankingError::BalanceMismatch { expected: closing, computed });
        }
        let id = Uuid::new_v4();
        let fmt = imp.source_format.clone().unwrap_or_else(|| "manual".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it explicitly onto the transaction we own,
        // so both the import header and every line insert pass their WITH CHECK.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, imp.company_id).await?;
        self.repos.imports.insert_import(&mut tx, &NewStatementImportRow {
            id,
            company_id: imp.company_id,
            bank_account_id: imp.bank_account_id,
            source_format: &fmt,
            statement_period_start: imp.period_start,
            statement_period_end: imp.period_end,
            opening_balance: money(imp.opening_balance),
            closing_balance: closing,
            file_ref: imp.file_ref.as_deref(),
            row_count: imp.lines.len() as i32,
        }).await?;
        for l in &imp.lines {
            self.repos.transactions.insert_transaction(&mut tx, &NewBankTransactionRow {
                id: Uuid::new_v4(),
                company_id: imp.company_id,
                bank_account_id: imp.bank_account_id,
                import_id: id,
                txn_date: l.txn_date,
                description: l.description.as_deref(),
                reference_no: l.reference_no.as_deref(),
                deposit: money(l.deposit),
                withdrawal: money(l.withdrawal),
            }).await?;
        }
        tx.commit().await?;
        self.sink.publish(BankingEvent::BankStatementImported(BankStatementImported {
            import_id: id, bank_account_id: imp.bank_account_id, row_count: imp.lines.len() as i32,
        }));
        Ok(id)
    }

    // ---- match --------------------------------------------------------------

    /// Propose a match for a statement line from supplied candidates: prefer an exact amount + exact
    /// reference (`exact`), else an exact amount (`fuzzy`). Pure selection — persists nothing.
    pub async fn propose_match(&self, bank_transaction_id: Uuid, candidates: &[MatchCandidate]) -> Result<Option<MatchCandidate>, BankingError> {
        // RLS scope (ADR-0008), ID-only pattern: identified by the line id alone — no company arg. This
        // rides the request-dedicated connection carrying the caller's `app.company_id`, so RLS fences
        // the lookup and another company's line is simply not found.
        let row = self.repos.transactions.fetch_match_basis(&self.db_pool, bank_transaction_id).await?
            .ok_or(BankingError::TransactionNotFound(bank_transaction_id))?;
        let net = row.deposit + row.withdrawal;
        let txn_ref: Option<String> = row.reference_no;
        // exact amount + reference first
        if let Some(c) = candidates.iter().find(|c| c.amount == net && c.reference.is_some() && c.reference == txn_ref) {
            return Ok(Some(c.clone()));
        }
        Ok(candidates.iter().find(|c| c.amount == net).cloned())
    }

    // ---- clear --------------------------------------------------------------

    /// Clear a matched statement line through the GL — the bank-side leg. received (deposit):
    /// `Dr Bank · Cr Clearing`; paid (withdrawal): `Dr Clearing · Cr Bank`. Bounded: the clearance
    /// cannot exceed the line's un-allocated remainder. Records a `BankClearance`, advances the line's
    /// `allocated_amount` + status, and emits `BankTransactionMatched` + `BankTransactionCleared`.
    pub async fn clear_transaction(&self, c: NewClearance, sink: &dyn GlPostSink) -> Result<ClearOutcome, BankingError> {
        if c.matched_amount <= Decimal::ZERO { return Err(BankingError::NonPositiveAmount); }
        let matched = money(c.matched_amount);
        // Load the line + its account's GL/clearing accounts.
        // RLS scope (ADR-0008), ID-only pattern — see `propose_match`. Having read the line we bind its
        // OWN company onto the clearing transaction below.
        let row = self.repos.transactions.fetch_clearing_line(&self.db_pool, c.bank_transaction_id).await?
            .ok_or(BankingError::TransactionNotFound(c.bank_transaction_id))?;
        let currency: String = row.currency;
        if currency != "IDR" { return Err(BankingError::UnsupportedCurrency(currency)); }
        let company_id = row.company_id;
        let deposit = row.deposit;
        let withdrawal = row.withdrawal;
        let allocated = row.allocated_amount;
        let bank_acct = row.gl_account_id;
        let clearing = row.clearing_account_id;
        let line_net = deposit + withdrawal;
        if matched > line_net - allocated {
            return Err(BankingError::OverAllocated { line_net, already: allocated, attempted: matched });
        }
        let is_receipt = deposit > Decimal::ZERO;
        let clearance_id = Uuid::new_v4();
        let lines = if is_receipt {
            vec![
                GlPostLine::debit(bank_acct, matched).with_description("Bank clearing (received)"),
                GlPostLine::credit(clearing, matched).with_description("Clear undeposited funds"),
            ]
        } else {
            vec![
                GlPostLine::debit(clearing, matched).with_description("Clear undeposited funds"),
                GlPostLine::credit(bank_acct, matched).with_description("Bank clearing (paid)"),
            ]
        };
        let env = AccountingPostEnvelope {
            idempotency_key: format!("bankclr:{}:{}", c.bank_transaction_id, clearance_id),
            company_id, branch_id: None, source_type: "settlement".into(), source_id: clearance_id,
            source_reference: Some(format!("clear {}", c.bank_transaction_id)),
            posting_date: c.clearance_date, currency, posting_type: "original".into(), reverses_post_id: None,
            description: Some("Bank clearing".into()), lines,
        };
        if !env.is_balanced() { return Err(BankingError::UnbalancedPost); }

        // Settlement-dimension bound (council 2026-07-05): the SUM of all clearances against this
        // settlement cannot exceed the settled document's amount — so one payment cannot be cleared
        // twice (a re-imported line, a retry, two operators). The line bound above only limits the
        // LINE; without this, two lines each matching one payment both pass and strand the clearing
        // account. Serialize per settlement with an advisory lock so concurrent first-clears can't race
        // the phantom-insert, and hold the tx across the post so the check + write are one unit.
        let mut tx = self.db_pool.begin().await?;
        // RLS scope (ADR-0008): bind the line's own company (read above) onto this transaction, so the
        // already-cleared SUM sees the tenant's clearances and the clearance insert passes WITH CHECK.
        company_scope::bind_company_on(&mut tx, company_id).await?;
        self.repos.clearances.lock_settlement(&mut tx, c.matched_source_id).await?;
        let already_cleared = self.repos.clearances
            .sum_cleared_against_settlement(&mut tx, &c.matched_source_type, c.matched_source_id).await?;
        if already_cleared + matched > money(c.matched_source_amount) {
            return Err(BankingError::SettlementOverCleared { settlement_amount: money(c.matched_source_amount), already_cleared, attempted: matched });
        }

        match sink.post(&env).await {
            Ok(ack) => {
                let match_method = c.match_method.clone().unwrap_or_else(|| "manual".into());
                self.repos.clearances.insert_clearance(&mut tx, &NewClearanceRow {
                    id: clearance_id,
                    company_id,
                    bank_transaction_id: c.bank_transaction_id,
                    matched_source_type: &c.matched_source_type,
                    matched_source_id: c.matched_source_id,
                    matched_amount: matched,
                    match_method: &match_method,
                    clearance_date: c.clearance_date,
                    accounting_post_id: ack.post_id,
                    journal_id: ack.journal_id,
                }).await?;
                let new_alloc = allocated + matched;
                let fully = new_alloc >= line_net;
                let status = if fully { "reconciled" } else { "partly_reconciled" };
                self.repos.transactions
                    .set_allocation(&mut tx, c.bank_transaction_id, new_alloc, status).await?;
                tx.commit().await?;

                self.sink.publish(BankingEvent::BankTransactionMatched(BankTransactionMatched {
                    bank_transaction_id: c.bank_transaction_id, matched_source_type: c.matched_source_type.clone(),
                    matched_source_id: c.matched_source_id, amount: matched,
                }));
                self.sink.publish(BankingEvent::BankTransactionCleared(BankTransactionCleared {
                    bank_transaction_id: c.bank_transaction_id, matched_source_type: c.matched_source_type,
                    matched_source_id: c.matched_source_id, company_id, journal_id: ack.journal_id,
                    post_id: ack.post_id, amount: matched,
                }));
                Ok(ClearOutcome { clearance_id, post_id: ack.post_id, journal_id: ack.journal_id, fully_reconciled: fully })
            }
            Err(rej) => Err(BankingError::GlRejected { code: rej.code, message: rej.message }),
        }
    }

    // ---- bank charge --------------------------------------------------------

    /// Recognise an outflow line as a bank charge: `Dr Bank Charges · Cr Bank`. Marks the line
    /// reconciled and emits `BankChargeRecognized`.
    pub async fn recognize_bank_charge(&self, ch: NewCharge, sink: &dyn GlPostSink) -> Result<ClearOutcome, BankingError> {
        if ch.amount <= Decimal::ZERO { return Err(BankingError::NonPositiveAmount); }
        let amount = money(ch.amount);
        // RLS scope (ADR-0008), ID-only pattern — see `propose_match`; the charge tx below binds the
        // line's own company.
        let row = self.repos.transactions.fetch_charge_line(&self.db_pool, ch.bank_transaction_id).await?
            .ok_or(BankingError::TransactionNotFound(ch.bank_transaction_id))?;
        let currency: String = row.currency;
        if currency != "IDR" { return Err(BankingError::UnsupportedCurrency(currency)); }
        let company_id = row.company_id;
        let allocated = row.allocated_amount;
        let line_net = row.deposit + row.withdrawal;
        let bank_acct = row.gl_account_id;
        if amount > line_net - allocated {
            return Err(BankingError::OverAllocated { line_net, already: allocated, attempted: amount });
        }
        let clearance_id = Uuid::new_v4();
        let env = AccountingPostEnvelope {
            idempotency_key: format!("bankchg:{}:{}", ch.bank_transaction_id, clearance_id),
            company_id, branch_id: None, source_type: "settlement".into(), source_id: clearance_id,
            source_reference: Some(format!("charge {}", ch.bank_transaction_id)),
            posting_date: ch.clearance_date, currency, posting_type: "original".into(), reverses_post_id: None,
            description: Some("Bank charge".into()),
            lines: vec![
                GlPostLine::debit(ch.charge_account_id, amount).with_description("Bank charges"),
                GlPostLine::credit(bank_acct, amount).with_description("Bank"),
            ],
        };
        if !env.is_balanced() { return Err(BankingError::UnbalancedPost); }
        match sink.post(&env).await {
            Ok(ack) => {
                let mut tx = self.db_pool.begin().await?;
                // RLS scope (ADR-0008): bind the line's own company (read above) onto this transaction.
                company_scope::bind_company_on(&mut tx, company_id).await?;
                self.repos.clearances.insert_charge_clearance(&mut tx, &NewChargeClearanceRow {
                    id: clearance_id,
                    company_id,
                    bank_transaction_id: ch.bank_transaction_id,
                    charge_account_id: ch.charge_account_id,
                    matched_amount: amount,
                    clearance_date: ch.clearance_date,
                    accounting_post_id: ack.post_id,
                    journal_id: ack.journal_id,
                }).await?;
                let new_alloc = allocated + amount;
                let fully = new_alloc >= line_net;
                self.repos.transactions.set_allocation(
                    &mut tx, ch.bank_transaction_id, new_alloc,
                    if fully { "reconciled" } else { "partly_reconciled" },
                ).await?;
                tx.commit().await?;
                self.sink.publish(BankingEvent::BankChargeRecognized(BankChargeRecognized {
                    bank_transaction_id: ch.bank_transaction_id, company_id, amount,
                    journal_id: ack.journal_id, post_id: ack.post_id,
                }));
                Ok(ClearOutcome { clearance_id, post_id: ack.post_id, journal_id: ack.journal_id, fully_reconciled: fully })
            }
            Err(rej) => Err(BankingError::GlRejected { code: rej.code, message: rej.message }),
        }
    }

    // ---- reconciliation session --------------------------------------------

    /// Open/close a reconciliation session (council 2026-07-05 — line-completeness close-gate). A
    /// session **cannot close on the two supplied balances agreeing alone** — it must also have zero
    /// open lines in the period, or it would sign off "the bank agrees with our books" while
    /// transactions sit unreconciled (a false attestation). Three-state (activating `ReconStatus`):
    ///   - `computed_difference ≠ 0` → **open** (numbers disagree)
    ///   - `= 0` but exceptions remain → **balanced** (agree, NOT finalized, no event)
    ///   - `= 0` and zero exceptions → **closed** (+ emit `BankReconciliationClosed`)
    /// `unreconciled_count` is persisted as the exception snapshot at sign-off (audit-reconstructable).
    /// Note: period-scoped by `txn_date ∈ [from_date, to_date]`. `ledger_balance` is still supplied
    /// (recompute is parked — see ADR-001); the line-count is `reconcile`'s real assertion.
    pub async fn reconcile(&self, r: NewReconciliation) -> Result<ReconcileOutcome, BankingError> {
        // RLS scope (ADR-0008): company is on the DTO — bind it for the whole body. The exception COUNT
        // is the close-gate's assertion, so it MUST be fenced: an unscoped count would read 0 rows and
        // wrongly close a session with open lines. The explicit `company_id=$1` filter stays as
        // defense-in-depth.
        let company = r.company_id;
        company_scope::with_company_scope(Some(company), async move {
        let diff = money(r.statement_closing_balance - r.ledger_balance);
        let id = Uuid::new_v4();
        let unreconciled: i64 = self.repos.transactions.count_open_in_period(
            &self.db_pool, r.company_id, r.bank_account_id, r.from_date, r.to_date,
        ).await?;
        let status = if !diff.is_zero() { "open" } else if unreconciled > 0 { "balanced" } else { "closed" };
        self.repos.reconciliations.insert_reconciliation(&self.db_pool, &NewReconciliationRow {
            id,
            company_id: r.company_id,
            bank_account_id: r.bank_account_id,
            from_date: r.from_date,
            to_date: r.to_date,
            statement_closing_balance: money(r.statement_closing_balance),
            ledger_balance: money(r.ledger_balance),
            computed_difference: diff,
            unreconciled_count: unreconciled as i32,
            status,
        }).await?;
        // Attest "the bank agrees with our books" ONLY when the session actually closes.
        if status == "closed" {
            self.sink.publish(BankingEvent::BankReconciliationClosed(BankReconciliationClosed {
                reconciliation_id: id, bank_account_id: r.bank_account_id, difference: diff,
            }));
        }
        Ok(ReconcileOutcome { id, difference: diff, unreconciled_count: unreconciled, status: status.to_string() })
        }).await
    }
}
