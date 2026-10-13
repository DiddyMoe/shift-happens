//! Narrow RAII wrapper over the *same* libsqlite3-sys linked by SQLx. No domain
//! queries or second data layer. Explicit finish/close errors are never ignored.
use crate::{
    database::{BUSY_TIMEOUT, SqliteEngine, require_safe_engine},
    error::{AppError, AppResult, ErrorCode},
};
use libsqlite3_sys as ffi;
use std::{
    ffi::{CStr, CString},
    path::Path,
    ptr,
    time::{Duration, Instant},
};

// Total elapsed budget includes successful incomplete steps, restart work and
// BUSY/LOCKED waits. A call can exceed the deadline by at most its busy timeout
// plus one finite 128-page step; no unbounded run_to_completion helper is used.
const COPY_TIME_LIMIT: Duration = Duration::from_secs(60);
const COPY_PAGE_BATCH: i32 = 128;

fn sqlite_error() -> AppError {
    AppError::new(
        ErrorCode::BackupCopyFailed,
        "SQLite could not complete the backup operation. No backup was published.",
    )
}
/// The same error, carrying SQLite's result code for the diagnostic log.
fn sqlite_failure(code: i32) -> AppError {
    let mut error = sqlite_error();
    error.context.sqlite_code = Some(code);
    error
}
fn copy_limit_error() -> AppError {
    AppError::new(
        ErrorCode::BackupCopyLimitExceeded,
        "The backup copy exceeded its 60-second work limit. Close other writers or choose another destination folder and try again.",
    )
}
fn checked(code: i32) -> AppResult<()> {
    if code == ffi::SQLITE_OK {
        Ok(())
    } else {
        Err(sqlite_failure(code))
    }
}
pub fn loaded_engine() -> AppResult<SqliteEngine> {
    // SAFETY: SQLite returns static null-terminated engine-identification strings.
    let engine = unsafe {
        SqliteEngine {
            version: CStr::from_ptr(ffi::sqlite3_libversion())
                .to_string_lossy()
                .into_owned(),
            source_id: CStr::from_ptr(ffi::sqlite3_sourceid())
                .to_string_lossy()
                .into_owned(),
        }
    };
    require_safe_engine(&engine)?;
    Ok(engine)
}

pub struct Connection(*mut ffi::sqlite3);
impl Connection {
    pub fn open(path: &Path, readonly: bool) -> AppResult<Self> {
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStrExt;
            CString::new(path.as_os_str().as_bytes()).map_err(|_| sqlite_error())?
        };
        #[cfg(windows)]
        let name =
            CString::new(path.to_str().ok_or_else(sqlite_error)?).map_err(|_| sqlite_error())?;
        let mut handle = ptr::null_mut();
        let flags = if readonly {
            ffi::SQLITE_OPEN_READONLY
        } else {
            ffi::SQLITE_OPEN_READWRITE
        } | ffi::SQLITE_OPEN_NOFOLLOW
            | ffi::SQLITE_OPEN_PRIVATECACHE
            | ffi::SQLITE_OPEN_NOMUTEX;
        // SAFETY: valid UTF-8/native byte pathname; creation and URI processing are
        // disabled. This connection stays exclusively on its blocking thread.
        let code = unsafe { ffi::sqlite3_open_v2(name.as_ptr(), &mut handle, flags, ptr::null()) };
        let connection = Self(handle);
        checked(code)?;
        checked(unsafe {
            ffi::sqlite3_busy_timeout(connection.0, BUSY_TIMEOUT.as_millis() as i32)
        })?;
        Ok(connection)
    }
    pub fn statement(&self, sql: &CStr) -> AppResult<Statement<'_>> {
        let mut statement = ptr::null_mut();
        // SAFETY: connection is live, sql is static and terminated; SQLite owns
        // the result until Statement finalizes it, before this connection closes.
        let code = unsafe {
            ffi::sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut statement, ptr::null_mut())
        };
        if code != ffi::SQLITE_OK {
            if !statement.is_null() {
                unsafe {
                    ffi::sqlite3_finalize(statement);
                }
            }
            return Err(sqlite_failure(code));
        }
        Ok(Statement {
            pointer: statement,
            _connection: self,
        })
    }
    pub fn single_text(&self, sql: &CStr) -> AppResult<String> {
        let mut statement = self.statement(sql)?;
        if !statement.next()? {
            return Err(sqlite_error());
        }
        let value = statement.text(0)?;
        if statement.next()? {
            return Err(sqlite_error());
        }
        statement.finish()?;
        Ok(value)
    }
    pub fn close(mut self) -> AppResult<()> {
        // SAFETY: all borrowed statements/backup handles have finished.
        checked(unsafe { ffi::sqlite3_close(self.0) })?;
        self.0 = ptr::null_mut();
        Ok(())
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // All dependent handles are lexical borrows and have already dropped.
            unsafe {
                ffi::sqlite3_close(self.0);
            }
        }
    }
}
pub struct Statement<'a> {
    pointer: *mut ffi::sqlite3_stmt,
    _connection: &'a Connection,
}
impl Statement<'_> {
    pub fn next(&mut self) -> AppResult<bool> {
        match unsafe { ffi::sqlite3_step(self.pointer) } {
            ffi::SQLITE_ROW => Ok(true),
            ffi::SQLITE_DONE => Ok(false),
            code => Err(sqlite_failure(code)),
        }
    }
    pub fn text(&self, column: i32) -> AppResult<String> {
        // Only fixed PRAGMA columns, read immediately while the statement is on
        // its current row. Never inspect employee/domain contents or error SQL.
        let text = unsafe { ffi::sqlite3_column_text(self.pointer, column) };
        if text.is_null() {
            return Err(sqlite_error());
        }
        Ok(unsafe { CStr::from_ptr(text.cast()) }
            .to_string_lossy()
            .into_owned())
    }
    pub fn integer(&self, column: i32) -> i64 {
        unsafe { ffi::sqlite3_column_int64(self.pointer, column) }
    }
    pub fn finish(mut self) -> AppResult<()> {
        let pointer = std::mem::replace(&mut self.pointer, ptr::null_mut());
        checked(unsafe { ffi::sqlite3_finalize(pointer) })
    }
}
impl Drop for Statement<'_> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            unsafe {
                ffi::sqlite3_finalize(self.pointer);
            }
        }
    }
}
struct OnlineBackup(*mut ffi::sqlite3_backup);
impl Drop for OnlineBackup {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ffi::sqlite3_backup_finish(self.0);
            }
        }
    }
}
pub fn online_copy(source: &Connection, destination: &mut Connection) -> AppResult<()> {
    let started = Instant::now();
    // SAFETY: distinct live connections; no destination API is used until finish.
    let pointer = unsafe {
        ffi::sqlite3_backup_init(destination.0, c"main".as_ptr(), source.0, c"main".as_ptr())
    };
    if pointer.is_null() {
        return Err(sqlite_error());
    }
    let mut backup = OnlineBackup(pointer);
    loop {
        if started.elapsed() >= COPY_TIME_LIMIT {
            return Err(copy_limit_error());
        }
        let result = unsafe { ffi::sqlite3_backup_step(backup.0, COPY_PAGE_BATCH) };
        if started.elapsed() >= COPY_TIME_LIMIT {
            return Err(copy_limit_error());
        }
        match result {
            ffi::SQLITE_DONE => break,
            ffi::SQLITE_OK => {}
            ffi::SQLITE_BUSY | ffi::SQLITE_LOCKED => std::thread::sleep(Duration::from_millis(10)),
            code => return Err(sqlite_failure(code)),
        }
    }
    let pointer = std::mem::replace(&mut backup.0, ptr::null_mut());
    checked(unsafe { ffi::sqlite3_backup_finish(pointer) })
}
