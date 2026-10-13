use crate::{
    employees::{
        ActiveStatus, MAX_REVISION, RecordFilter, RevisionRequest, TimeOfDay, check_revision,
        mutation_result, normalize_optional_notes, read_employee, require_not_archived,
        verify_metadata,
    },
    error::{AppError, AppResult, CommandResult, ErrorCode},
    local_values::{
        ANALYSIS_SOURCE_LIMIT, ExpandedIntervalBudget, LocalDate, LocalDateTime, LocalInterval,
        LocalTime, Weekday, input_limit_error,
    },
    metadata::{MetadataClock, clock_timestamp},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum RestrictionKind {
    TimeOfDayBlock,
    RecurringDayTimeBlock,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct RestrictionData {
    pub restriction_kind: RestrictionKind,
    pub time_of_day: Option<TimeOfDay>,
    pub weekday: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EmployeeRestriction {
    pub id: String,
    pub employee_id: String,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: RestrictionData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRestrictionRequest {
    pub employee_id: String,
    pub restriction: RestrictionData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRestrictionRequest {
    pub id: String,
    pub expected_revision: i64,
    pub restriction: RestrictionData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRestrictionsRequest {
    pub employee_id: String,
    #[serde(default)]
    pub filter: RecordFilter,
}
fn invalid() -> AppError {
    AppError::new(
        ErrorCode::InvalidRestriction,
        "Choose a time category alone, or a weekday with distinct start/end times in HHMM format.",
    )
}

impl RestrictionData {
    pub fn normalized(mut self) -> AppResult<Self> {
        self.notes = normalize_optional_notes(self.notes)?;
        match self.restriction_kind {
            RestrictionKind::TimeOfDayBlock
                if self.time_of_day.is_some()
                    && self.weekday.is_none()
                    && self.start_time.is_none()
                    && self.end_time.is_none() => {}
            RestrictionKind::RecurringDayTimeBlock if self.time_of_day.is_none() => {
                Weekday::parse(self.weekday.as_deref().ok_or_else(invalid)?)?;
                let start = LocalTime::parse(self.start_time.as_deref().ok_or_else(invalid)?)?;
                let end = LocalTime::parse(self.end_time.as_deref().ok_or_else(invalid)?)?;
                if start == end {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
        Ok(self)
    }
}
fn verify_restriction(r: &EmployeeRestriction) -> AppResult<()> {
    if r.data.clone().normalized()? != r.data
        || (r.archived_at.is_some() && r.data.active_status != ActiveStatus::Inactive)
    {
        return Err(AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A stored restriction requires investigation; it has not been changed.",
        ));
    }
    verify_metadata(&r.created_at, &r.updated_at, &r.archived_at, r.revision)
}
async fn read_restriction(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> AppResult<EmployeeRestriction> {
    let restriction = sqlx::query_as::<_, EmployeeRestriction>(
        "SELECT * FROM employee_restrictions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::new(ErrorCode::NotFound, "The restriction could not be found."))?;
    verify_restriction(&restriction)?;
    Ok(restriction)
}
pub async fn list_restrictions(
    pool: &SqlitePool,
    request: ListRestrictionsRequest,
) -> AppResult<Vec<EmployeeRestriction>> {
    let mut tx = pool.begin().await?;
    read_employee(&mut tx, &request.employee_id).await?;
    let (archive, active) = match request.filter {
        RecordFilter::Current => (Some(false), None),
        RecordFilter::Active => (Some(false), Some(ActiveStatus::Active)),
        RecordFilter::Inactive => (Some(false), Some(ActiveStatus::Inactive)),
        RecordFilter::Archived => (Some(true), None),
        RecordFilter::All => (None, None),
    };
    let rows = sqlx::query_as::<_, EmployeeRestriction>("SELECT * FROM employee_restrictions WHERE employee_id=?1 AND (?2 IS NULL OR (archived_at IS NOT NULL)=?2) AND (?3 IS NULL OR active_status=?3) ORDER BY restriction_kind, time_of_day, weekday, start_time, end_time, id")
        .bind(request.employee_id).bind(archive).bind(active).fetch_all(&mut *tx).await?;
    tx.rollback().await?;
    for row in &rows {
        verify_restriction(row)?;
    }
    Ok(rows)
}
async fn check_duplicate(
    tx: &mut Transaction<'_, Sqlite>,
    r: &EmployeeRestriction,
) -> AppResult<()> {
    if r.data.active_status == ActiveStatus::Inactive || r.archived_at.is_some() {
        return Ok(());
    }
    let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM employee_restrictions WHERE employee_id=?1 AND id<>?2 AND active_status='active' AND archived_at IS NULL AND restriction_kind=?3 AND time_of_day IS ?4 AND weekday IS ?5 AND start_time IS ?6 AND end_time IS ?7)")
        .bind(&r.employee_id).bind(&r.id).bind(r.data.restriction_kind).bind(r.data.time_of_day)
        .bind(&r.data.weekday).bind(&r.data.start_time).bind(&r.data.end_time).fetch_one(&mut **tx).await?;
    if found {
        return Err(AppError::new(
            ErrorCode::DuplicateRestriction,
            "An equivalent active restriction already exists. Edit it or make it inactive first.",
        ));
    }
    Ok(())
}
async fn write_restriction(
    tx: &mut Transaction<'_, Sqlite>,
    r: &EmployeeRestriction,
    create: bool,
) -> AppResult<()> {
    check_duplicate(tx, r).await?;
    let sql = if create {
        "INSERT INTO employee_restrictions (id, employee_id, restriction_kind, time_of_day, weekday, start_time, end_time, active_status, notes, created_at, updated_at, archived_at, revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"
    } else {
        "UPDATE employee_restrictions SET employee_id=?2, restriction_kind=?3, time_of_day=?4, weekday=?5, start_time=?6, end_time=?7, active_status=?8, notes=?9, created_at=?10, updated_at=?11, archived_at=?12, revision=?13 WHERE id=?1 AND revision=?13-1"
    };
    let d = &r.data;
    let result = sqlx::query(sql)
        .bind(&r.id)
        .bind(&r.employee_id)
        .bind(d.restriction_kind)
        .bind(d.time_of_day)
        .bind(&d.weekday)
        .bind(&d.start_time)
        .bind(&d.end_time)
        .bind(d.active_status)
        .bind(&d.notes)
        .bind(&r.created_at)
        .bind(&r.updated_at)
        .bind(&r.archived_at)
        .bind(r.revision)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the restriction before saving.",
        ));
    }
    Ok(())
}
pub async fn create_restriction(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateRestrictionRequest,
) -> AppResult<EmployeeRestriction> {
    let data = request.restriction.normalized()?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let employee = read_employee(&mut tx, &request.employee_id).await?;
    require_not_archived(&employee.archived_at)?;
    let timestamp = clock_timestamp(clock)?;
    let restriction = EmployeeRestriction {
        id: uuid::Uuid::new_v4().to_string(),
        employee_id: employee.id,
        data,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    write_restriction(&mut tx, &restriction, true).await?;
    tx.commit().await?;
    Ok(restriction)
}
pub async fn update_restriction(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateRestrictionRequest,
) -> CommandResult<EmployeeRestriction> {
    mutation_result(
        async {
            let data = request.restriction.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut r = read_restriction(&mut tx, &request.id).await?;
            check_revision(&r.id, r.revision, request.expected_revision)?;
            require_not_archived(&r.archived_at)?;
            let employee = read_employee(&mut tx, &r.employee_id).await?;
            require_not_archived(&employee.archived_at)?;
            if r.data == data {
                tx.rollback().await?;
                return Ok((r, false));
            }
            r.data = data;
            r.revision = r
                .revision
                .checked_add(1)
                .filter(|v| *v <= MAX_REVISION)
                .ok_or_else(invalid)?;
            r.updated_at = clock_timestamp(clock)?;
            write_restriction(&mut tx, &r, false).await?;
            tx.commit().await?;
            Ok((r, true))
        }
        .await,
    )
}
pub async fn archive_restriction(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<EmployeeRestriction> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut r = read_restriction(&mut tx, &request.id).await?;
            check_revision(&r.id, r.revision, request.expected_revision)?;
            let employee = read_employee(&mut tx, &r.employee_id).await?;
            require_not_archived(&employee.archived_at)?;
            if r.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((r, false));
            }
            r.revision = r
                .revision
                .checked_add(1)
                .filter(|v| *v <= MAX_REVISION)
                .ok_or_else(invalid)?;
            r.updated_at = clock_timestamp(clock)?;
            r.archived_at = Some(r.updated_at.clone());
            r.data.active_status = ActiveStatus::Inactive;
            write_restriction(&mut tx, &r, false).await?;
            tx.commit().await?;
            Ok((r, true))
        }
        .await,
    )
}

#[derive(Debug)]
struct Occurrence {
    id: String,
    interval: LocalInterval,
}
#[derive(Default)]
struct EmployeeIntervals {
    categories: Vec<(String, TimeOfDay)>,
    occurrences: Vec<Occurrence>,
}
/// Built once for merged actual-date windows, then reused across candidates in a snapshot.
pub struct RestrictionIndex {
    employees: BTreeMap<String, EmployeeIntervals>,
    windows: Vec<(LocalDate, LocalDate)>,
}
impl RestrictionIndex {
    /// Complete hard material for a retained generation diagnostic. Occurrences
    /// are already bounded and canonically sorted; nothing is expanded here.
    pub fn append_hard_material(
        &self,
        employee_id: &str,
        category: TimeOfDay,
        candidate: LocalInterval,
        material: &mut crate::rule_facts::MaterialFacts,
    ) -> AppResult<()> {
        let first = candidate.start().date().subtract_days(1)?;
        let first_start = LocalDateTime::from_date_time(first, LocalTime::parse("0000")?, 0)?;
        if let Some(employee) = self.employees.get(employee_id) {
            for (id, blocked) in &employee.categories {
                if *blocked == category {
                    material.text(id);
                    material.text("category_block");
                }
            }
            let begin = employee
                .occurrences
                .partition_point(|o| o.interval.start() < first_start);
            let end = employee
                .occurrences
                .partition_point(|o| o.interval.start() < candidate.end());
            for occurrence in &employee.occurrences[begin..end] {
                if occurrence.interval.overlaps(candidate) {
                    material.text(&occurrence.id);
                    material.interval(occurrence.interval);
                }
            }
        }
        Ok(())
    }
    /// Caller passes only restrictions belonging to active, non-archived employees.
    /// The same budget must also be passed to later availability/PTO expansion.
    pub fn build(
        rows: &[EmployeeRestriction],
        windows: &[LocalInterval],
        budget: &mut ExpandedIntervalBudget,
    ) -> AppResult<Self> {
        if rows.len() > ANALYSIS_SOURCE_LIMIT {
            return Err(input_limit_error(
                "source_rows",
                ANALYSIS_SOURCE_LIMIT,
                rows.len(),
            ));
        }
        if windows.len() > ANALYSIS_SOURCE_LIMIT {
            return Err(input_limit_error(
                "evaluation_windows",
                ANALYSIS_SOURCE_LIMIT,
                windows.len(),
            ));
        }
        let mut ranges = windows
            .iter()
            .map(|w| Ok((w.start().date().subtract_days(1)?, w.end().date())))
            .collect::<AppResult<Vec<_>>>()?;
        ranges.sort_unstable();
        let mut merged: Vec<(LocalDate, LocalDate)> = Vec::new();
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut()
                && start.days_since(last.1) <= 1
            {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        let mut employees: BTreeMap<String, EmployeeIntervals> = BTreeMap::new();
        let mut source_ids = BTreeSet::new();
        // Preflight/count before returning any authoritative index. A failure returns no partial index.
        for row in rows {
            verify_restriction(row)?;
            if !source_ids.insert(row.id.as_str()) {
                return Err(AppError::new(
                    ErrorCode::ConfigurationInvalid,
                    "Restriction source IDs must be unique in an analysis snapshot.",
                ));
            }
            if row.archived_at.is_some() || row.data.active_status == ActiveStatus::Inactive {
                continue;
            }
            let target = employees.entry(row.employee_id.clone()).or_default();
            if let Some(category) = row.data.time_of_day {
                target.categories.push((row.id.clone(), category));
                continue;
            }
            let weekday = Weekday::parse(row.data.weekday.as_deref().ok_or_else(invalid)?)?;
            let start_time = LocalTime::parse(row.data.start_time.as_deref().ok_or_else(invalid)?)?;
            let end_time = LocalTime::parse(row.data.end_time.as_deref().ok_or_else(invalid)?)?;
            for &(first, last) in &merged {
                let distance = (weekday as i64 - first.weekday() as i64).rem_euclid(7);
                if distance > last.days_since(first) {
                    continue;
                }
                let mut date = first.add_days(distance)?;
                loop {
                    budget.reserve_occurrence()?;
                    let interval = LocalInterval::new(
                        LocalDateTime::from_date_time(date, start_time, 0)?,
                        LocalDateTime::from_date_time(
                            date,
                            end_time,
                            i64::from(end_time < start_time),
                        )?,
                    )?;
                    target.occurrences.push(Occurrence {
                        id: row.id.clone(),
                        interval,
                    });
                    if last.days_since(date) < 7 {
                        break;
                    }
                    date = date.add_days(7)?;
                }
            }
        }
        for target in employees.values_mut() {
            target.categories.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            target.occurrences.sort_unstable_by(|a, b| {
                a.interval
                    .start()
                    .cmp(&b.interval.start())
                    .then_with(|| a.id.cmp(&b.id))
            });
        }
        Ok(Self {
            employees,
            windows: merged,
        })
    }
    /// Complete, bounded source IDs. Callers construct their diagnostic provenance from this result.
    pub fn evaluate<'a>(
        &'a self,
        employee_id: &str,
        category: TimeOfDay,
        candidate: LocalInterval,
    ) -> AppResult<Vec<&'a str>> {
        let first = candidate.start().date().subtract_days(1)?;
        let last = candidate.end().date();
        if !self.windows.iter().any(|&(a, b)| a <= first && last <= b) {
            return Err(AppError::new(
                ErrorCode::ConfigurationInvalid,
                "The candidate is outside the restriction snapshot's expanded windows.",
            ));
        }
        let Some(employee) = self.employees.get(employee_id) else {
            return Ok(Vec::new());
        };
        let mut matches = BTreeSet::new();
        for (id, blocked) in &employee.categories {
            if *blocked == category {
                matches.insert(id.as_str());
            }
        }
        let first_start = LocalDateTime::from_date_time(first, LocalTime::parse("0000")?, 0)?;
        let begin = employee
            .occurrences
            .partition_point(|o| o.interval.start() < first_start);
        let end = employee
            .occurrences
            .partition_point(|o| o.interval.start() < candidate.end());
        for occurrence in &employee.occurrences[begin..end] {
            if occurrence.interval.overlaps(candidate) {
                matches.insert(occurrence.id.as_str());
            }
        }
        Ok(matches.into_iter().collect())
    }
}
