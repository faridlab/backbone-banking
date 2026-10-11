// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The hand-written banking SQL's parameter/projection types. Every repository listed here is declared
// `user_owned` in metaphor.codegen.yaml — see the write service, which orchestrates them.
pub use bank_account_repository::NewBankAccountRow;
pub use bank_clearance_repository::{NewChargeClearanceRow, NewClearanceRow};
pub use bank_reconciliation_repository::NewReconciliationRow;
pub use bank_repository::NewBankRow;
pub use bank_statement_import_repository::NewStatementImportRow;
pub use bank_transaction_repository::{
    CandidateLineBasisRow, ChargeLineRow, ClearingLineRow, MatchBasisRow, NewBankTransactionRow,
};
pub use fx_gain_loss_repository::NewFxGainLossRow;
