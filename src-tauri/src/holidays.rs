use crate::{
    coverage::{DayType, read_coverage_template},
    employees::{
        ActiveStatus, HolidayGroup, IdRequest, RecordFilter, RevisionRequest,
        advance_source_metadata, check_revision, mutation_result, normalize_optional_notes,
        record_filter_values, require_not_archived,
    },
    error::{
        AppError, AppResult, CommandResult, DIAGNOSTIC_LIMIT, DIAGNOSTIC_SOURCE_ID_BYTES,
        DIAGNOSTIC_SOURCE_ID_LIMIT, ErrorCode,
    },
    local_values::{ANALYSIS_SOURCE_LIMIT, LocalDate, Weekday, input_limit_error},
    metadata::{MetadataClock, clock_timestamp},
    positions::verify_source_shape,
    settings::{SettingValue, read_in_transaction},
    text::{compare_text_with_ids, normalize_required_text, stable_text_order_key},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum HolidaySet {
    A,
    B,
    None,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum DateKind {
    Fixed,
    NthWeekday,
    LastWeekday,
    ExactDate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum CoverageBehavior {
    Normal,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct HolidayData {
    pub name: String,
    pub holiday_set: HolidaySet,
    pub date_kind: DateKind,
    pub month: Option<i64>,
    pub day_of_month: Option<i64>,
    pub weekday: Option<String>,
    pub nth: Option<i64>,
    pub exact_date: Option<String>,
    pub coverage_behavior: CoverageBehavior,
    pub custom_coverage_template_id: Option<String>,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
}
fn rule_error() -> AppError {
    AppError::new(
        ErrorCode::InvalidHolidayRule,
        "Use a supported complete date rule. Annual fixed dates must occur every year; use an exact date for February 29.",
    )
}
fn configuration_error() -> AppError {
    AppError::new(
        ErrorCode::ConfigurationInvalid,
        "Stored holiday configuration requires investigation; no data was changed.",
    )
}
impl HolidayData {
    fn normalized(mut self) -> AppResult<Self> {
        self.name = normalize_required_text(&self.name)?;
        self.notes = normalize_optional_notes(self.notes)?;
        self.validate_rule()?;
        match (self.coverage_behavior, &self.custom_coverage_template_id) {
            (CoverageBehavior::Normal, None) => {}
            (CoverageBehavior::Custom, Some(id)) if normalize_required_text(id)? == *id => {}
            _ => {
                return Err(AppError::new(
                    ErrorCode::InvalidHolidayCoverage,
                    "Normal coverage has no custom reference. Custom coverage requires a retained coverage template.",
                ));
            }
        }
        Ok(self)
    }
    pub fn validate_rule(&self) -> AppResult<()> {
        let no_recurring = self.month.is_none()
            && self.day_of_month.is_none()
            && self.weekday.is_none()
            && self.nth.is_none();
        let valid = match self.date_kind {
            DateKind::Fixed => {
                self.weekday.is_none()
                    && self.nth.is_none()
                    && self.exact_date.is_none()
                    && self
                        .month
                        .zip(self.day_of_month)
                        .is_some_and(|(m, d)| LocalDate::from_parts(2025, m, d).is_ok())
            }
            DateKind::NthWeekday | DateKind::LastWeekday => {
                self.month.is_some_and(|m| (1..=12).contains(&m))
                    && self
                        .weekday
                        .as_deref()
                        .is_some_and(|w| Weekday::parse(w).is_ok())
                    && self.day_of_month.is_none()
                    && self.exact_date.is_none()
                    && if self.date_kind == DateKind::NthWeekday {
                        self.nth.is_some_and(|n| (1..=4).contains(&n))
                    } else {
                        self.nth.is_none()
                    }
            }
            DateKind::ExactDate => {
                no_recurring
                    && self
                        .exact_date
                        .as_deref()
                        .is_some_and(|d| LocalDate::parse(d).is_ok())
            }
        };
        if valid { Ok(()) } else { Err(rule_error()) }
    }
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Holiday {
    pub id: String,
    pub seed_key: Option<String>,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: HolidayData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
pub(crate) fn verify_holiday(row: &Holiday) -> AppResult<()> {
    if row.id.len() > DIAGNOSTIC_SOURCE_ID_BYTES
        || normalize_required_text(&row.id).map_err(|_| configuration_error())? != row.id
        || row
            .data
            .clone()
            .normalized()
            .map_err(|_| configuration_error())?
            != row.data
    {
        return Err(configuration_error());
    }
    verify_source_shape(
        &row.seed_key,
        row.data.active_status,
        &row.created_at,
        &row.updated_at,
        &row.archived_at,
        row.revision,
    )
    .map_err(|_| configuration_error())
}
pub async fn require_custom_coverage(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    current: bool,
) -> AppResult<()> {
    let source = read_coverage_template(tx, id).await?;
    if !current {
        return Ok(());
    }
    let populated: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM coverage_template_items WHERE coverage_template_id=? AND active_status='active' AND archived_at IS NULL)")
        .bind(id).fetch_one(&mut **tx).await?;
    if source.data.day_type != DayType::HolidayCustom
        || source.data.active_status != ActiveStatus::Active
        || source.archived_at.is_some()
        || !populated
    {
        let mut error = AppError::new(
            ErrorCode::InvalidHolidayCoverage,
            "An active holiday requires active, non-archived, populated holiday-custom coverage. Reassign coverage or leave the holiday inactive.",
        );
        error.context.id = Some(id.to_owned());
        return Err(error);
    }
    Ok(())
}
async fn validate_reference(tx: &mut Transaction<'_, Sqlite>, row: &Holiday) -> AppResult<()> {
    verify_holiday(row)?;
    if let Some(id) = &row.data.custom_coverage_template_id {
        require_custom_coverage(
            tx,
            id,
            row.data.active_status == ActiveStatus::Active && row.archived_at.is_none(),
        )
        .await?;
    }
    Ok(())
}
pub async fn read_holiday(tx: &mut Transaction<'_, Sqlite>, id: &str) -> AppResult<Holiday> {
    normalize_required_text(id)?;
    let row = sqlx::query_as::<_, Holiday>("SELECT * FROM holidays WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "The holiday could not be found."))?;
    verify_holiday(&row)?;
    Ok(row)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateHolidayRequest {
    pub holiday: HolidayData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateHolidayRequest {
    pub id: String,
    pub expected_revision: i64,
    pub holiday: HolidayData,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListHolidaysRequest {
    #[serde(default)]
    pub filter: RecordFilter,
    pub year: Option<i64>,
    pub holiday_set: Option<HolidaySet>,
    pub coverage_behavior: Option<CoverageBehavior>,
}
#[derive(Serialize)]
pub struct HolidayListEntry {
    #[serde(flatten)]
    pub holiday: Holiday,
    pub resolved_date: Option<String>,
}
fn validate_year(year: i64) -> AppResult<()> {
    LocalDate::from_parts(year, 1, 1).map(|_| ())
}
/// Resolve once in the selected civil year; one-time dates never recur.
pub fn resolve_date(data: &HolidayData, year: i64) -> AppResult<Option<LocalDate>> {
    validate_year(year)?;
    data.validate_rule()?;
    if data.date_kind == DateKind::ExactDate {
        let date = LocalDate::parse(data.exact_date.as_deref().ok_or_else(rule_error)?)?;
        return Ok((date.year() == year).then_some(date));
    }
    let month = data.month.ok_or_else(rule_error)?;
    if data.date_kind == DateKind::Fixed {
        return Ok(Some(LocalDate::from_parts(
            year,
            month,
            data.day_of_month.ok_or_else(rule_error)?,
        )?));
    }
    let weekday = Weekday::parse(data.weekday.as_deref().ok_or_else(rule_error)?)?;
    // Both searches stay within the requested month, including December 9999.
    let base = if data.date_kind == DateKind::LastWeekday {
        let mut day = 31;
        while LocalDate::from_parts(year, month, day).is_err() {
            day -= 1;
        }
        LocalDate::from_parts(year, month, day)?
    } else {
        LocalDate::from_parts(year, month, 1)?
    };
    let delta = if data.date_kind == DateKind::LastWeekday {
        (base.weekday() as i64 - weekday as i64)
            .rem_euclid(7)
            .checked_neg()
    } else {
        let weeks = data
            .nth
            .ok_or_else(rule_error)?
            .checked_sub(1)
            .and_then(|n| n.checked_mul(7));
        weeks.and_then(|w| {
            (weekday as i64 - base.weekday() as i64)
                .rem_euclid(7)
                .checked_add(w)
        })
    }
    .ok_or_else(rule_error)?;
    let date = base.add_days(delta)?;
    if date.month() != month {
        return Err(rule_error());
    }
    Ok(Some(date))
}
pub async fn get_holiday(pool: &SqlitePool, request: IdRequest) -> AppResult<Holiday> {
    let mut tx = pool.begin().await?;
    let row = read_holiday(&mut tx, &request.id).await?;
    validate_reference(&mut tx, &row).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_holidays(
    pool: &SqlitePool,
    request: ListHolidaysRequest,
) -> AppResult<Vec<HolidayListEntry>> {
    if let Some(year) = request.year {
        validate_year(year)?;
    }
    let (archive, active) = record_filter_values(request.filter);
    let mut tx = pool.begin().await?;
    let rows = sqlx::query_as::<_,Holiday>("SELECT * FROM holidays WHERE (?1 IS NULL OR (archived_at IS NOT NULL)=?1) AND (?2 IS NULL OR active_status=?2) AND (?3 IS NULL OR holiday_set=?3) AND (?4 IS NULL OR coverage_behavior=?4) ORDER BY name_order_key,id LIMIT ?5")
        .bind(archive).bind(active).bind(request.holiday_set).bind(request.coverage_behavior)
        .bind((ANALYSIS_SOURCE_LIMIT + 1) as i64).fetch_all(&mut *tx).await?;
    if rows.len() > ANALYSIS_SOURCE_LIMIT {
        return Err(input_limit_error(
            "source_rows",
            ANALYSIS_SOURCE_LIMIT,
            rows.len(),
        ));
    }
    for row in &rows {
        validate_reference(&mut tx, row).await?;
    }
    tx.rollback().await?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut entries = rows
            .into_iter()
            .map(|holiday| {
                let resolved_date = request
                    .year
                    .map(|y| resolve_date(&holiday.data, y))
                    .transpose()?
                    .flatten()
                    .map(LocalDate::format);
                Ok(HolidayListEntry {
                    holiday,
                    resolved_date,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        entries.sort_by(|a, b| {
            a.resolved_date
                .is_none()
                .cmp(&b.resolved_date.is_none())
                .then_with(|| a.resolved_date.cmp(&b.resolved_date))
                .then_with(|| {
                    compare_text_with_ids(
                        &a.holiday.data.name,
                        &a.holiday.id,
                        &b.holiday.data.name,
                        &b.holiday.id,
                    )
                })
        });
        Ok(entries)
    })
    .await
    .map_err(|_| configuration_error())?
}
async fn write_holiday(
    tx: &mut Transaction<'_, Sqlite>,
    row: &Holiday,
    create: bool,
) -> AppResult<()> {
    validate_reference(tx, row).await?;
    let sql = if create {
        "INSERT INTO holidays (id,seed_key,name,name_order_key,holiday_set,date_kind,month,day_of_month,weekday,nth,exact_date,coverage_behavior,custom_coverage_template_id,active_status,notes,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"
    } else {
        "UPDATE holidays SET name=?3,name_order_key=?4,holiday_set=?5,date_kind=?6,month=?7,day_of_month=?8,weekday=?9,nth=?10,exact_date=?11,coverage_behavior=?12,custom_coverage_template_id=?13,active_status=?14,notes=?15,updated_at=?17,archived_at=?18,revision=?19 WHERE id=?1 AND revision=?19-1"
    };
    let d = &row.data;
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&row.seed_key)
        .bind(&d.name)
        .bind(stable_text_order_key(&d.name))
        .bind(d.holiday_set)
        .bind(d.date_kind)
        .bind(d.month)
        .bind(d.day_of_month)
        .bind(&d.weekday)
        .bind(d.nth)
        .bind(&d.exact_date)
        .bind(d.coverage_behavior)
        .bind(&d.custom_coverage_template_id)
        .bind(d.active_status)
        .bind(&d.notes)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .bind(&row.archived_at)
        .bind(row.revision)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the holiday before saving.",
        ));
    }
    Ok(())
}
pub async fn create_holiday(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateHolidayRequest,
) -> AppResult<Holiday> {
    let data = request.holiday.normalized()?;
    let timestamp = clock_timestamp(clock)?;
    let row = Holiday {
        id: uuid::Uuid::new_v4().to_string(),
        seed_key: None,
        data,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    write_holiday(&mut tx, &row, true).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn update_holiday(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateHolidayRequest,
) -> CommandResult<Holiday> {
    mutation_result(
        async {
            let data = request.holiday.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_holiday(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            let changed = row.data != data;
            row.data = data;
            // Even a normalized no-change activation request rechecks current sources.
            validate_reference(&mut tx, &row).await?;
            if !changed {
                tx.rollback().await?;
                return Ok((row, false));
            }
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            write_holiday(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_holiday(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<Holiday> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_holiday(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_holiday(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatePreviewRequest {
    pub holiday: HolidayData,
    pub year: i64,
}
#[derive(Serialize)]
pub struct DatePreview {
    pub year: i64,
    pub resolved_date: Option<String>,
}
pub fn preview_date(request: DatePreviewRequest) -> AppResult<DatePreview> {
    let data = request.holiday.normalized()?;
    Ok(DatePreview {
        year: request.year,
        resolved_date: resolve_date(&data, request.year)?.map(LocalDate::format),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationPreviewRequest {
    pub year: i64,
}
#[derive(Debug, Clone, Serialize)]
pub struct RotationPreview {
    pub year: i64,
    pub start_year: i64,
    pub set_a_group: HolidayGroup,
    pub set_b_group: HolidayGroup,
}
impl RotationPreview {
    /// The one set-to-group mapping for this rotation year; set None requires no group.
    pub fn required_group(&self, set: HolidaySet) -> Option<HolidayGroup> {
        match set {
            HolidaySet::A => Some(self.set_a_group),
            HolidaySet::B => Some(self.set_b_group),
            HolidaySet::None => None,
        }
    }
}
pub fn rotation_for_year(year: i64, start_year: i64) -> AppResult<RotationPreview> {
    validate_year(year)?;
    validate_year(start_year)?;
    let even = year
        .checked_sub(start_year)
        .ok_or_else(rule_error)?
        .rem_euclid(2)
        == 0;
    Ok(RotationPreview {
        year,
        start_year,
        set_a_group: if even {
            HolidayGroup::A
        } else {
            HolidayGroup::B
        },
        set_b_group: if even {
            HolidayGroup::B
        } else {
            HolidayGroup::A
        },
    })
}
pub async fn rotation_start_year(tx: &mut Transaction<'_, Sqlite>) -> AppResult<i64> {
    read_in_transaction(tx)
        .await?
        .into_iter()
        .find_map(|s| {
            if s.key == "holiday_rotation_start_year" {
                match s.value {
                    Some(SettingValue::Integer(y)) => Some(y),
                    _ => None,
                }
            } else {
                None
            }
        })
        .ok_or_else(configuration_error)
}
pub async fn preview_rotation(
    pool: &SqlitePool,
    request: RotationPreviewRequest,
) -> AppResult<RotationPreview> {
    let mut tx = pool.begin().await?;
    let start = rotation_start_year(&mut tx).await?;
    tx.rollback().await?;
    rotation_for_year(request.year, start)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupMatch {
    RequiredMatch,
    Mismatch,
    NotApplicable,
}
#[derive(Serialize)]
pub struct HolidayGroupMatch {
    pub result: GroupMatch,
    pub holiday_id: String,
    pub date: Option<String>,
    pub holiday_set: HolidaySet,
    pub employee_group: HolidayGroup,
    pub required_group: Option<HolidayGroup>,
    pub rotation: RotationPreview,
}
/// One canonical evaluator for later operational-date candidate services.
pub fn match_holiday_group(
    holiday: &Holiday,
    group: HolidayGroup,
    year: i64,
    start_year: i64,
) -> AppResult<HolidayGroupMatch> {
    verify_holiday(holiday)?;
    let date = resolve_date(&holiday.data, year)?;
    let rotation = rotation_for_year(year, start_year)?;
    let required_group = if date.is_none()
        || holiday.data.active_status != ActiveStatus::Active
        || holiday.archived_at.is_some()
    {
        None
    } else {
        rotation.required_group(holiday.data.holiday_set)
    };
    let result = match required_group {
        None => GroupMatch::NotApplicable,
        Some(required) if required == group => GroupMatch::RequiredMatch,
        _ => GroupMatch::Mismatch,
    };
    Ok(HolidayGroupMatch {
        result,
        holiday_id: holiday.id.clone(),
        date: date.map(LocalDate::format),
        holiday_set: holiday.data.holiday_set,
        employee_group: group,
        required_group,
        rotation,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupMatchRequest {
    pub id: String,
    pub employee_group: HolidayGroup,
    pub year: i64,
}
pub async fn preview_group_match(
    pool: &SqlitePool,
    request: GroupMatchRequest,
) -> AppResult<HolidayGroupMatch> {
    let mut tx = pool.begin().await?;
    let holiday = read_holiday(&mut tx, &request.id).await?;
    validate_reference(&mut tx, &holiday).await?;
    let start = rotation_start_year(&mut tx).await?;
    tx.rollback().await?;
    match_holiday_group(&holiday, request.employee_group, request.year, start)
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HolidayWindow {
    Date {
        date: String,
    },
    Year {
        year: i64,
    },
    Range {
        start_date: String,
        end_date: String,
    },
}
impl HolidayWindow {
    fn bounds(&self) -> AppResult<(LocalDate, LocalDate)> {
        let (start, end) = match self {
            Self::Date { date } => {
                let d = LocalDate::parse(date)?;
                (d, d)
            }
            Self::Year { year } => (
                LocalDate::from_parts(*year, 1, 1)?,
                LocalDate::from_parts(*year, 12, 31)?,
            ),
            Self::Range {
                start_date,
                end_date,
            } => (LocalDate::parse(start_date)?, LocalDate::parse(end_date)?),
        };
        if start > end {
            return Err(AppError::new(
                ErrorCode::InvalidDate,
                "The inclusive end date must follow or equal the start date.",
            ));
        }
        Ok((start, end))
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedHoliday {
    pub date: String,
    pub id: String,
    pub seed_key: Option<String>,
    pub name: String,
    pub holiday_set: HolidaySet,
    pub coverage_behavior: CoverageBehavior,
    pub custom_coverage_template_id: Option<String>,
}
pub async fn load_active_holidays(
    tx: &mut Transaction<'_, Sqlite>,
    start: LocalDate,
    end: LocalDate,
) -> AppResult<Vec<Holiday>> {
    if start > end {
        return Err(rule_error());
    }
    let rows = sqlx::query_as::<_,Holiday>("SELECT * FROM holidays WHERE active_status='active' AND archived_at IS NULL AND (date_kind<>'exact_date' OR exact_date BETWEEN ?1 AND ?2) ORDER BY name_order_key,id LIMIT ?3")
        .bind(start.format()).bind(end.format()).bind((ANALYSIS_SOURCE_LIMIT + 1) as i64).fetch_all(&mut **tx).await?;
    if rows.len() > ANALYSIS_SOURCE_LIMIT {
        return Err(input_limit_error(
            "source_rows",
            ANALYSIS_SOURCE_LIMIT,
            rows.len(),
        ));
    }
    for row in &rows {
        validate_reference(tx, row).await?;
    }
    Ok(rows)
}
pub fn resolve_active_holidays(
    rows: &[Holiday],
    start: LocalDate,
    end: LocalDate,
) -> AppResult<Vec<ResolvedHoliday>> {
    if start > end {
        return Err(rule_error());
    }
    let mut resolved = Vec::new();
    for row in rows {
        verify_holiday(row)?;
        if row.data.active_status != ActiveStatus::Active || row.archived_at.is_some() {
            continue;
        }
        let first_year = if row.data.date_kind == DateKind::ExactDate {
            LocalDate::parse(row.data.exact_date.as_deref().ok_or_else(rule_error)?)?.year()
        } else {
            start.year()
        };
        let last_year = if row.data.date_kind == DateKind::ExactDate {
            first_year
        } else {
            end.year()
        };
        for year in first_year..=last_year {
            if let Some(date) = resolve_date(&row.data, year)? {
                if date < start || date > end {
                    continue;
                }
                if resolved.len() == ANALYSIS_SOURCE_LIMIT {
                    return Err(input_limit_error(
                        "resolved_holidays",
                        ANALYSIS_SOURCE_LIMIT,
                        resolved.len() + 1,
                    ));
                }
                resolved.push(ResolvedHoliday {
                    date: date.format(),
                    id: row.id.clone(),
                    seed_key: row.seed_key.clone(),
                    name: row.data.name.clone(),
                    holiday_set: row.data.holiday_set,
                    coverage_behavior: row.data.coverage_behavior,
                    custom_coverage_template_id: row.data.custom_coverage_template_id.clone(),
                });
            }
        }
    }
    resolved.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then_with(|| compare_text_with_ids(&a.name, &a.id, &b.name, &b.id))
    });
    Ok(resolved)
}
#[derive(Serialize)]
pub struct CollisionSource {
    pub id: String,
    pub name: String,
}
#[derive(Serialize)]
pub struct HolidayCollision {
    pub date: String,
    pub holiday_count: usize,
    pub holidays: Vec<CollisionSource>,
    pub omitted_source_id_count: usize,
}
#[derive(Serialize)]
pub struct CollisionPreview {
    pub start_date: String,
    pub end_date: String,
    pub collision_count: usize,
    pub collisions: Vec<HolidayCollision>,
    pub omitted_diagnostic_count: usize,
}
/// The same complete collision decision serves UI and future preflight. Display
/// truncation never permits selecting the first holiday or authorizing generation.
pub fn collision_preview(
    rows: &[ResolvedHoliday],
    start: LocalDate,
    end: LocalDate,
) -> AppResult<CollisionPreview> {
    let mut collisions = Vec::new();
    let mut collision_count = 0usize;
    let mut index = 0;
    while index < rows.len() {
        let mut next = index + 1;
        while next < rows.len() && rows[next].date == rows[index].date {
            next += 1;
        }
        if next - index > 1 {
            collision_count = collision_count
                .checked_add(1)
                .ok_or_else(configuration_error)?;
            if collisions.len() < DIAGNOSTIC_LIMIT {
                let mut sources: Vec<&ResolvedHoliday> =
                    Vec::with_capacity(DIAGNOSTIC_SOURCE_ID_LIMIT);
                for source in &rows[index..next] {
                    if source.id.len() > DIAGNOSTIC_SOURCE_ID_BYTES {
                        return Err(configuration_error());
                    }
                    let position = sources.binary_search_by(|r| r.id.cmp(&source.id));
                    if let Err(position) = position
                        && position < DIAGNOSTIC_SOURCE_ID_LIMIT
                    {
                        if sources.len() == DIAGNOSTIC_SOURCE_ID_LIMIT {
                            sources.pop();
                        }
                        sources.insert(position, source);
                    }
                }
                let holiday_count = next - index;
                let holidays = sources
                    .into_iter()
                    .take(DIAGNOSTIC_SOURCE_ID_LIMIT)
                    .map(|r| CollisionSource {
                        id: r.id.clone(),
                        name: r.name.clone(),
                    })
                    .collect::<Vec<_>>();
                let omitted_source_id_count = holiday_count - holidays.len();
                collisions.push(HolidayCollision {
                    date: rows[index].date.clone(),
                    holiday_count,
                    holidays,
                    omitted_source_id_count,
                });
            }
        }
        index = next;
    }
    let omitted_diagnostic_count = collision_count - collisions.len();
    Ok(CollisionPreview {
        start_date: start.format(),
        end_date: end.format(),
        collision_count,
        collisions,
        omitted_diagnostic_count,
    })
}
pub fn require_no_collisions(preview: &CollisionPreview) -> AppResult<()> {
    if preview.collision_count == 0 {
        return Ok(());
    }
    let mut error = AppError::new(
        ErrorCode::HolidayCollision,
        "Multiple active holidays share a date. Resolve holiday collisions before generation.",
    );
    error.context.key = preview.collisions.first().map(|c| c.date.clone());
    if let Some(first) = preview.collisions.first() {
        error.context.dependency_ids = Some(first.holidays.iter().map(|h| h.id.clone()).collect());
        error.context.dependency_names =
            Some(first.holidays.iter().map(|h| h.name.clone()).collect());
        error.context.dependency_count = Some(first.holiday_count as i64);
        error.context.omitted_source_id_count = Some(first.omitted_source_id_count);
    }
    Err(error)
}
pub async fn read_resolved_holidays(
    pool: &SqlitePool,
    window: HolidayWindow,
) -> AppResult<Vec<ResolvedHoliday>> {
    let (start, end) = window.bounds()?;
    let mut tx = pool.begin().await?;
    let rows = load_active_holidays(&mut tx, start, end).await?;
    tx.rollback().await?;
    tauri::async_runtime::spawn_blocking(move || resolve_active_holidays(&rows, start, end))
        .await
        .map_err(|_| configuration_error())?
}
pub async fn read_collisions(
    pool: &SqlitePool,
    window: HolidayWindow,
) -> AppResult<CollisionPreview> {
    let (start, end) = window.bounds()?;
    let rows = read_resolved_holidays(pool, window).await?;
    tauri::async_runtime::spawn_blocking(move || collision_preview(&rows, start, end))
        .await
        .map_err(|_| configuration_error())?
}

#[derive(Clone, Copy)]
enum SeedRule {
    Fixed(i64, i64),
    Nth(i64, &'static str, i64),
    Last(i64, &'static str),
}
struct HolidaySeed {
    key: &'static str,
    name: &'static str,
    set: HolidaySet,
    rule: SeedRule,
}
const HOLIDAY_SEEDS: &[HolidaySeed] = &[
    HolidaySeed {
        key: "holiday_memorial_day",
        name: "Memorial Day",
        set: HolidaySet::A,
        rule: SeedRule::Last(5, "monday"),
    },
    HolidaySeed {
        key: "holiday_labor_day",
        name: "Labor Day",
        set: HolidaySet::A,
        rule: SeedRule::Nth(9, "monday", 1),
    },
    HolidaySeed {
        key: "holiday_christmas_eve",
        name: "Christmas Eve",
        set: HolidaySet::A,
        rule: SeedRule::Fixed(12, 24),
    },
    HolidaySeed {
        key: "holiday_new_years_day",
        name: "New Year's Day",
        set: HolidaySet::A,
        rule: SeedRule::Fixed(1, 1),
    },
    HolidaySeed {
        key: "holiday_independence_day",
        name: "July 4th",
        set: HolidaySet::B,
        rule: SeedRule::Fixed(7, 4),
    },
    HolidaySeed {
        key: "holiday_thanksgiving",
        name: "Thanksgiving",
        set: HolidaySet::B,
        rule: SeedRule::Nth(11, "thursday", 4),
    },
    HolidaySeed {
        key: "holiday_new_years_eve",
        name: "New Year's Eve",
        set: HolidaySet::B,
        rule: SeedRule::Fixed(12, 31),
    },
    HolidaySeed {
        key: "holiday_christmas_day",
        name: "Christmas Day",
        set: HolidaySet::B,
        rule: SeedRule::Fixed(12, 25),
    },
];
pub async fn seed_holidays(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
) -> AppResult<()> {
    let timestamp = clock_timestamp(clock)?;
    for seed in HOLIDAY_SEEDS {
        let HolidaySeed {
            key,
            name,
            set,
            rule,
        } = *seed;
        let (kind, month, day, weekday, nth) = match rule {
            SeedRule::Fixed(m, d) => (DateKind::Fixed, m, Some(d), None, None),
            SeedRule::Nth(m, w, n) => (DateKind::NthWeekday, m, None, Some(w), Some(n)),
            SeedRule::Last(m, w) => (DateKind::LastWeekday, m, None, Some(w), None),
        };
        let retained =
            sqlx::query_as::<_, Holiday>("SELECT * FROM holidays WHERE seed_key=? ORDER BY id")
                .bind(key)
                .fetch_all(&mut **tx)
                .await?;
        if retained.len() > 1 {
            return Err(configuration_error());
        }
        if let Some(row) = retained.first() {
            validate_reference(tx, row)
                .await
                .map_err(|_| configuration_error())?;
            continue;
        }
        let row = Holiday {
            id: uuid::Uuid::new_v4().to_string(),
            seed_key: Some(key.to_owned()),
            data: HolidayData {
                name: name.to_owned(),
                holiday_set: set,
                date_kind: kind,
                month: Some(month),
                day_of_month: day,
                weekday: weekday.map(str::to_owned),
                nth,
                exact_date: None,
                coverage_behavior: CoverageBehavior::Normal,
                custom_coverage_template_id: None,
                active_status: ActiveStatus::Active,
                notes: None,
            },
            created_at: timestamp.clone(),
            updated_at: timestamp.clone(),
            archived_at: None,
            revision: 1,
        };
        write_holiday(tx, &row, true).await?;
    }
    Ok(())
}
pub async fn validate_holidays(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    let rows = sqlx::query_as::<_, Holiday>("SELECT * FROM holidays ORDER BY name_order_key,id")
        .fetch_all(&mut **tx)
        .await?;
    let mut keys = BTreeSet::new();
    for row in &rows {
        validate_reference(tx, row)
            .await
            .map_err(|_| configuration_error())?;
        if let Some(key) = &row.seed_key
            && !keys.insert(key)
        {
            return Err(configuration_error());
        }
    }
    // Stored collisions are intentional editable configuration, never startup failures.
    Ok(())
}
