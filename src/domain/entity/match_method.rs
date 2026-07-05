use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "match_method", rename_all = "snake_case")]
pub enum MatchMethod {
    Manual,
    Exact,
    Fuzzy,
    AutoRule,
}

impl std::fmt::Display for MatchMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manual => write!(f, "manual"),
            Self::Exact => write!(f, "exact"),
            Self::Fuzzy => write!(f, "fuzzy"),
            Self::AutoRule => write!(f, "auto_rule"),
        }
    }
}

impl FromStr for MatchMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "manual" => Ok(Self::Manual),
            "exact" => Ok(Self::Exact),
            "fuzzy" => Ok(Self::Fuzzy),
            "auto_rule" => Ok(Self::AutoRule),
            _ => Err(format!("Unknown MatchMethod variant: {}", s)),
        }
    }
}

impl Default for MatchMethod {
    fn default() -> Self {
        Self::Manual
    }
}
