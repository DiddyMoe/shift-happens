//! Non-authoritative, process-only history; the coordinator orders commits and maintenance.
use crate::{
    error::{AppError, AppResult, ErrorCode},
    schedule_assignments::{
        Assignment, AssignmentEditable, EditableAssignmentState, read_assignment,
    },
    schedule_editor::ActionKind,
    schedules::Schedule,
};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const HISTORY_LIMIT: usize = 100;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Undo,
    Redo,
}
impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }
}
#[derive(Clone)]
pub(crate) struct Entry {
    pub id: String,
    pub assignment_id: String,
    pub kind: ActionKind,
    pub before: EditableAssignmentState,
    pub after: EditableAssignmentState,
}
#[derive(Clone)]
struct Baseline {
    revision: i64,
    state: AssignmentEditable,
}
#[derive(Clone, Default)]
struct History {
    entries: Vec<Entry>,
    cursor: usize,
    schedule_revision: i64,
    rows: BTreeMap<String, Baseline>,
}
#[derive(Clone, Serialize)]
pub struct Head {
    pub entry_id: String,
    pub direction: Direction,
    pub assignment_id: String,
    pub expected_assignment_revision: i64,
    pub expected_schedule_revision: i64,
    pub action: ActionKind,
}
#[derive(Clone, Default, Serialize)]
pub struct HistoryState {
    pub undo: Option<Head>,
    pub redo: Option<Head>,
    pub undo_count: usize,
    pub redo_count: usize,
    pub unavailable: bool,
}
impl History {
    fn head(&self, direction: Direction) -> Option<Head> {
        let index = match direction {
            Direction::Undo => self.cursor.checked_sub(1)?,
            Direction::Redo => self.cursor,
        };
        let entry = self.entries.get(index)?;
        Some(Head {
            entry_id: entry.id.clone(),
            direction,
            assignment_id: entry.assignment_id.clone(),
            expected_assignment_revision: self.rows.get(&entry.assignment_id)?.revision,
            expected_schedule_revision: self.schedule_revision,
            action: entry.kind,
        })
    }
    fn state(&self) -> HistoryState {
        HistoryState {
            undo: self.head(Direction::Undo),
            redo: self.head(Direction::Redo),
            undo_count: self.cursor,
            redo_count: self.entries.len() - self.cursor,
            unavailable: false,
        }
    }
}
pub struct EditorHistoryStore {
    data: Mutex<BTreeMap<String, History>>,
    disabled: AtomicBool,
}
static STORE: EditorHistoryStore = EditorHistoryStore {
    data: Mutex::new(BTreeMap::new()),
    disabled: AtomicBool::new(false),
};
pub(crate) fn unavailable_warning() -> AppError {
    AppError::new(
        ErrorCode::HistoryUnavailable,
        "The database change was saved. Undo and redo are unavailable; reload the schedule.",
    )
}
fn disabled() -> HistoryState {
    HistoryState {
        unavailable: true,
        ..HistoryState::default()
    }
}
impl EditorHistoryStore {
    // A poisoned mutex/panic makes the entire store unreachable without ever
    // turning an already committed write into an error response. No DB await here.
    fn maintain<R>(
        &self,
        operation: impl FnOnce(&mut BTreeMap<String, History>) -> R,
    ) -> Option<R> {
        if self.disabled.load(Ordering::SeqCst) {
            return None;
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut data = self.data.lock().ok()?;
            Some(operation(&mut data))
        }));
        match outcome {
            Ok(Some(result)) => Some(result),
            _ => {
                self.disabled.store(true, Ordering::SeqCst);
                None
            }
        }
    }
}
pub(crate) fn invalidate_after_commit(schedule_id: &str) -> Option<AppError> {
    STORE
        .maintain(|data| {
            data.remove(schedule_id);
        })
        .is_none()
        .then(unavailable_warning)
}
pub(crate) fn state(schedule_id: &str) -> HistoryState {
    STORE
        .maintain(|data| {
            data.get(schedule_id)
                .map(History::state)
                .unwrap_or_default()
        })
        .unwrap_or_else(disabled)
}
/// Copy narrow tracked rows, release mutex, then read the live DB under coordinator.
/// Delayed caller expectations are checked later and never invalidate valid history.
pub(crate) async fn verify_live(
    tx: &mut Transaction<'_, Sqlite>,
    schedule: &Schedule,
) -> AppResult<bool> {
    let saved = STORE
        .maintain(|data| data.get(&schedule.id).cloned())
        .flatten();
    let Some(saved) = saved else {
        return Ok(true);
    };
    let mut current = saved.schedule_revision == schedule.revision;
    if current {
        for (id, baseline) in &saved.rows {
            match read_assignment(tx, schedule, id).await {
                Ok(row) if row.revision == baseline.revision && row.editable == baseline.state => {}
                _ => {
                    current = false;
                    break;
                }
            }
        }
    }
    if !current {
        invalidate_after_commit(&schedule.id);
    }
    Ok(current)
}
pub(crate) fn expected_entry(
    schedule_id: &str,
    direction: Direction,
    entry_id: &str,
) -> AppResult<Entry> {
    let entry = STORE
        .maintain(|data| {
            let history = data.get(schedule_id)?;
            let head = history.head(direction)?;
            if head.entry_id != entry_id {
                return None;
            }
            let index = match direction {
                Direction::Undo => history.cursor - 1,
                Direction::Redo => history.cursor,
            };
            history.entries.get(index).cloned()
        })
        .flatten();
    entry.ok_or_else(|| {
        AppError::new(
            if STORE.disabled.load(Ordering::SeqCst) {
                ErrorCode::HistoryUnavailable
            } else {
                ErrorCode::HistoryStale
            },
            "This history head is no longer current. Reload undo and redo before trying again.",
        )
    })
}
pub(crate) fn record_after_commit(
    schedule: &Schedule,
    before: &Assignment,
    after: &Assignment,
    kind: ActionKind,
) -> HistoryState {
    STORE
        .maintain(|data| {
            let history = data.entry(schedule.id.clone()).or_default();
            history.entries.truncate(history.cursor);
            history.entries.push(Entry {
                id: uuid::Uuid::new_v4().to_string(),
                assignment_id: after.id.clone(),
                kind,
                before: EditableAssignmentState::capture(before),
                after: EditableAssignmentState::capture(after),
            });
            if history.entries.len() > HISTORY_LIMIT {
                history.entries.remove(0);
            }
            history.cursor = history.entries.len();
            history.schedule_revision = schedule.revision;
            history.rows.insert(
                after.id.clone(),
                Baseline {
                    revision: after.revision,
                    state: after.editable.clone(),
                },
            );
            history
                .rows
                .retain(|id, _| history.entries.iter().any(|e| &e.assignment_id == id));
            history.state()
        })
        .unwrap_or_else(disabled)
}
pub(crate) fn replay_after_commit(
    schedule: &Schedule,
    after: &Assignment,
    direction: Direction,
    entry_id: &str,
) -> HistoryState {
    let result = STORE
        .maintain(|data| {
            let history = data.get_mut(&schedule.id)?;
            if history
                .head(direction)
                .is_none_or(|h| h.entry_id != entry_id)
            {
                data.remove(&schedule.id);
                return None;
            }
            match direction {
                Direction::Undo => history.cursor -= 1,
                Direction::Redo => history.cursor += 1,
            }
            history.schedule_revision = schedule.revision;
            history.rows.insert(
                after.id.clone(),
                Baseline {
                    revision: after.revision,
                    state: after.editable.clone(),
                },
            );
            Some(history.state())
        })
        .flatten();
    result.unwrap_or_else(disabled)
}
