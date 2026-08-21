use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AuditMetadata;
use super::MatchOn;
use super::PresetStatus;

/// Strongly-typed ID for ReconcilePreset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReconcilePresetId(pub Uuid);

impl ReconcilePresetId {
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

impl std::fmt::Display for ReconcilePresetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ReconcilePresetId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for ReconcilePresetId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<ReconcilePresetId> for Uuid {
    fn from(id: ReconcilePresetId) -> Self {
        id.0
    }
}

impl AsRef<Uuid> for ReconcilePresetId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::ops::Deref for ReconcilePresetId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ReconcilePreset {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub note: Option<String>,
    pub priority: i32,
    pub match_on: MatchOn,
    pub tolerance_percent: Option<Decimal>,
    pub days_window: Option<i32>,
    pub status: PresetStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl ReconcilePreset {
    /// Create a builder for ReconcilePreset
    pub fn builder() -> ReconcilePresetBuilder {
        <ReconcilePresetBuilder as Default>::default()
    }

    /// Create a new ReconcilePreset with required fields
    pub fn new(
        company_id: Uuid,
        name: String,
        priority: i32,
        match_on: MatchOn,
        status: PresetStatus,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            name,
            note: None,
            priority,
            match_on,
            tolerance_percent: None,
            days_window: None,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> ReconcilePresetId {
        ReconcilePresetId(self.id)
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
    pub fn status(&self) -> &PresetStatus {
        &self.status
    }

    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the note field (chainable)
    pub fn with_note(mut self, value: String) -> Self {
        self.note = Some(value);
        self
    }

    /// Set the tolerance_percent field (chainable)
    pub fn with_tolerance_percent(mut self, value: Decimal) -> Self {
        self.tolerance_percent = Some(value);
        self
    }

    /// Set the days_window field (chainable)
    pub fn with_days_window(mut self, value: i32) -> Self {
        self.days_window = Some(value);
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
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.name = v;
                    }
                }
                "note" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.note = v;
                    }
                }
                "priority" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.priority = v;
                    }
                }
                "match_on" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.match_on = v;
                    }
                }
                "tolerance_percent" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.tolerance_percent = v;
                    }
                }
                "days_window" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.days_window = v;
                    }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.status = v;
                    }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for ReconcilePreset {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "ReconcilePreset"
    }
}

impl backbone_core::PersistentEntity for ReconcilePreset {
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

impl backbone_orm::EntityRepoMeta for ReconcilePreset {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("match_on".to_string(), "match_on".to_string());
        m.insert("status".to_string(), "preset_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["name"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for ReconcilePreset entity
///
/// Provides a fluent API for constructing ReconcilePreset instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct ReconcilePresetBuilder {
    company_id: Option<Uuid>,
    name: Option<String>,
    note: Option<String>,
    priority: Option<i32>,
    match_on: Option<MatchOn>,
    tolerance_percent: Option<Decimal>,
    days_window: Option<i32>,
    status: Option<PresetStatus>,
}

impl ReconcilePresetBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the name field (required)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the note field (optional)
    pub fn note(mut self, value: String) -> Self {
        self.note = Some(value);
        self
    }

    /// Set the priority field (default: `100`)
    pub fn priority(mut self, value: i32) -> Self {
        self.priority = Some(value);
        self
    }

    /// Set the match_on field (required)
    pub fn match_on(mut self, value: MatchOn) -> Self {
        self.match_on = Some(value);
        self
    }

    /// Set the tolerance_percent field (optional)
    pub fn tolerance_percent(mut self, value: Decimal) -> Self {
        self.tolerance_percent = Some(value);
        self
    }

    /// Set the days_window field (optional)
    pub fn days_window(mut self, value: i32) -> Self {
        self.days_window = Some(value);
        self
    }

    /// Set the status field (default: `PresetStatus::default()`)
    pub fn status(mut self, value: PresetStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the ReconcilePreset entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<ReconcilePreset, String> {
        let company_id = self
            .company_id
            .ok_or_else(|| "company_id is required".to_string())?;
        let name = self.name.ok_or_else(|| "name is required".to_string())?;
        let match_on = self
            .match_on
            .ok_or_else(|| "match_on is required".to_string())?;

        Ok(ReconcilePreset {
            id: Uuid::new_v4(),
            company_id,
            name,
            note: self.note,
            priority: self.priority.unwrap_or(100),
            match_on,
            tolerance_percent: self.tolerance_percent,
            days_window: self.days_window,
            status: self.status.unwrap_or_default(),
            metadata: AuditMetadata::default(),
        })
    }
}
