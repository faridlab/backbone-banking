//! Guarded route composition — the RECOMMENDED way to mount the banking module.
//!
//! Hand-authored (user-owned). Read documents + **validated import** (a statement with balance
//! continuity checked); generic create/update/delete CRUD is NOT mounted, so a caller cannot write a
//! statement whose lines don't reconcile or bypass the clearing path. Matching + clearing +
//! reconciliation need a `GlPostSink` / supplied candidates (a composition layer), so they are
//! service/job-driven, not HTTP routes.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
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
    company_id: Uuid,
    bank_account_id: Uuid,
    #[serde(default)] source_format: Option<String>,
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    #[serde(default)] opening_balance: Decimal,
    #[serde(default)] closing_balance: Decimal,
    #[serde(default)] file_ref: Option<String>,
    lines: Vec<LineBody>,
}
async fn import_statement(State(svc): State<Arc<BankingWriteService>>, Json(b): Json<ImportBody>) -> axum::response::Response {
    let imp = NewStatementImport {
        company_id: b.company_id, bank_account_id: b.bank_account_id, source_format: b.source_format,
        period_start: b.period_start, period_end: b.period_end, opening_balance: b.opening_balance,
        closing_balance: b.closing_balance, file_ref: b.file_ref,
        lines: b.lines.into_iter().map(Into::into).collect(),
    };
    match svc.import_statement(imp).await {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err(e),
    }
}

fn write_routes(svc: Arc<BankingWriteService>) -> Router {
    Router::new()
        .route("/bank-statements/import", post(import_statement))
        .with_state(svc)
}

/// Mount the banking module: read documents + validated statement import. Generic mutation is not
/// mounted; matching/clearing/reconciliation are service/job-driven.
/// **Prefer this over `BankingModule::all_crud_routes()` for any real deployment.**
pub fn create_guarded_banking_routes(m: &BankingModule, pool: PgPool) -> Router {
    let write = Arc::new(BankingWriteService::new(pool));
    Router::new()
        .merge(create_bank_read_routes(m.bank_service.clone()))
        .merge(create_bank_account_read_routes(m.bank_account_service.clone()))
        .merge(create_bank_statement_import_read_routes(m.bank_statement_import_service.clone()))
        .merge(create_bank_transaction_read_routes(m.bank_transaction_service.clone()))
        .merge(create_bank_reconciliation_read_routes(m.bank_reconciliation_service.clone()))
        .merge(write_routes(write))
}
