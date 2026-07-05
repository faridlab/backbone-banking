# ADR-0004: Banking owns reconciliation, posts the clearing leg only, and matches via supplied candidates

- **Status:** Accepted — Applied 2026-07-05
- **Date:** 2026-07-05
- **Deciders:** Farid (owner), build session 2026-07-05

> This is the handbook summary of the module decision. The full record — with the cross-module
> financials context and the maturity/completeness-council findings — is
> [`docs/adr/ADR-001-banking-boundary-and-clearing-seam.md`](../../adr/ADR-001-banking-boundary-and-clearing-seam.md).
> Where they differ, the full record wins.

## Context

`backbone-banking` is the cash & bank context of the Financials pillar — the reconciliation
truth-source between the ledger of record and the outside-world cash position. Payment settles a
receipt to a **clearing** account (undeposited funds); the money is not truly "in the bank" until the
statement confirms it. Banking ingests the statement, matches a line to the payment, and posts the
bank-side leg that moves value `clearing → bank`. It holds no masters beyond its own bank/account
catalogue; account, payment, and company are logical FKs. The open question: how much of the
financial machinery does *banking* own, and how does it touch its siblings without coupling to them?

## Decision

1. **Six entities, one clearing shape.** `Bank` + `BankAccount` (each account carries `gl_account_id`
   = real bank and `clearing_account_id` = undeposited funds), `BankStatementImport` +
   `BankTransaction`, `BankClearance`, `BankReconciliation`. Import validates **balance continuity**
   (`opening + Σdeposit − Σwithdrawal = closing`). On clear, banking assembles ONE balanced
   `AccountingPost` and refuses unless `Σdebit = Σcredit`: deposit → `Dr Bank · Cr Clearing`;
   withdrawal → `Dr Clearing · Cr Bank`. Banking posts the **bank-side leg only** — never the
   original A/R/A/P settlement (payment owns it). Generic mutating CRUD is not on the guarded surface.
2. **Matching is candidate-supplied — banking reads no sibling tables.** `propose_match` selects from
   a `Vec<MatchCandidate>` the composition supplies (from payment/billing): exact amount + reference →
   exact amount → none. The shipped library has **zero normal dependency** on payment/billing; the
   seam is a supplied candidate list plus emitted events, not a table read.
3. **Clearing is bounded on two dimensions.** A clearance cannot exceed (a) a line's un-allocated
   remainder (`over_allocated`) nor (b) the settled document's amount (`settlement_over_cleared`).
   Bound (b) sums prior `bank_clearances.matched_amount` for the settlement under a **per-settlement
   advisory lock** held across the post, so it is race-safe and closes both the double-match and the
   retry double-clear. It is an **amount** bound, not a unique constraint — one payment may
   legitimately land as several deposits. A rejected GL post writes no clearance and no allocation.
   IDR only for now.
4. **Reconciliation is line-completeness gated (three-state).** `reconcile` resolves to `open`
   (numbers disagree), `balanced` (agree, exceptions outstanding, **no** event), or `closed` (agree
   **and** zero open lines → `BankReconciliationClosed`). `unreconciled_count` is persisted as the
   exception snapshot. This stops a session attesting "the bank agrees with our books" while lines
   sit unreconciled.
5. **Bank charges are an expense** (`Dr Bank Charges · Cr Bank`), never a tax line.

## Alternatives considered

- **Banking posts both legs (settlement + bank).** Would duplicate ledger authority payment already
  owns and couple the two modules' posting logic. Rejected — one leg, one owner.
- **Banking queries payment/billing directly to find matches.** Simpler call site, but a hard
  table-level dependency that breaks independent deployment and the bounded-context rule. Rejected in
  favor of supplied candidates + a `GlPostSink` port.
- **A unique constraint to stop double-clearing** (one clearance per settlement). Would reject a
  payment that legitimately lands as several deposits. Rejected for the amount bound (b), which
  permits splits while still capping the total.
- **Two-state reconciliation (open/closed on the difference alone).** Lets a diff-zero session close
  with unreconciled lines — a false attestation on a financial control. Rejected for the three-state
  model; proven by golden case BGC-7.

## Consequences

**Proven, not asserted:** `tests/clearing_seam.rs` runs payment → accounting → banking → accounting
(CLSEAM-1/2/3): the clearing account nets to zero, the bank GL holds the cash, A/R stays settled, and
the settlement bound refuses the re-import double-clear. `scripts/clearing_seam_roundtrip.sh`
regenerates **both** modules and asserts every seam file is byte-identical and still green — the
design survives regen of both sides.

**To live with — deferred (per the brief):** live bank API / host-to-host files, a rule-learning
matcher, bank guarantee / invoice discounting, multi-currency/FX clearing.

**To live with — parked with gates:** sink↔write idempotency on retry (gate: the production
bus/outbox); `reconcile` trusts a supplied `ledger_balance` (gate: the read-side composition
recomputes from `accounting.ledgers`); `import_statement` has no file-level idempotency (gate: the
ingest workstream); deposit-XOR-withdrawal per line (gate: a one-line import guard); CSV parsing +
mapping templates (gate: the ingest/adapter workstream); a VA-reference normalizer (gate: a real
Indonesian statement sample).
