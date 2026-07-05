use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::TxnStatus;
use super::AuditMetadata;

/// Strongly-typed ID for BankTransaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BankTransactionId(pub Uuid);

impl BankTransactionId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BankTransactionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BankTransactionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BankTransactionId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BankTransactionId> for Uuid {
    fn from(id: BankTransactionId) -> Self { id.0 }
}

impl AsRef<Uuid> for BankTransactionId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BankTransactionId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BankTransaction {
    pub id: Uuid,
    pub company_id: Uuid,
    pub bank_account_id: Uuid,
    pub import_id: Uuid,
    pub txn_date: NaiveDate,
    pub value_date: Option<NaiveDate>,
    pub description: Option<String>,
    pub reference_no: Option<String>,
    pub deposit: Decimal,
    pub withdrawal: Decimal,
    pub currency: String,
    pub status: TxnStatus,
    pub allocated_amount: Decimal,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BankTransaction {
    /// Create a builder for BankTransaction
    pub fn builder() -> BankTransactionBuilder {
        BankTransactionBuilder::default()
    }

    /// Create a new BankTransaction with required fields
    pub fn new(company_id: Uuid, bank_account_id: Uuid, import_id: Uuid, txn_date: NaiveDate, deposit: Decimal, withdrawal: Decimal, currency: String, status: TxnStatus, allocated_amount: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            bank_account_id,
            import_id,
            txn_date,
            value_date: None,
            description: None,
            reference_no: None,
            deposit,
            withdrawal,
            currency,
            status,
            allocated_amount,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BankTransactionId {
        BankTransactionId(self.id)
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
    pub fn status(&self) -> &TxnStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the value_date field (chainable)
    pub fn with_value_date(mut self, value: NaiveDate) -> Self {
        self.value_date = Some(value);
        self
    }

    /// Set the description field (chainable)
    pub fn with_description(mut self, value: String) -> Self {
        self.description = Some(value);
        self
    }

    /// Set the reference_no field (chainable)
    pub fn with_reference_no(mut self, value: String) -> Self {
        self.reference_no = Some(value);
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
                "import_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.import_id = v; }
                }
                "txn_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.txn_date = v; }
                }
                "value_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.value_date = v; }
                }
                "description" => {
                    if let Ok(v) = serde_json::from_value(value) { self.description = v; }
                }
                "reference_no" => {
                    if let Ok(v) = serde_json::from_value(value) { self.reference_no = v; }
                }
                "deposit" => {
                    if let Ok(v) = serde_json::from_value(value) { self.deposit = v; }
                }
                "withdrawal" => {
                    if let Ok(v) = serde_json::from_value(value) { self.withdrawal = v; }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) { self.currency = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "allocated_amount" => {
                    if let Ok(v) = serde_json::from_value(value) { self.allocated_amount = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BankTransaction {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BankTransaction"
    }
}

impl backbone_core::PersistentEntity for BankTransaction {
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

impl backbone_orm::EntityRepoMeta for BankTransaction {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bank_account_id".to_string(), "uuid".to_string());
        m.insert("import_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "txn_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["currency"]
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("import", "bank_statement_imports", "importId")]
    }
}

/// Builder for BankTransaction entity
///
/// Provides a fluent API for constructing BankTransaction instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BankTransactionBuilder {
    company_id: Option<Uuid>,
    bank_account_id: Option<Uuid>,
    import_id: Option<Uuid>,
    txn_date: Option<NaiveDate>,
    value_date: Option<NaiveDate>,
    description: Option<String>,
    reference_no: Option<String>,
    deposit: Option<Decimal>,
    withdrawal: Option<Decimal>,
    currency: Option<String>,
    status: Option<TxnStatus>,
    allocated_amount: Option<Decimal>,
}

impl BankTransactionBuilder {
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

    /// Set the import_id field (required)
    pub fn import_id(mut self, value: Uuid) -> Self {
        self.import_id = Some(value);
        self
    }

    /// Set the txn_date field (required)
    pub fn txn_date(mut self, value: NaiveDate) -> Self {
        self.txn_date = Some(value);
        self
    }

    /// Set the value_date field (optional)
    pub fn value_date(mut self, value: NaiveDate) -> Self {
        self.value_date = Some(value);
        self
    }

    /// Set the description field (optional)
    pub fn description(mut self, value: String) -> Self {
        self.description = Some(value);
        self
    }

    /// Set the reference_no field (optional)
    pub fn reference_no(mut self, value: String) -> Self {
        self.reference_no = Some(value);
        self
    }

    /// Set the deposit field (default: `Decimal::from(0)`)
    pub fn deposit(mut self, value: Decimal) -> Self {
        self.deposit = Some(value);
        self
    }

    /// Set the withdrawal field (default: `Decimal::from(0)`)
    pub fn withdrawal(mut self, value: Decimal) -> Self {
        self.withdrawal = Some(value);
        self
    }

    /// Set the currency field (default: `"IDR".to_string()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the status field (default: `TxnStatus::default()`)
    pub fn status(mut self, value: TxnStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the allocated_amount field (default: `Decimal::from(0)`)
    pub fn allocated_amount(mut self, value: Decimal) -> Self {
        self.allocated_amount = Some(value);
        self
    }

    /// Build the BankTransaction entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BankTransaction, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let bank_account_id = self.bank_account_id.ok_or_else(|| "bank_account_id is required".to_string())?;
        let import_id = self.import_id.ok_or_else(|| "import_id is required".to_string())?;
        let txn_date = self.txn_date.ok_or_else(|| "txn_date is required".to_string())?;

        Ok(BankTransaction {
            id: Uuid::new_v4(),
            company_id,
            bank_account_id,
            import_id,
            txn_date,
            value_date: self.value_date,
            description: self.description,
            reference_no: self.reference_no,
            deposit: self.deposit.unwrap_or(Decimal::from(0)),
            withdrawal: self.withdrawal.unwrap_or(Decimal::from(0)),
            currency: self.currency.unwrap_or("IDR".to_string()),
            status: self.status.unwrap_or(TxnStatus::default()),
            allocated_amount: self.allocated_amount.unwrap_or(Decimal::from(0)),
            metadata: AuditMetadata::default(),
        })
    }
}
