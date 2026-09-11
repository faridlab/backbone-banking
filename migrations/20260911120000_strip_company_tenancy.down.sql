-- Hand-authored (user-owned). Not regenerated.
--
-- Best-effort restore sketch for the tenancy strip (ADR-0029). This is a breaking module
-- release against dev-stage databases: the down re-adds the company_id column as nullable
-- with its plain indexes and the company isolation policy shape, but restores NO data —
-- rows written after the strip (or after the decorator re-keyed them) carry org_unit_id
-- only. The composing service's tenancy decorator remains the live fence; treat this
-- down as a schema-shape sketch for archaeology, not a usable rollback.

ALTER TABLE banking.banks                    ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE banking.bank_accounts            ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE banking.bank_clearances          ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE banking.bank_reconciliations     ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE banking.bank_statement_imports   ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE banking.bank_transactions        ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE banking.fx_gain_losses           ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE banking.reconcile_presets        ADD COLUMN IF NOT EXISTS company_id uuid;

CREATE INDEX IF NOT EXISTS idx_banks_company_id_status
    ON banking.banks (company_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_bank_accounts_company_id_bank_id_account_number
    ON banking.bank_accounts (company_id, bank_id, account_number) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_bank_accounts_company_id_status
    ON banking.bank_accounts (company_id, status);
CREATE INDEX IF NOT EXISTS idx_bank_reconciliations_company_id_bank_account_id_status
    ON banking.bank_reconciliations (company_id, bank_account_id, status);
CREATE INDEX IF NOT EXISTS idx_bank_statement_imports_company_id_bank_account_id_status
    ON banking.bank_statement_imports (company_id, bank_account_id, status);
CREATE INDEX IF NOT EXISTS idx_bank_transactions_company_id_bank_account_id_status
    ON banking.bank_transactions (company_id, bank_account_id, status);
CREATE INDEX IF NOT EXISTS idx_fx_gain_losses_company_id_matched_source_id
    ON banking.fx_gain_losses (company_id, matched_source_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reconcile_presets_company_id_name
    ON banking.reconcile_presets (company_id, name) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_reconcile_presets_company_id_status_priority
    ON banking.reconcile_presets (company_id, status, priority);
