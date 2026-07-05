# BRD — backbone-banking

> Business Requirements & Rules. Tier 2 · Financials. Date: 2026-07-05. Pairs with
> `docs/business-flows/golden-cases.md`.

## Documents
Bank + BankAccount (masters; each account carries `gl_account_id` = real bank + `clearing_account_id`
= undeposited funds) · BankStatementImport (+BankTransaction lines) · BankClearance (match link) ·
BankReconciliation (session).

## Business rules
**BR-1 (balance continuity — import).** A statement imports only if `opening_balance + Σdeposit −
Σwithdrawal = closing_balance` (2dp). → `balance_mismatch`. ≥1 line (`empty_statement`); no negative
deposit/withdrawal.

**BR-2 (candidate-supplied matching — bounded context).** `propose_match` selects from candidates
**supplied by the composition** (from payment/billing): exact amount + exact reference (`exact`) →
exact amount (`fuzzy`) → none. Banking holds no normal dependency on payment/billing and reads none of
their tables — the seam is a supplied candidate list + an emitted event.

**BR-3 (clearing post — ADR-001).** Clearing emits ONE balanced `AccountingPost` (refused unless
`Σdebit = Σcredit`): received (deposit) `Dr Bank · Cr Clearing`; paid (withdrawal) `Dr Clearing · Cr
Bank`. Banking posts the **bank-side leg only** — never the original A/R/A/P settlement (payment owns
that). Only IDR end-to-end for now (`unsupported_currency`).

**BR-4 (bounded clearing — two dimensions).** A clearance cannot exceed (a) a line's un-allocated
remainder (`matched ≤ line_net − allocated_amount`, else `over_allocated`) **nor** (b) the settled
document's amount (`Σ clearances for a settlement + matched ≤ matched_source_amount`, else
`settlement_over_cleared` — council 2026-07-05). Both bounds matter: (a) stops over-clearing a line;
**(b) stops clearing one payment twice** (a re-imported line, a retry, two operators) — without it the
clearing account is left with a phantom credit and the bank GL is overstated. A line may still split
across clearances, and one payment may legitimately land as several deposits (the amount bound permits
that; a unique constraint would not). The settlement bound is taken under an advisory lock per
settlement inside the clearing transaction. A rejected GL post writes no clearance and no allocation.

**BR-5 (bank charge).** An outflow recognised as a fee posts `Dr Bank Charges (expense) · Cr Bank`
via its own post. Never a tax line.

**BR-6 (reconciliation close — three-state, line-completeness gated; council 2026-07-05).** A session
computes `computed_difference = statement_closing_balance − ledger_balance` **and** counts open lines
(`unreconciled`/`partly_reconciled`) in the period. It resolves to: `open` (numbers disagree) |
`balanced` (numbers agree but exceptions outstanding — **not** finalized) | `closed` (agree **and**
zero exceptions). `BankReconciliationClosed` fires **only** on `closed` — so the control never
attests "the bank agrees with our books" while transactions are unreconciled. `unreconciled_count` is
persisted as the exception snapshot at sign-off (audit-reconstructable). The ledger balance is still
supplied from accounting's read model (recompute parked — ADR-001).

**BR-7 (idempotency key per clearance).** Every clearing/charge post carries a distinct `source_id`
(the fresh clearance id) + `idempotency_key = bankclr:<txn>:<clearance>` — so partial clears of one
line are distinct posts, and a literal retry of the same clearance dedups at accounting.

## Events
`BankStatementImported`, `BankTransactionMatched`, `BankTransactionCleared`, `BankChargeRecognized`,
`BankReconciliationClosed`. (Consumed downstream: `BankTransactionCleared` by payment to mark a
settlement bank-confirmed; by cash-flow/treasury dashboards.)

## Deferred (with reason)
`bank_guarantee`, `invoice_discounting`, host-to-host file generation, live bank API polling,
rule-learning matcher, multi-currency/FX clearing.
