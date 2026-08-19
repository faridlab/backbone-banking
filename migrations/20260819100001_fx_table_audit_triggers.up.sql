-- Audit timestamp triggers for currencies, exchange_rates, and fx_gain_losses
-- (banking module). These stanzas originally sat in 20260426220009, before their
-- tables existed in the chain (added in place when the FX tables landed in
-- 20260426220010..12) — fresh databases failed there. They run here, after the
-- creates. Idempotent everywhere: CREATE OR REPLACE FUNCTION, DROP TRIGGER IF
-- EXISTS + CREATE TRIGGER.

-- Table: Currency (banking.currencies)
-- ==============================================================================

-- Function to set metadata timestamps
CREATE OR REPLACE FUNCTION banking.currencies_audit_timestamp() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{created_at}', to_jsonb(NOW()));
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    ELSIF TG_OP = 'UPDATE' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to set timestamps on INSERT
DROP TRIGGER IF EXISTS currencies_insert_audit ON banking.currencies;
CREATE TRIGGER currencies_insert_audit BEFORE INSERT ON banking.currencies
    FOR EACH ROW EXECUTE FUNCTION banking.currencies_audit_timestamp();

-- Trigger to set updated_at on UPDATE
DROP TRIGGER IF EXISTS currencies_update_audit ON banking.currencies;
CREATE TRIGGER currencies_update_audit BEFORE UPDATE ON banking.currencies
    FOR EACH ROW EXECUTE FUNCTION banking.currencies_audit_timestamp();

-- ==============================================================================
-- Table: ExchangeRate (banking.exchange_rates)
-- ==============================================================================

-- Function to set metadata timestamps
CREATE OR REPLACE FUNCTION banking.exchange_rates_audit_timestamp() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{created_at}', to_jsonb(NOW()));
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    ELSIF TG_OP = 'UPDATE' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to set timestamps on INSERT
DROP TRIGGER IF EXISTS exchange_rates_insert_audit ON banking.exchange_rates;
CREATE TRIGGER exchange_rates_insert_audit BEFORE INSERT ON banking.exchange_rates
    FOR EACH ROW EXECUTE FUNCTION banking.exchange_rates_audit_timestamp();

-- Trigger to set updated_at on UPDATE
DROP TRIGGER IF EXISTS exchange_rates_update_audit ON banking.exchange_rates;
CREATE TRIGGER exchange_rates_update_audit BEFORE UPDATE ON banking.exchange_rates
    FOR EACH ROW EXECUTE FUNCTION banking.exchange_rates_audit_timestamp();

-- ==============================================================================
-- Table: FxGainLoss (banking.fx_gain_losses)
-- ==============================================================================

-- Function to set metadata timestamps
CREATE OR REPLACE FUNCTION banking.fx_gain_losses_audit_timestamp() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{created_at}', to_jsonb(NOW()));
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    ELSIF TG_OP = 'UPDATE' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to set timestamps on INSERT
DROP TRIGGER IF EXISTS fx_gain_losses_insert_audit ON banking.fx_gain_losses;
CREATE TRIGGER fx_gain_losses_insert_audit BEFORE INSERT ON banking.fx_gain_losses
    FOR EACH ROW EXECUTE FUNCTION banking.fx_gain_losses_audit_timestamp();

-- Trigger to set updated_at on UPDATE
DROP TRIGGER IF EXISTS fx_gain_losses_update_audit ON banking.fx_gain_losses;
CREATE TRIGGER fx_gain_losses_update_audit BEFORE UPDATE ON banking.fx_gain_losses
    FOR EACH ROW EXECUTE FUNCTION banking.fx_gain_losses_audit_timestamp();

