<!-- Reader: App developer · Mode: Tutorial → How-to -->
# Developer Guide

Integrate `backbone-banking` into a service and drive the cash loop: import a statement, match a
line to a payment, clear it through the GL, and reconcile a period. The tutorial part holds your
hand once; the recipes assume you know your way around.

Commands here were run against `metaphor 0.2.0`. Where the top-level [README](../../README.md)
shows a `backbone-schema`/`backbone` command, use the `metaphor` form below — those are the ones
that work today.

## Prerequisites

- **Rust** (2021 edition toolchain) and **Cargo**.
- The **`metaphor`** CLI on your `PATH` (`metaphor --version` → `metaphor 0.2.0` or newer).
- A reachable **PostgreSQL** instance.
- For clearing/reconciliation: a **`GlPostSink`** implementation (the accounting module, or a test
  double) and a source of **match candidates** (payment/billing). Banking never reads those tables —
  the composition supplies them.

## Quickstart — prove the toolchain end to end

```bash
# From the module directory:
export DATABASE_URL="postgresql://root:password@localhost:5432/skeletondb"

# 1. Validate the schema.
metaphor schema schema validate banking

# 2. Apply the migrations (enums + the six banking tables + audit triggers).
metaphor migration run

# 3. Run the module's tests — including the golden-case oracle.
metaphor dev test
```

Expected: validation passes, migrations report the `banking.*` tables created, and the test run is
green — including `banking_golden_cases`, `integrity_probes`, and `clearing_seam`.

## Mount it in a service

`backbone-banking` is a library. A `backend-service` builds the module and mounts a router. Prefer
the **guarded** router for anything real:

```rust
use backbone_auth::tenant::TenantVerifier;
use backbone_banking::BankingModule;
use backbone_banking::presentation::http::create_guarded_banking_routes;

let banking = BankingModule::builder()
    .with_database(pool.clone())
    .build()?;

// RECOMMENDED: read routes + validated statement import. Generic mutation is NOT mounted.
// The import requires a signed Bearer token and takes its `company_id` from the token's claims —
// build one verifier from the service's JWT secret and hand it to the composer.
let verifier = TenantVerifier::hs256(jwt_secret.as_bytes());
let router = create_guarded_banking_routes(&banking, pool.clone(), verifier);

// Trusted/admin/seeding ONLY — all 12 CRUD endpoints per entity, no domain validation:
// let router = banking.all_crud_routes();
```

The guarded surface gives you:

| Route | What it does |
|-------|--------------|
| `GET /api/v1/banks`, `/bank_accounts`, `/bank_statement_imports`, `/bank_transactions`, `/bank_reconciliations` | read documents |
| `POST /bank-statements/import` | validated import — checks balance continuity before writing |

Matching, clearing, and reconciliation are **service/job-driven**, not HTTP routes — they need a
`GlPostSink` and supplied candidates. You call `BankingWriteService` from a composition layer or a
job.

## The cash loop — drive it

### 1. Import a statement (validated)

`import_statement` checks balance continuity (`opening + Σdeposit − Σwithdrawal = closing`) and
refuses a statement whose lines don't add up.

```bash
curl -s -X POST localhost:8080/bank-statements/import \
  -H 'content-type: application/json' \
  -d '{
    "companyId": "…", "bankAccountId": "…",
    "periodStart": "2026-06-01", "periodEnd": "2026-06-30",
    "openingBalance": "1000000.00", "closingBalance": "1300000.00",
    "lines": [
      {"txnDate": "2026-06-05", "referenceNo": "VA-12345", "deposit": "500000.00"},
      {"txnDate": "2026-06-20", "withdrawal": "200000.00"}
    ]
  }'
# → 201 { "id": "…" }        (status: imported, 2 lines, BankStatementImported emitted)
```

A wrong `closingBalance` returns `422 { "error": "balance_mismatch", … }`; an empty `lines` returns
`empty_statement`. (Golden case BGC-1.)

### 2. Match a line, then clear it (service-driven)

```rust
use backbone_banking::application::service::banking_write_service::*;

let svc = BankingWriteService::new(pool.clone());

// Match — candidates come from the composition (payment/billing), never a banking table read.
let winner = svc.propose_match(line_id, &candidates).await?; // exact amount+ref → amount → None

// Clear — assembles ONE balanced post (Dr Bank · Cr Clearing for a deposit) and emits it via the sink.
let outcome = svc.clear_transaction(
    NewClearance { /* bank_transaction_id, matched_source_*, matched_amount, clearance_date */ },
    &sink,          // your GlPostSink
).await?;
```

The clear is bounded twice: it cannot exceed the line's un-allocated remainder (`over_allocated`) nor
the settled document's amount (`settlement_over_cleared`). A rejected GL post writes **nothing** —
no clearance, no allocation. (Golden cases BGC-3/4, IP-1/2/3.)

### 3. Reconcile a period

```rust
let recon = svc.reconcile(NewReconciliation {
    /* company_id, bank_account_id, from_date, to_date, statement_closing_balance, ledger_balance */
}).await?;
// recon.status: open (numbers disagree) | balanced (agree, open lines remain) | closed (agree + zero open)
```

`closed` — and only `closed` — emits `BankReconciliationClosed`. A diff-zero session with an open
line stays `balanced` and emits nothing. (Golden case BGC-7.)

## Key concepts

Five ideas carry you the rest of the way. One line each; the linked page explains *why*.

- **Schema YAML is the source of truth.** You edit [`schema/models/*.model.yaml`](../schema/RULE_FORMAT_MODELS.md);
  the entities, DTOs, migrations, repositories, services, read handlers, and routes are generated.
  ([Philosophy](01-philosophy.md).)
- **Banking is a library, not a service.** No `main.rs`. A `backend-service` composes it via
  `BankingModule::builder().with_database(pool).build()?` and mounts a router. ([Architecture](04-architecture.md).)
- **Prefer the guarded router.** `create_guarded_banking_routes()` mounts reads + validated import;
  generic mutation is off. `all_crud_routes()` is unguarded — admin/seeding only.
- **Banking posts the bank-side leg only, and reads no sibling tables.** Matching is candidate-supplied;
  the GL effect is emitted through a `GlPostSink`. ([ADR-0004](adr/adr-0004-banking-boundary-and-clearing-seam.md).)
- **The write model is hand-written and regen-safe.** `banking_write_service.rs`, `banking_gl.rs`,
  `banking_events.rs`, and `guarded_routes.rs` are `user_owned` — the generator never touches them.
  ([ADR-0003](adr/adr-0003-custom-markers.md).)

## Recipes

### How do I add a second bank / account for a company?

Banks and accounts are ordinary catalogue entities. Create them via `BankingWriteService::create_bank`
/ `create_bank_account`, or (trusted context) the unguarded CRUD. Each `BankAccount` must carry its
two GL refs — `gl_account_id` (real bank account) and `clearing_account_id` (undeposited funds) —
both logical FKs into accounting.

### How do I add a business rule to the write model?

The write model already lives in `user_owned` files, so edit them directly — no CUSTOM markers
needed. Add a bound or branch in
[`banking_write_service.rs`](../../src/application/service/banking_write_service.rs), and cover it
with a golden case in `tests/banking_golden_cases.rs` or `tests/integrity_probes.rs`. Keep money
exact (`Decimal`, 2dp) and keep every clearing post balanced-or-refuse.

### How do I add a non-CRUD HTTP endpoint?

Compose it in [`guarded_routes.rs`](../../src/presentation/http/guarded_routes.rs) (already
`user_owned`), beside the existing `write_routes`. Don't touch the generated per-entity handlers.
See [Maintainer Guide → Adding a non-CRUD endpoint](05-maintainer-guide.md#adding-a-non-crud-endpoint).

### How do I reference a payment / GL account / user?

By **logical foreign key**, declared in the schema — never by copying the table in. Banking already
does this for the two GL refs, the matched-source id, and audit actors:

```yaml
# schema/models/index.model.yaml
external_imports:
  - module: sapiens
    types: [User]
created_by:
  type: uuid?
  attributes: ["@foreign_key(sapiens.User.id)"]
```

`gl_account_id`, `clearing_account_id`, and `matched_source_id` use `@exclude_from_foreign_key_check`
— logical refs with no DB constraint, so banking stays independently deployable.

### How do I seed sample data?

```bash
metaphor migration seed banking          # run the Rust seeders in src/seeders/
metaphor migration generate-seeds banking  # emit SQL seed files
```

## Configuration

Defaults live in [`config/application.yml`](../../config/application.yml); override per environment
and at runtime.

| Option | Default | When to change |
|--------|---------|----------------|
| `server.host` | `0.0.0.0` | Bind to a specific interface. |
| `server.port` | `8080` | Port conflicts / multi-service hosts. |
| `server.grpc_port` | `50051` | gRPC bind port (gRPC generators are disabled in this module's schema, but the service-level config carries the field). |
| `database.url` | `postgresql://root:password@localhost:5432/skeletondb` | **Always** in real deployments — override with the `DATABASE_URL` env var, which takes precedence. |
| `database.max_connections` | `10` | Tune to your Postgres pool budget. |
| `logging.level` | `info` | `debug`/`trace` when diagnosing; `warn` in noisy prod. |

`DATABASE_URL` in the environment always wins over the YAML.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `backbone-schema: command not found` | Following the stale README | Use `metaphor schema schema …`. `backbone-schema` is not a separate binary here. |
| `metaphor migration run` can't connect | `DATABASE_URL` unset or Postgres down | `export DATABASE_URL=postgresql://…`; confirm Postgres is reachable. |
| Import returns `422 balance_mismatch` | `openingBalance + Σdeposit − Σwithdrawal ≠ closingBalance` | Fix the statement numbers; the importer refuses a statement that doesn't reconcile. |
| Import returns `422 empty_statement` | No `lines` in the payload | A statement must have at least one line. |
| Clear returns `422 over_allocated` | `matched_amount` exceeds the line's un-allocated remainder | Clear at most the remainder; split across documents if needed. |
| Clear returns `422 settlement_over_cleared` | The same settlement is being cleared beyond its amount (double-match / re-import) | Expected — the settlement bound is protecting the GL. Don't re-clear an already-settled document. |
| Clear returns `422 unsupported_currency` | A non-IDR line | Multi-currency clearing is a non-goal today; IDR only. |
| Reconcile says `balanced`, not `closed` | Numbers agree but open lines remain | Clear the remaining `unreconciled`/`partly_reconciled` lines, then re-reconcile. This is correct, not a bug. |
| A custom change vanished after regen | Code sat outside a `user_owned` path or CUSTOM marker | Put write-model logic in the `user_owned` files ([Maintainer Guide](05-maintainer-guide.md#regen-safety--the-rules-that-keep-your-logic-alive)). |
| JSON field names look wrong (`created_at` vs `createdAt`) | Expecting snake_case on the wire | DTOs are `camelCase` by design; snake_case is DB/Rust only. |

---

Next: [Contributing](07-contributing.md) to send a change back, or the
[Glossary](08-glossary.md) to pin down a term.
