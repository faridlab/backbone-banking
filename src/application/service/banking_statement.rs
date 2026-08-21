//! Statement ingest + match proposal (hand-authored, user-owned).
//!
//! An `impl BankingWriteService` chunk over the vocabulary in [`super::banking_write_service`]:
//! validate a statement's balance continuity (`opening + Σdeposit − Σwithdrawal = closing`), persist the
//! import header + its lines as ONE unit of work, and emit `BankStatementImported`; then propose a match
//! for a line from supplied candidates (exact amount + reference, else exact amount). Pure selection —
//! `propose_match` persists nothing; `clear_transaction` (see [`super::banking_clearance`]) consumes
//! its output.
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! `BankStatementImportRepository` / `BankTransactionRepository`, whose custom methods take this
//! service's transaction so an import's header + lines commit as one unit.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::infrastructure::persistence::{NewBankTransactionRow, NewStatementImportRow};

use super::banking_events::{BankStatementImported, BankingEvent};
use super::banking_write_service::{
    money, BankingError, BankingWriteService, MatchCandidate, NewStatementImport,
};

impl BankingWriteService {
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
            return Err(BankingError::BalanceMismatch {
                expected: closing,
                computed,
            });
        }
        let id = Uuid::new_v4();
        let fmt = imp.source_format.clone().unwrap_or_else(|| "manual".into());
        // RLS scope (ADR-0008): company is on the DTO — bind it explicitly onto the transaction we own,
        // so both the import header and every line insert pass their WITH CHECK.
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, imp.company_id).await?;
        self.repos
            .imports
            .insert_import(
                &mut tx,
                &NewStatementImportRow {
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
                },
            )
            .await?;
        for l in &imp.lines {
            self.repos
                .transactions
                .insert_transaction(
                    &mut tx,
                    &NewBankTransactionRow {
                        id: Uuid::new_v4(),
                        company_id: imp.company_id,
                        bank_account_id: imp.bank_account_id,
                        import_id: id,
                        txn_date: l.txn_date,
                        description: l.description.as_deref(),
                        reference_no: l.reference_no.as_deref(),
                        deposit: money(l.deposit),
                        withdrawal: money(l.withdrawal),
                    },
                )
                .await?;
        }
        tx.commit().await?;
        self.sink
            .publish(BankingEvent::BankStatementImported(BankStatementImported {
                import_id: id,
                bank_account_id: imp.bank_account_id,
                row_count: imp.lines.len() as i32,
            }));
        Ok(id)
    }

    /// Propose a match for a statement line from supplied candidates: prefer an exact amount + exact
    /// reference (`exact`), else an exact amount (`fuzzy`). Pure selection — persists nothing.
    pub async fn propose_match(
        &self,
        bank_transaction_id: Uuid,
        candidates: &[MatchCandidate],
    ) -> Result<Option<MatchCandidate>, BankingError> {
        // RLS scope (ADR-0008), ID-only pattern: identified by the line id alone — no company arg. This
        // rides the request-dedicated connection carrying the caller's `app.company_id`, so RLS fences
        // the lookup and another company's line is simply not found.
        let row = self
            .repos
            .transactions
            .fetch_match_basis(&self.db_pool, bank_transaction_id)
            .await?
            .ok_or(BankingError::TransactionNotFound(bank_transaction_id))?;
        let net = row.deposit + row.withdrawal;
        let txn_ref: Option<String> = row.reference_no;
        // exact amount + reference first
        if let Some(c) = candidates
            .iter()
            .find(|c| c.amount == net && c.reference.is_some() && c.reference == txn_ref)
        {
            return Ok(Some(c.clone()));
        }
        Ok(candidates.iter().find(|c| c.amount == net).cloned())
    }
}
