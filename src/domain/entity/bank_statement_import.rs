use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::SourceFormat;
use super::ImportStatus;
use super::AuditMetadata;

/// Strongly-typed ID for BankStatementImport
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BankStatementImportId(pub Uuid);

impl BankStatementImportId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BankStatementImportId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BankStatementImportId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BankStatementImportId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BankStatementImportId> for Uuid {
    fn from(id: BankStatementImportId) -> Self { id.0 }
}

impl AsRef<Uuid> for BankStatementImportId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BankStatementImportId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BankStatementImport {
    pub id: Uuid,
    pub company_id: Uuid,
    pub bank_account_id: Uuid,
    pub source_format: SourceFormat,
    pub statement_period_start: NaiveDate,
    pub statement_period_end: NaiveDate,
    pub opening_balance: Decimal,
    pub closing_balance: Decimal,
    pub file_ref: Option<String>,
    pub status: ImportStatus,
    pub row_count: i32,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BankStatementImport {
    /// Create a builder for BankStatementImport
    pub fn builder() -> BankStatementImportBuilder {
        BankStatementImportBuilder::default()
    }

    /// Create a new BankStatementImport with required fields
    pub fn new(company_id: Uuid, bank_account_id: Uuid, source_format: SourceFormat, statement_period_start: NaiveDate, statement_period_end: NaiveDate, opening_balance: Decimal, closing_balance: Decimal, status: ImportStatus, row_count: i32) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            bank_account_id,
            source_format,
            statement_period_start,
            statement_period_end,
            opening_balance,
            closing_balance,
            file_ref: None,
            status,
            row_count,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BankStatementImportId {
        BankStatementImportId(self.id)
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
    pub fn status(&self) -> &ImportStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the file_ref field (chainable)
    pub fn with_file_ref(mut self, value: String) -> Self {
        self.file_ref = Some(value);
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
                "bank_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bank_account_id = v; }
                }
                "source_format" => {
                    if let Ok(v) = serde_json::from_value(value) { self.source_format = v; }
                }
                "statement_period_start" => {
                    if let Ok(v) = serde_json::from_value(value) { self.statement_period_start = v; }
                }
                "statement_period_end" => {
                    if let Ok(v) = serde_json::from_value(value) { self.statement_period_end = v; }
                }
                "opening_balance" => {
                    if let Ok(v) = serde_json::from_value(value) { self.opening_balance = v; }
                }
                "closing_balance" => {
                    if let Ok(v) = serde_json::from_value(value) { self.closing_balance = v; }
                }
                "file_ref" => {
                    if let Ok(v) = serde_json::from_value(value) { self.file_ref = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "row_count" => {
                    if let Ok(v) = serde_json::from_value(value) { self.row_count = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BankStatementImport {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BankStatementImport"
    }
}

impl backbone_core::PersistentEntity for BankStatementImport {
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

impl backbone_orm::EntityRepoMeta for BankStatementImport {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bank_account_id".to_string(), "uuid".to_string());
        m.insert("source_format".to_string(), "source_format".to_string());
        m.insert("status".to_string(), "import_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Builder for BankStatementImport entity
///
/// Provides a fluent API for constructing BankStatementImport instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BankStatementImportBuilder {
    company_id: Option<Uuid>,
    bank_account_id: Option<Uuid>,
    source_format: Option<SourceFormat>,
    statement_period_start: Option<NaiveDate>,
    statement_period_end: Option<NaiveDate>,
    opening_balance: Option<Decimal>,
    closing_balance: Option<Decimal>,
    file_ref: Option<String>,
    status: Option<ImportStatus>,
    row_count: Option<i32>,
}

impl BankStatementImportBuilder {
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

    /// Set the source_format field (default: `SourceFormat::default()`)
    pub fn source_format(mut self, value: SourceFormat) -> Self {
        self.source_format = Some(value);
        self
    }

    /// Set the statement_period_start field (required)
    pub fn statement_period_start(mut self, value: NaiveDate) -> Self {
        self.statement_period_start = Some(value);
        self
    }

    /// Set the statement_period_end field (required)
    pub fn statement_period_end(mut self, value: NaiveDate) -> Self {
        self.statement_period_end = Some(value);
        self
    }

    /// Set the opening_balance field (default: `Decimal::from(0)`)
    pub fn opening_balance(mut self, value: Decimal) -> Self {
        self.opening_balance = Some(value);
        self
    }

    /// Set the closing_balance field (default: `Decimal::from(0)`)
    pub fn closing_balance(mut self, value: Decimal) -> Self {
        self.closing_balance = Some(value);
        self
    }

    /// Set the file_ref field (optional)
    pub fn file_ref(mut self, value: String) -> Self {
        self.file_ref = Some(value);
        self
    }

    /// Set the status field (default: `ImportStatus::default()`)
    pub fn status(mut self, value: ImportStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the row_count field (default: `0`)
    pub fn row_count(mut self, value: i32) -> Self {
        self.row_count = Some(value);
        self
    }

    /// Build the BankStatementImport entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BankStatementImport, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let bank_account_id = self.bank_account_id.ok_or_else(|| "bank_account_id is required".to_string())?;
        let statement_period_start = self.statement_period_start.ok_or_else(|| "statement_period_start is required".to_string())?;
        let statement_period_end = self.statement_period_end.ok_or_else(|| "statement_period_end is required".to_string())?;

        Ok(BankStatementImport {
            id: Uuid::new_v4(),
            company_id,
            bank_account_id,
            source_format: self.source_format.unwrap_or(SourceFormat::default()),
            statement_period_start,
            statement_period_end,
            opening_balance: self.opening_balance.unwrap_or(Decimal::from(0)),
            closing_balance: self.closing_balance.unwrap_or(Decimal::from(0)),
            file_ref: self.file_ref,
            status: self.status.unwrap_or(ImportStatus::default()),
            row_count: self.row_count.unwrap_or(0),
            metadata: AuditMetadata::default(),
        })
    }
}
