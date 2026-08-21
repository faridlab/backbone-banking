-- Down: drop banking.reconcile_presets table
DROP TABLE IF EXISTS banking.reconcile_presets CASCADE;
DROP FUNCTION IF EXISTS banking.reconcile_presets_audit_timestamp() CASCADE;
