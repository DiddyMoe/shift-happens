//! One bounded, owned historical interpretation shared by XLSX, PDF and print.
//! Only this load phase can access SQLite; its dedicated read-only connection
//! and transaction are explicitly closed on every outcome before materialization.
use crate::{
    database::{self, DatabasePaths},
    error::{AppError, AppResult, DIAGNOSTIC_LIMIT, ErrorCode},
    local_values::{LocalDate, LocalDateTime, LocalTime},
    metadata::{MetadataClock, clock_timestamp},
};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, Row, Sqlite, Transaction, sqlite::SqliteRow};

pub const XLSX_ROW_LIMIT: usize = 100_000;
pub const PRINT_ROW_LIMIT: usize = 10_000;
pub const TEXT_BUDGET: usize = 67_108_864;
pub const XLSX_CELL_LIMIT: usize = 32_767;
pub const HEADERS: [&str; 20] = [
    "Schedule Date",
    "Operational Day",
    "Holiday",
    "Holiday Set",
    "Coverage",
    "Slot",
    "Position",
    "Shift",
    "Shift Time",
    "Actual Start",
    "Actual End",
    "Employee",
    "State",
    "Paid Minutes",
    "Paid H:MM",
    "Lunch Minutes",
    "Locked",
    "Source",
    "Manually Edited",
    "Notes",
];
pub const COMPACT_HEADERS: [&str; 10] = [
    "Date",
    "Day",
    "Holiday",
    "Slot",
    "Position",
    "Shift / Time",
    "Actual Time",
    "Employee / State",
    "Paid",
    "Flags",
];
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Xlsx,
    Pdf,
    Print,
}
impl ExportFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Xlsx => "xlsx",
            Self::Pdf => "pdf",
            Self::Print => "print",
        }
    }
    pub fn row_limit(self) -> usize {
        match self {
            Self::Xlsx => XLSX_ROW_LIMIT,
            _ => PRINT_ROW_LIMIT,
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ExportCell {
    Text(String),
    Integer(i64),
}
impl ExportCell {
    pub fn text(&self) -> String {
        match self {
            Self::Text(v) => v.clone(),
            Self::Integer(v) => v.to_string(),
        }
    }
}
#[derive(Serialize)]
pub struct DetailField {
    pub label: &'static str,
    pub value: String,
}
#[derive(Serialize)]
pub struct ExportRow {
    pub detail_key: String,
    pub cells: [ExportCell; 20],
    pub compact: [String; 10],
    pub details: Vec<DetailField>,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportDiagnosticCode {
    LegacyControlReplaced,
    IncompleteEmployeeSnapshot,
    UnknownAssignedEmployee,
}
#[derive(Serialize)]
pub struct ExportDiagnostic {
    pub code: ExportDiagnosticCode,
    pub row: Option<usize>,
    pub field: String,
}
#[derive(Serialize)]
pub struct ExportSnapshot {
    pub schedule_id: String,
    pub schedule_name: String,
    pub start_date: String,
    pub end_date: String,
    pub status: String,
    pub archived_at: Option<String>,
    pub revision: i64,
    pub exported_at_utc: String,
    pub schedule_notes: String,
    pub rows: Vec<ExportRow>,
    pub diagnostics: Vec<ExportDiagnostic>,
    pub omitted_diagnostic_count: usize,
    pub text_bytes: usize,
    pub xlsx_available: bool,
}
impl ExportSnapshot {
    pub fn metadata(&self) -> [String; 6] {
        [
            format!("Schedule: {}", self.schedule_name),
            format!("ID: {}", self.schedule_id),
            format!("Range: {} to {}", self.start_date, self.end_date),
            format!(
                "Status: {} | Archived: {}",
                self.status,
                if self.archived_at.is_some() {
                    "Yes"
                } else {
                    "No"
                }
            ),
            format!("Captured schedule revision: {}", self.revision),
            format!("Export metadata (UTC): {}", self.exported_at_utc),
        ]
    }
}
pub fn structural() -> AppError {
    AppError::new(
        ErrorCode::ExportStructureInvalid,
        "The schedule contains contradictory or invalid stored slot facts. No output was created and no data was changed.",
    )
}
pub fn text_limit(observed: usize) -> AppError {
    let mut error = AppError::new(
        ErrorCode::ExportTextBudgetExceeded,
        "The normalized schedule text exceeds the export limit. No output was created.",
    );
    error.context.observed_count = Some(observed);
    error.context.limit = Some(TEXT_BUDGET);
    error
}
pub fn row_limit(format: ExportFormat, count: usize, alternative: bool) -> AppError {
    let mut error = AppError::new(
        ErrorCode::ExportTooLargeForFormat,
        "This schedule exceeds the selected format's assignment row limit. No output was created.",
    );
    error.context.observed_count = Some(count);
    error.context.limit = Some(format.row_limit());
    error.context.diagnostics =
        Some(serde_json::json!({"format":format.name(), "xlsx_alternative":alternative}));
    error
}
pub fn paid_h_mm(minutes: i64) -> AppResult<String> {
    if minutes < 0 {
        return Err(structural());
    }
    Ok(format!("{}:{:02}", minutes / 60, minutes % 60))
}
struct TextBudget {
    used: usize,
    xlsx_available: bool,
}
impl TextBudget {
    fn reserve(&mut self, bytes: usize) -> AppResult<()> {
        let measured = self
            .used
            .checked_add(bytes)
            .ok_or_else(|| text_limit(usize::MAX))?;
        if measured > TEXT_BUDGET {
            return Err(text_limit(measured));
        }
        self.used = measured;
        Ok(())
    }
    fn copy(&mut self, value: &str) -> AppResult<String> {
        self.reserve(value.len())?;
        Ok(value.to_owned())
    }
    fn derived(&mut self, value: String) -> AppResult<String> {
        self.reserve(value.len())?;
        Ok(value)
    }
    fn join(&mut self, values: &[&str], separator: &str) -> AppResult<String> {
        let bytes = values
            .iter()
            .try_fold(0usize, |sum, v| sum.checked_add(v.len()))
            .and_then(|sum| {
                separator
                    .len()
                    .checked_mul(values.len().saturating_sub(1))
                    .and_then(|n| sum.checked_add(n))
            })
            .ok_or_else(|| text_limit(usize::MAX))?;
        self.reserve(bytes)?;
        Ok(values.join(separator))
    }
}
struct Diagnostics {
    values: Vec<ExportDiagnostic>,
    omitted: usize,
}
impl Diagnostics {
    fn push(&mut self, code: ExportDiagnosticCode, row: Option<usize>, field: &str) {
        if self.values.len() < DIAGNOSTIC_LIMIT {
            self.values.push(ExportDiagnostic {
                code,
                row,
                field: field.to_owned(),
            });
        } else {
            self.omitted += 1;
        }
    }
}
// Raw SQL byte lengths are checked before fetching values. A second exact
// normalized count handles visible control replacement expansion before allocation.
fn normalized(
    budget: &mut TextBudget,
    diagnostics: &mut Diagnostics,
    value: &str,
    row: Option<usize>,
    field: &str,
) -> AppResult<String> {
    let mut bytes = 0usize;
    let mut controls = false;
    for c in value.chars() {
        let size = if c.is_control() && !matches!(c, '\n' | '\t') {
            controls = true;
            // [U+0000] through [U+009F], visibly preserving the offending scalar.
            8
        } else {
            c.len_utf8()
        };
        bytes = bytes
            .checked_add(size)
            .ok_or_else(|| text_limit(usize::MAX))?;
    }
    budget.reserve(bytes)?;
    if controls {
        diagnostics.push(ExportDiagnosticCode::LegacyControlReplaced, row, field);
    }
    let mut output = String::with_capacity(bytes);
    for c in value.chars() {
        if c.is_control() && !matches!(c, '\n' | '\t') {
            use std::fmt::Write;
            write!(output, "[U+{:04X}]", u32::from(c)).map_err(|_| structural())?;
        } else {
            output.push(c);
        }
    }
    Ok(output)
}
async fn preflight_lengths(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    columns: &[&str],
    selector: &str,
    id: i64,
    schedule_id: &str,
    budget: &TextBudget,
) -> AppResult<()> {
    let projection = columns
        .iter()
        .map(|c| format!("coalesce(length(CAST({c} AS BLOB)),0) AS {c}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT {projection} FROM {table} WHERE {selector}");
    let row = sqlx::QueryBuilder::<Sqlite>::new("")
        .push(&sql)
        .build()
        .bind(schedule_id)
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    let mut bytes = budget.used;
    for column in columns {
        let length = usize::try_from(row.try_get::<i64, _>(*column)?).map_err(|_| structural())?;
        bytes = bytes
            .checked_add(length)
            .ok_or_else(|| text_limit(usize::MAX))?;
        if bytes > TEXT_BUDGET {
            return Err(text_limit(bytes));
        }
    }
    Ok(())
}
const SCHEDULE_TEXT: &[&str] = &[
    "id",
    "name",
    "start_date",
    "end_date",
    "status",
    "archived_at",
    "notes",
];
const ROW_TEXT: &[&str] = &[
    "id",
    "schedule_date",
    "actual_start_datetime",
    "actual_end_datetime",
    "coverage_item_id",
    "coverage_item_seed_key_snapshot",
    "coverage_item_display_snapshot",
    "coverage_template_name_snapshot",
    "coverage_day_type_snapshot",
    "position_name_snapshot",
    "position_display_snapshot",
    "shift_name_snapshot",
    "shift_display_snapshot",
    "shift_start_snapshot",
    "shift_end_snapshot",
    "shift_category_snapshot",
    "shift_color_snapshot",
    "holiday_id",
    "holiday_name_snapshot",
    "holiday_set_snapshot",
    "holiday_coverage_behavior_snapshot",
    "assigned_employee_id",
    "employee_display_name_snapshot",
    "employee_first_name_snapshot",
    "employee_last_name_snapshot",
    "assignment_source",
    "notes",
];
fn raw(row: &SqliteRow, field: &str) -> AppResult<String> {
    row.try_get::<Option<String>, _>(field)
        .map(|v| v.unwrap_or_default())
        .map_err(|_| structural())
}
fn integer(row: &SqliteRow, field: &str) -> AppResult<i64> {
    row.try_get::<i64, _>(field).map_err(|_| structural())
}
fn boolean(row: &SqliteRow, field: &str) -> AppResult<bool> {
    match integer(row, field)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(structural()),
    }
}
fn usable(value: &str) -> bool {
    !value.trim().is_empty()
}

async fn load(
    tx: &mut Transaction<'_, Sqlite>,
    schedule_id: &str,
    format: ExportFormat,
    clock: &dyn MetadataClock,
) -> AppResult<ExportSnapshot> {
    let mut budget = TextBudget {
        used: 0,
        xlsx_available: true,
    };
    let mut diagnostics = Diagnostics {
        values: Vec::new(),
        omitted: 0,
    };
    if !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM schedules WHERE id=?)")
        .bind(schedule_id)
        .fetch_one(&mut **tx)
        .await?
    {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "Select an existing schedule.",
        ));
    }
    // Limit the count itself; an observed violating lower bound is sufficient.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM (SELECT 1 FROM schedule_assignments WHERE schedule_id=? LIMIT 100001)")
        .bind(schedule_id).fetch_one(&mut **tx).await?;
    let count = usize::try_from(count).map_err(|_| structural())?;
    if count > XLSX_ROW_LIMIT {
        return Err(row_limit(format, count, false));
    }
    preflight_lengths(
        tx,
        "schedules",
        SCHEDULE_TEXT,
        "id=?1 AND ?2=0",
        0,
        schedule_id,
        &budget,
    )
    .await?;
    let sql = format!(
        "SELECT {},revision,typeof(revision)='integer' AS numeric_valid FROM schedules WHERE id=?",
        SCHEDULE_TEXT.join(",")
    );
    let s = sqlx::QueryBuilder::<Sqlite>::new("")
        .push(&sql)
        .build()
        .bind(schedule_id)
        .fetch_one(&mut **tx)
        .await?;
    let mut text = |field| normalized(&mut budget, &mut diagnostics, &raw(&s, field)?, None, field);
    let id = text("id")?;
    let name = text("name")?;
    let start_date = text("start_date")?;
    let end_date = text("end_date")?;
    let status = text("status")?;
    let archive = text("archived_at")?;
    let notes = text("notes")?;
    if LocalDate::parse(&start_date)? > LocalDate::parse(&end_date)?
        || !matches!(status.as_str(), "draft" | "final")
        || integer(&s, "numeric_valid")? != 1
        || integer(&s, "revision")? < 1
    {
        return Err(structural());
    }
    let timestamp = budget.derived(clock_timestamp(clock)?)?;
    let mut snapshot = ExportSnapshot {
        schedule_id: id,
        schedule_name: name,
        start_date,
        end_date,
        status,
        archived_at: if archive.is_empty() {
            None
        } else {
            Some(archive)
        },
        revision: integer(&s, "revision")?,
        exported_at_utc: timestamp,
        schedule_notes: notes,
        rows: Vec::new(),
        diagnostics: Vec::new(),
        omitted_diagnostic_count: 0,
        text_bytes: 0,
        xlsx_available: true,
    };
    budget.xlsx_available = snapshot.schedule_name.encode_utf16().count() + "Schedule: ".len()
        <= XLSX_CELL_LIMIT
        && snapshot.schedule_id.encode_utf16().count() + "ID: ".len() <= XLSX_CELL_LIMIT;
    // Row IDs are integers, never an unbounded legacy text projection. Fetch one
    // row at a time, in the canonical order, and stop immediately at any limit.
    let ids: Vec<i64> = sqlx::query_scalar("SELECT rowid FROM schedule_assignments WHERE schedule_id=? ORDER BY schedule_date,coverage_sort_order_snapshot,coverage_slot_index_snapshot,actual_start_datetime,id LIMIT 100000")
        .bind(schedule_id).fetch_all(&mut **tx).await?;
    let numeric = [
        "coverage_sort_order_snapshot",
        "coverage_required_count_snapshot",
        "coverage_slot_index_snapshot",
        "coverage_start_offset_snapshot",
        "coverage_end_offset_snapshot",
        "shift_lunch_minutes_snapshot",
        "shift_paid_minutes_snapshot",
        "shift_recovery_snapshot",
        "shift_night_snapshot",
        "locked",
        "open_shift",
        "manually_edited",
    ];
    let numeric_check = numeric
        .iter()
        .map(|c| format!("typeof({c})='integer'"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "SELECT {},{},({numeric_check}) AS numeric_valid FROM schedule_assignments WHERE schedule_id=?1 AND rowid=?2",
        ROW_TEXT.join(","),
        numeric.join(",")
    );
    for (index, id) in ids.into_iter().enumerate() {
        preflight_lengths(
            tx,
            "schedule_assignments",
            ROW_TEXT,
            "schedule_id=?1 AND rowid=?2",
            id,
            schedule_id,
            &budget,
        )
        .await?;
        let r = sqlx::QueryBuilder::<Sqlite>::new("")
            .push(&sql)
            .build()
            .bind(schedule_id)
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        let row = build_row(tx, &r, index + 1, &snapshot, &mut budget, &mut diagnostics).await?;
        if row
            .cells
            .iter()
            .any(|c| matches!(c,ExportCell::Text(v) if v.encode_utf16().count()>XLSX_CELL_LIMIT))
        {
            budget.xlsx_available = false;
        }
        // For a PDF/print row-limit error, check XLSX viability without building
        // an oversized output DTO or DOM. Only one normalized row is retained.
        if count <= format.row_limit() {
            snapshot.rows.push(row);
        }
    }
    if count > format.row_limit() {
        return Err(row_limit(format, count, budget.xlsx_available));
    }
    snapshot.diagnostics = diagnostics.values;
    snapshot.omitted_diagnostic_count = diagnostics.omitted;
    snapshot.text_bytes = budget.used;
    snapshot.xlsx_available = budget.xlsx_available;
    Ok(snapshot)
}
async fn build_row(
    tx: &mut Transaction<'_, Sqlite>,
    r: &SqliteRow,
    index: usize,
    schedule: &ExportSnapshot,
    budget: &mut TextBudget,
    diagnostics: &mut Diagnostics,
) -> AppResult<ExportRow> {
    if integer(r, "numeric_valid")? != 1 {
        return Err(structural());
    }
    let open = boolean(r, "open_shift")?;
    let assigned: Option<String> = r
        .try_get("assigned_employee_id")
        .map_err(|_| structural())?;
    if open != assigned.is_none()
        || assigned.as_ref().is_some_and(|v| !usable(v))
        || (open
            && [
                "employee_display_name_snapshot",
                "employee_first_name_snapshot",
                "employee_last_name_snapshot",
            ]
            .iter()
            .any(|f| r.try_get::<Option<String>, _>(*f).ok().flatten().is_some()))
    {
        return Err(structural());
    }
    let date_raw = raw(r, "schedule_date")?;
    let date = LocalDate::parse(&date_raw).map_err(|_| structural())?;
    if date < LocalDate::parse(&schedule.start_date)?
        || date > LocalDate::parse(&schedule.end_date)?
    {
        return Err(structural());
    }
    let start = raw(r, "shift_start_snapshot")?;
    let end = raw(r, "shift_end_snapshot")?;
    let start_offset = integer(r, "coverage_start_offset_snapshot")?;
    let end_offset = integer(r, "coverage_end_offset_snapshot")?;
    let actual_start = raw(r, "actual_start_datetime")?;
    let actual_end = raw(r, "actual_end_datetime")?;
    let paid = integer(r, "shift_paid_minutes_snapshot")?;
    let lunch = integer(r, "shift_lunch_minutes_snapshot")?;
    let shift_time =
        crate::shift_time::calculate_shift_time(&start, &end).map_err(|_| structural())?;
    let required = integer(r, "coverage_required_count_snapshot")?;
    let slot = integer(r, "coverage_slot_index_snapshot")?;
    if !matches!((start_offset, end_offset), (0, 0) | (0, 1) | (1, 1))
        || !(1..=100).contains(&required)
        || !(1..=required).contains(&slot)
        || LocalDateTime::from_date_time(date, LocalTime::parse(&start)?, start_offset)?.format()
            != actual_start
        || LocalDateTime::from_date_time(date, LocalTime::parse(&end)?, end_offset)?.format()
            != actual_end
        || actual_end <= actual_start
        || paid != shift_time.paid_minutes
        || lunch != shift_time.unpaid_lunch_minutes
    {
        return Err(structural());
    }
    let source = raw(r, "assignment_source")?;
    let category = raw(r, "shift_category_snapshot")?;
    crate::employees::validate_sort_order(integer(r, "coverage_sort_order_snapshot")?)
        .map_err(|_| structural())?;
    if !matches!(source.as_str(), "generated" | "manual")
        || !matches!(category.as_str(), "day" | "evening" | "night")
    {
        return Err(structural());
    }
    let holiday_id: Option<String> = r.try_get("holiday_id").map_err(|_| structural())?;
    if holiday_id.is_none()
        && (r
            .try_get::<Option<String>, _>("holiday_name_snapshot")?
            .is_some()
            || r.try_get::<Option<String>, _>("holiday_set_snapshot")?
                .is_some()
            || r.try_get::<Option<String>, _>("holiday_coverage_behavior_snapshot")?
                .is_some())
    {
        return Err(structural());
    }
    if holiday_id.is_some()
        && (!usable(&raw(r, "holiday_name_snapshot")?)
            || !matches!(raw(r, "holiday_set_snapshot")?.as_str(), "a" | "b" | "none")
            || !matches!(
                raw(r, "holiday_coverage_behavior_snapshot")?.as_str(),
                "normal" | "custom"
            ))
    {
        return Err(structural());
    }
    let display = raw(r, "employee_display_name_snapshot")?;
    let first = raw(r, "employee_first_name_snapshot")?;
    let last = raw(r, "employee_last_name_snapshot")?;
    let employee = if open {
        "Open Shift".to_owned()
    } else if usable(&display) {
        display
    } else if usable(&first) && usable(&last) {
        format!("{first} {last}")
    } else {
        diagnostics.push(
            ExportDiagnosticCode::IncompleteEmployeeSnapshot,
            Some(index),
            "employee",
        );
        // Preflight current fallback before retrieving/allocating its text.
        let employee_id = assigned.as_deref().ok_or_else(structural)?;
        let size: Option<i64> = sqlx::query_scalar("SELECT coalesce(length(CAST(display_name AS BLOB)),0)+coalesce(length(CAST(first_name AS BLOB)),0)+coalesce(length(CAST(last_name AS BLOB)),0) FROM employees WHERE id=?")
                .bind(employee_id).fetch_optional(&mut **tx).await?;
        if let Some(size) = size {
            let measured = budget
                .used
                .checked_add(usize::try_from(size).map_err(|_| structural())?)
                .ok_or_else(|| text_limit(usize::MAX))?;
            if measured > TEXT_BUDGET {
                return Err(text_limit(measured));
            }
        }
        let current =
            sqlx::query("SELECT display_name,first_name,last_name FROM employees WHERE id=?")
                .bind(employee_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(current) = current {
            let name = raw(&current, "display_name")?;
            let first = raw(&current, "first_name")?;
            let last = raw(&current, "last_name")?;
            if usable(&name) {
                name
            } else if usable(&first) && usable(&last) {
                format!("{first} {last}")
            } else {
                diagnostics.push(
                    ExportDiagnosticCode::UnknownAssignedEmployee,
                    Some(index),
                    "employee",
                );
                "Unknown employee (incomplete data)".to_owned()
            }
        } else {
            diagnostics.push(
                ExportDiagnosticCode::UnknownAssignedEmployee,
                Some(index),
                "employee",
            );
            "Unknown employee (incomplete data)".to_owned()
        }
    };
    let locked = boolean(r, "locked")?;
    let manual = boolean(r, "manually_edited")?;
    let recovery = boolean(r, "shift_recovery_snapshot")?;
    let night = boolean(r, "shift_night_snapshot")?;
    let mut norm =
        |field: &str| normalized(budget, diagnostics, &raw(r, field)?, Some(index), field);
    let assignment_id = norm("id")?;
    let holiday_id = norm("holiday_id")?;
    let employee_id = norm("assigned_employee_id")?;
    let first_snapshot = norm("employee_first_name_snapshot")?;
    let last_snapshot = norm("employee_last_name_snapshot")?;
    let display_snapshot = norm("employee_display_name_snapshot")?;
    let date = norm("schedule_date")?;
    let holiday = norm("holiday_name_snapshot")?;
    let holiday_set = norm("holiday_set_snapshot")?;
    let holiday_behavior = norm("holiday_coverage_behavior_snapshot")?;
    let coverage = norm("coverage_template_name_snapshot")?;
    let position_name = norm("position_name_snapshot")?;
    let position_label = norm("position_display_snapshot")?;
    let shift_name = norm("shift_name_snapshot")?;
    let shift_label = norm("shift_display_snapshot")?;
    let category = norm("shift_category_snapshot")?;
    let color = norm("shift_color_snapshot")?;
    let item = norm("coverage_item_display_snapshot")?;
    let item_key = norm("coverage_item_seed_key_snapshot")?;
    let item_id = norm("coverage_item_id")?;
    let day_type = norm("coverage_day_type_snapshot")?;
    let notes = norm("notes")?;
    // Match the canonical slot interpretation using captured facts alone.
    // Current source edits must never change the historical coverage meaning.
    let custom_holiday = holiday_behavior == "custom";
    if [
        assignment_id.as_str(),
        item_id.as_str(),
        employee_id.as_str(),
        holiday_id.as_str(),
        item_key.as_str(),
    ]
    .iter()
    .any(|id| id.len() > crate::error::DIAGNOSTIC_SOURCE_ID_BYTES)
        || !usable(&coverage)
        || !usable(&position_name)
        || !usable(&shift_name)
        || (custom_holiday && day_type != "holiday_custom")
        || (!custom_holiday && day_type != LocalDate::parse(&date)?.weekday().as_str())
        || !matches!(
            day_type.as_str(),
            "monday"
                | "tuesday"
                | "wednesday"
                | "thursday"
                | "friday"
                | "saturday"
                | "sunday"
                | "holiday_custom"
        )
    {
        return Err(structural());
    }
    let employee = normalized(budget, diagnostics, &employee, Some(index), "employee")?;
    let position = budget.copy(if usable(&position_label) {
        &position_label
    } else {
        &position_name
    })?;
    let shift = budget.copy(if usable(&shift_label) {
        &shift_label
    } else {
        &shift_name
    })?;
    let day = budget.copy(LocalDate::parse(&date)?.weekday().english_label())?;
    let time = budget.derived(format!("{start}–{end}"))?;
    let slot_text = budget.derived(format!("{slot} of {required}"))?;
    let actual_start = budget.copy(&actual_start)?;
    let actual_end = budget.copy(&actual_end)?;
    let state = budget.copy(if open { "Open" } else { "Assigned" })?;
    let paid_text = budget.derived(paid_h_mm(paid)?)?;
    let locked_text = budget.copy(if locked { "Yes" } else { "No" })?;
    let manual_text = budget.copy(if manual { "Yes" } else { "No" })?;
    let source = budget.copy(&source)?;
    let flags = budget.derived(
        [
            if locked { "locked" } else { "" },
            if manual { "manual" } else { "" },
            if source == "generated" {
                "generated"
            } else {
                ""
            },
            if open { "open" } else { "" },
        ]
        .into_iter()
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(", "),
    )?;
    let compact = [
        budget.copy(&date)?,
        budget.copy(&day)?,
        budget.copy(&holiday)?,
        budget.copy(&slot_text)?,
        budget.copy(&position)?,
        budget.join(&[&shift, &time], "\n")?,
        budget.derived(format!(
            "{}\n{}",
            actual_start[..16].replace('T', " "),
            actual_end[..16].replace('T', " ")
        ))?,
        budget.join(&[&employee, &state], "\n")?,
        budget.copy(&paid_text)?,
        flags,
    ];
    let detail_key = budget.derived(
        serde_json::to_string(&(
            &date,
            if usable(&item_key) {
                &item_key
            } else {
                &item_id
            },
            slot,
        ))
        .map_err(|_| structural())?,
    )?;
    let mut details = Vec::new();
    for (label, value) in [
        ("Assignment ID", assignment_id),
        ("Coverage item ID", item_id),
        ("Coverage item seed", item_key),
        ("Holiday ID", holiday_id),
        ("Assigned employee ID", employee_id),
        ("Employee first-name snapshot", first_snapshot),
        ("Employee last-name snapshot", last_snapshot),
        ("Employee display-name snapshot", display_snapshot),
        ("Coverage item", item),
        ("Coverage day type", day_type),
        (
            "Coverage start-day offset",
            budget.derived(start_offset.to_string())?,
        ),
        (
            "Coverage end-day offset",
            budget.derived(end_offset.to_string())?,
        ),
        ("Holiday coverage behavior", holiday_behavior),
        ("Position name", position_name),
        ("Position label", position_label),
        ("Shift name", shift_name),
        ("Shift label", shift_label),
        ("Shift category", category),
        ("Shift color", color),
        (
            "Recovery required",
            budget.copy(if recovery { "Yes" } else { "No" })?,
        ),
        (
            "Night shift",
            budget.copy(if night { "Yes" } else { "No" })?,
        ),
    ] {
        details.push(DetailField { label, value });
    }
    let cells = [
        ExportCell::Text(date),
        ExportCell::Text(day),
        ExportCell::Text(holiday),
        ExportCell::Text(holiday_set),
        ExportCell::Text(coverage),
        ExportCell::Text(slot_text),
        ExportCell::Text(position),
        ExportCell::Text(shift),
        ExportCell::Text(time),
        ExportCell::Text(actual_start),
        ExportCell::Text(actual_end),
        ExportCell::Text(employee),
        ExportCell::Text(state),
        ExportCell::Integer(paid),
        ExportCell::Text(paid_text),
        ExportCell::Integer(lunch),
        ExportCell::Text(locked_text),
        ExportCell::Text(source),
        ExportCell::Text(manual_text),
        ExportCell::Text(notes),
    ];
    Ok(ExportRow {
        detail_key,
        cells,
        compact,
        details,
    })
}
pub async fn read_snapshot(
    paths: &DatabasePaths,
    schedule_id: &str,
    format: ExportFormat,
    clock: &dyn MetadataClock,
) -> AppResult<ExportSnapshot> {
    crate::text::normalize_required_text(schedule_id)?;
    let mut connection = database::open_validation_reader(paths).await?;
    let loaded = async {
        let mut tx = connection.begin().await?;
        let result = load(&mut tx, schedule_id, format, clock).await;
        tx.rollback().await?;
        result
    }
    .await;
    connection.close().await?;
    loaded
}
