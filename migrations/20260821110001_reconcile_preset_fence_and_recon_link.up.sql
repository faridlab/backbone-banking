-- Strict company fence for banking.reconcile_presets (ADR-0014 posture) and the
-- reconciliation-session link to the preset that ordered its matches.
--
-- A reconcile preset is STRICTLY tenant-owned: a match rule that crossed
-- companies would rank one company's invoices against another's statement
-- lines. Unlike billing's global payment terms (split fence), there is no
-- NULL-company global row and no admin carve-out — the WITH CHECK forbids
-- writing one, not merely reading past it.

ALTER TABLE banking.reconcile_presets ENABLE ROW LEVEL SECURITY;
ALTER TABLE banking.reconcile_presets FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS reconcile_presets_company_isolation ON banking.reconcile_presets;
CREATE POLICY reconcile_presets_company_isolation ON banking.reconcile_presets
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Which preset governed a reconciliation session's candidate ordering.
-- Nullable + logical: sessions predating presets (and manual-only sessions)
-- carry NULL; history is never rewritten.
ALTER TABLE banking.bank_reconciliations ADD COLUMN IF NOT EXISTS preset_id UUID;
CREATE INDEX IF NOT EXISTS idx_bank_reconciliations_preset_id ON banking.bank_reconciliations (preset_id);
