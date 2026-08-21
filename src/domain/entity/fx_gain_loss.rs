use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AuditMetadata;
use super::FxDirection;

/// Strongly-typed ID for FxGainLoss
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FxGainLossId(pub Uuid);

impl FxGainLossId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
    pub fn into_inner(self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for FxGainLossId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for FxGainLossId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for FxGainLossId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<FxGainLossId> for Uuid {
    fn from(id: FxGainLossId) -> Self {
        id.0
    }
}

impl AsRef<Uuid> for FxGainLossId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::ops::Deref for FxGainLossId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FxGainLoss {
    pub id: Uuid,
    pub company_id: Uuid,
    pub bank_clearance_id: Option<Uuid>,
    pub matched_source_id: Uuid,
    pub currency: String,
    pub original_rate: Decimal,
    pub realised_rate: Decimal,
    pub base_amount_delta: Decimal,
    pub direction: FxDirection,
    pub fx_account_id: Uuid,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl FxGainLoss {
    /// Create a builder for FxGainLoss
    pub fn builder() -> FxGainLossBuilder {
        <FxGainLossBuilder as Default>::default()
    }

    /// Create a new FxGainLoss with required fields
    pub fn new(
        company_id: Uuid,
        matched_source_id: Uuid,
        currency: String,
        original_rate: Decimal,
        realised_rate: Decimal,
        base_amount_delta: Decimal,
        direction: FxDirection,
        fx_account_id: Uuid,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            bank_clearance_id: None,
            matched_source_id,
            currency,
            original_rate,
            realised_rate,
            base_amount_delta,
            direction,
            fx_account_id,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> FxGainLossId {
        FxGainLossId(self.id)
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

    /// Set the bank_clearance_id field (chainable)
    pub fn with_bank_clearance_id(mut self, value: Uuid) -> Self {
        self.bank_clearance_id = Some(value);
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
                    if let Ok(v) = serde_json::from_value(value) {
                        self.company_id = v;
                    }
                }
                "bank_clearance_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.bank_clearance_id = v;
                    }
                }
                "matched_source_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.matched_source_id = v;
                    }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.currency = v;
                    }
                }
                "original_rate" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.original_rate = v;
                    }
                }
                "realised_rate" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.realised_rate = v;
                    }
                }
                "base_amount_delta" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.base_amount_delta = v;
                    }
                }
                "direction" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.direction = v;
                    }
                }
                "fx_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.fx_account_id = v;
                    }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for FxGainLoss {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "FxGainLoss"
    }
}

impl backbone_core::PersistentEntity for FxGainLoss {
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

impl backbone_orm::EntityRepoMeta for FxGainLoss {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bank_clearance_id".to_string(), "uuid".to_string());
        m.insert("matched_source_id".to_string(), "uuid".to_string());
        m.insert("fx_account_id".to_string(), "uuid".to_string());
        m.insert("direction".to_string(), "fx_direction".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["currency"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for FxGainLoss entity
///
/// Provides a fluent API for constructing FxGainLoss instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct FxGainLossBuilder {
    company_id: Option<Uuid>,
    bank_clearance_id: Option<Uuid>,
    matched_source_id: Option<Uuid>,
    currency: Option<String>,
    original_rate: Option<Decimal>,
    realised_rate: Option<Decimal>,
    base_amount_delta: Option<Decimal>,
    direction: Option<FxDirection>,
    fx_account_id: Option<Uuid>,
}

impl FxGainLossBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the bank_clearance_id field (optional)
    pub fn bank_clearance_id(mut self, value: Uuid) -> Self {
        self.bank_clearance_id = Some(value);
        self
    }

    /// Set the matched_source_id field (required)
    pub fn matched_source_id(mut self, value: Uuid) -> Self {
        self.matched_source_id = Some(value);
        self
    }

    /// Set the currency field (required)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the original_rate field (required)
    pub fn original_rate(mut self, value: Decimal) -> Self {
        self.original_rate = Some(value);
        self
    }

    /// Set the realised_rate field (required)
    pub fn realised_rate(mut self, value: Decimal) -> Self {
        self.realised_rate = Some(value);
        self
    }

    /// Set the base_amount_delta field (required)
    pub fn base_amount_delta(mut self, value: Decimal) -> Self {
        self.base_amount_delta = Some(value);
        self
    }

    /// Set the direction field (required)
    pub fn direction(mut self, value: FxDirection) -> Self {
        self.direction = Some(value);
        self
    }

    /// Set the fx_account_id field (required)
    pub fn fx_account_id(mut self, value: Uuid) -> Self {
        self.fx_account_id = Some(value);
        self
    }

    /// Build the FxGainLoss entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<FxGainLoss, String> {
        let company_id = self
            .company_id
            .ok_or_else(|| "company_id is required".to_string())?;
        let matched_source_id = self
            .matched_source_id
            .ok_or_else(|| "matched_source_id is required".to_string())?;
        let currency = self
            .currency
            .ok_or_else(|| "currency is required".to_string())?;
        let original_rate = self
            .original_rate
            .ok_or_else(|| "original_rate is required".to_string())?;
        let realised_rate = self
            .realised_rate
            .ok_or_else(|| "realised_rate is required".to_string())?;
        let base_amount_delta = self
            .base_amount_delta
            .ok_or_else(|| "base_amount_delta is required".to_string())?;
        let direction = self
            .direction
            .ok_or_else(|| "direction is required".to_string())?;
        let fx_account_id = self
            .fx_account_id
            .ok_or_else(|| "fx_account_id is required".to_string())?;

        Ok(FxGainLoss {
            id: Uuid::new_v4(),
            company_id,
            bank_clearance_id: self.bank_clearance_id,
            matched_source_id,
            currency,
            original_rate,
            realised_rate,
            base_amount_delta,
            direction,
            fx_account_id,
            metadata: AuditMetadata::default(),
        })
    }
}
