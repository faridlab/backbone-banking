-- Down: restore the FX catalogue tables exactly as they retired
-- (post status-lifecycle flip: currencies.status currency_status, exchange_rates
-- with rate_type + source). RLS + strict company fences restored. The public
-- enum types are recreated guarded (they may survive elsewhere in a co-located
-- database). Data is NOT restored — the up-migration is a first-cycle retirement
-- over empty tables, and any environment that had collected rates under
-- corporate's ownership would re-import from there.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'currency_status') THEN
        CREATE TYPE currency_status AS ENUM ('active', 'inactive');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'rate_type') THEN
        CREATE TYPE rate_type AS ENUM ('spot', 'avg_period', 'period_end');
    END IF;
END
$$;

CREATE SCHEMA IF NOT EXISTS banking;

CREATE TABLE IF NOT EXISTS banking.currencies (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    symbol TEXT,
    scale INTEGER NOT NULL CHECK (scale >= 0),
    is_base BOOLEAN NOT NULL DEFAULT FALSE,
    status currency_status NOT NULL DEFAULT 'active',
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_currencies_company_id_code ON banking.currencies (company_id, code) WHERE (metadata->>'deleted_at') IS NULL;

CREATE TABLE IF NOT EXISTS banking.exchange_rates (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    from_currency TEXT NOT NULL,
    to_currency TEXT NOT NULL,
    rate NUMERIC(20, 8) NOT NULL,
    effective_at DATE NOT NULL,
    rate_type rate_type NOT NULL DEFAULT 'spot',
    source TEXT,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_exchange_rates_company_id_from_currency_to_currency_effective_at_rate_type ON banking.exchange_rates (company_id, from_currency, to_currency, effective_at, rate_type) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_exchange_rates_company_id_to_currency_effective_at ON banking.exchange_rates (company_id, to_currency, effective_at);

-- Restore the strict company fences (ADR-0014 posture these tables carried).

ALTER TABLE banking.currencies ENABLE ROW LEVEL SECURITY;
ALTER TABLE banking.currencies FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS currencies_company_isolation ON banking.currencies;
CREATE POLICY currencies_company_isolation ON banking.currencies
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE banking.exchange_rates ENABLE ROW LEVEL SECURITY;
ALTER TABLE banking.exchange_rates FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS exchange_rates_company_isolation ON banking.exchange_rates;
CREATE POLICY exchange_rates_company_isolation ON banking.exchange_rates
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Restore the audit timestamp triggers.

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

DROP TRIGGER IF EXISTS currencies_insert_audit ON banking.currencies;
CREATE TRIGGER currencies_insert_audit BEFORE INSERT ON banking.currencies
    FOR EACH ROW EXECUTE FUNCTION banking.currencies_audit_timestamp();
DROP TRIGGER IF EXISTS currencies_update_audit ON banking.currencies;
CREATE TRIGGER currencies_update_audit BEFORE UPDATE ON banking.currencies
    FOR EACH ROW EXECUTE FUNCTION banking.currencies_audit_timestamp();

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

DROP TRIGGER IF EXISTS exchange_rates_insert_audit ON banking.exchange_rates;
CREATE TRIGGER exchange_rates_insert_audit BEFORE INSERT ON banking.exchange_rates
    FOR EACH ROW EXECUTE FUNCTION banking.exchange_rates_audit_timestamp();
DROP TRIGGER IF EXISTS exchange_rates_update_audit ON banking.exchange_rates;
CREATE TRIGGER exchange_rates_update_audit BEFORE UPDATE ON banking.exchange_rates
    FOR EACH ROW EXECUTE FUNCTION banking.exchange_rates_audit_timestamp();
