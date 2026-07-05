use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::BankAccountType;
use super::AuditMetadata;

/// Strongly-typed ID for BankAccount
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BankAccountId(pub Uuid);

impl BankAccountId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BankAccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BankAccountId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BankAccountId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BankAccountId> for Uuid {
    fn from(id: BankAccountId) -> Self { id.0 }
}

impl AsRef<Uuid> for BankAccountId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BankAccountId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BankAccount {
    pub id: Uuid,
    pub company_id: Uuid,
    pub branch_id: Option<Uuid>,
    pub bank_id: Uuid,
    pub account_name: String,
    pub account_number: String,
    pub gl_account_id: Uuid,
    pub clearing_account_id: Uuid,
    pub currency: String,
    pub account_type: BankAccountType,
    pub is_default: bool,
    pub is_active: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BankAccount {
    /// Create a builder for BankAccount
    pub fn builder() -> BankAccountBuilder {
        BankAccountBuilder::default()
    }

    /// Create a new BankAccount with required fields
    pub fn new(company_id: Uuid, bank_id: Uuid, account_name: String, account_number: String, gl_account_id: Uuid, clearing_account_id: Uuid, currency: String, account_type: BankAccountType, is_default: bool, is_active: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            branch_id: None,
            bank_id,
            account_name,
            account_number,
            gl_account_id,
            clearing_account_id,
            currency,
            account_type,
            is_default,
            is_active,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BankAccountId {
        BankAccountId(self.id)
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

    /// Set the branch_id field (chainable)
    pub fn with_branch_id(mut self, value: Uuid) -> Self {
        self.branch_id = Some(value);
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
                "branch_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.branch_id = v; }
                }
                "bank_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bank_id = v; }
                }
                "account_name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.account_name = v; }
                }
                "account_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.account_number = v; }
                }
                "gl_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.gl_account_id = v; }
                }
                "clearing_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.clearing_account_id = v; }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.currency = v; }
                }
                "account_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.account_type = v; }
                }
                "is_default" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_default = v; }
                }
                "is_active" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_active = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BankAccount {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BankAccount"
    }
}

impl backbone_core::PersistentEntity for BankAccount {
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

impl backbone_orm::EntityRepoMeta for BankAccount {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("branch_id".to_string(), "uuid".to_string());
        m.insert("bank_id".to_string(), "uuid".to_string());
        m.insert("gl_account_id".to_string(), "uuid".to_string());
        m.insert("clearing_account_id".to_string(), "uuid".to_string());
        m.insert("account_type".to_string(), "bank_account_type".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["account_name", "account_number", "currency"]
    }
}

/// Builder for BankAccount entity
///
/// Provides a fluent API for constructing BankAccount instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BankAccountBuilder {
    company_id: Option<Uuid>,
    branch_id: Option<Uuid>,
    bank_id: Option<Uuid>,
    account_name: Option<String>,
    account_number: Option<String>,
    gl_account_id: Option<Uuid>,
    clearing_account_id: Option<Uuid>,
    currency: Option<String>,
    account_type: Option<BankAccountType>,
    is_default: Option<bool>,
    is_active: Option<bool>,
}

impl BankAccountBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the branch_id field (optional)
    pub fn branch_id(mut self, value: Uuid) -> Self {
        self.branch_id = Some(value);
        self
    }

    /// Set the bank_id field (required)
    pub fn bank_id(mut self, value: Uuid) -> Self {
        self.bank_id = Some(value);
        self
    }

    /// Set the account_name field (required)
    pub fn account_name(mut self, value: String) -> Self {
        self.account_name = Some(value);
        self
    }

    /// Set the account_number field (required)
    pub fn account_number(mut self, value: String) -> Self {
        self.account_number = Some(value);
        self
    }

    /// Set the gl_account_id field (required)
    pub fn gl_account_id(mut self, value: Uuid) -> Self {
        self.gl_account_id = Some(value);
        self
    }

    /// Set the clearing_account_id field (required)
    pub fn clearing_account_id(mut self, value: Uuid) -> Self {
        self.clearing_account_id = Some(value);
        self
    }

    /// Set the currency field (default: `"IDR".to_string()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the account_type field (default: `BankAccountType::default()`)
    pub fn account_type(mut self, value: BankAccountType) -> Self {
        self.account_type = Some(value);
        self
    }

    /// Set the is_default field (default: `false`)
    pub fn is_default(mut self, value: bool) -> Self {
        self.is_default = Some(value);
        self
    }

    /// Set the is_active field (default: `true`)
    pub fn is_active(mut self, value: bool) -> Self {
        self.is_active = Some(value);
        self
    }

    /// Build the BankAccount entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BankAccount, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let bank_id = self.bank_id.ok_or_else(|| "bank_id is required".to_string())?;
        let account_name = self.account_name.ok_or_else(|| "account_name is required".to_string())?;
        let account_number = self.account_number.ok_or_else(|| "account_number is required".to_string())?;
        let gl_account_id = self.gl_account_id.ok_or_else(|| "gl_account_id is required".to_string())?;
        let clearing_account_id = self.clearing_account_id.ok_or_else(|| "clearing_account_id is required".to_string())?;

        Ok(BankAccount {
            id: Uuid::new_v4(),
            company_id,
            branch_id: self.branch_id,
            bank_id,
            account_name,
            account_number,
            gl_account_id,
            clearing_account_id,
            currency: self.currency.unwrap_or("IDR".to_string()),
            account_type: self.account_type.unwrap_or(BankAccountType::default()),
            is_default: self.is_default.unwrap_or(false),
            is_active: self.is_active.unwrap_or(true),
            metadata: AuditMetadata::default(),
        })
    }
}
