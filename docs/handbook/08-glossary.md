<!-- Reader: All · Mode: Reference -->
# Glossary — ubiquitous language

One term, one meaning, used everywhere in this handbook and in the code. When a term here names a
type or file, that name is exact. If you find a doc using a different word for one of these, the doc
is the bug.

### Aggregate / Entity
A domain object with identity and a lifecycle, defined by one `schema/models/<name>.model.yaml`.
Banking owns six: `Bank`, `BankAccount`, `BankStatementImport`, `BankTransaction`, `BankClearance`,
`BankReconciliation`. Each is generated into `src/domain/entity/<name>.rs` with a strongly-typed id,
a builder, `apply_patch`, and audit accessors.

### Application layer
The use-case layer (`src/application/`): services and DTOs. Depends on the domain; knows nothing
about HTTP or SQL.

### Audit metadata
The `metadata` JSONB field (`created_at`, `updated_at`, `deleted_at`, `created_by`, `updated_by`,
`deleted_by`) added when `config.audit: true`. Timestamps are set by a Postgres trigger; the `*_by`
actor fields are logical FKs to `sapiens.User.id`.

### `BackboneCrudHandler`
The `backbone-core` type that produces an Axum `Router` with all **twelve** CRUD endpoints for an
entity. Invoked as `BackboneCrudHandler::<…>::routes(service, "/collection")`. You never hand-write
these routes.

### Bounded context
The single business domain a module owns. One module = one bounded context. A module never edits
another's schema; it references other modules by logical FK.

### Composition root
`src/module.rs` — the `Module` struct and `ModuleBuilder`. Wires each service to its repository and
composes the routers. The one place that is allowed to depend on every layer.

### CUSTOM marker
A `// <<< CUSTOM … // END CUSTOM` region inside a generated file. Content between the markers
survives regeneration. Spelling varies per file (`// <<< CUSTOM METHODS START >>>`, `// <<< CUSTOM
DTOs`, …) — match what is already there.

### DTO (Data Transfer Object)
A wire-shape struct in `src/application/dto/`. Per entity: `Create…Dto`, `Update…Dto`, `Patch…Dto`,
`…ResponseDto`, `…SummaryDto`, `…ListResponseDto`. Serialized `camelCase`. Generated, with
`From`/`Apply` conversions to and from the entity.

### Domain layer
The innermost layer (`src/domain/`): entities, value objects, enums, invariants, and repository
**traits** (ports). Depends on nothing.

### Generation targets
The 31 kinds of artifact `metaphor schema schema generate` can emit (`rust`, `sql`, `dto`,
`handler`, `repository`, `service`, `proto`, `openapi`, …). `--target all` (default) emits the lot;
a comma-separated subset emits part.

### `GenericCrudRepository` / `GenericCrudService`
The `backbone-orm` / `backbone-core` generics that carry all standard CRUD. A module's repository is
a **newtype** over `GenericCrudRepository<Entity, SoftDelete>`; its service is a **type alias** over
`GenericCrudService<Entity, CreateDto, UpdateDto, Repository>`. Inherited, never re-implemented.

### Infrastructure layer
The adapter layer (`src/infrastructure/`): repository implementations, cache, messaging, jobs.
Depends on domain and application.

### Logical foreign key
A cross-module reference declared with `@foreign_key(module.Type.field)` (e.g.
`@foreign_key(sapiens.User.id)`). It documents the relationship and is *not* enforced by a database
constraint, so modules stay independently deployable.

### `metaphor`
The workspace CLI (v0.2.0) that orchestrates the projects and dispatches to plugins
(`metaphor-schema`, `metaphor-codegen`, `metaphor-dev`). Prefer it over raw `cargo`/`sqlx`. Note:
the standalone `backbone-schema` binary the README mentions is **not** installed; use `metaphor
schema schema …`.

### Module
A **library crate** owning one bounded context in 4-layer DDD, schema-driven. `[lib]` only — no
`main.rs`. Composed into a `backend-service`; never run alone. This repo is the **`banking`** module — the
cash & bank reconciliation bounded context.

### Own schema (per module)
Each module gets its own Postgres schema (`schema: banking` in `index.model.yaml`). Migrations
`CREATE SCHEMA <module>` and qualify tables as `<module>.<table>`, so modules never collide on a
table name.

### Port / Adapter
The DDD names for the two repositories per entity (e.g. the two `BankAccountRepository`s): the
**port** is the domain-layer `trait` (the contract); the **adapter** is the infrastructure-layer
`struct` newtype (the Postgres implementation). `GlPostSink` is a port too — the outbound port
banking emits its clearing post through, implemented by the composition (accounting).

### Presentation layer
The transport layer (`src/presentation/`, `src/routes/`): HTTP handlers, route composition, and
optionally gRPC/GraphQL. Depends on the application layer.

### Regeneration (regen)
Re-running `metaphor schema schema generate … --force` to rebuild all downstream code from the
schema. Overwrites everything **outside** a protected region (CUSTOM markers, `*_custom.rs`,
`user_owned` globs).

### Schema (the SSoT)
`schema/models/*.model.yaml` — the single source of truth. Every entity struct, DTO, migration,
repository, service, handler, and route is generated from it. Not to be confused with the *Postgres
schema* (the per-module namespace).

### Soft delete
Marking a row deleted (`metadata.deleted_at` set) instead of removing it, enabled by
`config.soft_delete: true`. Backs the `soft_delete` / `restore` / `empty_trash` / `list_deleted`
endpoints.

### Twelve endpoints
The standard CRUD surface every entity gets from `BackboneCrudHandler`: `list`, `create`, `get`,
`update`, `patch`, `soft_delete`, `restore`, `empty_trash`, `bulk_create`, `upsert`, `find_by_id`,
`list_deleted`.

### `user_owned`
The `metaphor.codegen.yaml` key listing glob paths the generator skips wholesale — never reads,
merges, or deletes. In banking it protects the write-model core (`banking_write_service.rs`,
`banking_gl.rs`, `banking_events.rs`, `guarded_routes.rs`), the test oracles
(`banking_golden_cases.rs`, `integrity_probes.rs`, `clearing_seam.rs`), `scripts/**`,
`tests/features/**`, and `docs/**` (this handbook lives under one of them).

---

## Banking domain terms

The ubiquitous language of the cash & bank reconciliation context. Sourced from
[ADR-0004](adr/adr-0004-banking-boundary-and-clearing-seam.md) and the
[golden cases](../business-flows/golden-cases.md); used verbatim in the schema and the write model.

### Bank account (real) vs. clearing account
Every `BankAccount` carries **two** GL refs. `gl_account_id` is the **real bank** GL account — cash
actually in the bank. `clearing_account_id` is the **clearing / undeposited-funds** account — money
a payment has settled to but that the statement has not yet confirmed. Clearing a line moves value
`clearing → bank`.

### Balance continuity
The invariant an import must satisfy: `opening_balance + Σdeposit − Σwithdrawal = closing_balance`.
`import_statement` refuses a statement that fails it (`balance_mismatch`).

### Bank clearance
A `BankClearance` — the match of one statement line to the document it settles, plus the emitted GL
post. Records `matched_source_type`/`matched_source_id`, `matched_amount`, and the resulting
`accounting_post_id` / `journal_id`.

### Clearing leg (bank-side leg only)
The single balanced `AccountingPost` banking emits on a clear. **Deposit:** `Dr Bank · Cr Clearing`.
**Withdrawal:** `Dr Clearing · Cr Bank`. **Bank charge:** `Dr Bank Charges · Cr Bank`. Banking posts
*only* this leg — never the original A/R/A/P settlement, which payment owns.

### Match candidate
A `MatchCandidate` supplied by the composition (from payment/billing) to `propose_match`. Banking
selects `exact amount + reference → exact amount → none`. Banking **reads no sibling table**; the
candidate list *is* the seam.

### `GlPostSink`
The outbound port banking hands its assembled `AccountingPost` to. Implemented by the composition
(accounting). A rejected post (`GlRejected`) rolls back the whole clear — no clearance, no allocation.

### Allocation / `allocated_amount`
How much of a statement line has been cleared so far (`Σ matched_amount`). A line may split across
several documents; `allocated_amount` tracks the running total and drives the line's `TxnStatus`
(`unreconciled` → `partly_reconciled` → `reconciled`).

### The two clearing bounds
A clear cannot exceed **(a)** the line's un-allocated remainder (`over_allocated`) **nor** **(b)** the
settled document's amount (`settlement_over_cleared`, checked under a per-settlement advisory lock).
Bound (b) stops the same payment being cleared twice against two lines (a re-import, a retry).

### Reconciliation session & the three states
A `BankReconciliation` over an account + period. `reconcile` resolves to `open` (statement balance ≠
ledger balance), `balanced` (balances agree but open lines remain — **no** close event), or `closed`
(agree **and** zero open lines → `BankReconciliationClosed`). `unreconciled_count` is the persisted
exception snapshot at sign-off.

### Bank charge
A fee recognised as an **expense** (`Dr Bank Charges · Cr Bank`) via `recognize_bank_charge` — never a
tax line.
