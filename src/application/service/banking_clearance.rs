//! Clearing engine — the bank-side GL leg (hand-authored, user-owned).
//!
//! An `impl BankingWriteService` chunk over the vocabulary in [`super::banking_write_service`]. Two
//! symmetric seams that CLEAR a matched statement line through the GL:
//!   - **`clear_transaction`** (received/paid): `Dr Bank · Cr Clearing` (or its mirror), bounded by
//!     BOTH the line's un-allocated remainder AND the settlement's un-cleared remainder (a payment
//!     cannot be cleared twice).
//!   - **`recognize_bank_charge`**: `Dr Bank Charges · Cr Bank`.
//! Both post through the supplied `GlPostSink`, write a `BankClearance` row, advance the line's
//! `allocated_amount` + status, and emit the corresponding event.
//!
//! Per the module's 4-layer rule this file holds no SQL — the line reads, the settlement advisory
//! lock + already-cleared SUM, the clearance inserts, and the allocation watermark live on
//! `BankTransactionRepository` / `BankClearanceRepository`, whose methods take this service's
//! transaction so the post + clearance + allocation advance commit as one unit.

use backbone_orm::company_scope;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::domain::entity::TxnStatus;
use crate::infrastructure::persistence::{NewChargeClearanceRow, NewClearanceRow};

use super::banking_events::{
    BankChargeRecognized, BankTransactionCleared, BankTransactionMatched, BankingEvent,
};
use super::banking_gl::{
    AccountingPostEnvelope, GlPostLine, GlPostSink, ReconcileLine, ReconcileOrigin,
    ReconcilePairRequest, ReconcileSink,
};
use super::banking_write_service::{
    money, BankingError, BankingWriteService, ClearOutcome, NewCharge, NewClearance,
};

impl BankingWriteService {
    /// Clear a matched statement line through the GL — the bank-side leg. received (deposit):
    /// `Dr Bank · Cr Clearing`; paid (withdrawal): `Dr Clearing · Cr Bank`. Bounded: the clearance
    /// cannot exceed the line's un-allocated remainder. Records a `BankClearance`, advances the line's
    /// `allocated_amount` + status, and emits `BankTransactionMatched` + `BankTransactionCleared`.
    ///
    /// When the line was matched to a **payment**, the same unit of work also writes the
    /// reconciliation-graph edge on the clearing account: the payment's clearing leg meets the
    /// clearance's clearing leg (the pair that nets the undeposited-funds position to zero), through
    /// the shared [`ReconcileSink`] port with origin `Clearing`. Receipt: the payment DEPOSITED into
    /// clearing (`Dr Clearing`) and the clearance withdraws it (`Cr Clearing`); paid-out mirrors.
    /// **Fail-closed:** if the sink refuses the pair (unposted payment journal, a clearing account
    /// that is not flagged reconcilable, a clamp disagreement) the clearance row and the allocation
    /// advance roll back — banking refuses to record a clear whose ledger-side edge cannot exist.
    /// (The clearing post itself is committed by its own sink before the edge; a refusal therefore
    /// strands that journal — bounded by the settlement fence, which did not advance, so the
    /// operator fixes the cause and re-clears. The stranded post is an integrity-probe finding, not
    /// silent money.) Matched-source kinds other than `payment` keep the bounded clearance only —
    /// their graph convergence is the statement-side reconciliation surface.
    pub async fn clear_transaction(&self, c: NewClearance, sink: &dyn GlPostSink, reconcile: &dyn ReconcileSink) -> Result<ClearOutcome, BankingError> {
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
        self.repos.clearances.lock_settlement(&mut tx, company_id, c.matched_source_id).await?;
        let already_cleared = self.repos.clearances
            .sum_cleared_against_settlement(&mut tx, company_id, &c.matched_source_type, c.matched_source_id).await?;
        if already_cleared + matched > money(c.matched_source_amount) {
            return Err(BankingError::SettlementOverCleared { settlement_amount: money(c.matched_source_amount), already_cleared, attempted: matched });
        }

        match sink.post(&env).await {
            Ok(ack) => {
                // The clearing edge (payment-matched only). The post above has committed the
                // clearance's clearing leg into the ledger; this pairs it with the payment's own
                // clearing leg on the same account, on the SAME transaction as the clearance row +
                // allocation advance below — so the graph edge and the banking bookkeeping commit
                // or roll back together. Direction follows the flow: a receipt deposits INTO
                // clearing on the payment (its leg is the debit) and the clearance withdraws
                // (credit); a payout mirrors.
                if c.matched_source_type == "payment" {
                    let payment_leg = ReconcileLine::new("payment", c.matched_source_id, clearing);
                    let clearance_leg = ReconcileLine::new("settlement", clearance_id, clearing);
                    let (debit, credit) = if is_receipt {
                        (payment_leg, clearance_leg)
                    } else {
                        (clearance_leg, payment_leg)
                    };
                    let edge = reconcile
                        .reconcile_pair_on(&mut *tx, &ReconcilePairRequest {
                            company_id,
                            debit,
                            credit,
                            amount: matched,
                            origin: ReconcileOrigin::Clearing,
                        })
                        .await
                        .map_err(|r| BankingError::ReconcileRefused { code: r.code, message: r.message })?;
                    if edge.applied != matched {
                        // The graph clamped to a different amount than banking's own bound — the
                        // bank bookkeeping and the ledger disagree about what is clearable. Refuse
                        // rather than drift.
                        return Err(BankingError::ReconcileRefused {
                            code: "reconcile_clamp_mismatch".into(),
                            message: format!(
                                "banking cleared {matched} but the graph applied {}",
                                edge.applied
                            ),
                        });
                    }
                }
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
                let status = if fully { TxnStatus::Reconciled } else { TxnStatus::PartlyReconciled };
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
                    if fully { TxnStatus::Reconciled } else { TxnStatus::PartlyReconciled },
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
}
