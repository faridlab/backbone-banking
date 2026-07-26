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

-- Reverse the company RLS fence for banking.currencies
DROP POLICY IF EXISTS currencies_company_isolation ON banking.currencies;
ALTER TABLE banking.currencies NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.currencies DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for banking.exchange_rates
DROP POLICY IF EXISTS exchange_rates_company_isolation ON banking.exchange_rates;
ALTER TABLE banking.exchange_rates NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.exchange_rates DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for banking.fx_gain_losses
DROP POLICY IF EXISTS fx_gain_losses_company_isolation ON banking.fx_gain_losses;
ALTER TABLE banking.fx_gain_losses NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.fx_gain_losses DISABLE ROW LEVEL SECURITY;

