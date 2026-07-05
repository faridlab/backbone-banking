-- Down: drop banking.bank_reconciliations table
DROP TABLE IF EXISTS banking.bank_reconciliations CASCADE;
DROP FUNCTION IF EXISTS banking.bank_reconciliations_audit_timestamp() CASCADE;
