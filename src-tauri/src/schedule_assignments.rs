//! Coverage-derived storage only. No IPC mutation is registered here. Prompt 9
//! supplies an eligible slot plan; Prompt 10 authorizes editable targets/replay.
use crate::{
    coverage::{self, DayType},
    employees::{
        self, ActiveStatus, IdRequest, TimeOfDay, advance_source_metadata, check_revision,
        normalize_optional_label, normalize_optional_notes, validate_sort_order, verify_metadata,
    },
    error::{AppError, AppResult, ErrorCode},
    generation_snapshot::{ASSIGNMENT_LIMIT, GenerationSettingsSnapshot},
    holidays::{self, CoverageBehavior, HolidaySet},
    local_values::{LocalDate, LocalDateTime, LocalInterval},
    metadata::{MetadataClock, clock_timestamp},
    positions,
    schedules::{self, Schedule},
    shift_templates::{self, ShiftTemplateData},
    shift_time::{calculate_shift_time, normalize_color},
    text::normalize_required_text,
};
use serde::Serialize;
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum AssignmentSource {
    Generated,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct AssignmentStructure {
    pub schedule_date: String,
    pub actual_start_datetime: String,
    pub actual_end_datetime: String,
    pub coverage_template_id: String,
    pub coverage_item_id: String,
    pub shift_position_id: String,
    pub shift_template_id: String,
    pub coverage_template_seed_key_snapshot: Option<String>,
    pub coverage_template_name_snapshot: String,
    pub coverage_day_type_snapshot: DayType,
    pub coverage_item_seed_key_snapshot: Option<String>,
    pub coverage_item_display_snapshot: Option<String>,
    pub coverage_sort_order_snapshot: i64,
    pub coverage_required_count_snapshot: i64,
    pub coverage_slot_index_snapshot: i64,
    pub coverage_start_offset_snapshot: i64,
    pub coverage_end_offset_snapshot: i64,
    pub position_seed_key_snapshot: Option<String>,
    pub position_name_snapshot: String,
    pub position_display_snapshot: Option<String>,
    pub shift_seed_key_snapshot: Option<String>,
    pub shift_name_snapshot: String,
    pub shift_display_snapshot: Option<String>,
    pub shift_start_snapshot: String,
    pub shift_end_snapshot: String,
    pub shift_category_snapshot: TimeOfDay,
    pub shift_color_snapshot: Option<String>,
    pub shift_lunch_minutes_snapshot: i64,
    pub shift_paid_minutes_snapshot: i64,
    pub shift_recovery_snapshot: bool,
    pub shift_night_snapshot: bool,
    pub holiday_id: Option<String>,
    pub holiday_seed_key_snapshot: Option<String>,
    pub holiday_name_snapshot: Option<String>,
    pub holiday_set_snapshot: Option<HolidaySet>,
    pub holiday_coverage_behavior_snapshot: Option<CoverageBehavior>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct AssignmentEditable {
    pub assigned_employee_id: Option<String>,
    pub employee_first_name_snapshot: Option<String>,
    pub employee_last_name_snapshot: Option<String>,
    pub employee_display_name_snapshot: Option<String>,
    pub locked: bool,
    pub open_shift: bool,
    pub manually_edited: bool,
    pub assignment_source: AssignmentSource,
    pub notes: Option<String>,
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Assignment {
    pub id: String,
    pub schedule_id: String,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub structure: AssignmentStructure,
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub editable: AssignmentEditable,
    pub created_at: String,
    pub updated_at: String,
    pub revision: i64,
}
#[derive(Debug, Serialize)]
pub struct AssignmentView {
    #[serde(flatten)]
    pub assignment: Assignment,
    pub operational_day_label: &'static str,
}
fn invalid() -> AppError {
    AppError::new(
        ErrorCode::InvalidAssignment,
        "The assignment's stored slot, interval, or snapshots are invalid; no data was changed.",
    )
}
fn stored_invalid() -> AppError {
    AppError::new(
        ErrorCode::ConfigurationInvalid,
        "A stored assignment has missing or corrupt structural data; it has not been repaired.",
    )
}
fn exact_text(value: &str) -> AppResult<()> {
    if normalize_required_text(value)? != value {
        return Err(invalid());
    }
    Ok(())
}
fn exact_optional(value: &Option<String>) -> AppResult<()> {
    if normalize_optional_label(value.clone())? != *value {
        return Err(invalid());
    }
    Ok(())
}
impl AssignmentStructure {
    fn snapped_shift(&self) -> ShiftTemplateData {
        ShiftTemplateData {
            name: self.shift_name_snapshot.clone(),
            display_label: self.shift_display_snapshot.clone(),
            start_time: self.shift_start_snapshot.clone(),
            end_time: self.shift_end_snapshot.clone(),
            time_of_day: self.shift_category_snapshot,
            color_hex: self.shift_color_snapshot.clone(),
            requires_recovery_after_shift: self.shift_recovery_snapshot,
            is_night_shift: self.shift_night_snapshot,
            active_status: ActiveStatus::Active,
            notes: None,
        }
    }
    pub fn verify(&self, schedule: &Schedule) -> AppResult<()> {
        let date = LocalDate::parse(&self.schedule_date)?;
        if date < LocalDate::parse(&schedule.start_date)?
            || date > LocalDate::parse(&schedule.end_date)?
            || !(1..=100).contains(&self.coverage_required_count_snapshot)
            || !(1..=self.coverage_required_count_snapshot)
                .contains(&self.coverage_slot_index_snapshot)
        {
            return Err(invalid());
        }
        for value in [
            &self.coverage_template_id,
            &self.coverage_item_id,
            &self.shift_position_id,
            &self.shift_template_id,
            &self.coverage_template_name_snapshot,
            &self.position_name_snapshot,
            &self.shift_name_snapshot,
        ] {
            exact_text(value)?;
        }
        for value in [
            &self.coverage_template_seed_key_snapshot,
            &self.coverage_item_seed_key_snapshot,
            &self.coverage_item_display_snapshot,
            &self.position_seed_key_snapshot,
            &self.position_display_snapshot,
            &self.shift_seed_key_snapshot,
            &self.shift_display_snapshot,
            &self.holiday_seed_key_snapshot,
            &self.holiday_name_snapshot,
        ] {
            exact_optional(value)?;
        }
        validate_sort_order(self.coverage_sort_order_snapshot)?;
        if normalize_color(self.shift_color_snapshot.clone())? != self.shift_color_snapshot {
            return Err(invalid());
        }
        let interval = coverage::coverage_interval(
            date,
            &self.snapped_shift(),
            self.coverage_start_offset_snapshot,
            self.coverage_end_offset_snapshot,
        )?;
        let calculated =
            calculate_shift_time(&self.shift_start_snapshot, &self.shift_end_snapshot)?;
        if interval.actual_start != self.actual_start_datetime
            || interval.actual_end != self.actual_end_datetime
            || calculated.unpaid_lunch_minutes != self.shift_lunch_minutes_snapshot
            || calculated.paid_minutes != self.shift_paid_minutes_snapshot
        {
            return Err(invalid());
        }
        LocalInterval::new(
            LocalDateTime::parse(&self.actual_start_datetime)?,
            LocalDateTime::parse(&self.actual_end_datetime)?,
        )?;
        match &self.holiday_id {
            None if self.holiday_seed_key_snapshot.is_none()
                && self.holiday_name_snapshot.is_none()
                && self.holiday_set_snapshot.is_none()
                && self.holiday_coverage_behavior_snapshot.is_none() => {}
            Some(id)
                if self.holiday_name_snapshot.is_some()
                    && self.holiday_set_snapshot.is_some()
                    && self.holiday_coverage_behavior_snapshot.is_some() =>
            {
                exact_text(id)?;
            }
            _ => return Err(invalid()),
        }
        let custom = self.holiday_coverage_behavior_snapshot == Some(CoverageBehavior::Custom);
        if (custom && self.coverage_day_type_snapshot != DayType::HolidayCustom)
            || (!custom && self.coverage_day_type_snapshot.as_str() != date.weekday().as_str())
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl AssignmentEditable {
    fn verify(&self) -> AppResult<()> {
        let names = [
            &self.employee_first_name_snapshot,
            &self.employee_last_name_snapshot,
            &self.employee_display_name_snapshot,
        ];
        if self.open_shift {
            if self.assigned_employee_id.is_some() || names.iter().any(|n| n.is_some()) {
                return Err(invalid());
            }
        } else {
            exact_text(self.assigned_employee_id.as_deref().ok_or_else(invalid)?)?;
            for name in names {
                exact_text(name.as_deref().ok_or_else(invalid)?)?;
            }
        }
        if normalize_optional_notes(self.notes.clone())? != self.notes {
            return Err(invalid());
        }
        Ok(())
    }
}
pub fn verify_assignment(row: &Assignment, schedule: &Schedule) -> AppResult<()> {
    exact_text(&row.id)?;
    if row.schedule_id != schedule.id {
        return Err(invalid());
    }
    row.structure.verify(schedule)?;
    row.editable.verify()?;
    verify_metadata(&row.created_at, &row.updated_at, &None, row.revision)
}
async fn verify_retained_links(
    tx: &mut Transaction<'_, Sqlite>,
    row: &Assignment,
) -> AppResult<()> {
    let s = &row.structure;
    // Source identities are retained, but their current names, clocks, links and
    // active state are not historical authority. All relationships inside this
    // slot are checked against its immutable snapshots, not today's configuration.
    let complete: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM coverage_templates WHERE id=?1) AND EXISTS(SELECT 1 FROM coverage_template_items WHERE id=?2) AND EXISTS(SELECT 1 FROM shift_positions WHERE id=?3) AND EXISTS(SELECT 1 FROM shift_templates WHERE id=?4) AND (?5 IS NULL OR EXISTS(SELECT 1 FROM holidays WHERE id=?5)) AND (?6 IS NULL OR EXISTS(SELECT 1 FROM employees WHERE id=?6))")
        .bind(&s.coverage_template_id).bind(&s.coverage_item_id).bind(&s.shift_position_id).bind(&s.shift_template_id)
        .bind(&s.holiday_id).bind(&row.editable.assigned_employee_id).fetch_one(&mut **tx).await?;
    if !complete {
        return Err(invalid());
    }
    Ok(())
}
pub async fn read_assignment(
    tx: &mut Transaction<'_, Sqlite>,
    schedule: &Schedule,
    id: &str,
) -> AppResult<Assignment> {
    exact_text(id)?;
    let row = sqlx::query_as::<_, Assignment>(
        "SELECT * FROM schedule_assignments WHERE schedule_id=? AND id=?",
    )
    .bind(&schedule.id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "The schedule assignment could not be found.",
        )
    })?;
    verify_assignment(&row, schedule).map_err(|_| stored_invalid())?;
    verify_retained_links(tx, &row)
        .await
        .map_err(|_| stored_invalid())?;
    Ok(row)
}
pub async fn list_assignments(
    pool: &SqlitePool,
    request: IdRequest,
) -> AppResult<Vec<AssignmentView>> {
    let mut tx = pool.begin().await?;
    let schedule = schedules::read_schedule(&mut tx, &request.id).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM schedule_assignments WHERE schedule_id=?")
            .bind(&schedule.id)
            .fetch_one(&mut *tx)
            .await?;
    if count > ASSIGNMENT_LIMIT as i64 {
        return Err(crate::local_values::input_limit_error(
            "assignments",
            ASSIGNMENT_LIMIT,
            count as usize,
        ));
    }
    let rows=sqlx::query_as::<_,Assignment>("SELECT * FROM schedule_assignments WHERE schedule_id=? ORDER BY schedule_date,coverage_sort_order_snapshot,coverage_slot_index_snapshot,actual_start_datetime,id")
        .bind(&schedule.id).fetch_all(&mut *tx).await?;
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        verify_assignment(&row, &schedule).map_err(|_| stored_invalid())?;
        views.push(AssignmentView {
            operational_day_label: LocalDate::parse(&row.structure.schedule_date)?
                .weekday()
                .english_label(),
            assignment: row,
        });
    }
    tx.rollback().await?;
    Ok(views)
}

/// Cannot be constructed from IPC or a raw snapshot. Sources are loaded and
/// validated in the caller's consistent transaction before a slot is trusted.
pub struct ValidatedCoverageSlot {
    structure: AssignmentStructure,
    coverage_revision: i64,
}
impl ValidatedCoverageSlot {
    /// Retained structural snapshots are authoritative after generation. Current
    /// source activity, clocks and labels cannot rewrite an existing slot.
    pub(crate) fn from_assignment(row: &Assignment, schedule: &Schedule) -> AppResult<Self> {
        verify_assignment(row, schedule)?;
        Ok(Self {
            structure: row.structure.clone(),
            coverage_revision: 0,
        })
    }
    /// In-memory creation from the same transaction's bounded source snapshot.
    /// Reuses the canonical source validators; no per-slot database reads.
    pub(crate) fn from_sources(
        operational_date: LocalDate,
        item: &coverage::CoverageItem,
        coverage: &coverage::CoverageTemplate,
        position: &positions::Position,
        shift: &shift_templates::ShiftTemplate,
        holiday: Option<&holidays::Holiday>,
        slot_index: i64,
    ) -> AppResult<Self> {
        coverage::verify_item(item)?;
        coverage::verify_template(coverage)?;
        positions::verify_position(position)?;
        shift_templates::verify_template(shift)?;
        for (active, archived) in [
            (item.data.active_status, &item.archived_at),
            (coverage.data.active_status, &coverage.archived_at),
            (position.data.active_status, &position.archived_at),
            (shift.data.active_status, &shift.archived_at),
        ] {
            if active != ActiveStatus::Active || archived.is_some() {
                return Err(invalid());
            }
        }
        if item.data.coverage_template_id != coverage.id
            || item.data.shift_position_id != position.id
            || item.data.shift_template_id != shift.id
            || !(1..=item.data.required_count).contains(&slot_index)
        {
            return Err(invalid());
        }
        if let Some(h) = holiday {
            holidays::verify_holiday(h)?;
        }
        let custom = holiday.is_some_and(|h| h.data.coverage_behavior == CoverageBehavior::Custom);
        if (custom && coverage.data.day_type != DayType::HolidayCustom)
            || (!custom
                && (coverage.data.day_type.as_str() != operational_date.weekday().as_str()
                    || !coverage.is_default))
        {
            return Err(invalid());
        }
        if let Some(h) = holiday
            && (h.archived_at.is_some()
                || h.data.active_status != ActiveStatus::Active
                || holidays::resolve_date(&h.data, operational_date.year())?
                    != Some(operational_date)
                || (h.data.coverage_behavior == CoverageBehavior::Custom
                    && h.data.custom_coverage_template_id.as_deref() != Some(&coverage.id)))
        {
            return Err(invalid());
        }
        let actual = coverage::coverage_interval(
            operational_date,
            &shift.data,
            item.data.actual_start_day_offset,
            item.data.actual_end_day_offset,
        )?;
        let structure = AssignmentStructure {
            schedule_date: operational_date.format(),
            actual_start_datetime: actual.actual_start,
            actual_end_datetime: actual.actual_end,
            coverage_template_id: coverage.id.clone(),
            coverage_item_id: item.id.clone(),
            shift_position_id: position.id.clone(),
            shift_template_id: shift.id.clone(),
            coverage_template_seed_key_snapshot: coverage.seed_key.clone(),
            coverage_template_name_snapshot: coverage.data.name.clone(),
            coverage_day_type_snapshot: coverage.data.day_type,
            coverage_item_seed_key_snapshot: item.seed_key.clone(),
            coverage_item_display_snapshot: item.data.display_label.clone(),
            coverage_sort_order_snapshot: item.data.sort_order,
            coverage_required_count_snapshot: item.data.required_count,
            coverage_slot_index_snapshot: slot_index,
            coverage_start_offset_snapshot: item.data.actual_start_day_offset,
            coverage_end_offset_snapshot: item.data.actual_end_day_offset,
            position_seed_key_snapshot: position.seed_key.clone(),
            position_name_snapshot: position.data.name.clone(),
            position_display_snapshot: position.data.display_label.clone(),
            shift_seed_key_snapshot: shift.seed_key.clone(),
            shift_name_snapshot: shift.data.name.clone(),
            shift_display_snapshot: shift.data.display_label.clone(),
            shift_start_snapshot: shift.data.start_time.clone(),
            shift_end_snapshot: shift.data.end_time.clone(),
            shift_category_snapshot: shift.data.time_of_day,
            shift_color_snapshot: shift.data.color_hex.clone(),
            shift_lunch_minutes_snapshot: shift.unpaid_lunch_minutes,
            shift_paid_minutes_snapshot: shift.paid_minutes,
            shift_recovery_snapshot: shift.data.requires_recovery_after_shift,
            shift_night_snapshot: shift.data.is_night_shift,
            holiday_id: holiday.as_ref().map(|h| h.id.clone()),
            holiday_seed_key_snapshot: holiday.as_ref().and_then(|h| h.seed_key.clone()),
            holiday_name_snapshot: holiday.as_ref().map(|h| h.data.name.clone()),
            holiday_set_snapshot: holiday.as_ref().map(|h| h.data.holiday_set),
            holiday_coverage_behavior_snapshot: holiday.as_ref().map(|h| h.data.coverage_behavior),
        };
        Ok(Self {
            structure,
            coverage_revision: coverage.revision,
        })
    }
    pub fn structure(&self) -> &AssignmentStructure {
        &self.structure
    }
}
pub struct GeneratedEmployee {
    id: String,
    first_name: String,
    last_name: String,
    display_name: String,
}
impl GeneratedEmployee {
    pub(crate) fn from_employee(employee: &employees::Employee) -> AppResult<Self> {
        employees::verify_employee(employee)?;
        if employee.data.active_status != ActiveStatus::Active || employee.archived_at.is_some() {
            return Err(invalid());
        }
        Ok(Self {
            id: employee.id.clone(),
            first_name: employee.data.first_name.clone(),
            last_name: employee.data.last_name.clone(),
            display_name: employee.data.display_name.clone(),
        })
    }
}
pub(crate) enum EmployeeTarget {
    Open,
    Assign(String),
}
pub(crate) enum EditableTarget {
    Employee(EmployeeTarget),
    Note(Option<String>),
    Lock(bool),
}
/// Private backend-captured state, bound to the row from which history was read.
/// Later history authorization owns whether this capture may be replayed.
#[derive(Clone)]
pub(crate) struct EditableAssignmentState {
    schedule_id: String,
    assignment_id: String,
    state: AssignmentEditable,
}
impl EditableAssignmentState {
    pub(crate) fn capture(row: &Assignment) -> Self {
        Self {
            schedule_id: row.schedule_id.clone(),
            assignment_id: row.id.clone(),
            state: row.editable.clone(),
        }
    }
    pub(crate) fn state(&self) -> &AssignmentEditable {
        &self.state
    }
}
pub struct InitialScheduleContext {
    schedule: Schedule,
}
pub(crate) enum AssignmentWriteIntent<'a> {
    Create {
        context: &'a InitialScheduleContext,
        slot: &'a ValidatedCoverageSlot,
        employee: Option<GeneratedEmployee>,
    },
    Edit {
        schedule_id: &'a str,
        assignment_id: &'a str,
        expected_schedule_revision: i64,
        expected_assignment_revision: i64,
        target: EditableTarget,
    },
    Replay {
        schedule_id: &'a str,
        assignment_id: &'a str,
        expected_schedule_revision: i64,
        expected_assignment_revision: i64,
        captured: &'a EditableAssignmentState,
        lock_transition: bool,
    },
}
async fn employee_state(
    tx: &mut Transaction<'_, Sqlite>,
    target: EmployeeTarget,
    state: &mut AssignmentEditable,
) -> AppResult<()> {
    match target {
        EmployeeTarget::Open => {
            state.assigned_employee_id = None;
            state.employee_first_name_snapshot = None;
            state.employee_last_name_snapshot = None;
            state.employee_display_name_snapshot = None;
            state.open_shift = true;
        }
        EmployeeTarget::Assign(id) => {
            let employee = employees::read_employee(tx, &id).await?;
            if employee.archived_at.is_some() || employee.data.active_status != ActiveStatus::Active
            {
                return Err(AppError::new(
                    ErrorCode::InvalidEmployee,
                    "Choose an active, non-archived employee.",
                ));
            }
            state.assigned_employee_id = Some(employee.id);
            state.employee_first_name_snapshot = Some(employee.data.first_name);
            state.employee_last_name_snapshot = Some(employee.data.last_name);
            state.employee_display_name_snapshot = Some(employee.data.display_name);
            state.open_shift = false;
        }
    }
    Ok(())
}
/// The only assignment-write owner. It never begins/commits a transaction or
/// takes the coordinator. Structural columns occur in INSERT only.
pub(crate) async fn write_assignment(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
    intent: AssignmentWriteIntent<'_>,
) -> AppResult<(Assignment, bool)> {
    match intent {
        AssignmentWriteIntent::Create {
            context,
            slot,
            employee,
        } => {
            schedules::require_draft(&context.schedule)?;
            if context.schedule.revision != 1 {
                return Err(invalid());
            }
            let mut state = AssignmentEditable {
                assigned_employee_id: None,
                employee_first_name_snapshot: None,
                employee_last_name_snapshot: None,
                employee_display_name_snapshot: None,
                locked: false,
                open_shift: true,
                manually_edited: false,
                assignment_source: AssignmentSource::Generated,
                notes: None,
            };
            if let Some(employee) = employee {
                state.assigned_employee_id = Some(employee.id);
                state.employee_first_name_snapshot = Some(employee.first_name);
                state.employee_last_name_snapshot = Some(employee.last_name);
                state.employee_display_name_snapshot = Some(employee.display_name);
                state.open_shift = false;
            }
            let row = Assignment {
                id: uuid::Uuid::new_v4().to_string(),
                schedule_id: context.schedule.id.clone(),
                structure: slot.structure.clone(),
                editable: state,
                created_at: context.schedule.created_at.clone(),
                updated_at: context.schedule.created_at.clone(),
                revision: 1,
            };
            verify_assignment(&row, &context.schedule)?;
            insert_assignment(tx, &row).await?;
            Ok((row, true))
        }
        AssignmentWriteIntent::Edit {
            schedule_id,
            assignment_id,
            expected_schedule_revision,
            expected_assignment_revision,
            target,
        } => {
            let (mut schedule, mut row) = editable_assignment(
                tx,
                schedule_id,
                assignment_id,
                expected_schedule_revision,
                expected_assignment_revision,
            )
            .await?;
            let previous = row.editable.clone();
            let same = match &target {
                EditableTarget::Employee(EmployeeTarget::Open) => previous.open_shift,
                EditableTarget::Employee(EmployeeTarget::Assign(id)) => {
                    previous.assigned_employee_id.as_ref() == Some(id)
                }
                EditableTarget::Note(notes) => {
                    previous.notes == normalize_optional_notes(notes.clone())?
                }
                EditableTarget::Lock(locked) => previous.locked == *locked,
            };
            if same {
                return Ok((row, false));
            }
            if previous.locked && !matches!(target, EditableTarget::Lock(false)) {
                return Err(AppError::new(
                    ErrorCode::AssignmentLocked,
                    "Unlock this assignment before changing it.",
                ));
            }
            let changes_employee = matches!(target, EditableTarget::Employee(_));
            match target {
                EditableTarget::Employee(employee) => {
                    employee_state(tx, employee, &mut row.editable).await?
                }
                EditableTarget::Note(notes) => {
                    row.editable.notes = normalize_optional_notes(notes)?
                }
                EditableTarget::Lock(locked) => row.editable.locked = locked,
            }
            if row.editable == previous {
                return Ok((row, false));
            }
            if changes_employee {
                row.editable.assignment_source = AssignmentSource::Manual;
            }
            row.editable.manually_edited = true;
            persist_edit(tx, clock, &mut schedule, &mut row).await?;
            Ok((row, true))
        }
        AssignmentWriteIntent::Replay {
            schedule_id,
            assignment_id,
            expected_schedule_revision,
            expected_assignment_revision,
            captured,
            lock_transition,
        } => {
            let (mut schedule, mut row) = editable_assignment(
                tx,
                schedule_id,
                assignment_id,
                expected_schedule_revision,
                expected_assignment_revision,
            )
            .await?;
            if captured.schedule_id != schedule_id || captured.assignment_id != assignment_id {
                return Err(invalid());
            }
            if row.editable == captured.state {
                return Ok((row, false));
            }
            if row.editable.locked && !lock_transition {
                return Err(AppError::new(
                    ErrorCode::AssignmentLocked,
                    "Unlock this assignment before replaying this action.",
                ));
            }
            if lock_transition
                && (row.editable.assigned_employee_id != captured.state.assigned_employee_id
                    || row.editable.open_shift != captured.state.open_shift
                    || row.editable.notes != captured.state.notes)
            {
                return Err(invalid());
            }
            row.editable = captured.state.clone();
            persist_edit(tx, clock, &mut schedule, &mut row).await?;
            Ok((row, true))
        }
    }
}
async fn editable_assignment(
    tx: &mut Transaction<'_, Sqlite>,
    schedule_id: &str,
    assignment_id: &str,
    expected_schedule_revision: i64,
    expected_assignment_revision: i64,
) -> AppResult<(Schedule, Assignment)> {
    let schedule = schedules::read_schedule(tx, schedule_id).await?;
    check_revision(&schedule.id, schedule.revision, expected_schedule_revision)?;
    schedules::require_draft(&schedule)?;
    let row = read_assignment(tx, &schedule, assignment_id).await?;
    check_revision(&row.id, row.revision, expected_assignment_revision)?;
    Ok((schedule, row))
}
async fn persist_edit(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
    schedule: &mut Schedule,
    row: &mut Assignment,
) -> AppResult<()> {
    let previous_assignment = row.revision;
    let previous_schedule = schedule.revision;
    row.editable.verify()?;
    verify_retained_links(tx, row).await?;
    advance_source_metadata(&mut row.revision, &mut row.updated_at, clock)?;
    advance_source_metadata(&mut schedule.revision, &mut schedule.updated_at, clock)?;
    schedule.updated_at = row.updated_at.clone();
    let e = &row.editable;
    let result=sqlx::query("UPDATE schedule_assignments SET assigned_employee_id=?,employee_first_name_snapshot=?,employee_last_name_snapshot=?,employee_display_name_snapshot=?,locked=?,open_shift=?,manually_edited=?,assignment_source=?,notes=?,updated_at=?,revision=? WHERE id=? AND schedule_id=? AND revision=?")
        .bind(&e.assigned_employee_id).bind(&e.employee_first_name_snapshot).bind(&e.employee_last_name_snapshot).bind(&e.employee_display_name_snapshot)
        .bind(e.locked).bind(e.open_shift).bind(e.manually_edited).bind(e.assignment_source).bind(&e.notes)
        .bind(&row.updated_at).bind(row.revision).bind(&row.id).bind(&row.schedule_id).bind(previous_assignment).execute(&mut **tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the assignment before editing.",
        ));
    }
    let result=sqlx::query("UPDATE schedules SET updated_at=?,revision=? WHERE id=? AND revision=? AND status='draft' AND archived_at IS NULL")
        .bind(&schedule.updated_at).bind(schedule.revision).bind(&schedule.id).bind(previous_schedule).execute(&mut **tx).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::new(
            ErrorCode::StaleRevision,
            "Reload the schedule before editing.",
        ));
    }
    Ok(())
}

async fn insert_assignment(tx: &mut Transaction<'_, Sqlite>, row: &Assignment) -> AppResult<()> {
    sqlx::query("INSERT INTO schedule_assignments (id,schedule_id,schedule_date,actual_start_datetime,actual_end_datetime,coverage_template_id,coverage_item_id,shift_position_id,shift_template_id,coverage_template_seed_key_snapshot,coverage_template_name_snapshot,coverage_day_type_snapshot,coverage_item_seed_key_snapshot,coverage_item_display_snapshot,coverage_sort_order_snapshot,coverage_required_count_snapshot,coverage_slot_index_snapshot,coverage_start_offset_snapshot,coverage_end_offset_snapshot,position_seed_key_snapshot,position_name_snapshot,position_display_snapshot,shift_seed_key_snapshot,shift_name_snapshot,shift_display_snapshot,shift_start_snapshot,shift_end_snapshot,shift_category_snapshot,shift_color_snapshot,shift_lunch_minutes_snapshot,shift_paid_minutes_snapshot,shift_recovery_snapshot,shift_night_snapshot,holiday_id,holiday_seed_key_snapshot,holiday_name_snapshot,holiday_set_snapshot,holiday_coverage_behavior_snapshot,assigned_employee_id,employee_first_name_snapshot,employee_last_name_snapshot,employee_display_name_snapshot,locked,open_shift,manually_edited,assignment_source,notes,created_at,updated_at,revision) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&row.id)
        .bind(&row.schedule_id)
        .bind(&row.structure.schedule_date)
        .bind(&row.structure.actual_start_datetime)
        .bind(&row.structure.actual_end_datetime)
        .bind(&row.structure.coverage_template_id)
        .bind(&row.structure.coverage_item_id)
        .bind(&row.structure.shift_position_id)
        .bind(&row.structure.shift_template_id)
        .bind(&row.structure.coverage_template_seed_key_snapshot)
        .bind(&row.structure.coverage_template_name_snapshot)
        .bind(row.structure.coverage_day_type_snapshot)
        .bind(&row.structure.coverage_item_seed_key_snapshot)
        .bind(&row.structure.coverage_item_display_snapshot)
        .bind(row.structure.coverage_sort_order_snapshot)
        .bind(row.structure.coverage_required_count_snapshot)
        .bind(row.structure.coverage_slot_index_snapshot)
        .bind(row.structure.coverage_start_offset_snapshot)
        .bind(row.structure.coverage_end_offset_snapshot)
        .bind(&row.structure.position_seed_key_snapshot)
        .bind(&row.structure.position_name_snapshot)
        .bind(&row.structure.position_display_snapshot)
        .bind(&row.structure.shift_seed_key_snapshot)
        .bind(&row.structure.shift_name_snapshot)
        .bind(&row.structure.shift_display_snapshot)
        .bind(&row.structure.shift_start_snapshot)
        .bind(&row.structure.shift_end_snapshot)
        .bind(row.structure.shift_category_snapshot)
        .bind(&row.structure.shift_color_snapshot)
        .bind(row.structure.shift_lunch_minutes_snapshot)
        .bind(row.structure.shift_paid_minutes_snapshot)
        .bind(row.structure.shift_recovery_snapshot)
        .bind(row.structure.shift_night_snapshot)
        .bind(&row.structure.holiday_id)
        .bind(&row.structure.holiday_seed_key_snapshot)
        .bind(&row.structure.holiday_name_snapshot)
        .bind(row.structure.holiday_set_snapshot)
        .bind(row.structure.holiday_coverage_behavior_snapshot)
        .bind(&row.editable.assigned_employee_id)
        .bind(&row.editable.employee_first_name_snapshot)
        .bind(&row.editable.employee_last_name_snapshot)
        .bind(&row.editable.employee_display_name_snapshot)
        .bind(row.editable.locked)
        .bind(row.editable.open_shift)
        .bind(row.editable.manually_edited)
        .bind(row.editable.assignment_source)
        .bind(&row.editable.notes)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .bind(row.revision)
        .execute(&mut **tx).await?;
    Ok(())
}

pub struct PlannedAssignment {
    pub slot: ValidatedCoverageSlot,
    pub employee: Option<GeneratedEmployee>,
}
pub struct GeneratedSchedulePlan {
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub notes: Option<String>,
    pub snapshot: GenerationSettingsSnapshot,
    pub assignments: Vec<PlannedAssignment>,
}
/// Storage seam for Prompt 9, not a generator or public empty-schedule creator.
/// The caller holds the process gate and immediate transaction. Persist the whole
/// preflighted plan, verify count, and leave the single commit to that caller.
pub async fn store_generated_plan(
    tx: &mut Transaction<'_, Sqlite>,
    clock: &dyn MetadataClock,
    plan: GeneratedSchedulePlan,
) -> AppResult<Schedule> {
    let name = normalize_required_text(&plan.name)?;
    let notes = normalize_optional_notes(plan.notes)?;
    let start = LocalDate::parse(&plan.start_date)?;
    let end = LocalDate::parse(&plan.end_date)?;
    if start > end
        || end.days_since(start) >= crate::local_values::SCHEDULE_DAY_LIMIT as i64
        || plan.assignments.is_empty()
        || plan.assignments.len() > ASSIGNMENT_LIMIT
        || plan.assignments.len() != plan.snapshot.expected_assignment_count as usize
    {
        return Err(AppError::new(
            ErrorCode::InvalidSchedule,
            "A schedule requires a complete nonempty coverage plan matching its captured slot count and at most 370 days.",
        ));
    }
    let timestamp = clock_timestamp(clock)?;
    let schedule = Schedule {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        start_date: start.format(),
        end_date: end.format(),
        status: schedules::ScheduleStatus::Draft,
        notes,
        generation_settings_snapshot: plan.snapshot.canonical_json()?,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        archived_at: None,
        revision: 1,
        finalized_once: false,
    };
    let mut identities = BTreeSet::new();
    let mut dates = BTreeSet::new();
    let mut groups = std::collections::BTreeMap::<(String, String), (i64, BTreeSet<i64>)>::new();
    for assignment in &plan.assignments {
        let s = &assignment.slot.structure;
        s.verify(&schedule)?;
        if !identities.insert((
            s.schedule_date.clone(),
            s.coverage_item_id.clone(),
            s.coverage_slot_index_snapshot,
        )) {
            return Err(invalid());
        }
        let selected = plan
            .snapshot
            .selected_coverage_by_date
            .get(&s.schedule_date)
            .ok_or_else(invalid)?;
        if selected.id != s.coverage_template_id
            || selected.seed_key != s.coverage_template_seed_key_snapshot
            || selected.revision != assignment.slot.coverage_revision
        {
            return Err(invalid());
        }
        dates.insert(s.schedule_date.clone());
        let group = groups
            .entry((s.schedule_date.clone(), s.coverage_item_id.clone()))
            .or_insert((s.coverage_required_count_snapshot, BTreeSet::new()));
        if group.0 != s.coverage_required_count_snapshot {
            return Err(invalid());
        }
        group.1.insert(s.coverage_slot_index_snapshot);
    }
    if dates.len() as i64 != end.days_since(start) + 1
        || plan.snapshot.selected_coverage_by_date.len() != dates.len()
        || groups
            .values()
            .any(|(count, slots)| slots.len() as i64 != *count)
    {
        return Err(invalid());
    }
    sqlx::query("INSERT INTO schedules (id,name,start_date,end_date,status,notes,generation_settings_snapshot,created_at,updated_at,revision) VALUES (?,?,?,?,'draft',?,?,?,?,1)")
        .bind(&schedule.id).bind(&schedule.name).bind(&schedule.start_date).bind(&schedule.end_date).bind(&schedule.notes)
        .bind(&schedule.generation_settings_snapshot).bind(&schedule.created_at).bind(&schedule.updated_at).execute(&mut **tx).await?;
    let context = InitialScheduleContext { schedule };
    for assignment in plan.assignments {
        write_assignment(
            tx,
            clock,
            AssignmentWriteIntent::Create {
                context: &context,
                slot: &assignment.slot,
                employee: assignment.employee,
            },
        )
        .await?;
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM schedule_assignments WHERE schedule_id=?")
            .bind(&context.schedule.id)
            .fetch_one(&mut **tx)
            .await?;
    if count != i64::from(plan.snapshot.expected_assignment_count) {
        return Err(invalid());
    }
    Ok(context.schedule)
}
