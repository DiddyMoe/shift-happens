use crate::{
    employees::{
        ActiveStatus, IdRequest, RecordFilter, RevisionRequest, advance_source_metadata,
        check_revision, mutation_result, normalize_optional_label, normalize_optional_notes,
        record_filter_values, require_not_archived, validate_sort_order,
    },
    error::{AppError, AppResult, CommandResult, ErrorCode},
    local_values::{LocalDate, LocalDateTime, LocalInterval, LocalTime, wall_clock_minutes},
    metadata::{MetadataClock, clock_timestamp},
    positions::{configuration_error, read_position, verify_source_shape},
    shift_templates::{
        DependencySource, ShiftTemplateData, read_template, require_can_deactivate_source,
        source_deactivation_dependencies,
    },
    shift_time::calculate_shift_time,
    text::{compare_text_with_ids, normalize_required_text, stable_text_order_key},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum DayType {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
    HolidayCustom,
}
impl DayType {
    pub const WEEKDAYS: [Self; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monday => "monday",
            Self::Tuesday => "tuesday",
            Self::Wednesday => "wednesday",
            Self::Thursday => "thursday",
            Self::Friday => "friday",
            Self::Saturday => "saturday",
            Self::Sunday => "sunday",
            Self::HolidayCustom => "holiday_custom",
        }
    }
    pub fn reference_date(self) -> AppResult<LocalDate> {
        // A Monday-based civil reference week; holiday custom uses its first day.
        LocalDate::parse("2026-10-05")?.add_days(match self {
            Self::HolidayCustom => 0,
            _ => self as i64,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct CoverageTemplateData {
    pub name: String,
    pub day_type: DayType,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
}
impl CoverageTemplateData {
    fn normalized(mut self) -> AppResult<Self> {
        self.name = normalize_required_text(&self.name)?;
        self.notes = normalize_optional_notes(self.notes)?;
        Ok(self)
    }
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CoverageTemplate {
    pub id: String,
    pub seed_key: Option<String>,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: CoverageTemplateData,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCoverageTemplateRequest {
    pub template: CoverageTemplateData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCoverageTemplateRequest {
    pub id: String,
    pub expected_revision: i64,
    pub template: CoverageTemplateData,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListCoverageTemplatesRequest {
    #[serde(default)]
    pub filter: RecordFilter,
    pub day_type: Option<DayType>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetDefaultCoverageTemplateRequest {
    pub id: String,
    pub expected_revision: i64,
    pub day_type: DayType,
    pub previous_default_id: String,
    pub previous_default_revision: i64,
}
pub(crate) fn verify_template(row: &CoverageTemplate) -> AppResult<()> {
    if row
        .data
        .clone()
        .normalized()
        .map_err(|_| configuration_error())?
        != row.data
        || (row.is_default
            && (row.data.day_type == DayType::HolidayCustom
                || row.data.active_status != ActiveStatus::Active
                || row.archived_at.is_some()))
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
}
pub async fn read_coverage_template(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> AppResult<CoverageTemplate> {
    normalize_required_text(id)?;
    let row = sqlx::query_as::<_, CoverageTemplate>("SELECT * FROM coverage_templates WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "The coverage template could not be found.",
            )
        })?;
    verify_template(&row)?;
    Ok(row)
}
pub async fn get_template(pool: &SqlitePool, request: IdRequest) -> AppResult<CoverageTemplate> {
    let mut tx = pool.begin().await?;
    let row = read_coverage_template(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_templates(
    pool: &SqlitePool,
    request: ListCoverageTemplatesRequest,
) -> AppResult<Vec<CoverageTemplate>> {
    let (archive, active) = record_filter_values(request.filter);
    let mut rows=sqlx::query_as::<_,CoverageTemplate>("SELECT * FROM coverage_templates WHERE (?1 IS NULL OR (archived_at IS NOT NULL)=?1) AND (?2 IS NULL OR active_status=?2) AND (?3 IS NULL OR day_type=?3) ORDER BY day_type,name_order_key,id")
        .bind(archive).bind(active).bind(request.day_type).fetch_all(pool).await?;
    for row in &rows {
        verify_template(row)?;
    }
    rows.sort_by(|a, b| {
        a.data
            .day_type
            .cmp(&b.data.day_type)
            .then_with(|| compare_text_with_ids(&a.data.name, &a.id, &b.data.name, &b.id))
    });
    Ok(rows)
}
async fn write_template(
    tx: &mut Transaction<'_, Sqlite>,
    row: &CoverageTemplate,
    create: bool,
) -> AppResult<()> {
    verify_template(row)?;
    let sql = if create {
        "INSERT INTO coverage_templates (id,seed_key,name,name_order_key,day_type,is_default,active_status,notes,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
    } else {
        // is_default has a separate, dedicated mutation path.
        "UPDATE coverage_templates SET name=?3,name_order_key=?4,day_type=?5,active_status=?7,notes=?8,updated_at=?10,archived_at=?11,revision=?12 WHERE id=?1 AND revision=?12-1"
    };
    let d = &row.data;
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&row.seed_key)
        .bind(&d.name)
        .bind(stable_text_order_key(&d.name))
        .bind(d.day_type)
        .bind(row.is_default)
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
            "Reload the coverage template before saving.",
        ));
    }
    Ok(())
}
fn default_error(day: DayType) -> AppError {
    let mut error = AppError::new(
        ErrorCode::WeekdayDefaultRequired,
        "Each weekday requires one selected, active, populated default. Populate and select a replacement before removing its default or last active item.",
    );
    error.context.key = Some(day.as_str().to_owned());
    error
}
/// Partial uniqueness handles at-most-one; the transaction checks exactly-one and nonempty.
pub async fn validate_weekday_defaults(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    for day in DayType::WEEKDAYS {
        let (count,populated): (i64,i64)=sqlx::query_as("SELECT count(*),coalesce(sum(EXISTS(SELECT 1 FROM coverage_template_items i WHERE i.coverage_template_id=t.id AND i.active_status='active' AND i.archived_at IS NULL)),0) FROM coverage_templates t WHERE t.day_type=? AND t.is_default=1 AND t.active_status='active' AND t.archived_at IS NULL")
            .bind(day).fetch_one(&mut **tx).await?;
        if count != 1 || populated != 1 {
            return Err(default_error(day));
        }
    }
    Ok(())
}
pub async fn create_template(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateCoverageTemplateRequest,
) -> AppResult<CoverageTemplate> {
    let data = request.template.normalized()?;
    let timestamp = clock_timestamp(clock)?;
    let row = CoverageTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        seed_key: None,
        data,
        is_default: false,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    write_template(&mut tx, &row, true).await?;
    validate_weekday_defaults(&mut tx).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn update_template(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateCoverageTemplateRequest,
) -> CommandResult<CoverageTemplate> {
    mutation_result(
        async {
            let data = request.template.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_coverage_template(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            validate_weekday_defaults(&mut tx).await?;
            if row.data == data {
                tx.rollback().await?;
                return Ok((row, false));
            }
            if row.is_default
                && (row.data.day_type != data.day_type
                    || data.active_status != ActiveStatus::Active)
            {
                return Err(default_error(row.data.day_type));
            }
            if data.active_status == ActiveStatus::Inactive {
                require_can_deactivate_source(&mut tx, DependencySource::CoverageTemplate, &row.id)
                    .await?;
            }
            if data.day_type != DayType::HolidayCustom {
                require_can_deactivate_source(&mut tx, DependencySource::HolidayCoverage, &row.id)
                    .await?;
            }
            row.data = data;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            write_template(&mut tx, &row, false).await?;
            validate_weekday_defaults(&mut tx).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_template(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<CoverageTemplate> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_coverage_template(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            validate_weekday_defaults(&mut tx).await?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            if row.is_default {
                return Err(default_error(row.data.day_type));
            }
            require_can_deactivate_source(&mut tx, DependencySource::CoverageTemplate, &row.id)
                .await?;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_template(&mut tx, &row, false).await?;
            validate_weekday_defaults(&mut tx).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn set_default_template(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: SetDefaultCoverageTemplateRequest,
) -> CommandResult<CoverageTemplate> {
    mutation_result(async {
        let mut tx=pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_weekday_defaults(&mut tx).await?;
        let mut row=read_coverage_template(&mut tx,&request.id).await?;
        check_revision(&row.id,row.revision,request.expected_revision)?;
        require_not_archived(&row.archived_at)?;
        if request.day_type==DayType::HolidayCustom || row.data.day_type!=request.day_type || row.data.active_status!=ActiveStatus::Active {
            return Err(AppError::new(ErrorCode::InvalidCoverageDefault,"Select an active, non-archived, populated template for the matching normal weekday."));
        }
        let current=sqlx::query_as::<_,CoverageTemplate>("SELECT * FROM coverage_templates WHERE day_type=? AND is_default=1 AND active_status='active' AND archived_at IS NULL").bind(request.day_type).fetch_one(&mut *tx).await?;
        verify_template(&current)?;
        if current.id!=request.previous_default_id {
            let mut error=AppError::new(ErrorCode::StaleRevision,"The selected default changed. Reload the templates before selecting again.");
            error.context.id=Some(current.id); error.context.current_revision=Some(current.revision); return Err(error);
        }
        check_revision(&current.id,current.revision,request.previous_default_revision)?;
        let items=sqlx::query_as::<_,CoverageItem>("SELECT * FROM coverage_template_items WHERE coverage_template_id=? AND active_status='active' AND archived_at IS NULL ORDER BY sort_order,id").bind(&row.id).fetch_all(&mut *tx).await?;
        if items.is_empty() { return Err(AppError::new(ErrorCode::InvalidCoverageDefault,"Add an active coverage item before selecting this template as the weekday default.")); }
        for item in &items { validate_item_sources(&mut tx,item).await?; }
        if current.id==row.id { tx.rollback().await?; return Ok((row,false)); }
        // Clear first to satisfy partial uniqueness, but expose neither intermediate state.
        let mut previous=current.clone();
        advance_source_metadata(&mut previous.revision,&mut previous.updated_at,clock)?;
        advance_source_metadata(&mut row.revision,&mut row.updated_at,clock)?;
        row.updated_at=previous.updated_at.clone();
        let result=sqlx::query("UPDATE coverage_templates SET is_default=0,updated_at=?,revision=? WHERE id=? AND revision=?")
            .bind(&previous.updated_at).bind(previous.revision).bind(&current.id).bind(current.revision).execute(&mut *tx).await?;
        if result.rows_affected()!=1 { return Err(AppError::new(ErrorCode::StaleRevision,"Reload the selected default before changing it.")); }
        let result=sqlx::query("UPDATE coverage_templates SET is_default=1,updated_at=?,revision=? WHERE id=? AND revision=?")
            .bind(&row.updated_at).bind(row.revision).bind(&row.id).bind(request.expected_revision).execute(&mut *tx).await?;
        if result.rows_affected()!=1 { return Err(AppError::new(ErrorCode::StaleRevision,"Reload the replacement default before changing it.")); }
        row.is_default=true;
        validate_weekday_defaults(&mut tx).await?;
        tx.commit().await?;
        Ok((row,true))
    }.await)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct CoverageItemData {
    pub coverage_template_id: String,
    pub shift_position_id: String,
    pub shift_template_id: String,
    pub display_label: Option<String>,
    pub sort_order: i64,
    pub required_count: i64,
    pub actual_start_day_offset: i64,
    pub actual_end_day_offset: i64,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
}
fn validate_offsets(start: i64, end: i64) -> AppResult<()> {
    if !matches!((start, end), (0, 0) | (0, 1) | (1, 1)) {
        return Err(AppError::new(
            ErrorCode::InvalidCoverageOffset,
            "Choose explicit offsets 0,0, 0,1, or 1,1. Offsets are never repaired automatically.",
        ));
    }
    Ok(())
}
impl CoverageItemData {
    fn normalized(mut self) -> AppResult<Self> {
        for id in [
            &self.coverage_template_id,
            &self.shift_position_id,
            &self.shift_template_id,
        ] {
            if normalize_required_text(id)? != *id {
                return Err(AppError::new(
                    ErrorCode::InvalidCoverageItem,
                    "Choose saved coverage, position, and shift records.",
                ));
            }
        }
        self.display_label = normalize_optional_label(self.display_label)?;
        self.notes = normalize_optional_notes(self.notes)?;
        validate_sort_order(self.sort_order)?;
        if !(1..=100).contains(&self.required_count) {
            return Err(AppError::new(
                ErrorCode::InvalidRequiredCount,
                "Required count must be a whole number from 1 through 100.",
            ));
        }
        validate_offsets(self.actual_start_day_offset, self.actual_end_day_offset)?;
        Ok(self)
    }
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CoverageItem {
    pub id: String,
    pub seed_key: Option<String>,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: CoverageItemData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCoverageItemRequest {
    pub item: CoverageItemData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCoverageItemRequest {
    pub id: String,
    pub expected_revision: i64,
    pub item: CoverageItemData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListCoverageItemsRequest {
    pub coverage_template_id: String,
    #[serde(default)]
    pub filter: RecordFilter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewCoverageIntervalRequest {
    pub shift_template_id: String,
    pub operational_date: String,
    pub actual_start_day_offset: i64,
    pub actual_end_day_offset: i64,
}
#[derive(Debug, Serialize)]
pub struct CoverageIntervalPreview {
    pub operational_date: String,
    pub operational_weekday: &'static str,
    pub actual_start: String,
    pub actual_end: String,
    pub actual_start_weekday: &'static str,
    pub actual_end_weekday: &'static str,
    pub elapsed_minutes: i64,
}
/// The same actual interval calculation serves previews, active-item writes, seeds and source edits.
pub fn coverage_interval(
    date: LocalDate,
    shift: &ShiftTemplateData,
    start_offset: i64,
    end_offset: i64,
) -> AppResult<CoverageIntervalPreview> {
    validate_offsets(start_offset, end_offset)?;
    let start =
        LocalDateTime::from_date_time(date, LocalTime::parse(&shift.start_time)?, start_offset)?;
    let end = LocalDateTime::from_date_time(date, LocalTime::parse(&shift.end_time)?, end_offset)?;
    LocalInterval::new(start, end).map_err(|_| {
        AppError::new(
            ErrorCode::InvalidCoverageOffset,
            "The actual end must follow the actual start using the stored offsets.",
        )
    })?;
    let elapsed = wall_clock_minutes(start, end)?;
    if elapsed != calculate_shift_time(&shift.start_time, &shift.end_time)?.elapsed_minutes {
        return Err(AppError::new(
            ErrorCode::InvalidCoverageOffset,
            "The explicit offsets must preserve the linked shift's elapsed wall-clock duration.",
        ));
    }
    Ok(CoverageIntervalPreview {
        operational_date: date.format(),
        operational_weekday: date.weekday().as_str(),
        actual_start: start.format(),
        actual_end: end.format(),
        actual_start_weekday: start.date().weekday().as_str(),
        actual_end_weekday: end.date().weekday().as_str(),
        elapsed_minutes: elapsed,
    })
}
pub async fn preview_interval(
    pool: &SqlitePool,
    request: PreviewCoverageIntervalRequest,
) -> AppResult<CoverageIntervalPreview> {
    let date = LocalDate::parse(&request.operational_date)?;
    let mut tx = pool.begin().await?;
    let shift = read_template(&mut tx, &request.shift_template_id).await?;
    let preview = coverage_interval(
        date,
        &shift.data,
        request.actual_start_day_offset,
        request.actual_end_day_offset,
    )?;
    tx.rollback().await?;
    Ok(preview)
}
pub(crate) fn verify_item(row: &CoverageItem) -> AppResult<()> {
    if row
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
}
pub(crate) async fn read_item(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> AppResult<CoverageItem> {
    normalize_required_text(id)?;
    let row = sqlx::query_as::<_, CoverageItem>("SELECT * FROM coverage_template_items WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            AppError::new(ErrorCode::NotFound, "The coverage item could not be found.")
        })?;
    verify_item(&row)?;
    Ok(row)
}
pub async fn get_item(pool: &SqlitePool, request: IdRequest) -> AppResult<CoverageItem> {
    let mut tx = pool.begin().await?;
    let row = read_item(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_items(
    pool: &SqlitePool,
    request: ListCoverageItemsRequest,
) -> AppResult<Vec<CoverageItem>> {
    let (archive, active) = record_filter_values(request.filter);
    let mut tx = pool.begin().await?;
    read_coverage_template(&mut tx, &request.coverage_template_id).await?;
    let rows=sqlx::query_as::<_,CoverageItem>("SELECT * FROM coverage_template_items WHERE coverage_template_id=?1 AND (?2 IS NULL OR (archived_at IS NOT NULL)=?2) AND (?3 IS NULL OR active_status=?3) ORDER BY sort_order,id")
        .bind(&request.coverage_template_id).bind(archive).bind(active).fetch_all(&mut *tx).await?;
    for row in &rows {
        verify_item(row)?;
    }
    tx.rollback().await?;
    Ok(rows)
}
pub(crate) async fn validate_item_sources(
    tx: &mut Transaction<'_, Sqlite>,
    row: &CoverageItem,
) -> AppResult<()> {
    verify_item(row)?;
    let d = &row.data;
    let template = read_coverage_template(tx, &d.coverage_template_id).await?;
    let position = read_position(tx, &d.shift_position_id).await?;
    let shift = read_template(tx, &d.shift_template_id).await?;
    if d.active_status == ActiveStatus::Active && row.archived_at.is_none() {
        for (id, active, archived) in [
            (
                &template.id,
                template.data.active_status,
                template.archived_at.is_some(),
            ),
            (
                &position.id,
                position.data.active_status,
                position.archived_at.is_some(),
            ),
            (
                &shift.id,
                shift.data.active_status,
                shift.archived_at.is_some(),
            ),
        ] {
            if archived || active != ActiveStatus::Active {
                let mut error = AppError::new(
                    ErrorCode::InvalidCoverageItem,
                    "Active coverage requires active, non-archived coverage, position, and shift records.",
                );
                error.context.id = Some(id.clone());
                return Err(error);
            }
        }
        coverage_interval(
            template.data.day_type.reference_date()?,
            &shift.data,
            d.actual_start_day_offset,
            d.actual_end_day_offset,
        )?;
    }
    Ok(())
}
async fn write_item(
    tx: &mut Transaction<'_, Sqlite>,
    row: &CoverageItem,
    create: bool,
) -> AppResult<()> {
    validate_item_sources(tx, row).await?;
    let d = &row.data;
    if d.active_status == ActiveStatus::Active && row.archived_at.is_none() {
        let duplicate:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM coverage_template_items WHERE coverage_template_id=? AND shift_position_id=? AND shift_template_id=? AND actual_start_day_offset=? AND actual_end_day_offset=? AND id<>? AND active_status='active' AND archived_at IS NULL)")
            .bind(&d.coverage_template_id).bind(&d.shift_position_id).bind(&d.shift_template_id).bind(d.actual_start_day_offset).bind(d.actual_end_day_offset).bind(&row.id).fetch_one(&mut **tx).await?;
        if duplicate {
            return Err(AppError::new(
                ErrorCode::DuplicateCoverageItem,
                "An active item already uses this position, shift, and offsets. Edit its required count for more slots.",
            ));
        }
    }
    let sql = if create {
        "INSERT INTO coverage_template_items (id,seed_key,coverage_template_id,shift_position_id,shift_template_id,display_label,sort_order,required_count,actual_start_day_offset,actual_end_day_offset,active_status,notes,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"
    } else {
        "UPDATE coverage_template_items SET coverage_template_id=?3,shift_position_id=?4,shift_template_id=?5,display_label=?6,sort_order=?7,required_count=?8,actual_start_day_offset=?9,actual_end_day_offset=?10,active_status=?11,notes=?12,updated_at=?14,archived_at=?15,revision=?16 WHERE id=?1 AND revision=?16-1"
    };
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&row.seed_key)
        .bind(&d.coverage_template_id)
        .bind(&d.shift_position_id)
        .bind(&d.shift_template_id)
        .bind(&d.display_label)
        .bind(d.sort_order)
        .bind(d.required_count)
        .bind(d.actual_start_day_offset)
        .bind(d.actual_end_day_offset)
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
            "Reload the coverage item before saving.",
        ));
    }
    Ok(())
}
// Check before writing so a rejected last-item edit returns actionable domain
// dependencies instead of the database trigger's generic persistence error.
async fn require_can_remove_holiday_item(
    tx: &mut Transaction<'_, Sqlite>,
    row: &CoverageItem,
) -> AppResult<()> {
    if row.data.active_status != ActiveStatus::Active || row.archived_at.is_some() {
        return Ok(());
    }
    let other: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM coverage_template_items WHERE coverage_template_id=? AND id<>? AND active_status='active' AND archived_at IS NULL)")
        .bind(&row.data.coverage_template_id).bind(&row.id).fetch_one(&mut **tx).await?;
    if !other {
        require_can_deactivate_source(
            tx,
            DependencySource::HolidayCoverage,
            &row.data.coverage_template_id,
        )
        .await?;
    }
    Ok(())
}
pub async fn create_item(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateCoverageItemRequest,
) -> AppResult<CoverageItem> {
    let data = request.item.normalized()?;
    let timestamp = clock_timestamp(clock)?;
    let row = CoverageItem {
        id: uuid::Uuid::new_v4().to_string(),
        seed_key: None,
        data,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    write_item(&mut tx, &row, true).await?;
    validate_weekday_defaults(&mut tx).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn update_item(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateCoverageItemRequest,
) -> CommandResult<CoverageItem> {
    mutation_result(
        async {
            let data = request.item.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_item(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            validate_weekday_defaults(&mut tx).await?;
            if row.data == data {
                validate_item_sources(&mut tx, &row).await?;
                tx.rollback().await?;
                return Ok((row, false));
            }
            if row.data.coverage_template_id != data.coverage_template_id
                || data.active_status == ActiveStatus::Inactive
            {
                require_can_remove_holiday_item(&mut tx, &row).await?;
            }
            row.data = data;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            write_item(&mut tx, &row, false).await?;
            validate_weekday_defaults(&mut tx).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_item(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<CoverageItem> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_item(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            validate_weekday_defaults(&mut tx).await?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            require_can_remove_holiday_item(&mut tx, &row).await?;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_item(&mut tx, &row, false).await?;
            validate_weekday_defaults(&mut tx).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
/// Proposed source edits must preserve every active item's explicit interval.
pub async fn validate_shift_change(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    proposed: &ShiftTemplateData,
) -> AppResult<()> {
    let rows=sqlx::query_as::<_,CoverageItem>("SELECT * FROM coverage_template_items WHERE shift_template_id=? AND active_status='active' AND archived_at IS NULL ORDER BY id").bind(id).fetch_all(&mut **tx).await?;
    for row in &rows {
        verify_item(row)?;
        if coverage_interval(
            LocalDate::parse("2026-10-05")?,
            proposed,
            row.data.actual_start_day_offset,
            row.data.actual_end_day_offset,
        )
        .is_err()
        {
            let mut error = AppError::new(
                ErrorCode::InvalidCoverageOffset,
                "This time change would invalidate an active coverage item's stored offsets. Update the coverage configuration first.",
            );
            error.context.id = Some(id.to_owned());
            let dependencies =
                source_deactivation_dependencies(tx, DependencySource::ShiftTemplate, id).await?;
            error.context.dependency_ids = Some(dependencies.ids);
            error.context.dependency_names = Some(dependencies.names);
            error.context.dependency_count = Some(dependencies.count);
            error.context.omitted_source_id_count = Some(dependencies.omitted_source_id_count);
            return Err(error);
        }
    }
    Ok(())
}

pub async fn seed_coverage(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
) -> AppResult<()> {
    let timestamp = clock_timestamp(clock)?;
    for day in DayType::WEEKDAYS {
        let key = format!("coverage_{}", day.as_str());
        let retained = sqlx::query_as::<_, CoverageTemplate>(
            "SELECT * FROM coverage_templates WHERE seed_key=?",
        )
        .bind(&key)
        .fetch_optional(&mut **tx)
        .await?;
        let template = if let Some(row) = retained {
            verify_template(&row)?;
            row
        } else {
            let has_default:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM coverage_templates WHERE day_type=? AND is_default=1 AND active_status='active' AND archived_at IS NULL)").bind(day).fetch_one(&mut **tx).await?;
            let title = match day {
                DayType::Monday => "Monday",
                DayType::Tuesday => "Tuesday",
                DayType::Wednesday => "Wednesday",
                DayType::Thursday => "Thursday",
                DayType::Friday => "Friday",
                DayType::Saturday => "Saturday",
                DayType::Sunday => "Sunday",
                DayType::HolidayCustom => unreachable!(),
            };
            let row = CoverageTemplate {
                id: uuid::Uuid::new_v4().to_string(),
                seed_key: Some(key.clone()),
                data: CoverageTemplateData {
                    name: format!("{title} Coverage"),
                    day_type: day,
                    active_status: ActiveStatus::Active,
                    notes: None,
                },
                is_default: !has_default,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
                archived_at: None,
                revision: 1,
            };
            write_template(tx, &row, true).await?;
            row
        };
        // This matrix is used only for initial stored rows, never runtime coverage selection.
        let mut matrix = vec![
            ("position_t1", "shift_day_0645_1515", 0, 0),
            (
                "position_t2",
                if day == DayType::Wednesday {
                    "shift_wednesday_t2_0730_1600"
                } else {
                    "shift_day_0645_1515"
                },
                0,
                0,
            ),
            (
                match day {
                    DayType::Saturday => "position_t8",
                    DayType::Sunday => "position_t12",
                    _ => "position_t3",
                },
                match day {
                    DayType::Saturday => "shift_t8_1000_1400",
                    DayType::Sunday => "shift_t12_1230_2100",
                    _ => "shift_t3_0900_1730",
                },
                0,
                0,
            ),
            ("position_t4", "shift_evening_1500_2330", 0, 0),
        ];
        if day != DayType::Sunday {
            matrix.push((
                "position_t5",
                if day == DayType::Wednesday {
                    "shift_wednesday_t5_1600_0030"
                } else {
                    "shift_evening_1500_2330"
                },
                0,
                if day == DayType::Wednesday { 1 } else { 0 },
            ));
        }
        if day == DayType::Wednesday {
            matrix.push(("position_ortho_or", "shift_ortho_or_1900_2300", 0, 0));
        }
        matrix.push((
            "position_midnight_night_shift",
            match day {
                DayType::Wednesday => "shift_wednesday_midnight_0015_0700",
                DayType::Saturday | DayType::Sunday => "shift_weekend_midnight_1930_0700",
                _ => "shift_weekday_midnight_1900_0700",
            },
            if day == DayType::Wednesday { 1 } else { 0 },
            1,
        ));
        for (index, (position_key, shift_key, start, end)) in matrix.iter().enumerate() {
            let sort_order = (index as i64 + 1) * 10;
            let item_key = format!("{key}:{position_key}:{shift_key}:{sort_order}");
            if let Some(retained) = sqlx::query_as::<_, CoverageItem>(
                "SELECT * FROM coverage_template_items WHERE seed_key=?",
            )
            .bind(&item_key)
            .fetch_optional(&mut **tx)
            .await?
            {
                // Historical items keep their shape and links without matching edited source times.
                validate_item_sources(tx, &retained)
                    .await
                    .map_err(|_| configuration_error())?;
                continue;
            }
            let position_id: String =
                sqlx::query_scalar("SELECT id FROM shift_positions WHERE seed_key=?")
                    .bind(position_key)
                    .fetch_one(&mut **tx)
                    .await?;
            let shift_id: String =
                sqlx::query_scalar("SELECT id FROM shift_templates WHERE seed_key=?")
                    .bind(shift_key)
                    .fetch_one(&mut **tx)
                    .await?;
            let row = CoverageItem {
                id: uuid::Uuid::new_v4().to_string(),
                seed_key: Some(item_key),
                data: CoverageItemData {
                    coverage_template_id: template.id.clone(),
                    shift_position_id: position_id,
                    shift_template_id: shift_id,
                    display_label: None,
                    sort_order,
                    required_count: 1,
                    actual_start_day_offset: *start,
                    actual_end_day_offset: *end,
                    active_status: ActiveStatus::Active,
                    notes: None,
                },
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
                archived_at: None,
                revision: 1,
            };
            write_item(tx, &row, true)
                .await
                .map_err(|_| configuration_error())?;
        }
    }
    Ok(())
}
pub async fn validate_coverage(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    let templates = sqlx::query_as::<_, CoverageTemplate>(
        "SELECT * FROM coverage_templates ORDER BY day_type,name_order_key,id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in &templates {
        verify_template(row)?;
    }
    let items = sqlx::query_as::<_, CoverageItem>(
        "SELECT * FROM coverage_template_items ORDER BY coverage_template_id,sort_order,id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in &items {
        validate_item_sources(tx, row)
            .await
            .map_err(|_| configuration_error())?;
    }
    validate_weekday_defaults(tx).await
}
