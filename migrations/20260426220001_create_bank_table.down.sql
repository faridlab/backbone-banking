-- Down: drop banking.banks table
DROP TABLE IF EXISTS banking.banks CASCADE;
DROP FUNCTION IF EXISTS banking.banks_audit_timestamp() CASCADE;
