use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "source_format", rename_all = "snake_case")]
pub enum SourceFormat {
    Manual,
    Csv,
    Mt940,
    Camt053,
    Api,
}

impl std::fmt::Display for SourceFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manual => write!(f, "manual"),
            Self::Csv => write!(f, "csv"),
            Self::Mt940 => write!(f, "mt940"),
            Self::Camt053 => write!(f, "camt053"),
            Self::Api => write!(f, "api"),
        }
    }
}

impl FromStr for SourceFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "manual" => Ok(Self::Manual),
            "csv" => Ok(Self::Csv),
            "mt940" => Ok(Self::Mt940),
            "camt053" => Ok(Self::Camt053),
            "api" => Ok(Self::Api),
            _ => Err(format!("Unknown SourceFormat variant: {}", s)),
        }
    }
}

impl Default for SourceFormat {
    fn default() -> Self {
        Self::Manual
    }
}
