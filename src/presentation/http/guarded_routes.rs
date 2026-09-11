//! Guarded route composition — the RECOMMENDED way to mount the banking module.
//!
//! Hand-authored (user-owned). Read documents + **validated import** (a statement with balance
//! continuity checked); generic create/update/delete CRUD is NOT mounted, so a caller cannot write a
//! statement whose lines don't reconcile or bypass the clearing path. The import is ID-only — the
//! session is the one `org_auth` resolved from the Bearer token, never a request-body field.
//! Matching + clearing + reconciliation need a `GlPostSink` / supplied candidates (a composition
//! layer), so they are service/job-driven, not HTTP routes.
//!
//! Tenancy (ADR-0029): the module carries no tenancy of its own. `org_auth` verifies the Bearer
//! token, resolves the session's org scope against the request's tenant tree, and runs every
//! handler inside that scope — the module's statements ride the request-dedicated connection it
//! binds, and the composing service's tenancy decorator does the actual row-level fencing. The
//! guard reads the tenant database from the `backbone_orm::PgPool` request extension, so this
//! surface must be mounted inside the composing service's tenant router.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware::from_fn_with_state,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use backbone_auth::org::{org_auth, OrgVerifier};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::service::banking_write_service::{
    BankingError, BankingWriteService, NewStatementImport, NewStatementLine,
};
use crate::BankingModule;

use super::{
    create_bank_account_read_routes, create_bank_read_routes,
    create_bank_reconciliation_read_routes, create_bank_statement_import_read_routes,
    create_bank_transaction_read_routes, create_reconcile_preset_read_routes,
};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
    message: String,
}
#[derive(Debug, Serialize)]
struct IdResponse {
    id: Uuid,
}
fn err(e: BankingError) -> axum::response::Response {
    let s = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        s,
        Json(ErrorBody {
            error: e.code(),
            message: e.to_string(),
        }),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LineBody {
    txn_date: chrono::NaiveDate,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    reference_no: Option<String>,
    #[serde(default)]
    deposit: Decimal,
    #[serde(default)]
    withdrawal: Decimal,
}
impl From<LineBody> for NewStatementLine {
    fn from(b: LineBody) -> Self {
        NewStatementLine {
            txn_date: b.txn_date,
            description: b.description,
            reference_no: b.reference_no,
            deposit: b.deposit,
            withdrawal: b.withdrawal,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportBody {
    // ID-only: the session is the one `org_auth` resolved and bound for the request, never a body
    // field — a client must not be able to name the tenant whose bank statement it imports.
    bank_account_id: Uuid,
    #[serde(default)]
    source_format: Option<String>,
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    #[serde(default)]
    opening_balance: Decimal,
    #[serde(default)]
    closing_balance: Decimal,
    #[serde(default)]
    file_ref: Option<String>,
    lines: Vec<LineBody>,
}
async fn import_statement(
    State(svc): State<Arc<BankingWriteService>>,
    Json(b): Json<ImportBody>,
) -> axum::response::Response {
    let imp = NewStatementImport {
        bank_account_id: b.bank_account_id,
        source_format: b.source_format,
        period_start: b.period_start,
        period_end: b.period_end,
        opening_balance: b.opening_balance,
        closing_balance: b.closing_balance,
        file_ref: b.file_ref,
        lines: b.lines.into_iter().map(Into::into).collect(),
    };
    match svc.import_statement(imp).await {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err(e),
    }
}

#[derive(Debug, Deserialize)]
struct CandidatesQuery {
    txn_id: Uuid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidateBody {
    payment_id: Uuid,
    payment_number: String,
    open_amount: Decimal,
    posting_date: chrono::NaiveDate,
    score: i64,
    reason: String,
}

/// `GET /reconcile-presets/:id/candidates?txn_id=<line>` — the ordered candidate list for one
/// statement line under one preset. ID-only: the session is the one `org_auth` resolved; both the
/// preset and the line are fence-read, and another unit's rows are indistinguishable from absence —
/// refused as not-found (the decorator's row-level fence, ADR-0029).
async fn preset_candidates(
    State(svc): State<Arc<BankingWriteService>>,
    Path(preset_id): Path<Uuid>,
    Query(q): Query<CandidatesQuery>,
) -> axum::response::Response {
    match svc.order_candidates(preset_id, q.txn_id).await {
        Ok(list) => (
            StatusCode::OK,
            Json(
                list.into_iter()
                    .map(|c| CandidateBody {
                        payment_id: c.payment_id,
                        payment_number: c.payment_number,
                        open_amount: c.open_amount,
                        posting_date: c.posting_date,
                        score: c.score,
                        reason: c.reason,
                    })
                    .collect::<Vec<_>>(),
            ),
        )
            .into_response(),
        Err(e) => err(e),
    }
}

fn write_routes(svc: Arc<BankingWriteService>, verifier: OrgVerifier) -> Router {
    Router::new()
        .route("/bank-statements/import", post(import_statement))
        // Preset-driven candidate ordering — a READ (side-effect-free by construction; the ordering
        // probe pins it), mounted beside the import because it shares the write service's injected
        // candidate-pool port. The operator confirms a candidate through the clear verb, not here.
        .route("/reconcile-presets/:id/candidates", get(preset_candidates))
        // The import is scope-bound: `org_auth` rejects a request whose token is absent, invalid,
        // or names a unit outside this tenant's tree, and runs the handler inside the resolved org
        // scope — a handler only ever executes with a proven, fenced session.
        //
        // `route_layer`, not `layer`: `layer` would also wrap this router's fallback, so once merged
        // every *unmatched* path (e.g. the generic CRUD paths this surface deliberately does not mount)
        // would answer 401 instead of 404 — leaking "auth required" for routes that do not exist, and
        // masking the CRUD-bypass probes.
        .route_layer(from_fn_with_state(verifier, org_auth))
        .with_state(svc)
}

/// Mount the banking module: read documents + validated, scope-fenced statement import. Generic
/// mutation is not mounted; matching/clearing/reconciliation are service/job-driven.
/// **Prefer this over `BankingModule::all_crud_routes()` for any real deployment.**
///
/// The composing service builds one [`OrgVerifier`] from its JWT secret and passes it here; the
/// surface derives its session from the token, so no tenant crosses the wire in a body. Mount
/// inside the tenant router with the `backbone_orm::PgPool` request extension attached —
/// `org_auth` resolves the scope against that pool.
pub fn create_guarded_banking_routes(
    m: &BankingModule,
    pool: PgPool,
    verifier: OrgVerifier,
) -> Router {
    let write = Arc::new(BankingWriteService::new(pool));
    Router::new()
        .merge(create_bank_read_routes(m.bank_service.clone()))
        .merge(create_bank_account_read_routes(
            m.bank_account_service.clone(),
        ))
        .merge(create_bank_statement_import_read_routes(
            m.bank_statement_import_service.clone(),
        ))
        .merge(create_bank_transaction_read_routes(
            m.bank_transaction_service.clone(),
        ))
        .merge(create_bank_reconciliation_read_routes(
            m.bank_reconciliation_service.clone(),
        ))
        .merge(create_reconcile_preset_read_routes(
            m.reconcile_preset_service.clone(),
        ))
        .merge(write_routes(write, verifier))
}
