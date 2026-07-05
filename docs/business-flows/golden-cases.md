# Banking — Golden Cases (the numeric oracle)

Mirrors `tests/banking_golden_cases.rs`, `tests/integrity_probes.rs`, and the cross-module clearing
seam in `tests/clearing_seam.rs`. Money is exact IDR (2dp, half-up).

## Write path (`tests/banking_golden_cases.rs`)

| Case | Input | Expected |
|------|-------|----------|
| **BGC-1** | import: opening 1,000,000 + deposit 500,000 − withdrawal 200,000 = closing 1,300,000 | `imported`, 2 lines, `BankStatementImported`. Wrong closing → `balance_mismatch`; no lines → `empty_statement`. |
| **BGC-2** | propose_match: line 500,000 ref `VA-12345` vs two 500,000 candidates | the one whose reference matches wins; no candidate at the amount → none. |
| **BGC-3** | clear a 750,000 deposit | post `Dr Bank 750,000 · Cr Clearing 750,000` (balanced); line `reconciled`; `BankTransactionCleared`. |
| **BGC-4** | clear a 400,000 withdrawal in two parts (250k + 150k) | post `Dr Clearing · Cr Bank`; after 250k → `partly_reconciled` (allocated 250,000); after 150k → `reconciled`. |
| **BGC-5** | recognise a 15,000 outflow as a bank charge | post `Dr Bank Charges 15,000 · Cr Bank 15,000`. |
| **BGC-6** | reconcile a fresh account (no lines): closing 1,300,000 vs ledger 1,300,000 | difference `0` **and zero open lines** → `closed` + `BankReconciliationClosed`; ledger 1,250,000 → difference `50,000`, `open`. |
| **BGC-7** (council 2026-07-05) | reconcile with one line still `unreconciled`, closing == ledger (diff 0) | `balanced` (**not** `closed`), **no** `BankReconciliationClosed`, `unreconciled_count=1` (persisted); after clearing the line, re-reconcile → `closed` + event + count 0. Fails against a two-state close (a session that lies). |

## Integrity probes (`tests/integrity_probes.rs`)

| Case | Input | Expected |
|------|-------|----------|
| **IP-1** | clear 600,000 against a 500,000 line | `over_allocated`; the line's `allocated_amount` is untouched. |
| **IP-2** | GL sink rejects the clearing post | `GlRejected`; **no** clearance row, **no** allocation written. |
| **IP-3** | clear a non-IDR (USD) line | `unsupported_currency`; no mis-valued clearing reaches the ledger. |

## Clearing seam — payment ↔ banking ↔ accounting (`tests/clearing_seam.rs` + `scripts/clearing_seam_roundtrip.sh`)

| Case | Input | Expected |
|------|-------|----------|
| **CLSEAM-1** | payment settles 750,000 to the **clearing** account (`Dr Clearing · Cr A/R`); banking imports the statement, matches the deposit to the payment, clears (`Dr Bank · Cr Clearing`) | both journals balance; **clearing account nets to `0`** (funds no longer undeposited); bank GL holds `750,000`; A/R stays settled (`−750,000` net); `BankTransactionCleared` carries the matched payment. Zero normal Cargo edge. |
| **CLSEAM-2** (council 2026-07-05) | one payment 500,000; a duplicate/re-imported statement shows the SAME deposit on two lines, both matching it; clear both | first clear fine; **second refused (`settlement_over_cleared`)** — clearing stays at `0` (not `−500,000`), bank GL not overstated (`500,000`, not `1,000,000`). Fails without the settlement bound. |
| **CLSEAM-3** (council 2026-07-05) | one payment 750,000 legitimately lands as TWO deposits (500,000 + 250,000); clear both against it | both succeed; clearing nets to `0`; bank GL holds `750,000` — the amount bound permits legitimate splits (a unique constraint would reject the second). |
| **§5 round-trip** | regen BOTH banking + payment, re-run | all seam ACL/consumer files byte-identical; CLSEAM-1/2/3 still green — survives regen of both modules. |

## Conventions
- Banking posts the **bank-side clearing leg only** — never the original A/R/A/P settlement (payment's).
- Matching is **candidate-supplied**; banking reads no payment/billing tables (zero normal edge).
- Import validates balance continuity; clearing is bounded per line + balanced-or-refuse; IDR-only for now.
- A bank charge is an **expense**, never a tax line.
