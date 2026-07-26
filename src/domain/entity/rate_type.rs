use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "rate_type", rename_all = "snake_case")]
pub enum RateType {
    Spot,
    AvgPeriod,
    PeriodEnd,
}

impl std::fmt::Display for RateType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spot => write!(f, "spot"),
            Self::AvgPeriod => write!(f, "avg_period"),
            Self::PeriodEnd => write!(f, "period_end"),
        }
    }
}

impl FromStr for RateType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "spot" => Ok(Self::Spot),
            "avg_period" => Ok(Self::AvgPeriod),
            "period_end" => Ok(Self::PeriodEnd),
            _ => Err(format!("Unknown RateType variant: {}", s)),
        }
    }
}

impl Default for RateType {
    fn default() -> Self {
        Self::Spot
    }
}
