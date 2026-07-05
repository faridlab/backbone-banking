-- Down: drop banking.bank_transactions table
DROP TABLE IF EXISTS banking.bank_transactions CASCADE;
DROP FUNCTION IF EXISTS banking.bank_transactions_audit_timestamp() CASCADE;
