use crate::{
    employees::{
        ActiveStatus, IdRequest, RecordFilter, RevisionRequest, advance_source_metadata,
        check_revision, mutation_result, normalize_optional_notes, read_employee,
        record_filter_values, require_not_archived, verify_metadata,
    },
    error::{AppError, AppResult, CommandResult, ErrorCode},
    metadata::{MetadataClock, clock_timestamp},
    shift_templates::read_template,
    text::normalize_required_text,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum TemplateRestrictionType {
    CannotWork,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct TemplateRestrictionData {
    pub shift_template_id: String,
    pub restriction_type: TemplateRestrictionType,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
}
impl TemplateRestrictionData {
    fn normalized(mut self) -> AppResult<Self> {
        // IDs identify a stored row; do not silently trim into a different reference.
        if normalize_required_text(&self.shift_template_id)? != self.shift_template_id {
            return Err(AppError::new(
                ErrorCode::InvalidRestriction,
                "Choose a saved shift template.",
            ));
        }
        self.notes = normalize_optional_notes(self.notes)?;
        Ok(self)
    }
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TemplateRestriction {
    pub id: String,
    pub employee_id: String,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: TemplateRestrictionData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTemplateRestrictionRequest {
    pub employee_id: String,
    pub restriction: TemplateRestrictionData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTemplateRestrictionRequest {
    pub id: String,
    pub expected_revision: i64,
    pub restriction: TemplateRestrictionData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListTemplateRestrictionsRequest {
    pub employee_id: String,
    #[serde(default)]
    pub filter: RecordFilter,
}

pub(crate) fn verify_restriction(row: &TemplateRestriction) -> AppResult<()> {
    if row.data.clone().normalized().map_err(|_| {
        AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A stored template restriction is invalid.",
        )
    })? != row.data
        || (row.archived_at.is_some() && row.data.active_status != ActiveStatus::Inactive)
    {
        return Err(AppError::new(
            ErrorCode::ConfigurationInvalid,
            "A stored template restriction requires investigation; it has not been changed.",
        ));
    }
    verify_metadata(
        &row.created_at,
        &row.updated_at,
        &row.archived_at,
        row.revision,
    )
}
async fn read_restriction(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> AppResult<TemplateRestriction> {
    normalize_required_text(id)?;
    let row = sqlx::query_as::<_, TemplateRestriction>(
        "SELECT * FROM employee_shift_template_restrictions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "The template restriction could not be found.",
        )
    })?;
    verify_restriction(&row)?;
    Ok(row)
}
pub async fn get_restriction(
    pool: &SqlitePool,
    request: IdRequest,
) -> AppResult<TemplateRestriction> {
    let mut tx = pool.begin().await?;
    let row = read_restriction(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_restrictions(
    pool: &SqlitePool,
    request: ListTemplateRestrictionsRequest,
) -> AppResult<Vec<TemplateRestriction>> {
    let (archive, active) = record_filter_values(request.filter);
    let mut tx = pool.begin().await?;
    read_employee(&mut tx, &request.employee_id).await?;
    let rows = sqlx::query_as::<_, TemplateRestriction>("SELECT * FROM employee_shift_template_restrictions WHERE employee_id=?1 AND (?2 IS NULL OR (archived_at IS NOT NULL)=?2) AND (?3 IS NULL OR active_status=?3) ORDER BY shift_template_id, id")
        .bind(request.employee_id).bind(archive).bind(active).fetch_all(&mut *tx).await?;
    tx.rollback().await?;
    for row in &rows {
        verify_restriction(row)?;
    }
    Ok(rows)
}

async fn require_active_sources(
    tx: &mut Transaction<'_, Sqlite>,
    employee_id: &str,
    template_id: &str,
) -> AppResult<()> {
    let employee = read_employee(tx, employee_id).await?;
    let template = read_template(tx, template_id).await?;
    if employee.archived_at.is_some() || employee.data.active_status != ActiveStatus::Active {
        let mut error = AppError::new(
            ErrorCode::InvalidRestriction,
            "Choose an active, non-archived employee before adding or reactivating a template restriction.",
        );
        error.context.id = Some(employee.id);
        return Err(error);
    }
    if template.archived_at.is_some() || template.data.active_status != ActiveStatus::Active {
        let mut error = AppError::new(
            ErrorCode::InvalidRestriction,
            "Choose an active, non-archived template before adding or reactivating a template restriction.",
        );
        error.context.id = Some(template.id);
        return Err(error);
    }
    Ok(())
}
async fn write_restriction(
    tx: &mut Transaction<'_, Sqlite>,
    row: &TemplateRestriction,
    create: bool,
) -> AppResult<()> {
    if row.data.active_status == ActiveStatus::Active && row.archived_at.is_none() {
        let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM employee_shift_template_restrictions WHERE employee_id=? AND shift_template_id=? AND id<>? AND active_status='active' AND archived_at IS NULL)")
            .bind(&row.employee_id).bind(&row.data.shift_template_id).bind(&row.id).fetch_one(&mut **tx).await?;
        if duplicate {
            return Err(AppError::new(
                ErrorCode::DuplicateRestriction,
                "An active restriction already exists for this employee and template. Edit it or make it inactive first.",
            ));
        }
    }
    let sql = if create {
        "INSERT INTO employee_shift_template_restrictions (id,employee_id,shift_template_id,restriction_type,active_status,notes,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
    } else {
        "UPDATE employee_shift_template_restrictions SET shift_template_id=?3,restriction_type=?4,active_status=?5,notes=?6,updated_at=?8,archived_at=?9,revision=?10 WHERE id=?1 AND revision=?10-1"
    };
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&row.employee_id)
        .bind(&row.data.shift_template_id)
        .bind(row.data.restriction_type)
        .bind(row.data.active_status)
        .bind(&row.data.notes)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .bind(&row.archived_at)
        .bind(row.revision)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the template restriction before saving.",
        ));
    }
    Ok(())
}
pub async fn create_restriction(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreateTemplateRestrictionRequest,
) -> AppResult<TemplateRestriction> {
    let data = request.restriction.normalized()?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    require_active_sources(&mut tx, &request.employee_id, &data.shift_template_id).await?;
    let timestamp = clock_timestamp(clock)?;
    let row = TemplateRestriction {
        id: uuid::Uuid::new_v4().to_string(),
        employee_id: request.employee_id,
        data,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    write_restriction(&mut tx, &row, true).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn update_restriction(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateTemplateRestrictionRequest,
) -> CommandResult<TemplateRestriction> {
    mutation_result(
        async {
            let data = request.restriction.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_restriction(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            if row.data == data {
                tx.rollback().await?;
                return Ok((row, false));
            }
            if row.data.shift_template_id != data.shift_template_id
                || (row.data.active_status == ActiveStatus::Inactive
                    && data.active_status == ActiveStatus::Active)
            {
                require_active_sources(&mut tx, &row.employee_id, &data.shift_template_id).await?;
            }
            row.data = data;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            write_restriction(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_restriction(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<TemplateRestriction> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_restriction(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_restriction(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}

pub async fn validate_restrictions(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    let rows = sqlx::query_as::<_, TemplateRestriction>("SELECT * FROM employee_shift_template_restrictions ORDER BY employee_id, shift_template_id, id").fetch_all(&mut **tx).await?;
    for row in &rows {
        verify_restriction(row)?;
        // Source existence/shape remains mandatory; source activity is not a historical invariant.
        read_employee(tx, &row.employee_id).await?;
        read_template(tx, &row.data.shift_template_id).await?;
    }
    Ok(())
}
