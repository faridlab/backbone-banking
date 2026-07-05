use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "txn_status", rename_all = "snake_case")]
pub enum TxnStatus {
    Unreconciled,
    PartlyReconciled,
    Reconciled,
    Ignored,
}

impl std::fmt::Display for TxnStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreconciled => write!(f, "unreconciled"),
            Self::PartlyReconciled => write!(f, "partly_reconciled"),
            Self::Reconciled => write!(f, "reconciled"),
            Self::Ignored => write!(f, "ignored"),
        }
    }
}

impl FromStr for TxnStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "unreconciled" => Ok(Self::Unreconciled),
            "partly_reconciled" => Ok(Self::PartlyReconciled),
            "reconciled" => Ok(Self::Reconciled),
            "ignored" => Ok(Self::Ignored),
            _ => Err(format!("Unknown TxnStatus variant: {}", s)),
        }
    }
}

impl Default for TxnStatus {
    fn default() -> Self {
        Self::Unreconciled
    }
}
