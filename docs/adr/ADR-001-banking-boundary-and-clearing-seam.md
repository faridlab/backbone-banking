# ADR-001: Banking owns bank reconciliation; it posts the clearing leg only, matching via supplied candidates

**Status**: Accepted — Applied 2026-07-05
**Deciders**: Farid (owner), build session 2026-07-05
**Related**: `docs/erp/financials.md`, `docs/erp/gl-posting-contract.md`,
`docs/erp/modules/backbone-banking.md`, payment ADR-001/002 (settlement), `extension-contract.md` §5

## Context

`backbone-banking` is the cash & bank context of the Financials pillar — the reconciliation
truth-source between the ledger of record and the outside-world cash position. Payment settles a
receipt to a **clearing** account (undeposited funds); the money is not truly "in the bank" until the
statement confirms it. Banking is the module that ingests the statement, matches the line to the
payment, and posts the bank-side leg that moves value clearing → bank. It holds no masters beyond its
own bank/account catalogue; account/payment/company are logical FKs.

## Decision

1. **Six entities, one clearing shape.** Bank + BankAccount (each account carries `gl_account_id` =
   real bank + `clearing_account_id` = undeposited funds), BankStatementImport (+BankTransaction),
   BankClearance, BankReconciliation. Import validates **balance continuity** (`opening + Σdeposit −
   Σwithdrawal = closing`); generic CRUD is not mounted on the guarded surface. On clear, banking
   assembles ONE balanced `AccountingPost` and refuses unless `Σdebit = Σcredit`:
   - **received (deposit):** `Dr Bank · Cr Clearing`.
   - **paid (withdrawal):** `Dr Clearing · Cr Bank`.
   Banking posts the **bank-side leg only** — never the original A/R/A/P settlement (payment owns it).
2. **Matching is candidate-supplied — banking reads no sibling tables.** `propose_match` selects from a
   `Vec<MatchCandidate>` the composition supplies (from payment/billing): exact amount + reference →
   exact amount → none. The shipped banking library has **zero normal dependency** on payment/billing;
   the seam is a supplied candidate list + emitted events, not a table read.
3. **Clearing is bounded on TWO dimensions (council 2026-07-05).** A clearance cannot exceed (a) a
   line's un-allocated remainder (`over_allocated`) **nor** (b) the settled document's amount
   (`Σ clearances for a settlement + matched ≤ matched_source_amount`, else `settlement_over_cleared`).
   Bound (b) is the fix the maturity council forced: the line bound alone let the SAME payment be
   cleared against two bank lines (a re-imported statement, a retry) — each passing its own line's
   guard — stranding the clearing account at a phantom credit and overstating the bank GL. The bound
   sums prior `bank_clearances.matched_amount` for the settlement under a **per-settlement advisory
   lock** inside the clearing transaction (held across the post), so it is race-safe and closes both
   the double-match and the retry double-clear. It is an **amount** bound, not a unique constraint —
   one payment may legitimately land as several deposits. `matched_source_amount` is supplied by the
   composition (from `MatchCandidate.amount`); banking still reads no sibling tables. Each clearance
   uses a fresh `source_id` so partial clears are distinct posts; a rejected GL post writes no
   clearance and no allocation. Only IDR for now.
4. **Reconciliation is line-completeness gated (three-state; completeness council 2026-07-05).** The
   `ReconStatus` enum's `balanced` variant existed in the schema but was written nowhere — the code
   collapsed to `closed`/`open` on the difference alone, so a session could sign off "the bank agrees
   with our books" while transactions sat unreconciled (a false attestation on a financial control).
   `reconcile` now counts open lines (`unreconciled`/`partly_reconciled`) in the period and resolves:
   `open` (numbers disagree) | `balanced` (agree, exceptions outstanding, **no** event) | `closed`
   (agree **and** zero exceptions → `BankReconciliationClosed`). `unreconciled_count` is persisted as
   the exception snapshot at sign-off. Period-scoped by `txn_date ∈ [from_date, to_date]`.
   `ledger_balance` remains supplied (recompute parked, below) — the line-count is `reconcile`'s real
   assertion. Proven by golden case **BGC-7** (a diff-0 session with an open line stays `balanced`,
   emits no event; fails against a two-state close).
5. **Bank charges are an expense** (`Dr Bank Charges · Cr Bank`), never a tax line.

## Consequences

- **Proven, not asserted:** `tests/clearing_seam.rs` runs payment → accounting → banking → accounting:
  a payment settles 750,000 to the clearing account (`Dr Clearing · Cr A/R`), banking imports the
  statement, matches the deposit to the payment, and clears (`Dr Bank · Cr Clearing`) — the **clearing
  account nets to zero**, the bank GL holds 750,000, and A/R stays settled. Zero normal Cargo edges.
- **Extension-contract §5 discharged:** `scripts/clearing_seam_roundtrip.sh` regenerates **both**
  modules and asserts every ACL/consumer file is byte-identical and the seam stays green.
- This is the **fifth proven cross-module GL seam** and the fourth GL leg of the cash loop — the cash
  position now reconciles document → settlement → bank end-to-end.
- Deferred (per the brief): live bank API / host-to-host files, rule-learning matcher, bank
  guarantee / invoice discounting, multi-currency/FX clearing. Residual: a production bus + a real
  candidate-query service to own the composition; the on-clear callback to payment to mark a
  settlement bank-confirmed.
- **Parked (completeness council 2026-07-05):** (a) **CSV parsing + a mapping-template entity** —
  `import_statement` takes already-parsed `NewStatementLine[]` (`source_format` enum + `file_ref`
  exist); a byte-level parser carries no domain invariant. Gate: the ingest/adapter workstream owns
  `bytes → NewStatementLine[]` + per-bank field maps. (b) **VA-reference normalizer** — exact
  `reference_no` match is implemented + proven; a prefix/pad normalizer is a refinement. Gate: a real
  Indonesian statement sample showing VA echoes are not byte-identical.
- **Parked with gates (maturity council 2026-07-05):** (1) **sink↔write atomicity** — the clearing
  post and the `bank_clearances` write are one tx now, but a crash between the ledger commit and the
  clearance commit would re-post under a fresh key on retry; gate = the production bus/outbox increment
  (derive the idempotency key from `(bank_transaction_id, matched_source_id)` then). (2) **`reconcile`
  trusts a supplied `ledger_balance`** — recompute from `accounting.ledgers`; gate = the read-side
  composition. (3) **`import_statement` has no file-level idempotency** — a re-import duplicates lines
  (the settlement bound now neutralises the *money* consequence); gate = the ingest workstream.
  (4) **deposit-XOR-withdrawal** — a line with both set mis-signs; gate = a 1-line import guard.
