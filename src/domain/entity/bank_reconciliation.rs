use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::ReconStatus;
use super::AuditMetadata;

/// Strongly-typed ID for BankReconciliation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BankReconciliationId(pub Uuid);

impl BankReconciliationId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BankReconciliationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BankReconciliationId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BankReconciliationId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BankReconciliationId> for Uuid {
    fn from(id: BankReconciliationId) -> Self { id.0 }
}

impl AsRef<Uuid> for BankReconciliationId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BankReconciliationId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BankReconciliation {
    pub id: Uuid,
    pub company_id: Uuid,
    pub bank_account_id: Uuid,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub statement_closing_balance: Decimal,
    pub ledger_balance: Decimal,
    pub computed_difference: Decimal,
    pub unreconciled_count: i32,
    pub status: ReconStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BankReconciliation {
    /// Create a builder for BankReconciliation
    pub fn builder() -> BankReconciliationBuilder {
        BankReconciliationBuilder::default()
    }

    /// Create a new BankReconciliation with required fields
    pub fn new(company_id: Uuid, bank_account_id: Uuid, from_date: NaiveDate, to_date: NaiveDate, statement_closing_balance: Decimal, ledger_balance: Decimal, computed_difference: Decimal, unreconciled_count: i32, status: ReconStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            bank_account_id,
            from_date,
            to_date,
            statement_closing_balance,
            ledger_balance,
            computed_difference,
            unreconciled_count,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BankReconciliationId {
        BankReconciliationId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }

    /// Get the current status
    pub fn status(&self) -> &ReconStatus {
        &self.status
    }


    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "bank_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bank_account_id = v; }
                }
                "from_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.from_date = v; }
                }
                "to_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.to_date = v; }
                }
                "statement_closing_balance" => {
                    if let Ok(v) = serde_json::from_value(value) { self.statement_closing_balance = v; }
                }
                "ledger_balance" => {
                    if let Ok(v) = serde_json::from_value(value) { self.ledger_balance = v; }
                }
                "computed_difference" => {
                    if let Ok(v) = serde_json::from_value(value) { self.computed_difference = v; }
                }
                "unreconciled_count" => {
                    if let Ok(v) = serde_json::from_value(value) { self.unreconciled_count = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BankReconciliation {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BankReconciliation"
    }
}

impl backbone_core::PersistentEntity for BankReconciliation {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for BankReconciliation {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bank_account_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "recon_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Builder for BankReconciliation entity
///
/// Provides a fluent API for constructing BankReconciliation instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BankReconciliationBuilder {
    company_id: Option<Uuid>,
    bank_account_id: Option<Uuid>,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
    statement_closing_balance: Option<Decimal>,
    ledger_balance: Option<Decimal>,
    computed_difference: Option<Decimal>,
    unreconciled_count: Option<i32>,
    status: Option<ReconStatus>,
}

impl BankReconciliationBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the bank_account_id field (required)
    pub fn bank_account_id(mut self, value: Uuid) -> Self {
        self.bank_account_id = Some(value);
        self
    }

    /// Set the from_date field (required)
    pub fn from_date(mut self, value: NaiveDate) -> Self {
        self.from_date = Some(value);
        self
    }

    /// Set the to_date field (required)
    pub fn to_date(mut self, value: NaiveDate) -> Self {
        self.to_date = Some(value);
        self
    }

    /// Set the statement_closing_balance field (default: `Decimal::from(0)`)
    pub fn statement_closing_balance(mut self, value: Decimal) -> Self {
        self.statement_closing_balance = Some(value);
        self
    }

    /// Set the ledger_balance field (default: `Decimal::from(0)`)
    pub fn ledger_balance(mut self, value: Decimal) -> Self {
        self.ledger_balance = Some(value);
        self
    }

    /// Set the computed_difference field (default: `Decimal::from(0)`)
    pub fn computed_difference(mut self, value: Decimal) -> Self {
        self.computed_difference = Some(value);
        self
    }

    /// Set the unreconciled_count field (default: `0`)
    pub fn unreconciled_count(mut self, value: i32) -> Self {
        self.unreconciled_count = Some(value);
        self
    }

    /// Set the status field (default: `ReconStatus::default()`)
    pub fn status(mut self, value: ReconStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the BankReconciliation entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BankReconciliation, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let bank_account_id = self.bank_account_id.ok_or_else(|| "bank_account_id is required".to_string())?;
        let from_date = self.from_date.ok_or_else(|| "from_date is required".to_string())?;
        let to_date = self.to_date.ok_or_else(|| "to_date is required".to_string())?;

        Ok(BankReconciliation {
            id: Uuid::new_v4(),
            company_id,
            bank_account_id,
            from_date,
            to_date,
            statement_closing_balance: self.statement_closing_balance.unwrap_or(Decimal::from(0)),
            ledger_balance: self.ledger_balance.unwrap_or(Decimal::from(0)),
            computed_difference: self.computed_difference.unwrap_or(Decimal::from(0)),
            unreconciled_count: self.unreconciled_count.unwrap_or(0),
            status: self.status.unwrap_or(ReconStatus::default()),
            metadata: AuditMetadata::default(),
        })
    }
}
