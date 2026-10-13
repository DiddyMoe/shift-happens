use crate::{
    employees::validate_revision,
    error::{AppError, AppResult, ErrorCode},
    local_values::LocalDate,
    settings::{REGISTRY, SettingValue},
    text::normalize_required_text,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ASSIGNMENT_LIMIT: usize = 100_000;
pub const GENERATOR_VERSION: &str = "greedy_v1";
pub(crate) const SNAPSHOT_SETTINGS: [&str; 8] = [
    "workweek_start_weekday",
    "two_week_period_anchor_date",
    "holiday_rotation_start_year",
    "max_consecutive_workdays",
    "quick_turnaround_rule_enabled",
    "minimum_rest_minutes_between_shifts",
    "weekly_overtime_threshold_minutes",
    "local_workplace_time_policy",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedSetting {
    pub value: Option<SettingValue>,
    pub revision: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedCoverage {
    pub id: String,
    pub seed_key: Option<String>,
    pub revision: i64,
}
/// The first persisted format; there is no older supported schedule snapshot.
/// Struct field order and BTreeMaps give one compact deterministic JSON encoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationSettingsSnapshot {
    pub version: u32,
    pub generator_version: String,
    pub expected_assignment_count: u32,
    pub settings: BTreeMap<String, CapturedSetting>,
    pub selected_coverage_by_date: BTreeMap<String, SelectedCoverage>,
}
fn invalid() -> AppError {
    AppError::new(
        ErrorCode::InvalidGenerationSnapshot,
        "The generation snapshot must contain the supported format, settings, selected coverage, and a whole slot count from 1 through 100,000.",
    )
}
impl GenerationSettingsSnapshot {
    pub fn validate(&self) -> AppResult<()> {
        if self.version != 1
            || self.generator_version != GENERATOR_VERSION
            || !(1..=ASSIGNMENT_LIMIT as u32).contains(&self.expected_assignment_count)
            || self.settings.len() != SNAPSHOT_SETTINGS.len()
            || self.selected_coverage_by_date.is_empty()
            || self.selected_coverage_by_date.len() > crate::local_values::SCHEDULE_DAY_LIMIT
        {
            return Err(invalid());
        }
        for key in SNAPSHOT_SETTINGS {
            let captured = self.settings.get(key).ok_or_else(invalid)?;
            validate_revision(captured.revision).map_err(|_| invalid())?;
            REGISTRY
                .iter()
                .find(|d| d.key == key)
                .ok_or_else(invalid)?
                .validate(captured.value.as_ref())
                .map_err(|_| invalid())?;
        }
        for (date, coverage) in &self.selected_coverage_by_date {
            LocalDate::parse(date).map_err(|_| invalid())?;
            if normalize_required_text(&coverage.id).map_err(|_| invalid())? != coverage.id {
                return Err(invalid());
            }
            if let Some(seed) = &coverage.seed_key
                && normalize_required_text(seed).map_err(|_| invalid())? != *seed
            {
                return Err(invalid());
            }
            validate_revision(coverage.revision).map_err(|_| invalid())?;
        }
        Ok(())
    }
    pub fn canonical_json(&self) -> AppResult<String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| invalid())
    }
    pub fn parse(value: &str) -> AppResult<Self> {
        let parsed: Self = serde_json::from_str(value).map_err(|_| invalid())?;
        if parsed.canonical_json()? != value {
            return Err(invalid());
        }
        Ok(parsed)
    }
}
