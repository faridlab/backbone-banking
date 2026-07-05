-- Down: drop banking.bank_statement_imports table
DROP TABLE IF EXISTS banking.bank_statement_imports CASCADE;
DROP FUNCTION IF EXISTS banking.bank_statement_imports_audit_timestamp() CASCADE;
