-- Revert the ADR-0014 strict fence re-statement for banking module.
-- The fence predates this migration (ADR-0008-era), so the honest reverse is to
-- re-state the same live policy, not to disarm the tables: a down that disabled RLS
-- would leave company data unfenced — a posture this module never had.

-- Re-state the pre-existing fence for banking.bank_accounts (identical policy; see header).
DROP POLICY IF EXISTS bank_accounts_company_isolation ON banking.bank_accounts;
CREATE POLICY bank_accounts_company_isolation ON banking.bank_accounts
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.bank_clearances (identical policy; see header).
DROP POLICY IF EXISTS bank_clearances_company_isolation ON banking.bank_clearances;
CREATE POLICY bank_clearances_company_isolation ON banking.bank_clearances
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.bank_reconciliations (identical policy; see header).
DROP POLICY IF EXISTS bank_reconciliations_company_isolation ON banking.bank_reconciliations;
CREATE POLICY bank_reconciliations_company_isolation ON banking.bank_reconciliations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.bank_statement_imports (identical policy; see header).
DROP POLICY IF EXISTS bank_statement_imports_company_isolation ON banking.bank_statement_imports;
CREATE POLICY bank_statement_imports_company_isolation ON banking.bank_statement_imports
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.bank_transactions (identical policy; see header).
DROP POLICY IF EXISTS bank_transactions_company_isolation ON banking.bank_transactions;
CREATE POLICY bank_transactions_company_isolation ON banking.bank_transactions
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.banks (identical policy; see header).
DROP POLICY IF EXISTS banks_company_isolation ON banking.banks;
CREATE POLICY banks_company_isolation ON banking.banks
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.currencies (identical policy; see header).
DROP POLICY IF EXISTS currencies_company_isolation ON banking.currencies;
CREATE POLICY currencies_company_isolation ON banking.currencies
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.exchange_rates (identical policy; see header).
DROP POLICY IF EXISTS exchange_rates_company_isolation ON banking.exchange_rates;
CREATE POLICY exchange_rates_company_isolation ON banking.exchange_rates
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for banking.fx_gain_losses (identical policy; see header).
DROP POLICY IF EXISTS fx_gain_losses_company_isolation ON banking.fx_gain_losses;
CREATE POLICY fx_gain_losses_company_isolation ON banking.fx_gain_losses
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

