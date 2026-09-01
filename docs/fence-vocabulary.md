# Fence vocabulary: shared-master unique indexes (the COALESCE recipe)

This module's porting fences use a shared vocabulary. This entry records the
**COALESCE(company_id, nil-uuid) unique-index recipe**: what it is, and the exact places
in this module where it applies.

## The recipe

A per-company partial unique index does not dedupe SHARED rows. Postgres treats `NULL` as
distinct in unique indexes, so with a shared-master posture (ADR-0014: `company_id IS
NULL` = one row shared by every company), the plain form lets any number of shared
duplicates through:

```sql
-- WRONG for a shared-capable master: NULL company_id rows never collide.
CREATE UNIQUE INDEX ... ON banking.banks (company_id, swift_bic);

-- RIGHT: fold the shared bucket (NULL company) into its own sentinel tenant, so there is
-- exactly ONE shared row per business key, alongside one per real company.
CREATE UNIQUE INDEX ... ON banking.banks (
    COALESCE(company_id, '00000000-0000-0000-0000-000000000000'::uuid),
    swift_bic
  )
  WHERE swift_bic IS NOT NULL;
```

The sentinel uuid is never a real `organization.companies.id` (all-zero nil), so the
shared bucket cannot collide with a real tenant's. The RLS policy is unaffected — only
the uniqueness domain changes.

## Where it applies in banking

**`banking.banks` is the natural shared master here** — a bank catalogue (BCA, Mandiri,
Chase) describes the world, not one company's books. Today it is company-scoped strict
(ADR-0014) with NO business unique key at all. IF/when the catalogue is promoted to a
shared master (NULL company = the one shared catalogue every company sees), the promotion
migration must add the dedupe key with the recipe — not a plain per-company unique:

| Future index (at the shared-catalogue promotion) | Shape |
|---|---|
| `banks` SWIFT identity | `(COALESCE(company_id, nil-uuid), swift_bic) WHERE swift_bic IS NOT NULL` |
| `banks` name identity (SWIFT-less banks) | `(COALESCE(company_id, nil-uuid), country, name)` — or a reviewed single-key choice |

Recording the recipe here is the point: the easy mistake at that promotion is copying the
adjacent per-company pattern, which would let N shared "Bank Central Asia" rows in.

## Where it deliberately does NOT apply

- **`bank_accounts (company_id, bank_id, account_number)`** — a company's accounts are
  strictly company-PRIVATE books data; there is intentionally no shared bucket, so the
  plain per-company unique (plus the partial deleted_at guard) is already the correct
  shape.
- **`reconcile_presets (company_id, name)`** — reconciliation policy is per-company by
  construction; same reasoning.
- Statement lines, clearances, transactions — per-company transactional rows with no
  cross-tenant business keys.
- The retired FX catalogue (currencies/exchange rates) retired with the FX move; no
  shared master remains there to fence.
