-- Down: drop the preset link column and the preset table's fence. The table
-- itself (and its enums) belongs to the create-table migration's own down.

ALTER TABLE banking.bank_reconciliations DROP COLUMN IF EXISTS preset_id;

DROP POLICY IF EXISTS reconcile_presets_company_isolation ON banking.reconcile_presets;
ALTER TABLE banking.reconcile_presets FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.reconcile_presets NO FORCE ROW LEVEL SECURITY;
ALTER TABLE banking.reconcile_presets DISABLE ROW LEVEL SECURITY;
