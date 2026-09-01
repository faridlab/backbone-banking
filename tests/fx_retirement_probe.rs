//! FXP — the FX-ownership retirement probe. The currency catalogue + exchange-rate table moved to
//! backbone-corporate (single FX owner); banking keeps ONLY the realised gain/loss engine.
//!
//! - FXP-1: `compute_fx_gain_loss` still computes + records gain and loss (uses `fx_gain_losses`
//!   alone — no rate tables anywhere on the path).
//! - FXP-2: the retired tables are GONE (`banking.currencies`, `banking.exchange_rates` → NULL),
//!   the surviving table is PRESENT (`banking.fx_gain_losses`), and the shared enum TYPES survive
//!   the retirement — `fx_direction` backs the surviving table, while `currency_status` +
//!   `rate_type` stay in the public namespace because corporate's guarded `CREATE TYPE`s adopt
//!   them in a shared database (the rollover contract: types outlive banking's ownership).
//! - FXP-3: the HTTP surface exposes NO currency/exchange-rate route — a fully authenticated GET
//!   matches no route (404), while a control route on the same router answers non-404. Retirement
//!   that left CRUD mounted would keep dragging writes through a dead fence.
//!
//! Requires DATABASE_URL (:5433/backbone_banking with banking migrated, incl. the retirement).

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_banking::application::service::fx_service::FxService;

fn d(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap()
}

#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string()
    });
    PgPool::connect(&url).await.expect("connect DB")
}

/// FXP-1 — the realisation engine survives the retirement untouched: a favourable move records a
/// gain, an unfavourable one a loss, both into `banking.fx_gain_losses`.
#[tokio::test]
async fn realised_gain_loss_survives_the_rate_table_retirement() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let svc = FxService::new(pool.clone());
    let fx_acct = Uuid::new_v4();

    // Original 15,700, realised 15,800, foreign 100 USD → delta = 100 × 100 = 10,000 IDR (gain).
    let gain = svc
        .compute_fx_gain_loss(
            company,
            None,
            Uuid::new_v4(),
            "USD",
            d("15700"),
            d("15800"),
            d("100"),
            fx_acct,
        )
        .await
        .unwrap();
    assert_eq!(gain.direction, "gain");
    assert_eq!(gain.base_amount_delta, d("10000.00"));

    // Mirror: original 15,800, realised 15,700 → −10,000 IDR (loss).
    let loss = svc
        .compute_fx_gain_loss(
            company,
            None,
            Uuid::new_v4(),
            "USD",
            d("15800"),
            d("15700"),
            d("100"),
            fx_acct,
        )
        .await
        .unwrap();
    assert_eq!(loss.direction, "loss");
    assert_eq!(loss.base_amount_delta, d("-10000.00"));

    // Both rows landed with their direction stamped.
    let dirs: Vec<String> = sqlx::query_scalar(
        "SELECT direction::text FROM banking.fx_gain_losses WHERE id = ANY($1) ORDER BY direction",
    )
    .bind(vec![gain.gain_loss_id, loss.gain_loss_id])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(dirs, vec!["gain".to_string(), "loss".to_string()]);
}

/// FXP-2 — the schema shape after the retirement: retired tables dropped, survivor present, shared
/// enum types kept (fx_direction backs the survivor; currency_status + rate_type await corporate's
/// adoption — dropping them would break corporate's dependent columns in a shared database).
#[tokio::test]
async fn retired_tables_are_gone_and_shared_enum_types_survive() {
    let pool = pool().await;

    for gone in ["banking.currencies", "banking.exchange_rates"] {
        let present: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
            .bind(&gone)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            present.is_none(),
            "{gone} must be dropped — corporate owns the FX catalogue now"
        );
    }
    let survivor: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('banking.fx_gain_losses')::text")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        survivor.is_some(),
        "fx_gain_losses stays — realisation is a banking fact"
    );

    for typ in ["fx_direction", "currency_status", "rate_type"] {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pg_type WHERE typname = $1")
            .bind(typ)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1, "enum type {typ} must survive the retirement");
    }
}

/// FXP-3 — no currency/exchange-rate route survives on the guarded surface. The probe mints a real
/// HS256 token so the discriminator cannot be "unauthenticated": the control POST answers non-404
/// through the same router + token, while the retired bases match no route at all.
#[tokio::test]
#[expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]
async fn no_http_route_survives_for_the_retired_catalogue() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let pool = pool().await;
    let company = Uuid::new_v4();
    let m = backbone_banking::BankingModule::builder()
        .with_database(pool.clone())
        .build()
        .expect("module");
    let secret = b"fxp-probe-secret";
    let verifier = backbone_auth::company::CompanyVerifier::hs256(secret);

    #[derive(serde::Serialize)]
    struct Claims {
        sub: String,
        exp: usize,
        company_id: Uuid,
    }
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: "probe".into(),
            exp: (chrono::Utc::now().timestamp() + 600) as usize,
            company_id: company,
        },
        &jsonwebtoken::EncodingKey::from_secret(secret),
    )
    .unwrap();

    let router = backbone_banking::presentation::http::create_guarded_banking_routes(
        &m,
        pool.clone(),
        verifier,
    );

    // Control: a mounted route answers non-404 with this token (an empty import body fails
    // validation, proving routing + auth both pass — it is NOT "no route").
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/bank-statements/import")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let control = resp.status();
    assert_ne!(
        control,
        StatusCode::NOT_FOUND,
        "control route must be reachable through the same token"
    );

    // Probes: the retired catalogue bases match no route — 404, not 401/405/422.
    for uri in [
        "/currencies",
        "/exchange_rates",
        "/currencies/00000000-0000-0000-0000-000000000000",
    ] {
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "GET {uri} must match no route, got {}",
            resp.status()
        );
    }
}
