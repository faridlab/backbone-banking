# FSD — backbone-banking

> Functional Spec. Tier 2 · Financials. Date: 2026-07-05.

## Entities (schema/models/*.model.yaml — SSoT)
Bank · BankAccount (`gl_account_id` + `clearing_account_id`, `account_type`) · BankStatementImport
(+lines) · BankTransaction (deposit/withdrawal, `status`, `allocated_amount`) · BankClearance
(`matched_source_type`/`matched_source_id`, `matched_amount`, `accounting_post_id`) ·
BankReconciliation (`computed_difference`, `status`). Cross-module ids are logical FKs
(`@exclude_from_foreign_key_check`): account→accounting, `matched_source_id`→payment/billing,
company/branch→organization.

## Services (application/service — hand-authored, user_owned)
- `BankingWriteService` — `create_bank` / `create_bank_account`; `import_statement` (balance
  continuity + persist lines + `BankStatementImported`); `propose_match` (candidate selection, pure);
  `clear_transaction` (bounded → build ONE balanced clearing `AccountingPostEnvelope` → emit through a
  `GlPostSink` → record `BankClearance` + advance the line + `BankTransactionMatched`/`Cleared`);
  `recognize_bank_charge` (`Dr Bank Charges · Cr Bank`); `reconcile` (difference + close).
- `banking_gl` — the outbound GL port: `GlPostLine`, `AccountingPostEnvelope`, `GlPostAck`,
  `GlPostRejected`, `GlPostSink`. The wire contract; zero normal edge.
- `banking_events` — `BankingEvent` {`BankStatementImported`, `BankTransactionMatched`,
  `BankTransactionCleared`, `BankChargeRecognized`, `BankReconciliationClosed`} + `BankingEventSink`.

## HTTP surface (presentation/http/guarded_routes.rs)
`create_guarded_banking_routes(&BankingModule, pool, TenantVerifier)` — read documents + validated
`POST /bank-statements/import` (balance continuity checked). The import is tenant-guarded: it requires
a signed Bearer token and derives `company_id` from the token's claims, never from the request body.
No generic mutation. Matching / clearing / reconciliation need a `GlPostSink` + supplied candidates,
so they are service/job-driven.

## State machines
- Import (`ImportStatus`): `draft → imported → reconciling → completed` / `failed`.
- Line (`TxnStatus`): `unreconciled → partly_reconciled → reconciled` / `ignored`.
- Session (`ReconStatus`): `open` (numbers disagree) → `balanced` (agree, open lines remain) →
  `closed` (agree + zero open lines; the only state that emits `BankReconciliationClosed`).

## Integration seams
- **Clearing seam (proven, marquee):** a payment settles to a **clearing** account (`Dr Clearing · Cr
  A/R`); banking imports the statement, matches the line to that payment (candidate supplied), and
  clears (`Dr Bank · Cr Clearing`) into the real ledger — the clearing account **nets to zero**, the
  bank GL holds the cash, A/R stays settled. Zero normal Cargo edge. ADR-001,
  `tests/clearing_seam.rs`, `scripts/clearing_seam_roundtrip.sh`.
- **Outbound:** `BankTransactionCleared` → payment (mark a settlement bank-confirmed) / cash-flow
  dashboards. **Inbound (future):** live bank API, host-to-host payment-order files.

## Test oracle
`banking_golden_cases` (7: import continuity, match preference, clear deposit/withdrawal + partial,
bank charge, reconcile close, **BGC-7 reconcile can't close over open lines** — council 2026-07-05),
`integrity_probes` (3: over-clearing refused, rejected-post writes nothing, non-IDR refused),
`clearing_seam` (3, real ledger — CLSEAM-1 clearing nets to zero; CLSEAM-2 a settlement can't be
cleared twice; CLSEAM-3 one settlement splits across two lines; + §5). **13 tests** (of the
hand-authored suite).
