use crate::{
    error::{AppError, AppResult, CommandResult, ErrorCode},
    local_values::{LocalDate, Weekday},
    metadata::{MetadataClock, clock_timestamp, parse_metadata_instant},
    text::{
        compare_text_with_ids, normalize_notes, normalize_required_text, stable_text_order_key,
    },
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

pub const EMPLOYEE_RULE_LIMIT: i64 = 1_000_000;
pub const MAX_REVISION: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum EmploymentType {
    FullTime,
    PartTime,
    Prn,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum ActiveStatus {
    Active,
    Inactive,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum HolidayGroup {
    A,
    B,
    None,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum TimeOfDay {
    Day,
    Evening,
    Night,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum RulePeriod {
    Week,
    TwoWeek,
    None,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum WeekendPattern {
    None,
    EveryOtherWeekend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct EmployeeData {
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub employment_type: EmploymentType,
    pub active_status: ActiveStatus,
    pub holiday_group: HolidayGroup,
    pub can_work_weekends: bool,
    pub can_work_holidays: bool,
    pub can_work_night_shift: bool,
    pub can_work_ortho_or: bool,
    pub preferred_time_of_day: Option<TimeOfDay>,
    pub requires_explicit_availability: bool,
    pub minimum_paid_minutes: Option<i64>,
    pub minimum_assignment_count: Option<i64>,
    pub minimum_period: RulePeriod,
    pub maximum_paid_minutes: Option<i64>,
    pub maximum_period: RulePeriod,
    pub weekend_rotation_pattern: WeekendPattern,
    pub weekend_rotation_anchor_date: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Employee {
    pub id: String,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: EmployeeData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmploymentDefaults {
    pub minimum_paid_minutes: Option<i64>,
    pub minimum_assignment_count: Option<i64>,
    pub minimum_period: RulePeriod,
    pub requires_explicit_availability: bool,
}

pub fn employment_defaults(kind: EmploymentType) -> EmploymentDefaults {
    EmploymentDefaults {
        minimum_paid_minutes: match kind {
            EmploymentType::Prn => None,
            _ => Some(2400),
        },
        minimum_assignment_count: None,
        minimum_period: match kind {
            EmploymentType::FullTime => RulePeriod::Week,
            EmploymentType::PartTime => RulePeriod::TwoWeek,
            EmploymentType::Prn => RulePeriod::None,
        },
        requires_explicit_availability: false,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinimumRule {
    pub minimum_paid_minutes: Option<i64>,
    pub minimum_assignment_count: Option<i64>,
    pub minimum_period: RulePeriod,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaximumRule {
    pub maximum_paid_minutes: Option<i64>,
    pub maximum_period: RulePeriod,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeekendRule {
    pub weekend_rotation_pattern: WeekendPattern,
    pub weekend_rotation_anchor_date: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateEmployeeOptions {
    pub active_status: Option<ActiveStatus>,
    pub holiday_group: Option<HolidayGroup>,
    pub can_work_weekends: Option<bool>,
    pub can_work_holidays: Option<bool>,
    pub can_work_night_shift: Option<bool>,
    pub can_work_ortho_or: Option<bool>,
    pub preferred_time_of_day: Option<TimeOfDay>,
    pub requires_explicit_availability: Option<bool>,
    pub minimum: Option<MinimumRule>,
    pub maximum: Option<MaximumRule>,
    pub weekend: Option<WeekendRule>,
    pub notes: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateEmployeeRequest {
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub employment_type: EmploymentType,
    #[serde(default)]
    pub options: CreateEmployeeOptions,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEmployeeRequest {
    pub id: String,
    pub expected_revision: i64,
    pub employee: EmployeeData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionRequest {
    pub id: String,
    pub expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdRequest {
    pub id: String,
}
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordFilter {
    #[default]
    Current,
    Active,
    Inactive,
    Archived,
    All,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListEmployeesRequest {
    #[serde(default)]
    pub filter: RecordFilter,
    pub employment_type: Option<EmploymentType>,
    pub search: Option<String>,
}

pub fn invalid_employee(message: &'static str) -> AppError {
    AppError::new(ErrorCode::InvalidEmployee, message)
}
pub fn normalize_optional_notes(notes: Option<String>) -> AppResult<Option<String>> {
    Ok(notes
        .map(|v| normalize_notes(&v))
        .transpose()?
        .filter(|v| !v.is_empty()))
}
pub fn normalize_weekend_anchor(value: &str) -> AppResult<String> {
    let date = LocalDate::parse(value)?;
    match date.weekday() {
        Weekday::Saturday => Ok(date.format()),
        Weekday::Sunday => Ok(date.subtract_days(1)?.format()),
        _ => Err(invalid_employee(
            "Choose a Saturday or Sunday for the weekend anchor.",
        )),
    }
}
/// Operational weekend dates only. A weekday has no weekend-rotation decision.
pub fn weekend_is_on_rotation(
    operational_date: LocalDate,
    anchor: LocalDate,
) -> AppResult<Option<bool>> {
    if anchor.weekday() != Weekday::Saturday {
        return Err(invalid_employee(
            "The stored weekend anchor must be a Saturday.",
        ));
    }
    let saturday = match operational_date.weekday() {
        Weekday::Saturday => operational_date,
        Weekday::Sunday => operational_date.subtract_days(1)?,
        _ => return Ok(None),
    };
    Ok(Some(
        saturday.days_since(anchor).div_euclid(7).rem_euclid(2) == 0,
    ))
}

impl EmployeeData {
    pub fn normalized(mut self) -> AppResult<Self> {
        self.first_name = normalize_required_text(&self.first_name)?;
        self.last_name = normalize_required_text(&self.last_name)?;
        self.display_name = normalize_required_text(&self.display_name)?;
        self.notes = normalize_optional_notes(self.notes)?;
        for value in [
            self.minimum_paid_minutes,
            self.minimum_assignment_count,
            self.maximum_paid_minutes,
        ]
        .into_iter()
        .flatten()
        {
            if !(1..=EMPLOYEE_RULE_LIMIT).contains(&value) {
                return Err(invalid_employee(
                    "Hours and assignment counts must be more than zero and within the allowed range.",
                ));
            }
        }
        let valid_minimum = match self.minimum_period {
            RulePeriod::None => {
                self.minimum_paid_minutes.is_none() && self.minimum_assignment_count.is_none()
            }
            _ => self.minimum_paid_minutes.is_some() != self.minimum_assignment_count.is_some(),
        };
        if !valid_minimum {
            return Err(invalid_employee(
                "Choose one minimum metric with a weekly or two-week period, or no minimum.",
            ));
        }
        if (self.maximum_period == RulePeriod::None) != self.maximum_paid_minutes.is_none() {
            return Err(invalid_employee(
                "A maximum requires paid hours and a weekly or two-week period.",
            ));
        }
        if self.minimum_period == self.maximum_period
            && let (Some(min), Some(max)) = (self.minimum_paid_minutes, self.maximum_paid_minutes)
            && min > max
        {
            return Err(invalid_employee(
                "The minimum paid hours cannot exceed the maximum for the same period.",
            ));
        }
        match (
            self.weekend_rotation_pattern,
            self.weekend_rotation_anchor_date.as_deref(),
        ) {
            (WeekendPattern::None, None) => {}
            (WeekendPattern::EveryOtherWeekend, Some(anchor)) => {
                self.weekend_rotation_anchor_date = Some(normalize_weekend_anchor(anchor)?)
            }
            _ => {
                return Err(invalid_employee(
                    "Every-other-weekend rotation requires an anchor; no rotation requires an empty anchor.",
                ));
            }
        }
        Ok(self)
    }
    pub fn reset_minimum(&mut self) {
        let defaults = employment_defaults(self.employment_type);
        self.minimum_paid_minutes = defaults.minimum_paid_minutes;
        self.minimum_assignment_count = defaults.minimum_assignment_count;
        self.minimum_period = defaults.minimum_period;
        self.requires_explicit_availability = defaults.requires_explicit_availability;
    }
}

pub fn validate_revision(expected: i64) -> AppResult<()> {
    if !(1..=MAX_REVISION).contains(&expected) {
        return Err(AppError::new(
            ErrorCode::InvalidRevision,
            "Reload the record to obtain a valid revision.",
        ));
    }
    Ok(())
}
pub fn check_revision(id: &str, current: i64, expected: i64) -> AppResult<()> {
    validate_revision(expected)?;
    if current != expected {
        let mut error = AppError::new(
            ErrorCode::StaleRevision,
            "This record changed. Reload it before applying your changes.",
        );
        error.context.id = Some(id.to_owned());
        error.context.current_revision = Some(current);
        return Err(error);
    }
    Ok(())
}
pub fn require_not_archived(archived_at: &Option<String>) -> AppResult<()> {
    if archived_at.is_some() {
        return Err(AppError::new(
            ErrorCode::RecordArchived,
            "Archived records are read-only.",
        ));
    }
    Ok(())
}
pub fn verify_metadata(
    created: &str,
    updated: &str,
    archived: &Option<String>,
    revision: i64,
) -> AppResult<()> {
    parse_metadata_instant(created)?;
    parse_metadata_instant(updated)?;
    if let Some(value) = archived {
        parse_metadata_instant(value)?;
    }
    validate_revision(revision)
}
pub fn mutation_result<T>(result: AppResult<(T, bool)>) -> CommandResult<T> {
    match result {
        Ok((data, true)) => CommandResult::Success {
            data,
            warnings: Vec::new(),
        },
        Ok((data, false)) => CommandResult::NoChange { data },
        Err(error) => Err(error).into(),
    }
}

pub async fn read_employee(tx: &mut Transaction<'_, Sqlite>, id: &str) -> AppResult<Employee> {
    normalize_required_text(id)?;
    let employee = sqlx::query_as::<_, Employee>("SELECT * FROM employees WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "The employee could not be found."))?;
    verify_employee(&employee)?;
    Ok(employee)
}
pub(crate) fn verify_employee(employee: &Employee) -> AppResult<()> {
    if employee.data.clone().normalized()? != employee.data
        || (employee.archived_at.is_some() && employee.data.active_status != ActiveStatus::Inactive)
    {
        return Err(AppError::new(
            ErrorCode::ConfigurationInvalid,
            "The stored employee requires investigation; it has not been changed.",
        ));
    }
    verify_metadata(
        &employee.created_at,
        &employee.updated_at,
        &employee.archived_at,
        employee.revision,
    )
}
pub async fn get_employee(pool: &SqlitePool, request: IdRequest) -> AppResult<Employee> {
    let mut tx = pool.begin().await?;
    let employee = read_employee(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(employee)
}
pub async fn list_employees(
    pool: &SqlitePool,
    request: ListEmployeesRequest,
) -> AppResult<Vec<Employee>> {
    let search = request
        .search
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(normalize_required_text)
        .transpose()?
        .map(|v| stable_text_order_key(&v));
    let mut tx = pool.begin().await?;
    let (archive, active) = match request.filter {
        RecordFilter::Current => (Some(false), None),
        RecordFilter::Active => (Some(false), Some(ActiveStatus::Active)),
        RecordFilter::Inactive => (Some(false), Some(ActiveStatus::Inactive)),
        RecordFilter::Archived => (Some(true), None),
        RecordFilter::All => (None, None),
    };
    let mut employees = sqlx::query_as::<_, Employee>("SELECT * FROM employees WHERE (?1 IS NULL OR (archived_at IS NOT NULL) = ?1) AND (?2 IS NULL OR active_status = ?2) AND (?3 IS NULL OR employment_type = ?3) ORDER BY display_name_order_key, id")
        .bind(archive).bind(active).bind(request.employment_type).fetch_all(&mut *tx).await?;
    tx.rollback().await?;
    for employee in &employees {
        verify_employee(employee)?;
    }
    if let Some(search) = search {
        employees.retain(|e| {
            [&e.data.display_name, &e.data.first_name, &e.data.last_name]
                .iter()
                .any(|v| stable_text_order_key(v).contains(&search))
        });
    }
    employees.sort_by(|a, b| {
        compare_text_with_ids(&a.data.display_name, &a.id, &b.data.display_name, &b.id)
    });
    Ok(employees)
}

async fn write_employee(
    tx: &mut Transaction<'_, Sqlite>,
    employee: &Employee,
    create: bool,
) -> AppResult<()> {
    let sql = if create {
        "INSERT INTO employees (id, first_name, last_name, display_name, display_name_order_key, employment_type, active_status, holiday_group, can_work_weekends, can_work_holidays, can_work_night_shift, can_work_ortho_or, preferred_time_of_day, requires_explicit_availability, minimum_paid_minutes, minimum_assignment_count, minimum_period, maximum_paid_minutes, maximum_period, weekend_rotation_pattern, weekend_rotation_anchor_date, notes, created_at, updated_at, archived_at, revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)"
    } else {
        "UPDATE employees SET first_name=?2, last_name=?3, display_name=?4, display_name_order_key=?5, employment_type=?6, active_status=?7, holiday_group=?8, can_work_weekends=?9, can_work_holidays=?10, can_work_night_shift=?11, can_work_ortho_or=?12, preferred_time_of_day=?13, requires_explicit_availability=?14, minimum_paid_minutes=?15, minimum_assignment_count=?16, minimum_period=?17, maximum_paid_minutes=?18, maximum_period=?19, weekend_rotation_pattern=?20, weekend_rotation_anchor_date=?21, notes=?22, created_at=?23, updated_at=?24, archived_at=?25, revision=?26 WHERE id=?1 AND revision=?26-1"
    };
    let d = &employee.data;
    let result = sqlx::query(sql)
        .bind(&employee.id)
        .bind(&d.first_name)
        .bind(&d.last_name)
        .bind(&d.display_name)
        .bind(stable_text_order_key(&d.display_name))
        .bind(d.employment_type)
        .bind(d.active_status)
        .bind(d.holiday_group)
        .bind(d.can_work_weekends)
        .bind(d.can_work_holidays)
        .bind(d.can_work_night_shift)
        .bind(d.can_work_ortho_or)
        .bind(d.preferred_time_of_day)
        .bind(d.requires_explicit_availability)
        .bind(d.minimum_paid_minutes)
        .bind(d.minimum_assignment_count)
        .bind(d.minimum_period)
        .bind(d.maximum_paid_minutes)
        .bind(d.maximum_period)
        .bind(d.weekend_rotation_pattern)
        .bind(&d.weekend_rotation_anchor_date)
        .bind(&d.notes)
        .bind(&employee.created_at)
        .bind(&employee.updated_at)
        .bind(&employee.archived_at)
        .bind(employee.revision)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the employee before saving.",
        ));
    }
    Ok(())
}
pub async fn create_employee(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateEmployeeRequest,
) -> AppResult<Employee> {
    let defaults = employment_defaults(request.employment_type);
    let o = request.options;
    let min = o.minimum.unwrap_or(MinimumRule {
        minimum_paid_minutes: defaults.minimum_paid_minutes,
        minimum_assignment_count: defaults.minimum_assignment_count,
        minimum_period: defaults.minimum_period,
    });
    let max = o.maximum.unwrap_or(MaximumRule {
        maximum_paid_minutes: None,
        maximum_period: RulePeriod::None,
    });
    let weekend = o.weekend.unwrap_or(WeekendRule {
        weekend_rotation_pattern: WeekendPattern::None,
        weekend_rotation_anchor_date: None,
    });
    let data = EmployeeData {
        first_name: request.first_name,
        last_name: request.last_name,
        display_name: request.display_name,
        employment_type: request.employment_type,
        active_status: o.active_status.unwrap_or(ActiveStatus::Active),
        holiday_group: o.holiday_group.unwrap_or(HolidayGroup::None),
        can_work_weekends: o.can_work_weekends.unwrap_or(true),
        can_work_holidays: o.can_work_holidays.unwrap_or(true),
        can_work_night_shift: o.can_work_night_shift.unwrap_or(false),
        can_work_ortho_or: o.can_work_ortho_or.unwrap_or(false),
        preferred_time_of_day: o.preferred_time_of_day,
        requires_explicit_availability: o
            .requires_explicit_availability
            .unwrap_or(defaults.requires_explicit_availability),
        minimum_paid_minutes: min.minimum_paid_minutes,
        minimum_assignment_count: min.minimum_assignment_count,
        minimum_period: min.minimum_period,
        maximum_paid_minutes: max.maximum_paid_minutes,
        maximum_period: max.maximum_period,
        weekend_rotation_pattern: weekend.weekend_rotation_pattern,
        weekend_rotation_anchor_date: weekend.weekend_rotation_anchor_date,
        notes: o.notes,
    }
    .normalized()?;
    let timestamp = clock_timestamp(clock)?;
    let employee = Employee {
        id: uuid::Uuid::new_v4().to_string(),
        data,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    write_employee(&mut tx, &employee, true).await?;
    tx.commit().await?;
    Ok(employee)
}

async fn commit_employee_change(
    tx: Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
    mut employee: Employee,
    data: EmployeeData,
) -> AppResult<(Employee, bool)> {
    let mut tx = tx;
    if employee.data == data {
        tx.rollback().await?;
        return Ok((employee, false));
    }
    employee.revision = employee
        .revision
        .checked_add(1)
        .filter(|v| *v <= MAX_REVISION)
        .ok_or_else(|| invalid_employee("The revision limit has been reached."))?;
    employee.data = data;
    employee.updated_at = clock_timestamp(clock)?;
    write_employee(&mut tx, &employee, false).await?;
    tx.commit().await?;
    Ok((employee, true))
}
pub async fn update_employee(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateEmployeeRequest,
) -> CommandResult<Employee> {
    mutation_result(
        async {
            let data = request.employee.normalized()?;
            validate_revision(request.expected_revision)?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let employee = read_employee(&mut tx, &request.id).await?;
            check_revision(&employee.id, employee.revision, request.expected_revision)?;
            require_not_archived(&employee.archived_at)?;
            commit_employee_change(tx, clock, employee, data).await
        }
        .await,
    )
}

#[derive(Serialize)]
pub struct ResetDefaultsPreview {
    pub id: String,
    pub revision: i64,
    pub employment_type: EmploymentType,
    pub before: EmploymentDefaults,
    pub after: EmploymentDefaults,
}
pub async fn preview_employment_defaults(
    pool: &SqlitePool,
    request: RevisionRequest,
) -> AppResult<ResetDefaultsPreview> {
    let mut tx = pool.begin().await?;
    let employee = read_employee(&mut tx, &request.id).await?;
    check_revision(&employee.id, employee.revision, request.expected_revision)?;
    require_not_archived(&employee.archived_at)?;
    tx.rollback().await?;
    Ok(ResetDefaultsPreview {
        id: employee.id,
        revision: employee.revision,
        employment_type: employee.data.employment_type,
        before: EmploymentDefaults {
            minimum_paid_minutes: employee.data.minimum_paid_minutes,
            minimum_assignment_count: employee.data.minimum_assignment_count,
            minimum_period: employee.data.minimum_period,
            requires_explicit_availability: employee.data.requires_explicit_availability,
        },
        after: employment_defaults(employee.data.employment_type),
    })
}
pub async fn reset_employment_defaults(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<Employee> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let employee = read_employee(&mut tx, &request.id).await?;
            check_revision(&employee.id, employee.revision, request.expected_revision)?;
            require_not_archived(&employee.archived_at)?;
            let mut data = employee.data.clone();
            data.reset_minimum();
            // A manager's retained maximum can conflict with the proposed default minimum.
            let data = data.normalized()?;
            commit_employee_change(tx, clock, employee, data).await
        }
        .await,
    )
}

/// Employee restrictions describe this employee and can remain historical on archive.
/// Future active configuration references must be checked here, inside the archive transaction.
pub async fn employee_archive_dependencies(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> AppResult<Vec<String>> {
    read_employee(tx, id).await?;
    // Prompt 03 has no active configuration tables referencing employees.
    Ok(Vec::new())
}
pub async fn archive_employee(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<Employee> {
    mutation_result(async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut employee = read_employee(&mut tx, &request.id).await?;
        check_revision(&employee.id, employee.revision, request.expected_revision)?;
        if employee.archived_at.is_some() { tx.rollback().await?; return Ok((employee, false)); }
        let dependencies = employee_archive_dependencies(&mut tx, &employee.id).await?;
        if !dependencies.is_empty() {
            let mut error = AppError::new(ErrorCode::ActiveDependencies, "Update the active configuration that depends on this employee before archiving.");
            error.context.id = Some(employee.id);
            error.context.dependency_ids = Some(dependencies);
            return Err(error);
        }
        employee.revision = employee.revision.checked_add(1).filter(|v| *v <= MAX_REVISION).ok_or_else(|| invalid_employee("The revision limit has been reached."))?;
        employee.updated_at = clock_timestamp(clock)?;
        employee.archived_at = Some(employee.updated_at.clone());
        employee.data.active_status = ActiveStatus::Inactive;
        write_employee(&mut tx, &employee, false).await?;
        tx.commit().await?;
        Ok((employee, true))
    }.await)
}

// Shared source-record helpers used by positions and coverage configuration.
pub fn normalize_optional_label(value: Option<String>) -> AppResult<Option<String>> {
    value
        .filter(|v| !v.trim().is_empty())
        .map(|v| normalize_required_text(&v))
        .transpose()
}
pub fn validate_sort_order(value: i64) -> AppResult<()> {
    if !(-MAX_REVISION..=MAX_REVISION).contains(&value) {
        return Err(AppError::new(
            ErrorCode::OutOfRange,
            "Use a whole sort order within the supported integer range.",
        ));
    }
    Ok(())
}
pub fn record_filter_values(filter: RecordFilter) -> (Option<bool>, Option<ActiveStatus>) {
    match filter {
        RecordFilter::Current => (Some(false), None),
        RecordFilter::Active => (Some(false), Some(ActiveStatus::Active)),
        RecordFilter::Inactive => (Some(false), Some(ActiveStatus::Inactive)),
        RecordFilter::Archived => (Some(true), None),
        RecordFilter::All => (None, None),
    }
}
pub fn advance_source_metadata(
    revision: &mut i64,
    updated_at: &mut String,
    clock: &dyn MetadataClock,
) -> AppResult<()> {
    *revision = revision
        .checked_add(1)
        .filter(|v| *v <= MAX_REVISION)
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::InvalidRevision,
                "The revision limit has been reached.",
            )
        })?;
    *updated_at = clock_timestamp(clock)?;
    Ok(())
}
