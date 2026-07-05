use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "bank_account_type", rename_all = "snake_case")]
pub enum BankAccountType {
    Checking,
    Savings,
    VirtualAccount,
    Wallet,
    Escrow,
}

impl std::fmt::Display for BankAccountType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Checking => write!(f, "checking"),
            Self::Savings => write!(f, "savings"),
            Self::VirtualAccount => write!(f, "virtual_account"),
            Self::Wallet => write!(f, "wallet"),
            Self::Escrow => write!(f, "escrow"),
        }
    }
}

impl FromStr for BankAccountType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "checking" => Ok(Self::Checking),
            "savings" => Ok(Self::Savings),
            "virtual_account" => Ok(Self::VirtualAccount),
            "wallet" => Ok(Self::Wallet),
            "escrow" => Ok(Self::Escrow),
            _ => Err(format!("Unknown BankAccountType variant: {}", s)),
        }
    }
}

impl Default for BankAccountType {
    fn default() -> Self {
        Self::Checking
    }
}
