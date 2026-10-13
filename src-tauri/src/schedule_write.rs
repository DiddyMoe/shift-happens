use crate::error::AppResult;
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::ops::DerefMut;
use tauri::async_runtime::Mutex;

/// One process-wide gate for outer schedule/current operations. Caller-owned
/// transactions are passed to inner services, which never acquire this gate.
pub struct ScheduleWriteCoordinator {
    gate: Mutex<()>,
}
pub static SCHEDULE_WRITES: ScheduleWriteCoordinator = ScheduleWriteCoordinator {
    gate: Mutex::const_new(()),
};

impl ScheduleWriteCoordinator {
    /// Waits for in-flight schedule writes without opening a transaction.
    /// Restore holds this while it closes the pool.
    pub async fn lock(&self) -> impl DerefMut<Target = ()> + '_ {
        self.gate.lock().await
    }
    /// SQLx 0.9 Pool::begin_with checks out one connection; SQLite's worker
    /// executes this statement once at depth zero. Transaction's Drop rolls
    /// back on cancellation/error. Keep the returned guard until commit/rollback.
    pub async fn begin(
        &self,
        pool: &SqlitePool,
    ) -> AppResult<(
        impl DerefMut<Target = ()> + '_,
        Transaction<'static, Sqlite>,
    )> {
        let guard = self.gate.lock().await;
        let tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        Ok((guard, tx))
    }
}
