-- Down: remove the company RLS fence for banking module

-- Reverse the company RLS fence for banking.banks
DROP POLICY IF EXISTS banks_company_isolation ON banking.banks;
ALTER TABLE banking.banks NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.banks DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for banking.bank_accounts
DROP POLICY IF EXISTS bank_accounts_company_isolation ON banking.bank_accounts;
ALTER TABLE banking.bank_accounts NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.bank_accounts DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for banking.bank_clearances
DROP POLICY IF EXISTS bank_clearances_company_isolation ON banking.bank_clearances;
ALTER TABLE banking.bank_clearances NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.bank_clearances DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for banking.bank_reconciliations
DROP POLICY IF EXISTS bank_reconciliations_company_isolation ON banking.bank_reconciliations;
ALTER TABLE banking.bank_reconciliations NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.bank_reconciliations DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for banking.bank_statement_imports
DROP POLICY IF EXISTS bank_statement_imports_company_isolation ON banking.bank_statement_imports;
ALTER TABLE banking.bank_statement_imports NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.bank_statement_imports DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for banking.bank_transactions
DROP POLICY IF EXISTS bank_transactions_company_isolation ON banking.bank_transactions;
ALTER TABLE banking.bank_transactions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.bank_transactions DISABLE ROW LEVEL SECURITY;

