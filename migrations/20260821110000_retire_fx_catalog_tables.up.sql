-- Retire the FX catalogue tables from banking.
-- The currency catalogue and the exchange-rate table moved to backbone-corporate,
-- the single FX owner: corporate owns the money master and the effective-dated
-- rate table; banking keeps only the FX REALISATION (fx_gain_losses), whose
-- trigger — a bank clearance — is a banking fact.
--
-- First-cycle retirement: the FX ownership moved before any environment holds
-- real rates (dev and every test DB carry zero rows here), so the tables drop
-- with no data path. An environment that ever DID hold rates would need them
-- copied into corporate.currency_exchanges and parity-probed BEFORE this
-- migration applies — the window semantics differ (corporate keeps
-- effective-dated windows; banking kept point-in-time rows), and the parity
-- read is corporate's latest-on-or-before.
--
-- The public enum types currency_status and rate_type are deliberately NOT
-- dropped: they are unqualified (shared namespace) and corporate's guarded
-- CREATE TYPE adopts them where present — dropping them here would break
-- corporate's columns in a co-located database, and an unused type is inert.
-- fx_direction stays: fx_gain_losses.direction still uses it.

DROP TABLE IF EXISTS banking.exchange_rates;
DROP TABLE IF EXISTS banking.currencies;

-- The audit trigger functions are table-scoped leftovers once the tables go.
DROP FUNCTION IF EXISTS banking.exchange_rates_audit_timestamp();
DROP FUNCTION IF EXISTS banking.currencies_audit_timestamp();
