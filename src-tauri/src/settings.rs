use crate::{
    error::{AppError, AppResult, CommandResult, ErrorCode},
    local_values::{
        LOCAL_DATE_FORMAT, LOCAL_DATETIME_FORMAT, LOCAL_TIME_FORMAT, LOCAL_WORKPLACE_TIME_POLICY,
        LocalDate, Weekday,
    },
    metadata::{MetadataClock, clock_timestamp, parse_metadata_instant},
};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};

// Keep integer values and concurrency revisions exactly representable across IPC.
const MAX_IPC_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    String,
    Integer,
    Boolean,
    Date,
    Weekday,
    ScheduleId,
}

impl ValueType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::Weekday => "weekday",
            Self::ScheduleId => "schedule_id",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SettingValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Date(String),
    Weekday(Weekday),
    ScheduleId(String),
}

impl SettingValue {
    fn value_type(&self) -> ValueType {
        match self {
            Self::String(_) => ValueType::String,
            Self::Integer(_) => ValueType::Integer,
            Self::Boolean(_) => ValueType::Boolean,
            Self::Date(_) => ValueType::Date,
            Self::Weekday(_) => ValueType::Weekday,
            Self::ScheduleId(_) => ValueType::ScheduleId,
        }
    }
    fn format(&self) -> String {
        match self {
            Self::String(v) | Self::Date(v) | Self::ScheduleId(v) => v.clone(),
            Self::Integer(v) => v.to_string(),
            Self::Boolean(v) => v.to_string(),
            Self::Weekday(v) => v.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingOwner {
    Settings,
    Holidays,
    Schedule,
}

pub struct SettingDefinition {
    pub key: &'static str,
    pub value_type: ValueType,
    pub default: Option<&'static str>,
    pub nullable: bool,
    pub editable: bool,
    pub owner: SettingOwner,
    pub description: &'static str,
}

// Each key's parsing, formatting and validation live beside this one registry.
pub const REGISTRY: &[SettingDefinition] = &[
    SettingDefinition {
        key: "holiday_rotation_start_year",
        value_type: ValueType::Integer,
        default: Some("2026"),
        nullable: false,
        editable: false,
        owner: SettingOwner::Holidays,
        description: "Holiday rotation reference year",
    },
    SettingDefinition {
        key: "workweek_start_weekday",
        value_type: ValueType::Weekday,
        default: Some("sunday"),
        nullable: false,
        editable: true,
        owner: SettingOwner::Settings,
        description: "First day of each workweek",
    },
    SettingDefinition {
        key: "two_week_period_anchor_date",
        value_type: ValueType::Date,
        default: None,
        nullable: true,
        editable: true,
        owner: SettingOwner::Settings,
        description: "A known payroll period start date",
    },
    SettingDefinition {
        key: "max_consecutive_workdays",
        value_type: ValueType::Integer,
        default: Some("5"),
        nullable: false,
        editable: true,
        owner: SettingOwner::Settings,
        description: "Maximum consecutive worked calendar days (1–31)",
    },
    SettingDefinition {
        key: "quick_turnaround_rule_enabled",
        value_type: ValueType::Boolean,
        default: Some("true"),
        nullable: false,
        editable: true,
        owner: SettingOwner::Settings,
        description: "Check minimum rest between shifts",
    },
    SettingDefinition {
        key: "minimum_rest_minutes_between_shifts",
        value_type: ValueType::Integer,
        default: Some("600"),
        nullable: false,
        editable: true,
        owner: SettingOwner::Settings,
        description: "Minimum rest between shifts, in hours (0–48)",
    },
    SettingDefinition {
        key: "theme_preference",
        value_type: ValueType::String,
        default: Some("dark"),
        nullable: false,
        editable: true,
        owner: SettingOwner::Settings,
        description: "Saved appearance preference",
    },
    SettingDefinition {
        key: "weekly_overtime_threshold_minutes",
        value_type: ValueType::Integer,
        default: Some("2400"),
        nullable: false,
        editable: false,
        owner: SettingOwner::Settings,
        description: "Weekly paid hours before overtime",
    },
    SettingDefinition {
        key: "local_workplace_time_policy",
        value_type: ValueType::String,
        default: Some(LOCAL_WORKPLACE_TIME_POLICY),
        nullable: false,
        editable: false,
        owner: SettingOwner::Settings,
        description: "Scheduled wall-clock time, without daylight-saving adjustment",
    },
    SettingDefinition {
        key: "local_date_format",
        value_type: ValueType::String,
        default: Some(LOCAL_DATE_FORMAT),
        nullable: false,
        editable: false,
        owner: SettingOwner::Settings,
        description: "Canonical workplace date format",
    },
    SettingDefinition {
        key: "local_time_format",
        value_type: ValueType::String,
        default: Some(LOCAL_TIME_FORMAT),
        nullable: false,
        editable: false,
        owner: SettingOwner::Settings,
        description: "Canonical workplace time format",
    },
    SettingDefinition {
        key: "local_datetime_format",
        value_type: ValueType::String,
        default: Some(LOCAL_DATETIME_FORMAT),
        nullable: false,
        editable: false,
        owner: SettingOwner::Settings,
        description: "Canonical workplace datetime format",
    },
    SettingDefinition {
        key: "current_schedule_id",
        value_type: ValueType::ScheduleId,
        default: None,
        nullable: true,
        editable: false,
        owner: SettingOwner::Schedule,
        description: "Selected schedule",
    },
];

fn invalid(def: &SettingDefinition) -> AppError {
    AppError::new(
        ErrorCode::InvalidSetting,
        "The setting value does not meet its required type or range.",
    )
    .setting(def.key)
}

impl SettingDefinition {
    pub fn parse(&self, value: Option<&str>) -> AppResult<Option<SettingValue>> {
        let Some(value) = value else {
            return if self.nullable {
                Ok(None)
            } else {
                Err(invalid(self))
            };
        };
        let parsed = match self.value_type {
            ValueType::String => SettingValue::String(value.to_owned()),
            ValueType::Integer => {
                let integer = value.parse::<i64>().map_err(|_| invalid(self))?;
                if integer.to_string() != value {
                    return Err(invalid(self));
                }
                SettingValue::Integer(integer)
            }
            ValueType::Boolean => SettingValue::Boolean(match value {
                "true" => true,
                "false" => false,
                _ => return Err(invalid(self)),
            }),
            ValueType::Date => {
                SettingValue::Date(LocalDate::parse(value).map_err(|_| invalid(self))?.format())
            }
            ValueType::Weekday => {
                SettingValue::Weekday(Weekday::parse(value).map_err(|_| invalid(self))?)
            }
            ValueType::ScheduleId => SettingValue::ScheduleId(value.to_owned()),
        };
        self.validate(Some(&parsed))?;
        Ok(Some(parsed))
    }

    pub fn validate(&self, value: Option<&SettingValue>) -> AppResult<()> {
        let Some(value) = value else {
            return if self.nullable {
                Ok(())
            } else {
                Err(invalid(self))
            };
        };
        if value.value_type() != self.value_type {
            return Err(invalid(self));
        }
        let valid = match (self.key, value) {
            ("holiday_rotation_start_year", SettingValue::Integer(v)) => (1..=9999).contains(v),
            ("max_consecutive_workdays", SettingValue::Integer(v)) => (1..=31).contains(v),
            ("minimum_rest_minutes_between_shifts", SettingValue::Integer(v)) => {
                (0..=2880).contains(v)
            }
            // Keep integer IPC values exact in JavaScript as well as Rust.
            ("weekly_overtime_threshold_minutes", SettingValue::Integer(v)) => {
                (1..=MAX_IPC_INTEGER).contains(v)
            }
            ("theme_preference", SettingValue::String(v)) => {
                matches!(v.as_str(), "dark" | "light" | "system")
            }
            ("local_workplace_time_policy", SettingValue::String(v)) => {
                v == LOCAL_WORKPLACE_TIME_POLICY
            }
            ("local_date_format", SettingValue::String(v)) => v == LOCAL_DATE_FORMAT,
            ("local_time_format", SettingValue::String(v)) => v == LOCAL_TIME_FORMAT,
            ("local_datetime_format", SettingValue::String(v)) => v == LOCAL_DATETIME_FORMAT,
            ("current_schedule_id", SettingValue::ScheduleId(v)) => !v.trim().is_empty(),
            ("two_week_period_anchor_date", SettingValue::Date(v)) => LocalDate::parse(v).is_ok(),
            ("workweek_start_weekday", SettingValue::Weekday(_))
            | ("quick_turnaround_rule_enabled", SettingValue::Boolean(_)) => true,
            _ => false,
        };
        if valid { Ok(()) } else { Err(invalid(self)) }
    }
}

#[derive(Serialize)]
pub struct SettingView {
    pub key: &'static str,
    pub value_type: ValueType,
    pub value: Option<SettingValue>,
    pub default_value: Option<SettingValue>,
    pub nullable: bool,
    pub editable: bool,
    pub owner: SettingOwner,
    pub description: &'static str,
    pub revision: i64,
}

pub async fn read_in_transaction(tx: &mut Transaction<'_, Sqlite>) -> AppResult<Vec<SettingView>> {
    // One read of exactly the rows a per-key `key = ?` lookup would match. `key`
    // is the TEXT primary key, so each registry key matches at most one row.
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT key, value, value_type, revision, created_at, updated_at FROM app_settings WHERE key IN (",
    );
    let mut keys = query.separated(", ");
    for def in REGISTRY {
        keys.push_bind(def.key);
    }
    keys.push_unseparated(")");
    let rows = query.build().fetch_all(&mut **tx).await?;
    let mut views = Vec::with_capacity(REGISTRY.len());
    // Registry order decides which problem is reported first, as before.
    for def in REGISTRY {
        let row = rows
            .iter()
            .find(|row| {
                row.try_get::<&str, _>("key")
                    .is_ok_and(|key| key == def.key)
            })
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::ConfigurationInvalid,
                    "A required setting is missing.",
                )
                .setting(def.key)
            })?;
        let view = (|| -> AppResult<SettingView> {
            let value_type: String = row.try_get("value_type")?;
            let revision: i64 = row.try_get("revision")?;
            if value_type != def.value_type.as_str() || !(1..=MAX_IPC_INTEGER).contains(&revision) {
                return Err(invalid(def));
            }
            parse_metadata_instant(&row.try_get::<String, _>("created_at")?)?;
            parse_metadata_instant(&row.try_get::<String, _>("updated_at")?)?;
            let stored: Option<String> = row.try_get("value")?;
            Ok(SettingView {
                key: def.key,
                value_type: def.value_type,
                value: def.parse(stored.as_deref())?,
                default_value: def.parse(def.default)?,
                nullable: def.nullable,
                editable: def.editable,
                owner: def.owner,
                description: def.description,
                revision,
            })
        })()
        .map_err(|_| {
            AppError::new(
                ErrorCode::ConfigurationInvalid,
                "A stored setting is invalid; it has not been replaced.",
            )
            .setting(def.key)
        })?;
        views.push(view);
    }
    Ok(views)
}

pub async fn seed_settings(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
) -> AppResult<()> {
    let timestamp = clock_timestamp(clock)?;
    for def in REGISTRY {
        def.parse(def.default)?;
        sqlx::query("INSERT INTO app_settings (key, value, value_type, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(key) DO NOTHING")
            .bind(def.key).bind(def.default).bind(def.value_type.as_str()).bind(def.description)
            .bind(&timestamp).bind(&timestamp).execute(&mut **tx).await?;
    }
    Ok(())
}

pub async fn get_settings(pool: &SqlitePool) -> AppResult<Vec<SettingView>> {
    let mut tx = pool.begin().await?;
    let values = read_in_transaction(&mut tx).await?;
    tx.rollback().await?;
    Ok(values)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateManagerSettingRequest {
    pub key: String,
    #[serde(deserialize_with = "deserialize_required_value")]
    pub value: Option<SettingValue>,
    pub expected_revision: i64,
}

// Explicit null clears a nullable setting. An omitted value is a malformed request.
fn deserialize_required_value<'de, D>(deserializer: D) -> Result<Option<SettingValue>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<SettingValue>::deserialize(deserializer)
}

pub async fn update_manager_setting(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateManagerSettingRequest,
) -> CommandResult<SettingView> {
    async fn update(
        pool: &SqlitePool,
        clock: &dyn MetadataClock,
        request: UpdateManagerSettingRequest,
    ) -> AppResult<(SettingView, bool)> {
        let def = REGISTRY
            .iter()
            .find(|def| def.key == request.key)
            .ok_or_else(|| {
                AppError::new(ErrorCode::InvalidSetting, "This setting is not recognized.")
            })?;
        if !def.editable || !matches!(def.owner, SettingOwner::Settings) {
            return Err(AppError::new(
                ErrorCode::SettingNotEditable,
                "This setting is read-only or owned by another service.",
            )
            .setting(def.key));
        }
        if !(1..=MAX_IPC_INTEGER).contains(&request.expected_revision) {
            return Err(invalid(def));
        }
        def.validate(request.value.as_ref())?;
        // Serialize writers before reading the revision; stale writes never race a deferred transaction upgrade.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let views = read_in_transaction(&mut tx).await?;
        let current = views
            .into_iter()
            .find(|v| v.key == def.key)
            .ok_or_else(|| invalid(def))?;
        if current.revision != request.expected_revision {
            let mut error = AppError::new(
                ErrorCode::StaleRevision,
                "This setting changed. Reload it before saving again.",
            )
            .setting(def.key);
            error.context.current_revision = Some(current.revision);
            tx.rollback().await?;
            return Err(error);
        }
        if current.value == request.value {
            tx.rollback().await?;
            return Ok((current, false));
        }
        if current.revision == MAX_IPC_INTEGER {
            return Err(invalid(def));
        }
        let timestamp = clock_timestamp(clock)?;
        let updated = sqlx::query("UPDATE app_settings SET value = ?, updated_at = ?, revision = revision + 1 WHERE key = ? AND revision = ?")
            .bind(request.value.as_ref().map(SettingValue::format)).bind(timestamp).bind(def.key).bind(request.expected_revision).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            return Err(AppError::new(
                ErrorCode::StaleRevision,
                "Reload this setting before saving.",
            ));
        }
        let view = read_in_transaction(&mut tx)
            .await?
            .into_iter()
            .find(|v| v.key == def.key)
            .ok_or_else(|| invalid(def))?;
        tx.commit().await?;
        Ok((view, true))
    }
    match update(pool, clock, request).await {
        Ok((data, true)) => CommandResult::Success {
            data,
            warnings: Vec::new(),
        },
        Ok((data, false)) => CommandResult::NoChange { data },
        Err(error) => CommandResult::from(Err(error)),
    }
}
