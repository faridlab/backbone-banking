-- Down: drop banking.currencies table
DROP TABLE IF EXISTS banking.currencies CASCADE;
DROP FUNCTION IF EXISTS banking.currencies_audit_timestamp() CASCADE;
