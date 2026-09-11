//! Integrity probes for banking — invariants that must hold against a REAL Postgres beyond the golden
//! math. Requires DATABASE_URL (:5433/backbone_banking).
//!
//! IP-1..IP-3   the clearing invariants (service level).
//! IGT-1..IGT-3 the tenancy invariants on the guarded HTTP surface — the session is the one
//!              `org_auth` resolves from a signed token against the tenant's organization tree,
//!              never a request-body field.

#![expect(clippy::expect_used, reason = "test harness: a panic here names the setup failure precisely")]

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use backbone_auth::org::OrgVerifier;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_banking::application::service::banking_gl::{
    AccountingPostEnvelope, GlPostAck, GlPostRejected, GlPostSink, ReconcileEdgeAck,
    ReconcilePairRequest, ReconcileRejected, ReconcileSink, UnreconcilePairRequest,
};
use backbone_banking::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewBank, NewBankAccount, NewClearance, NewStatementImport,
    NewStatementLine,
};
use backbone_banking::presentation::http::create_guarded_banking_routes;
use backbone_banking::BankingModule;

const SECRET: &[u8] = b"banking-integrity-probe-secret";

#[derive(Serialize)]
struct TestClaims {
    sub: String,
    exp: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    org_unit_id: Option<Uuid>,
}

/// Mint an HS256 org token. `org_unit_id = None` models a token that authenticates a user but
/// names no acting unit — the org guard must refuse it (a writer without a resolvable session
/// must never run).
fn token(org_unit_id: Option<Uuid>) -> String {
    let claims = TestClaims {
        sub: "probe-user".into(),
        exp: 9_999_999_999,
        org_unit_id,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(SECRET),
    )
    .unwrap()
}

async fn module(pool: &PgPool) -> BankingModule {
    BankingModule::builder()
        .with_database(pool.clone())
        .build()
        .unwrap()
}
fn app(pool: &PgPool, m: &BankingModule) -> axum::Router {
    create_guarded_banking_routes(m, pool.clone(), OrgVerifier::hs256(SECRET))
}

/// POST a statement import with an optional bearer token. The request carries the tenant-database
/// extension `org_auth` reads — the composing service's tenant router provides it in a real
/// deployment.
async fn post_import(
    app: axum::Router,
    pool: &PgPool,
    body: String,
    bearer: Option<String>,
) -> (StatusCode, String) {
    post_import_to(app, pool, "/bank-statements/import", body, bearer).await
}

/// POST to an arbitrary path — lets a probe assert what the guard does NOT cover.
async fn post_import_to(
    app: axum::Router,
    pool: &PgPool,
    uri: &str,
    body: String,
    bearer: Option<String>,
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let mut req = builder.body(Body::from(body)).unwrap();
    req.extensions_mut().insert(pool.clone());
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// A well-formed import body (opening 0 → closing 500000 via one deposit), keyed by `file_ref`.
/// Carries no tenant field at all — the session is `org_auth`'s, never a body field.
fn import_body(bank_account_id: Uuid, file_ref: &str) -> String {
    format!(
        r#"{{"bankAccountId":"{bank_account_id}","periodStart":"2026-07-01","periodEnd":"2026-07-31",
             "openingBalance":"0","closingBalance":"500000","fileRef":"{file_ref}",
             "lines":[{{"txnDate":"2026-07-05","deposit":"500000","withdrawal":"0"}}]}}"#
    )
}

/// Create a bank account so the import body can name a real account.
async fn bank_account_for(w: &BankingWriteService) -> Uuid {
    let bank = w
        .create_bank(NewBank {
            name: uq("Bank"),
            swift_bic: None,
            country: None,
        })
        .await
        .unwrap();
    w.create_bank_account(NewBankAccount {
        branch_id: None,
        bank_id: bank,
        account_name: "Ops".into(),
        account_number: uq("ACC"),
        gl_account_id: Uuid::new_v4(),
        clearing_account_id: Uuid::new_v4(),
        currency: None,
        account_type: None,
    })
    .await
    .unwrap()
}

fn d(s: &str) -> Decimal {
    Decimal::from_str_exact(s).unwrap()
}
fn day(n: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 7, n).unwrap()
}
fn uq(p: &str) -> String {
    format!("{p}-{}", &Uuid::new_v4().simple().to_string()[..8])
}
async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5433/backbone_banking".to_string()
    });
    PgPool::connect(&url).await.expect("connect DB")
}

struct OkGl;
#[async_trait::async_trait]
impl GlPostSink for OkGl {
    async fn post(&self, _e: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        Ok(GlPostAck {
            post_id: Uuid::new_v4(),
            journal_id: Uuid::new_v4(),
            idempotent_reuse: false,
        })
    }
}
struct RejectingGl;
#[async_trait::async_trait]
impl GlPostSink for RejectingGl {
    async fn post(&self, _e: &AccountingPostEnvelope) -> Result<GlPostAck, GlPostRejected> {
        Err(GlPostRejected {
            code: "period_closed".into(),
            message: "closed".into(),
        })
    }
}
/// Refuses every graph call. These probes are refused BEFORE the reconciliation edge (line bound,
/// GL rejection, currency), so the sink never fires — if one ever does, the probe fails loudly
/// rather than recording a clearance whose edge went somewhere unexamined.
struct NoEdgeSink;
#[async_trait::async_trait]
impl ReconcileSink for NoEdgeSink {
    async fn reconcile_pair_on(
        &self,
        _conn: &mut sqlx::PgConnection,
        _req: &ReconcilePairRequest,
    ) -> Result<ReconcileEdgeAck, ReconcileRejected> {
        Err(ReconcileRejected {
            code: "unexpected_edge".into(),
            message: "probe refused before the edge; the sink must not be reached".into(),
        })
    }
    async fn unreconcile_pair_on(
        &self,
        _conn: &mut sqlx::PgConnection,
        _req: &UnreconcilePairRequest,
    ) -> Result<(), ReconcileRejected> {
        Err(ReconcileRejected {
            code: "unexpected_edge".into(),
            message: "probe refused before the edge; the sink must not be reached".into(),
        })
    }
}

// IP-1: a clearance cannot exceed the line's un-allocated remainder — over-clearing is refused and the
// line's allocation is untouched.
#[tokio::test]
async fn over_clearing_is_refused() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let bank = w
        .create_bank(NewBank {
            name: uq("Bank"),
            swift_bic: None,
            country: None,
        })
        .await
        .unwrap();
    let acct = w
        .create_bank_account(NewBankAccount {
            branch_id: None,
            bank_id: bank,
            account_name: "Ops".into(),
            account_number: uq("ACC"),
            gl_account_id: Uuid::new_v4(),
            clearing_account_id: Uuid::new_v4(),
            currency: None,
            account_type: None,
        })
        .await
        .unwrap();
    let import_id = w
        .import_statement(NewStatementImport {
            bank_account_id: acct,
            source_format: None,
            period_start: day(1),
            period_end: day(31),
            opening_balance: Decimal::ZERO,
            closing_balance: d("500000"),
            file_ref: None,
            lines: vec![NewStatementLine {
                txn_date: day(5),
                description: None,
                reference_no: None,
                deposit: d("500000"),
                withdrawal: Decimal::ZERO,
            }],
        })
        .await
        .unwrap();
    let txn: Uuid =
        sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1")
            .bind(import_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    let e = w
        .clear_transaction(
            NewClearance {
                bank_transaction_id: txn,
                matched_source_type: "payment".into(),
                matched_source_id: Uuid::new_v4(),
                matched_source_amount: d("600000"),
                matched_amount: d("600000"),
                match_method: None,
                clearance_date: day(6),
            },
            &OkGl,
            &NoEdgeSink,
        )
        .await
        .unwrap_err();
    assert!(matches!(e, BankingError::OverAllocated { .. }));
    let al: Decimal =
        sqlx::query_scalar("SELECT allocated_amount FROM banking.bank_transactions WHERE id=$1")
            .bind(txn)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        al,
        d("0.00"),
        "a refused clearance leaves the line untouched"
    );
}

// IP-2: a rejected GL post surfaces as an error and writes no clearance / no allocation.
#[tokio::test]
async fn rejected_clear_writes_nothing() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let bank = w
        .create_bank(NewBank {
            name: uq("Bank"),
            swift_bic: None,
            country: None,
        })
        .await
        .unwrap();
    let acct = w
        .create_bank_account(NewBankAccount {
            branch_id: None,
            bank_id: bank,
            account_name: "Ops".into(),
            account_number: uq("ACC"),
            gl_account_id: Uuid::new_v4(),
            clearing_account_id: Uuid::new_v4(),
            currency: None,
            account_type: None,
        })
        .await
        .unwrap();
    let import_id = w
        .import_statement(NewStatementImport {
            bank_account_id: acct,
            source_format: None,
            period_start: day(1),
            period_end: day(31),
            opening_balance: Decimal::ZERO,
            closing_balance: d("300000"),
            file_ref: None,
            lines: vec![NewStatementLine {
                txn_date: day(5),
                description: None,
                reference_no: None,
                deposit: d("300000"),
                withdrawal: Decimal::ZERO,
            }],
        })
        .await
        .unwrap();
    let txn: Uuid =
        sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1")
            .bind(import_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let e = w
        .clear_transaction(
            NewClearance {
                bank_transaction_id: txn,
                matched_source_type: "payment".into(),
                matched_source_id: Uuid::new_v4(),
                matched_source_amount: d("300000"),
                matched_amount: d("300000"),
                match_method: None,
                clearance_date: day(6),
            },
            &RejectingGl,
            &NoEdgeSink,
        )
        .await
        .unwrap_err();
    assert!(matches!(e, BankingError::GlRejected { .. }));
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM banking.bank_clearances WHERE bank_transaction_id=$1",
    )
    .bind(txn)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "no clearance row on a rejected post");
    let al: Decimal =
        sqlx::query_scalar("SELECT allocated_amount FROM banking.bank_transactions WHERE id=$1")
            .bind(txn)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(al, d("0.00"));
}

// IP-3: a non-IDR statement line is refused at clear time (no mis-valued clearing reaches the ledger).
#[tokio::test]
async fn non_idr_refused_at_clear() {
    let pool = pool().await;
    let w = BankingWriteService::new(pool.clone());
    let bank = w
        .create_bank(NewBank {
            name: uq("Bank"),
            swift_bic: None,
            country: None,
        })
        .await
        .unwrap();
    let acct = w
        .create_bank_account(NewBankAccount {
            branch_id: None,
            bank_id: bank,
            account_name: "Ops".into(),
            account_number: uq("ACC"),
            gl_account_id: Uuid::new_v4(),
            clearing_account_id: Uuid::new_v4(),
            currency: None,
            account_type: None,
        })
        .await
        .unwrap();
    let import_id = w
        .import_statement(NewStatementImport {
            bank_account_id: acct,
            source_format: None,
            period_start: day(1),
            period_end: day(31),
            opening_balance: Decimal::ZERO,
            closing_balance: d("100000"),
            file_ref: None,
            lines: vec![NewStatementLine {
                txn_date: day(5),
                description: None,
                reference_no: None,
                deposit: d("100000"),
                withdrawal: Decimal::ZERO,
            }],
        })
        .await
        .unwrap();
    let txn: Uuid =
        sqlx::query_scalar("SELECT id FROM banking.bank_transactions WHERE import_id=$1")
            .bind(import_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("UPDATE banking.bank_transactions SET currency='USD' WHERE id=$1")
        .bind(txn)
        .execute(&pool)
        .await
        .unwrap();
    let e = w
        .clear_transaction(
            NewClearance {
                bank_transaction_id: txn,
                matched_source_type: "payment".into(),
                matched_source_id: Uuid::new_v4(),
                matched_source_amount: d("100000"),
                matched_amount: d("100000"),
                match_method: None,
                clearance_date: day(6),
            },
            &OkGl,
            &NoEdgeSink,
        )
        .await
        .unwrap_err();
    assert!(matches!(e, BankingError::UnsupportedCurrency(c) if c == "USD"));
}

// IGT-0: the tenant guard is mounted with `route_layer`, so it wraps the import route ONLY — an
// unmatched path (generic CRUD this surface does not mount) still answers 404/405, not 401. A plain
// `.layer` would wrap the fallback too and make every non-existent route claim "auth required".
#[tokio::test]
async fn tenant_guard_does_not_swallow_unmatched_routes() {
    let pool = pool().await;
    let m = module(&pool).await;
    let (status, _) =
        post_import_to(app(&pool, &m), &pool, "/bank-statements/bulk", "[]".into(), None).await;
    assert!(
        status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND,
        "an unmounted path must not answer 401; got {status}"
    );
}

// IGT-1: an unauthenticated import is rejected. Before the tenant guard this import succeeded and
// stamped whatever `companyId` the caller put in the body.
#[tokio::test]
async fn guarded_import_rejects_unauthenticated() {
    let pool = pool().await;
    let m = module(&pool).await;
    let w = BankingWriteService::new(pool.clone());
    let acct = bank_account_for(&w).await;
    let (status, _) =
        post_import(app(&pool, &m), &pool, import_body(acct, &uq("FILE")), None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an unauthenticated import must not reach the service"
    );
}

// IGT-2: a token that authenticates a user but carries no `org_unit_id` claim is rejected — a
// writer with no resolvable session must never run.
#[tokio::test]
async fn guarded_import_rejects_token_without_org_unit() {
    let pool = pool().await;
    let m = module(&pool).await;
    let w = BankingWriteService::new(pool.clone());
    let acct = bank_account_for(&w).await;
    let (status, _) = post_import(
        app(&pool, &m),
        &pool,
        import_body(acct, &uq("FILE")),
        Some(token(None)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a token with no acting unit must not write"
    );
}

// IGT-3: a `companyId` smuggled in the body cannot name a tenant — there is nothing left for it to
// override. The module carries no tenancy of its own (ADR-0029): the import body has no tenant
// field, the tables carry no tenant column, and the only tenancy that ever applies is the session
// `org_auth` resolves — refused here fail-closed, since this probe's database holds no organization
// tree for the token's unit.
#[tokio::test]
async fn body_company_id_cannot_name_the_tenant() {
    let pool = pool().await;
    let m = module(&pool).await;
    let w = BankingWriteService::new(pool.clone());
    let attacker_company = Uuid::new_v4();
    let acct = bank_account_for(&w).await;
    let file_ref = uq("FILE");

    // Same well-formed body, plus a `companyId` no code path reads any more.
    let body = format!(
        r#"{{"companyId":"{attacker_company}","bankAccountId":"{acct}","periodStart":"2026-07-01",
             "periodEnd":"2026-07-31","openingBalance":"0","closingBalance":"500000",
             "fileRef":"{file_ref}",
             "lines":[{{"txnDate":"2026-07-05","deposit":"500000","withdrawal":"0"}}]}}"#
    );

    // A token naming a unit this database has no tree for: the guard refuses the session before any
    // handler runs — a write can only ever happen inside a resolved org scope, never by body fiat.
    let (status, resp) = post_import(
        app(&pool, &m),
        &pool,
        body,
        Some(token(Some(Uuid::new_v4()))),
    )
    .await;
    assert_ne!(
        status,
        StatusCode::CREATED,
        "a session the guard could not resolve must never write, got: {resp}"
    );

    // And structurally: the statement tables carry no tenant column the body could have stamped.
    for (table, column) in [
        ("bank_statement_imports", "company_id"),
        ("bank_transactions", "company_id"),
    ] {
        let hits: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns \
             WHERE table_schema='banking' AND table_name=$1 AND column_name=$2",
        )
        .bind(table)
        .bind(column)
        .fetch_one(&pool)
        .await
        .expect("column probe");
        assert_eq!(
            hits, 0,
            "banking.{table} must carry no tenant column (ADR-0029)"
        );
    }
}
