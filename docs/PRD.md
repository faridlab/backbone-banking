# PRD — backbone-banking

> Tier 2 · Financials · Indonesia-first ERP. Status: built. Date: 2026-07-05.

## Problem & intent
The ledger says money moved; the **bank** says what actually landed. `backbone-banking` is the
reconciliation truth-source between the two: it owns the bank/account catalogue, ingests statements,
matches lines to settlements recorded by `backbone-payment`/`backbone-billing`, and **clears** them
through the GL — posting the bank-side leg payment deliberately left open. It answers "does the bank
agree with our books?" and closes the cash position. It is the **5th GL producer** (clearing posts).

## Goals
- Own **Bank** + **BankAccount** (each carrying a real bank GL account **and** a clearing account),
  **BankStatementImport** + **BankTransaction** (statement lines), **BankClearance** (match links),
  **BankReconciliation** (sessions).
- **Import** statements with **balance-continuity** validation (`opening + Σdeposit − Σwithdrawal =
  closing`); guarded surface (no generic mutation of a statement).
- **Match** a line to a settled document from supplied candidates (exact amount + reference → amount).
  Banking never reads payment/billing tables — candidates are supplied by the composition layer.
- **Clear** through the GL: received `Dr Bank · Cr Clearing`; paid `Dr Clearing · Cr Bank`; the
  clearing account nets to zero once a payment's settlement is bank-confirmed. Bounded per line.
- **Bank charges** (`Dr Bank Charges · Cr Bank`) and a **reconciliation session** that closes when the
  statement closing balance equals the ledger balance (difference 0).

## Non-goals (this phase / deferred)
`bank_guarantee`, `invoice_discounting`, host-to-host payment-order file generation, live bank API
polling (start with file/manual import), automated rule-learning matcher, multi-currency/FX clearing,
tax lines (a bank charge is an expense, never a tax line).

## Personas
Treasury/finance clerk (imports statements, reconciles, closes the month), Integrating engineer
(supplies match candidates from payment, subscribes to `BankTransactionCleared`), Auditor (bank ↔ GL
reconciliation as the control).

## Success criteria
- Import continuity + match + clear + charge + reconcile locked by a numeric oracle (6 golden cases) +
  integrity probes (3, incl. over-clearing refused + rejected-post writes nothing).
- The clearing seam proven end-to-end against the real ledger (CLSEAM-1: **clearing account nets to
  zero**, bank GL holds the cash, A/R stays settled) + survives regen of both modules (§5).
- Indonesia-ready: Virtual Account account type + VA-reference matching; per-bank CSV mapping and the
  local bank seed (BCA/Mandiri/BRI/BNI) layer via the `id` overlay, not the base.
