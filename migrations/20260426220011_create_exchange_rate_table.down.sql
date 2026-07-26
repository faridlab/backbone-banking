-- Down: drop banking.exchange_rates table
DROP TABLE IF EXISTS banking.exchange_rates CASCADE;
DROP FUNCTION IF EXISTS banking.exchange_rates_audit_timestamp() CASCADE;
