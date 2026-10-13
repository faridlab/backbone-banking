-- Down: drop the declared foreign keys for banking module
ALTER TABLE IF EXISTS banking.bank_transactions DROP CONSTRAINT IF EXISTS fk_bank_transactions_import_id;
ALTER TABLE IF EXISTS banking.bank_statement_imports DROP CONSTRAINT IF EXISTS fk_bank_statement_imports_bank_account_id;
ALTER TABLE IF EXISTS banking.bank_clearances DROP CONSTRAINT IF EXISTS fk_bank_clearances_bank_transaction_id;
ALTER TABLE IF EXISTS banking.bank_accounts DROP CONSTRAINT IF EXISTS fk_bank_accounts_bank_id;
