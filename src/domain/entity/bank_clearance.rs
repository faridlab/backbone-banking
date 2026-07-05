use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::MatchedSourceType;
use super::MatchMethod;
use super::AuditMetadata;

/// Strongly-typed ID for BankClearance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BankClearanceId(pub Uuid);

impl BankClearanceId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BankClearanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BankClearanceId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BankClearanceId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BankClearanceId> for Uuid {
    fn from(id: BankClearanceId) -> Self { id.0 }
}

impl AsRef<Uuid> for BankClearanceId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BankClearanceId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BankClearance {
    pub id: Uuid,
    pub company_id: Uuid,
    pub bank_transaction_id: Uuid,
    pub matched_source_type: MatchedSourceType,
    pub matched_source_id: Uuid,
    pub matched_amount: Decimal,
    pub match_method: MatchMethod,
    pub clearance_date: NaiveDate,
    pub accounting_post_id: Option<Uuid>,
    pub journal_id: Option<Uuid>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BankClearance {
    /// Create a builder for BankClearance
    pub fn builder() -> BankClearanceBuilder {
        BankClearanceBuilder::default()
    }

    /// Create a new BankClearance with required fields
    pub fn new(company_id: Uuid, bank_transaction_id: Uuid, matched_source_type: MatchedSourceType, matched_source_id: Uuid, matched_amount: Decimal, match_method: MatchMethod, clearance_date: NaiveDate) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            bank_transaction_id,
            matched_source_type,
            matched_source_id,
            matched_amount,
            match_method,
            clearance_date,
            accounting_post_id: None,
            journal_id: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BankClearanceId {
        BankClearanceId(self.id)
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


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the accounting_post_id field (chainable)
    pub fn with_accounting_post_id(mut self, value: Uuid) -> Self {
        self.accounting_post_id = Some(value);
        self
    }

    /// Set the journal_id field (chainable)
    pub fn with_journal_id(mut self, value: Uuid) -> Self {
        self.journal_id = Some(value);
        self
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
                "bank_transaction_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bank_transaction_id = v; }
                }
                "matched_source_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.matched_source_type = v; }
                }
                "matched_source_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.matched_source_id = v; }
                }
                "matched_amount" => {
                    if let Ok(v) = serde_json::from_value(value) { self.matched_amount = v; }
                }
                "match_method" => {
                    if let Ok(v) = serde_json::from_value(value) { self.match_method = v; }
                }
                "clearance_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.clearance_date = v; }
                }
                "accounting_post_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.accounting_post_id = v; }
                }
                "journal_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.journal_id = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BankClearance {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BankClearance"
    }
}

impl backbone_core::PersistentEntity for BankClearance {
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

impl backbone_orm::EntityRepoMeta for BankClearance {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bank_transaction_id".to_string(), "uuid".to_string());
        m.insert("matched_source_id".to_string(), "uuid".to_string());
        m.insert("accounting_post_id".to_string(), "uuid".to_string());
        m.insert("journal_id".to_string(), "uuid".to_string());
        m.insert("matched_source_type".to_string(), "matched_source_type".to_string());
        m.insert("match_method".to_string(), "match_method".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Builder for BankClearance entity
///
/// Provides a fluent API for constructing BankClearance instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BankClearanceBuilder {
    company_id: Option<Uuid>,
    bank_transaction_id: Option<Uuid>,
    matched_source_type: Option<MatchedSourceType>,
    matched_source_id: Option<Uuid>,
    matched_amount: Option<Decimal>,
    match_method: Option<MatchMethod>,
    clearance_date: Option<NaiveDate>,
    accounting_post_id: Option<Uuid>,
    journal_id: Option<Uuid>,
}

impl BankClearanceBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the bank_transaction_id field (required)
    pub fn bank_transaction_id(mut self, value: Uuid) -> Self {
        self.bank_transaction_id = Some(value);
        self
    }

    /// Set the matched_source_type field (required)
    pub fn matched_source_type(mut self, value: MatchedSourceType) -> Self {
        self.matched_source_type = Some(value);
        self
    }

    /// Set the matched_source_id field (required)
    pub fn matched_source_id(mut self, value: Uuid) -> Self {
        self.matched_source_id = Some(value);
        self
    }

    /// Set the matched_amount field (required)
    pub fn matched_amount(mut self, value: Decimal) -> Self {
        self.matched_amount = Some(value);
        self
    }

    /// Set the match_method field (default: `MatchMethod::default()`)
    pub fn match_method(mut self, value: MatchMethod) -> Self {
        self.match_method = Some(value);
        self
    }

    /// Set the clearance_date field (required)
    pub fn clearance_date(mut self, value: NaiveDate) -> Self {
        self.clearance_date = Some(value);
        self
    }

    /// Set the accounting_post_id field (optional)
    pub fn accounting_post_id(mut self, value: Uuid) -> Self {
        self.accounting_post_id = Some(value);
        self
    }

    /// Set the journal_id field (optional)
    pub fn journal_id(mut self, value: Uuid) -> Self {
        self.journal_id = Some(value);
        self
    }

    /// Build the BankClearance entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BankClearance, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let bank_transaction_id = self.bank_transaction_id.ok_or_else(|| "bank_transaction_id is required".to_string())?;
        let matched_source_type = self.matched_source_type.ok_or_else(|| "matched_source_type is required".to_string())?;
        let matched_source_id = self.matched_source_id.ok_or_else(|| "matched_source_id is required".to_string())?;
        let matched_amount = self.matched_amount.ok_or_else(|| "matched_amount is required".to_string())?;
        let clearance_date = self.clearance_date.ok_or_else(|| "clearance_date is required".to_string())?;

        Ok(BankClearance {
            id: Uuid::new_v4(),
            company_id,
            bank_transaction_id,
            matched_source_type,
            matched_source_id,
            matched_amount,
            match_method: self.match_method.unwrap_or(MatchMethod::default()),
            clearance_date,
            accounting_post_id: self.accounting_post_id,
            journal_id: self.journal_id,
            metadata: AuditMetadata::default(),
        })
    }
}
