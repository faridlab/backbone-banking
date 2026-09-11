-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the banking tables (ADR-0029): the module is
-- tenant-agnostic; org scoping is installed by the COMPOSING service's tenancy decorator,
-- never by the module. Dropped here, per table: the company-leading indexes, the
-- <table>_company_isolation RLS policy, and the company_id column itself.
--
-- Ordering guard (the decorator must run FIRST on any database with data): the module
-- never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from company_id —
--      or b) it is empty (a fresh database: the earlier chain files created it empty).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping a column
-- that still holds the only tenancy key. The file is re-runnable (every drop is IF EXISTS
-- and the tracker has no checksums), so a failed run retries cleanly after the decorator
-- lands.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those now.
-- The retired FX catalogue tables (currencies, exchange_rates) were dropped whole by an
-- earlier chain file and are outside this strip.
--
-- No unique is re-based tenant-free here: the module's own statements never conflict
-- against the per-unit uniques (the one-live-account-number-per-bank unique and the
-- one-live-preset-name unique), so both arrive org-scoped from the composing service's
-- tenancy decorator.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'banks', 'bank_accounts', 'bank_clearances', 'bank_reconciliations',
        'bank_statement_imports', 'bank_transactions', 'fx_gain_losses', 'reconcile_presets'
    ]
    LOOP
        IF to_regclass(format('banking.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'banking' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM banking.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM banking.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' banking.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

-- ── banks ──────────────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS banking.idx_banks_company_id_status;
DROP POLICY IF EXISTS banks_company_isolation ON banking.banks;
ALTER TABLE banking.banks DROP COLUMN IF EXISTS company_id;

-- ── bank_accounts ──────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS banking.idx_bank_accounts_company_id_bank_id_account_number;
DROP INDEX IF EXISTS banking.idx_bank_accounts_company_id_status;
DROP POLICY IF EXISTS bank_accounts_company_isolation ON banking.bank_accounts;
ALTER TABLE banking.bank_accounts DROP COLUMN IF EXISTS company_id;

-- ── bank_clearances ────────────────────────────────────────────────────────────
DROP POLICY IF EXISTS bank_clearances_company_isolation ON banking.bank_clearances;
ALTER TABLE banking.bank_clearances DROP COLUMN IF EXISTS company_id;

-- ── bank_reconciliations ───────────────────────────────────────────────────────
DROP INDEX IF EXISTS banking.idx_bank_reconciliations_company_id_bank_account_id_status;
DROP POLICY IF EXISTS bank_reconciliations_company_isolation ON banking.bank_reconciliations;
ALTER TABLE banking.bank_reconciliations DROP COLUMN IF EXISTS company_id;

-- ── bank_statement_imports ─────────────────────────────────────────────────────
DROP INDEX IF EXISTS banking.idx_bank_statement_imports_company_id_bank_account_id_status;
DROP POLICY IF EXISTS bank_statement_imports_company_isolation ON banking.bank_statement_imports;
ALTER TABLE banking.bank_statement_imports DROP COLUMN IF EXISTS company_id;

-- ── bank_transactions ──────────────────────────────────────────────────────────
DROP INDEX IF EXISTS banking.idx_bank_transactions_company_id_bank_account_id_status;
DROP POLICY IF EXISTS bank_transactions_company_isolation ON banking.bank_transactions;
ALTER TABLE banking.bank_transactions DROP COLUMN IF EXISTS company_id;

-- ── fx_gain_losses ─────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS banking.idx_fx_gain_losses_company_id_matched_source_id;
DROP POLICY IF EXISTS fx_gain_losses_company_isolation ON banking.fx_gain_losses;
ALTER TABLE banking.fx_gain_losses DROP COLUMN IF EXISTS company_id;

-- ── reconcile_presets ──────────────────────────────────────────────────────────
DROP INDEX IF EXISTS banking.idx_reconcile_presets_company_id_name;
DROP INDEX IF EXISTS banking.idx_reconcile_presets_company_id_status_priority;
DROP POLICY IF EXISTS reconcile_presets_company_isolation ON banking.reconcile_presets;
ALTER TABLE banking.reconcile_presets DROP COLUMN IF EXISTS company_id;
