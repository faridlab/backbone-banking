//! Reconcile-preset candidate ordering — a READ, never a match (hand-authored, user-owned).
//!
//! An `impl BankingWriteService` chunk over the vocabulary in [`super::banking_write_service`].
//! A [`ReconcilePreset`](crate::domain::entity::ReconcilePreset) ranks the open settlements a
//! statement line could clear against, with a human-readable reason per candidate. Matching stays
//! where it was: the operator confirms through the existing `clear_transaction` verb. This path is
//! **side-effect-free by construction** — it takes no sink, writes nothing, and advances no
//! allocation watermark; the ordering probe pins that.
//!
//! The candidate POOL comes from [`CandidatePoolPort`]. Banking never imports payment (zero cargo
//! edge); the default [`PaymentPoolRead`] reads the posted, still-open payment headers through a
//! `to_regclass`-guarded cross-schema query (the reconcilability-read precedent), and the composing
//! host may inject its own. The open amount of each candidate is computed from BANKING's own
//! `bank_clearances` — the same already-cleared sum the clear verb bounds against — so ordering and
//! clearing can never disagree about what is open.
//!
//! Signals: `reference_exact`, `amount_exact`, `amount_within_tolerance`, and `days_window` rank
//! from the line + candidate data banking holds. `party` is vocabulary-only for now — a statement
//! line carries no counterparty until a composition supplies one, so that preset ranks nothing and
//! says so in its reason (an empty lie would be worse).

use rust_decimal::{Decimal, RoundingStrategy};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use backbone_orm::org_scope;

use super::banking_write_service::{BankingError, BankingWriteService};

/// One open settlement the line might clear against, as the ordering sees it.
#[derive(Debug, Clone)]
pub struct OpenCandidate {
    pub payment_id: Uuid,
    pub payment_number: String,
    pub paid_amount: Decimal,
    pub posting_date: chrono::NaiveDate,
    pub reference_no: Option<String>,
}

/// A ranked candidate, as the route returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedCandidate {
    pub payment_id: Uuid,
    pub payment_number: String,
    pub open_amount: Decimal,
    pub posting_date: chrono::NaiveDate,
    pub score: i64,
    pub reason: String,
}

/// The line side of the ordering, read from banking's own statement tables.
#[derive(Debug, Clone)]
pub struct CandidateLineBasis {
    pub bank_account_id: Uuid,
    pub deposit: Decimal,
    pub withdrawal: Decimal,
    pub txn_date: chrono::NaiveDate,
    pub reference_no: Option<String>,
}

/// Where the candidate pool comes from. Injectable so module tests (and non-payment compositions)
/// substitute their own; the default is the guarded payment-header read.
#[async_trait::async_trait]
pub trait CandidatePoolPort: Send + Sync {
    async fn open_candidates(
        &self,
        pool: &PgPool,
        bank_account_id: Uuid,
    ) -> Result<Vec<OpenCandidate>, BankingError>;
}

/// The default pool: posted payments against the same bank account, read from
/// `payment.payment_entries` through a `to_regclass` guard. Schema absent (standalone module
/// database, mis-composed host) ⇒ REFUSE — ranking against a pool banking cannot see would present
/// fiction as candidates (fail-closed, the reconcilability-read precedent).
pub struct PaymentPoolRead;

#[async_trait::async_trait]
impl CandidatePoolPort for PaymentPoolRead {
    async fn open_candidates(
        &self,
        pool: &PgPool,
        bank_account_id: Uuid,
    ) -> Result<Vec<OpenCandidate>, BankingError> {
        let present: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('payment.payment_entries')::text")
                .fetch_optional(pool)
                .await
                .map_err(BankingError::Db)?
                .flatten();
        if present.is_none() {
            return Err(BankingError::CandidatePoolRefused(
                "payment.payment_entries is absent — the candidate pool cannot be read".into(),
            ));
        }
        // Tenancy (ADR-0029): relay the AMBIENT request org scope onto a short transaction and
        // read there. A raw pool fetch binds nothing on the connection, so whichever fence the
        // payment schema carries (its legacy company lane today, its decorator's org fence after
        // its own strip) would filter every row and the pool would read as empty — the ambient
        // scope binds all the fence variables the remote schema evaluates. No scope bound
        // (undecorated deployment) reads unfenced, by design.
        //
        // Join-key note: payment's `bank_account_id` is a GL ACCOUNT reference (its model says
        // "Bank/Cash GL account"), not a banking bank-account id — comparing the two ids directly
        // matches nothing, ever. A payment destined for this bank account touches one of ITS two
        // GL accounts: a cash-style payment lands on the bank GL, a transfer sits in the clearing
        // GL until a clearance moves it. The pool therefore matches payments whose GL is either.
        let mut tx = pool.begin().await.map_err(BankingError::Db)?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut tx, &scope)
                .await
                .map_err(BankingError::Db)?;
        }
        let rows = sqlx::query(
            r#"SELECT id, payment_number, paid_amount, posting_date, reference_no
                   FROM payment.payment_entries
                   WHERE bank_account_id IN (
                       SELECT gl_account_id FROM banking.bank_accounts WHERE id=$1
                       UNION
                       SELECT clearing_account_id FROM banking.bank_accounts WHERE id=$1
                     )
                     AND posting_state='posted' AND status IN ('in_flight','paid')
                     AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(bank_account_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(BankingError::Db)?;
        tx.commit().await.map_err(BankingError::Db)?;
        Ok(rows
            .into_iter()
            .map(|r| OpenCandidate {
                payment_id: r.get("id"),
                payment_number: r.get("payment_number"),
                paid_amount: r.get("paid_amount"),
                posting_date: r.get("posting_date"),
                reference_no: r.get("reference_no"),
            })
            .collect())
    }
}

impl BankingWriteService {
    /// Order the candidates for one statement line under one preset. Pure ranking over reads —
    /// no writes, no watermark, no events. Deterministic: equal scores break on `payment_number`
    /// ascending, so the same committed state always yields the same list (the ordering probe's
    /// pin).
    pub async fn order_candidates(
        &self,
        preset_id: Uuid,
        line_id: Uuid,
    ) -> Result<Vec<RankedCandidate>, BankingError> {
        // Preset (fenced read, ADR-0029): the read rides the request-dedicated connection carrying
        // the composing service's org scope, so the decorator's row-level fence decides visibility —
        // another unit's preset is indistinguishable from absence, which maps to the 404 a missing
        // preset owes the caller (not the 500 a RowNotFound would surface).
        let preset = org_scope::fetch_optional_row_scoped(
            &self.db_pool,
            sqlx::query(
                r#"SELECT name, match_on::text AS mo, tolerance_percent, days_window, status::text AS st
                   FROM banking.reconcile_presets WHERE id=$1 AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(preset_id),
        )
        .await
        .map_err(BankingError::Db)?
        .ok_or(BankingError::PresetNotFound(preset_id))?;
        let preset_status: String = preset.get("st");
        if preset_status != "active" {
            return Err(BankingError::PresetInactive(preset_id));
        }
        let preset_name: String = preset.get("name");
        let match_on: String = preset.get("mo");
        let tolerance: Option<Decimal> = preset.get("tolerance_percent");
        let days_window: Option<i32> = preset.get("days_window");

        // Line (fenced read, same discipline).
        let row = self
            .repos
            .transactions
            .fetch_candidate_basis(&self.db_pool, line_id)
            .await?
            .ok_or(BankingError::TransactionNotFound(line_id))?;
        let line = CandidateLineBasis {
            bank_account_id: row.bank_account_id,
            deposit: row.deposit,
            withdrawal: row.withdrawal,
            txn_date: row.txn_date,
            reference_no: row.reference_no,
        };

        // Pool + per-candidate already-cleared (banking's own table — same sum the clear verb uses).
        let pool_rows = self
            .candidates
            .open_candidates(&self.db_pool, line.bank_account_id)
            .await?;
        let ids: Vec<Uuid> = pool_rows.iter().map(|c| c.payment_id).collect();
        let cleared = self
            .repos
            .clearances
            .sums_cleared_for_payments(&self.db_pool, &ids)
            .await?;

        let line_amount = if line.deposit > Decimal::ZERO {
            line.deposit
        } else {
            line.withdrawal
        };
        let mut ranked: Vec<RankedCandidate> = Vec::new();
        for c in pool_rows {
            let already = cleared.get(&c.payment_id).copied().unwrap_or(Decimal::ZERO);
            let open = c.paid_amount - already;
            if open <= Decimal::ZERO {
                continue; // fully cleared against banking's own watermark — not a candidate
            }
            let (score, reason) = match match_on.as_str() {
                "reference_exact" => {
                    let hit = line
                        .reference_no
                        .as_deref()
                        .zip(c.reference_no.as_deref())
                        .map(|(l, p)| l == p)
                        .unwrap_or(false);
                    if !hit {
                        continue;
                    }
                    (
                        100,
                        format!(
                            "reference_exact: line ref '{}' equals payment ref '{}'",
                            line.reference_no.clone().unwrap_or_default(),
                            c.reference_no.clone().unwrap_or_default()
                        ),
                    )
                }
                "amount_exact" => {
                    if open != line_amount {
                        continue;
                    }
                    (
                        90,
                        format!("amount_exact: open {open} equals line amount {line_amount}"),
                    )
                }
                "amount_within_tolerance" => {
                    let tol = tolerance.ok_or(BankingError::PresetMissingTolerance(preset_id))?;
                    if line_amount <= Decimal::ZERO {
                        continue;
                    }
                    let deviation = ((open - line_amount).abs() / line_amount * Decimal::from(100))
                        .round_dp_with_strategy(6, RoundingStrategy::MidpointAwayFromZero);
                    if deviation > tol {
                        continue;
                    }
                    (
                        80,
                        format!(
                            "amount_within_tolerance: open {open} deviates {deviation}% from line {line_amount} (tolerance {tol}%)"
                        ),
                    )
                }
                "days_window" => {
                    let window =
                        days_window.ok_or(BankingError::PresetMissingDaysWindow(preset_id))? as i64;
                    let delta = (c.posting_date - line.txn_date).num_days().abs();
                    if delta > window {
                        continue;
                    }
                    (70, format!("days_window: payment dated {} is {delta}d from line date {} (window {window}d)", c.posting_date, line.txn_date))
                }
                // Vocabulary-only until a composition supplies line counterparties: rank nothing,
                // say why — an empty silent list reads as "no candidates", which would be a lie.
                "party" => {
                    continue;
                }
                other => return Err(BankingError::UnknownMatchOn(other.to_string())),
            };
            ranked.push(RankedCandidate {
                payment_id: c.payment_id,
                payment_number: c.payment_number,
                open_amount: open,
                posting_date: c.posting_date,
                score,
                reason: format!("[{preset_name}] {reason}"),
            });
        }
        ranked.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.payment_number.cmp(&b.payment_number))
        });
        Ok(ranked)
    }
}
