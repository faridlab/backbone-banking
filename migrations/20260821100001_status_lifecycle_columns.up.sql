-- Migration: replace lifecycle booleans with status enums
-- banks, bank_accounts, currencies each carried `is_active BOOLEAN NOT NULL
-- DEFAULT TRUE`. The tree-wide convention is one `status` enum field per
-- lifecycle (see docs/refactoring-schema in the serpa workspace): the boolean
-- migrates only rows deviating from its own column default — everything else
-- keeps the 'active' the new column arrives with. The two
-- (company_id, is_active) indexes become (company_id, status) indexes in the
-- same migration. Fence posture is unchanged: same tables, same WITH CHECK.

DO $$ BEGIN
    CREATE TYPE banking.bank_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE banking.bank_account_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE banking.currency_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

ALTER TABLE banking.banks ADD COLUMN status banking.bank_status NOT NULL DEFAULT 'active';
UPDATE banking.banks SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE banking.banks DROP COLUMN is_active;
DROP INDEX IF EXISTS banking.idx_banks_company_id_is_active;
CREATE INDEX idx_banks_company_id_status ON banking.banks (company_id, status);

ALTER TABLE banking.bank_accounts ADD COLUMN status banking.bank_account_status NOT NULL DEFAULT 'active';
UPDATE banking.bank_accounts SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE banking.bank_accounts DROP COLUMN is_active;
DROP INDEX IF EXISTS banking.idx_bank_accounts_company_id_is_active;
CREATE INDEX idx_bank_accounts_company_id_status ON banking.bank_accounts (company_id, status);

ALTER TABLE banking.currencies ADD COLUMN status banking.currency_status NOT NULL DEFAULT 'active';
UPDATE banking.currencies SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE banking.currencies DROP COLUMN is_active;
