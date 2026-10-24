use crate::{
    backup_sqlite::{self, Connection},
    database::{self, SqliteEngine},
    error::{
        AppError, AppResult, CommandResult, DIAGNOSTIC_LIMIT, DIAGNOSTIC_SOURCE_ID_BYTES, ErrorCode,
    },
    metadata::{MetadataClock, clock_timestamp, parse_metadata_instant},
    safe_destination::{self, DestinationPolicy, PreparedResult, SafeDestinationService},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationPolicy {
    ManualPreserve,
    PreMigrationStrict,
}
#[derive(Clone, Debug, Serialize)]
pub struct ForeignKeyDetail {
    pub table: String,
    pub parent: String,
    pub foreign_key_index: i64,
    pub violation_count: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ForeignKeyResult {
    pub violation_count: u64,
    pub details: Vec<ForeignKeyDetail>,
    pub omitted_violation_count: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct BackupFile {
    pub destination_path: String,
    pub file_size_bytes: u64,
    pub started_at: String,
    pub completed_at: String,
    pub integrity: &'static str,
    pub foreign_keys: ForeignKeyResult,
    pub validation_policy: ValidationPolicy,
    pub destination_policy: &'static str,
    pub engine: SqliteEngine,
}
#[derive(Clone)]
pub struct BackupAttempt {
    pub destination_path: String,
    pub started_at: String,
    pub completed_at: String,
}
pub enum BackupFileResult {
    Success {
        file: BackupFile,
        warnings: Vec<AppError>,
    },
    RequiresOverwrite {
        destination_path: String,
        overwrite_token: String,
    },
    Failure {
        error: AppError,
        attempt: Option<BackupAttempt>,
    },
}
fn validation_error() -> AppError {
    AppError::new(
        ErrorCode::BackupValidationFailed,
        "The backup failed independent SQLite integrity or standalone-file validation. No backup was published.",
    )
}
fn schema_identifier(value: String) -> String {
    if value.len() <= DIAGNOSTIC_SOURCE_ID_BYTES
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        value
    } else {
        "unrecognized_schema_identifier".to_owned()
    }
}
fn validate(connection: &Connection) -> AppResult<ForeignKeyResult> {
    if connection.single_text(c"PRAGMA journal_mode")? != "delete" {
        return Err(validation_error());
    }
    if connection.single_text(c"PRAGMA integrity_check")? != "ok" {
        return Err(validation_error());
    }
    let mut statement = connection.statement(c"PRAGMA foreign_key_check")?;
    let mut details: BTreeMap<(String, String, i64), u64> = BTreeMap::new();
    let mut violation_count = 0u64;
    let mut omitted_violation_count = 0u64;
    while statement.next()? {
        violation_count = violation_count
            .checked_add(1)
            .ok_or_else(validation_error)?;
        let key = (
            schema_identifier(statement.text(0)?),
            schema_identifier(statement.text(2)?),
            statement.integer(3),
        );
        // Bound diagnostic allocation. Keep deterministic smallest keys regardless
        // of SQLite row order. Omitted counts never include row contents/notes.
        if let Some(count) = details.get_mut(&key) {
            *count += 1;
        } else if details.len() < DIAGNOSTIC_LIMIT {
            details.insert(key, 1);
        } else if details
            .last_key_value()
            .is_some_and(|(last, _)| &key < last)
        {
            if let Some((_, count)) = details.pop_last() {
                omitted_violation_count += count;
            }
            details.insert(key, 1);
        } else {
            omitted_violation_count += 1;
        }
    }
    statement.finish()?;
    Ok(ForeignKeyResult {
        violation_count,
        omitted_violation_count,
        details: details
            .into_iter()
            .map(
                |((table, parent, foreign_key_index), violation_count)| ForeignKeyDetail {
                    table,
                    parent,
                    foreign_key_index,
                    violation_count,
                },
            )
            .collect(),
    })
}
fn header(path: &Path) -> AppResult<()> {
    safe_destination::require_regular_file(path)?;
    let mut file = safe_destination::open_narrow(path, false).map_err(|_| validation_error())?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header)
        .map_err(|_| validation_error())?;
    if &header != b"SQLite format 3\0" {
        return Err(validation_error());
    }
    Ok(())
}

/// Pure file operation, independent of Tauri, SQLx pools, migrations and history.
/// Exactly one implementation serves manual preservation and strict migration backup.
/// `check_deadline` interrupts the independent checks; restore passes one because
/// its source comes from outside the app.
// Eight independent inputs that each of the four callers sets; a parameter
// struct would only wrap them.
#[allow(clippy::too_many_arguments)]
pub fn create_consistent_sqlite_backup(
    source_database_path: &Path,
    final_destination_path: &Path,
    overwrite_authorization: Option<&str>,
    destination_policy: DestinationPolicy,
    validation_policy: ValidationPolicy,
    check_deadline: Option<Instant>,
    metadata_clock: &dyn MetadataClock,
    destinations: &SafeDestinationService,
) -> BackupFileResult {
    let prepared = (|| {
        header(source_database_path).map_err(|_| {
            AppError::new(
                ErrorCode::BackupSourceInvalid,
                "The backup source must be an existing regular SQLite database.",
            )
        })?;
        backup_sqlite::loaded_engine()?;
        if matches!(validation_policy, ValidationPolicy::PreMigrationStrict)
            && !matches!(
                destination_policy,
                DestinationPolicy::AppOwnedNoOverwrite { .. }
            )
        {
            return Err(AppError::new(
                ErrorCode::InvalidDestination,
                "Pre-migration backups require an app-owned no-overwrite destination.",
            ));
        }
        destinations.prepare(
            source_database_path,
            final_destination_path,
            "sqlite",
            overwrite_authorization,
            destination_policy.clone(),
        )
    })();
    let destination = match prepared {
        Ok(PreparedResult::Ready(destination)) => destination,
        Ok(PreparedResult::RequiresOverwrite {
            destination_path,
            overwrite_token,
        }) => {
            return BackupFileResult::RequiresOverwrite {
                destination_path,
                overwrite_token,
            };
        }
        Err(error) => {
            return BackupFileResult::Failure {
                error,
                attempt: None,
            };
        }
    };
    let started_at = match clock_timestamp(metadata_clock) {
        Ok(value) => value,
        Err(error) => {
            return BackupFileResult::Failure {
                error,
                attempt: None,
            };
        }
    };
    let mut attempt = BackupAttempt {
        destination_path: safe_destination::display_path(destination.final_path()),
        started_at: started_at.clone(),
        completed_at: started_at.clone(),
    };
    let mut temporary = match destinations.claim_temporary(&destination, true) {
        Ok(temporary) => temporary,
        Err(error) => {
            attempt.completed_at = clock_timestamp(metadata_clock).unwrap_or(started_at);
            return BackupFileResult::Failure {
                error,
                attempt: Some(attempt),
            };
        }
    };
    let written = (|| {
        temporary.verify()?;
        safe_destination::require_no_sidecars(&temporary.path)?;
        let source = Connection::open(source_database_path, true)?;
        let engine = backup_sqlite::loaded_engine()?;
        // Require the identification read through this real source connection too.
        if source.single_text(c"SELECT sqlite_version()")? != engine.version
            || source.single_text(c"SELECT sqlite_source_id()")? != engine.source_id
        {
            return Err(validation_error());
        }
        temporary.verify()?;
        safe_destination::require_no_sidecars(&temporary.path)?;
        let mut writer = Connection::open(&temporary.path, false)?;
        temporary.verify()?;
        // This writer alone uses our private, unpublished file. Exclusive
        // locking keeps a copied WAL header from allocating a disk SHM index.
        // Only an empty file uses MEMORY: bulk copy has no original pages to
        // journal, and a full disk cannot strand a disk rollback journal.
        if writer.single_text(c"PRAGMA main.locking_mode=EXCLUSIVE")? != "exclusive"
            || writer.single_text(c"PRAGMA main.page_count")? != "0"
            || writer.single_text(c"PRAGMA main.journal_mode=MEMORY")? != "memory"
        {
            return Err(validation_error());
        }
        let copied = backup_sqlite::online_copy(&source, &mut writer);
        // No explicit source read transaction is opened; finish releases backup's
        // source read locks, then close every source handle before destination work.
        let source_closed = source.close();
        copied?;
        source_closed?;
        // A copied WAL header can reopen the destination in WAL mode. Normalize
        // it under MEMORY first: switching directly to DELETE would journal the
        // header rewrite on disk and reintroduce retention on a full volume.
        if writer.single_text(c"PRAGMA main.journal_mode=MEMORY")? != "memory"
            || writer.single_text(c"PRAGMA main.journal_mode=DELETE")? != "delete"
        {
            return Err(validation_error());
        }
        writer.close()?;
        temporary.verify()?;
        let size = temporary.synchronize()?; // exact narrow handle closes here
        if size == 0 {
            return Err(validation_error());
        }
        safe_destination::require_no_sidecars(&temporary.path)?;
        header(&temporary.path)?;
        // One independent real read-only connection remains open for all checks.
        let mut reader = Connection::open(&temporary.path, true)?;
        if let Some(deadline) = check_deadline {
            reader.interrupt_at(deadline);
        }
        let validation = validate(&reader).map_err(|error| match error.code {
            ErrorCode::BackupCopyLimitExceeded => error,
            _ => validation_error(),
        });
        let reader_closed = reader.close();
        let foreign_keys = validation?;
        reader_closed?;
        safe_destination::require_no_sidecars(&temporary.path)?;
        temporary.verify()?;
        if foreign_keys.violation_count > 0
            && matches!(validation_policy, ValidationPolicy::PreMigrationStrict)
        {
            let mut error = AppError::new(
                ErrorCode::BackupForeignKeyViolation,
                "Foreign-key violations block a pre-migration backup. No backup was published.",
            );
            error.context.diagnostics = serde_json::to_value(&foreign_keys).ok();
            return Err(error);
        }
        Ok((size, foreign_keys, engine))
    })();
    let (size, foreign_keys, engine) = match written {
        Ok(result) => result,
        Err(mut error) => {
            error.context.cleanup_warnings.extend(temporary.cleanup());
            if let Err(sidecar) = safe_destination::require_no_sidecars(&temporary.path) {
                error.context.cleanup_warnings.push(sidecar);
            }
            attempt.completed_at = clock_timestamp(metadata_clock).unwrap_or(started_at);
            return BackupFileResult::Failure {
                error,
                attempt: Some(attempt),
            };
        }
    };
    // No writer/reader/sync handles survive this call; service rechecks immediately.
    let mut warnings = match destinations.publish(&destination, &mut temporary, true) {
        Ok(warnings) => warnings,
        Err(mut error) => {
            error.context.cleanup_warnings.extend(temporary.cleanup());
            attempt.completed_at = clock_timestamp(metadata_clock).unwrap_or(started_at);
            return BackupFileResult::Failure {
                error,
                attempt: Some(attempt),
            };
        }
    };
    // Publication is the commit boundary. Clock/history/cleanup failure cannot
    // become a retryable failure after this point.
    let completed_at = clock_timestamp(metadata_clock).unwrap_or_else(|error| {
        warnings.push(error);
        started_at.clone()
    });
    if foreign_keys.violation_count > 0 {
        let mut warning = AppError::new(
            ErrorCode::ForeignKeyViolationsPreserved,
            "The backup is physically valid. Existing foreign-key violations were preserved for recovery.",
        );
        warning.context.diagnostics = serde_json::to_value(&foreign_keys).ok();
        warnings.push(warning);
    }
    BackupFileResult::Success {
        file: BackupFile {
            destination_path: attempt.destination_path,
            file_size_bytes: size,
            started_at,
            completed_at,
            integrity: "ok",
            foreign_keys,
            validation_policy,
            destination_policy: destination_policy.name(),
            engine,
        },
        warnings,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualBackupRequest {
    pub destination_path: Option<String>,
    pub overwrite_token: Option<String>,
}
#[derive(Default)]
pub struct BackupRuntime {
    pub destinations: Arc<SafeDestinationService>,
    in_flight: Arc<AtomicBool>,
    /// Single-use staged restore confirmation; see `restore.rs`.
    pub(crate) restores: crate::restore::StagedRestores,
}
/// Backup and restore share one in-flight gate; the guard releases it on drop.
pub struct InFlight(Arc<AtomicBool>);
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl BackupRuntime {
    pub(crate) fn try_begin(&self) -> Option<InFlight> {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| InFlight(self.in_flight.clone()))
    }
}

/// Prompt 15 can manage this only after its trusted startup file classification.
/// No normal pool is opened solely for preservation or attempt metadata.
#[derive(Clone)]
pub struct ClassifiedStartupBackupSource {
    pub(crate) path: PathBuf,
    pub(crate) clock: Arc<dyn MetadataClock>,
    paths: database::DatabasePaths,
    boundary: Arc<Mutex<database::FileSet>>,
    wal: bool,
}
impl ClassifiedStartupBackupSource {
    pub fn from_classified(
        paths: &database::DatabasePaths,
        classified: &database::FileSet,
        clock: Arc<dyn MetadataClock>,
    ) -> AppResult<Self> {
        if classified.classification()? != database::FileClassification::ExistingDatabase {
            return Err(AppError::new(
                ErrorCode::BackupSourceInvalid,
                "Startup preservation requires a classified existing database.",
            ));
        }
        Ok(Self {
            path: paths.main.clone(),
            clock,
            paths: paths.clone(),
            boundary: Arc::new(Mutex::new(classified.clone())),
            wal: false,
        })
    }
    pub(crate) fn with_verified_wal(mut self, wal: bool) -> Self {
        self.wal = wal;
        self
    }
}

#[derive(Serialize, FromRow)]
pub struct BackupRun {
    pub id: String,
    pub backup_type: String,
    pub destination_path: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: String,
    pub file_size_bytes: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
}
#[derive(Serialize)]
pub struct BackupScreen {
    pub default_filename: String,
    pub last_manual_backup: Option<BackupRun>,
    pub history_available: bool,
}
pub async fn screen(
    pool: Option<&SqlitePool>,
    clock: &dyn MetadataClock,
) -> AppResult<(BackupScreen, Vec<AppError>)> {
    let now = clock_timestamp(clock)?;
    // YYYYMMDDHHMMSS, as restore and startup preservation names are built.
    let stamp: String = now.chars().filter(char::is_ascii_digit).take(14).collect();
    let mut warnings = Vec::new();
    let (last_manual_backup, history_available) = if let Some(pool) = pool {
        match last_manual_backup(pool).await {
            Ok(last) => (last, true),
            Err(_) => {
                warnings.push(metadata_warning());
                (None, false)
            }
        }
    } else {
        (None, false)
    };
    Ok((
        BackupScreen {
            default_filename: format!("shift-happens-backup-{stamp}.sqlite"),
            last_manual_backup,
            history_available,
        },
        warnings,
    ))
}
async fn last_manual_backup(pool: &SqlitePool) -> AppResult<Option<BackupRun>> {
    let mut last = None;
    // Stream the indexed type-filtered history and retain one parsed instant.
    // Raw RFC 3339 text ordering is deliberately not used.
    let mut rows = sqlx::query_as::<_, BackupRun>("SELECT id, backup_type, destination_path, status, started_at, completed_at, file_size_bytes, error_message, created_at FROM backup_runs WHERE backup_type = 'manual'").fetch(pool);
    use futures_util::TryStreamExt;
    while let Some(row) = rows.try_next().await? {
        let instant = parse_metadata_instant(&row.completed_at)?;
        if last
            .as_ref()
            .is_none_or(|(old_instant, old): &(time::OffsetDateTime, BackupRun)| {
                (instant, &row.id) > (*old_instant, &old.id)
            })
        {
            last = Some((instant, row));
        }
    }
    Ok(last.map(|(_, row)| row))
}
fn metadata_warning() -> AppError {
    AppError::new(
        ErrorCode::BackupMetadataUnavailable,
        "The file result is unchanged, but backup history could not be recorded or read.",
    )
}
async fn record(
    pool: &SqlitePool,
    attempt: &BackupAttempt,
    success: Option<u64>,
    error: Option<&AppError>,
    backup_type: &str,
) -> AppResult<()> {
    let error_message = error.map(|error| {
        let code = serde_json::to_value(error.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "backup_copy_failed".to_owned());
        format!("{code}: {}", error.message)
    });
    sqlx::query("INSERT INTO backup_runs (id, backup_type, destination_path, status, started_at, completed_at, file_size_bytes, error_message, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string()).bind(backup_type).bind(&attempt.destination_path)
        .bind(if success.is_some() { "success" } else { "failed" })
        .bind(&attempt.started_at).bind(&attempt.completed_at)
        .bind(success.map(i64::try_from).transpose().map_err(|_| metadata_warning())?)
        .bind(error_message).bind(&attempt.completed_at).execute(pool).await.map_err(|_| metadata_warning())?;
    Ok(())
}

pub(crate) async fn record_pre_migration(pool: &SqlitePool, file: &BackupFile) -> AppResult<()> {
    record(
        pool,
        &BackupAttempt {
            destination_path: file.destination_path.clone(),
            started_at: file.started_at.clone(),
            completed_at: file.completed_at.clone(),
        },
        Some(file.file_size_bytes),
        None,
        "pre_migration",
    )
    .await
}

/// Real command orchestration, reusable with a trusted startup source and no pool.
pub async fn manual_backup(
    source: PathBuf,
    pool: Option<&SqlitePool>,
    clock: Arc<dyn MetadataClock>,
    runtime: &BackupRuntime,
    request: ManualBackupRequest,
) -> CommandResult<BackupFile> {
    manual_backup_from(source, pool, clock, runtime, request, None).await
}

pub(crate) async fn preservation_backup(
    source: &ClassifiedStartupBackupSource,
    runtime: &BackupRuntime,
    request: ManualBackupRequest,
) -> CommandResult<BackupFile> {
    manual_backup_from(
        source.path.clone(),
        None,
        source.clock.clone(),
        runtime,
        request,
        Some(source.clone()),
    )
    .await
}

async fn manual_backup_from(
    source: PathBuf,
    pool: Option<&SqlitePool>,
    clock: Arc<dyn MetadataClock>,
    runtime: &BackupRuntime,
    request: ManualBackupRequest,
    classified: Option<ClassifiedStartupBackupSource>,
) -> CommandResult<BackupFile> {
    let Some(destination_path) = request.destination_path else {
        return CommandResult::Canceled;
    };
    let Some(guard) = runtime.try_begin() else {
        return CommandResult::Blocked {
            error: AppError::new(
                ErrorCode::BackupInProgress,
                "A backup is already in progress. Wait for its result.",
            ),
        };
    };
    // Runtime gate confirms both bindings identify the same loaded engine.
    let sqlx_engine = match database::loaded_sqlite_engine().await {
        Ok(engine) => engine,
        Err(error) => return CommandResult::Failure { error },
    };
    let destinations = runtime.destinations.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard; // survives cancellation of the awaiting command
        match backup_sqlite::loaded_engine() {
            Ok(engine)
                if engine.version == sqlx_engine.version
                    && engine.source_id == sqlx_engine.source_id => {}
            _ => {
                return BackupFileResult::Failure {
                    error: AppError::new(
                        ErrorCode::UnsafeSqliteEngine,
                        "The backup and application SQLite runtimes do not match.",
                    ),
                    attempt: None,
                };
            }
        }
        let boundary = if let Some(classified) = &classified {
            let observed = classified
                .boundary
                .lock()
                .map_err(|_| {
                    AppError::new(
                        ErrorCode::BackupSourceInvalid,
                        "The startup source is unavailable.",
                    )
                })
                .and_then(|files| {
                    files.require_unchanged(&classified.paths)?;
                    Ok(files.clone())
                });
            match observed {
                Ok(files) => Some(files),
                Err(error) => {
                    return BackupFileResult::Failure {
                        error,
                        attempt: None,
                    };
                }
            }
        } else {
            None
        };
        let result = create_consistent_sqlite_backup(
            &source,
            Path::new(&destination_path),
            request.overwrite_token.as_deref(),
            DestinationPolicy::Manual,
            ValidationPolicy::ManualPreserve,
            None,
            clock.as_ref(),
            &destinations,
        );
        if let (Some(classified), Some(boundary)) = (&classified, boundary) {
            // Accept only housekeeping for the inspected journal mode.
            // Never bless changed main contents or WAL frames for the next click. A
            // published backup remains success even if the source then drifts.
            if let Ok(next) = boundary.after_reader(&classified.paths, classified.wal)
                && let Ok(mut stored) = classified.boundary.lock()
            {
                *stored = next;
            }
        }
        result
    })
    .await;
    match result {
        Ok(BackupFileResult::Success { file, mut warnings }) => {
            if let Some(pool) = pool {
                let attempt = BackupAttempt {
                    destination_path: file.destination_path.clone(),
                    started_at: file.started_at.clone(),
                    completed_at: file.completed_at.clone(),
                };
                if let Err(warning) =
                    record(pool, &attempt, Some(file.file_size_bytes), None, "manual").await
                {
                    warnings.push(warning);
                }
            }
            CommandResult::Success {
                data: file,
                warnings,
            }
        }
        Ok(BackupFileResult::RequiresOverwrite {
            destination_path,
            overwrite_token,
        }) => CommandResult::RequiresOverwrite {
            destination_path,
            overwrite_token,
        },
        Ok(BackupFileResult::Failure { mut error, attempt }) => {
            if let (Some(pool), Some(attempt)) = (pool, attempt)
                && let Err(warning) = record(pool, &attempt, None, Some(&error), "manual").await
            {
                error.context.cleanup_warnings.push(warning);
            }
            CommandResult::Failure { error }
        }
        Err(_) => CommandResult::Failure {
            error: AppError::new(
                ErrorCode::BackupCopyFailed,
                "The backup worker stopped unexpectedly. Inspect the chosen destination before retrying.",
            ),
        },
    }
}
