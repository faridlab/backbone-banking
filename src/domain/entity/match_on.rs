use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "match_on", rename_all = "snake_case")]
pub enum MatchOn {
    ReferenceExact,
    AmountExact,
    AmountWithinTolerance,
    Party,
    DaysWindow,
}

impl std::fmt::Display for MatchOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReferenceExact => write!(f, "reference_exact"),
            Self::AmountExact => write!(f, "amount_exact"),
            Self::AmountWithinTolerance => write!(f, "amount_within_tolerance"),
            Self::Party => write!(f, "party"),
            Self::DaysWindow => write!(f, "days_window"),
        }
    }
}

impl FromStr for MatchOn {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "reference_exact" => Ok(Self::ReferenceExact),
            "amount_exact" => Ok(Self::AmountExact),
            "amount_within_tolerance" => Ok(Self::AmountWithinTolerance),
            "party" => Ok(Self::Party),
            "days_window" => Ok(Self::DaysWindow),
            _ => Err(format!("Unknown MatchOn variant: {}", s)),
        }
    }
}

impl Default for MatchOn {
    fn default() -> Self {
        Self::ReferenceExact
    }
}
