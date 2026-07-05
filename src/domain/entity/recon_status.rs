use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "recon_status", rename_all = "snake_case")]
pub enum ReconStatus {
    Open,
    Balanced,
    Closed,
}

impl std::fmt::Display for ReconStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Balanced => write!(f, "balanced"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

impl FromStr for ReconStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "balanced" => Ok(Self::Balanced),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("Unknown ReconStatus variant: {}", s)),
        }
    }
}

impl Default for ReconStatus {
    fn default() -> Self {
        Self::Open
    }
}
