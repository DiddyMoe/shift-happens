//! Deterministic read-only validation. Emission passes: header/limits, rows and
//! structural slots, source/configuration hard errors, assignment hard rules,
//! maxima, overlap sweep, then audit/assignment/workload/run/rest warnings.
//! Each pass uses stable employee/date/assignment-ID order and stops immediately
//! on output exhaustion. Presentation sorts errors first, then date, datetime,
//! code, assignment, counterpart and identity; nullable keys sort last.
pub use crate::validation_snapshot::{
    ValidationSnapshot, load_validation_snapshot, validate_schedule,
};
use crate::{
    availability_rules::{AvailabilityPtoIndex, RuleCode},
    employee_restrictions::RestrictionIndex,
    employees::{Employee, EmploymentType, RulePeriod, TimeOfDay},
    error::{AppResult, DIAGNOSTIC_SOURCE_ID_LIMIT},
    generation::ExclusionReason,
    generation_inputs::Policy,
    generation_snapshot::{ASSIGNMENT_LIMIT, GenerationSettingsSnapshot},
    holidays::{self, HolidaySet},
    local_values::{
        ANALYSIS_EMPLOYEE_LIMIT, ANALYSIS_SOURCE_LIMIT, ExpandedIntervalBudget, LocalDate,
        LocalDateTime, LocalInterval, LocalTime, wall_clock_minutes,
    },
    rule_facts::MaterialFacts,
    scheduling_rules::{self, RuleIndexes, SlotRules},
    validation_snapshot::Facts,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const VALIDATION_OUTPUT_LIMIT: usize = 10_000;
#[derive(Clone, Copy)]
pub struct ValidationLimits {
    pub schedule_days: usize,
    pub assignments: usize,
    pub employees: usize,
    pub sources: usize,
    pub expanded_intervals: usize,
    pub findings: usize,
}
impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            schedule_days: crate::local_values::SCHEDULE_DAY_LIMIT,
            assignments: ASSIGNMENT_LIMIT,
            employees: ANALYSIS_EMPLOYEE_LIMIT,
            sources: ANALYSIS_SOURCE_LIMIT,
            expanded_intervals: ANALYSIS_SOURCE_LIMIT,
            findings: VALIDATION_OUTPUT_LIMIT,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}
// One authoritative code/severity/message table; frontend consumes this table.
macro_rules! codes {
    ($($variant:ident => ($wire:expr,$severity:ident,$message:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum ValidationCode { $($variant),+ }
        impl ValidationCode {
            pub fn as_str(self)->&'static str { match self { $(Self::$variant=>$wire),+ } }
            pub fn severity(self)->Severity { match self { $(Self::$variant=>Severity::$severity),+ } }
            pub fn message(self)->&'static str { match self { $(Self::$variant=>$message),+ } }
            fn table()->Vec<CodeDescription> { vec![$(CodeDescription { code:Self::$variant,severity:Severity::$severity,message:$message }),+] }
        }
        impl Serialize for ValidationCode {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    }
}
codes! {
    InvalidScheduleState => ("invalid_schedule_state",Error,"The schedule dates or status are invalid."),
    CorruptAssignmentState => ("corrupt_assignment_state",Error,"An assignment has inconsistent state or required snapshots."),
    DuplicateScheduleSlot => ("duplicate_schedule_slot",Error,"Two shifts fill the same coverage slot."),
    IncompleteScheduleSlots => ("incomplete_schedule_slots",Error,"Some of this schedule's shifts are missing or inconsistent."),
    AssignmentOutsideScheduleRange => ("assignment_outside_schedule_range",Error,"This shift's date is outside the schedule's dates."),
    InvalidAssignmentInterval => ("invalid_assignment_interval",Error,"This shift's start and end times are inconsistent."),
    AssignmentPaidTimeMismatch => ("assignment_paid_time_mismatch",Error,"This shift's paid hours don't match its length minus lunch."),
    LunchPolicyViolation => ("lunch_policy_violation",Error,"Shifts of 8 hours or more need a 0.5 h unpaid lunch; shorter shifts have none."),
    AssignmentOverlapConflict => ("assignment_overlap_conflict",Error,"This employee is booked on overlapping shifts."),
    MissingReference => ("missing_reference",Error,"A required employee or record can't be found."),
    HardRuleDataIncomplete => ("hard_rule_data_incomplete",Error,"An eligibility or maximum-hours rule can't be fully checked because some data is missing."),
    ApprovedPtoConflict => (RuleCode::ApprovedPtoConflict.as_str(),Error,"This shift overlaps approved PTO."),
    AvailabilityHardConflict => (RuleCode::AvailabilityHardConflict.as_str(),Error,"This shift overlaps time the employee is unavailable."),
    ExplicitAvailabilityMissing => (RuleCode::ExplicitAvailabilityMissing.as_str(),Error,"The employee's available time doesn't cover this whole shift."),
    EmployeeRestrictionConflict => ("employee_restriction_conflict",Error,"This shift breaks one of the employee's restrictions."),
    ShiftTemplateRestrictionConflict => ("shift_template_restriction_conflict",Error,"The employee can't work this shift template."),
    PositionRestrictionConflict => ("position_restriction_conflict",Error,"The employee can't work this position."),
    NightShiftIneligible => ("night_shift_ineligible",Error,"This employee isn't set to be scheduled for night shifts."),
    OrthoOrIneligible => ("ortho_or_ineligible",Error,"This employee isn't set to be scheduled for ORTHO & OR."),
    InactiveOrArchivedEmployeeAssigned => ("inactive_or_archived_employee_assigned",Error,"The assigned employee is inactive or archived."),
    WeekendIneligible => ("weekend_ineligible",Error,"This employee isn't set to be scheduled for weekends."),
    HolidayIneligible => ("holiday_ineligible",Error,"This employee isn't set to be scheduled for holidays."),
    MaximumPaidMinutesExceeded => ("maximum_paid_minutes_exceeded",Error,"This puts the employee over their maximum paid hours."),
    ValidationInputLimitExceeded => ("validation_input_limit_exceeded",Error,"This schedule is too large to check completely."),
    ValidationOutputLimitExceeded => ("validation_output_limit_exceeded",Error,"There are too many issues to list. Fix some and check again; finalizing is blocked until then."),
    OpenShift => ("open_shift",Warning,"No one is assigned to this shift."),
    RequestedPtoOverlap => (RuleCode::RequestedPtoOverlap.as_str(),Warning,"This shift overlaps requested PTO."),
    PreferredTimeMismatch => ("preferred_time_mismatch",Warning,"This shift's time of day isn't the employee's preference."),
    PreferredAvailabilityNotMet => (RuleCode::PreferredAvailabilityNotMet.as_str(),Warning,"This shift is outside the employee's preferred availability."),
    WeekendRotationMismatch => ("weekend_rotation_mismatch",Warning,"This weekend is outside the employee's rotation."),
    HolidayGroupMismatch => ("holiday_group_mismatch",Warning,"The employee's holiday group isn't the one working this holiday."),
    PrnWeekdayAvailabilityMissing => ("prn_weekday_availability_missing",Warning,"This PRN employee's availability doesn't cover this whole weekday shift."),
    OvertimeWeekExceeded => ("overtime_week_exceeded",Warning,"Planned weekly paid hours are over the overtime threshold."),
    FullTimeWeekMinimumNotMet => ("full_time_week_minimum_not_met",Warning,"Below the full-time weekly minimum hours."),
    PartTimeTwoWeekMinimumNotMet => ("part_time_two_week_minimum_not_met",Warning,"Below the part-time two-week minimum hours."),
    CustomEmployeeMinimumNotMet => ("custom_employee_minimum_not_met",Warning,"Below the employee's minimum for this period."),
    ConsecutiveWorkdaysExceeded => ("consecutive_workdays_exceeded",Warning,"More consecutive workdays than the limit allows."),
    MinimumRestViolation => ("minimum_rest_violation",Warning,"Less than the minimum rest between shifts."),
    RecoveryRuleViolation => ("recovery_rule_violation",Warning,"Not enough recovery time after the previous shift."),
    TwoWeekAnchorMissing => ("two_week_anchor_missing",Warning,"A two-week rule needs the two-week pay period start, which isn't set."),
    IncompleteData => ("incomplete_data",Warning,"Some preferences can't be fully checked because data is missing."),
}
use ValidationCode as C;
#[derive(Serialize)]
pub struct CodeDescription {
    code: ValidationCode,
    severity: Severity,
    message: &'static str,
}
#[derive(Serialize)]
pub struct ValidationFinding {
    pub id: String,
    pub code: ValidationCode,
    pub severity: Severity,
    pub schedule_id: String,
    pub employee_id: Option<String>,
    pub assignment_id: Option<String>,
    pub counterpart_assignment_id: Option<String>,
    pub operational_date: Option<String>,
    pub actual_datetime: Option<String>,
    pub message: &'static str,
    pub context: Value,
}
#[derive(Serialize)]
pub struct ValidationResult {
    pub schedule_id: String,
    pub findings: Vec<ValidationFinding>,
    pub error_count: usize,
    pub warning_count: usize,
    pub truncated: bool,
    pub complete: bool,
    pub codes: Vec<CodeDescription>,
}
#[derive(Default)]
struct Target {
    employee: Option<String>,
    assignment: Option<String>,
    counterpart: Option<String>,
    date: Option<String>,
    actual: Option<String>,
}
impl Target {
    fn employee(id: &str) -> Self {
        Self {
            employee: Some(id.to_owned()),
            ..Self::default()
        }
    }
    fn row(row: &Checked) -> Self {
        Self {
            employee: row.employee.clone(),
            assignment: Some(row.raw.id().to_owned()),
            date: row.date.map(LocalDate::format),
            actual: row.start.map(LocalDateTime::format),
            counterpart: None,
        }
    }
    fn pair(a: &Checked, b: &Checked) -> Self {
        let (first, second) = if a.raw.id() < b.raw.id() {
            (a, b)
        } else {
            (b, a)
        };
        Self {
            employee: a.employee.clone(),
            assignment: Some(first.raw.id().to_owned()),
            counterpart: Some(second.raw.id().to_owned()),
            date: a.date.zip(b.date).map(|(a, b)| a.min(b).format()),
            actual: a.start.zip(b.start).map(|(a, b)| a.min(b).format()),
        }
    }
}
struct Output {
    schedule: String,
    findings: Vec<ValidationFinding>,
    seen: BTreeSet<String>,
    limit: usize,
    truncated: bool,
    incomplete: bool,
}
impl Output {
    fn new(id: &str, limit: usize) -> Self {
        Self {
            schedule: id.to_owned(),
            findings: Vec::new(),
            seen: BTreeSet::new(),
            limit: limit.clamp(1, VALIDATION_OUTPUT_LIMIT),
            truncated: false,
            incomplete: false,
        }
    }
    fn emit(
        &mut self,
        code: C,
        target: Target,
        context: Value,
        extra: impl FnOnce(&mut MaterialFacts) -> AppResult<()>,
    ) -> AppResult<bool> {
        if self.truncated {
            return Ok(false);
        }
        // A semantic emission key suppresses duplicates before material hashing.
        let key = serde_json::to_string(&json!([
            code.as_str(),
            target.employee,
            target.assignment,
            target.counterpart,
            target.date,
            context.get("period_start"),
            context.get("run_start"),
            context.get("slot_identity")
        ]))
        .map_err(|_| crate::generation_inputs::arithmetic_error())?;
        if self.seen.contains(&key) {
            return Ok(true);
        }
        if self.findings.len() >= self.limit - 1 {
            self.truncated = true;
            return Ok(false);
        }
        self.seen.insert(key);
        let mut facts = MaterialFacts::new("validate_schedule", code.as_str());
        facts.text(&self.schedule);
        for value in [
            &target.employee,
            &target.assignment,
            &target.counterpart,
            &target.date,
            &target.actual,
        ] {
            facts.text(value.as_deref().unwrap_or(""));
        }
        let mut material = context.clone();
        if let Some(m) = material.as_object_mut() {
            m.remove("policy_revisions");
        }
        facts.text(
            &serde_json::to_string(&material)
                .map_err(|_| crate::generation_inputs::arithmetic_error())?,
        );
        extra(&mut facts)?;
        self.findings.push(ValidationFinding {
            id: facts.finish(),
            code,
            severity: code.severity(),
            schedule_id: self.schedule.clone(),
            employee_id: target.employee,
            assignment_id: target.assignment,
            counterpart_assignment_id: target.counterpart,
            operational_date: target.date,
            actual_datetime: target.actual,
            message: code.message(),
            context,
        });
        Ok(true)
    }
    fn plain(&mut self, code: C, target: Target, context: Value) -> AppResult<bool> {
        self.emit(code, target, context, |_| Ok(()))
    }
    fn finish(mut self) -> AppResult<ValidationResult> {
        self.findings.sort_unstable_by(|a, b| {
            (
                a.severity,
                (a.operational_date.is_none(), &a.operational_date),
                (a.actual_datetime.is_none(), &a.actual_datetime),
                a.code.as_str(),
                (a.assignment_id.is_none(), &a.assignment_id),
                (
                    a.counterpart_assignment_id.is_none(),
                    &a.counterpart_assignment_id,
                ),
                &a.id,
            )
                .cmp(&(
                    b.severity,
                    (b.operational_date.is_none(), &b.operational_date),
                    (b.actual_datetime.is_none(), &b.actual_datetime),
                    b.code.as_str(),
                    (b.assignment_id.is_none(), &b.assignment_id),
                    (
                        b.counterpart_assignment_id.is_none(),
                        &b.counterpart_assignment_id,
                    ),
                    &b.id,
                ))
        });
        if self.truncated {
            let context = json!({"limit":self.limit,"truncated":true});
            let mut facts = MaterialFacts::new(
                "validate_schedule",
                C::ValidationOutputLimitExceeded.as_str(),
            );
            facts.text(&self.schedule);
            facts.text(&context.to_string());
            self.findings.push(ValidationFinding {
                id: facts.finish(),
                code: C::ValidationOutputLimitExceeded,
                severity: Severity::Error,
                schedule_id: self.schedule.clone(),
                employee_id: None,
                assignment_id: None,
                counterpart_assignment_id: None,
                operational_date: None,
                actual_datetime: None,
                message: C::ValidationOutputLimitExceeded.message(),
                context,
            });
        }
        Ok(ValidationResult {
            schedule_id: self.schedule,
            error_count: self
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .count(),
            warning_count: self
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Warning)
                .count(),
            truncated: self.truncated,
            complete: !self.truncated && !self.incomplete,
            findings: self.findings,
            codes: C::table(),
        })
    }
}
struct Checked {
    raw: Facts,
    date: Option<LocalDate>,
    start: Option<LocalDateTime>,
    interval: Option<LocalInterval>,
    trustworthy_interval: bool,
    employee: Option<String>,
    membership: bool,
    structural: bool,
    paid: Option<i64>,
    category: Option<TimeOfDay>,
    night: Option<bool>,
    recovery: Option<bool>,
    holiday: bool,
    holiday_set: Option<HolidaySet>,
    legacy: Vec<(String, String)>,
    missing: Vec<String>,
    bad: Vec<String>,
}
fn fallback(
    row: &mut Facts,
    snapshot: &ValidationSnapshot,
    table: &'static str,
    id_key: &str,
    mapping: &[(&str, &str)],
    legacy: &mut Vec<(String, String)>,
    missing: &mut Vec<String>,
) {
    for &(dest, source) in mapping {
        if !row.absent(dest) {
            continue;
        }
        let id = row.s(id_key).unwrap_or("").to_owned();
        let value = snapshot
            .fallback
            .get(table)
            .and_then(|m| m.get(&id))
            .and_then(|r| r.0.get(source))
            .filter(|v| !v.is_null())
            .cloned();
        if let Some(v) = value {
            if let Some(m) = row.0.as_object_mut() {
                m.insert(dest.to_owned(), v);
            }
            legacy.push((dest.to_owned(), id));
        } else {
            missing.push(dest.to_owned());
        }
    }
}
fn check(
    mut raw: Facts,
    snapshot: &ValidationSnapshot,
    start: LocalDate,
    end: LocalDate,
) -> Checked {
    let mut legacy = Vec::new();
    let mut missing = Vec::new();
    let mut bad = Vec::new();
    fallback(
        &mut raw,
        snapshot,
        "shift_templates",
        "shift_template_id",
        &[
            ("shift_start_snapshot", "start_time"),
            ("shift_end_snapshot", "end_time"),
            ("shift_category_snapshot", "time_of_day"),
            ("shift_lunch_minutes_snapshot", "unpaid_lunch_minutes"),
            ("shift_paid_minutes_snapshot", "paid_minutes"),
            ("shift_recovery_snapshot", "requires_recovery_after_shift"),
            ("shift_night_snapshot", "is_night_shift"),
        ],
        &mut legacy,
        &mut missing,
    );
    fallback(
        &mut raw,
        snapshot,
        "coverage_template_items",
        "coverage_item_id",
        &[
            ("coverage_required_count_snapshot", "required_count"),
            ("coverage_start_offset_snapshot", "actual_start_day_offset"),
            ("coverage_end_offset_snapshot", "actual_end_day_offset"),
            ("coverage_sort_order_snapshot", "sort_order"),
        ],
        &mut legacy,
        &mut missing,
    );
    fallback(
        &mut raw,
        snapshot,
        "coverage_templates",
        "coverage_template_id",
        &[("coverage_day_type_snapshot", "day_type")],
        &mut legacy,
        &mut missing,
    );
    let holiday = !raw.absent("holiday_id");
    if holiday {
        fallback(
            &mut raw,
            snapshot,
            "holidays",
            "holiday_id",
            &[
                ("holiday_set_snapshot", "holiday_set"),
                ("holiday_coverage_behavior_snapshot", "coverage_behavior"),
            ],
            &mut legacy,
            &mut missing,
        );
    }
    // A nullable position seed is authoritative when the position snapshot exists.
    if raw.b("position_name_valid") != Some(true) {
        fallback(
            &mut raw,
            snapshot,
            "shift_positions",
            "shift_position_id",
            &[("position_seed_key_snapshot", "seed_key")],
            &mut legacy,
            &mut missing,
        );
    }
    let employee = raw.s("assigned_employee_id").map(str::to_owned);
    let names = [
        raw.b("first_present"),
        raw.b("last_present"),
        raw.b("display_present"),
    ];
    let membership = match raw.b("open_shift") {
        Some(true) => raw.absent("assigned_employee_id") && names.iter().all(|v| *v == Some(false)),
        Some(false) => employee.is_some() && raw.b("names_valid") == Some(true),
        None => false,
    };
    if !membership {
        bad.push("employee_membership_or_snapshots".to_owned());
    }
    let date = raw
        .s("schedule_date")
        .and_then(|v| LocalDate::parse(v).ok());
    let actual_start = raw
        .s("actual_start_datetime")
        .and_then(|v| LocalDateTime::parse(v).ok());
    let actual_end = raw
        .s("actual_end_datetime")
        .and_then(|v| LocalDateTime::parse(v).ok());
    let interval = actual_start
        .zip(actual_end)
        .and_then(|(a, b)| LocalInterval::new(a, b).ok())
        .filter(|i| wall_clock_minutes(i.start(), i.end()).is_ok_and(|n| (1..=1439).contains(&n)));
    let snapped_start = raw
        .s("shift_start_snapshot")
        .and_then(|v| LocalTime::parse(v).ok());
    let snapped_end = raw
        .s("shift_end_snapshot")
        .and_then(|v| LocalTime::parse(v).ok());
    let offsets = raw
        .n("coverage_start_offset_snapshot")
        .zip(raw.n("coverage_end_offset_snapshot"))
        .filter(|v| matches!(v, (0, 0) | (0, 1) | (1, 1)));
    let snapped = date
        .zip(snapped_start.zip(snapped_end))
        .zip(offsets)
        .and_then(|((d, (a, b)), (x, y))| {
            Some((
                LocalDateTime::from_date_time(d, a, x).ok()?,
                LocalDateTime::from_date_time(d, b, y).ok()?,
            ))
        });
    let trustworthy_interval = interval.is_some()
        && snapped_start.zip(snapped_end).is_some_and(|(a, b)| a != b)
        && snapped == actual_start.zip(actual_end);
    let category = raw
        .s("shift_category_snapshot")
        .and_then(|v| serde_json::from_value::<TimeOfDay>(json!(v)).ok());
    let night = raw.b("shift_night_snapshot");
    let recovery = raw.b("shift_recovery_snapshot");
    let holiday_set = raw
        .s("holiday_set_snapshot")
        .and_then(|v| serde_json::from_value::<HolidaySet>(json!(v)).ok());
    if category.is_none() || night.is_none() || recovery.is_none() {
        bad.push("shift_rule_snapshots".to_owned());
    }
    if holiday
        && (holiday_set.is_none()
            || !matches!(
                raw.s("holiday_coverage_behavior_snapshot"),
                Some("normal" | "custom")
            )
            || raw.b("holiday_name_valid") != Some(true))
        || !holiday
            && [
                "holiday_set_snapshot",
                "holiday_seed_key_snapshot",
                "holiday_coverage_behavior_snapshot",
            ]
            .iter()
            .any(|k| !raw.absent(k))
        || !holiday && raw.b("holiday_name_present") != Some(false)
    {
        bad.push("holiday_snapshots".to_owned());
    }
    let count = raw.n("coverage_required_count_snapshot");
    let index = raw.n("coverage_slot_index_snapshot");
    let slot = count.is_some_and(|n| (1..=100).contains(&n))
        && index.zip(count).is_some_and(|(i, n)| i >= 1 && i <= n);
    if !slot {
        bad.push("slot_index_or_count".to_owned());
    }
    let custom = raw.s("holiday_coverage_behavior_snapshot") == Some("custom");
    let day_type_ok = date.is_some_and(|d| {
        raw.s("coverage_day_type_snapshot")
            == Some(if custom {
                "holiday_custom"
            } else {
                d.weekday().as_str()
            })
    });
    if !day_type_ok {
        bad.push("coverage_day_type".to_owned());
    }
    for key in [
        "id",
        "coverage_template_id",
        "coverage_item_id",
        "shift_position_id",
        "shift_template_id",
    ] {
        if raw.s(key).is_none_or(|v| {
            v.trim().is_empty() || v.len() > crate::error::DIAGNOSTIC_SOURCE_ID_BYTES
        }) {
            bad.push(key.to_owned());
        }
    }
    for key in ["assigned_employee_id", "holiday_id"] {
        if !raw.absent(key)
            && raw.s(key).is_none_or(|v| {
                v.trim().is_empty() || v.len() > crate::error::DIAGNOSTIC_SOURCE_ID_BYTES
            })
        {
            bad.push(key.to_owned());
        }
    }
    if raw.b("shift_name_valid") != Some(true)
        || raw.b("position_name_valid") != Some(true)
        || raw.b("coverage_name_valid") != Some(true)
    {
        bad.push("required_display_snapshots".to_owned());
    }
    if raw.n("coverage_sort_order_snapshot").is_none()
        || !matches!(raw.s("assignment_source"), Some("generated" | "manual"))
        || ["locked", "manually_edited"]
            .iter()
            .any(|k| raw.b(k).is_none())
    {
        bad.push("assignment_flags_or_sort_order".to_owned());
    }
    if !raw.absent("shift_color_snapshot")
        && raw.s("shift_color_snapshot").is_none_or(|c| {
            crate::shift_time::normalize_color(Some(c.to_owned())).ok() != Some(Some(c.to_owned()))
        })
    {
        bad.push("shift_color_snapshot".to_owned());
    }
    if !raw.absent("position_seed_key_snapshot")
        && raw.s("position_seed_key_snapshot").is_none_or(|s| {
            crate::employees::normalize_optional_label(Some(s.to_owned())).ok()
                != Some(Some(s.to_owned()))
        })
    {
        bad.push("position_seed_key_snapshot".to_owned());
    }
    let paid = raw
        .n("shift_paid_minutes_snapshot")
        .filter(|n| (1..=1439).contains(n));
    if paid.is_none() {
        bad.push("paid_minutes".to_owned());
    }
    let structural = slot
        && date.is_some_and(|d| d >= start && d <= end)
        && raw.s("coverage_item_id").is_some_and(|v| !v.is_empty())
        && day_type_ok;
    bad.sort();
    bad.dedup();
    missing.sort();
    missing.dedup();
    legacy.sort();
    legacy.dedup();
    Checked {
        raw,
        date,
        start: actual_start,
        interval,
        trustworthy_interval,
        employee,
        membership,
        structural,
        paid,
        category,
        night,
        recovery,
        holiday,
        holiday_set,
        legacy,
        missing,
        bad,
    }
}
fn revisions(snapshot: &ValidationSnapshot, keys: &[&str]) -> Value {
    json!(
        snapshot
            .settings
            .iter()
            .filter(|s| keys.contains(&s.key))
            .map(|s| (s.key, s.revision))
            .collect::<BTreeMap<_, _>>()
    )
}
fn limited<T: Serialize>(values: impl IntoIterator<Item = T>) -> Value {
    let mut kept = Vec::new();
    let mut omitted = 0;
    for v in values {
        if kept.len() < DIAGNOSTIC_SOURCE_ID_LIMIT {
            kept.push(v);
        } else {
            omitted += 1;
        }
    }
    json!({"items":kept,"omitted_source_id_count":omitted})
}
fn limited_ids<'a>(values: impl IntoIterator<Item = &'a str>) -> Value {
    let ids: BTreeSet<_> = values.into_iter().collect();
    let mut kept = Vec::new();
    let mut omitted = 0;
    for id in ids {
        if id.len() <= crate::error::DIAGNOSTIC_SOURCE_ID_BYTES
            && kept.len() < DIAGNOSTIC_SOURCE_ID_LIMIT
        {
            kept.push(id);
        } else {
            omitted += 1;
        }
    }
    json!({"items":kept,"omitted_source_id_count":omitted})
}
fn rule_code(reason: ExclusionReason) -> C {
    match reason {
        ExclusionReason::ApprovedPtoConflict => C::ApprovedPtoConflict,
        ExclusionReason::AvailabilityHardConflict => C::AvailabilityHardConflict,
        ExclusionReason::ExplicitAvailabilityMissing => C::ExplicitAvailabilityMissing,
        ExclusionReason::EmployeeRestriction => C::EmployeeRestrictionConflict,
        ExclusionReason::ShiftTemplateRestriction => C::ShiftTemplateRestrictionConflict,
        ExclusionReason::PositionRestriction => C::PositionRestrictionConflict,
        ExclusionReason::NightCapabilityMissing => C::NightShiftIneligible,
        ExclusionReason::OrthoOrCapabilityMissing => C::OrthoOrIneligible,
        ExclusionReason::WeekendCapabilityMissing => C::WeekendIneligible,
        ExclusionReason::HolidayCapabilityMissing => C::HolidayIneligible,
        ExclusionReason::WeekendOffRotation => C::WeekendRotationMismatch,
        ExclusionReason::HolidayGroupMismatch => C::HolidayGroupMismatch,
        ExclusionReason::PrnWeekdayAvailabilityMissing => C::PrnWeekdayAvailabilityMissing,
        _ => C::HardRuleDataIncomplete,
    }
}
fn holiday_group(row: &Checked, policy: &Policy) -> Option<crate::employees::HolidayGroup> {
    holidays::rotation_for_year(row.date?.year(), policy.holiday_year)
        .ok()?
        .required_group(row.holiday_set?)
}
fn slot_rules<'a>(row: &'a Checked, policy: &Policy) -> Option<SlotRules<'a>> {
    // Missing holiday/position facts cannot establish rotation or capability.
    // Unrelated display/slot corruption still permits independent eligibility.
    if row.holiday && row.holiday_set.is_none()
        || row
            .bad
            .iter()
            .any(|field| field == "position_seed_key_snapshot")
    {
        return None;
    }
    Some(SlotRules {
        date: row.date?,
        interval: row.interval?,
        category: row.category?,
        night: row.night?,
        ortho: row.raw.s("position_seed_key_snapshot") == Some("position_ortho_or"),
        is_holiday: row.holiday,
        holiday_group: holiday_group(row, policy),
        shift_id: row.raw.s("shift_template_id")?,
        position_id: row.raw.s("shift_position_id")?,
    })
}

pub fn validate_snapshot(mut snapshot: ValidationSnapshot) -> AppResult<ValidationResult> {
    let mut out = Output::new(&snapshot.schedule_id, snapshot.limits.findings);
    let Some(header) = &snapshot.header else {
        out.incomplete = true;
        out.plain(
            C::MissingReference,
            Target::default(),
            json!({"missing_fact":"schedule"}),
        )?;
        return out.finish();
    };
    let start = header
        .s("start_date")
        .and_then(|v| LocalDate::parse(v).ok());
    let end = header.s("end_date").and_then(|v| LocalDate::parse(v).ok());
    let (Some(start), Some(end)) = (start, end) else {
        out.incomplete = true;
        out.plain(
            C::InvalidScheduleState,
            Target::default(),
            json!({"missing_fact":"canonical_schedule_dates"}),
        )?;
        return out.finish();
    };
    if start > end || !matches!(header.s("status"), Some("draft" | "final")) {
        out.incomplete = true;
        out.plain(C::InvalidScheduleState,Target::default(),json!({"start_date":start.format(),"end_date":end.format(),"status":header.0.get("status")}))?;
        return out.finish();
    }
    if let Some((kind, limit, observed)) = snapshot.stopped {
        out.incomplete = true;
        out.plain(
            C::ValidationInputLimitExceeded,
            Target::default(),
            json!({"limit_kind":kind,"limit":limit,"observed_count":observed}),
        )?;
        return out.finish();
    }
    // Pure callers receive the same header/count boundaries before any expansion.
    // Failed source rows live only in source_problems; each selected physical
    // source is counted once, including sources used for legacy fallback.
    let source_count = [
        snapshot.restrictions.len(),
        snapshot.availability.len(),
        snapshot.pto.len(),
        snapshot.template_blocks.len(),
        snapshot.position_blocks.len(),
        snapshot.source_problems.len(),
    ]
    .into_iter()
    .chain(snapshot.fallback.values().map(BTreeMap::len))
    .try_fold(0usize, usize::checked_add)
    .unwrap_or(usize::MAX);
    for (kind, limit, observed) in [
        (
            "schedule_days",
            snapshot.limits.schedule_days,
            (end.days_since(start) + 1) as usize,
        ),
        (
            "assignments",
            snapshot.limits.assignments,
            snapshot.rows.len(),
        ),
        (
            "employee_rows",
            snapshot.limits.employees,
            snapshot.employee_states.len(),
        ),
        ("source_rows", snapshot.limits.sources, source_count),
    ] {
        if observed > limit {
            out.incomplete = true;
            out.plain(
                C::ValidationInputLimitExceeded,
                Target::default(),
                json!({"limit_kind":kind,"limit":limit,"observed_count":observed}),
            )?;
            return out.finish();
        }
    }
    let audit = header
        .s("generation_settings_snapshot")
        .and_then(|v| serde_json::from_str::<Value>(v).ok());
    let known = audit
        .as_ref()
        .and_then(|v| v.get("version"))
        .and_then(Value::as_u64)
        == Some(1);
    let expected = if known {
        audit
            .as_ref()
            .and_then(|v| v.get("expected_assignment_count"))
            .and_then(Value::as_u64)
            .filter(|n| *n >= 1 && *n <= ASSIGNMENT_LIMIT as u64)
    } else {
        None
    };
    let audit_bad = header
        .s("generation_settings_snapshot")
        .is_none_or(|s| GenerationSettingsSnapshot::parse(s).is_err());
    let mut rows: Vec<_> = std::mem::take(&mut snapshot.rows)
        .into_iter()
        .map(|r| check(r, &snapshot, start, end))
        .collect();
    rows.sort_unstable_by(|a, b| {
        (&a.raw.0["schedule_date"].as_str(), a.raw.id())
            .cmp(&(&b.raw.0["schedule_date"].as_str(), b.raw.id()))
    });
    let windows: Vec<_> = rows
        .iter()
        .filter(|r| r.employee.is_some() && r.membership)
        .filter_map(|r| r.interval)
        .collect();
    let mut expanded = ExpandedIntervalBudget {
        limit: snapshot.limits.expanded_intervals,
        count: 0,
    };
    let indexes = (|| -> AppResult<Option<RuleIndexes>> {
        if windows.is_empty() {
            return Ok(None);
        }
        let restrictions =
            RestrictionIndex::build(&snapshot.restrictions, &windows, &mut expanded)?;
        let availability = AvailabilityPtoIndex::build(
            &snapshot.availability,
            &snapshot.pto,
            &windows,
            &mut expanded,
        )?;
        let mut template_blocks = BTreeMap::<String, BTreeMap<String, Vec<String>>>::new();
        for r in &snapshot.template_blocks {
            template_blocks
                .entry(r.employee_id.clone())
                .or_default()
                .entry(r.data.shift_template_id.clone())
                .or_default()
                .push(r.id.clone());
        }
        let mut position_blocks = BTreeMap::<String, BTreeMap<String, Vec<String>>>::new();
        for r in &snapshot.position_blocks {
            position_blocks
                .entry(r.employee_id.clone())
                .or_default()
                .entry(r.data.shift_position_id.clone())
                .or_default()
                .push(r.id.clone());
        }
        Ok(Some(RuleIndexes {
            restrictions,
            availability,
            template_blocks,
            position_blocks,
        }))
    })();
    let mut index_failed = false;
    let indexes = match indexes {
        Ok(v) => v,
        Err(e) if matches!(e.code, crate::error::ErrorCode::InputLimitExceeded) => {
            // Input-limit responses discard partial findings and make no validation claim.
            let mut limited = Output::new(&snapshot.schedule_id, snapshot.limits.findings);
            limited.incomplete = true;
            limited.plain(C::ValidationInputLimitExceeded,Target::default(),json!({"limit_kind":e.context.limit_kind,"limit":e.context.limit,"observed_count":e.context.observed_count}))?;
            return limited.finish();
        }
        Err(_) => {
            index_failed = true;
            None
        }
    };
    let mut tainted = BTreeMap::<String, BTreeSet<String>>::new();
    let mut unknown_membership = false;
    let mut by_employee = BTreeMap::<String, Vec<usize>>::new();
    let mut groups = BTreeMap::<(String, String), Vec<usize>>::new();
    let mut duplicate = BTreeMap::<(String, String, i64), Vec<usize>>::new();
    let mut valid_dates = BTreeSet::new();
    for (i, row) in rows.iter().enumerate() {
        if let (Some(date), Some(item)) = (row.date, row.raw.s("coverage_item_id")) {
            groups
                .entry((date.format(), item.to_owned()))
                .or_default()
                .push(i);
            if let Some(index) = row.raw.n("coverage_slot_index_snapshot") {
                duplicate
                    .entry((date.format(), item.to_owned(), index))
                    .or_default()
                    .push(i);
            }
            if row.structural {
                valid_dates.insert(date);
            }
        }
        if let Some(id) = &row.employee {
            by_employee.entry(id.clone()).or_default().push(i);
        }
        if !row.membership && row.employee.is_none() {
            unknown_membership = true;
        }
        if !row.bad.is_empty()
            && !out.plain(
                C::CorruptAssignmentState,
                Target::row(row),
                json!({"invalid_facts":row.bad}),
            )?
        {
            return out.finish();
        }
        if !row.missing.is_empty()
            && !out.plain(
                C::MissingReference,
                Target::row(row),
                json!({"missing_snapshots_and_sources":row.missing}),
            )?
        {
            return out.finish();
        }
        if row.date.is_none_or(|d|d<start||d>end)&&!out.plain(C::AssignmentOutsideScheduleRange,Target::row(row),json!({"schedule_date":row.raw.0.get("schedule_date"),"start_date":start.format(),"end_date":end.format()}))?{return out.finish();}
        if !row.trustworthy_interval&&!out.plain(C::InvalidAssignmentInterval,Target::row(row),json!({"actual_start":row.raw.0.get("actual_start_datetime"),"actual_end":row.raw.0.get("actual_end_datetime"),"snapped_start":row.raw.0.get("shift_start_snapshot"),"snapped_end":row.raw.0.get("shift_end_snapshot"),"start_offset":row.raw.0.get("coverage_start_offset_snapshot"),"end_offset":row.raw.0.get("coverage_end_offset_snapshot")}))?{return out.finish();}
        let elapsed = row
            .interval
            .and_then(|v| wall_clock_minutes(v.start(), v.end()).ok());
        let lunch = row.raw.n("shift_lunch_minutes_snapshot");
        let lunch_ok = elapsed.is_some_and(|n| lunch == Some(if n >= 480 { 30 } else { 0 }));
        let paid_ok = elapsed
            .zip(lunch)
            .and_then(|(n, l)| n.checked_sub(l))
            .zip(row.paid)
            .is_some_and(|(a, b)| a == b);
        if elapsed.is_some()&&!lunch_ok&&!out.plain(C::LunchPolicyViolation,Target::row(row),json!({"elapsed_minutes":elapsed,"lunch_minutes":row.raw.0.get("shift_lunch_minutes_snapshot"),"required_lunch_minutes":elapsed.map(|n|if n>=480{30}else{0})}))?{return out.finish();}
        if elapsed.is_some()&&!paid_ok&&!out.plain(C::AssignmentPaidTimeMismatch,Target::row(row),json!({"elapsed_minutes":elapsed,"lunch_minutes":lunch,"paid_minutes":row.raw.0.get("shift_paid_minutes_snapshot")}))?{return out.finish();}
        if let Some(id) = &row.employee {
            if !row.membership
                || !row.trustworthy_interval
                || !paid_ok
                || !lunch_ok
                || row.date.is_none_or(|d| d < start || d > end)
            {
                tainted
                    .entry(id.clone())
                    .or_default()
                    .insert("assignment_membership_interval_or_paid_minutes".to_owned());
            }
            if !snapshot.employee_states.contains_key(id)
                && !out.plain(
                    C::MissingReference,
                    Target::row(row),
                    json!({"missing_fact":"assigned_employee","employee_id":id}),
                )?
            {
                return out.finish();
            }
            if snapshot.employee_states.get(id) == Some(&false)
                && !out.plain(
                    C::InactiveOrArchivedEmployeeAssigned,
                    Target::row(row),
                    json!({"employee_id":id,"currently_active":false}),
                )?
            {
                return out.finish();
            }
        }
    }
    for (identity, ids) in duplicate.iter().filter(|(_, v)| v.len() > 1) {
        let row = &rows[ids[0]];
        let full: Vec<_> = ids.iter().map(|i| rows[*i].raw.id()).collect();
        if !out.emit(
            C::DuplicateScheduleSlot,
            Target {
                date: row.date.map(LocalDate::format),
                ..Target::default()
            },
            json!({"slot_identity":identity,"assignment_ids":limited_ids(full.iter().copied())}),
            |f| {
                for id in &full {
                    f.text(id);
                }
                Ok(())
            },
        )? {
            return out.finish();
        }
    }
    let mut missing_dates = Vec::new();
    let mut date = start;
    loop {
        if !valid_dates.contains(&date) {
            missing_dates.push(date.format());
        }
        if date == end {
            break;
        }
        date = date.add_days(1)?;
    }
    let mut bad_groups = Vec::new();
    for ((date, item), ids) in &groups {
        let counts: BTreeSet<_> = ids
            .iter()
            .map(|i| rows[*i].raw.n("coverage_required_count_snapshot"))
            .collect();
        let indices: BTreeSet<_> = ids
            .iter()
            .filter_map(|i| rows[*i].raw.n("coverage_slot_index_snapshot"))
            .collect();
        let expected = counts
            .iter()
            .next()
            .copied()
            .flatten()
            .filter(|n| (1..=100).contains(n));
        let correct = counts.len() == 1
            && expected.is_some_and(|n| {
                indices.len() == n as usize
                    && indices.first() == Some(&1)
                    && indices.last() == Some(&n)
                    && ids.len() == n as usize
            });
        if !correct {
            bad_groups.push(json!({"schedule_date":date,"coverage_item_id":item,"required_counts":limited(counts),"observed_indices":limited(indices),"observed_rows":ids.len()}));
        }
    }
    let count_bad = known && expected.is_none_or(|n| n as usize != rows.len());
    let incomplete_slots = count_bad || !missing_dates.is_empty() || !bad_groups.is_empty();
    if incomplete_slots {
        let context = json!({"missing_dates":limited(&missing_dates),"invalid_groups":limited(&bad_groups),"expected_assignment_count":expected,"observed_assignment_count":rows.len(),"recognized_count_format":known,"count_invalid_or_mismatched":count_bad});
        if !out.emit(
            C::IncompleteScheduleSlots,
            Target::default(),
            context,
            |f| {
                for d in &missing_dates {
                    f.text(d);
                }
                for g in &bad_groups {
                    f.text(&g.to_string());
                }
                Ok(())
            },
        )? {
            return out.finish();
        }
    }
    let policy = if snapshot.settings_bad.is_empty() {
        Policy::from_settings(&snapshot.settings).ok()
    } else {
        None
    };
    if policy.is_none()
        && !out.plain(
            C::HardRuleDataIncomplete,
            Target::default(),
            json!({"missing_or_invalid_settings":snapshot.settings_bad}),
        )?
    {
        return out.finish();
    }
    if unknown_membership&&!out.plain(C::HardRuleDataIncomplete,Target::default(),json!({"missing_fact":"assignment_employee_membership","dependent_workload_claims_suppressed":true}))?{return out.finish();}
    if index_failed && !out.plain(C::HardRuleDataIncomplete, Target::default(), json!({"missing_fact":"current_rule_interval_index","dependent_eligibility_claims_suppressed":true}))? { return out.finish(); }
    let mut hard_problems =
        BTreeMap::<String, Vec<&crate::validation_snapshot::SourceProblem>>::new();
    let mut soft_problems =
        BTreeMap::<String, Vec<&crate::validation_snapshot::SourceProblem>>::new();
    for problem in &snapshot.source_problems {
        if problem.hard {
            hard_problems
                .entry(problem.employee.clone())
                .or_default()
                .push(problem);
        } else {
            soft_problems
                .entry(problem.employee.clone())
                .or_default()
                .push(problem);
        }
    }
    let mut source_failures = BTreeMap::<&str, BTreeSet<&str>>::new();
    for problem in &snapshot.source_problems {
        source_failures
            .entry(&problem.employee)
            .or_default()
            .insert(problem.kind);
    }
    let mut hard_reasons = snapshot.employee_bad.clone();
    for (id, reasons) in &tainted {
        hard_reasons
            .entry(id.clone())
            .or_default()
            .extend(reasons.iter().cloned());
    }
    for (id, problems) in &hard_problems {
        for p in problems {
            hard_reasons
                .entry(id.clone())
                .or_default()
                .push(format!("unsupported_or_corrupt_{}", p.kind));
        }
    }
    if let Some(policy) = &policy {
        for (id, e) in &snapshot.employees {
            if e.data.maximum_period == RulePeriod::TwoWeek
                && e.data.maximum_paid_minutes.is_some()
                && policy.anchor.is_none()
            {
                hard_reasons
                    .entry(id.clone())
                    .or_default()
                    .push("two_week_maximum_anchor".to_owned());
            }
        }
    }
    for (id, reasons) in &mut hard_reasons {
        reasons.sort();
        reasons.dedup();
        if !out.emit(C::HardRuleDataIncomplete,Target::employee(id),json!({"missing_or_invalid_facts":reasons,"source_ids":limited_ids(hard_problems.get(id).into_iter().flatten().map(|p|p.id.as_str())),"policy_revisions":revisions(&snapshot,&["two_week_period_anchor_date"])}),|f|{for p in hard_problems.get(id).into_iter().flatten(){f.text(&p.id);f.text(p.kind);f.text(&p.material.to_string());}Ok(())})?{return out.finish();}
    }
    let mut evaluations = BTreeMap::<
        usize,
        (
            Vec<ExclusionReason>,
            crate::availability_rules::AvailabilityAssessment,
        ),
    >::new();
    if let (Some(policy), Some(indexes)) = (&policy, &indexes) {
        for (i, row) in rows.iter().enumerate() {
            let Some(employee) = row
                .employee
                .as_ref()
                .and_then(|id| snapshot.employees.get(id))
            else {
                continue;
            };
            let rules = slot_rules(row, policy);
            if !row.membership || !row.trustworthy_interval {
                continue;
            }
            let Some(rules) = rules else {
                continue;
            };
            match scheduling_rules::evaluate_slot(rules, employee, indexes.refs()) {
                Ok(result) => {
                    for reason in &result.0 {
                        let code = rule_code(*reason);
                        if !source_authoritative(code, &employee.id, &source_failures) {
                            continue;
                        }
                        if code.severity() != Severity::Error {
                            continue;
                        }
                        if !emit_rule(&mut out, code, row, employee, policy, indexes, &snapshot)? {
                            return out.finish();
                        }
                    }
                    evaluations.insert(i, result);
                }
                Err(_) => {
                    if !out.plain(
                        C::HardRuleDataIncomplete,
                        Target::row(row),
                        json!({"missing_fact":"assignment_rule_evaluation"}),
                    )? {
                        return out.finish();
                    }
                }
            }
        }
    }
    let mut periods =
        BTreeMap::<(String, &'static str, LocalDate), (LocalDate, i64, i64, bool)>::new();
    if let Some(policy) = &policy {
        for (id, e) in &snapshot.employees {
            if tainted.contains_key(id)
                || snapshot.employee_bad.contains_key(id)
                || unknown_membership
            {
                continue;
            }
            let two_week = e.data.minimum_period == RulePeriod::TwoWeek
                || e.data.maximum_period == RulePeriod::TwoWeek;
            for kind in [RulePeriod::Week, RulePeriod::TwoWeek] {
                if kind == RulePeriod::TwoWeek && (!two_week || policy.anchor.is_none()) {
                    continue;
                }
                let label = if kind == RulePeriod::Week {
                    "week"
                } else {
                    "two_week"
                };
                let mut date = start;
                loop {
                    match policy.period(date, kind) {
                        Ok(p) => {
                            periods.entry((id.clone(), label, p.start)).or_insert((
                                p.end_exclusive,
                                0,
                                0,
                                p.start >= start && p.end_exclusive.days_since(end) <= 1,
                            ));
                        }
                        Err(_) => {
                            if !out.plain(
                                C::HardRuleDataIncomplete,
                                Target::employee(id),
                                json!({"missing_fact":"bounded_work_period","period":label}),
                            )? {
                                return out.finish();
                            }
                            break;
                        }
                    }
                    if date == end {
                        break;
                    }
                    date = date.add_days(1)?;
                }
            }
        }
        for row in &rows {
            let (Some(id), Some(date), Some(paid)) = (&row.employee, row.date, row.paid) else {
                continue;
            };
            if !row.membership || tainted.contains_key(id) || unknown_membership {
                continue;
            }
            for (kind, label) in [
                (RulePeriod::Week, "week"),
                (RulePeriod::TwoWeek, "two_week"),
            ] {
                if kind == RulePeriod::TwoWeek && policy.anchor.is_none() {
                    continue;
                }
                if let Ok(p) = policy.period(date, kind)
                    && let Some(total) = periods.get_mut(&(id.clone(), label, p.start))
                {
                    if let (Some(minutes), Some(count)) =
                        (total.1.checked_add(paid), total.2.checked_add(1))
                    {
                        total.1 = minutes;
                        total.2 = count;
                    } else {
                        tainted
                            .entry(id.clone())
                            .or_default()
                            .insert("paid_total_overflow".to_owned());
                        if !out.plain(
                            C::HardRuleDataIncomplete,
                            Target::employee(id),
                            json!({"missing_fact":"checked_period_totals"}),
                        )? {
                            return out.finish();
                        }
                    }
                }
            }
        }
        for ((id, kind, first), total) in &periods {
            let e = &snapshot.employees[id];
            let rule = if *kind == "week" {
                RulePeriod::Week
            } else {
                RulePeriod::TwoWeek
            };
            if tainted.contains_key(id) {
                continue;
            }
            if e.data.maximum_period == rule
                && e.data.maximum_paid_minutes.is_some_and(|m| total.1 > m)
            {
                let max = e.data.maximum_paid_minutes.unwrap_or_default();
                if !out.plain(C::MaximumPaidMinutesExceeded,Target{date:Some(first.format()),..Target::employee(id)},json!({"period":kind,"period_start":first.format(),"period_end_exclusive":total.0.format(),"complete_period":total.3,"metric":"paid_minutes","total":total.1,"threshold":max,"excess":total.1-max,"policy_revisions":revisions(&snapshot,&["workweek_start_weekday","two_week_period_anchor_date"])}))?{return out.finish();}
            }
        }
    }
    // Sweep: O(n log n + emitted pairs), never an all-pairs collection. Active
    // IDs give deterministic pair emission; expiry is ordered by actual end.
    let mut overlap_participants = BTreeSet::<usize>::new();
    let mut chronological = BTreeMap::<String, Vec<usize>>::new();
    let mut unorderable = BTreeSet::<String>::new();
    for (id, indices) in &by_employee {
        if indices.iter().any(|i| rows[*i].start.is_none()) {
            unorderable.insert(id.clone());
        }
        let mut order = indices.clone();
        order.sort_unstable_by_key(|i| {
            (
                rows[*i].start,
                rows[*i].interval.map(|v| v.end()),
                rows[*i].raw.id(),
            )
        });
        let mut active = BTreeMap::<String, usize>::new();
        let mut expires = BTreeSet::<(LocalDateTime, String, usize)>::new();
        for &i in &order {
            let row = &rows[i];
            let Some(interval) = row.interval.filter(|_| row.membership) else {
                continue;
            };
            while let Some((end, id, j)) = expires.first().cloned() {
                if end > interval.start() {
                    break;
                }
                expires.remove(&(end, id.clone(), j));
                active.remove(&id);
            }
            for &j in active.values() {
                let other = &rows[j];
                let Some(other_interval) = other.interval else {
                    continue;
                };
                if !out.plain(C::AssignmentOverlapConflict,Target::pair(other,row),json!({"intervals":pair_intervals(other,row),"intersection_start":interval.start().max(other_interval.start()).format(),"intersection_end":interval.end().min(other_interval.end()).format()}))?{return out.finish();}
                overlap_participants.insert(i);
                overlap_participants.insert(j);
            }
            active.insert(row.raw.id().to_owned(), i);
            expires.insert((interval.end(), row.raw.id().to_owned(), i));
        }
        chronological.insert(id.clone(), order);
    }
    // Every hard pass has finished. Warning passes begin here.
    if audit_bad&&!out.plain(C::IncompleteData,Target::default(),json!({"missing_fact":"generation_audit_metadata","recognized_count_format":known,"historical_count_evidence_available":known&&expected.is_some(),"current_policy_used":true}))?{return out.finish();}
    if let Some(policy)=&policy&&policy.anchor.is_none()&&snapshot.employees.values().any(|e|e.data.minimum_period==RulePeriod::TwoWeek||e.data.maximum_period==RulePeriod::TwoWeek) && !out.plain(C::TwoWeekAnchorMissing,Target::default(),json!({"anchor":null,"minimums_suppressed":true,"policy_revisions":revisions(&snapshot,&["two_week_period_anchor_date"])}))?{return out.finish();}
    for (id, problems) in &soft_problems {
        if !out.emit(C::IncompleteData,Target::employee(id),json!({"missing_fact":"soft_rule_source","source_ids":limited_ids(problems.iter().map(|p|p.id.as_str()))}),|f|{for p in problems{f.text(&p.id);f.text(p.kind);f.text(&p.material.to_string());}Ok(())})?{return out.finish();}
    }
    for (i, row) in rows.iter().enumerate() {
        if !row.legacy.is_empty()
            && !out.emit(
                C::IncompleteData,
                Target::row(row),
                json!({"missing_fact":"legacy_snapshots","fallbacks":limited(&row.legacy)}),
                |f| {
                    for (field, id) in &row.legacy {
                        f.text(field);
                        f.text(id);
                        f.text(&row.raw.0[field].to_string());
                    }
                    Ok(())
                },
            )?
        {
            return out.finish();
        }
        let valid_open = row.raw.b("open_shift") == Some(true)
            && row.membership
            && row.bad.is_empty()
            && row.missing.is_empty()
            && row.trustworthy_interval
            && row.structural
            && row.paid.is_some()
            && row.interval.is_some_and(|i| {
                wall_clock_minutes(i.start(), i.end()).is_ok_and(|elapsed| {
                    row.raw.n("shift_lunch_minutes_snapshot")
                        == Some(if elapsed >= 480 { 30 } else { 0 })
                        && row.paid
                            == elapsed.checked_sub(
                                row.raw.n("shift_lunch_minutes_snapshot").unwrap_or(-1),
                            )
                })
            });
        if valid_open && !out.plain(C::OpenShift,Target::row(row),json!({"operational_date":row.date.map(LocalDate::format),"actual_start":row.raw.0.get("actual_start_datetime"),"actual_end":row.raw.0.get("actual_end_datetime"),"coverage_item_id":row.raw.s("coverage_item_id"),"slot_index":row.raw.n("coverage_slot_index_snapshot")}))?{return out.finish();}
        if let (Some(policy), Some(indexes), Some((reasons, assessment)), Some(employee)) = (
            &policy,
            &indexes,
            evaluations.get(&i),
            row.employee
                .as_ref()
                .and_then(|id| snapshot.employees.get(id)),
        ) {
            for reason in reasons {
                let code = rule_code(*reason);
                if !source_authoritative(code, &employee.id, &source_failures) {
                    continue;
                }
                if code.severity() != Severity::Warning {
                    continue;
                }
                if !emit_rule(&mut out, code, row, employee, policy, indexes, &snapshot)? {
                    return out.finish();
                }
            }
            if assessment.requested_pto
                && source_authoritative(C::RequestedPtoOverlap, &employee.id, &source_failures)
                && !emit_rule(
                    &mut out,
                    C::RequestedPtoOverlap,
                    row,
                    employee,
                    policy,
                    indexes,
                    &snapshot,
                )?
            {
                return out.finish();
            }
            if assessment.facts.has_relevant_preferred_entries
                && !assessment.facts.has_preferred_overlap
                && source_authoritative(
                    C::PreferredAvailabilityNotMet,
                    &employee.id,
                    &source_failures,
                )
                && !emit_rule(
                    &mut out,
                    C::PreferredAvailabilityNotMet,
                    row,
                    employee,
                    policy,
                    indexes,
                    &snapshot,
                )?
            {
                return out.finish();
            }
            if employee.data.preferred_time_of_day.is_some_and(|p|Some(p)!=row.category)&&!out.plain(C::PreferredTimeMismatch,Target::row(row),json!({"preferred_category":employee.data.preferred_time_of_day,"assignment_category":row.category,"actual_start":row.start.map(LocalDateTime::format),"actual_end":row.interval.map(|i|i.end().format())}))?{return out.finish();}
        }
    }
    if let Some(policy) = &policy {
        for ((id, kind, first), total) in &periods {
            let e = &snapshot.employees[id];
            if tainted.contains_key(id) {
                continue;
            }
            let rule = if *kind == "week" {
                RulePeriod::Week
            } else {
                RulePeriod::TwoWeek
            };
            if *kind=="week"&&total.1>policy.overtime&&!out.plain(C::OvertimeWeekExceeded,Target{date:Some(first.format()),..Target::employee(id)},json!({"period":"week","period_start":first.format(),"period_end_exclusive":total.0.format(),"complete_period":total.3,"metric":"paid_minutes","total":total.1,"threshold":policy.overtime,"excess":total.1-policy.overtime,"policy_revisions":revisions(&snapshot,&["workweek_start_weekday","weekly_overtime_threshold_minutes"])}))?{return out.finish();}
            if snapshot.employee_states.get(id) != Some(&true)
                || !total.3
                || incomplete_slots
                || e.data.minimum_period != rule
            {
                continue;
            }
            let (metric, amount, required) = if let Some(n) = e.data.minimum_paid_minutes {
                ("paid_minutes", total.1, n)
            } else if let Some(n) = e.data.minimum_assignment_count {
                ("assignment_count", total.2, n)
            } else {
                continue;
            };
            if amount < required {
                let code = match (e.data.employment_type, metric, rule, required) {
                    (EmploymentType::FullTime, "paid_minutes", RulePeriod::Week, 2400) => {
                        C::FullTimeWeekMinimumNotMet
                    }
                    (EmploymentType::PartTime, "paid_minutes", RulePeriod::TwoWeek, 2400) => {
                        C::PartTimeTwoWeekMinimumNotMet
                    }
                    _ => C::CustomEmployeeMinimumNotMet,
                };
                if !out.plain(code,Target{date:Some(first.format()),..Target::employee(id)},json!({"period":kind,"period_start":first.format(),"period_end_exclusive":total.0.format(),"complete_period":true,"metric":metric,"total":amount,"threshold":required,"shortfall":required-amount,"policy_revisions":revisions(&snapshot,&[if rule==RulePeriod::Week{"workweek_start_weekday"}else{"two_week_period_anchor_date"}])}))?{return out.finish();}
            }
        }
        for (id, indices) in &by_employee {
            if unknown_membership
                || indices
                    .iter()
                    .any(|i| !rows[*i].membership || rows[*i].date.is_none())
            {
                continue;
            }
            let dates: BTreeSet<_> = indices.iter().filter_map(|i| rows[*i].date).collect();
            let mut run: Option<(LocalDate, LocalDate)> = None;
            for date in dates {
                match run {
                    Some((a, b)) if date.days_since(b) == 1 => run = Some((a, date)),
                    Some((a, b)) => {
                        if !emit_run(&mut out, id, a, b, policy, &snapshot)? {
                            return out.finish();
                        }
                        run = Some((date, date));
                    }
                    None => run = Some((date, date)),
                }
            }
            if let Some((a, b)) = run
                && !emit_run(&mut out, id, a, b, policy, &snapshot)?
            {
                return out.finish();
            }
        }
        for (id, indices) in &chronological {
            if unorderable.contains(id) {
                continue;
            }
            for pair in indices.windows(2) {
                let (a, b) = (&rows[pair[0]], &rows[pair[1]]);
                if overlap_participants.contains(&pair[0])
                    || overlap_participants.contains(&pair[1])
                    || !a.membership
                    || !b.membership
                    || !a.trustworthy_interval
                    || !b.trustworthy_interval
                {
                    continue;
                }
                let (Some(ai), Some(bi), Some(recovery)) = (a.interval, b.interval, a.recovery)
                else {
                    continue;
                };
                let (gap, deficit, violates, also_minimum_rest) =
                    scheduling_rules::rest_fact(ai, bi, recovery, policy)?;
                if !violates {
                    continue;
                }
                let code = if recovery {
                    C::RecoveryRuleViolation
                } else {
                    C::MinimumRestViolation
                };
                if !out.plain(code,Target::pair(a,b),json!({"prior_assignment_id":a.raw.id(),"next_assignment_id":b.raw.id(),"intervals":pair_intervals(a,b),"required_rest_minutes":policy.rest,"gap_minutes":gap,"deficit_minutes":deficit,"recovery":recovery,"also_minimum_rest":also_minimum_rest,"policy_revisions":revisions(&snapshot,&["minimum_rest_minutes_between_shifts","quick_turnaround_rule_enabled"])}))?{return out.finish();}
            }
        }
    }
    out.finish()
}
fn pair_intervals(a: &Checked, b: &Checked) -> Value {
    let (first, second) = if a.raw.id() < b.raw.id() {
        (a, b)
    } else {
        (b, a)
    };
    json!([{ "assignment_id":first.raw.id(),"start":first.raw.s("actual_start_datetime"),"end":first.raw.s("actual_end_datetime") },{ "assignment_id":second.raw.id(),"start":second.raw.s("actual_start_datetime"),"end":second.raw.s("actual_end_datetime") }])
}
fn emit_run(
    out: &mut Output,
    id: &str,
    a: LocalDate,
    b: LocalDate,
    policy: &Policy,
    snapshot: &ValidationSnapshot,
) -> AppResult<bool> {
    let days = b.days_since(a) + 1;
    if days <= policy.consecutive {
        return Ok(true);
    }
    out.plain(C::ConsecutiveWorkdaysExceeded,Target{date:Some(a.format()),..Target::employee(id)},json!({"run_start":a.format(),"run_end":b.format(),"distinct_operational_days":days,"threshold":policy.consecutive,"excess":days-policy.consecutive,"policy_revisions":revisions(snapshot,&["max_consecutive_workdays"])}))
}
fn emit_rule(
    out: &mut Output,
    code: C,
    row: &Checked,
    employee: &Employee,
    policy: &Policy,
    indexes: &RuleIndexes,
    snapshot: &ValidationSnapshot,
) -> AppResult<bool> {
    let Some(interval) = row.interval else {
        return Ok(true);
    };
    let mut context =
        json!({"actual_start":interval.start().format(),"actual_end":interval.end().format()});
    let availability_code = match code {
        C::ApprovedPtoConflict => Some(RuleCode::ApprovedPtoConflict),
        C::AvailabilityHardConflict => Some(RuleCode::AvailabilityHardConflict),
        C::ExplicitAvailabilityMissing => Some(RuleCode::ExplicitAvailabilityMissing),
        C::RequestedPtoOverlap => Some(RuleCode::RequestedPtoOverlap),
        C::PreferredAvailabilityNotMet => Some(RuleCode::PreferredAvailabilityNotMet),
        _ => None,
    };
    let mut material = MaterialFacts::new("validation_source_facts", code.as_str());
    if let Some(rule) = availability_code {
        context["intervals"] =
            indexes
                .availability
                .display_material(&employee.id, interval, rule)?;
        if code.severity() == Severity::Warning {
            let (ids, omitted) = indexes.availability.material_sources(
                &employee.id,
                interval,
                rule,
                &mut material,
            )?;
            context["source_ids"] = json!(ids);
            context["omitted_source_id_count"] = json!(omitted);
        } else {
            indexes.availability.append_hard_material(
                &employee.id,
                interval,
                code == C::ExplicitAvailabilityMissing,
                &mut material,
            )?;
        }
        if code == C::ExplicitAvailabilityMissing {
            context["requires_explicit_availability"] = json!(true);
            context["coverage"] =
                indexes
                    .availability
                    .positive_material(&employee.id, interval, &mut material)?;
        }
    }
    match code {
        C::EmployeeRestrictionConflict => {
            if let Some(category) = row.category {
                let ids = indexes
                    .restrictions
                    .evaluate(&employee.id, category, interval)?;
                context["source_ids"] = limited_ids(ids.iter().copied());
                indexes.restrictions.append_hard_material(
                    &employee.id,
                    category,
                    interval,
                    &mut material,
                )?;
            }
        }
        C::ShiftTemplateRestrictionConflict | C::PositionRestrictionConflict => {
            let (map, key) = if code == C::ShiftTemplateRestrictionConflict {
                (&indexes.template_blocks, row.raw.s("shift_template_id"))
            } else {
                (&indexes.position_blocks, row.raw.s("shift_position_id"))
            };
            if let Some(key) = key
                && let Some(ids) = map.get(&employee.id).and_then(|m| m.get(key))
            {
                context["source_ids"] = limited_ids(ids.iter().map(String::as_str));
                material.text(key);
                for id in ids {
                    material.text(id);
                }
            }
        }
        C::WeekendRotationMismatch => {
            context["anchor"] = json!(employee.data.weekend_rotation_anchor_date);
            context["pattern"] = json!(employee.data.weekend_rotation_pattern);
        }
        C::HolidayGroupMismatch => {
            context["holiday_set"] = json!(row.holiday_set);
            context["required_group"] = json!(holiday_group(row, policy));
            context["employee_group"] = json!(employee.data.holiday_group);
            context["rotation_start_year"] = json!(policy.holiday_year);
            context["policy_revisions"] = revisions(snapshot, &["holiday_rotation_start_year"]);
        }
        C::PrnWeekdayAvailabilityMissing => {
            context["employment_type"] = json!(employee.data.employment_type);
            context["ordinary_weekday"] = json!(true);
            context["requires_explicit_availability"] = json!(false);
            context["coverage"] =
                indexes
                    .availability
                    .positive_material(&employee.id, interval, &mut material)?;
        }
        C::NightShiftIneligible => {
            context["night_snapshot"] = json!(row.night);
            context["can_work_nights"] = json!(employee.data.can_work_night_shift);
        }
        C::OrthoOrIneligible => {
            context["position_seed"] = json!(row.raw.s("position_seed_key_snapshot"));
            context["can_work_ortho_or"] = json!(employee.data.can_work_ortho_or);
        }
        _ => {}
    }
    let digest = material.finish();
    out.emit(code, Target::row(row), context, |f| {
        f.text(&digest);
        Ok(())
    })
}

/// Manager-invoked CLI access to the same read-only command. Summary only: no
/// assignment contents, names, notes or PTO details are printed or persisted.
pub fn execute_cli(mut args: impl Iterator<Item = String>) {
    use crate::{
        database::DatabasePaths,
        error::{AppError, CommandResult, ErrorCode},
    };
    use std::path::PathBuf;
    #[derive(Serialize)]
    struct Summary {
        schedule_id: String,
        error_count: usize,
        warning_count: usize,
        complete: bool,
        truncated: bool,
        code_counts: BTreeMap<ValidationCode, usize>,
        finding_ids: Vec<String>,
    }
    let result = match args.next().zip(args.next()) {
        Some((database, id)) if args.next().is_none() => tauri::async_runtime::block_on(async {
            let main = PathBuf::from(database);
            if !main.is_absolute() {
                return Err(AppError::new(
                    ErrorCode::InvalidText,
                    "Specify an absolute path to an existing database.",
                ));
            }
            let directory = main
                .parent()
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::InvalidText,
                        "The database needs a parent directory.",
                    )
                })?
                .to_path_buf();
            let value = validate_schedule(&DatabasePaths { directory, main }, &id).await?;
            let mut code_counts = BTreeMap::new();
            for finding in &value.findings {
                *code_counts.entry(finding.code).or_default() += 1;
            }
            Ok(Summary {
                schedule_id: value.schedule_id,
                error_count: value.error_count,
                warning_count: value.warning_count,
                complete: value.complete,
                truncated: value.truncated,
                code_counts,
                finding_ids: value.findings.into_iter().map(|f| f.id).collect(),
            })
        }),
        _ => Err(AppError::new(
            ErrorCode::InvalidSchedule,
            "Usage: shift-happens --validate-schedule ABSOLUTE_DATABASE_PATH SCHEDULE_ID. This reads saved data without changing it.",
        )),
    };
    let failed = result.is_err();
    match serde_json::to_string(&CommandResult::from(result)) {
        Ok(value) => println!("{value}"),
        Err(_) => {
            eprintln!("The validation summary could not be formatted.");
            std::process::exit(1);
        }
    }
    if failed {
        std::process::exit(1);
    }
}

fn source_authoritative(
    code: C,
    employee: &str,
    failures: &BTreeMap<&str, BTreeSet<&str>>,
) -> bool {
    // A valid source interval proves a positive conflict even when another
    // source row is corrupt. In particular, malformed soft PTO/preference data
    // must never hide an independently observed approved-PTO or hard-block
    // error. Only absence/full-coverage claims require the complete source set:
    // an omitted interval could supply availability or a preferred overlap.
    let source = match code {
        C::ExplicitAvailabilityMissing
        | C::PrnWeekdayAvailabilityMissing
        | C::PreferredAvailabilityNotMet => "availability_entries",
        _ => return true,
    };
    !failures
        .get(employee)
        .is_some_and(|kinds| kinds.contains(source))
}
