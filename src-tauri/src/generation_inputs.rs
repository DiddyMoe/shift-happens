//! Bounded, note-free source reads and complete structural preflight.
use crate::{
    availability_pto::{AvailabilityEntry, PtoEntry},
    availability_rules::AvailabilityPtoIndex,
    coverage::{self, CoverageItem, CoverageTemplate},
    employee_restrictions::{EmployeeRestriction, RestrictionIndex},
    employees::{self, Employee, RulePeriod},
    error::{AppError, AppResult, ErrorCode},
    generation_snapshot::{
        ASSIGNMENT_LIMIT, CapturedSetting, GENERATOR_VERSION, GenerationSettingsSnapshot,
        SelectedCoverage,
    },
    holidays::{self, CoverageBehavior, Holiday},
    local_values::{
        ANALYSIS_EMPLOYEE_LIMIT, ExpandedIntervalBudget, GENERATION_CANDIDATE_LIMIT, LocalDate,
        LocalDateRange, LocalDateTime, LocalInterval, SourceRowBudget, Weekday,
        anchored_two_week_range, input_limit_error, workweek_range,
    },
    position_restrictions::PositionRestriction,
    positions::Position,
    schedule_assignments::ValidatedCoverageSlot,
    settings::{self, SettingValue, SettingView},
    shift_templates::ShiftTemplate,
    template_restrictions::TemplateRestriction,
};
use serde::Serialize;
use sqlx::{FromRow, QueryBuilder, Sqlite, Transaction, sqlite::SqliteRow};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn generation_error(mut error: AppError) -> AppError {
    if matches!(error.code, ErrorCode::InputLimitExceeded) {
        error.code = ErrorCode::GenerationInputLimitExceeded;
    }
    error
}
pub(crate) fn arithmetic_error() -> AppError {
    AppError::new(
        ErrorCode::OutOfRange,
        "Generation arithmetic exceeds the supported range; nothing was created.",
    )
}
pub(crate) fn add(a: i64, b: i64) -> AppResult<i64> {
    a.checked_add(b).ok_or_else(arithmetic_error)
}
pub(crate) fn fatal(
    code: ErrorCode,
    message: &'static str,
    date: Option<LocalDate>,
    id: Option<&str>,
) -> AppError {
    let mut error = AppError::new(code, message);
    error.context.key = date.map(LocalDate::format);
    error.context.id = id.map(str::to_owned);
    error
}

pub(crate) struct Policy {
    pub workweek: Weekday,
    pub anchor: Option<LocalDate>,
    pub holiday_year: i64,
    pub consecutive: i64,
    pub quick: bool,
    pub rest: i64,
    pub overtime: i64,
}
impl Policy {
    pub fn from_settings(settings: &[SettingView]) -> AppResult<Self> {
        let value = |key: &str| {
            settings
                .iter()
                .find(|s| s.key == key)
                .map(|s| s.value.as_ref())
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::InvalidSetting,
                        "A generation setting is missing.",
                    )
                    .setting(key)
                })
        };
        let integer = |key: &str| match value(key)? {
            Some(SettingValue::Integer(v)) => Ok(*v),
            _ => Err(AppError::new(
                ErrorCode::InvalidSetting,
                "A generation setting has the wrong type.",
            )
            .setting(key)),
        };
        Ok(Self {
            workweek: match value("workweek_start_weekday")? {
                Some(SettingValue::Weekday(w)) => *w,
                _ => return Err(arithmetic_error()),
            },
            anchor: match value("two_week_period_anchor_date")? {
                None => None,
                Some(SettingValue::Date(d)) => Some(LocalDate::parse(d)?),
                _ => return Err(arithmetic_error()),
            },
            holiday_year: integer("holiday_rotation_start_year")?,
            consecutive: integer("max_consecutive_workdays")?,
            quick: match value("quick_turnaround_rule_enabled")? {
                Some(SettingValue::Boolean(v)) => *v,
                _ => return Err(arithmetic_error()),
            },
            rest: integer("minimum_rest_minutes_between_shifts")?,
            overtime: integer("weekly_overtime_threshold_minutes")?,
        })
    }
    pub fn period(&self, date: LocalDate, period: RulePeriod) -> AppResult<LocalDateRange> {
        match period {
            RulePeriod::Week => workweek_range(date, self.workweek),
            RulePeriod::TwoWeek => anchored_two_week_range(date, self.anchor.ok_or_else(|| AppError::new(ErrorCode::TwoWeekAnchorRequired, "Set the two-week payroll anchor in Settings / About before generating schedules for active two-week rules."))?),
            RulePeriod::None => Err(arithmetic_error()),
        }
    }
}

// Explicit projections never select notes. The service uses supported schema only;
// unknown enum/row shapes fail closed, rather than inventing legacy recurrence.
pub(crate) const EMPLOYEE_COLUMNS: &str = "id,first_name,last_name,display_name,employment_type,active_status,holiday_group,can_work_weekends,can_work_holidays,can_work_night_shift,can_work_ortho_or,preferred_time_of_day,requires_explicit_availability,minimum_paid_minutes,minimum_assignment_count,minimum_period,maximum_paid_minutes,maximum_period,weekend_rotation_pattern,weekend_rotation_anchor_date,NULL AS notes,created_at,updated_at,archived_at,revision";
const COVERAGE_COLUMNS: &str = "id,seed_key,name,day_type,is_default,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
const ITEM_COLUMNS: &str = "id,seed_key,coverage_template_id,shift_position_id,shift_template_id,display_label,sort_order,required_count,actual_start_day_offset,actual_end_day_offset,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
const POSITION_COLUMNS: &str = "id,seed_key,name,display_label,sort_order,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
const SHIFT_COLUMNS: &str = "id,seed_key,name,display_label,start_time,end_time,time_of_day,unpaid_lunch_minutes,paid_minutes,requires_recovery_after_shift,is_night_shift,color_hex,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
const HOLIDAY_COLUMNS: &str = "id,seed_key,name,holiday_set,date_kind,month,day_of_month,weekday,nth,exact_date,coverage_behavior,custom_coverage_template_id,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
pub(crate) const RESTRICTION_COLUMNS: &str = "id,employee_id,restriction_kind,time_of_day,weekday,start_time,end_time,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
pub(crate) const AVAILABILITY_COLUMNS: &str = "id,employee_id,start_date,end_date,is_full_day,start_time,end_time,entry_type,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
pub(crate) const PTO_COLUMNS: &str = "id,employee_id,start_date,end_date,is_full_day,start_time,end_time,status,active_status,NULL AS notes,created_at,updated_at,archived_at,revision";
// Malformed active hard-rule rows must reach the authoritative shape validators;
// a lexical range/kind filter cannot silently make them appear irrelevant.
pub(crate) const RESTRICTION_FILTER: &str = "active_status='active' AND archived_at IS NULL AND employee_id IN (SELECT id FROM employees WHERE active_status='active' AND archived_at IS NULL) AND (restriction_kind='time_of_day_block' OR weekday IN (SELECT value FROM json_each(?3)) OR restriction_kind IS NULL OR restriction_kind NOT IN ('time_of_day_block','recurring_day_time_block') OR weekday IS NULL OR weekday NOT IN ('sunday','monday','tuesday','wednesday','thursday','friday','saturday'))";
pub(crate) const AVAILABILITY_FILTER: &str = "active_status='active' AND archived_at IS NULL AND employee_id IN (SELECT value FROM json_each(?3)) AND (start_date<=?2 AND end_date>=?1 OR date(start_date,'+0 days') IS NULL OR date(end_date,'+0 days') IS NULL OR date(start_date,'+0 days')<>start_date OR date(end_date,'+0 days')<>end_date OR start_date>end_date)";
pub(crate) const PTO_FILTER: &str = "active_status='active' AND archived_at IS NULL AND (status IN ('requested','approved') OR status IS NULL OR status NOT IN ('denied','canceled')) AND employee_id IN (SELECT value FROM json_each(?3)) AND (start_date<=?2 AND end_date>=?1 OR date(start_date,'+0 days') IS NULL OR date(end_date,'+0 days') IS NULL OR date(start_date,'+0 days')<>start_date OR date(end_date,'+0 days')<>end_date OR start_date>end_date)";
// Generation and replacement analysis use the same source-activity boundary.
// Historical assignments keep their snapshots after a source is retired, while
// current source-specific restrictions stop applying until it is active again.
pub(crate) const TEMPLATE_BLOCK_FILTER: &str = "active_status='active' AND archived_at IS NULL AND shift_template_id IN (SELECT value FROM json_each(?3)) AND shift_template_id IN (SELECT id FROM shift_templates WHERE active_status='active' AND archived_at IS NULL) AND employee_id IN (SELECT id FROM employees WHERE active_status='active' AND archived_at IS NULL)";
pub(crate) const POSITION_BLOCK_FILTER: &str = "active_status='active' AND archived_at IS NULL AND shift_position_id IN (SELECT value FROM json_each(?3)) AND shift_position_id IN (SELECT id FROM shift_positions WHERE active_status='active' AND archived_at IS NULL) AND employee_id IN (SELECT id FROM employees WHERE active_status='active' AND archived_at IS NULL)";

pub(crate) fn json_ids<'a>(ids: impl IntoIterator<Item = &'a str>) -> AppResult<String> {
    serde_json::to_string(&ids.into_iter().collect::<BTreeSet<_>>()).map_err(|_| arithmetic_error())
}
/// All callers pass repository-owned SQL fragments, never manager text. Count
/// each physical row once before materializing it in the same transaction view.
pub(crate) async fn load_counted<T>(
    tx: &mut Transaction<'_, Sqlite>,
    budget: &mut SourceRowBudget,
    sql_reads: &mut i64,
    source: (&'static str, &'static str, &'static str),
    window: (&str, &str, &str),
) -> AppResult<Vec<T>>
where
    T: for<'r> FromRow<'r, SqliteRow> + Send + Unpin,
{
    *sql_reads = add(*sql_reads, 2)?;
    let (table, columns, filter) = source;
    let (first, last, ids) = window;
    let mut count_query = QueryBuilder::<Sqlite>::new(
        "SELECT count(*),coalesce(sum(length(cast(id AS BLOB))>?4),0) FROM ",
    );
    count_query
        .push(table)
        .push(" WHERE ")
        .push(filter)
        .push(" AND ?1<=?2 AND ?3 IS NOT NULL");
    let (count, invalid_ids): (i64, i64) = count_query
        .build_query_as()
        .bind(first)
        .bind(last)
        .bind(ids)
        .bind(crate::error::DIAGNOSTIC_SOURCE_ID_BYTES as i64)
        .fetch_one(&mut **tx)
        .await?;
    if invalid_ids != 0 {
        return Err(AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A generation source ID exceeds the supported diagnostic byte limit.",
        ));
    }
    budget.reserve(count)?;
    let mut query = QueryBuilder::<Sqlite>::new("SELECT ");
    query
        .push(columns)
        .push(" FROM ")
        .push(table)
        .push(" WHERE ")
        .push(filter)
        .push(" AND ?1<=?2 AND ?3 IS NOT NULL ORDER BY id");
    query
        .build_query_as::<T>()
        .bind(first)
        .bind(last)
        .bind(ids)
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| {
            AppError::new(
                ErrorCode::ConfigurationInvalid,
                "A generation source has unsupported or corrupt fields; nothing was created.",
            )
        })
}

pub(crate) struct PlannedSlot {
    pub slot: ValidatedCoverageSlot,
    pub date: LocalDate,
    pub interval: LocalInterval,
    pub week: LocalDateRange,
    pub two_week: Option<LocalDateRange>,
    pub holiday_group: Option<employees::HolidayGroup>,
}
#[derive(Serialize)]
pub struct PartialMinimumPeriod {
    pub period: RulePeriod,
    pub start_date: String,
    pub end_exclusive: String,
    pub employee_count: i64,
}
#[derive(Serialize)]
pub struct GenerationReadiness {
    pub active_employee_count: usize,
    pub planned_slot_count: usize,
    pub candidate_evaluation_count: usize,
    pub current_setting_revision: i64,
    pub two_week_anchor_required: bool,
    pub two_week_anchor_date: Option<String>,
    pub partial_minimum_periods: Vec<PartialMinimumPeriod>,
}
pub(crate) struct RuleInputs {
    pub employees: Vec<Employee>,
    pub slots: Vec<PlannedSlot>,
    pub policy: Policy,
    pub restrictions: RestrictionIndex,
    pub availability: AvailabilityPtoIndex,
    pub template_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    pub position_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}
pub(crate) struct Inputs {
    pub rules: RuleInputs,
    pub snapshot: GenerationSettingsSnapshot,
    pub readiness: GenerationReadiness,
    pub source_count: usize,
    pub sql_read_count: i64,
    pub expanded_count: usize,
}
impl std::ops::Deref for Inputs {
    type Target = RuleInputs;
    fn deref(&self) -> &Self::Target {
        &self.rules
    }
}

pub(crate) async fn preflight(
    tx: &mut Transaction<'_, Sqlite>,
    start: LocalDate,
    end: LocalDate,
    expected_current: Option<i64>,
) -> AppResult<Inputs> {
    let settings = settings::read_in_transaction(tx).await?;
    let mut sql_reads = settings::REGISTRY.len() as i64;
    let policy = Policy::from_settings(&settings)?;
    let current_revision = settings
        .iter()
        .find(|s| s.key == "current_schedule_id")
        .ok_or_else(arithmetic_error)?
        .revision;
    if let Some(expected) = expected_current {
        employees::check_revision("current_schedule_id", current_revision, expected)?;
    }
    sql_reads = add(sql_reads, 2)?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM employees WHERE active_status='active' AND archived_at IS NULL",
    )
    .fetch_one(&mut **tx)
    .await?;
    if count > ANALYSIS_EMPLOYEE_LIMIT as i64 {
        return Err(input_limit_error(
            "employee_rows",
            ANALYSIS_EMPLOYEE_LIMIT,
            count as usize,
        ));
    }
    let mut employee_query = QueryBuilder::<Sqlite>::new("SELECT ");
    employee_query
        .push(EMPLOYEE_COLUMNS)
        .push(" FROM employees WHERE active_status='active' AND archived_at IS NULL ORDER BY id");
    let employees = employee_query
        .build_query_as::<Employee>()
        .fetch_all(&mut **tx)
        .await?;
    for employee in &employees {
        employees::verify_employee(employee)?;
    }
    let anchor_required = employees.iter().any(|e| {
        e.data.minimum_period == RulePeriod::TwoWeek || e.data.maximum_period == RulePeriod::TwoWeek
    });
    if anchor_required && policy.anchor.is_none() {
        return Err(AppError::new(
            ErrorCode::TwoWeekAnchorRequired,
            "Active employees have two-week rules. Set the payroll period anchor in Settings / About, then reload readiness.",
        ));
    }
    // No date or source expansion occurs before the mandatory anchor check.
    let mut dates = Vec::new();
    let mut weekdays = BTreeSet::new();
    let mut months = BTreeSet::new();
    let mut date = start;
    loop {
        dates.push(date);
        weekdays.insert(date.weekday().as_str());
        months.insert(
            date.format()[5..7]
                .parse::<i64>()
                .map_err(|_| arithmetic_error())?,
        );
        if date == end {
            break;
        }
        date = date.add_days(1)?;
    }
    let first = start.format();
    let last = end.format();
    let mut budget = SourceRowBudget::default();
    let months = serde_json::to_string(&months).map_err(|_| arithmetic_error())?;
    let holidays: Vec<Holiday> = load_counted(tx, &mut budget, &mut sql_reads, ("holidays", HOLIDAY_COLUMNS, "active_status='active' AND archived_at IS NULL AND ((date_kind='exact_date' AND exact_date BETWEEN ?1 AND ?2) OR (date_kind<>'exact_date' AND month IN (SELECT value FROM json_each(?3))))"), (&first, &last, &months)).await?;
    let resolved = holidays::resolve_active_holidays(&holidays, start, end)?;
    holidays::require_no_collisions(&holidays::collision_preview(&resolved, start, end)?)?;
    let holidays_by_date: BTreeMap<_, _> = resolved
        .iter()
        .map(|h| (h.date.as_str(), h.id.as_str()))
        .collect();
    let holiday_rows: BTreeMap<_, _> = holidays.iter().map(|h| (h.id.as_str(), h)).collect();
    let mut coverage_ids = BTreeSet::new();
    let mut defaults = BTreeMap::new();
    // At most seven bounded selection reads; alternative templates are never chosen.
    for weekday in weekdays {
        let needed = dates
            .iter()
            .filter(|d| d.weekday().as_str() == weekday)
            .any(|d| {
                holidays_by_date
                    .get(d.format().as_str())
                    .and_then(|id| holiday_rows.get(id))
                    .is_none_or(|h| h.data.coverage_behavior == CoverageBehavior::Normal)
            });
        if !needed {
            continue;
        }
        sql_reads = add(sql_reads, 1)?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM coverage_templates WHERE day_type=? AND is_default=1 AND active_status='active' AND archived_at IS NULL").bind(weekday).fetch_one(&mut **tx).await?;
        if count == 0 {
            return Err(fatal(
                ErrorCode::CoverageMissing,
                "Select one active normal default for this weekday in Positions and Coverage.",
                None,
                Some(weekday),
            ));
        }

        if count != 1 {
            return Err(fatal(
                ErrorCode::CoverageAmbiguous,
                "More than one normal default is selected for a weekday. Resolve coverage selection before generation.",
                None,
                Some(weekday),
            ));
        }
        sql_reads = add(sql_reads, 1)?;
        let id: String = sqlx::query_scalar("SELECT id FROM coverage_templates WHERE day_type=? AND is_default=1 AND active_status='active' AND archived_at IS NULL").bind(weekday).fetch_one(&mut **tx).await?;
        defaults.insert(weekday, id.clone());
        coverage_ids.insert(id);
    }
    for holiday in &resolved {
        if holiday.coverage_behavior == CoverageBehavior::Custom {
            coverage_ids.insert(
                holiday
                    .custom_coverage_template_id
                    .clone()
                    .ok_or_else(arithmetic_error)?,
            );
        }
    }
    let ids = json_ids(coverage_ids.iter().map(String::as_str))?;
    let templates: Vec<CoverageTemplate> = load_counted(
        tx,
        &mut budget,
        &mut sql_reads,
        (
            "coverage_templates",
            COVERAGE_COLUMNS,
            "id IN (SELECT value FROM json_each(?3))",
        ),
        (&first, &last, &ids),
    )
    .await?;
    let templates: BTreeMap<_, _> = templates.into_iter().map(|t| (t.id.clone(), t)).collect();
    let mut selections = Vec::new();
    let mut selected = BTreeMap::new();
    let mut planned_count = 0i64;
    let mut template_counts = BTreeMap::new();
    for id in &coverage_ids {
        let template = templates.get(id).ok_or_else(|| {
            fatal(
                ErrorCode::CoverageSourceInvalid,
                "A selected coverage source is missing.",
                None,
                Some(id),
            )
        })?;
        coverage::verify_template(template)?;
        if template.archived_at.is_some()
            || template.data.active_status != employees::ActiveStatus::Active
        {
            return Err(fatal(
                ErrorCode::CoverageSourceInvalid,
                "Selected coverage must be active and non-archived.",
                None,
                Some(id),
            ));
        }
        sql_reads = add(sql_reads, 2)?;
        let bad: i64 = sqlx::query_scalar("SELECT count(*) FROM coverage_template_items WHERE coverage_template_id=? AND active_status='active' AND archived_at IS NULL AND (typeof(required_count)<>'integer' OR required_count NOT BETWEEN 1 AND 100)").bind(id).fetch_one(&mut **tx).await?;
        if bad != 0 {
            return Err(fatal(
                ErrorCode::InvalidRequiredCount,
                "Repair invalid required counts in the selected coverage.",
                None,
                Some(id),
            ));
        }
        let count: i64 = sqlx::query_scalar("SELECT coalesce(sum(required_count),0) FROM coverage_template_items WHERE coverage_template_id=? AND active_status='active' AND archived_at IS NULL").bind(id).fetch_one(&mut **tx).await?;
        template_counts.insert(id.clone(), count);
    }
    for date in dates {
        let date_text = date.format();
        let holiday = holidays_by_date
            .get(date_text.as_str())
            .and_then(|id| holiday_rows.get(id))
            .copied();
        let id = match holiday {
            Some(h) if h.data.coverage_behavior == CoverageBehavior::Custom => h
                .data
                .custom_coverage_template_id
                .as_ref()
                .ok_or_else(arithmetic_error)?,
            _ => defaults
                .get(date.weekday().as_str())
                .ok_or_else(arithmetic_error)?,
        };
        let count = *template_counts.get(id).ok_or_else(arithmetic_error)?;
        if count == 0 {
            return Err(fatal(
                ErrorCode::CoverageEmpty,
                "Add active items to the selected coverage template before generation.",
                Some(date),
                Some(id),
            ));
        }
        planned_count = add(planned_count, count)?;
        if planned_count > ASSIGNMENT_LIMIT as i64 {
            let mut error = fatal(
                ErrorCode::PlannedSlotLimitExceeded,
                "The complete plan exceeds 100,000 slots. Reduce the range or required counts.",
                Some(date),
                Some(id),
            );
            error.context.limit_kind = Some("planned_slots");
            error.context.limit = Some(ASSIGNMENT_LIMIT);
            error.context.observed_count = Some(planned_count as usize);
            return Err(error);
        }
        let template = templates.get(id).ok_or_else(arithmetic_error)?;
        selected.insert(
            date_text,
            SelectedCoverage {
                id: id.clone(),
                seed_key: template.seed_key.clone(),
                revision: template.revision,
            },
        );
        selections.push((date, id.clone(), holiday.map(|h| h.id.clone())));
    }
    // Check the full product before allocating the per-slot plan or evaluating any candidate.
    let evaluations = planned_count.checked_mul(count).ok_or_else(|| {
        input_limit_error(
            "candidate_evaluations",
            GENERATION_CANDIDATE_LIMIT,
            usize::MAX,
        )
    })?;
    if evaluations > GENERATION_CANDIDATE_LIMIT as i64 {
        return Err(input_limit_error(
            "candidate_evaluations",
            GENERATION_CANDIDATE_LIMIT,
            evaluations as usize,
        ));
    }
    let items: Vec<CoverageItem> = load_counted(tx, &mut budget, &mut sql_reads, ("coverage_template_items", ITEM_COLUMNS, "active_status='active' AND archived_at IS NULL AND coverage_template_id IN (SELECT value FROM json_each(?3))"), (&first, &last, &ids)).await?;
    let position_ids = json_ids(items.iter().map(|i| i.data.shift_position_id.as_str()))?;
    let shift_ids = json_ids(items.iter().map(|i| i.data.shift_template_id.as_str()))?;
    let positions: Vec<Position> = load_counted(
        tx,
        &mut budget,
        &mut sql_reads,
        (
            "shift_positions",
            POSITION_COLUMNS,
            "id IN (SELECT value FROM json_each(?3))",
        ),
        (&first, &last, &position_ids),
    )
    .await?;
    let shifts: Vec<ShiftTemplate> = load_counted(
        tx,
        &mut budget,
        &mut sql_reads,
        (
            "shift_templates",
            SHIFT_COLUMNS,
            "id IN (SELECT value FROM json_each(?3))",
        ),
        (&first, &last, &shift_ids),
    )
    .await?;
    let employee_ids = json_ids(employees.iter().map(|e| e.id.as_str()))?;
    let shift_rows: BTreeMap<_, _> = shifts.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut items_by_template: BTreeMap<&str, Vec<&CoverageItem>> = BTreeMap::new();
    for item in &items {
        items_by_template
            .entry(&item.data.coverage_template_id)
            .or_default()
            .push(item);
    }
    let mut actual_first = None;
    let mut actual_last = None;
    for (date, id, _) in &selections {
        for item in items_by_template
            .get(id.as_str())
            .ok_or_else(arithmetic_error)?
        {
            coverage::verify_item(item)?;
            let shift = shift_rows
                .get(item.data.shift_template_id.as_str())
                .ok_or_else(|| {
                    fatal(
                        ErrorCode::CoverageSourceInvalid,
                        "A coverage shift source is missing.",
                        Some(*date),
                        Some(&item.id),
                    )
                })?;
            let actual = coverage::coverage_interval(
                *date,
                &shift.data,
                item.data.actual_start_day_offset,
                item.data.actual_end_day_offset,
            )?;
            let start = LocalDateTime::parse(&actual.actual_start)?;
            let end = LocalDateTime::parse(&actual.actual_end)?;
            actual_first = Some(actual_first.map_or(start, |old: LocalDateTime| old.min(start)));
            actual_last = Some(actual_last.map_or(end, |old: LocalDateTime| old.max(end)));
        }
    }
    let actual_first = actual_first
        .ok_or_else(arithmetic_error)?
        .date()
        .subtract_days(1)?;
    let actual_last = actual_last.ok_or_else(arithmetic_error)?.date();
    let source_first = actual_first.format();
    let source_last = actual_last.format();
    let mut relevant_weekdays = BTreeSet::new();
    let mut source_date = actual_first;
    loop {
        relevant_weekdays.insert(source_date.weekday().as_str());
        if source_date == actual_last {
            break;
        }
        source_date = source_date.add_days(1)?;
    }
    let restriction_days = json_ids(relevant_weekdays)?;
    let restrictions: Vec<EmployeeRestriction> = load_counted(
        tx,
        &mut budget,
        &mut sql_reads,
        (
            "employee_restrictions",
            RESTRICTION_COLUMNS,
            RESTRICTION_FILTER,
        ),
        (&source_first, &source_last, &restriction_days),
    )
    .await?;
    let availability: Vec<AvailabilityEntry> = load_counted(
        tx,
        &mut budget,
        &mut sql_reads,
        (
            "availability_entries",
            AVAILABILITY_COLUMNS,
            AVAILABILITY_FILTER,
        ),
        (&source_first, &source_last, &employee_ids),
    )
    .await?;
    let pto: Vec<PtoEntry> = load_counted(
        tx,
        &mut budget,
        &mut sql_reads,
        ("pto_entries", PTO_COLUMNS, PTO_FILTER),
        (&source_first, &source_last, &employee_ids),
    )
    .await?;
    let template_restrictions: Vec<TemplateRestriction> = load_counted(tx, &mut budget, &mut sql_reads, ("employee_shift_template_restrictions", "id,employee_id,shift_template_id,restriction_type,active_status,NULL AS notes,created_at,updated_at,archived_at,revision", TEMPLATE_BLOCK_FILTER), (&first, &last, &shift_ids)).await?;
    let position_restrictions: Vec<PositionRestriction> = load_counted(tx, &mut budget, &mut sql_reads, ("employee_position_restrictions", "id,employee_id,shift_position_id,restriction_type,active_status,NULL AS notes,created_at,updated_at,archived_at,revision", POSITION_BLOCK_FILTER), (&first, &last, &position_ids)).await?;
    let captured = settings
        .into_iter()
        .filter(|s| crate::generation_snapshot::SNAPSHOT_SETTINGS.contains(&s.key))
        .map(|s| {
            (
                s.key.to_owned(),
                CapturedSetting {
                    value: s.value,
                    revision: s.revision,
                },
            )
        })
        .collect();
    let snapshot = GenerationSettingsSnapshot {
        version: 1,
        generator_version: GENERATOR_VERSION.to_owned(),
        expected_assignment_count: u32::try_from(planned_count).map_err(|_| arithmetic_error())?,
        settings: captured,
        selected_coverage_by_date: selected,
    };
    let source_count = budget.count;
    // CPU-only expansion/ranking work runs off the async executor. Generation
    // deliberately retains its immediate transaction until the complete commit.
    tauri::async_runtime::spawn_blocking(move || {
        let positions: BTreeMap<_, _> = positions.into_iter().map(|p| (p.id.clone(), p)).collect();
        let shifts: BTreeMap<_, _> = shifts.into_iter().map(|s| (s.id.clone(), s)).collect();
        let holidays: BTreeMap<_, _> = holidays.into_iter().map(|h| (h.id.clone(), h)).collect();
        let mut item_map: BTreeMap<&str, Vec<&CoverageItem>> = BTreeMap::new();
        for item in &items {
            item_map
                .entry(&item.data.coverage_template_id)
                .or_default()
                .push(item);
        }
        let mut slots = Vec::with_capacity(planned_count as usize);
        let mut identities = BTreeSet::new();
        for (date, id, holiday_id) in selections {
            let template = templates.get(&id).ok_or_else(arithmetic_error)?;
            let holiday = holiday_id.as_ref().and_then(|id| holidays.get(id));
            let week = policy.period(date, RulePeriod::Week)?;
            let two_week = if anchor_required {
                Some(policy.period(date, RulePeriod::TwoWeek)?)
            } else {
                None
            };
            let holiday_group = if let Some(holiday) = holiday {
                holidays::match_holiday_group(
                    holiday,
                    employees::HolidayGroup::None,
                    date.year(),
                    policy.holiday_year,
                )?
                .required_group
            } else {
                None
            };
            for item in item_map.get(id.as_str()).ok_or_else(arithmetic_error)? {
                let position = positions.get(&item.data.shift_position_id).ok_or_else(|| {
                    fatal(
                        ErrorCode::CoverageSourceInvalid,
                        "A coverage position is missing.",
                        Some(date),
                        Some(&item.id),
                    )
                })?;
                let shift = shifts.get(&item.data.shift_template_id).ok_or_else(|| {
                    fatal(
                        ErrorCode::CoverageSourceInvalid,
                        "A coverage shift is missing.",
                        Some(date),
                        Some(&item.id),
                    )
                })?;
                for index in 1..=item.data.required_count {
                    let slot = ValidatedCoverageSlot::from_sources(
                        date, item, template, position, shift, holiday, index,
                    )
                    .map_err(|mut error| {
                        error.context.id = Some(item.id.clone());
                        error.context.key = Some(date.format());
                        error
                    })?;
                    let s = slot.structure();
                    if !identities.insert((date, item.id.clone(), index)) {
                        return Err(fatal(
                            ErrorCode::DuplicatePlannedSlot,
                            "A duplicate structural slot was found; nothing was created.",
                            Some(date),
                            Some(&item.id),
                        ));
                    }
                    let interval = LocalInterval::new(
                        LocalDateTime::parse(&s.actual_start_datetime)?,
                        LocalDateTime::parse(&s.actual_end_datetime)?,
                    )?;
                    slots.push(PlannedSlot {
                        slot,
                        date,
                        interval,
                        week: LocalDateRange {
                            start: week.start,
                            end_exclusive: week.end_exclusive,
                        },
                        two_week: two_week.as_ref().map(|p| LocalDateRange {
                            start: p.start,
                            end_exclusive: p.end_exclusive,
                        }),
                        holiday_group,
                    });
                }
            }
        }
        if slots.len() != planned_count as usize {
            return Err(arithmetic_error());
        }
        slots.sort_unstable_by(|a, b| {
            let a = a.slot.structure();
            let b = b.slot.structure();
            (
                &a.schedule_date,
                a.coverage_template_seed_key_snapshot
                    .as_deref()
                    .unwrap_or(&a.coverage_template_id),
                a.coverage_sort_order_snapshot,
                a.coverage_item_seed_key_snapshot
                    .as_deref()
                    .unwrap_or(&a.coverage_item_id),
                a.coverage_slot_index_snapshot,
                &a.actual_start_datetime,
                &a.coverage_item_id,
            )
                .cmp(&(
                    &b.schedule_date,
                    b.coverage_template_seed_key_snapshot
                        .as_deref()
                        .unwrap_or(&b.coverage_template_id),
                    b.coverage_sort_order_snapshot,
                    b.coverage_item_seed_key_snapshot
                        .as_deref()
                        .unwrap_or(&b.coverage_item_id),
                    b.coverage_slot_index_snapshot,
                    &b.actual_start_datetime,
                    &b.coverage_item_id,
                ))
        });
        let windows: Vec<_> = slots.iter().map(|s| s.interval).collect();
        let mut expanded = ExpandedIntervalBudget::default();
        let restrictions = RestrictionIndex::build(&restrictions, &windows, &mut expanded)?;
        let availability =
            AvailabilityPtoIndex::build(&availability, &pto, &windows, &mut expanded)?;
        let mut template_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for r in template_restrictions {
            crate::template_restrictions::verify_restriction(&r)?;
            template_blocks
                .entry(r.employee_id)
                .or_default()
                .entry(r.data.shift_template_id)
                .or_default()
                .push(r.id);
        }
        let mut position_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for r in position_restrictions {
            crate::position_restrictions::verify_restriction(&r)?;
            position_blocks
                .entry(r.employee_id)
                .or_default()
                .entry(r.data.shift_position_id)
                .or_default()
                .push(r.id);
        }
        let operational_dates: BTreeSet<_> = slots.iter().map(|s| s.date).collect();
        let mut partial: BTreeMap<(&str, LocalDate, LocalDate), (RulePeriod, BTreeSet<&str>)> =
            BTreeMap::new();
        for employee in &employees {
            if employee.data.minimum_period == RulePeriod::None {
                continue;
            }
            for &date in &operational_dates {
                let period = policy.period(date, employee.data.minimum_period)?;
                if period.start < start || period.end_exclusive.days_since(end) > 1 {
                    let key = (
                        if employee.data.minimum_period == RulePeriod::Week {
                            "week"
                        } else {
                            "two_week"
                        },
                        period.start,
                        period.end_exclusive,
                    );
                    partial
                        .entry(key)
                        .or_insert_with(|| (employee.data.minimum_period, BTreeSet::new()))
                        .1
                        .insert(&employee.id);
                }
            }
        }
        let partial_minimum_periods = partial
            .into_iter()
            .map(|((_, start, end), (period, ids))| PartialMinimumPeriod {
                period,
                start_date: start.format(),
                end_exclusive: end.format(),
                employee_count: ids.len() as i64,
            })
            .collect();
        let readiness = GenerationReadiness {
            active_employee_count: employees.len(),
            planned_slot_count: slots.len(),
            candidate_evaluation_count: evaluations as usize,
            current_setting_revision: current_revision,
            two_week_anchor_required: anchor_required,
            two_week_anchor_date: policy.anchor.map(LocalDate::format),
            partial_minimum_periods,
        };
        snapshot.validate()?;
        Ok(Inputs {
            rules: RuleInputs {
                employees,
                slots,
                policy,
                restrictions,
                availability,
                template_blocks,
                position_blocks,
            },
            snapshot,
            readiness,
            source_count,
            sql_read_count: sql_reads,
            expanded_count: expanded.count,
        })
    })
    .await
    .map_err(|_| arithmetic_error())?
}
