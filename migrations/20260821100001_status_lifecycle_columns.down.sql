-- Down: restore the is_active booleans exactly as they were.
-- Rows whose status is the column default ('active') map back to the boolean
-- default TRUE without an UPDATE; only 'inactive' rows are written back as
-- FALSE. The (company_id, status) indexes revert to (company_id, is_active).

ALTER TABLE banking.currencies ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE banking.currencies SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE banking.currencies DROP COLUMN status;
DROP TYPE IF EXISTS banking.currency_status;

ALTER TABLE banking.bank_accounts ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE banking.bank_accounts SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE banking.bank_accounts DROP COLUMN status;
DROP INDEX IF EXISTS banking.idx_bank_accounts_company_id_status;
CREATE INDEX idx_bank_accounts_company_id_is_active ON banking.bank_accounts (company_id, is_active);
DROP TYPE IF EXISTS banking.bank_account_status;

ALTER TABLE banking.banks ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE banking.banks SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE banking.banks DROP COLUMN status;
DROP INDEX IF EXISTS banking.idx_banks_company_id_status;
CREATE INDEX idx_banks_company_id_is_active ON banking.banks (company_id, is_active);
DROP TYPE IF EXISTS banking.bank_status;
