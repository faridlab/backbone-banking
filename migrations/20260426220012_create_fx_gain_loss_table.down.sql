-- Down: drop banking.fx_gain_losses table
DROP TABLE IF EXISTS banking.fx_gain_losses CASCADE;
DROP FUNCTION IF EXISTS banking.fx_gain_losses_audit_timestamp() CASCADE;
