// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod banking_events;
pub mod banking_gl;
pub mod banking_write_service;
// Fail-closed IBAN validation for the bank-account write path (registry table, mod-97
// checksum, named unknown-country escape; local-format account numbers pass unchanged).
pub mod iban_validation;
// The write surface, chunked: each is an `impl BankingWriteService` block over the vocabulary that
// stays in `banking_write_service` (so the `banking_write_service::{NewClearance, ...}` import paths
// are unchanged).
pub mod banking_clearance;
pub mod banking_reconciliation;
pub mod banking_statement;
pub mod reconcile_preset_candidates;
pub use reconcile_preset_candidates::{
    CandidateLineBasis, CandidatePoolPort, OpenCandidate, PaymentPoolRead, RankedCandidate,
};
pub mod fx_service;
pub use fx_service::{ExchangeRateProvider, ExchangeRateSnapshot, FxError, FxResult, FxService};
pub use iban_validation::{
    is_iban_shaped, normalize_iban, validate_iban, validate_iban_with, IbanError,
    IbanValidationPolicy, ALLOW_UNKNOWN_COUNTRIES_ENV,
};
pub use banking_events::{
    BankChargeRecognized, BankReconciliationClosed, BankStatementImported, BankTransactionCleared,
    BankTransactionMatched, BankingEvent, BankingEventSink, LoggingSink,
};
pub use banking_gl::{AccountingPostEnvelope, GlPostAck, GlPostLine, GlPostRejected, GlPostSink};
pub use banking_write_service::{
    BankingError, BankingWriteService, ClearOutcome, MatchCandidate, NewBank, NewBankAccount,
    NewCharge, NewClearance, NewReconciliation, NewStatementImport, NewStatementLine,
    ReconcileOutcome,
};
