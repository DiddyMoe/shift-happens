//! Best-effort, data-free diagnostic log for field support.
//!
//! The file is `<app local data>/logs/shift-happens.log`; debug builds use the
//! `development` subfolder, like the database. Past 1 MiB it becomes
//! `shift-happens.log.1`, replacing the previous one.
//!
//! Each line is `<UTC timestamp> <app version> <event> key=value ...`. Only
//! failures and startup, restore, migration and backup outcomes are recorded.
//! Values are codes, numbers, source locations and schema text compiled into
//! the app. Never add names, notes, row contents, SQL parameters, request
//! bodies or user document paths: every entry point takes typed codes, so no
//! caller can pass free text. Writing is best-effort; a failure to log never
//! affects the operation that triggered it.
use crate::{
    database::{self, MIGRATOR},
    error::{AppError, CommandResult, ErrorCode},
    startup::StartupReport,
};
use serde::Serialize;
use std::{
    fmt::Write as _,
    fs,
    io::Write as _,
    panic::PanicHookInfo,
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

const FILE_NAME: &str = "shift-happens.log";
const PREVIOUS_FILE_NAME: &str = "shift-happens.log.1";
const ROTATE_AT_BYTES: u64 = 1024 * 1024;

/// The log directory. Holding the lock also serializes appends.
static DIRECTORY: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Set once the setup hook has finished; later crashes are not startup failures.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Tauri creates the main window before the setup hook resolves paths, so a
/// WebView2 failure happens first. Until then, use the same root as Tauri's
/// resolver on Windows: `%LOCALAPPDATA%\<identifier>`.
pub fn use_fallback_directory(identifier: &str) {
    let root = std::env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join(identifier));
    if let Some(root) = root
        && let Ok(paths) = database::resolve_database_paths(|| Ok(root), cfg!(debug_assertions))
    {
        use_database_directory(&paths.directory);
    }
}

/// The authoritative location: beside the database Tauri resolved.
pub fn use_database_directory(directory: &Path) {
    *DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner) = Some(directory.join("logs"));
}

pub fn log_file() -> Option<PathBuf> {
    DIRECTORY
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|directory| directory.join(FILE_NAME))
}

pub fn mark_started() {
    STARTED.store(true, Ordering::Release);
}

pub fn started() -> bool {
    STARTED.load(Ordering::Acquire)
}

/// Appends one line. Returns whether it was written.
fn record(event: &str, fields: &str) -> bool {
    let timestamp = crate::metadata::format_metadata_instant(time::OffsetDateTime::now_utc())
        .unwrap_or_else(|_| "unknown-time".to_owned());
    let separator = if fields.is_empty() { "" } else { " " };
    let line = format!(
        "{timestamp} {} {event}{separator}{fields}\n",
        env!("CARGO_PKG_VERSION")
    );
    let directory = DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner);
    directory
        .as_deref()
        .is_some_and(|directory| append(directory, line.as_bytes()).is_ok())
}

fn append(directory: &Path, line: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(directory)?;
    let path = directory.join(FILE_NAME);
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= ROTATE_AT_BYTES) {
        // Keeps one previous file. A failed rotation still appends below.
        let _ = fs::rename(&path, directory.join(PREVIOUS_FILE_NAME));
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line)
}

/// The wire name of a fieldless enum such as `ErrorCode`.
fn name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "unknown".to_owned(),
    }
}

fn error_fields(fields: &mut String, error: &AppError) {
    let _ = write!(fields, " code={}", name(&error.code));
    if let Some(stage) = error.context.stage {
        let _ = write!(fields, " stage={stage}");
    }
    if let Some(cause) = error.context.cause {
        let _ = write!(fields, " cause={}", name(&cause));
    }
    if let Some(sqlite) = error.context.sqlite_code {
        let _ = write!(fields, " sqlite={sqlite}");
    }
}

fn outcome<T>(result: &CommandResult<T>) -> (&'static str, Option<&AppError>) {
    match result {
        CommandResult::Success { .. } => ("success", None),
        CommandResult::NoChange { .. } => ("no_change", None),
        CommandResult::RequiresConfirmation { .. } => ("requires_confirmation", None),
        CommandResult::RequiresOverwrite { .. } => ("requires_overwrite", None),
        CommandResult::Canceled => ("canceled", None),
        CommandResult::ValidationError { error } => ("validation_error", Some(error)),
        CommandResult::StaleRevision { error } => ("stale_revision", Some(error)),
        CommandResult::Blocked { error } => ("blocked", Some(error)),
        CommandResult::Failure { error } => ("failure", Some(error)),
    }
}

/// Failed commands only: `failure`, plus `blocked` for broken configuration or
/// engine state. Expected outcomes (validation, stale revisions, business
/// rules) are not failures of the app.
pub fn command<T>(command: &'static str, result: &CommandResult<T>) {
    let (status, Some(error)) = outcome(result) else {
        return;
    };
    let failed = match result {
        CommandResult::Failure { .. } => true,
        CommandResult::Blocked { .. } => matches!(
            error.code,
            ErrorCode::ConfigurationInvalid
                | ErrorCode::UnsafeSqliteEngine
                | ErrorCode::WalRequired
        ),
        _ => false,
    };
    if failed {
        let mut fields = format!("command={command} status={status}");
        error_fields(&mut fields, error);
        record("command_failed", &fields);
    }
}

/// Backup and restore commands: every decided outcome, not just failures.
pub fn file_operation<T>(command: &'static str, result: &CommandResult<T>) {
    if matches!(result, CommandResult::Canceled) {
        return;
    }
    let (status, error) = outcome(result);
    let mut fields = format!("command={command} status={status}");
    if let Some(error) = error {
        error_fields(&mut fields, error);
    }
    record("file_operation", &fields);
}

/// A SQLite error at the point it becomes an `AppError`, before any wrapping.
/// A trigger's RAISE text is recorded only when it is a literal of an embedded
/// migration, so the logged text is schema compiled into the app.
pub fn sqlite_error(
    error: &sqlx::Error,
    sqlite_code: Option<i32>,
    sqlite_message: Option<&str>,
    mapped: ErrorCode,
) {
    let source = match error {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::PoolTimedOut => "pool_timed_out",
        sqlx::Error::PoolClosed => "pool_closed",
        sqlx::Error::RowNotFound => "row_not_found",
        sqlx::Error::ColumnNotFound(_) | sqlx::Error::ColumnIndexOutOfBounds { .. } => "column",
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => "decode",
        sqlx::Error::Encode(_) => "encode",
        sqlx::Error::Protocol(_) => "protocol",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::WorkerCrashed => "worker_crashed",
        _ => "other",
    };
    let mut fields = format!("mapped={} source={source}", name(&mapped));
    if let Some(code) = sqlite_code {
        let _ = write!(fields, " sqlite={code}");
    }
    if sqlite_code == Some(libsqlite3_sys::SQLITE_CONSTRAINT_TRIGGER)
        && let Some(text) = sqlite_message.and_then(schema_literal)
    {
        let _ = write!(fields, " trigger={text:?}");
    }
    record("sqlite_error", &fields);
}

/// The text inside a quoted literal of an embedded migration, if `message` is one.
fn schema_literal(message: &str) -> Option<&'static str> {
    let quoted = format!("'{message}'");
    MIGRATOR.iter().find_map(|migration| {
        let sql: &'static str = migration.sql.as_str();
        let start = sql.find(&quoted)? + 1;
        sql.get(start..start + message.len())
    })
}

/// The outcome of database startup, including restore, pre-migration backup and
/// migration. One entry per launch.
pub fn startup(report: &StartupReport) {
    if let Some(restore) = &report.restore {
        record(
            "restore",
            &format!("status={} migrated={}", restore.status, restore.migrated),
        );
    }
    if report.pre_migration_backup.status != "not_attempted" {
        let mut fields = format!("status={}", report.pre_migration_backup.status);
        if let Some(error) = &report.pre_migration_backup.error {
            error_fields(&mut fields, error);
        }
        record("pre_migration_backup", &fields);
    }
    let classification = name(&report.classification);
    if report.ready && classification == "pending" {
        let version = MIGRATOR
            .iter()
            .last()
            .map_or(0, |migration| migration.version);
        record("migration", &format!("status=applied version={version}"));
    }
    let mut fields = format!(
        "status={} classification={classification}",
        if report.ready { "ready" } else { "blocked" }
    );
    if let Some(error) = &report.error {
        error_fields(&mut fields, error);
    }
    if !report.warnings.is_empty() {
        let codes: Vec<_> = report
            .warnings
            .iter()
            .map(|warning| name(&warning.code))
            .collect();
        let _ = write!(fields, " warnings={}", codes.join(","));
    }
    record("startup", &fields);
}

/// The Tauri runtime failed before or while running. `detail` is an error's
/// `Debug` text; only its leading variant names and any HRESULT are kept.
pub fn runtime_failure(started: bool, detail: Option<&str>) -> bool {
    let mut fields = format!("stage={}", if started { "running" } else { "starting" });
    if let Some(detail) = detail {
        let kind = variant_path(detail);
        if !kind.is_empty() {
            let _ = write!(fields, " error={kind}");
        }
        if let Some(code) = hresult(detail) {
            let _ = write!(fields, " hresult={code}");
        }
    }
    record("runtime_failed", &fields)
}

/// Panic location, thread kind and a message only when it is a string literal.
/// Formatted messages can embed runtime values (for example slicing errors
/// quote the string), so they are reduced to any HRESULT they contain.
pub fn panic(info: &PanicHookInfo<'_>) {
    let mut fields = String::new();
    if let Some(location) = info.location() {
        let _ = write!(
            fields,
            "location={}:{}:{}",
            short_source_path(location.file()),
            location.line(),
            location.column()
        );
    }
    let main = std::thread::current().name() == Some("main");
    let _ = write!(fields, " thread={}", if main { "main" } else { "other" });
    if let Some(message) = info.payload().downcast_ref::<&'static str>() {
        let _ = write!(fields, " message={message:?}");
    } else if let Some(code) = info.payload_as_str().and_then(hresult) {
        let _ = write!(fields, " hresult={code}");
    }
    record("panic", fields.trim_start());
}

/// Events without fields, such as a window that could not be focused.
pub fn event(kind: &'static str) {
    record(kind, "");
}

/// `C:\...\registry\src\...\tauri-2.12.3\src\app.rs` becomes
/// `tauri-2.12.3/src/app.rs`, dropping the build machine's directories; this
/// crate's relative paths are unchanged. Without a `src` part, only the file name.
fn short_source_path(file: &str) -> String {
    let parts: Vec<&str> = file.split(['/', '\\']).collect();
    let start = parts
        .iter()
        .rposition(|part| *part == "src")
        .map_or(parts.len().saturating_sub(1), |index| {
            index.saturating_sub(1)
        });
    parts.get(start..).unwrap_or_default().join("/")
}

/// Leading `Variant(Inner(...` names of a `Debug` rendering, without values.
fn variant_path(detail: &str) -> &str {
    let end = detail
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '('))
        .unwrap_or(detail.len())
        .min(120);
    detail.get(..end).unwrap_or_default().trim_end_matches('(')
}

/// `0x80070002` from `HRESULT(0x80070002)`, as Windows errors render.
fn hresult(text: &str) -> Option<&str> {
    let start = text.find("HRESULT(0x")? + "HRESULT(".len();
    let digits = text
        .get(start + 2..)?
        .bytes()
        .take_while(u8::is_ascii_hexdigit)
        .count();
    if digits == 0 {
        return None;
    }
    text.get(start..start + 2 + digits)
}
