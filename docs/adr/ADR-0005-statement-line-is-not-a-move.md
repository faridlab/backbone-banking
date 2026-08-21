# ADR-0005 — A statement line is not a move

- Status: Accepted (2026-08-21)
- Context: banking statement import + clearing engine
- Supersedes: none; refines the delegation stance of ADR-001

## Decision

An imported bank statement line is **evidence, not accounting**. Import never touches the GL. The
first GL contact a line can have is `clear_transaction`'s `Dr Bank · Cr Clearing` (or its mirror,
plus the reconciliation-graph edge when the match is a payment). Until a clear commits, an
uncleared line is GL-invisible — that is the invariant, and the `born_posted_invariant_suite`
pins it:

- no draft lifecycle exists anywhere on the statement side (`import_status` is ingest state, not
  posting state — a line is never "waiting to post");
- exactly one deterministic clearance post per `bankclr:{txn}:{source_type}:{source_id}:{cumulative}`
  identity (uuid5) — a retried clear reuses the stranded journal, never doubles it;
- an uncleared line changes no account balance, journal, or reconciliation-graph edge.

## Why (the double-count argument)

The tempting alternative — post a suspense entry at import (`Dr Bank · Cr Suspense` per line),
then swap suspense for the matched document at clear — double-counts the bank leg. The clear's own
post already writes `Dr Bank`; an import-time bank debit would leave the bank account holding each
movement twice until matching caught up, and an import that never matches (a bank fee nobody
clears, a duplicate download) would leave a permanent, *wrong* suspense balance. Under our fences
the wrongness is not hypothetical: the reconciliation session's `ledger_balance` is read from the
GL, so import-time posts would corrupt the close gate itself.

Delegation shape: a statement line delegates by **reference** (like payment's entry → its
allocations), NOT `_inherits`-style copying. The line owns its identity and ingest facts; the
clearance owns the accounting facts. Merging the two shapes (a "line that is also a move") is the
anti-unification note: payment entries and statement lines look similar (both await bank
confirmation) but differ in who owns the money — a payment moves OUR money on OUR books, a
statement line records the BANK's claim about the same world. Converging them would force one
status machine to mean both "our journal is committed" and "their statement mentions it", which
are different truths at different times.

## Parked: import-time suspense redesign (costed sketch)

If a future need demands import-time GL presence (intraday cash visibility before matching), the
sound shape is:

1. Import posts `Dr Bank · Cr Suspense` **per statement closing balance**, not per line — a
   period-level capitalisation, so re-imports dedup on (account, period).
2. `clear_transaction` posts only the clearing leg (`Dr Clearing · Cr Suspense-path`), with the
   bank leg suppressed by an explicit `suppress_bank_leg` flag derived from whether the period was
   capitalised — the two paths must never both write the bank debit.
3. Recon sessions subtract uncapitalised periods from `ledger_balance`.

Cost: a second posting mode through the whole clear path, a suppression flag that must be correct
under retries, and a recon adjustment. Benefit: cash visibility between import and match. Verdict
when parked: not worth it before the bank-rec UI exists; revisit when operators actually ask for
intraday GL cash. Tracked as a parked follow-up in the workspace DIT tracker.

## Consequences

- Uncleared lines are invisible to the GL and to reconciliation closing; the exception snapshot
  (`unreconciled_count`) is the operator's worklist, not a GL state.
- `import_status` stays an ingest lifecycle; no statement-side posting state will be added.
- FX realisation (`FxGainLoss`) keys off the clearance, not the line — consistent with this ADR.
- The FX rate read moved with the catalogue: corporate's `spot_on_or_before` is a deliberate
  semantic change from banking's retired point-in-time table — it keeps latest-on-or-before
  semantics on a gapless chain but REFUSES across a gap (a window deliberately closed stays
  retired; the old table had no end dates and could not see holes). First-cycle retirement: the
  drop lands before any environment holds real rates.
