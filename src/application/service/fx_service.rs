//! The FX realisation engine (hand-authored, user-owned).
//!
//! Banking owns ONLY the realised FX gain/loss on a foreign-currency settlement — the trigger (a
//! bank clearance) is a banking fact. The currency catalogue and the exchange-rate table moved to
//! backbone-corporate (the single FX owner); banking's rate tables are retired and dropped.
//!
//! [`ExchangeRateProvider`] stays as the read port definition (zero cargo edge — the composing host
//! implements it over corporate's `FxService` and injects it where billing/payment resolve spot
//! rates through the composition ACL).
//!
//! Real-world rule: an invoice posts at the spot rate on posting_date; settlement at a later bank-
//! transacted rate realises an FX gain/loss to a dedicated account.

use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::PgPool;
use uuid::Uuid;

use chrono::NaiveDate;

use crate::infrastructure::persistence::{FxGainLossRepository, NewFxGainLossRow};

// --- the read port (zero cargo edge — the host seam implements this over corporate's FX engine) --

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
        &self,
        company_id: Uuid,
        from_currency: &str,
        to_currency: &str,
        date: NaiveDate,
    ) -> Result<ExchangeRateSnapshot, String>;
}

// --- error ------------------------------------------------------------------

#[derive(Debug)]
pub enum FxError {
    Db(sqlx::Error),
}
impl std::fmt::Display for FxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FxError::Db(e) => write!(f, "db: {e}"),
        }
    }
}
impl std::error::Error for FxError {}
impl From<sqlx::Error> for FxError {
    fn from(e: sqlx::Error) -> Self {
        FxError::Db(e)
    }
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
    pub fn new(db_pool: PgPool) -> Self {
        Self { db_pool }
    }

    /// Compute + record the realised FX gain/loss on a foreign-currency settlement.
    ///
    /// `delta = foreign_amount × (realised_rate − original_rate)`. Positive = gain; negative = loss.
    /// Records an `FxGainLoss` row and returns the result.
    pub async fn compute_fx_gain_loss(
        &self,
        company_id: Uuid,
        bank_clearance_id: Option<Uuid>,
        matched_source_id: Uuid,
        currency: &str,
        original_rate: Decimal,
        realised_rate: Decimal,
        foreign_amount: Decimal,
        fx_account_id: Uuid,
    ) -> Result<FxResult, FxError> {
        let base_original = money(foreign_amount * original_rate);
        let base_realised = money(foreign_amount * realised_rate);
        let delta = base_realised - base_original;
        let direction = if delta >= Decimal::ZERO {
            "gain"
        } else {
            "loss"
        };

        let mut tx = self.db_pool.begin().await?;
        backbone_orm::company_scope::bind_company_on(&mut tx, company_id).await?;
        let id = Uuid::new_v4();
        let gain_losses = FxGainLossRepository::new(self.db_pool.clone());
        gain_losses
            .insert_gain_loss(
                &mut *tx,
                &NewFxGainLossRow {
                    id,
                    company_id,
                    bank_clearance_id,
                    matched_source_id,
                    currency,
                    original_rate,
                    realised_rate,
                    base_amount_delta: delta,
                    direction,
                    fx_account_id,
                },
            )
            .await?;
        tx.commit().await?;

        Ok(FxResult {
            gain_loss_id: id,
            base_amount_delta: delta,
            direction: direction.into(),
        })
    }
}

fn money(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}
