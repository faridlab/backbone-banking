# ADR-002: FX / Multi-currency — banking owns the rate catalogue + realised gain/loss

**Status**: Accepted — Applied 2026-07-26
**Related**: billing ADR-001 (boundary), ADR-002 (the seam pattern), accounting ADR-003 (deferred scope)

## Context

Billing is IDR-only (`unsupported_currency` otherwise). Payment stores `currency` + `exchange_rate`
on PaymentEntry but doesn't compute gain/loss. Accounting's JournalLine already carries `exchange_rate`
+ `base_*_amount` columns (reserved for a future FX layer). The gap: exchange rates, currency
conversion, and the realised FX gain/loss on settlement — all deferred. This ADR fills that gap.

## Decision

1. **Banking owns the currency catalogue, exchange rates, and realised FX gain/loss.** A rate
   without a bank is imaginary — the rate that matters for realisation is the one the bank transacted,
   and that lands on the statement banking already ingests.

2. **Entities:** `Currency` (ISO 4217 catalogue, one `is_base` per company), `ExchangeRate` (immutable,
   dated, typed: spot / avg_period / period_end), `FxGainLoss` (realised delta on clearance).

3. **ExchangeRateProvider port** — banking implements it; billing/payment call it via the composition
   ACL to resolve spot rates. Zero cargo edges (`cargo tree -e normal -i backbone-banking` empty in
   the shipped billing/payment crates).

4. **Realised FX = `foreign_amount × (realised_rate − original_rate)`.** Positive = gain; negative =
   loss. Computed at bank clearance (the moment cash lands at the transacted rate). Period-end
   *unrealised* revaluation stays in accounting (banking only supplies the `period_end` rate).

5. **Immutable rates.** `ExchangeRate` is append-only; a correction is a new row (the unique fence
   on `(company, pair, date, type)` prevents duplicates). Historical rates never change — a posted
   journal that referenced them must not see a different rate tomorrow.

## Consequences

- Proven by `tests/fx_seam.rs` (FXSEAM-1): rate recording → spot lookup → gain (+10,000 IDR) → loss
  (−10,000 IDR) → idempotent re-record.
- Deferred: hedging/forwards, crypto currencies, real-time streaming rates, automated period-end
  revaluation (accounting's job), pegged currencies.
