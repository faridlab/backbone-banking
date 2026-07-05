# Extension Guide — backbone-banking

> Public contract per `docs/erp/extension-contract.md`. Stable path:
> `backbone_banking::application::service::*` (the generated `exports/` tree is unwired scaffolding).

## Public surface
**A. Domain events** (`banking_events`, the 5-variant `BankingEvent`): `BankStatementImported`,
`BankTransactionMatched`, `BankTransactionCleared` {bank_transaction_id, matched_source_type,
matched_source_id, journal_id, post_id, amount}, `BankChargeRecognized`, `BankReconciliationClosed`.

**B. The GL-posting port** (`banking_gl`) — `AccountingPostEnvelope` is the serialized wire contract
into `backbone-accounting`; a consumer implements `GlPostSink` (async `post(&envelope)`) over
accounting's `PostingService`. Banking never imports accounting in the shipped library.

**C. Match candidates** — `clear_transaction` acts on a `MatchCandidate` the composition supplies
(from payment/billing). Banking selects (`propose_match`) but never reads their tables.

## How a consumer extends
1. **Post to the GL** — implement `GlPostSink`, mapping `AccountingPostEnvelope` → accounting's
   `PostingRequest`; pass it to `clear_transaction` / `recognize_bank_charge`. (Reference ACL:
   `tests/clearing_seam.rs`.)
2. **Feed match candidates** — query payment/billing for open settlements and hand banking a
   `Vec<MatchCandidate>`; `propose_match` returns the best; then `clear_transaction`.
3. **React to a clear** — subscribe to `BankTransactionCleared` to mark a payment settlement
   bank-confirmed, or drive a cash-flow dashboard.
4. **Custom matchers** — a fuzzier/rule-learning matcher lives in a `*_custom.rs` (Tier B) inside
   banking; the base stays candidate-driven.
5. Keep logic in `user_owned`/`*_custom.rs` — survives regen (proven by
   `scripts/clearing_seam_roundtrip.sh`).

## Bounded-context split (important)
Banking posts the **bank-side clearing leg only** — never the original A/R/A/P settlement (payment
owns that). It matches *against* payment/billing documents via supplied candidates + emitted events;
it holds no normal Cargo edge to them.

## Not a contract
Generated CRUD events; internal repositories/parsers; `// <<< CUSTOM` blocks (own edits only).

## Deferred surfaces
Live bank API / host-to-host files, rule-learning matcher, bank guarantee / invoice discounting,
multi-currency/FX clearing — additive when built.
