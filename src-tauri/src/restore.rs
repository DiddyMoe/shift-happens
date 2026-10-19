//! Whole-database restore from a backup file, in three phases.
//!
//! 1. `stage` copies the chosen file into `restore-staging` with the strict
//!    backup primitive (header, online copy, DELETE mode, full integrity, zero
//!    foreign-key violations), classifies the copy and returns a preview.
//! 2. `apply` (after confirmation) saves a safety backup of the live data, records
//!    `restore-pending.json` and asks the app to restart.
//! 3. `apply_pending_restore` runs at the next startup before the live file set is
//!    observed. It moves the live main file and sidecars aside and installs the
//!    staged copy; normal classification, pre-migration backup and migration follow.
//!
//! While a marker exists, startup never falls through to creating a new database.
use crate::{
    backup::{self, BackupFileResult, BackupRuntime, InFlight, ValidationPolicy},
    database::{self, DatabasePaths, MIGRATOR},
    error::{AppError, AppResult, CommandResult, ErrorCode},
    metadata::{MetadataClock, clock_timestamp},
    safe_destination::{self, DestinationPolicy, FileIdentity, display_path, is_reparse},
    startup::{self, Classification},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqlitePool};
use std::{
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

const MARKER_VERSION: u32 = 1;
const MARKER_LIMIT: u64 = 64 * 1024;
/// Windows opens SQLite files without delete sharing, and a closing process or a
/// scanner can briefly hold them. Moves retry for a bounded time.
const MOVE_RETRY_LIMIT: Duration = Duration::from_secs(10);
const MOVE_RETRY_INTERVAL: Duration = Duration::from_millis(200);
const SIDECAR_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];

fn source_error(message: &'static str) -> AppError {
    AppError::new(ErrorCode::RestoreSourceInvalid, message)
}
fn unreadable_source() -> AppError {
    source_error(
        "The backup file could not be read. It may be damaged or not a Shift Happens database.",
    )
}
fn not_shift_happens() -> AppError {
    source_error("This file is not a Shift Happens database.")
}
fn damaged_source() -> AppError {
    source_error("This backup failed its integrity check and cannot be restored.")
}
fn restore_failed(message: &'static str) -> AppError {
    AppError::new(ErrorCode::RestoreFailed, message)
}
fn pending_error() -> AppError {
    restore_failed(
        "A restore is already waiting to be applied. Close all Shift Happens windows and open it again.",
    )
}
fn startup_blocked() -> AppError {
    restore_failed(
        "The restore could not be completed. Close all Shift Happens windows and open it again.",
    )
}
fn startup_conflict(replaced: &Path) -> AppError {
    let mut error = restore_failed(
        "The restore could not be completed because the database files changed. Your previous data is kept in the folder shown.",
    );
    error.context.recovery_path = Some(display_path(replaced));
    error
}

/// One staged copy at a time. A newer stage or a successful apply consumes it.
struct StagedRestore {
    id: String,
    path: PathBuf,
    length: u64,
    sha256: String,
    source_name: String,
}
#[derive(Default)]
pub struct StagedRestores(Mutex<Option<StagedRestore>>);
impl StagedRestores {
    fn replace(&self, next: Option<StagedRestore>) {
        let previous = match self.0.lock() {
            Ok(mut slot) => std::mem::replace(&mut *slot, next),
            Err(poisoned) => std::mem::replace(&mut *poisoned.into_inner(), next),
        };
        if let Some(previous) = previous {
            remove_quietly(&previous.path);
        }
    }
    fn take(&self, id: &str) -> Option<StagedRestore> {
        let mut slot = self.0.lock().ok()?;
        if slot.as_ref().is_some_and(|staged| staged.id == id) {
            slot.take()
        } else {
            None
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageRestoreRequest {
    pub source_path: Option<String>,
}
#[derive(Serialize)]
pub struct RestorePreview {
    pub staged_id: String,
    pub file_name: String,
    pub source_path: String,
    pub file_size_bytes: u64,
    /// Highest migration applied in the backup.
    pub schema_version: i64,
    pub current_schema_version: i64,
    pub will_migrate: bool,
    pub employee_count: Option<i64>,
    pub schedule_count: Option<i64>,
    pub latest_schedule_end_date: Option<String>,
    /// True when the running app saves a copy of the current data first. During
    /// blocked startup the current files are moved aside instead.
    pub safety_backup: bool,
    pub restore_backups_path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyRestoreRequest {
    pub staged_id: String,
}
#[derive(Serialize)]
pub struct RestoreScheduled {
    pub safety_backup: Option<String>,
    pub replaced_files_folder: String,
}
/// Reported once by the startup that handled a pending restore.
#[derive(Clone, Serialize)]
pub struct RestoreReport {
    /// `applied` or `abandoned`.
    pub status: &'static str,
    pub source_name: Option<String>,
    pub safety_backup: Option<String>,
    pub replaced_dir: Option<String>,
    pub migrated: bool,
    pub completed_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct PendingRestore {
    version: u32,
    staged_path: String,
    staged_sha256: String,
    staged_length: u64,
    source_name: String,
    safety_backup: Option<String>,
    replaced_dir: Option<String>,
    created_at: String,
}

fn hex(bytes: impl IntoIterator<Item = u8>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 15)] as char);
    }
    encoded
}
fn stamp(clock: &dyn MetadataClock) -> AppResult<String> {
    Ok(clock_timestamp(clock)?
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect())
}
fn regular_file(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_reparse(&metadata) => Ok(Some(metadata)),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
/// Length and SHA-256 of a regular file, read through a no-follow handle.
fn file_digest(path: &Path) -> io::Result<(u64, String)> {
    if regular_file(path)?.is_none() {
        return Err(io::ErrorKind::NotFound.into());
    }
    let mut file = safe_destination::open_narrow(path, false)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    let mut length = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        length += read as u64;
    }
    Ok((length, hex(hasher.finalize())))
}
fn matches_digest(path: &Path, length: u64, sha256: &str) -> bool {
    file_digest(path)
        .is_ok_and(|(actual_length, actual)| actual_length == length && actual == sha256)
        && safe_destination::require_no_sidecars(path).is_ok()
}
fn remove_quietly(path: &Path) {
    if regular_file(path).is_ok_and(|file| file.is_some()) {
        let _ = fs::remove_file(path);
    }
}
fn directory_identity(path: &Path) -> io::Result<FileIdentity> {
    let path = fs::canonicalize(path)?;
    let handle = safe_destination::open_narrow(&path, false)?;
    if !handle.metadata()?.is_dir() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    safe_destination::handle_identity(&handle)
}
fn live_files(paths: &DatabasePaths) -> AppResult<Vec<(PathBuf, String)>> {
    let name = paths
        .main
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(startup_blocked)?;
    let mut files = vec![(paths.main.clone(), name.to_owned())];
    for suffix in SIDECAR_SUFFIXES {
        let mut path = paths.main.as_os_str().to_os_string();
        path.push(suffix);
        files.push((PathBuf::from(path), format!("{name}{suffix}")));
    }
    Ok(files)
}

/// No-replace by default. Windows uses a same-volume write-through move.
fn move_file(from: &Path, to: &Path, replace: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        safe_destination::native_publish(from, to, replace)
    }
    #[cfg(not(windows))]
    {
        if !replace && fs::symlink_metadata(to).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        fs::rename(from, to)
    }
}
fn move_with_retry(from: &Path, to: &Path, replace: bool) -> io::Result<()> {
    with_retry(|| move_file(from, to, replace))
}
/// Retries a move or removal for a bounded time. A missing source or an existing
/// target is final rather than transient.
fn with_retry(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let started = Instant::now();
    loop {
        match operation() {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::AlreadyExists
                ) || started.elapsed() >= MOVE_RETRY_LIMIT =>
            {
                return Err(error);
            }
            Err(_) => std::thread::sleep(MOVE_RETRY_INTERVAL),
        }
    }
}

/// Temp file plus native move. `replace` is false only for the first marker.
fn write_marker(paths: &DatabasePaths, marker: &PendingRestore, replace: bool) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(marker).map_err(io::Error::other)?;
    let temporary = paths
        .directory
        .join(format!(".restore-pending-{}.tmp", Uuid::new_v4()));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        move_with_retry(&temporary, &paths.restore_marker(), replace)
    })();
    if written.is_err() {
        remove_quietly(&temporary);
    }
    written
}
fn read_marker(path: &Path) -> Option<PendingRestore> {
    let file = safe_destination::open_narrow(path, false).ok()?;
    let mut bytes = Vec::new();
    file.take(MARKER_LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MARKER_LIMIT {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
fn marker_exists(paths: &DatabasePaths) -> bool {
    // Fail closed: anything other than a clean "not found" counts as pending.
    !matches!(fs::symlink_metadata(paths.restore_marker()), Err(error) if error.kind() == io::ErrorKind::NotFound)
}

/// Removes abandoned staged copies and temporaries from app-owned folders.
fn cleanup_staging(paths: &DatabasePaths) {
    let owned = |directory: &Path, keep: &dyn Fn(&str) -> bool| {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name) = name.to_str()
                && keep(name)
            {
                remove_quietly(&entry.path());
            }
        }
    };
    owned(&paths.restore_staging(), &|name| {
        (name.starts_with("staged-") && name.ends_with(".sqlite"))
            || (name.starts_with(".shift-happens-") && name.ends_with(".tmp"))
    });
    owned(&paths.directory, &|name| {
        name.starts_with(".restore-pending-") && name.ends_with(".tmp")
    });
}

// ---------- Phase A: stage and preview ----------

/// Absolute regular file, not a link, not the live database or its sidecars,
/// and not an already staged copy. Returns the source length.
fn check_source(paths: &DatabasePaths, source: &Path) -> AppResult<u64> {
    let text = source.to_str().ok_or_else(unreadable_source)?;
    if !source.is_absolute()
        || text.starts_with(r"\\?\")
        || text.starts_with(r"\\.\")
        || source
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| name.contains(':'))
    {
        return Err(unreadable_source());
    }
    let metadata = regular_file(source)
        .map_err(|_| unreadable_source())?
        .ok_or_else(unreadable_source)?;
    let identity =
        safe_destination::require_regular_file(source).map_err(|_| unreadable_source())?;
    for (live, _) in live_files(paths)? {
        match fs::symlink_metadata(&live) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            // Fail closed: an uninspectable live file could also never be moved aside.
            _ => match safe_destination::require_regular_file(&live) {
                Ok(id) if id != identity => {}
                Ok(_) => {
                    return Err(source_error(
                        "This is the database Shift Happens is using now. Choose a backup file instead.",
                    ));
                }
                Err(_) => {
                    return Err(restore_failed(
                        "The current database files cannot be inspected, so a backup cannot be restored.",
                    ));
                }
            },
        }
    }
    let parent = source.parent().ok_or_else(unreadable_source)?;
    // Blocked startup may have stopped before creating the app-owned folders.
    fs::create_dir_all(paths.restore_staging())
        .map_err(|_| restore_failed("The restore folder could not be created."))?;
    let staging = directory_identity(&paths.restore_staging());
    if directory_identity(parent).map_err(|_| unreadable_source())?
        == staging.map_err(|_| unreadable_source())?
    {
        return Err(source_error(
            "Files in the restore staging folder cannot be restored. Choose the original backup file.",
        ));
    }
    Ok(metadata.len())
}
fn copy_error(error: AppError) -> AppError {
    match error.code {
        ErrorCode::BackupSourceInvalid => not_shift_happens(),
        ErrorCode::BackupValidationFailed | ErrorCode::BackupForeignKeyViolation => {
            damaged_source()
        }
        ErrorCode::BackupCopyFailed => unreadable_source(),
        _ => error,
    }
}
async fn read_contents(staged: &DatabasePaths) -> (Option<i64>, Option<i64>, Option<String>) {
    let Ok(mut connection) = database::open_main(staged, true).await else {
        return (None, None, None);
    };
    // Older backups may lack a table; each fact is optional.
    let employees: Option<i64> =
        sqlx::query_scalar("SELECT count(*) FROM employees WHERE archived_at IS NULL")
            .fetch_one(&mut connection)
            .await
            .ok();
    let schedules: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT count(*), max(end_date) FROM schedules WHERE archived_at IS NULL")
            .fetch_one(&mut connection)
            .await
            .ok();
    let _ = connection.close().await;
    let (schedule_count, latest) = match schedules {
        Some((count, latest)) => (Some(count), latest),
        None => (None, None),
    };
    (employees, schedule_count, latest)
}

pub async fn stage(
    paths: &DatabasePaths,
    running: bool,
    clock: Arc<dyn MetadataClock>,
    runtime: &BackupRuntime,
    request: StageRestoreRequest,
) -> CommandResult<RestorePreview> {
    let Some(source) = request.source_path else {
        return CommandResult::Canceled;
    };
    let Some(guard) = runtime.try_begin() else {
        return CommandResult::Blocked {
            error: AppError::new(
                ErrorCode::BackupInProgress,
                "A backup or restore is already in progress. Wait for its result.",
            ),
        };
    };
    // A new choice always supersedes an unconfirmed earlier one.
    runtime.restores.replace(None);
    let result = stage_copy(paths, running, clock, runtime, PathBuf::from(source)).await;
    drop(guard);
    match result {
        Ok(preview) => CommandResult::RequiresConfirmation { data: preview },
        Err(error) => Err(error).into(),
    }
}

async fn stage_copy(
    paths: &DatabasePaths,
    running: bool,
    clock: Arc<dyn MetadataClock>,
    runtime: &BackupRuntime,
    source: PathBuf,
) -> AppResult<RestorePreview> {
    if marker_exists(paths) {
        return Err(pending_error());
    }
    database::loaded_sqlite_engine().await?;
    let staging = paths.restore_staging();
    let destination = staging.join(format!("staged-{}.sqlite", Uuid::new_v4()));
    let owned = paths.clone();
    let destinations = runtime.destinations.clone();
    let copy_source = source.clone();
    let copy_destination = destination.clone();
    let (source_length, staged) = tauri::async_runtime::spawn_blocking(move || {
        let length = check_source(&owned, &copy_source)?;
        match backup::create_consistent_sqlite_backup(
            &copy_source,
            &copy_destination,
            None,
            DestinationPolicy::AppOwnedNoOverwrite { directory: staging },
            ValidationPolicy::PreMigrationStrict,
            clock.as_ref(),
            &destinations,
        ) {
            BackupFileResult::Success { .. } => Ok((length, copy_destination)),
            BackupFileResult::Failure { error, .. } => Err(copy_error(error)),
            BackupFileResult::RequiresOverwrite { .. } => Err(unreadable_source()),
        }
    })
    .await
    .map_err(|_| unreadable_source())??;
    let inspected = inspect_staged(paths, &staged).await;
    let (schema_version, will_migrate, contents, digest) = match inspected {
        Ok(value) => value,
        Err(error) => {
            remove_quietly(&staged);
            return Err(error);
        }
    };
    let staged_id = Uuid::new_v4().to_string();
    let file_name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    runtime.restores.replace(Some(StagedRestore {
        id: staged_id.clone(),
        path: staged,
        length: digest.0,
        sha256: digest.1,
        source_name: file_name.clone(),
    }));
    Ok(RestorePreview {
        staged_id,
        file_name,
        source_path: display_path(&source),
        file_size_bytes: source_length,
        schema_version,
        current_schema_version: MIGRATOR.iter().last().map_or(0, |m| m.version),
        will_migrate,
        employee_count: contents.0,
        schedule_count: contents.1,
        latest_schedule_end_date: contents.2,
        safety_backup: running,
        restore_backups_path: display_path(&paths.restore_backups()),
    })
}

type StagedFacts = (
    i64,
    bool,
    (Option<i64>, Option<i64>, Option<String>),
    (u64, String),
);
/// The same read-only classification that startup uses. Only a current or
/// upgradeable Shift Happens history is accepted.
async fn inspect_staged(paths: &DatabasePaths, staged: &Path) -> AppResult<StagedFacts> {
    let staged_paths = DatabasePaths {
        directory: paths.restore_staging(),
        main: staged.to_owned(),
    };
    let inspection = startup::inspect_existing(&staged_paths)
        .await
        .map_err(|_| unreadable_source())?;
    let will_migrate = match inspection.classification {
        Classification::Current => false,
        Classification::Pending => true,
        Classification::NewerOrUnknownMigration => {
            return Err(source_error(
                "This backup was made by a newer version of Shift Happens.",
            ));
        }
        Classification::MigrationMetadataMissing | Classification::ChecksumMismatch => {
            return Err(not_shift_happens());
        }
        Classification::DatabaseIntegrityError => return Err(damaged_source()),
        Classification::FailedOrDirtyMigration => {
            return Err(source_error(
                "This backup has an incomplete upgrade history and cannot be restored.",
            ));
        }
        _ => return Err(unreadable_source()),
    };
    let schema_version = inspection.highest_applied.ok_or_else(not_shift_happens)?;
    let contents = read_contents(&staged_paths).await;
    let owned = staged.to_owned();
    let digest = tauri::async_runtime::spawn_blocking(move || {
        safe_destination::require_no_sidecars(&owned).map_err(|_| unreadable_source())?;
        file_digest(&owned).map_err(|_| unreadable_source())
    })
    .await
    .map_err(|_| unreadable_source())??;
    Ok((schema_version, will_migrate, contents, digest))
}

// ---------- Phase B: confirm, save the current data, schedule ----------

pub struct ApplyOutcome {
    pub result: CommandResult<RestoreScheduled>,
    /// Present when the app must restart; holds the backup gate until then.
    pub restart: Option<InFlight>,
}

/// `live` is the running pool and clock. Blocked startup passes `None`; its files
/// are preserved by the startup move-aside instead of a copy.
pub async fn apply(
    paths: &DatabasePaths,
    live: Option<&SqlitePool>,
    clock: Arc<dyn MetadataClock>,
    runtime: &BackupRuntime,
    request: ApplyRestoreRequest,
) -> ApplyOutcome {
    let Some(guard) = runtime.try_begin() else {
        return ApplyOutcome {
            result: CommandResult::Blocked {
                error: AppError::new(
                    ErrorCode::BackupInProgress,
                    "A backup or restore is already in progress. Wait for its result.",
                ),
            },
            restart: None,
        };
    };
    let Some(staged) = runtime.restores.take(&request.staged_id) else {
        return ApplyOutcome {
            result: CommandResult::ValidationError {
                error: AppError::new(
                    ErrorCode::RestoreConfirmationExpired,
                    "This restore confirmation expired. Choose the backup again.",
                ),
            },
            restart: None,
        };
    };
    // Nothing has changed yet; a failed precheck leaves the app running.
    let owned = paths.clone();
    let checked = {
        let path = staged.path.clone();
        let sha256 = staged.sha256.clone();
        let length = staged.length;
        tauri::async_runtime::spawn_blocking(move || {
            if marker_exists(&owned) {
                return Err(pending_error());
            }
            if !matches_digest(&path, length, &sha256) {
                return Err(restore_failed(
                    "The checked copy of the backup changed. Choose the backup again.",
                ));
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err(restore_failed("The restore could not be prepared.")))
    };
    if let Err(error) = checked {
        // The confirmation is consumed; its staged copy has no further use.
        remove_quietly(&staged.path);
        return ApplyOutcome {
            result: CommandResult::Failure { error },
            restart: None,
        };
    }
    let mut safety_backup = None;
    if let Some(pool) = live {
        // Finish any schedule write, then release every pooled connection so the
        // last close checkpoints WAL. The app restarts after this point.
        let _writes = crate::schedule_write::SCHEDULE_WRITES.lock().await;
        pool.close().await;
        match save_current(paths, clock.clone(), runtime).await {
            Ok(path) => safety_backup = Some(path),
            Err(_) => {
                remove_quietly(&staged.path);
                return ApplyOutcome {
                    result: CommandResult::Failure {
                        error: restore_failed(
                            "Your current data could not be saved, so nothing was restored. Shift Happens will restart.",
                        ),
                    },
                    restart: Some(guard),
                };
            }
        }
    }
    let marker = PendingRestore {
        version: MARKER_VERSION,
        staged_path: staged.path.to_string_lossy().into_owned(),
        staged_sha256: staged.sha256.clone(),
        staged_length: staged.length,
        source_name: staged.source_name.clone(),
        safety_backup: safety_backup.clone(),
        replaced_dir: None,
        created_at: clock_timestamp(clock.as_ref()).unwrap_or_default(),
    };
    let owned = paths.clone();
    let written =
        tauri::async_runtime::spawn_blocking(move || write_marker(&owned, &marker, false))
            .await
            .is_ok_and(|result| result.is_ok());
    if !written {
        remove_quietly(&staged.path);
        let error = if live.is_some() {
            restore_failed(
                "The restore could not be scheduled. Your data was not changed. Shift Happens will restart.",
            )
        } else {
            restore_failed("The restore could not be scheduled. Your data was not changed.")
        };
        return ApplyOutcome {
            result: CommandResult::Failure { error },
            // The pool is closed; only a restart returns the app to a usable state.
            restart: live.map(|_| guard),
        };
    }
    ApplyOutcome {
        result: CommandResult::Success {
            data: RestoreScheduled {
                safety_backup,
                replaced_files_folder: display_path(&paths.restore_backups()),
            },
            warnings: Vec::new(),
        },
        restart: Some(guard),
    }
}

/// A validated copy of the quiescent live database. History is not recorded:
/// the pool is closed and the data is about to be replaced.
async fn save_current(
    paths: &DatabasePaths,
    clock: Arc<dyn MetadataClock>,
    runtime: &BackupRuntime,
) -> AppResult<String> {
    let directory = paths.restore_backups();
    let destination = directory.join(format!(
        "shift-happens-before-restore-{}.sqlite",
        stamp(clock.as_ref())?
    ));
    let source = paths.main.clone();
    let destinations = runtime.destinations.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        backup::create_consistent_sqlite_backup(
            &source,
            &destination,
            None,
            DestinationPolicy::AppOwnedNoOverwrite { directory },
            ValidationPolicy::ManualPreserve,
            clock.as_ref(),
            &destinations,
        )
    })
    .await
    .map_err(|_| restore_failed("The current data could not be saved."))?;
    match result {
        BackupFileResult::Success { file, .. } => Ok(file.destination_path),
        BackupFileResult::Failure { error, .. } => Err(error),
        BackupFileResult::RequiresOverwrite { .. } => {
            Err(restore_failed("The current data could not be saved."))
        }
    }
}

// ---------- Phase C: startup applies a pending restore ----------

fn marker_is_owned(paths: &DatabasePaths, marker: &PendingRestore) -> bool {
    let staged = Path::new(&marker.staged_path);
    let staged_owned = staged
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("staged-")
                && name.ends_with(".sqlite")
                && paths.restore_staging().join(name) == staged
        });
    let replaced_owned = marker.replaced_dir.as_deref().is_none_or(|directory| {
        let directory = Path::new(directory);
        directory
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("replaced-") && paths.restore_backups().join(name) == directory
            })
    });
    marker.version == MARKER_VERSION
        && staged_owned
        && replaced_owned
        && marker.staged_sha256.len() == 64
        && marker
            .staged_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Startup entry point. Returns the restore outcome and warnings, or an error
/// that blocks startup while keeping the marker for the next launch.
pub(crate) fn apply_pending_restore(
    paths: &DatabasePaths,
    clock: &dyn MetadataClock,
) -> AppResult<(Option<RestoreReport>, Vec<AppError>)> {
    let marker_path = paths.restore_marker();
    match regular_file(&marker_path) {
        Ok(None) => {
            cleanup_staging(paths);
            return Ok((None, Vec::new()));
        }
        Ok(Some(_)) => {}
        Err(_) => return Err(startup_blocked()),
    }
    let marker = read_marker(&marker_path).filter(|marker| marker_is_owned(paths, marker));
    let Some(mut marker) = marker else {
        // Markers are published whole, so this is unexpected. Without a live
        // database the files may already be aside: never start a new database.
        if regular_file(&paths.main).map_or(true, |main| main.is_none()) {
            return Err(startup_conflict(&paths.restore_backups()));
        }
        return abandon(paths, None, clock);
    };
    let staged = PathBuf::from(&marker.staged_path);
    let staged_valid = matches_digest(&staged, marker.staged_length, &marker.staged_sha256);
    if !staged_valid {
        let Some(replaced) = marker.replaced_dir.clone().map(PathBuf::from) else {
            // Nothing was moved yet: the live files are untouched.
            return abandon(paths, Some(&marker), clock);
        };
        if matches_digest(&paths.main, marker.staged_length, &marker.staged_sha256) {
            // The final move completed before the marker could be removed.
            return finish(paths, &marker, clock);
        }
        roll_back(paths, &replaced)?;
        return abandon(paths, Some(&marker), clock);
    }
    let replaced = match marker.replaced_dir.clone() {
        Some(directory) => {
            let directory = PathBuf::from(directory);
            fs::create_dir_all(&directory).map_err(|_| startup_blocked())?;
            directory
        }
        None => {
            let stamp = stamp(clock)?;
            let mut directory = paths.restore_backups().join(format!("replaced-{stamp}"));
            if fs::symlink_metadata(&directory).is_ok() {
                directory = paths.restore_backups().join(format!(
                    "replaced-{stamp}-{}",
                    &Uuid::new_v4().simple().to_string()[..8]
                ));
            }
            fs::create_dir(&directory).map_err(|_| startup_blocked())?;
            // Record the folder before anything moves into it.
            marker.replaced_dir = Some(directory.to_string_lossy().into_owned());
            write_marker(paths, &marker, true).map_err(|_| startup_blocked())?;
            directory
        }
    };
    let metadata = fs::symlink_metadata(&replaced).map_err(|_| startup_blocked())?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(startup_blocked());
    }
    for (live, name) in live_files(paths)? {
        match regular_file(&live) {
            Ok(None) => continue,
            Ok(Some(_)) => {}
            Err(_) => return Err(startup_conflict(&replaced)),
        }
        let target = replaced.join(name);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(startup_conflict(&replaced));
        }
        move_with_retry(&live, &target, false).map_err(|_| startup_blocked())?;
    }
    move_with_retry(&staged, &paths.main, false).map_err(|_| startup_blocked())?;
    if !matches_digest(&paths.main, marker.staged_length, &marker.staged_sha256) {
        return Err(startup_conflict(&replaced));
    }
    finish(paths, &marker, clock)
}

fn finish(
    paths: &DatabasePaths,
    marker: &PendingRestore,
    clock: &dyn MetadataClock,
) -> AppResult<(Option<RestoreReport>, Vec<AppError>)> {
    // The marker must be gone before any writer opens the installed copy. A later
    // start recognizes an unfinished install only while the main file still
    // matches the staged digest, so this start stays blocked instead.
    match with_retry(|| fs::remove_file(paths.restore_marker())) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(startup_blocked()),
    }
    cleanup_staging(paths);
    Ok((
        Some(RestoreReport {
            status: "applied",
            source_name: Some(marker.source_name.clone()),
            safety_backup: marker.safety_backup.clone(),
            replaced_dir: marker
                .replaced_dir
                .as_deref()
                .map(|directory| display_path(Path::new(directory))),
            migrated: false,
            completed_at: clock_timestamp(clock).ok(),
        }),
        Vec::new(),
    ))
}

/// Moves files back after an interrupted restore whose staged copy is gone.
fn roll_back(paths: &DatabasePaths, replaced: &Path) -> AppResult<()> {
    for (live, name) in live_files(paths)? {
        let moved = replaced.join(name);
        match regular_file(&moved) {
            Ok(None) => continue,
            Ok(Some(_)) => {}
            Err(_) => return Err(startup_conflict(replaced)),
        }
        if fs::symlink_metadata(&live).is_ok() {
            return Err(startup_conflict(replaced));
        }
        move_with_retry(&moved, &live, false).map_err(|_| startup_blocked())?;
    }
    let _ = fs::remove_dir(replaced);
    Ok(())
}

/// The marker is set aside, never deleted, so the attempt remains inspectable.
fn abandon(
    paths: &DatabasePaths,
    marker: Option<&PendingRestore>,
    clock: &dyn MetadataClock,
) -> AppResult<(Option<RestoreReport>, Vec<AppError>)> {
    let aside = paths.restore_backups().join(format!(
        "abandoned-restore-{}-{}.json",
        stamp(clock)?,
        &Uuid::new_v4().simple().to_string()[..8]
    ));
    move_with_retry(&paths.restore_marker(), &aside, false).map_err(|_| startup_blocked())?;
    cleanup_staging(paths);
    Ok((
        Some(RestoreReport {
            status: "abandoned",
            source_name: marker.map(|marker| marker.source_name.clone()),
            safety_backup: marker.and_then(|marker| marker.safety_backup.clone()),
            replaced_dir: None,
            migrated: false,
            completed_at: clock_timestamp(clock).ok(),
        }),
        vec![restore_failed(
            "A pending restore could not be applied and was set aside. Your data was not changed.",
        )],
    ))
}
