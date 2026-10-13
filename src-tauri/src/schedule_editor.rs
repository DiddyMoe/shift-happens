//! Explicit schedule actions; all eligibility authorization happens in the committing view.
use crate::{
    editor_analysis::{self, Analysis, CandidateStatus},
    editor_history::{self, Direction, HistoryState},
    employees::{check_revision, normalize_optional_notes},
    error::{AppError, AppResult, CommandResult, DIAGNOSTIC_LIMIT, ErrorCode},
    metadata::MetadataClock,
    schedule_assignments::{
        self, Assignment, AssignmentWriteIntent, EditableTarget, EmployeeTarget,
    },
    schedule_write::SCHEDULE_WRITES,
    schedules::{self, Schedule},
    text::{normalize_required_text, stable_text_order_key},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Assign,
    Open,
    Note,
    Lock,
    Unlock,
}
impl ActionKind {
    pub(crate) fn is_lock(self) -> bool {
        matches!(self, Self::Lock | Self::Unlock)
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorAction {
    Assign { employee_id: String },
    Open,
    Note { notes: Option<String> },
    Lock,
    Unlock,
}
impl EditorAction {
    fn kind(&self) -> ActionKind {
        match self {
            Self::Assign { .. } => ActionKind::Assign,
            Self::Open => ActionKind::Open,
            Self::Note { .. } => ActionKind::Note,
            Self::Lock => ActionKind::Lock,
            Self::Unlock => ActionKind::Unlock,
        }
    }
    fn target(&self) -> EditableTarget {
        match self {
            Self::Assign { employee_id } => {
                EditableTarget::Employee(EmployeeTarget::Assign(employee_id.clone()))
            }
            Self::Open => EditableTarget::Employee(EmployeeTarget::Open),
            Self::Note { notes } => EditableTarget::Note(notes.clone()),
            Self::Lock => EditableTarget::Lock(true),
            Self::Unlock => EditableTarget::Lock(false),
        }
    }
    fn same(&self, row: &Assignment) -> AppResult<bool> {
        Ok(match self {
            Self::Assign { employee_id } => {
                if normalize_required_text(employee_id)? != *employee_id {
                    return Err(AppError::new(
                        ErrorCode::InvalidEmployee,
                        "Choose an employee from the candidate list.",
                    ));
                }
                row.editable.assigned_employee_id.as_ref() == Some(employee_id)
            }
            Self::Open => row.editable.open_shift,
            Self::Note { notes } => normalize_optional_notes(notes.clone())? == row.editable.notes,
            Self::Lock => row.editable.locked,
            Self::Unlock => !row.editable.locked,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub schedule_id: String,
    pub assignment_id: String,
    pub expected_assignment_revision: i64,
    pub expected_schedule_revision: i64,
    pub action: EditorAction,
    pub acknowledged_warning_ids: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayRequest {
    pub schedule_id: String,
    pub assignment_id: String,
    pub entry_id: String,
    pub direction: Direction,
    pub expected_assignment_revision: i64,
    pub expected_schedule_revision: i64,
    pub acknowledged_warning_ids: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidatesRequest {
    pub schedule_id: String,
    pub assignment_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    pub schedule_id: String,
}
#[derive(Serialize)]
pub struct Candidate {
    pub employee_id: String,
    pub display_name: String,
    #[serde(flatten)]
    pub analysis: Analysis,
}
#[derive(Serialize)]
pub struct Candidates {
    pub schedule_id: String,
    pub assignment_id: String,
    pub assignment_revision: i64,
    pub schedule_revision: i64,
    pub setting_revisions: BTreeMap<String, i64>,
    pub candidates: Vec<Candidate>,
    pub open: Analysis,
}
#[derive(Serialize)]
pub struct EditorOutcome {
    pub schedule: Schedule,
    pub assignment: Assignment,
    pub history: HistoryState,
    pub analysis: Analysis,
    pub action: Option<EditorAction>,
    pub replay: Option<ReplayContext>,
}
#[derive(Serialize)]
pub struct ReplayContext {
    pub entry_id: String,
    pub direction: Direction,
    pub target_employee_id: Option<String>,
    pub target_open: bool,
    pub expected_assignment_revision: i64,
    pub expected_schedule_revision: i64,
}
pub(crate) fn acknowledgement(ids: Option<Vec<String>>, limit: usize) -> AppResult<Vec<String>> {
    let mut ids = ids.unwrap_or_default();
    if ids.len() > limit
        || ids
            .iter()
            .any(|id| id.len() > crate::error::DIAGNOSTIC_SOURCE_ID_BYTES)
    {
        return Err(AppError::new(
            ErrorCode::InvalidAcknowledgement,
            "Supply the exact current warning IDs.",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    if ids.iter().any(|id| !seen.insert(id)) {
        return Err(AppError::new(
            ErrorCode::InvalidAcknowledgement,
            "Duplicate warning acknowledgements are invalid.",
        ));
    }
    ids.sort();
    Ok(ids)
}
fn confirmed(analysis: &Analysis, supplied: &[String]) -> bool {
    let mut required: Vec<_> = analysis
        .warnings
        .iter()
        .map(|w| w.warning_id.clone())
        .collect();
    required.sort();
    required == supplied
}
fn response(outcome: EditorOutcome, changed: bool) -> CommandResult<EditorOutcome> {
    if changed {
        let warnings = if outcome.history.unavailable {
            vec![editor_history::unavailable_warning()]
        } else {
            Vec::new()
        };
        CommandResult::Success {
            data: outcome,
            warnings,
        }
    } else {
        CommandResult::NoChange { data: outcome }
    }
}
fn blocked(analysis: &Analysis) -> AppError {
    let mut error = AppError::new(
        ErrorCode::EmployeeIneligible,
        "This replacement has hard blockers. Review candidate details and reload if rules changed.",
    );
    error.context.dependency_ids = Some(
        analysis
            .blockers
            .iter()
            .take(crate::error::DIAGNOSTIC_SOURCE_ID_LIMIT)
            .map(|f| f.code.clone())
            .collect(),
    );
    error.context.dependency_count = Some(analysis.blockers.len() as i64);
    error.context.diagnostics = serde_json::to_value(analysis).ok();
    error
}
pub async fn candidates(pool: &SqlitePool, request: CandidatesRequest) -> AppResult<Candidates> {
    let mut tx = pool.begin().await?;
    let schedule = schedules::read_schedule(&mut tx, &request.schedule_id).await?;
    schedules::require_draft(&schedule)?;
    let row =
        schedule_assignments::read_assignment(&mut tx, &schedule, &request.assignment_id).await?;
    let sources = editor_analysis::load(&mut tx, &schedule)
        .await
        .map_err(editor_analysis::editor_error)?;
    // rollback consumes the pooled transaction and releases its connection. The
    // pure candidate loop and interval expansion own data only, on a blocking worker.
    tx.rollback().await?;
    tauri::async_runtime::spawn_blocking(move || {
        let replacement = sources.build(&schedule, &row.id)?;
        let mut candidates = Vec::new();
        let open = replacement.evaluate(None, "replace_assignment")?;
        let mut diagnostic_count = open.blockers.len() + open.warnings.len();
        for e in replacement
            .rules
            .employees
            .iter()
            .filter(|e| editor_analysis::active(e))
        {
            let analysis = replacement.evaluate(Some(&e.id), "replace_assignment")?;
            diagnostic_count = diagnostic_count
                .checked_add(analysis.blockers.len() + analysis.warnings.len())
                .ok_or_else(|| editor_analysis::output_limit(usize::MAX))?;
            if diagnostic_count > DIAGNOSTIC_LIMIT {
                return Err(editor_analysis::output_limit(diagnostic_count));
            }
            candidates.push(Candidate {
                employee_id: e.id.clone(),
                display_name: e.data.display_name.clone(),
                analysis,
            });
        }
        candidates.sort_by(|a, b| {
            (stable_text_order_key(&a.display_name), &a.employee_id)
                .cmp(&(stable_text_order_key(&b.display_name), &b.employee_id))
        });
        Ok(Candidates {
            schedule_id: schedule.id,
            assignment_id: row.id,
            assignment_revision: row.revision,
            schedule_revision: schedule.revision,
            setting_revisions: replacement.settings_revisions,
            candidates,
            open,
        })
    })
    .await
    .map_err(|_| {
        AppError::new(
            ErrorCode::ConfigurationInvalid,
            "Candidate analysis could not complete.",
        )
    })?
    .map_err(editor_analysis::editor_error)
}
pub async fn history(pool: &SqlitePool, request: HistoryRequest) -> AppResult<HistoryState> {
    let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
    let schedule = schedules::read_schedule(&mut tx, &request.schedule_id).await?;
    editor_history::verify_live(&mut tx, &schedule).await?;
    let result = editor_history::state(&schedule.id);
    tx.rollback().await?;
    Ok(result)
}
pub async fn edit(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: ActionRequest,
) -> CommandResult<EditorOutcome> {
    async fn apply(
        pool: &SqlitePool,
        clock: &dyn MetadataClock,
        request: ActionRequest,
    ) -> AppResult<CommandResult<EditorOutcome>> {
        let ids = acknowledgement(request.acknowledged_warning_ids, DIAGNOSTIC_LIMIT)?;
        let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
        let schedule = schedules::read_schedule(&mut tx, &request.schedule_id).await?;
        check_revision(
            &schedule.id,
            schedule.revision,
            request.expected_schedule_revision,
        )?;
        schedules::require_draft(&schedule)?;
        let row = schedule_assignments::read_assignment(&mut tx, &schedule, &request.assignment_id)
            .await?;
        check_revision(&row.id, row.revision, request.expected_assignment_revision)?;
        editor_history::verify_live(&mut tx, &schedule).await?;
        if request.action.same(&row)? {
            tx.rollback().await?;
            return Ok(response(
                EditorOutcome {
                    history: editor_history::state(&schedule.id),
                    schedule,
                    assignment: row,
                    analysis: Analysis::default(),
                    action: Some(request.action),
                    replay: None,
                },
                false,
            ));
        }
        if row.editable.locked && !matches!(request.action, EditorAction::Unlock) {
            return Err(AppError::new(
                ErrorCode::AssignmentLocked,
                "Unlock this assignment before changing it.",
            ));
        }
        let analysis = match &request.action {
            EditorAction::Assign { .. } | EditorAction::Open => {
                let target = match &request.action {
                    EditorAction::Assign { employee_id } => Some(employee_id.clone()),
                    _ => None,
                };
                let sources = editor_analysis::load(&mut tx, &schedule).await?;
                let owned_schedule = schedule.clone();
                let assignment_id = row.id.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    sources
                        .build(&owned_schedule, &assignment_id)?
                        .evaluate(target.as_deref(), "replace_assignment")
                })
                .await
                .map_err(|_| {
                    AppError::new(
                        ErrorCode::ConfigurationInvalid,
                        "Assignment analysis could not complete.",
                    )
                })??
            }
            _ => Analysis::default(),
        };
        if matches!(analysis.status, CandidateStatus::Blocked) {
            return Err(blocked(&analysis));
        }
        if !confirmed(&analysis, &ids) {
            tx.rollback().await?;
            return Ok(CommandResult::RequiresConfirmation {
                data: EditorOutcome {
                    history: editor_history::state(&schedule.id),
                    schedule,
                    assignment: row,
                    analysis,
                    action: Some(request.action),
                    replay: None,
                },
            });
        }
        let (after, changed) = schedule_assignments::write_assignment(
            &mut tx,
            clock,
            AssignmentWriteIntent::Edit {
                schedule_id: &schedule.id,
                assignment_id: &row.id,
                expected_schedule_revision: schedule.revision,
                expected_assignment_revision: row.revision,
                target: request.action.target(),
            },
        )
        .await?;
        let committed_schedule = schedules::read_schedule(&mut tx, &schedule.id).await?;
        let mut outcome = EditorOutcome {
            schedule: committed_schedule,
            assignment: after,
            history: HistoryState::default(),
            analysis,
            action: Some(request.action.clone()),
            replay: None,
        };
        if changed {
            tx.commit().await?;
            outcome.history = editor_history::record_after_commit(
                &outcome.schedule,
                &row,
                &outcome.assignment,
                request.action.kind(),
            );
        } else {
            tx.rollback().await?;
            outcome.history = editor_history::state(&schedule.id);
        }
        Ok(response(outcome, changed))
    }
    match apply(pool, clock, request).await {
        Ok(result) => result,
        Err(error) => Err(editor_analysis::editor_error(error)).into(),
    }
}
pub async fn replay(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: ReplayRequest,
) -> CommandResult<EditorOutcome> {
    async fn apply(
        pool: &SqlitePool,
        clock: &dyn MetadataClock,
        request: ReplayRequest,
    ) -> AppResult<CommandResult<EditorOutcome>> {
        let ids = acknowledgement(request.acknowledged_warning_ids, DIAGNOSTIC_LIMIT)?;
        let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
        let schedule = schedules::read_schedule(&mut tx, &request.schedule_id).await?;
        schedules::require_draft(&schedule)?;
        if !editor_history::verify_live(&mut tx, &schedule).await? {
            return Err(AppError::new(
                ErrorCode::HistoryStale,
                "The saved schedule changed outside this history. Undo and redo were cleared.",
            ));
        }
        let entry =
            editor_history::expected_entry(&schedule.id, request.direction, &request.entry_id)?;
        check_revision(
            &schedule.id,
            schedule.revision,
            request.expected_schedule_revision,
        )?;
        if entry.assignment_id != request.assignment_id {
            return Err(AppError::new(
                ErrorCode::HistoryStale,
                "Reload the current history head.",
            ));
        }
        let row =
            schedule_assignments::read_assignment(&mut tx, &schedule, &entry.assignment_id).await?;
        check_revision(&row.id, row.revision, request.expected_assignment_revision)?;
        let (expected, target) = match request.direction {
            Direction::Undo => (&entry.after, &entry.before),
            Direction::Redo => (&entry.before, &entry.after),
        };
        if &row.editable != expected.state() {
            editor_history::invalidate_after_commit(&schedule.id);
            return Err(AppError::new(
                ErrorCode::HistoryStale,
                "The current editable state differs from this history. Undo and redo were cleared.",
            ));
        }
        if row.editable.locked && !entry.kind.is_lock() {
            return Err(AppError::new(
                ErrorCode::AssignmentLocked,
                "Unlock this assignment before replaying this action.",
            ));
        }
        let changes_employee = row.editable.assigned_employee_id
            != target.state().assigned_employee_id
            || row.editable.open_shift != target.state().open_shift;
        let context = ReplayContext {
            entry_id: entry.id.clone(),
            direction: request.direction,
            target_employee_id: target.state().assigned_employee_id.clone(),
            target_open: target.state().open_shift,
            expected_assignment_revision: row.revision,
            expected_schedule_revision: schedule.revision,
        };
        let analysis = if changes_employee {
            let sources = editor_analysis::load(&mut tx, &schedule).await?;
            let owned_schedule = schedule.clone();
            let assignment_id = row.id.clone();
            let employee_id = context.target_employee_id.clone();
            let operation = format!("replay:{}:{}", entry.id, request.direction.as_str());
            tauri::async_runtime::spawn_blocking(move || {
                sources
                    .build(&owned_schedule, &assignment_id)?
                    .evaluate(employee_id.as_deref(), &operation)
            })
            .await
            .map_err(|_| {
                AppError::new(
                    ErrorCode::ConfigurationInvalid,
                    "Replay analysis could not complete.",
                )
            })??
        } else {
            Analysis::default()
        };
        if matches!(analysis.status, CandidateStatus::Blocked) {
            return Err(blocked(&analysis));
        }
        if !confirmed(&analysis, &ids) {
            tx.rollback().await?;
            return Ok(CommandResult::RequiresConfirmation {
                data: EditorOutcome {
                    history: editor_history::state(&schedule.id),
                    schedule,
                    assignment: row,
                    analysis,
                    action: None,
                    replay: Some(context),
                },
            });
        }
        let (after, changed) = schedule_assignments::write_assignment(
            &mut tx,
            clock,
            AssignmentWriteIntent::Replay {
                schedule_id: &schedule.id,
                assignment_id: &row.id,
                expected_schedule_revision: schedule.revision,
                expected_assignment_revision: row.revision,
                captured: target,
                lock_transition: entry.kind.is_lock(),
            },
        )
        .await?;
        let committed_schedule = schedules::read_schedule(&mut tx, &schedule.id).await?;
        let mut outcome = EditorOutcome {
            schedule: committed_schedule,
            assignment: after,
            history: HistoryState::default(),
            analysis,
            action: None,
            replay: Some(context),
        };
        if changed {
            tx.commit().await?;
            outcome.history = editor_history::replay_after_commit(
                &outcome.schedule,
                &outcome.assignment,
                request.direction,
                &entry.id,
            );
        } else {
            tx.rollback().await?;
            outcome.history = editor_history::state(&schedule.id);
        }
        Ok(response(outcome, changed))
    }
    let schedule_id = request.schedule_id.clone();
    match apply(pool, clock, request).await {
        Ok(result) => result,
        Err(mut error) => {
            error.context.history = serde_json::to_value(editor_history::state(&schedule_id)).ok();
            Err(editor_analysis::editor_error(error)).into()
        }
    }
}
