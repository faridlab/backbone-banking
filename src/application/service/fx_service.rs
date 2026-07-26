//! The FX / multi-currency engine (hand-authored, user-owned).
//!
//! `FxService` owns the currency catalogue, the immutable exchange-rate table, and the realised
//! FX gain/loss computation. It implements `ExchangeRateProvider` (the zero-cargo-edge read port
//! that billing/payment call via the composition ACL to resolve spot rates).
//!
//! Real-world rule: an invoice posts at the spot rate on posting_date; settlement at a later bank-
//! transacted rate realises an FX gain/loss to a dedicated account.

use backbone_orm::company_scope;
use chrono::NaiveDate;
use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use uuid::Uuid;

use crate::infrastructure::persistence::{
    ExchangeRateRepository, FxGainLossRepository, NewExchangeRateRow, NewFxGainLossRow,
};

// --- the read port (zero cargo edge — billing/payment use this via the composition ACL) --------

#[derive(Debug, Clone)]
pub struct ExchangeRateSnapshot {
    pub rate: Decimal,
    pub rate_id: Uuid,
    pub effective_at: NaiveDate,
}

#[async_trait::async_trait]
pub trait ExchangeRateProvider: Send + Sync {
    /// Resolve the latest spot rate `from → to` effective on or before `date`.
    async fn spot(
        &self, company_id: Uuid, from_currency: &str, to_currency: &str, date: NaiveDate,
    ) -> Result<ExchangeRateSnapshot, String>;
}

// --- error ------------------------------------------------------------------

#[derive(Debug)]
pub enum FxError {
    RateNotFound(String, String, NaiveDate),
    Db(sqlx::Error),
}
impl std::fmt::Display for FxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FxError::RateNotFound(from, to, d) => write!(f, "no spot rate for {from}→{to} on {d}"),
            FxError::Db(e) => write!(f, "db: {e}"),
        }
    }
}
impl std::error::Error for FxError {}
impl From<sqlx::Error> for FxError {
    fn from(e: sqlx::Error) -> Self { FxError::Db(e) }
}

// --- the service ------------------------------------------------------------

#[derive(Clone)]
pub struct FxService {
    db_pool: PgPool,
}

/// A realised FX computation result.
#[derive(Debug, Clone, PartialEq)]
pub struct FxResult {
    pub gain_loss_id: Uuid,
    pub base_amount_delta: Decimal,
    pub direction: String, // "gain" | "loss"
}

impl FxService {
    pub fn new(db_pool: PgPool) -> Self { Self { db_pool } }

    /// Record an immutable exchange rate. The unique (company, pair, date, type) fence makes a
    /// duplicate insert for the same key a no-op (idempotent).
    pub async fn record_rate(
        &self, company_id: Uuid, from_currency: &str, to_currency: &str,
        rate: Decimal, effective_at: NaiveDate, rate_type: &str, source: Option<&str>,
    ) -> Result<Uuid, FxError> {
        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let id = Uuid::new_v4();
        let rates = ExchangeRateRepository::new(self.db_pool.clone());
        rates.insert_rate(&mut *tx, &NewExchangeRateRow {
            id, company_id, from_currency, to_currency, rate, effective_at, rate_type, source,
        }).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Compute + record the realised FX gain/loss on a foreign-currency settlement.
    ///
    /// `delta = foreign_amount × (realised_rate − original_rate)`. Positive = gain; negative = loss.
    /// Records an `FxGainLoss` row and returns the result.
    pub async fn compute_fx_gain_loss(
        &self, company_id: Uuid, bank_clearance_id: Option<Uuid>, matched_source_id: Uuid,
        currency: &str, original_rate: Decimal, realised_rate: Decimal, foreign_amount: Decimal,
        fx_account_id: Uuid,
    ) -> Result<FxResult, FxError> {
        let base_original = money(foreign_amount * original_rate);
        let base_realised = money(foreign_amount * realised_rate);
        let delta = base_realised - base_original;
        let direction = if delta >= Decimal::ZERO { "gain" } else { "loss" };

        let mut tx = self.db_pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company_id).await?;
        let id = Uuid::new_v4();
        let gain_losses = FxGainLossRepository::new(self.db_pool.clone());
        gain_losses.insert_gain_loss(&mut *tx, &NewFxGainLossRow {
            id, company_id, bank_clearance_id, matched_source_id, currency,
            original_rate, realised_rate, base_amount_delta: delta, direction, fx_account_id,
        }).await?;
        tx.commit().await?;

        Ok(FxResult { gain_loss_id: id, base_amount_delta: delta, direction: direction.into() })
    }
}

// ExchangeRateProvider is implemented by FxService (the composition ACL injects it into
// billing/payment so they can resolve spot rates without importing banking).
#[async_trait::async_trait]
impl ExchangeRateProvider for FxService {
    async fn spot(
        &self, company_id: Uuid, from_currency: &str, to_currency: &str, date: NaiveDate,
    ) -> Result<ExchangeRateSnapshot, String> {
        let pool = self.db_pool.clone();
        let rates = ExchangeRateRepository::new(pool.clone());
        let from_owned = from_currency.to_string();
        let to_owned = to_currency.to_string();
        // Fence the read to company_id via the scoped helper (not just the WHERE clause), so under
        // the non-super app role the ADR-0008 RLS fence admits the row instead of failing closed.
        let row = company_scope::with_company_scope(Some(company_id), async move {
            rates.find_spot(&pool, company_id, &from_owned, &to_owned, date).await
        })
        .await
        .map_err(|e| e.to_string())?;
        match row {
            Some(r) => Ok(ExchangeRateSnapshot {
                rate: r.rate, rate_id: r.rate_id, effective_at: r.effective_at,
            }),
            None => Err(format!("no spot rate for {from_currency}→{to_currency} on/before {date}")),
        }
    }
}

fn money(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}
