use crate::{
    employees::{
        ActiveStatus, IdRequest, RecordFilter, RevisionRequest, advance_source_metadata,
        check_revision, mutation_result, normalize_optional_label, normalize_optional_notes,
        record_filter_values, require_not_archived, validate_sort_order, verify_metadata,
    },
    error::{AppError, AppResult, CommandResult, ErrorCode},
    metadata::{MetadataClock, clock_timestamp},
    shift_templates::{DependencySource, require_can_deactivate_source},
    text::{compare_text_with_ids, normalize_required_text, stable_text_order_key},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(deny_unknown_fields)]
pub struct PositionData {
    pub name: String,
    pub display_label: Option<String>,
    pub sort_order: i64,
    pub active_status: ActiveStatus,
    pub notes: Option<String>,
}
impl PositionData {
    fn normalized(mut self) -> AppResult<Self> {
        self.name = normalize_required_text(&self.name)?;
        self.display_label = normalize_optional_label(self.display_label)?;
        self.notes = normalize_optional_notes(self.notes)?;
        validate_sort_order(self.sort_order)?;
        Ok(self)
    }
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Position {
    pub id: String,
    pub seed_key: Option<String>,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub data: PositionData,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePositionRequest {
    pub position: PositionData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePositionRequest {
    pub id: String,
    pub expected_revision: i64,
    pub position: PositionData,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListPositionsRequest {
    #[serde(default)]
    pub filter: RecordFilter,
}

pub(crate) fn configuration_error() -> AppError {
    AppError::new(
        ErrorCode::ConfigurationInvalid,
        "Stored positions or coverage require investigation; no data was changed.",
    )
}
pub(crate) fn verify_source_shape(
    seed_key: &Option<String>,
    active: ActiveStatus,
    created: &str,
    updated: &str,
    archived: &Option<String>,
    revision: i64,
) -> AppResult<()> {
    if (archived.is_some() && active != ActiveStatus::Inactive)
        || seed_key
            .as_ref()
            .is_some_and(|key| !normalize_required_text(key).is_ok_and(|v| v == *key))
    {
        return Err(configuration_error());
    }
    verify_metadata(created, updated, archived, revision)
}
pub(crate) fn verify_position(row: &Position) -> AppResult<()> {
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
pub async fn read_position(tx: &mut Transaction<'_, Sqlite>, id: &str) -> AppResult<Position> {
    normalize_required_text(id)?;
    let row = sqlx::query_as::<_, Position>("SELECT * FROM shift_positions WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "The position could not be found."))?;
    verify_position(&row)?;
    Ok(row)
}
pub async fn get_position(pool: &SqlitePool, request: IdRequest) -> AppResult<Position> {
    let mut tx = pool.begin().await?;
    let row = read_position(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
pub async fn list_positions(
    pool: &SqlitePool,
    request: ListPositionsRequest,
) -> AppResult<Vec<Position>> {
    let (archive, active) = record_filter_values(request.filter);
    let mut rows = sqlx::query_as::<_, Position>("SELECT * FROM shift_positions WHERE (?1 IS NULL OR (archived_at IS NOT NULL)=?1) AND (?2 IS NULL OR active_status=?2) ORDER BY sort_order,name_order_key,id")
        .bind(archive).bind(active).fetch_all(pool).await?;
    for row in &rows {
        verify_position(row)?;
    }
    rows.sort_by(|a, b| {
        a.data
            .sort_order
            .cmp(&b.data.sort_order)
            .then_with(|| compare_text_with_ids(&a.data.name, &a.id, &b.data.name, &b.id))
    });
    Ok(rows)
}
async fn write_position(
    tx: &mut Transaction<'_, Sqlite>,
    row: &Position,
    create: bool,
) -> AppResult<()> {
    verify_position(row)?;
    let sql = if create {
        "INSERT INTO shift_positions (id,seed_key,name,name_order_key,display_label,sort_order,active_status,notes,created_at,updated_at,archived_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
    } else {
        "UPDATE shift_positions SET name=?3,name_order_key=?4,display_label=?5,sort_order=?6,active_status=?7,notes=?8,updated_at=?10,archived_at=?11,revision=?12 WHERE id=?1 AND revision=?12-1"
    };
    let d = &row.data;
    let result = sqlx::query(sql)
        .bind(&row.id)
        .bind(&row.seed_key)
        .bind(&d.name)
        .bind(stable_text_order_key(&d.name))
        .bind(&d.display_label)
        .bind(d.sort_order)
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
            "Reload the position before saving.",
        ));
    }
    Ok(())
}
pub async fn create_position(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: CreatePositionRequest,
) -> AppResult<Position> {
    let data = request.position.normalized()?;
    let timestamp = clock_timestamp(clock)?;
    let row = Position {
        id: uuid::Uuid::new_v4().to_string(),
        seed_key: None,
        data,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
    };
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    write_position(&mut tx, &row, true).await?;
    tx.commit().await?;
    Ok(row)
}
pub async fn update_position(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdatePositionRequest,
) -> CommandResult<Position> {
    mutation_result(
        async {
            let data = request.position.normalized()?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_position(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            if row.data == data {
                tx.rollback().await?;
                return Ok((row, false));
            }
            if data.active_status == ActiveStatus::Inactive {
                require_can_deactivate_source(&mut tx, DependencySource::Position, &row.id).await?;
            }
            row.data = data;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            write_position(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
pub async fn archive_position(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<Position> {
    mutation_result(
        async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let mut row = read_position(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false));
            }
            require_can_deactivate_source(&mut tx, DependencySource::Position, &row.id).await?;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            row.data.active_status = ActiveStatus::Inactive;
            write_position(&mut tx, &row, false).await?;
            tx.commit().await?;
            Ok((row, true))
        }
        .await,
    )
}
const POSITION_SEEDS: &[(&str, &str)] = &[
    ("position_t1", "T1"),
    ("position_t2", "T2"),
    ("position_t3", "T3"),
    ("position_t4", "T4"),
    ("position_t5", "T5"),
    ("position_t8", "T8"),
    ("position_t12", "T12"),
    ("position_ortho_or", "ORTHO & OR"),
    ("position_midnight_night_shift", "Midnight / Night Shift"),
];
pub async fn seed_positions(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
) -> AppResult<()> {
    let timestamp = clock_timestamp(clock)?;
    for (index, (key, name)) in POSITION_SEEDS.iter().enumerate() {
        if let Some(row) =
            sqlx::query_as::<_, Position>("SELECT * FROM shift_positions WHERE seed_key=?")
                .bind(key)
                .fetch_optional(&mut **tx)
                .await?
        {
            verify_position(&row)?;
            continue;
        }
        let row = Position {
            id: uuid::Uuid::new_v4().to_string(),
            seed_key: Some((*key).to_owned()),
            data: PositionData {
                name: (*name).to_owned(),
                display_label: None,
                sort_order: (index as i64 + 1) * 10,
                active_status: ActiveStatus::Active,
                notes: None,
            },
            created_at: timestamp.clone(),
            updated_at: timestamp.clone(),
            archived_at: None,
            revision: 1,
        };
        write_position(tx, &row, true).await?;
    }
    Ok(())
}
pub async fn validate_positions(tx: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    let rows = sqlx::query_as::<_, Position>(
        "SELECT * FROM shift_positions ORDER BY sort_order,name_order_key,id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in &rows {
        verify_position(row)?;
    }
    Ok(())
}
