//! FXSEAM-1 — the FX engine: rate recording, spot lookup, realised gain/loss.
//! Records an immutable exchange rate, resolves it via the ExchangeRateProvider port,
//! and computes a realised FX gain/loss. Requires DATABASE_URL (:5433/backbone_banking).

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_banking::application::service::fx_service::{ExchangeRateProvider, FxService};

fn d(s: &str) -> Decimal { Decimal::from_str_exact(s).unwrap() }

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string());
    PgPool::connect(&url).await.expect("connect DB")
}

#[tokio::test]
async fn records_rate_resolves_spot_and_computes_fx_gain() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let today = chrono::Utc::now().date_naive();
    let svc = FxService::new(pool.clone());

    // 1) Record a USD→IDR spot rate @ 15,800.
    svc.record_rate(company, "USD", "IDR", d("15800"), today, "spot", Some("test"))
        .await.unwrap();

    // 2) Resolve the spot rate via the ExchangeRateProvider port.
    let snap = svc.spot(company, "USD", "IDR", today).await.unwrap();
    assert_eq!(snap.rate, d("15800"), "spot rate resolves");

    // 3) Compute a realised FX gain: original 15,700, realised 15,800, foreign 100 USD.
    //    delta = 100 × (15800 − 15700) = 10,000 IDR (gain).
    let fx_acct = Uuid::new_v4();
    let source = Uuid::new_v4();
    let result = svc.compute_fx_gain_loss(
        company, None, source, "USD", d("15700"), d("15800"), d("100"), fx_acct,
    ).await.unwrap();
    assert_eq!(result.direction, "gain", "favourable rate → gain");
    assert_eq!(result.base_amount_delta, d("10000.00"), "delta = 100 × (15800−15700)");

    // 4) Assert the FxGainLoss row.
    let row_direction: String = sqlx::query_scalar(
        "SELECT direction::text FROM banking.fx_gain_losses WHERE id = $1")
        .bind(result.gain_loss_id).fetch_one(&pool).await.unwrap();
    assert_eq!(row_direction, "gain");

    // 5) Compute a realised FX loss: original 15,800, realised 15,700, foreign 100 USD.
    //    delta = 100 × (15700 − 15800) = −10,000 IDR (loss).
    let result2 = svc.compute_fx_gain_loss(
        company, None, Uuid::new_v4(), "USD", d("15800"), d("15700"), d("100"), fx_acct,
    ).await.unwrap();
    assert_eq!(result2.direction, "loss", "unfavourable rate → loss");
    assert_eq!(result2.base_amount_delta, d("-10000.00"), "delta = 100 × (15700−15800)");

    // 6) Idempotency: re-record the same rate → no error (ON CONFLICT DO NOTHING).
    svc.record_rate(company, "USD", "IDR", d("15800"), today, "spot", Some("test"))
        .await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM banking.exchange_rates WHERE company_id = $1 AND from_currency = 'USD'")
        .bind(company).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1, "re-record is idempotent (one rate row)");
}
