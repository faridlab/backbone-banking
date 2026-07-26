//! Reconciliation session — the close-gate (hand-authored, user-owned).
//!
//! An `impl BankingWriteService` chunk over the vocabulary in [`super::banking_write_service`]. Open
//! or close a reconciliation session under the council 2026-07-05 line-completeness gate: a session
//! **cannot close on the two supplied balances agreeing alone** — it must also have zero open lines in
//! the period, or it would sign off "the bank agrees with our books" while transactions sit
//! unreconciled (a false attestation). Three-state (the persisted `status`):
//!   - `computed_difference ≠ 0` → **open** (numbers disagree)
//!   - `= 0` but exceptions remain → **balanced** (agree, NOT finalized, no event)
//!   - `= 0` and zero exceptions → **closed** (+ emit `BankReconciliationClosed`)
//!
//! Per the module's 4-layer rule this file holds no SQL — the exception COUNT (the close-gate's
//! assertion) lives on `BankTransactionRepository`, and the session row on `BankReconciliationRepository`.

use backbone_orm::company_scope;
use uuid::Uuid;

use crate::infrastructure::persistence::NewReconciliationRow;

use super::banking_events::{BankReconciliationClosed, BankingEvent};
use super::banking_write_service::{money, BankingError, BankingWriteService, NewReconciliation, ReconcileOutcome};

impl BankingWriteService {
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
