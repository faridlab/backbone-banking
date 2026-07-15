//! Guarded route composition — the RECOMMENDED way to mount the banking module.
//!
//! Hand-authored (user-owned). Read documents + **validated import** (a statement with balance
//! continuity checked); generic create/update/delete CRUD is NOT mounted, so a caller cannot write a
//! statement whose lines don't reconcile or bypass the clearing path. The import derives its tenant
//! from a signed Bearer token (`TenantContext`) rather than the request body. Matching + clearing +
//! reconciliation need a `GlPostSink` / supplied candidates (a composition layer), so they are
//! service/job-driven, not HTTP routes.

use std::sync::Arc;

use axum::{
    extract::State, http::StatusCode, middleware::from_fn_with_state, response::IntoResponse,
    routing::post, Json, Router,
};
use backbone_auth::tenant::{tenant_auth, TenantContext, TenantVerifier};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewStatementImport, NewStatementLine,
};
use crate::BankingModule;

use super::{
    create_bank_account_read_routes, create_bank_read_routes, create_bank_reconciliation_read_routes,
    create_bank_statement_import_read_routes, create_bank_transaction_read_routes,
};

#[derive(Debug, Serialize)]
struct ErrorBody { error: String, message: String }
#[derive(Debug, Serialize)]
struct IdResponse { id: Uuid }
fn err(e: BankingError) -> axum::response::Response {
    let s = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(ErrorBody { error: e.code(), message: e.to_string() })).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LineBody {
    txn_date: chrono::NaiveDate,
    #[serde(default)] description: Option<String>,
    #[serde(default)] reference_no: Option<String>,
    #[serde(default)] deposit: Decimal,
    #[serde(default)] withdrawal: Decimal,
}
impl From<LineBody> for NewStatementLine {
    fn from(b: LineBody) -> Self {
        NewStatementLine { txn_date: b.txn_date, description: b.description, reference_no: b.reference_no, deposit: b.deposit, withdrawal: b.withdrawal }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportBody {
    // No `company_id`: the tenant is derived from the signed token via `TenantContext`, never from the
    // request body — a client must not be able to name the company whose bank statement it imports.
    bank_account_id: Uuid,
    #[serde(default)] source_format: Option<String>,
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    #[serde(default)] opening_balance: Decimal,
    #[serde(default)] closing_balance: Decimal,
    #[serde(default)] file_ref: Option<String>,
    lines: Vec<LineBody>,
}
async fn import_statement(
    State(svc): State<Arc<BankingWriteService>>,
    tenant: TenantContext,
    Json(b): Json<ImportBody>,
) -> axum::response::Response {
    let imp = NewStatementImport {
        company_id: tenant.company_id, bank_account_id: b.bank_account_id, source_format: b.source_format,
        period_start: b.period_start, period_end: b.period_end, opening_balance: b.opening_balance,
        closing_balance: b.closing_balance, file_ref: b.file_ref,
        lines: b.lines.into_iter().map(Into::into).collect(),
    };
    match svc.import_statement(imp).await {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err(e),
    }
}

fn write_routes(svc: Arc<BankingWriteService>, verifier: TenantVerifier) -> Router {
    Router::new()
        .route("/bank-statements/import", post(import_statement))
        // The import is tenant-scoped: `tenant_auth` rejects a request whose token is absent, invalid,
        // or carries no `company_id`, so the writer only ever runs with a proven tenant.
        //
        // `route_layer`, not `layer`: `layer` would also wrap this router's fallback, so once merged
        // every *unmatched* path (e.g. the generic CRUD paths this surface deliberately does not mount)
        // would answer 401 instead of 404 — leaking "auth required" for routes that do not exist, and
        // masking the CRUD-bypass probes.
        .route_layer(from_fn_with_state(verifier, tenant_auth))
        .with_state(svc)
}

/// Mount the banking module: read documents + validated, tenant-scoped statement import. Generic
/// mutation is not mounted; matching/clearing/reconciliation are service/job-driven.
/// **Prefer this over `BankingModule::all_crud_routes()` for any real deployment.**
///
/// The composing service builds one [`TenantVerifier`] from its JWT secret and passes it here; the
/// import derives `company_id` from the token, so no tenant crosses the wire in a body.
pub fn create_guarded_banking_routes(
    m: &BankingModule,
    pool: PgPool,
    verifier: TenantVerifier,
) -> Router {
    let write = Arc::new(BankingWriteService::new(pool));
    Router::new()
        .merge(create_bank_read_routes(m.bank_service.clone()))
        .merge(create_bank_account_read_routes(m.bank_account_service.clone()))
        .merge(create_bank_statement_import_read_routes(m.bank_statement_import_service.clone()))
        .merge(create_bank_transaction_read_routes(m.bank_transaction_service.clone()))
        .merge(create_bank_reconciliation_read_routes(m.bank_reconciliation_service.clone()))
        .merge(write_routes(write, verifier))
}
