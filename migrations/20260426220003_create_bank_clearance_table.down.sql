-- Down: drop banking.bank_clearances table
DROP TABLE IF EXISTS banking.bank_clearances CASCADE;
DROP FUNCTION IF EXISTS banking.bank_clearances_audit_timestamp() CASCADE;
