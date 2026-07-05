use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "matched_source_type", rename_all = "snake_case")]
pub enum MatchedSourceType {
    Payment,
    Settlement,
    Invoice,
    Journal,
    Charge,
    FxAdjustment,
}

impl std::fmt::Display for MatchedSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Payment => write!(f, "payment"),
            Self::Settlement => write!(f, "settlement"),
            Self::Invoice => write!(f, "invoice"),
            Self::Journal => write!(f, "journal"),
            Self::Charge => write!(f, "charge"),
            Self::FxAdjustment => write!(f, "fx_adjustment"),
        }
    }
}

impl FromStr for MatchedSourceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "payment" => Ok(Self::Payment),
            "settlement" => Ok(Self::Settlement),
            "invoice" => Ok(Self::Invoice),
            "journal" => Ok(Self::Journal),
            "charge" => Ok(Self::Charge),
            "fx_adjustment" => Ok(Self::FxAdjustment),
            _ => Err(format!("Unknown MatchedSourceType variant: {}", s)),
        }
    }
}

impl Default for MatchedSourceType {
    fn default() -> Self {
        Self::Payment
    }
}
