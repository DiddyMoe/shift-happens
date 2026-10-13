use crate::{
    employees::{
        IdRequest, RevisionRequest, advance_source_metadata, check_revision, mutation_result,
        normalize_optional_notes, require_not_archived, validate_revision, verify_metadata,
    },
    error::{AppError, AppResult, CommandResult, ErrorCode},
    generation_snapshot::GenerationSettingsSnapshot,
    local_values::LocalDate,
    metadata::{MetadataClock, parse_metadata_instant},
    schedule_write::SCHEDULE_WRITES,
    settings::{self, SettingValue},
    text::normalize_required_text,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum ScheduleStatus {
    Draft,
    Final,
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub status: ScheduleStatus,
    pub notes: Option<String>,
    pub generation_settings_snapshot: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub revision: i64,
    /// Set by finalization and kept after reopening. Once-final plans are records.
    pub finalized_once: bool,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleFilter {
    #[default]
    NonArchived,
    Draft,
    Final,
    Archived,
    Current,
    All,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSchedulesRequest {
    #[serde(default)]
    pub filter: ScheduleFilter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateScheduleRequest {
    pub id: String,
    pub expected_revision: i64,
    pub name: String,
    pub notes: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReopenScheduleRequest {
    pub id: String,
    pub expected_revision: i64,
    pub confirmed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetCurrentScheduleRequest {
    pub schedule_id: String,
    pub expected_setting_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClearCurrentScheduleRequest {
    pub expected_setting_revision: i64,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurrentSchedule {
    None {
        setting_revision: i64,
    },
    Current {
        setting_revision: i64,
        schedule: Box<Schedule>,
    },
    StaleSetting {
        setting_revision: i64,
        schedule_id: String,
    },
}
fn corrupt() -> AppError {
    AppError::new(
        ErrorCode::ConfigurationInvalid,
        "The stored schedule is invalid; it has not been changed.",
    )
}
pub fn verify_schedule(row: &Schedule) -> AppResult<()> {
    if normalize_required_text(&row.id)? != row.id
        || normalize_required_text(&row.name)? != row.name
        || normalize_optional_notes(row.notes.clone())? != row.notes
        || LocalDate::parse(&row.start_date)? > LocalDate::parse(&row.end_date)?
    {
        return Err(corrupt());
    }
    let snapshot = GenerationSettingsSnapshot::parse(&row.generation_settings_snapshot)
        .map_err(|_| corrupt())?;
    let start = LocalDate::parse(&row.start_date)?;
    let end = LocalDate::parse(&row.end_date)?;
    if snapshot.selected_coverage_by_date.len() as i64 != end.days_since(start) + 1
        || snapshot
            .selected_coverage_by_date
            .keys()
            .any(|date| date < &row.start_date || date > &row.end_date)
    {
        return Err(corrupt());
    }
    verify_metadata(
        &row.created_at,
        &row.updated_at,
        &row.archived_at,
        row.revision,
    )
}
pub fn require_draft(row: &Schedule) -> AppResult<()> {
    require_not_archived(&row.archived_at)?;
    if row.status != ScheduleStatus::Draft {
        return Err(AppError::new(
            ErrorCode::ScheduleReadOnly,
            "Reopen this final schedule before editing it.",
        ));
    }
    Ok(())
}
pub async fn read_schedule(tx: &mut Transaction<'_, Sqlite>, id: &str) -> AppResult<Schedule> {
    normalize_required_text(id)?;
    let row = sqlx::query_as::<_, Schedule>("SELECT * FROM schedules WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "The schedule could not be found."))?;
    verify_schedule(&row).map_err(|_| corrupt())?;
    Ok(row)
}
pub async fn get_schedule(pool: &SqlitePool, request: IdRequest) -> AppResult<Schedule> {
    let mut tx = pool.begin().await?;
    let row = read_schedule(&mut tx, &request.id).await?;
    tx.rollback().await?;
    Ok(row)
}
async fn current_setting(tx: &mut Transaction<'_, Sqlite>) -> AppResult<(Option<String>, i64)> {
    let setting = settings::read_in_transaction(tx)
        .await?
        .into_iter()
        .find(|s| s.key == "current_schedule_id")
        .ok_or_else(corrupt)?;
    let id = match setting.value {
        None => None,
        Some(SettingValue::ScheduleId(id)) => Some(id),
        _ => return Err(corrupt()),
    };
    Ok((id, setting.revision))
}
pub async fn read_current(tx: &mut Transaction<'_, Sqlite>) -> AppResult<CurrentSchedule> {
    let (id, setting_revision) = current_setting(tx).await?;
    let Some(schedule_id) = id else {
        return Ok(CurrentSchedule::None { setting_revision });
    };
    let row = sqlx::query_as::<_, Schedule>("SELECT * FROM schedules WHERE id=?")
        .bind(&schedule_id)
        .fetch_optional(&mut **tx)
        .await?;
    match row {
        Some(schedule) if schedule.archived_at.is_none() => {
            verify_schedule(&schedule).map_err(|_| corrupt())?;
            Ok(CurrentSchedule::Current {
                setting_revision,
                schedule: Box::new(schedule),
            })
        }
        _ => Ok(CurrentSchedule::StaleSetting {
            setting_revision,
            schedule_id,
        }),
    }
}
pub async fn get_current(pool: &SqlitePool) -> AppResult<CurrentSchedule> {
    let mut tx = pool.begin().await?;
    let current = read_current(&mut tx).await?;
    tx.rollback().await?;
    Ok(current)
}
pub async fn list_schedules(
    pool: &SqlitePool,
    request: ListSchedulesRequest,
) -> AppResult<Vec<Schedule>> {
    let mut tx = pool.begin().await?;
    let rows = list_in_transaction(&mut tx, request.filter).await?;
    tx.rollback().await?;
    Ok(rows)
}

async fn list_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    filter: ScheduleFilter,
) -> AppResult<Vec<Schedule>> {
    let (current_id, _) = current_setting(tx).await?;
    let (archive, status, only_current) = match filter {
        ScheduleFilter::NonArchived => (Some(false), None, false),
        ScheduleFilter::Draft => (Some(false), Some(ScheduleStatus::Draft), false),
        ScheduleFilter::Final => (Some(false), Some(ScheduleStatus::Final), false),
        ScheduleFilter::Archived => (Some(true), None, false),
        ScheduleFilter::Current => (Some(false), None, true),
        ScheduleFilter::All => (None, None, false),
    };
    // No SQL order: the sort below is total (ties end at the unique id), and every
    // row is verified whatever its position, with the same error for any row.
    let mut rows = sqlx::query_as::<_, Schedule>("SELECT * FROM schedules WHERE (?1 IS NULL OR (archived_at IS NOT NULL)=?1) AND (?2 IS NULL OR status=?2) AND (?3=0 OR id=?4)")
        .bind(archive).bind(status).bind(only_current).bind(current_id).fetch_all(&mut **tx).await?;
    // Parse once, before sorting. Older RFC3339 precisions do not sort as text.
    let mut keyed = Vec::with_capacity(rows.len());
    for row in rows.drain(..) {
        verify_schedule(&row).map_err(|_| corrupt())?;
        keyed.push((parse_metadata_instant(&row.updated_at)?, row));
    }
    keyed.sort_by(|a, b| {
        b.1.end_date
            .cmp(&a.1.end_date)
            .then_with(|| b.1.start_date.cmp(&a.1.start_date))
            .then_with(|| b.0.cmp(&a.0))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });
    Ok(keyed.into_iter().map(|(_, row)| row).collect())
}

#[derive(Clone, Serialize)]
pub struct ScheduleSummary {
    pub schedule: Schedule,
    pub open_shift_count: i64,
}
#[derive(Serialize)]
pub struct ScheduleDashboard {
    pub current: CurrentSchedule,
    pub shown: Option<ScheduleSummary>,
    pub recent: Vec<ScheduleSummary>,
}
/// Open-shift counts for these schedules in one grouped read. A schedule with no
/// open shift has no group and counts zero, as its own `count(*)` would.
async fn open_shift_counts(
    tx: &mut Transaction<'_, Sqlite>,
    schedules: &[&Schedule],
) -> AppResult<Vec<(String, i64)>> {
    if schedules.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT schedule_id, count(*) FROM schedule_assignments WHERE open_shift=1 AND schedule_id IN (",
    );
    let mut ids = query.separated(", ");
    for schedule in schedules {
        ids.push_bind(schedule.id.as_str());
    }
    ids.push_unseparated(") GROUP BY schedule_id");
    Ok(query.build_query_as().fetch_all(&mut **tx).await?)
}
fn summary(counts: &[(String, i64)], schedule: Schedule) -> ScheduleSummary {
    let open_shift_count = counts
        .iter()
        .find(|(id, _)| *id == schedule.id)
        .map_or(0, |(_, count)| *count);
    ScheduleSummary {
        schedule,
        open_shift_count,
    }
}
/// A single read-only view; fallback never repairs the explicit current setting.
pub async fn dashboard(pool: &SqlitePool) -> AppResult<ScheduleDashboard> {
    let mut tx = pool.begin().await?;
    let current = read_current(&mut tx).await?;
    let mut rows = list_in_transaction(&mut tx, ScheduleFilter::NonArchived).await?;
    rows.truncate(5);
    let current_schedule = match &current {
        CurrentSchedule::Current { schedule, .. } => Some(schedule.as_ref()),
        _ => None,
    };
    let counted: Vec<&Schedule> = rows.iter().chain(current_schedule).collect();
    let counts = open_shift_counts(&mut tx, &counted).await?;
    let shown = current_schedule.map(|schedule| summary(&counts, schedule.clone()));
    let recent: Vec<_> = rows
        .into_iter()
        .map(|schedule| summary(&counts, schedule))
        .collect();
    let shown = shown.or_else(|| recent.first().cloned());
    tx.rollback().await?;
    Ok(ScheduleDashboard {
        current,
        shown,
        recent,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizeScheduleRequest {
    pub schedule_id: String,
    pub expected_revision: i64,
    #[serde(deserialize_with = "required_acknowledgement")]
    pub acknowledged_warning_ids: Option<Vec<String>>,
}
fn required_acknowledgement<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error> {
    Option::<Vec<String>>::deserialize(deserializer)
}
#[derive(Serialize)]
pub struct FinalizationOutcome {
    pub schedule: Schedule,
    /// None only for an exact-revision already-final no_change response.
    pub validation: Option<crate::validation::ValidationResult>,
    pub open_shift_count: i64,
}
/// The only finalization owner. Validation and authorization share this write view.
pub async fn finalize_schedule(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: FinalizeScheduleRequest,
) -> CommandResult<FinalizationOutcome> {
    async fn finalize(
        pool: &SqlitePool,
        clock: &dyn MetadataClock,
        request: FinalizeScheduleRequest,
    ) -> AppResult<CommandResult<FinalizationOutcome>> {
        let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
        let mut schedule = read_schedule(&mut tx, &request.schedule_id).await?;
        require_not_archived(&schedule.archived_at)?;
        check_revision(&schedule.id, schedule.revision, request.expected_revision)?;
        let ids = crate::schedule_editor::acknowledgement(
            request.acknowledged_warning_ids,
            crate::validation::VALIDATION_OUTPUT_LIMIT,
        )?;
        let open_shift_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM schedule_assignments WHERE schedule_id=? AND open_shift=1",
        )
        .bind(&schedule.id)
        .fetch_one(&mut *tx)
        .await?;
        if schedule.status == ScheduleStatus::Final {
            tx.rollback().await?;
            return Ok(CommandResult::NoChange {
                data: FinalizationOutcome {
                    schedule,
                    validation: None,
                    open_shift_count,
                },
            });
        }
        require_draft(&schedule)?;
        let snapshot = crate::validation::load_validation_snapshot(
            &mut tx,
            &schedule.id,
            crate::validation::ValidationLimits::default(),
        )
        .await?;
        // The owned evaluator is identical to public validation. The transaction
        // deliberately stays open because its result authorizes this mutation.
        let validation = tauri::async_runtime::spawn_blocking(move || {
            crate::validation::validate_snapshot(snapshot)
        })
        .await
        .map_err(|_| corrupt())??;
        if validation.error_count > 0 || !validation.complete || validation.truncated {
            let mut error = AppError::new(
                ErrorCode::FinalizationBlocked,
                "Finalization is blocked. Resolve the errors and run validation again; errors cannot be overridden.",
            );
            error.context.id = Some(schedule.id);
            error.context.diagnostics =
                Some(serde_json::to_value(validation).map_err(|_| corrupt())?);
            tx.rollback().await?;
            return Ok(CommandResult::Blocked { error });
        }
        let mut required: Vec<_> = validation
            .findings
            .iter()
            .filter(|f| f.severity == crate::validation::Severity::Warning)
            .map(|f| f.id.clone())
            .collect();
        required.sort();
        if ids != required {
            tx.rollback().await?;
            return Ok(CommandResult::RequiresConfirmation {
                data: FinalizationOutcome {
                    schedule,
                    validation: Some(validation),
                    open_shift_count,
                },
            });
        }
        schedule.status = ScheduleStatus::Final;
        schedule.finalized_once = true;
        advance_source_metadata(&mut schedule.revision, &mut schedule.updated_at, clock)?;
        save_lifecycle(&mut tx, &schedule, request.expected_revision).await?;
        tx.commit().await?;
        // Still under the coordinator; poisoned/panicking history is quarantined.
        let warnings = crate::editor_history::invalidate_after_commit(&schedule.id)
            .into_iter()
            .collect();
        Ok(CommandResult::Success {
            data: FinalizationOutcome {
                schedule,
                validation: Some(validation),
                open_shift_count,
            },
            warnings,
        })
    }
    match finalize(pool, clock, request).await {
        Ok(result) => result,
        Err(error) => Err(error).into(),
    }
}
async fn save_lifecycle(
    tx: &mut Transaction<'_, Sqlite>,
    row: &Schedule,
    expected: i64,
) -> AppResult<()> {
    let result = sqlx::query("UPDATE schedules SET name=?,notes=?,status=?,archived_at=?,updated_at=?,revision=?,finalized_once=? WHERE id=? AND revision=?")
        .bind(&row.name).bind(&row.notes).bind(row.status).bind(&row.archived_at).bind(&row.updated_at)
        .bind(row.revision).bind(row.finalized_once).bind(&row.id).bind(expected).execute(&mut **tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload this schedule before saving.",
        ));
    }
    Ok(())
}
pub async fn update_schedule(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: UpdateScheduleRequest,
) -> CommandResult<Schedule> {
    lifecycle_result(
        async {
            let name = normalize_required_text(&request.name)?;
            let notes = normalize_optional_notes(request.notes)?;
            let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
            let mut row = read_schedule(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_draft(&row)?;
            if name == row.name && notes == row.notes {
                tx.rollback().await?;
                return Ok((row, false, Vec::new()));
            }
            row.name = name;
            row.notes = notes;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            save_lifecycle(&mut tx, &row, request.expected_revision).await?;
            tx.commit().await?;
            let warnings = crate::editor_history::invalidate_after_commit(&row.id)
                .into_iter()
                .collect();
            Ok((row, true, warnings))
        }
        .await,
    )
}
pub async fn reopen_schedule(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: ReopenScheduleRequest,
) -> CommandResult<Schedule> {
    lifecycle_result(
        async {
            let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
            let mut row = read_schedule(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            require_not_archived(&row.archived_at)?;
            if !request.confirmed {
                return Err(AppError::new(
                    ErrorCode::ReopenConfirmationRequired,
                    "Confirm reopening this final schedule to draft.",
                ));
            }
            if row.status == ScheduleStatus::Draft {
                tx.rollback().await?;
                return Ok((row, false, Vec::new()));
            }
            row.status = ScheduleStatus::Draft;
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            save_lifecycle(&mut tx, &row, request.expected_revision).await?;
            tx.commit().await?;
            let warnings = crate::editor_history::invalidate_after_commit(&row.id)
                .into_iter()
                .collect();
            Ok((row, true, warnings))
        }
        .await,
    )
}
/// Caller transaction only: generator/current/archive share this setting owner.
pub async fn change_current_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
    target: Option<&str>,
    expected_revision: i64,
) -> AppResult<bool> {
    validate_revision(expected_revision)?;
    if let Some(id) = target {
        let schedule = read_schedule(tx, id).await?;
        require_not_archived(&schedule.archived_at)?;
    }
    let (current, revision) = current_setting(tx).await?;
    check_revision("current_schedule_id", revision, expected_revision)?;
    if current.as_deref() == target {
        return Ok(false);
    }
    let mut next_revision = revision;
    let mut timestamp = String::new();
    advance_source_metadata(&mut next_revision, &mut timestamp, clock)?;
    let result = sqlx::query("UPDATE app_settings SET value=?,updated_at=?,revision=? WHERE key='current_schedule_id' AND revision=?")
        .bind(target).bind(timestamp).bind(next_revision).bind(revision).execute(&mut **tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the current schedule selection.",
        ));
    }
    Ok(true)
}
async fn change_current(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    target: Option<&str>,
    revision: i64,
) -> CommandResult<CurrentSchedule> {
    mutation_result(
        async {
            let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
            let changed = change_current_in_transaction(&mut tx, clock, target, revision).await?;
            let result = read_current(&mut tx).await?;
            if changed {
                tx.commit().await?;
            } else {
                tx.rollback().await?;
            }
            Ok((result, changed))
        }
        .await,
    )
}
pub async fn set_current(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: SetCurrentScheduleRequest,
) -> CommandResult<CurrentSchedule> {
    change_current(
        pool,
        clock,
        Some(&request.schedule_id),
        request.expected_setting_revision,
    )
    .await
}
pub async fn clear_current(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: ClearCurrentScheduleRequest,
) -> CommandResult<CurrentSchedule> {
    change_current(pool, clock, None, request.expected_setting_revision).await
}
pub async fn archive_schedule(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: RevisionRequest,
) -> CommandResult<Schedule> {
    lifecycle_result(
        async {
            let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
            let mut row = read_schedule(&mut tx, &request.id).await?;
            check_revision(&row.id, row.revision, request.expected_revision)?;
            if row.archived_at.is_some() {
                tx.rollback().await?;
                return Ok((row, false, Vec::new()));
            }
            let (current, revision) = current_setting(&mut tx).await?;
            if current.as_deref() == Some(&row.id) {
                change_current_in_transaction(&mut tx, clock, None, revision).await?;
            }
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            save_lifecycle(&mut tx, &row, request.expected_revision).await?;
            tx.commit().await?;
            let warnings = crate::editor_history::invalidate_after_commit(&row.id)
                .into_iter()
                .collect();
            Ok((row, true, warnings))
        }
        .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteScheduleRequest {
    pub id: String,
    pub expected_revision: i64,
    pub confirmed: bool,
}
/// The preview (`requires_confirmation`) and the committed deletion share one shape.
/// `cleared_current` is true only when a committed deletion cleared the selection.
#[derive(Serialize)]
pub struct ScheduleDeletion {
    pub id: String,
    pub name: String,
    pub assignment_count: i64,
    pub is_current: bool,
    pub cleared_current: bool,
}
fn retained_record() -> AppError {
    AppError::new(
        ErrorCode::FinalizedScheduleRetained,
        "Schedules that were finalized are kept as records. Archive it instead.",
    )
}
/// Only a draft that was never finalized can be deleted. Final plans, and drafts
/// reopened after finalization, are kept as payroll/hours records; archive them.
pub async fn delete_schedule(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: DeleteScheduleRequest,
) -> CommandResult<ScheduleDeletion> {
    async fn delete(
        pool: &SqlitePool,
        clock: &dyn MetadataClock,
        request: DeleteScheduleRequest,
    ) -> AppResult<CommandResult<ScheduleDeletion>> {
        let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
        let mut row = read_schedule(&mut tx, &request.id).await?;
        check_revision(&row.id, row.revision, request.expected_revision)?;
        if row.status != ScheduleStatus::Draft || row.finalized_once {
            return Err(retained_record());
        }
        let assignment_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM schedule_assignments WHERE schedule_id=?")
                .bind(&row.id)
                .fetch_one(&mut *tx)
                .await?;
        let (current, setting_revision) = current_setting(&mut tx).await?;
        let is_current = current.as_deref() == Some(row.id.as_str());
        if !request.confirmed {
            tx.rollback().await?;
            return Ok(CommandResult::RequiresConfirmation {
                data: ScheduleDeletion {
                    id: row.id,
                    name: row.name,
                    assignment_count,
                    is_current,
                    cleared_current: false,
                },
            });
        }
        if is_current {
            change_current_in_transaction(&mut tx, clock, None, setting_revision).await?;
        }
        // Storage permits deletion only for archived never-final drafts, so an
        // active draft is archived first inside this same transaction.
        if row.archived_at.is_none() {
            advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
            row.archived_at = Some(row.updated_at.clone());
            save_lifecycle(&mut tx, &row, request.expected_revision).await?;
        }
        let slots = sqlx::query("DELETE FROM schedule_assignments WHERE schedule_id=?")
            .bind(&row.id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if i64::try_from(slots).ok() != Some(assignment_count) {
            return Err(corrupt());
        }
        let deleted = sqlx::query("DELETE FROM schedules WHERE id=? AND revision=? AND status='draft' AND finalized_once=0")
            .bind(&row.id)
            .bind(row.revision)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted != 1 {
            return Err(AppError::new(
                ErrorCode::StaleRevision,
                "Reload this schedule before deleting it.",
            ));
        }
        let remaining: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM schedules WHERE id=?1) + (SELECT count(*) FROM schedule_assignments WHERE schedule_id=?1)",
        )
        .bind(&row.id)
        .fetch_one(&mut *tx)
        .await?;
        if remaining != 0 {
            return Err(corrupt());
        }
        tx.commit().await?;
        // Still under the coordinator; history for a deleted plan is unreachable.
        let warnings = crate::editor_history::invalidate_after_commit(&row.id)
            .into_iter()
            .collect();
        Ok(CommandResult::Success {
            data: ScheduleDeletion {
                id: row.id,
                name: row.name,
                assignment_count,
                is_current,
                cleared_current: is_current,
            },
            warnings,
        })
    }
    match delete(pool, clock, request).await {
        Ok(result) => result,
        Err(error) => Err(error).into(),
    }
}

fn lifecycle_result(result: AppResult<(Schedule, bool, Vec<AppError>)>) -> CommandResult<Schedule> {
    match result {
        Ok((data, true, warnings)) => CommandResult::Success { data, warnings },
        Ok((data, false, _)) => CommandResult::NoChange { data },
        Err(error) => Err(error).into(),
    }
}
