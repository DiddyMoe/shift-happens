use crate::{
    employees::{
        ActiveStatus, IdRequest, RecordFilter, RevisionRequest, advance_source_metadata,
        check_revision, mutation_result, normalize_optional_notes, read_employee,
        record_filter_values, require_not_archived, verify_metadata,
    },
    entry_intervals::EntryShape,
    error::{AppError, AppResult, CommandResult, ErrorCode},
    metadata::{MetadataClock, clock_timestamp},
    text::normalize_required_text,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum AvailabilityType {
    Available,
    Unavailable,
    Preferred,
    HardBlock,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum PtoStatus {
    Requested,
    Approved,
    Denied,
    Canceled,
}

fn exact_id(id: &str) -> AppResult<()> {
    if normalize_required_text(id)? != id {
        return Err(AppError::new(
            ErrorCode::InvalidText,
            "Choose a saved record ID.",
        ));
    }
    Ok(())
}
pub async fn require_entry_employee(
    tx: &mut Transaction<'_, Sqlite>,
    employee_id: &str,
    active: ActiveStatus,
) -> AppResult<()> {
    exact_id(employee_id)?;
    let employee = read_employee(tx, employee_id).await?;
    if active == ActiveStatus::Active
        && (employee.archived_at.is_some() || employee.data.active_status != ActiveStatus::Active)
    {
        let mut error = AppError::new(
            ErrorCode::InvalidEmployee,
            "An active entry requires an active, non-archived employee. You can still deactivate or archive existing entries.",
        );
        error.context.id = Some(employee.id);
        return Err(error);
    }
    Ok(())
}
fn date_filter(start: &Option<String>, end: &Option<String>) -> AppResult<()> {
    if let Some(v) = start {
        crate::local_values::LocalDate::parse(v)?;
    }
    if let Some(v) = end {
        crate::local_values::LocalDate::parse(v)?;
    }
    if let (Some(a), Some(b)) = (start, end)
        && a > b
    {
        return Err(AppError::new(
            ErrorCode::InvalidInterval,
            "The filter end date must be on or after its start date.",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct AvailabilityData {
    pub employee_id: String,
    pub start_date: String,
    pub end_date: String,
    pub is_full_day: bool,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
    pub entry_type: AvailabilityType,
}
impl AvailabilityData {
    fn normalized(mut self) -> AppResult<Self> {
        exact_id(&self.employee_id)?;
        self.shape()?;
        self.notes = normalize_optional_notes(self.notes)?;
        Ok(self)
    }
    pub fn shape(&self) -> AppResult<EntryShape> {
        EntryShape::parse(
            &self.start_date,
            &self.end_date,
            self.is_full_day,
            self.start_time.as_deref(),
            self.end_time.as_deref(),
        )
    }
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AvailabilityEntry {
    pub id: String,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: AvailabilityData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAvailabilityRequest {
    pub entry: AvailabilityData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAvailabilityRequest {
    pub id: String,
    pub expected_revision: i64,
    pub entry: AvailabilityData,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAvailabilityRequest {
    pub employee_id: Option<String>,
    #[serde(default)]
    pub filter: RecordFilter,
    pub entry_type: Option<AvailabilityType>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}
pub fn verify_availability(row: &AvailabilityEntry) -> AppResult<()> {
    if row.data.clone().normalized().map_err(|_| {
        AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A stored availability entry requires investigation; it has not been changed.",
        )
    })? != row.data
        || (row.archived_at.is_some() && row.data.active_status != ActiveStatus::Inactive)
    {
        return Err(AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A stored availability entry requires investigation; it has not been changed.",
        ));
    }
    exact_id(&row.id)?;
    verify_metadata(
        &row.created_at,
        &row.updated_at,
        &row.archived_at,
        row.revision,
    )
}
async fn read_availability(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> AppResult<AvailabilityEntry> {
    exact_id(id)?;
    let row =
        sqlx::query_as::<_, AvailabilityEntry>("SELECT * FROM availability_entries WHERE id=?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::NotFound,
                    "The availability entry could not be found.",
                )
            })?;
    verify_availability(&row)?;
    Ok(row)
}
pub async fn get_availability(
    pool: &SqlitePool,
    request: IdRequest,
) -> AppResult<AvailabilityEntry> {
    let mut tx = pool.begin().await?;
    let row = read_availability(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_availability(
    pool: &SqlitePool,
    request: ListAvailabilityRequest,
) -> AppResult<Vec<AvailabilityEntry>> {
    if let Some(id) = &request.employee_id {
        exact_id(id)?;
    }
    date_filter(&request.start_date, &request.end_date)?;
    let (archive, active) = record_filter_values(request.filter);
    let rows = sqlx::query_as::<_, AvailabilityEntry>("SELECT * FROM availability_entries WHERE (?1 IS NULL OR employee_id=?1) AND (?2 IS NULL OR (archived_at IS NOT NULL)=?2) AND (?3 IS NULL OR active_status=?3) AND (?4 IS NULL OR entry_type=?4) AND (?5 IS NULL OR end_date>=?5) AND (?6 IS NULL OR start_date<=?6) ORDER BY start_date,end_date,start_time,end_time,entry_type,employee_id,id")
        .bind(request.employee_id).bind(archive).bind(active).bind(request.entry_type).bind(request.start_date).bind(request.end_date).fetch_all(pool).await?;
    for row in &rows {
        verify_availability(row)?;
    }
    Ok(rows)
}
async fn write_availability(
    tx: &mut Transaction<'_, Sqlite>,
    row: &AvailabilityEntry,
    create: bool,
) -> AppResult<()> {
    verify_availability(row)?;
    let sql = if create {
        "INSERT INTO availability_entries (id,employee_id,start_date,end_date,is_full_day,start_time,end_time,active_status,notes,entry_type,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"
    } else {
        "UPDATE availability_entries SET employee_id=?2,start_date=?3,end_date=?4,is_full_day=?5,start_time=?6,end_time=?7,active_status=?8,notes=?9,entry_type=?10,updated_at=?12,archived_at=?13,revision=?14 WHERE id=?1 AND revision=?14-1"
    };
    let d = &row.data;
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&d.employee_id)
        .bind(&d.start_date)
        .bind(&d.end_date)
        .bind(d.is_full_day)
        .bind(&d.start_time)
        .bind(&d.end_time)
        .bind(d.active_status)
        .bind(&d.notes)
        .bind(d.entry_type)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .bind(&row.archived_at)
        .bind(row.revision)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the entry before saving.",
        ));
    }
    Ok(())
}
pub async fn create_availability(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateAvailabilityRequest,
) -> AppResult<AvailabilityEntry> {
    let data = request.entry.normalized()?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    require_entry_employee(&mut tx, &data.employee_id, data.active_status).await?;
    let stamp = clock_timestamp(clock)?;
    let row = AvailabilityEntry {
        id: uuid::Uuid::new_v4().to_string(),
        data,
        created_at: stamp.clone(),
        updated_at: stamp,
        archived_at: None,
        revision: 1,
    };
    write_availability(&mut tx, &row, true).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn update_availability(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateAvailabilityRequest,
) -> CommandResult<AvailabilityEntry> {
    mutation_result(
        async {
            let data = request.entry.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_availability(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            // Recheck on every write leaving an entry active, even a normalized no-op.
            require_entry_employee(&mut tx, &data.employee_id, data.active_status).await?;
            if row.data == data {
                tx.rollback().await?;
                return Ok((row, false));
            }
            row.data = data;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            write_availability(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_availability(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<AvailabilityEntry> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_availability(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_availability(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct PtoData {
    pub employee_id: String,
    pub start_date: String,
    pub end_date: String,
    pub is_full_day: bool,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
    pub status: PtoStatus,
}
impl PtoData {
    fn normalized(mut self) -> AppResult<Self> {
        exact_id(&self.employee_id)?;
        self.shape()?;
        self.notes = normalize_optional_notes(self.notes)?;
        Ok(self)
    }
    pub fn shape(&self) -> AppResult<EntryShape> {
        EntryShape::parse(
            &self.start_date,
            &self.end_date,
            self.is_full_day,
            self.start_time.as_deref(),
            self.end_time.as_deref(),
        )
    }
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PtoEntry {
    pub id: String,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: PtoData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePtoRequest {
    pub entry: PtoData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePtoRequest {
    pub id: String,
    pub expected_revision: i64,
    pub entry: PtoData,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListPtoRequest {
    pub employee_id: Option<String>,
    #[serde(default)]
    pub filter: RecordFilter,
    pub status: Option<PtoStatus>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}
pub fn verify_pto(row: &PtoEntry) -> AppResult<()> {
    if row.data.clone().normalized().map_err(|_| {
        AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A stored pto entry requires investigation; it has not been changed.",
        )
    })? != row.data
        || (row.archived_at.is_some() && row.data.active_status != ActiveStatus::Inactive)
    {
        return Err(AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A stored pto entry requires investigation; it has not been changed.",
        ));
    }
    exact_id(&row.id)?;
    verify_metadata(
        &row.created_at,
        &row.updated_at,
        &row.archived_at,
        row.revision,
    )
}
async fn read_pto(tx: &mut Transaction<'_, Sqlite>, id: &str) -> AppResult<PtoEntry> {
    exact_id(id)?;
    let row = sqlx::query_as::<_, PtoEntry>("SELECT * FROM pto_entries WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "The pto entry could not be found."))?;
    verify_pto(&row)?;
    Ok(row)
}
pub async fn get_pto(pool: &SqlitePool, request: IdRequest) -> AppResult<PtoEntry> {
    let mut tx = pool.begin().await?;
    let row = read_pto(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_pto(pool: &SqlitePool, request: ListPtoRequest) -> AppResult<Vec<PtoEntry>> {
    if let Some(id) = &request.employee_id {
        exact_id(id)?;
    }
    date_filter(&request.start_date, &request.end_date)?;
    let (archive, active) = record_filter_values(request.filter);
    let rows = sqlx::query_as::<_, PtoEntry>("SELECT * FROM pto_entries WHERE (?1 IS NULL OR employee_id=?1) AND (?2 IS NULL OR (archived_at IS NOT NULL)=?2) AND (?3 IS NULL OR active_status=?3) AND (?4 IS NULL OR status=?4) AND (?5 IS NULL OR end_date>=?5) AND (?6 IS NULL OR start_date<=?6) ORDER BY start_date,end_date,start_time,end_time,status,employee_id,id")
        .bind(request.employee_id).bind(archive).bind(active).bind(request.status).bind(request.start_date).bind(request.end_date).fetch_all(pool).await?;
    for row in &rows {
        verify_pto(row)?;
    }
    Ok(rows)
}
async fn write_pto(
    tx: &mut Transaction<'_, Sqlite>,
    row: &PtoEntry,
    create: bool,
) -> AppResult<()> {
    verify_pto(row)?;
    let sql = if create {
        "INSERT INTO pto_entries (id,employee_id,start_date,end_date,is_full_day,start_time,end_time,active_status,notes,status,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"
    } else {
        "UPDATE pto_entries SET employee_id=?2,start_date=?3,end_date=?4,is_full_day=?5,start_time=?6,end_time=?7,active_status=?8,notes=?9,status=?10,updated_at=?12,archived_at=?13,revision=?14 WHERE id=?1 AND revision=?14-1"
    };
    let d = &row.data;
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&d.employee_id)
        .bind(&d.start_date)
        .bind(&d.end_date)
        .bind(d.is_full_day)
        .bind(&d.start_time)
        .bind(&d.end_time)
        .bind(d.active_status)
        .bind(&d.notes)
        .bind(d.status)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .bind(&row.archived_at)
        .bind(row.revision)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the entry before saving.",
        ));
    }
    Ok(())
}
pub async fn create_pto(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreatePtoRequest,
) -> AppResult<PtoEntry> {
    let data = request.entry.normalized()?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    require_entry_employee(&mut tx, &data.employee_id, data.active_status).await?;
    let stamp = clock_timestamp(clock)?;
    let row = PtoEntry {
        id: uuid::Uuid::new_v4().to_string(),
        data,
        created_at: stamp.clone(),
        updated_at: stamp,
        archived_at: None,
        revision: 1,
    };
    write_pto(&mut tx, &row, true).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn update_pto(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdatePtoRequest,
) -> CommandResult<PtoEntry> {
    mutation_result(
        async {
            let data = request.entry.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_pto(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            // Recheck on every write leaving an entry active, even a normalized no-op.
            require_entry_employee(&mut tx, &data.employee_id, data.active_status).await?;
            if row.data == data {
                tx.rollback().await?;
                return Ok((row, false));
            }
            row.data = data;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            write_pto(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_pto(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<PtoEntry> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_pto(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_pto(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}

pub async fn validate_entries(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    let availability =
        sqlx::query_as::<_, AvailabilityEntry>("SELECT * FROM availability_entries ORDER BY id")
            .fetch_all(&mut **tx)
            .await?;
    for row in &availability {
        verify_availability(row)?;
        read_employee(tx, &row.data.employee_id).await?;
    }
    let pto = sqlx::query_as::<_, PtoEntry>("SELECT * FROM pto_entries ORDER BY id")
        .fetch_all(&mut **tx)
        .await?;
    for row in &pto {
        verify_pto(row)?;
        read_employee(tx, &row.data.employee_id).await?;
    }
    // Employee activity is a write-time invariant, not a historical startup invariant.
    Ok(())
}
