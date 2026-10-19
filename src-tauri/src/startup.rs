//! The sole production startup decision. Inspection has no pool, migrator or
//! journal-mode setter; a pending upgrade cannot reach a writer before backup.
use crate::{
    backup::{self, BackupFile, BackupFileResult, ClassifiedStartupBackupSource, ValidationPolicy},
    database::{self, AppState, DatabasePaths, FileClassification, FileSet, MIGRATOR},
    error::{AppError, AppResult, ErrorCode},
    metadata::{MetadataClock, clock_timestamp},
    safe_destination::{DestinationPolicy, SafeDestinationService},
};
use serde::Serialize;
use sqlx::{Connection, SqliteConnection};
use std::sync::Arc;
use uuid::Uuid;

// Declaration order is the deterministic primary blocking precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    DatabaseFileSetInconsistent,
    DatabaseAccessError,
    DatabaseIntegrityError,
    FailedOrDirtyMigration,
    ChecksumMismatch,
    NewerOrUnknownMigration,
    MigrationMetadataMissing,
    NewDatabase,
    UninitializedEmptyDatabase,
    Current,
    Pending,
}
impl Classification {
    fn permits_startup(self) -> bool {
        matches!(
            self,
            Self::NewDatabase | Self::UninitializedEmptyDatabase | Self::Current | Self::Pending
        )
    }
}

#[derive(Clone, Serialize)]
pub struct BackupStatus {
    pub status: &'static str,
    pub path: Option<String>,
    pub error: Option<AppError>,
}
#[derive(Clone, Serialize)]
pub struct StartupReport {
    pub version: &'static str,
    pub database_path: Option<String>,
    pub classification: Classification,
    pub reasons: Vec<Classification>,
    pub ready: bool,
    pub error: Option<AppError>,
    pub pre_migration_backup: BackupStatus,
    pub preservation_available: bool,
    pub preservation_unavailable_reason: Option<&'static str>,
    pub preservation_filename: Option<String>,
    pub warnings: Vec<AppError>,
    /// Outcome of a restore handled by this startup, if one was pending.
    pub restore: Option<crate::restore::RestoreReport>,
}
impl StartupReport {
    fn new(paths: Option<&DatabasePaths>) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            database_path: paths.map(|p| crate::safe_destination::display_path(&p.main)),
            classification: Classification::DatabaseAccessError,
            reasons: vec![],
            ready: false,
            error: None,
            pre_migration_backup: BackupStatus {
                status: "not_attempted",
                path: None,
                error: None,
            },
            preservation_available: false,
            preservation_unavailable_reason: Some(
                "The source has not passed safe read-only inspection.",
            ),
            preservation_filename: None,
            warnings: vec![],
            restore: None,
        }
    }
}
pub struct StartupContext {
    pub report: StartupReport,
    pub source: Option<ClassifiedStartupBackupSource>,
    /// Resolved paths, kept so blocked startup can still stage a restore.
    pub paths: Option<DatabasePaths>,
}
impl StartupContext {
    pub fn unresolved(error: AppError) -> Self {
        let mut report = StartupReport::new(None);
        report.reasons.push(Classification::DatabaseAccessError);
        report.error = Some(error);
        Self {
            report,
            source: None,
            paths: None,
        }
    }
}
pub struct StartupOutcome {
    pub state: Option<AppState>,
    pub context: StartupContext,
}

fn access_error() -> AppError {
    AppError::new(
        ErrorCode::DatabaseUnavailable,
        "The database could not be safely inspected. Close the app and preserve its files for developer inspection.",
    )
}
fn integrity_error() -> AppError {
    AppError::new(
        ErrorCode::DatabaseIntegrityError,
        "SQLite integrity or foreign-key checks failed. Startup remains blocked.",
    )
}
fn classified_error() -> AppError {
    AppError::new(
        ErrorCode::MigrationInvalid,
        "The database requires developer inspection. No upgrade or journal-mode change was attempted.",
    )
}
fn reason_for(error: &AppError) -> Classification {
    match error.code {
        ErrorCode::StartupClassificationChanged => Classification::DatabaseFileSetInconsistent,
        ErrorCode::DatabaseIntegrityError => Classification::DatabaseIntegrityError,
        _ => Classification::DatabaseAccessError,
    }
}

async fn observe(paths: &DatabasePaths) -> AppResult<FileSet> {
    let paths = paths.clone();
    tauri::async_runtime::spawn_blocking(move || database::inspect_database_file_set(&paths))
        .await
        .map_err(|_| access_error())?
}
async fn recheck(paths: &DatabasePaths, files: &FileSet) -> AppResult<()> {
    let paths = paths.clone();
    let files = files.clone();
    tauri::async_runtime::spawn_blocking(move || files.require_unchanged(&paths))
        .await
        .map_err(|_| access_error())?
}
async fn after_reader(paths: &DatabasePaths, files: &FileSet, wal: bool) -> AppResult<FileSet> {
    let paths = paths.clone();
    let files = files.clone();
    tauri::async_runtime::spawn_blocking(move || files.after_reader(&paths, wal))
        .await
        .map_err(|_| access_error())?
}
async fn after_writer(paths: &DatabasePaths, files: &FileSet) -> AppResult<FileSet> {
    let paths = paths.clone();
    let files = files.clone();
    tauri::async_runtime::spawn_blocking(move || files.after_writer(&paths))
        .await
        .map_err(|_| access_error())?
}

async fn health(connection: &mut SqliteConnection, full: bool) -> AppResult<bool> {
    let integrity: Vec<String> = sqlx::query_scalar(if full {
        "PRAGMA integrity_check"
    } else {
        "PRAGMA quick_check"
    })
    .fetch_all(&mut *connection)
    .await
    .map_err(database::sqlite_inspection_error)?;
    let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(&mut *connection)
        .await
        .map_err(database::sqlite_inspection_error)?;
    if integrity != ["ok"] {
        return Err(integrity_error());
    }
    Ok(foreign_keys.is_none())
}

pub(crate) struct Inspection {
    pub(crate) classification: Classification,
    pub(crate) reasons: Vec<Classification>,
    pub(crate) highest_applied: Option<i64>,
    pub(crate) physically_readable: bool,
    pub(crate) wal: bool,
}

/// One real read-only, create-disabled, normal-locking connection. All checks
/// share a read transaction, including the current committed WAL-backed view.
/// Restore staging reuses it on its own validated copy.
pub(crate) async fn inspect_existing(paths: &DatabasePaths) -> AppResult<Inspection> {
    let mut connection = database::open_main(paths, true).await?;
    let result = async {
        database::inspect_engine(&mut connection).await?;
        let wal = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&mut connection)
            .await
            .map_err(|_| access_error())?
            == "wal";
        let mut tx = connection.begin().await.map_err(|_| access_error())?;
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='_sqlx_migrations'",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| access_error())?;
        let mut reasons = vec![];
        let mut physically_readable = true;
        match health(&mut tx, false).await {
            Ok(true) => {}
            Ok(false) => reasons.push(Classification::DatabaseIntegrityError),
            Err(error) => {
                physically_readable = false;
                reasons.push(reason_for(&error));
            }
        }
        let embedded: Vec<_> = MIGRATOR.iter().collect();
        let mut versions = vec![];
        // An unreadable/corrupt health result provides no safe basis for more
        // metadata inspection. Foreign-key-only failures remain readable.
        if physically_readable && exists == 0 {
            reasons.push(Classification::MigrationMetadataMissing);
        } else if physically_readable {
            match database::read_migration_history(&mut tx).await {
                Ok(rows) => {
                    if rows.len() > database::MIGRATION_HISTORY_LIMIT {
                        reasons.push(Classification::FailedOrDirtyMigration);
                    }
                    for row in rows {
                        let Some(database::AppliedMigration {
                            version,
                            succeeded,
                            checksum,
                        }) = row
                        else {
                            reasons.push(Classification::FailedOrDirtyMigration);
                            continue;
                        };
                        if !succeeded || versions.last() == Some(&version) {
                            reasons.push(Classification::FailedOrDirtyMigration);
                        }
                        versions.push(version);
                        match embedded.iter().find(|m| m.version == version) {
                            Some(m) if m.checksum.as_ref() != checksum.as_slice() => {
                                reasons.push(Classification::ChecksumMismatch)
                            }
                            Some(_) => {}
                            None => reasons.push(Classification::NewerOrUnknownMigration),
                        }
                    }
                    let known: Vec<i64> = versions
                        .iter()
                        .copied()
                        .filter(|v| embedded.iter().any(|m| m.version == *v))
                        .collect();
                    if !known
                        .iter()
                        .zip(embedded.iter())
                        .all(|(v, m)| *v == m.version)
                        || known.len() > embedded.len()
                    {
                        reasons.push(Classification::FailedOrDirtyMigration);
                    }
                }
                Err(error) => {
                    // A malformed metadata table is dirty history. Locking or
                    // physical read errors must retain their own classification.
                    let malformed = matches!(&error, sqlx::Error::Database(error)
                        if error.code().and_then(|code| code.parse::<i32>().ok())
                            .is_some_and(|code| code & 0xff == 1));
                    if malformed {
                        reasons.push(Classification::FailedOrDirtyMigration);
                    } else {
                        return Err(database::sqlite_inspection_error(error));
                    }
                }
            }
        }
        reasons.sort();
        reasons.dedup();
        let classification =
            reasons
                .first()
                .copied()
                .unwrap_or(if versions.len() == embedded.len() {
                    Classification::Current
                } else {
                    Classification::Pending
                });
        // Routine current startup uses quick_check. A pending schema requires
        // the full check before even attempting the preservation primitive.
        if classification == Classification::Pending {
            match health(&mut tx, true).await {
                Ok(true) => {}
                Ok(false) => reasons.push(Classification::DatabaseIntegrityError),
                Err(error) => {
                    physically_readable = false;
                    reasons.push(reason_for(&error));
                }
            }
        }
        tx.rollback().await.map_err(|_| access_error())?;
        reasons.sort();
        reasons.dedup();
        Ok(Inspection {
            classification: reasons.first().copied().unwrap_or(classification),
            reasons,
            highest_applied: versions.last().copied(),
            physically_readable,
            wal,
        })
    }
    .await;
    let closed = connection.close().await.map_err(|_| access_error());
    closed?;
    result
}

async fn post_migration_health(paths: &DatabasePaths) -> AppResult<bool> {
    let mut connection = database::open_main(paths, true).await?;
    let result = async {
        let wal = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&mut connection)
            .await?
            == "wal";
        let mut tx = connection.begin().await?;
        let healthy = health(&mut tx, true).await?;
        tx.rollback().await?;
        if !healthy {
            let mut error = integrity_error();
            error.context.cause = Some(ErrorCode::ForeignKeyViolationsPreserved);
            return Err(error);
        }
        Ok(wal)
    }
    .await;
    connection.close().await.map_err(|_| access_error())?;
    result
}

#[derive(Default)]
struct Progress {
    files: Option<FileSet>,
    physically_readable: bool,
    wal: bool,
    stage: &'static str,
}

async fn initialize(
    paths: &DatabasePaths,
    clock: Arc<dyn MetadataClock>,
    report: &mut StartupReport,
    progress: &mut Progress,
) -> AppResult<AppState> {
    progress.stage = "owned_directories";
    let owned = paths.clone();
    tauri::async_runtime::spawn_blocking(move || database::create_owned_directories(&owned))
        .await
        .map_err(|_| access_error())??;
    // A confirmed restore is installed before the live file set is observed.
    // While its marker remains, an error here blocks startup.
    progress.stage = "restore";
    let owned = paths.clone();
    let restore_clock = clock.clone();
    let (restore, restore_warnings) = tauri::async_runtime::spawn_blocking(move || {
        crate::restore::apply_pending_restore(&owned, restore_clock.as_ref())
    })
    .await
    .map_err(|_| access_error())??;
    report.restore = restore;
    report.warnings.extend(restore_warnings);
    progress.stage = "file_classification";
    let mut files = observe(paths).await?;
    let physical = files.classification()?;
    report.classification = match physical {
        FileClassification::NewDatabase => Classification::NewDatabase,
        FileClassification::UninitializedEmptyDatabase => {
            Classification::UninitializedEmptyDatabase
        }
        FileClassification::ExistingDatabase => Classification::DatabaseAccessError,
    };
    progress.files = Some(files.clone());
    database::validate_embedded_migrations()?;
    database::loaded_sqlite_engine().await?;
    let mut pending_backup: Option<BackupFile> = None;
    match physical {
        FileClassification::NewDatabase => {
            progress.stage = "claim_new_database";
            let owned = paths.clone();
            let classified = files.clone();
            files = tauri::async_runtime::spawn_blocking(move || {
                database::claim_new_database(&owned, &classified)
            })
            .await
            .map_err(|_| access_error())??;
            progress.files = Some(files.clone());
        }
        FileClassification::UninitializedEmptyDatabase => {}
        FileClassification::ExistingDatabase => {
            progress.stage = "read_only_inspection";
            recheck(paths, &files).await?;
            let inspection = inspect_existing(paths).await?;
            files = after_reader(paths, &files, inspection.wal).await?;
            progress.files = Some(files.clone());
            progress.physically_readable = inspection.physically_readable;
            progress.wal = inspection.wal;
            report.classification = inspection.classification;
            report.reasons = inspection.reasons;
            if !report.classification.permits_startup() {
                let mut error = if report.classification == Classification::DatabaseIntegrityError {
                    integrity_error()
                } else {
                    classified_error()
                };
                if report.classification == Classification::DatabaseIntegrityError
                    && progress.physically_readable
                {
                    error.context.cause = Some(ErrorCode::ForeignKeyViolationsPreserved);
                }
                return Err(error);
            }
            if report.classification == Classification::Pending {
                progress.stage = "pre_migration_backup";
                recheck(paths, &files).await?;
                let stamp = clock_timestamp(clock.as_ref())?;
                let stamp = stamp.get(..19).ok_or_else(access_error)?.replace(':', "");
                let from = inspection
                    .highest_applied
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "none".into());
                let to = MIGRATOR.iter().last().ok_or_else(classified_error)?.version;
                let destination = paths.migration_backups().join(format!(
                    "shift-happens-pre-migration-from-{from}-to-{to}-{stamp}-{}.sqlite",
                    Uuid::new_v4()
                ));
                report.pre_migration_backup.path =
                    Some(crate::safe_destination::display_path(&destination));
                report.pre_migration_backup.status = "failed";
                let source = paths.main.clone();
                let directory = paths.migration_backups();
                let backup_clock = clock.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    backup::create_consistent_sqlite_backup(
                        &source,
                        &destination,
                        None,
                        DestinationPolicy::AppOwnedNoOverwrite { directory },
                        ValidationPolicy::PreMigrationStrict,
                        backup_clock.as_ref(),
                        &SafeDestinationService::default(),
                    )
                })
                .await
                .map_err(|_| access_error())?;
                // A published backup remains authoritative if the source drifts.
                let backup_error = match result {
                    BackupFileResult::Success { file, warnings } => {
                        report.pre_migration_backup.status = "success";
                        report.warnings.extend(warnings);
                        pending_backup = Some(file);
                        None
                    }
                    BackupFileResult::Failure { error, .. } => {
                        report.pre_migration_backup.error = Some(error.clone());
                        Some(error)
                    }
                    BackupFileResult::RequiresOverwrite { .. } => Some(classified_error()),
                };
                // Release/verify even on failure. WAL shared-memory participation
                // is SQLite housekeeping; changed main/WAL/journal still blocks.
                files = after_reader(paths, &files, progress.wal).await?;
                progress.files = Some(files.clone());
                if let Some(error) = backup_error {
                    return Err(error);
                }
            }
        }
    }

    if report.classification != Classification::Current {
        progress.stage = "migrate";
        recheck(paths, &files).await?;
        database::apply_embedded_migrations(paths).await?;
        files = after_writer(paths, &files).await?;
        progress.files = Some(files.clone());
        progress.stage = "post_migration_health";
        recheck(paths, &files).await?;
        progress.wal = post_migration_health(paths).await?;
        files = after_reader(paths, &files, progress.wal).await?;
        progress.files = Some(files.clone());
        progress.physically_readable = true;
        if let Some(restore) = &mut report.restore
            && restore.status == "applied"
        {
            restore.migrated = report.classification == Classification::Pending;
        }
    }
    progress.stage = "initialize_wal";
    recheck(paths, &files).await?;
    database::initialize_startup_wal(paths).await?;
    files = after_writer(paths, &files).await?;
    progress.files = Some(files.clone());
    progress.wal = true;
    progress.stage = "open_pool";
    recheck(paths, &files).await?;
    let pool = database::open_normal_pool(paths).await?;
    let initialized = async {
        files = after_writer(paths, &files).await?;
        progress.files = Some(files.clone());
        progress.stage = "post_schema_initialization";
        recheck(paths, &files).await?;
        database::run_post_schema_initialization(&pool, clock.as_ref()).await?;
        files = after_writer(paths, &files).await?;
        progress.files = Some(files.clone());
        // History never authorizes a migration. Publish success is authoritative;
        // optional metadata is recorded only after all required initialization.
        if let Some(file) = &pending_backup {
            progress.stage = "backup_metadata";
            recheck(paths, &files).await?;
            if let Err(warning) = backup::record_pre_migration(&pool, file).await {
                report.warnings.push(warning);
            }
            files = after_writer(paths, &files).await?;
            progress.files = Some(files.clone());
        }
        recheck(paths, &files).await?;
        Ok::<_, AppError>(())
    }
    .await;
    if let Err(error) = initialized {
        pool.close().await;
        return Err(error);
    }
    report.ready = true;
    report.preservation_unavailable_reason = Some("Use Backup in the running application.");
    Ok(AppState {
        paths: paths.clone(),
        pool,
        clock,
    })
}

pub async fn start_database(paths: DatabasePaths, clock: Arc<dyn MetadataClock>) -> StartupOutcome {
    let mut report = StartupReport::new(Some(&paths));
    let mut progress = Progress::default();
    let result = initialize(&paths, clock.clone(), &mut report, &mut progress).await;
    let mut source = None;
    let state = match result {
        Ok(state) => Some(state),
        Err(mut error) => {
            if error.context.stage.is_none() {
                error.context.stage = Some(progress.stage);
            }
            let reason = reason_for(&error);
            if matches!(
                error.code,
                ErrorCode::StartupClassificationChanged | ErrorCode::DatabaseIntegrityError
            ) || error.code.is_database_operation_failure()
                || report.classification == Classification::DatabaseAccessError
            {
                report.reasons.push(reason);
                if matches!(error.code, ErrorCode::DatabaseIntegrityError)
                    && !matches!(
                        error.context.cause,
                        Some(ErrorCode::ForeignKeyViolationsPreserved)
                    )
                {
                    progress.physically_readable = false;
                }
            }
            if progress.stage == "migrate" {
                report.reasons.push(Classification::FailedOrDirtyMigration);
            }
            report.reasons.sort();
            report.reasons.dedup();
            if let Some(reason) = report.reasons.first() {
                report.classification = *reason;
            }
            if report.pre_migration_backup.status == "failed"
                && report.pre_migration_backup.error.is_none()
            {
                report.pre_migration_backup.error = Some(error.clone());
            }
            // No normal state/pool is constructed to offer preservation. Retain
            // only a trusted path and its verified identity; every click rechecks.
            if progress.physically_readable
                && !matches!(error.code, ErrorCode::StartupClassificationChanged)
                && let Some(files) = &progress.files
            {
                let observed = if matches!(
                    progress.stage,
                    "migrate"
                        | "initialize_wal"
                        | "open_pool"
                        | "post_schema_initialization"
                        | "backup_metadata"
                ) {
                    after_writer(&paths, files).await
                } else {
                    recheck(&paths, files).await.map(|()| files.clone())
                };
                if let Ok(files) = observed {
                    source = ClassifiedStartupBackupSource::from_classified(
                        &paths,
                        &files,
                        clock.clone(),
                    )
                    .map(|source| source.with_verified_wal(progress.wal))
                    .ok();
                }
            }
            if source.is_some() {
                report.preservation_available = true;
                report.preservation_unavailable_reason = None;
                if let Ok(stamp) = clock_timestamp(clock.as_ref()) {
                    let digits: String = stamp
                        .chars()
                        .filter(char::is_ascii_digit)
                        .take(14)
                        .collect();
                    report.preservation_filename =
                        Some(format!("shift-happens-preservation-{digits}.sqlite"));
                }
            }
            report.error = Some(error);
            None
        }
    };
    StartupOutcome {
        state,
        context: StartupContext {
            report,
            source,
            paths: Some(paths),
        },
    }
}
