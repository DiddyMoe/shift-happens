//! Owned, note-free validation input. SQL here is SELECT-only and accepts the caller's transaction.
use crate::{
    availability_pto::{AvailabilityEntry, PtoEntry},
    database::{self, DatabasePaths},
    employee_restrictions::EmployeeRestriction,
    employees::{Employee, validate_revision},
    error::{AppError, AppResult, ErrorCode},
    generation_inputs,
    generation_snapshot::SNAPSHOT_SETTINGS,
    local_values::{LocalDate, LocalDateTime, LocalInterval, SourceRowBudget},
    position_restrictions::PositionRestriction,
    settings::{REGISTRY, SettingView},
    template_restrictions::TemplateRestriction,
    validation::{ValidationLimits, ValidationResult, validate_snapshot},
};
use serde_json::{Value, json};
use sqlx::{Connection, FromRow, Row, Sqlite, Transaction, sqlite::SqliteRow};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(crate) struct Facts(pub Value);
impl Facts {
    pub fn s(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.as_str()
    }
    pub fn n(&self, key: &str) -> Option<i64> {
        self.0.get(key)?.as_i64()
    }
    pub fn b(&self, key: &str) -> Option<bool> {
        match self.n(key)? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    pub fn absent(&self, key: &str) -> bool {
        self.0.get(key).is_none_or(Value::is_null)
    }
    pub fn id(&self) -> &str {
        self.s("id").unwrap_or("invalid_id")
    }
}
pub(crate) struct SourceProblem {
    pub employee: String,
    pub id: String,
    pub kind: &'static str,
    pub hard: bool,
    pub material: Value,
}
pub struct ValidationSnapshot {
    pub(crate) schedule_id: String,
    pub(crate) header: Option<Facts>,
    pub(crate) rows: Vec<Facts>,
    pub(crate) fallback: BTreeMap<&'static str, BTreeMap<String, Facts>>,
    pub(crate) employees: BTreeMap<String, Employee>,
    pub(crate) employee_states: BTreeMap<String, bool>,
    pub(crate) employee_bad: BTreeMap<String, Vec<String>>,
    pub(crate) settings: Vec<SettingView>,
    pub(crate) settings_bad: Vec<String>,
    pub(crate) restrictions: Vec<EmployeeRestriction>,
    pub(crate) availability: Vec<AvailabilityEntry>,
    pub(crate) pto: Vec<PtoEntry>,
    pub(crate) template_blocks: Vec<TemplateRestriction>,
    pub(crate) position_blocks: Vec<PositionRestriction>,
    pub(crate) source_problems: Vec<SourceProblem>,
    pub(crate) stopped: Option<(&'static str, usize, usize)>,
    pub(crate) limits: ValidationLimits,
}
fn empty(id: &str, limits: ValidationLimits) -> ValidationSnapshot {
    ValidationSnapshot {
        schedule_id: id.to_owned(),
        header: None,
        rows: Vec::new(),
        fallback: BTreeMap::new(),
        employees: BTreeMap::new(),
        employee_states: BTreeMap::new(),
        employee_bad: BTreeMap::new(),
        settings: Vec::new(),
        settings_bad: Vec::new(),
        restrictions: Vec::new(),
        availability: Vec::new(),
        pto: Vec::new(),
        template_blocks: Vec::new(),
        position_blocks: Vec::new(),
        source_problems: Vec::new(),
        stopped: None,
        limits,
    }
}
// Repository-owned identifiers only. JSON retains actual SQLite numeric types;
// malformed bools/floats/nulls never turn into a valid typed assignment by coercion.
fn projection(columns: &[&str]) -> String {
    let mut result = String::from("json_object(");
    for (i, column) in columns.iter().enumerate() {
        if i != 0 {
            result.push(',');
        }
        result.push_str(&format!(
            "'{column}',CASE WHEN typeof({column})='blob' THEN NULL ELSE {column} END"
        ));
    }
    result.push(')');
    result
}
async fn facts(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    columns: &[&str],
    filter: &str,
    ids: &str,
) -> AppResult<Vec<Facts>> {
    let sql = format!(
        "SELECT {} FROM {table} WHERE {filter} ORDER BY id",
        projection(columns)
    );
    let rows: Vec<String> = sqlx::QueryBuilder::<Sqlite>::new("")
        .push(&sql)
        .build_query_scalar()
        .bind(ids)
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|s| {
            serde_json::from_str(&s).map(Facts).map_err(|_| {
                AppError::new(
                    ErrorCode::ConfigurationInvalid,
                    "Stored validation facts could not be read.",
                )
            })
        })
        .collect()
}
const ASSIGNMENT_FACTS: &[&str] = &[
    "id",
    "schedule_id",
    "schedule_date",
    "actual_start_datetime",
    "actual_end_datetime",
    "coverage_template_id",
    "coverage_item_id",
    "shift_position_id",
    "shift_template_id",
    "coverage_day_type_snapshot",
    "coverage_sort_order_snapshot",
    "coverage_required_count_snapshot",
    "coverage_slot_index_snapshot",
    "coverage_start_offset_snapshot",
    "coverage_end_offset_snapshot",
    "position_seed_key_snapshot",
    "shift_start_snapshot",
    "shift_end_snapshot",
    "shift_category_snapshot",
    "shift_color_snapshot",
    "shift_lunch_minutes_snapshot",
    "shift_paid_minutes_snapshot",
    "shift_recovery_snapshot",
    "shift_night_snapshot",
    "holiday_id",
    "holiday_seed_key_snapshot",
    "holiday_set_snapshot",
    "holiday_coverage_behavior_snapshot",
    "assigned_employee_id",
    "open_shift",
    "locked",
    "manually_edited",
    "assignment_source",
];

pub async fn load_validation_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    schedule_id: &str,
    limits: ValidationLimits,
) -> AppResult<ValidationSnapshot> {
    let mut snapshot = empty(schedule_id, limits);
    let mut headers = facts(
        tx,
        "schedules",
        &[
            "id",
            "start_date",
            "end_date",
            "status",
            "generation_settings_snapshot",
        ],
        "id=?1",
        schedule_id,
    )
    .await?;
    snapshot.header = headers.pop();
    let Some(header) = &snapshot.header else {
        return Ok(snapshot);
    };
    let start = header
        .s("start_date")
        .and_then(|v| LocalDate::parse(v).ok());
    let end = header.s("end_date").and_then(|v| LocalDate::parse(v).ok());
    let (Some(start), Some(end)) = (start, end) else {
        return Ok(snapshot);
    };
    if start > end || !matches!(header.s("status"), Some("draft" | "final")) {
        return Ok(snapshot);
    }
    let days = usize::try_from(end.days_since(start))
        .ok()
        .and_then(|v| v.checked_add(1))
        .unwrap_or(usize::MAX);
    if days > limits.schedule_days {
        snapshot.stopped = Some(("schedule_days", limits.schedule_days, days));
        return Ok(snapshot);
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM schedule_assignments WHERE schedule_id=?")
            .bind(schedule_id)
            .fetch_one(&mut **tx)
            .await?;
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    if count > limits.assignments {
        snapshot.stopped = Some(("assignments", limits.assignments, count));
        return Ok(snapshot);
    }
    snapshot.rows = facts(
        tx,
        "schedule_assignments",
        ASSIGNMENT_FACTS,
        "schedule_id=?1",
        schedule_id,
    )
    .await?;
    // Names are checked for snapshot presence in SQL and are never loaded into validation.
    let presence = sqlx::query("SELECT id, employee_first_name_snapshot IS NOT NULL AS first_present, employee_last_name_snapshot IS NOT NULL AS last_present, employee_display_name_snapshot IS NOT NULL AS display_present, length(trim(employee_first_name_snapshot))>0 AND length(trim(employee_last_name_snapshot))>0 AND length(trim(employee_display_name_snapshot))>0 AS names_valid, length(trim(shift_name_snapshot))>0 AS shift_name_valid, length(trim(position_name_snapshot))>0 AS position_name_valid, length(trim(coverage_template_name_snapshot))>0 AS coverage_name_valid, length(trim(holiday_name_snapshot))>0 AS holiday_name_valid, holiday_name_snapshot IS NOT NULL AS holiday_name_present FROM schedule_assignments WHERE schedule_id=? ORDER BY id").bind(schedule_id).fetch_all(&mut **tx).await?;
    for (row, flags) in snapshot.rows.iter_mut().zip(presence) {
        if let Some(object) = row.0.as_object_mut() {
            for key in [
                "first_present",
                "last_present",
                "display_present",
                "names_valid",
                "shift_name_valid",
                "position_name_valid",
                "coverage_name_valid",
                "holiday_name_valid",
                "holiday_name_present",
            ] {
                object.insert(
                    key.to_string(),
                    json!(flags.try_get::<Option<i64>, _>(key).ok().flatten()),
                );
            }
        }
    }
    let employee_filter = "(active_status='active' AND archived_at IS NULL) OR id IN (SELECT assigned_employee_id FROM schedule_assignments WHERE schedule_id=?1)";
    let count_sql = format!("SELECT count(*) FROM employees WHERE {employee_filter}");
    let count: i64 = sqlx::QueryBuilder::<Sqlite>::new("")
        .push(&count_sql)
        .build_query_scalar()
        .bind(schedule_id)
        .fetch_one(&mut **tx)
        .await?;
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    if count > limits.employees {
        snapshot.stopped = Some(("employee_rows", limits.employees, count));
        snapshot.rows.clear();
        return Ok(snapshot);
    }
    let columns = generation_inputs::EMPLOYEE_COLUMNS.replace(
        "first_name,last_name,display_name",
        "'Employee' AS first_name,'Record' AS last_name,'Employee Record' AS display_name",
    );
    let employee_sql = format!(
        "SELECT {columns},typeof(can_work_weekends)='integer' AND can_work_weekends IN (0,1) AND typeof(can_work_holidays)='integer' AND can_work_holidays IN (0,1) AND typeof(can_work_night_shift)='integer' AND can_work_night_shift IN (0,1) AND typeof(can_work_ortho_or)='integer' AND can_work_ortho_or IN (0,1) AND typeof(requires_explicit_availability)='integer' AND requires_explicit_availability IN (0,1) AND (minimum_paid_minutes IS NULL OR typeof(minimum_paid_minutes)='integer') AND (minimum_assignment_count IS NULL OR typeof(minimum_assignment_count)='integer') AND (maximum_paid_minutes IS NULL OR typeof(maximum_paid_minutes)='integer') AS numeric_valid FROM employees WHERE {employee_filter} ORDER BY id"
    );
    for row in sqlx::QueryBuilder::<Sqlite>::new("")
        .push(&employee_sql)
        .build()
        .bind(schedule_id)
        .fetch_all(&mut **tx)
        .await?
    {
        let id: String = row.try_get("id")?;
        let active = row.try_get::<String, _>("active_status").ok().as_deref() == Some("active")
            && row.try_get::<Option<String>, _>("archived_at").ok() == Some(None);
        snapshot.employee_states.insert(id.clone(), active);
        let employee = Employee::from_row(&row).ok().filter(|e| {
            row.try_get::<i64, _>("numeric_valid").ok() == Some(1)
                && e.data.clone().normalized().is_ok_and(|v| v == e.data)
        });
        match employee {
            Some(e) => {
                snapshot.employees.insert(id, e);
            }
            None => {
                snapshot
                    .employee_bad
                    .insert(id, vec!["employee_rules_or_capabilities".to_owned()]);
            }
        }
    }
    for def in REGISTRY
        .iter()
        .filter(|d| SNAPSHOT_SETTINGS.contains(&d.key))
    {
        let row = sqlx::query("SELECT value,value_type,revision,typeof(revision)='integer' AS integer_revision FROM app_settings WHERE key=?").bind(def.key).fetch_optional(&mut **tx).await?;
        let view = row.and_then(|r| {
            let value: Option<String> = r.try_get("value").ok()?;
            let revision: i64 = r.try_get("revision").ok()?;
            if r.try_get::<String, _>("value_type").ok()?.as_str() != def.value_type.as_str()
                || r.try_get::<i64, _>("integer_revision").ok() != Some(1)
                || validate_revision(revision).is_err()
            {
                return None;
            }
            Some(SettingView {
                key: def.key,
                value_type: def.value_type,
                value: def.parse(value.as_deref()).ok()?,
                default_value: None,
                nullable: def.nullable,
                editable: def.editable,
                owner: def.owner,
                description: def.description,
                revision,
            })
        });
        match view {
            Some(v) => snapshot.settings.push(v),
            None => snapshot.settings_bad.push(def.key.to_owned()),
        }
    }
    // Zero-assignment employees still need workload configuration, but their
    // availability and restriction sources cannot affect an assignment here.
    let assigned_ids: BTreeSet<_> = snapshot
        .rows
        .iter()
        .filter_map(|r| r.s("assigned_employee_id"))
        .collect();
    let ids = generation_inputs::json_ids(assigned_ids)?;
    // Restrict actual source windows to parseable, bounded intervals (at most 24h).
    // Unorderable/huge corrupt rows get structural errors, never source expansion.
    let intervals: Vec<_> = snapshot
        .rows
        .iter()
        .filter(|r| r.s("assigned_employee_id").is_some())
        .filter_map(|r| {
            let a = LocalDateTime::parse(r.s("actual_start_datetime")?).ok()?;
            let b = LocalDateTime::parse(r.s("actual_end_datetime")?).ok()?;
            let i = LocalInterval::new(a, b).ok()?;
            let duration = crate::local_values::wall_clock_minutes(a, b).ok()?;
            if !(1..=1439).contains(&duration) {
                return None;
            }
            Some(i)
        })
        .collect();
    let first = intervals
        .iter()
        .map(|i| i.start().date())
        .min()
        .unwrap_or(start);
    let last = intervals
        .iter()
        .map(|i| i.end().date())
        .max()
        .unwrap_or(end);
    let first = first.subtract_days(1).unwrap_or(first);
    // Seven consecutive dates cover every weekday. Bound this selection even
    // when corrupt assignment dates make the actual envelope unusually wide.
    let mut weekdays = BTreeSet::new();
    for offset in 0..=last.days_since(first).min(6) {
        weekdays.insert(first.add_days(offset)?.weekday().as_str());
    }
    let restriction_days = generation_inputs::json_ids(weekdays)?;
    let first = first.format();
    let last = last.format();
    let mut budget = SourceRowBudget {
        count: 0,
        limit: limits.sources,
    };
    let mut reads = 0;
    macro_rules! sources {
        ($table:literal,$cols:expr,$filter:literal,$type:ty,$target:ident,$verify:expr,$hard:expr) => {{
            let rows = load_rows(
                tx,
                &mut budget,
                &mut reads,
                ($table, $cols, $filter),
                (&first, &last, &ids, &restriction_days),
                schedule_id,
            )
            .await;
            match rows {
                Err(e) if matches!(e.code, ErrorCode::InputLimitExceeded) => {
                    snapshot.stopped = Some((
                        e.context.limit_kind.unwrap_or("source_rows"),
                        e.context.limit.unwrap_or(limits.sources),
                        e.context.observed_count.unwrap_or(usize::MAX),
                    ));
                    return Ok(snapshot);
                }
                Err(e) => return Err(e),
                Ok(rows) => {
                    for r in rows {
                        let id: String = r.try_get("id")?;
                        let employee: String = r.try_get("employee_id")?;
                        let hard = ($hard)(&r);
                        let numeric_valid =
                            r.try_get::<i64, _>("validation_numeric_valid").ok() == Some(1);
                        match <$type>::from_row(&r).ok().filter(|v| {
                            numeric_valid
                                && id.len() <= crate::error::DIAGNOSTIC_SOURCE_ID_BYTES
                                && ($verify)(v)
                        }) {
                            Some(v) => snapshot.$target.push(v),
                            None => snapshot.source_problems.push(SourceProblem {
                                employee,
                                id,
                                kind: $table,
                                hard,
                                material: serde_json::from_str(
                                    &r.try_get::<String, _>("validation_material")?,
                                )
                                .map_err(|_| {
                                    AppError::new(
                                        ErrorCode::ConfigurationInvalid,
                                        "Stored rule facts could not be read.",
                                    )
                                })?,
                            }),
                        }
                    }
                }
            }
        }};
    }
    sources!(
        "employee_restrictions",
        generation_inputs::RESTRICTION_COLUMNS,
        "active_status='active' AND archived_at IS NULL AND employee_id IN (SELECT value FROM json_each(?3)) AND (restriction_kind='time_of_day_block' OR weekday IN (SELECT value FROM json_each(?5)) OR restriction_kind IS NULL OR restriction_kind NOT IN ('time_of_day_block','recurring_day_time_block') OR weekday IS NULL OR weekday NOT IN ('sunday','monday','tuesday','wednesday','thursday','friday','saturday'))",
        EmployeeRestriction,
        restrictions,
        |r: &EmployeeRestriction| r.data.clone().normalized().is_ok_and(|v| v == r.data),
        |_: &SqliteRow| true
    );
    sources!(
        "availability_entries",
        generation_inputs::AVAILABILITY_COLUMNS,
        "active_status='active' AND archived_at IS NULL AND employee_id IN (SELECT value FROM json_each(?3)) AND (start_date<=?2 AND end_date>=?1 OR date(start_date,'+0 days') IS NULL OR date(end_date,'+0 days') IS NULL OR date(start_date,'+0 days')<>start_date OR date(end_date,'+0 days')<>end_date OR start_date>end_date)",
        AvailabilityEntry,
        availability,
        |r: &AvailabilityEntry| crate::availability_pto::verify_availability(r).is_ok(),
        |r: &SqliteRow| match r.try_get::<String, _>("entry_type").ok().as_deref() {
            Some("preferred") => false,
            Some("available") => r
                .try_get::<String, _>("employee_id")
                .ok()
                .and_then(|id| snapshot.employees.get(&id))
                .is_none_or(|e| e.data.requires_explicit_availability),
            _ => true,
        }
    );
    sources!(
        "pto_entries",
        generation_inputs::PTO_COLUMNS,
        "active_status='active' AND archived_at IS NULL AND employee_id IN (SELECT value FROM json_each(?3)) AND (status IN ('requested','approved') OR status IS NULL OR status NOT IN ('denied','canceled')) AND (start_date<=?2 AND end_date>=?1 OR date(start_date,'+0 days') IS NULL OR date(end_date,'+0 days') IS NULL OR date(start_date,'+0 days')<>start_date OR date(end_date,'+0 days')<>end_date OR start_date>end_date)",
        PtoEntry,
        pto,
        |r: &PtoEntry| crate::availability_pto::verify_pto(r).is_ok(),
        |r: &SqliteRow| r.try_get::<String, _>("status").ok().as_deref() != Some("requested")
    );
    sources!(
        "employee_shift_template_restrictions",
        "id,employee_id,shift_template_id,restriction_type,active_status,NULL AS notes,created_at,updated_at,archived_at,revision",
        "active_status='active' AND archived_at IS NULL AND employee_id IN (SELECT value FROM json_each(?3)) AND employee_id IN (SELECT id FROM employees WHERE active_status='active' AND archived_at IS NULL) AND shift_template_id IN (SELECT shift_template_id FROM schedule_assignments WHERE schedule_id IN (SELECT id FROM schedules WHERE id=?4)) AND shift_template_id IN (SELECT id FROM shift_templates WHERE active_status='active' AND archived_at IS NULL)",
        TemplateRestriction,
        template_blocks,
        |r: &TemplateRestriction| crate::template_restrictions::verify_restriction(r).is_ok(),
        |_: &SqliteRow| true
    );
    sources!(
        "employee_position_restrictions",
        "id,employee_id,shift_position_id,restriction_type,active_status,NULL AS notes,created_at,updated_at,archived_at,revision",
        "active_status='active' AND archived_at IS NULL AND employee_id IN (SELECT value FROM json_each(?3)) AND employee_id IN (SELECT id FROM employees WHERE active_status='active' AND archived_at IS NULL) AND shift_position_id IN (SELECT shift_position_id FROM schedule_assignments WHERE schedule_id IN (SELECT id FROM schedules WHERE id=?4)) AND shift_position_id IN (SELECT id FROM shift_positions WHERE active_status='active' AND archived_at IS NULL)",
        PositionRestriction,
        position_blocks,
        |r: &PositionRestriction| crate::position_restrictions::verify_restriction(r).is_ok(),
        |_: &SqliteRow| true
    );
    // Legacy fallback is selected only for absent snapshots; current source edits
    // never override a present (even malformed) historical snapshot.
    for (table, key, columns, missing) in [
        (
            "shift_templates",
            "shift_template_id",
            &[
                "id",
                "start_time",
                "end_time",
                "time_of_day",
                "color_hex",
                "unpaid_lunch_minutes",
                "paid_minutes",
                "requires_recovery_after_shift",
                "is_night_shift",
            ][..],
            "shift_start_snapshot IS NULL OR shift_end_snapshot IS NULL OR shift_category_snapshot IS NULL OR shift_paid_minutes_snapshot IS NULL OR shift_lunch_minutes_snapshot IS NULL OR shift_recovery_snapshot IS NULL OR shift_night_snapshot IS NULL",
        ),
        (
            "coverage_template_items",
            "coverage_item_id",
            &[
                "id",
                "required_count",
                "actual_start_day_offset",
                "actual_end_day_offset",
                "sort_order",
            ][..],
            "coverage_required_count_snapshot IS NULL OR coverage_start_offset_snapshot IS NULL OR coverage_end_offset_snapshot IS NULL OR coverage_sort_order_snapshot IS NULL",
        ),
        (
            "coverage_templates",
            "coverage_template_id",
            &["id", "day_type"][..],
            "coverage_day_type_snapshot IS NULL",
        ),
        (
            "shift_positions",
            "shift_position_id",
            &["id", "seed_key"][..],
            "position_name_snapshot IS NULL",
        ),
        (
            "holidays",
            "holiday_id",
            &["id", "holiday_set", "coverage_behavior"][..],
            "holiday_id IS NOT NULL AND (holiday_set_snapshot IS NULL OR holiday_coverage_behavior_snapshot IS NULL)",
        ),
    ] {
        let filter = format!(
            "id IN (SELECT {key} FROM schedule_assignments WHERE schedule_id=?1 AND ({missing}))"
        );
        let count_sql = format!("SELECT count(*) FROM {table} WHERE {filter}");
        let count: i64 = sqlx::QueryBuilder::<Sqlite>::new("")
            .push(&count_sql)
            .build_query_scalar()
            .bind(schedule_id)
            .fetch_one(&mut **tx)
            .await?;
        if let Err(e) = budget.reserve(count) {
            snapshot.stopped = Some((
                "source_rows",
                limits.sources,
                e.context.observed_count.unwrap_or(usize::MAX),
            ));
            return Ok(snapshot);
        }
        snapshot.fallback.insert(
            table,
            facts(tx, table, columns, &filter, schedule_id)
                .await?
                .into_iter()
                .map(|r| (r.id().to_owned(), r))
                .collect(),
        );
    }
    Ok(snapshot)
}
async fn load_rows(
    tx: &mut Transaction<'_, Sqlite>,
    budget: &mut SourceRowBudget,
    reads: &mut i64,
    source: (&str, &str, &str),
    window: (&str, &str, &str, &str),
    schedule_id: &str,
) -> AppResult<Vec<SqliteRow>> {
    let (table, columns, filter) = source;
    let (first, last, ids, restriction_days) = window;
    let count_sql = format!(
        "SELECT count(*) FROM {table} WHERE ({filter}) AND ?1<=?2 AND ?3 IS NOT NULL AND ?4 IS NOT NULL AND ?5 IS NOT NULL"
    );
    let count: i64 = sqlx::QueryBuilder::<Sqlite>::new("")
        .push(&count_sql)
        .build_query_scalar()
        .bind(first)
        .bind(last)
        .bind(ids)
        .bind(schedule_id)
        .bind(restriction_days)
        .fetch_one(&mut **tx)
        .await?;
    budget.reserve(count)?;
    *reads += 2;
    let numeric = if matches!(table, "availability_entries" | "pto_entries") {
        "typeof(is_full_day)='integer' AND is_full_day IN (0,1)"
    } else {
        "1"
    };
    let material_columns: &[&str] = match table {
        "employee_restrictions" => &[
            "id",
            "employee_id",
            "restriction_kind",
            "time_of_day",
            "weekday",
            "start_time",
            "end_time",
        ],
        "availability_entries" => &[
            "id",
            "employee_id",
            "entry_type",
            "start_date",
            "end_date",
            "is_full_day",
            "start_time",
            "end_time",
        ],
        "pto_entries" => &[
            "id",
            "employee_id",
            "status",
            "start_date",
            "end_date",
            "is_full_day",
            "start_time",
            "end_time",
        ],
        "employee_shift_template_restrictions" => {
            &["id", "employee_id", "shift_template_id", "restriction_type"]
        }
        "employee_position_restrictions" => {
            &["id", "employee_id", "shift_position_id", "restriction_type"]
        }
        _ => &[],
    };
    let sql = format!(
        "SELECT {columns}, {numeric} AS validation_numeric_valid, {} AS validation_material FROM {table} WHERE ({filter}) AND ?1<=?2 AND ?3 IS NOT NULL AND ?4 IS NOT NULL AND ?5 IS NOT NULL ORDER BY id",
        projection(material_columns)
    );
    Ok(sqlx::QueryBuilder::<Sqlite>::new("")
        .push(&sql)
        .build()
        .bind(first)
        .bind(last)
        .bind(ids)
        .bind(schedule_id)
        .bind(restriction_days)
        .fetch_all(&mut **tx)
        .await?)
}

pub async fn validate_schedule(
    paths: &DatabasePaths,
    schedule_id: &str,
) -> AppResult<ValidationResult> {
    crate::text::normalize_required_text(schedule_id)?;
    let mut connection = database::open_validation_reader(paths).await?;
    // Every exit (including failed loads) ends the read transaction and closes
    // this dedicated connection before any CPU validation or result is returned.
    let loaded = async {
        let mut tx = connection.begin().await?;
        let result =
            load_validation_snapshot(&mut tx, schedule_id, ValidationLimits::default()).await;
        tx.rollback().await?;
        result
    }
    .await;
    connection.close().await?;
    let snapshot = loaded?;
    tauri::async_runtime::spawn_blocking(move || validate_snapshot(snapshot))
        .await
        .map_err(|_| {
            AppError::new(
                ErrorCode::ConfigurationInvalid,
                "Schedule validation could not complete.",
            )
        })?
}
