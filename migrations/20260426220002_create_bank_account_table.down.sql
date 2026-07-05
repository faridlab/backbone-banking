-- Down: drop banking.bank_accounts table
DROP TABLE IF EXISTS banking.bank_accounts CASCADE;
DROP FUNCTION IF EXISTS banking.bank_accounts_audit_timestamp() CASCADE;
