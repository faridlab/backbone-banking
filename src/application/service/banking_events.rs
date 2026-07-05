//! Banking domain events (hand-authored, user-owned) — the public extension surface.
//!
//! `BankTransactionCleared` lets a consumer (e.g. payment, to mark a settlement bank-confirmed, or a
//! cash-flow dashboard) react. Banking is downstream of payment/billing — it *matches against* their
//! documents and confirms the cash actually landed.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A statement was imported with N lines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BankStatementImported {
    pub import_id: Uuid,
    pub bank_account_id: Uuid,
    pub row_count: i32,
}

/// A statement line was matched to a settled document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BankTransactionMatched {
    pub bank_transaction_id: Uuid,
    pub matched_source_type: String,
    pub matched_source_id: Uuid,
    pub amount: Decimal,
}

/// A statement line was cleared through the GL (the clearing post landed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BankTransactionCleared {
    pub bank_transaction_id: Uuid,
    pub matched_source_type: String,
    pub matched_source_id: Uuid,
    pub company_id: Uuid,
    pub journal_id: Uuid,
    pub post_id: Uuid,
    pub amount: Decimal,
}

/// A reconciliation session was closed (difference == 0).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BankReconciliationClosed {
    pub reconciliation_id: Uuid,
    pub bank_account_id: Uuid,
    pub difference: Decimal,
}

/// An outflow was recognised as a bank charge and posted to expense.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BankChargeRecognized {
    pub bank_transaction_id: Uuid,
    pub company_id: Uuid,
    pub amount: Decimal,
    pub journal_id: Uuid,
    pub post_id: Uuid,
}

/// The banking domain-event union.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum BankingEvent {
    BankStatementImported(BankStatementImported),
    BankTransactionMatched(BankTransactionMatched),
    BankTransactionCleared(BankTransactionCleared),
    BankReconciliationClosed(BankReconciliationClosed),
    BankChargeRecognized(BankChargeRecognized),
}

/// Sink for banking domain events. Fire-and-forget; a real adapter wires a bus, tests record.
pub trait BankingEventSink: Send + Sync {
    fn publish(&self, event: BankingEvent);
}

/// Default sink — emits structured tracing events.
pub struct LoggingSink;

impl BankingEventSink for LoggingSink {
    fn publish(&self, event: BankingEvent) {
        tracing::info!(target: "banking.events", ?event, "banking domain event");
    }
}
