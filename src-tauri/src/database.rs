use crate::{
    error::{AppError, AppResult, ErrorCode},
    metadata::MetadataClock,
    safe_destination::{self, FileIdentity, is_reparse},
    settings::{read_in_transaction, seed_settings},
    shift_templates::{seed_shift_templates, validate_templates},
    template_restrictions,
};
use serde::Serialize;
use sqlx::{
    ConnectOptions, Connection, Row, SqliteConnection, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous},
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
pub const DATABASE_FILENAME: &str = "shift-happens.sqlite";
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_POOL_CONNECTIONS: u32 = 4;

#[derive(Clone)]
pub struct DatabasePaths {
    pub directory: PathBuf,
    pub main: PathBuf,
}

impl DatabasePaths {
    pub fn migration_backups(&self) -> PathBuf {
        self.directory.join("migration-backups")
    }
    /// Validated copies of a chosen backup that wait for confirmation.
    pub fn restore_staging(&self) -> PathBuf {
        self.directory.join("restore-staging")
    }
    /// Safety copies and moved-aside files from restores. Never deleted.
    pub fn restore_backups(&self) -> PathBuf {
        self.directory.join("restore-backups")
    }
    /// Present only between a confirmed restore and the startup that applies it.
    pub fn restore_marker(&self) -> PathBuf {
        self.directory.join("restore-pending.json")
    }
}

/// The resolver is injectable, but the real entry point supplies Tauri's stable app-local directory.
pub fn resolve_database_paths(
    resolve: impl FnOnce() -> AppResult<PathBuf>,
    development: bool,
) -> AppResult<DatabasePaths> {
    let root = resolve()?;
    if !root.is_absolute() {
        return Err(classification_error());
    }
    let directory = if development {
        root.join("development")
    } else {
        root
    };
    Ok(DatabasePaths {
        main: directory.join(DATABASE_FILENAME),
        directory,
    })
}

fn classification_error() -> AppError {
    AppError::new(
        ErrorCode::StartupClassificationChanged,
        "The database file set is unexpected or changed. Startup stopped without replacing it.",
    )
}

fn database_error() -> AppError {
    AppError::new(
        ErrorCode::DatabaseUnavailable,
        "The local database could not be opened or inspected.",
    )
}

/// How long a launch waits for the database directory. A restart starts its
/// successor just before the previous process exits.
const DIRECTORY_LOCK_WAIT: Duration = Duration::from_secs(10);

/// Held for the process lifetime, so exactly one process works with a database
/// directory. The OS releases it when the handle closes, including after a
/// crash, so the empty lock file itself is never deleted.
pub struct DirectoryLock {
    _file: fs::File,
}

/// `None` means another live process kept the directory for the whole wait.
pub fn lock_directory(paths: &DatabasePaths) -> AppResult<Option<DirectoryLock>> {
    fs::create_dir_all(&paths.directory).map_err(|_| database_error())?;
    let path = paths.directory.join("shift-happens.lock");
    // `create_new` never follows an existing link, and the no-follow reopen
    // below refuses one.
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(database_error()),
    }
    let file = safe_destination::open_narrow(&path, true).map_err(|_| database_error())?;
    let metadata = file.metadata().map_err(|_| database_error())?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(classification_error());
    }
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(DirectoryLock { _file: file })),
            Err(fs::TryLockError::WouldBlock) if started.elapsed() < DIRECTORY_LOCK_WAIT => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(_)) => return Err(database_error()),
        }
    }
}

pub fn create_owned_directories(paths: &DatabasePaths) -> AppResult<()> {
    fs::create_dir_all(&paths.directory).map_err(|_| database_error())?;
    let metadata = fs::symlink_metadata(&paths.directory).map_err(|_| classification_error())?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(classification_error());
    }
    for owned in [
        paths.migration_backups(),
        paths.restore_staging(),
        paths.restore_backups(),
    ] {
        fs::create_dir_all(&owned).map_err(|_| database_error())?;
        let metadata = fs::symlink_metadata(owned).map_err(|_| classification_error())?;
        if !metadata.is_dir() || is_reparse(&metadata) {
            return Err(classification_error());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileEntry {
    identity: FileIdentity,
    length: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}

fn inspect_file(path: &Path) -> AppResult<Option<FileEntry>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || is_reparse(&metadata) {
                return Err(classification_error());
            }
            let handle =
                safe_destination::open_narrow(path, false).map_err(|_| database_error())?;
            let current = handle.metadata().map_err(|_| database_error())?;
            if !current.is_file() || is_reparse(&current) {
                return Err(classification_error());
            }
            Ok(Some(FileEntry {
                identity: safe_destination::handle_identity(&handle)
                    .map_err(|_| database_error())?,
                length: current.len(),
                modified: current.modified().map_err(|_| classification_error())?,
                created: current.created().ok(),
            }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(database_error()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSet {
    parent: FileIdentity,
    main: Option<FileEntry>,
    sidecars: [Option<FileEntry>; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileClassification {
    NewDatabase,
    UninitializedEmptyDatabase,
    ExistingDatabase,
}

impl FileSet {
    fn require_identity(&self, paths: &DatabasePaths) -> AppResult<()> {
        let next = inspect_database_file_set(paths)?;
        if next.parent != self.parent
            || next.main.as_ref().map(|entry| &entry.identity)
                != self.main.as_ref().map(|entry| &entry.identity)
            || next.classification()? != FileClassification::ExistingDatabase
            || next.sidecars[2].is_some()
        {
            return Err(classification_error());
        }
        Ok(())
    }
    pub(crate) fn require_unchanged(&self, paths: &DatabasePaths) -> AppResult<()> {
        if inspect_database_file_set(paths)? != *self {
            return Err(classification_error());
        }
        Ok(())
    }

    /// Read-only SQLite may create an empty WAL and participate in shared memory
    /// when the main header already selects WAL. It cannot write WAL frames,
    /// mutate the main database or recover a rollback journal.
    pub(crate) fn after_reader(&self, paths: &DatabasePaths, wal: bool) -> AppResult<Self> {
        let next = inspect_database_file_set(paths)?;
        let wal_unchanged = next.sidecars[0] == self.sidecars[0]
            || (wal
                && self.sidecars[0].is_none()
                && next.sidecars[0]
                    .as_ref()
                    .is_some_and(|entry| entry.length == 0));
        if next.parent != self.parent
            || next.main != self.main
            || !wal_unchanged
            || next.sidecars[2] != self.sidecars[2]
            || (!wal && next.sidecars[1] != self.sidecars[1])
        {
            return Err(classification_error());
        }
        next.classification()?;
        Ok(next)
    }

    /// Migrations, WAL initialization, pool lifecycle and seeding own SQLite
    /// housekeeping. Require the same main inode/file ID and safe file types;
    /// never compare their output to the original absent/empty observation.
    pub(crate) fn after_writer(&self, paths: &DatabasePaths) -> AppResult<Self> {
        let next = inspect_database_file_set(paths)?;
        if next.parent != self.parent
            || self.main.as_ref().map(|entry| &entry.identity)
                != next.main.as_ref().map(|entry| &entry.identity)
            || next.classification()? != FileClassification::ExistingDatabase
            || next.sidecars[2].is_some()
        {
            return Err(classification_error());
        }
        Ok(next)
    }

    pub fn classification(&self) -> AppResult<FileClassification> {
        let sidecars_absent = self.sidecars.iter().all(Option::is_none);
        match &self.main {
            None if sidecars_absent => Ok(FileClassification::NewDatabase),
            Some(entry) if entry.length == 0 && sidecars_absent => {
                Ok(FileClassification::UninitializedEmptyDatabase)
            }
            Some(entry) if entry.length > 0 => Ok(FileClassification::ExistingDatabase),
            _ => Err(classification_error()),
        }
    }
}

pub fn inspect_database_file_set(paths: &DatabasePaths) -> AppResult<FileSet> {
    let suffix = |suffix: &str| {
        let mut name = paths.main.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };
    Ok(FileSet {
        parent: {
            let metadata = fs::symlink_metadata(&paths.directory).map_err(|_| database_error())?;
            if !metadata.is_dir() || is_reparse(&metadata) {
                return Err(classification_error());
            }
            let handle = safe_destination::open_narrow(&paths.directory, false)
                .map_err(|_| database_error())?;
            let current = handle.metadata().map_err(|_| database_error())?;
            if !current.is_dir() || is_reparse(&current) {
                return Err(classification_error());
            }
            safe_destination::handle_identity(&handle).map_err(|_| database_error())?
        },
        main: inspect_file(&paths.main)?,
        sidecars: [
            inspect_file(&suffix("-wal"))?,
            inspect_file(&suffix("-shm"))?,
            inspect_file(&suffix("-journal"))?,
        ],
    })
}

/// This is the only main-file creation operation. SQLite itself is always create-disabled.
pub fn claim_new_database(paths: &DatabasePaths, classified: &FileSet) -> AppResult<FileSet> {
    if classified.classification()? != FileClassification::NewDatabase
        || inspect_database_file_set(paths)? != *classified
    {
        return Err(classification_error());
    }
    let handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.main)
        .map_err(|_| classification_error())?;
    let claimed = handle.metadata().map_err(|_| classification_error())?;
    let identity = FileEntry {
        identity: safe_destination::handle_identity(&handle).map_err(|_| classification_error())?,
        length: claimed.len(),
        modified: claimed.modified().map_err(|_| classification_error())?,
        created: claimed.created().ok(),
    };
    drop(handle);
    let observed = inspect_database_file_set(paths)?;
    if observed.classification()? != FileClassification::UninitializedEmptyDatabase
        || observed.parent != classified.parent
        || observed.main.as_ref() != Some(&identity)
    {
        return Err(classification_error());
    }
    Ok(observed)
}

/// No journal_mode preference, immutable, nolock, or creation side effect.
pub(crate) fn connection_options(path: &Path, read_only: bool) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(read_only)
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(BUSY_TIMEOUT)
        .disable_statement_logging()
}

pub(crate) async fn open_main(
    paths: &DatabasePaths,
    read_only: bool,
) -> AppResult<SqliteConnection> {
    require_existing_main(paths).await?;
    SqliteConnection::connect_with(&connection_options(&paths.main, read_only))
        .await
        .map_err(|error| {
            if read_only {
                // Opening has not established a safely recognized database yet.
                database_error()
            } else {
                error.into()
            }
        })
}

/// SQLite extended result codes retain their primary code in the low byte.
/// After recognition, corruption differs from locking and recovery access.
pub(crate) fn sqlite_inspection_error(error: sqlx::Error) -> AppError {
    let corrupt = match &error {
        sqlx::Error::Database(error) => error
            .code()
            .and_then(|code| code.parse::<i32>().ok())
            .is_some_and(|code| matches!(code & 0xff, 11 | 26)),
        _ => false,
    };
    if corrupt {
        AppError::new(
            ErrorCode::DatabaseIntegrityError,
            "SQLite could not read a valid database. Close the app and preserve its files for developer inspection.",
        )
    } else {
        database_error()
    }
}

/// A dedicated live reader observes committed WAL data with normal SQLite locks.
pub(crate) async fn open_validation_reader(paths: &DatabasePaths) -> AppResult<SqliteConnection> {
    open_main(paths, true).await
}

async fn require_existing_main(paths: &DatabasePaths) -> AppResult<()> {
    let main = paths.main.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if inspect_file(&main)?.is_none() {
            return Err(classification_error());
        }
        Ok(())
    })
    .await
    .map_err(|_| database_error())?
}

#[derive(Debug, Clone, Serialize)]
pub struct SqliteEngine {
    pub version: String,
    pub source_id: String,
}

pub fn require_safe_engine(engine: &SqliteEngine) -> AppResult<()> {
    let parts: Vec<u32> = engine
        .version
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| database_error())?;
    let safe = match parts.as_slice() {
        [major, minor, patch] => {
            (*major, *minor, *patch) >= (3, 51, 3)
                || (*major == 3 && *minor == 50 && *patch >= 7)
                || (*major == 3 && *minor == 44 && *patch >= 6)
        }
        _ => false,
    };
    if !safe || engine.source_id.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::UnsafeSqliteEngine,
            "This SQLite runtime lacks the required WAL-reset fix. Startup stopped.",
        ));
    }
    Ok(())
}

pub(crate) async fn inspect_engine(connection: &mut SqliteConnection) -> AppResult<SqliteEngine> {
    let (version, source_id): (String, String) =
        sqlx::query_as("SELECT sqlite_version(), sqlite_source_id()")
            .fetch_one(&mut *connection)
            .await?;
    let engine = SqliteEngine { version, source_id };
    require_safe_engine(&engine)?;
    Ok(engine)
}

/// Focused CLI/release check of the exact loaded linkage. This opens only an isolated in-memory connection.
pub async fn loaded_sqlite_engine() -> AppResult<SqliteEngine> {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .create_if_missing(false)
            .disable_statement_logging(),
    )
    .await?;
    let result = inspect_engine(&mut connection).await;
    connection.close().await?;
    let engine = result?;
    let backup_engine = tauri::async_runtime::spawn_blocking(|| {
        let engine = crate::backup_sqlite::loaded_engine()?;
        let connection = crate::backup_sqlite::Connection::open(Path::new(":memory:"), true)?;
        let result = (|| {
            if connection.single_text(c"SELECT sqlite_version()")? != engine.version
                || connection.single_text(c"SELECT sqlite_source_id()")? != engine.source_id
            {
                return Err(database_error());
            }
            Ok(engine)
        })();
        connection.close()?;
        result
    })
    .await
    .map_err(|_| database_error())??;
    if engine.version != backup_engine.version || engine.source_id != backup_engine.source_id {
        return Err(AppError::new(
            ErrorCode::UnsafeSqliteEngine,
            "The application and backup SQLite runtimes do not match.",
        ));
    }
    Ok(engine)
}

pub fn validate_embedded_migrations() -> AppResult<()> {
    for migration in MIGRATOR.iter() {
        // Fail closed even for quoted identifiers or statements in comments. This token belongs only to startup.
        if migration
            .sql
            .as_str()
            .to_ascii_lowercase()
            .contains("journal_mode")
            || migration.no_tx
        {
            return Err(AppError::new(
                ErrorCode::MigrationInvalid,
                "Embedded migrations must be transactional and cannot change journal mode.",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationState {
    pub applied_versions: Vec<i64>,
    pub embedded_versions: Vec<i64>,
    pub current: bool,
}

/// A real history has one row per embedded migration; more rows are malformed.
pub(crate) const MIGRATION_HISTORY_LIMIT: usize = 1000;

pub(crate) struct AppliedMigration {
    pub(crate) version: i64,
    pub(crate) succeeded: bool,
    pub(crate) checksum: Vec<u8>,
}

/// Reads `_sqlx_migrations` in version order, at most one row past the limit.
/// `success` is read as a raw integer that must be exactly 1: SQLx would decode
/// any nonzero value as `true`. `None` is a row with unexpected column types.
pub(crate) async fn read_migration_history(
    connection: &mut SqliteConnection,
) -> Result<Vec<Option<AppliedMigration>>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version LIMIT 1001",
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .iter()
        .map(|row| {
            Some(AppliedMigration {
                version: row.try_get("version").ok()?,
                succeeded: row.try_get::<i64, _>("success").ok()? == 1,
                checksum: row.try_get("checksum").ok()?,
            })
        })
        .collect())
}

async fn migration_state(connection: &mut SqliteConnection) -> AppResult<MigrationState> {
    validate_embedded_migrations()?;
    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(&mut *connection)
    .await?;
    let embedded_versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    if exists == 0 {
        return Ok(MigrationState {
            applied_versions: vec![],
            embedded_versions,
            current: false,
        });
    }
    let rows = read_migration_history(&mut *connection).await?;
    let mut applied_versions = Vec::with_capacity(rows.len());
    let mut matches = rows.len() <= MIGRATION_HISTORY_LIMIT;
    for row in rows {
        let Some(row) = row else {
            matches = false;
            continue;
        };
        matches &= row.succeeded
            && MIGRATOR.iter().any(|m| {
                m.version == row.version && m.checksum.as_ref() == row.checksum.as_slice()
            });
        applied_versions.push(row.version);
    }
    let current = matches && applied_versions == embedded_versions;
    Ok(MigrationState {
        applied_versions,
        embedded_versions,
        current,
    })
}

pub async fn inspect_migration_state(paths: &DatabasePaths) -> AppResult<MigrationState> {
    let mut connection = open_main(paths, true).await?;
    inspect_engine(&mut connection).await?;
    let mut tx = connection.begin().await?;
    let result = migration_state(&mut tx).await;
    tx.rollback().await?;
    connection.close().await?;
    result
}

/// Caller owns the backup/upgrade authorization. This stage never creates the file or sets WAL.
pub async fn apply_embedded_migrations(paths: &DatabasePaths) -> AppResult<()> {
    validate_embedded_migrations()?;
    let mut connection = open_main(paths, false).await?;
    inspect_engine(&mut connection).await?;
    let result = MIGRATOR.run(&mut connection).await.map_err(|_| {
        AppError::new(
            ErrorCode::MigrationInvalid,
            "The embedded database migration could not complete.",
        )
    });
    connection.close().await?;
    result
}

pub async fn initialize_startup_wal(paths: &DatabasePaths) -> AppResult<()> {
    let mut connection = open_main(paths, false).await?;
    let result = async {
        inspect_engine(&mut connection).await?;
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut connection)
            .await?;
        let final_mode = if mode == "wal" {
            mode
        } else {
            sqlx::query_scalar::<_, String>("PRAGMA journal_mode = WAL")
                .fetch_one(&mut connection)
                .await?
        };
        if final_mode != "wal" {
            return Err(AppError::new(
                ErrorCode::WalRequired,
                "The database could not enter WAL mode.",
            ));
        }
        Ok(())
    }
    .await;
    connection.close().await?;
    result
}

async fn verify_connection(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut *connection)
        .await?;
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await?;
    let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(&mut *connection)
        .await?;
    let timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *connection)
        .await?;
    if mode != "wal"
        || foreign_keys != 1
        || synchronous != 2
        || timeout != BUSY_TIMEOUT.as_millis() as i64
    {
        return Err(sqlx::Error::Protocol(
            "Required SQLite connection options are unavailable".to_owned(),
        ));
    }
    Ok(())
}

pub async fn open_normal_pool(paths: &DatabasePaths) -> AppResult<SqlitePool> {
    require_existing_main(paths).await?;
    let boundary = inspect_database_file_set(paths)?;
    let connected_paths = paths.clone();
    let acquired_paths = paths.clone();
    let acquired_boundary = boundary.clone();
    SqlitePoolOptions::new()
        .max_connections(MAX_POOL_CONNECTIONS)
        .min_connections(1)
        .acquire_timeout(BUSY_TIMEOUT)
        .after_connect(move |connection, _| {
            let paths = connected_paths.clone();
            let boundary = boundary.clone();
            Box::pin(async move {
                verify_pool_file(&paths, &boundary).await?;
                verify_connection(connection).await
            })
        })
        .before_acquire(move |_, _| {
            let paths = acquired_paths.clone();
            let boundary = acquired_boundary.clone();
            Box::pin(async move {
                verify_pool_file(&paths, &boundary).await?;
                Ok(true)
            })
        })
        .connect_with(connection_options(&paths.main, false))
        .await
        .map_err(|_| {
            AppError::new(
                ErrorCode::WalRequired,
                "The database pool could not verify WAL and its required pragmas.",
            )
        })
}

async fn verify_pool_file(paths: &DatabasePaths, boundary: &FileSet) -> Result<(), sqlx::Error> {
    let paths = paths.clone();
    let boundary = boundary.clone();
    tauri::async_runtime::spawn_blocking(move || boundary.require_identity(&paths))
        .await
        .map_err(|_| sqlx::Error::Protocol("The database file identity is unavailable".into()))?
        .map_err(|_| sqlx::Error::Protocol("The database file set changed".into()))
}

pub async fn run_post_schema_initialization(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
) -> AppResult<()> {
    let mut stage = "begin";
    let result = async {
        // An early return drops `tx`, which rolls it back before its pooled
        // connection is reused or closed. No explicit rollback can then replace
        // the stage's own error.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        stage = "typed_settings";
        seed_settings(&mut tx, clock).await?;
        stage = "shift_templates";
        seed_shift_templates(&mut tx, clock).await?;
        stage = "positions";
        crate::positions::seed_positions(&mut tx, clock).await?;
        stage = "coverage";
        crate::coverage::seed_coverage(&mut tx, clock).await?;
        stage = "holidays";
        crate::holidays::seed_holidays(&mut tx, clock).await?;
        stage = "validate_settings";
        read_in_transaction(&mut tx).await?;
        stage = "validate_shift_templates";
        validate_templates(&mut tx).await?;
        stage = "validate_template_restrictions";
        template_restrictions::validate_restrictions(&mut tx).await?;
        stage = "validate_positions";
        crate::positions::validate_positions(&mut tx).await?;
        stage = "validate_coverage";
        crate::coverage::validate_coverage(&mut tx).await?;
        stage = "validate_position_restrictions";
        crate::position_restrictions::validate_restrictions(&mut tx).await?;
        stage = "validate_availability_pto";
        crate::availability_pto::validate_entries(&mut tx).await?;
        stage = "validate_holidays";
        crate::holidays::validate_holidays(&mut tx).await?;
        stage = "commit";
        tx.commit().await?;
        Ok::<_, AppError>(())
    }
    .await;
    result.map_err(|cause| {
        let mut error = AppError::new(
            ErrorCode::PostSchemaInitializationFailed,
            "Database initialization failed. Application state was not created.",
        );
        error.context.stage = Some(stage);
        error.context.cause = Some(cause.code);
        error.context.sqlite_code = cause.context.sqlite_code;
        error
    })
}

pub struct AppState {
    pub paths: DatabasePaths,
    pub pool: SqlitePool,
    pub clock: Arc<dyn MetadataClock>,
}

/// The existing development convenience path uses the same safety decision as
/// installed startup. It cannot bypass classification or backup-before-upgrade.
pub async fn start_development_database(
    paths: DatabasePaths,
    clock: Arc<dyn MetadataClock>,
) -> AppResult<AppState> {
    if !cfg!(debug_assertions) {
        return Err(AppError::new(
            ErrorCode::ReleaseStartupNotQualified,
            "Installed database startup requires the Prompt 15 safety wrapper.",
        ));
    }
    let outcome = crate::startup::start_database(paths, clock).await;
    outcome
        .state
        .ok_or_else(|| outcome.context.report.error.unwrap_or_else(database_error))
}

#[derive(Serialize)]
pub struct DatabaseHealth {
    pub path: String,
    pub exists: bool,
    pub migrations: MigrationState,
    pub foreign_keys: bool,
    pub journal_mode: String,
    pub synchronous: String,
    pub busy_timeout_ms: i64,
    pub max_pool_connections: u32,
    pub engine: SqliteEngine,
    pub health: &'static str,
}

pub async fn get_database_health(paths: &DatabasePaths) -> AppResult<DatabaseHealth> {
    let mut connection = open_main(paths, true).await?;
    let result = async {
        let engine = inspect_engine(&mut connection).await?;
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut connection)
            .await?;
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut connection)
            .await?;
        let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(&mut connection)
            .await?;
        let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&mut connection)
            .await?;
        let mut tx = connection.begin().await?;
        let migrations = migration_state(&mut tx).await?;
        let integrity: Vec<String> = sqlx::query_scalar("PRAGMA quick_check(1)")
            .fetch_all(&mut *tx)
            .await?;
        let foreign_key_findings = sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
        tx.rollback().await?;
        let healthy = integrity == ["ok"]
            && !foreign_key_findings
            && migrations.current
            && journal_mode == "wal"
            && foreign_keys == 1
            && synchronous == 2;
        Ok(DatabaseHealth {
            path: paths.main.to_string_lossy().into_owned(),
            exists: true,
            migrations,
            foreign_keys: foreign_keys == 1,
            journal_mode,
            synchronous: if synchronous == 2 {
                "FULL".to_owned()
            } else {
                synchronous.to_string()
            },
            busy_timeout_ms,
            max_pool_connections: MAX_POOL_CONNECTIONS,
            engine,
            health: if healthy { "ok" } else { "attention_required" },
        })
    }
    .await;
    connection.close().await?;
    result
}
