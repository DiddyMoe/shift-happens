//! Deterministic greedy scheduling. No lookahead, random ties, or cross-schedule totals.
//! Generation never breaks a work-rule limit (one shift per operational day, rest,
//! recovery, consecutive days); a slot no one can take within them stays open.
use crate::{
    availability_rules::RuleCode,
    employees::{EmploymentType, RulePeriod},
    error::{AppError, AppResult, ErrorCode},
    generation_inputs::{self, PlannedSlot, RuleInputs, add, arithmetic_error},
    local_values::{LocalDate, LocalDateTime, LocalInterval, Weekday},
    metadata::MetadataClock,
    rule_facts::MaterialFacts,
    schedule_assignments::{
        GeneratedEmployee, GeneratedSchedulePlan, PlannedAssignment, store_generated_plan,
    },
    schedule_write::SCHEDULE_WRITES,
    schedules::{self, Schedule},
    text::{normalize_required_text, stable_text_order_key},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use std::time::Instant;

pub const GENERATION_WARNING_LIMIT: usize = 10_000;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateScheduleRequest {
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub make_current: bool,
    pub expected_current_setting_revision: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationReadinessRequest {
    pub start_date: String,
    pub end_date: String,
}
fn range(start: &str, end: &str) -> AppResult<(LocalDate, LocalDate)> {
    let start = LocalDate::parse(start)?;
    let end = LocalDate::parse(end)?;
    if start > end || end.days_since(start) >= crate::local_values::SCHEDULE_DAY_LIMIT as i64 {
        return Err(AppError::new(
            ErrorCode::InvalidSchedule,
            "Choose an inclusive range from 1 through 370 days, with the start on or before the end.",
        ));
    }
    Ok((start, end))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GenerationCode {
    CandidateEligibilityDataIncomplete,
    GenerationWarningOutputLimitExceeded,
    OpenShiftNoQualifyingCandidate,
    OvertimeAvoidanceFailed,
    PreferredAvailabilityNotMet,
    PreferredTimeMismatch,
    RequestedPtoAvoidanceFailed,
    SoftRuleDataIncomplete,
}
impl GenerationCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CandidateEligibilityDataIncomplete => "candidate_eligibility_data_incomplete",
            Self::GenerationWarningOutputLimitExceeded => {
                "generation_warning_output_limit_exceeded"
            }
            Self::OpenShiftNoQualifyingCandidate => "open_shift_no_qualifying_candidate",
            Self::OvertimeAvoidanceFailed => "overtime_avoidance_failed",
            Self::PreferredAvailabilityNotMet => RuleCode::PreferredAvailabilityNotMet.as_str(),
            Self::PreferredTimeMismatch => {
                crate::validation::ValidationCode::PreferredTimeMismatch.as_str()
            }
            Self::RequestedPtoAvoidanceFailed => "requested_pto_avoidance_failed",
            Self::SoftRuleDataIncomplete => "soft_rule_data_incomplete",
        }
    }
}
impl Serialize for GenerationCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
// ConsecutiveWorkdayLimit, MinimumRestNotMet, RecoveryNotMet and SameDayAssignment
// exclude only during generation; manual edits report them as warnings to acknowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    ApprovedPtoConflict,
    AvailabilityHardConflict,
    CandidateEligibilityDataIncomplete,
    ConsecutiveWorkdayLimit,
    EmployeeAssignmentOverlap,
    EmployeeRestriction,
    ExplicitAvailabilityMissing,
    HolidayCapabilityMissing,
    HolidayGroupMismatch,
    MaximumPaidMinutesExceeded,
    MinimumRestNotMet,
    NightCapabilityMissing,
    OrthoOrCapabilityMissing,
    PositionRestriction,
    PrnWeekdayAvailabilityMissing,
    RecoveryNotMet,
    SameDayAssignment,
    ShiftTemplateRestriction,
    WeekendCapabilityMissing,
    WeekendOffRotation,
}
#[derive(Serialize)]
pub struct GenerationWarning {
    pub code: GenerationCode,
    pub warning_id: String,
    pub context: Value,
    pub source_ids: Vec<String>,
    pub omitted_source_id_count: usize,
}
#[derive(Default)]
struct Warnings {
    details: Vec<GenerationWarning>,
    counts: BTreeMap<GenerationCode, i64>,
    total: i64,
    truncated: bool,
}
impl Warnings {
    fn emit(
        &mut self,
        code: GenerationCode,
        make: impl FnOnce() -> AppResult<GenerationWarning>,
    ) -> AppResult<()> {
        self.total = add(self.total, 1)?;
        let count = self.counts.entry(code).or_default();
        *count = add(*count, 1)?;
        if self.details.len() < GENERATION_WARNING_LIMIT - 1 {
            self.details.push(make()?);
        } else {
            self.truncated = true;
        }
        Ok(())
    }
    fn finish(mut self, start: LocalDate, end: LocalDate) -> AppResult<Self> {
        if self.truncated {
            let code = GenerationCode::GenerationWarningOutputLimitExceeded;
            let context = json!({"details_truncated": true, "limit": GENERATION_WARNING_LIMIT, "total_warning_count": self.total, "warning_counts": self.counts, "start_date": start.format(), "end_date": end.format()});
            let mut facts = MaterialFacts::new("generate_schedule", code.as_str());
            facts.text(&serde_json::to_string(&context).map_err(|_| arithmetic_error())?);
            self.details.push(GenerationWarning {
                code,
                warning_id: facts.finish(),
                context,
                source_ids: Vec::new(),
                omitted_source_id_count: 0,
            });
        }
        Ok(self)
    }
}
#[derive(Serialize)]
pub struct GenerationDiagnostics {
    pub source_row_count: usize,
    pub preflight_sql_read_count: i64,
    pub expanded_interval_count: usize,
    pub candidate_evaluation_count: i64,
    pub elapsed_milliseconds: u128,
}
#[derive(Serialize)]
pub struct GenerationResult {
    pub schedule: Schedule,
    pub assignment_count: usize,
    pub assigned_count: i64,
    pub open_count: i64,
    pub made_current: bool,
    pub warnings: Vec<GenerationWarning>,
    pub warning_counts: BTreeMap<GenerationCode, i64>,
    pub total_warning_count: i64,
    pub details_truncated: bool,
    pub partial_minimum_periods: Vec<generation_inputs::PartialMinimumPeriod>,
    pub diagnostics: GenerationDiagnostics,
}
#[derive(Default, Clone, Copy)]
pub(crate) struct Totals {
    pub paid: i64,
    pub assignments: i64,
}
impl Totals {
    pub(crate) fn projected(self, paid: i64) -> AppResult<Self> {
        Ok(Self {
            paid: add(self.paid, paid)?,
            assignments: add(self.assignments, 1)?,
        })
    }
}
struct SelectedInterval {
    end: LocalDateTime,
    slot: usize,
}
#[derive(Default)]
pub(crate) struct Workload {
    weeks: BTreeMap<LocalDate, Totals>,
    two_weeks: BTreeMap<LocalDate, Totals>,
    pub whole: Totals,
    intervals: BTreeMap<(LocalDateTime, usize), SelectedInterval>,
    has_overlapping_intervals: bool,
    // Disjoint maximal inclusive runs, so bridge lookup is logarithmic.
    runs: BTreeMap<LocalDate, LocalDate>,
    // Selected slots by operational date.
    days: BTreeMap<LocalDate, Vec<usize>>,
}
impl Workload {
    pub(crate) fn same_day(&self, date: LocalDate) -> &[usize] {
        self.days.get(&date).map_or(&[], Vec::as_slice)
    }
    pub(crate) fn total(&self, slot: &PlannedSlot, period: RulePeriod) -> AppResult<Totals> {
        match period {
            RulePeriod::Week => Ok(self
                .weeks
                .get(&slot.week.start)
                .copied()
                .unwrap_or_default()),
            RulePeriod::TwoWeek => Ok(self
                .two_weeks
                .get(&slot.two_week.as_ref().ok_or_else(arithmetic_error)?.start)
                .copied()
                .unwrap_or_default()),
            RulePeriod::None => Ok(Totals::default()),
        }
    }
    pub(crate) fn neighbors(
        &self,
        interval: LocalInterval,
    ) -> (Option<usize>, Option<usize>, bool) {
        let prior = self.intervals.range(..(interval.start(), 0)).next_back();
        let next = self.intervals.range((interval.start(), 0)..).next();
        let prior_overlap = if self.has_overlapping_intervals {
            self.intervals
                .range(..(interval.start(), 0))
                .any(|(_, p)| p.end > interval.start())
        } else {
            prior.is_some_and(|(_, p)| p.end > interval.start())
        };
        let overlap = prior_overlap || next.is_some_and(|(start, _)| start.0 < interval.end());
        (
            prior.map(|(_, p)| p.slot),
            next.map(|(_, p)| p.slot),
            overlap,
        )
    }
    fn projected_run(&self, date: LocalDate) -> (LocalDate, LocalDate) {
        let prior = self.runs.range(..=date).next_back();
        if let Some((&first, &last)) = prior
            && date <= last
        {
            return (first, last);
        }
        let first = prior
            .filter(|(_, last)| date.days_since(**last) == 1)
            .map_or(date, |(first, _)| *first);
        let last = self
            .runs
            .range(date..)
            .next()
            .filter(|(first, _)| first.days_since(date) == 1)
            .map_or(date, |(_, last)| *last);
        (first, last)
    }
    pub(crate) fn select(&mut self, slot: &PlannedSlot, index: usize) -> AppResult<()> {
        let paid = slot.slot.structure().shift_paid_minutes_snapshot;
        let week = self.weeks.entry(slot.week.start).or_default();
        *week = week.projected(paid)?;
        if let Some(period) = &slot.two_week {
            let total = self.two_weeks.entry(period.start).or_default();
            *total = total.projected(paid)?;
        }
        self.whole = self.whole.projected(paid)?;
        let (first, last) = self.projected_run(slot.date);
        if let Some((&key, &end)) = self.runs.range(..=slot.date).next_back()
            && slot.date.days_since(end) <= 1
        {
            self.runs.remove(&key);
        }
        if let Some((&key, _)) = self.runs.range(slot.date..).next()
            && key.days_since(slot.date) == 1
        {
            self.runs.remove(&key);
        }
        self.runs.insert(first, last);
        self.days.entry(slot.date).or_default().push(index);
        self.has_overlapping_intervals |= self.neighbors(slot.interval).2;
        self.intervals.insert(
            (slot.interval.start(), index),
            SelectedInterval {
                end: slot.interval.end(),
                slot: index,
            },
        );
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) struct GapFact {
    pub prior: usize,
    pub next: usize,
    pub gap: i64,
    pub deficit: i64,
    pub recovery: bool,
    pub also_minimum_rest: bool,
    pub violates: bool,
}
pub(crate) struct Candidate {
    // Fifteen numeric tuple components in Prompt 9 order. Missing/non-authoritative
    // minimum completion = 10,000; absent/unevaluable time preference = 1;
    // absent availability preference = 1. Lower integer risk always wins.
    // Components 0–4 (recovery, rest, consecutive excess) are always zero for a
    // generation candidate, since a shortfall excludes it; manual edits warn on them.
    pub rank: [i64; 15],
    pub employee: usize,
    pub gaps: Vec<GapFact>,
    pub run: (LocalDate, LocalDate),
    // The employee's other slots on this operational date.
    pub same_day: Vec<usize>,
    pub overrides: Vec<ExclusionReason>,
    pub exclusions: Vec<ExclusionReason>,
}
pub(crate) fn gap_fact(
    prior: usize,
    next: usize,
    slots: &[PlannedSlot],
    inputs: &RuleInputs,
    include_safe: bool,
) -> AppResult<Option<GapFact>> {
    let a = &slots[prior];
    let b = &slots[next];
    let recovery = a.slot.structure().shift_recovery_snapshot;
    let (gap, deficit, violates, also_minimum_rest) =
        crate::scheduling_rules::rest_fact(a.interval, b.interval, recovery, &inputs.policy)?;
    if !include_safe && !violates {
        return Ok(None);
    }
    Ok(Some(GapFact {
        prior,
        next,
        gap,
        deficit,
        recovery,
        violates,
        also_minimum_rest,
    }))
}
pub(crate) fn evaluate(
    inputs: &RuleInputs,
    state: &Workload,
    slot_index: usize,
    employee_index: usize,
    start: LocalDate,
    end: LocalDate,
    manual: bool,
) -> AppResult<Result<Candidate, Vec<ExclusionReason>>> {
    let slot = &inputs.slots[slot_index];
    let s = slot.slot.structure();
    let employee = &inputs.employees[employee_index];
    let e = &employee.data;
    let (mut exclusions, availability) = crate::scheduling_rules::evaluate_slot(
        crate::scheduling_rules::SlotRules {
            date: slot.date,
            interval: slot.interval,
            category: s.shift_category_snapshot,
            shift_id: &s.shift_template_id,
            position_id: &s.shift_position_id,
            night: s.shift_night_snapshot,
            ortho: s.position_seed_key_snapshot.as_deref() == Some("position_ortho_or"),
            is_holiday: s.holiday_id.is_some(),
            holiday_group: slot.holiday_group,
        },
        employee,
        crate::scheduling_rules::IndexRefs {
            restrictions: &inputs.restrictions,
            availability: &inputs.availability,
            template_blocks: &inputs.template_blocks,
            position_blocks: &inputs.position_blocks,
        },
    )?;
    let maximum_incomplete = manual
        && e.maximum_paid_minutes.is_some()
        && e.maximum_period == RulePeriod::TwoWeek
        && inputs.policy.anchor.is_none();
    if maximum_incomplete {
        exclusions.push(ExclusionReason::CandidateEligibilityDataIncomplete);
    }
    if !maximum_incomplete
        && let Some(maximum) = e.maximum_paid_minutes
        && state
            .total(slot, e.maximum_period)?
            .projected(s.shift_paid_minutes_snapshot)?
            .paid
            > maximum
    {
        exclusions.push(ExclusionReason::MaximumPaidMinutesExceeded);
    }
    let (prior, next, overlap) = state.neighbors(slot.interval);
    if overlap {
        exclusions.push(ExclusionReason::EmployeeAssignmentOverlap);
    }
    let mut gaps = Vec::with_capacity(2);
    if let Some(prior) = prior
        && let Some(gap) = gap_fact(prior, slot_index, &inputs.slots, inputs, false)?
    {
        gaps.push(gap);
    }
    if let Some(next) = next
        && let Some(gap) = gap_fact(slot_index, next, &inputs.slots, inputs, false)?
    {
        gaps.push(gap);
    }
    let mut recovery_deficit = 0;
    let mut recovery_pairs = 0;
    let mut rest_deficit = 0;
    let mut rest_pairs = 0;
    for gap in &gaps {
        if gap.recovery {
            recovery_deficit = add(recovery_deficit, gap.deficit)?;
            recovery_pairs = add(recovery_pairs, 1)?;
        } else {
            rest_deficit = add(rest_deficit, gap.deficit)?;
            rest_pairs = add(rest_pairs, 1)?;
        }
    }
    let run = state.projected_run(slot.date);
    let consecutive_excess = add(run.1.days_since(run.0), 1)?
        .checked_sub(inputs.policy.consecutive)
        .ok_or_else(arithmetic_error)?
        .max(0);
    let same_day = state.same_day(slot.date);
    let mut overrides = Vec::new();
    if manual {
        exclusions.retain(|reason| {
            if matches!(
                reason,
                ExclusionReason::WeekendOffRotation
                    | ExclusionReason::HolidayGroupMismatch
                    | ExclusionReason::PrnWeekdayAvailabilityMissing
            ) {
                overrides.push(*reason);
                false
            } else {
                true
            }
        });
    } else {
        for (breaks, reason) in [
            (!same_day.is_empty(), ExclusionReason::SameDayAssignment),
            (recovery_pairs > 0, ExclusionReason::RecoveryNotMet),
            (rest_pairs > 0, ExclusionReason::MinimumRestNotMet),
            (
                consecutive_excess > 0,
                ExclusionReason::ConsecutiveWorkdayLimit,
            ),
        ] {
            if breaks {
                exclusions.push(reason);
            }
        }
        if !exclusions.is_empty() {
            return Ok(Err(exclusions));
        }
    }
    let week = state
        .total(slot, RulePeriod::Week)?
        .projected(s.shift_paid_minutes_snapshot)?;
    let whole = state.whole.projected(s.shift_paid_minutes_snapshot)?;
    let (mut minimum_class, mut completion) = (1, 10_000);
    let minimum_incomplete =
        manual && e.minimum_period == RulePeriod::TwoWeek && inputs.policy.anchor.is_none();
    if e.minimum_period != RulePeriod::None && !minimum_incomplete {
        let period = inputs.policy.period(slot.date, e.minimum_period)?;
        if period.start >= start && period.end_exclusive.days_since(end) <= 1 {
            let current = state.total(slot, e.minimum_period)?;
            let projected = current.projected(s.shift_paid_minutes_snapshot)?;
            let (current, projected, required) = if let Some(required) = e.minimum_paid_minutes {
                (current.paid, projected.paid, required)
            } else {
                (
                    current.assignments,
                    projected.assignments,
                    e.minimum_assignment_count.ok_or_else(arithmetic_error)?,
                )
            };
            minimum_class = if current < required { 0 } else { 2 };
            completion = projected
                .checked_mul(10_000)
                .ok_or_else(arithmetic_error)?
                .checked_div(required)
                .ok_or_else(arithmetic_error)?
                .min(10_000);
        }
    }
    let preference = if availability.facts.has_preferred_overlap {
        0
    } else if !availability.facts.has_relevant_preferred_entries {
        1
    } else {
        2
    };
    let time_preference = e
        .preferred_time_of_day
        .map_or(1, |p| if p == s.shift_category_snapshot { 0 } else { 2 });
    Ok(Ok(Candidate {
        rank: [
            recovery_deficit,
            recovery_pairs,
            rest_deficit,
            rest_pairs,
            consecutive_excess,
            i64::from(availability.requested_pto),
            week.paid
                .checked_sub(inputs.policy.overtime)
                .ok_or_else(arithmetic_error)?
                .max(0),
            minimum_class,
            completion,
            week.paid,
            whole.paid,
            whole.assignments,
            i64::from(minimum_incomplete),
            preference,
            time_preference,
        ],
        employee: employee_index,
        gaps,
        run,
        same_day: same_day.to_vec(),
        overrides,
        exclusions,
    }))
}
fn slot_identity(slot: &PlannedSlot) -> Value {
    let s = slot.slot.structure();
    json!({"schedule_date": s.schedule_date, "coverage_template_id": s.coverage_template_id, "coverage_item_id": s.coverage_item_id, "slot_index": s.coverage_slot_index_snapshot, "actual_start": s.actual_start_datetime, "actual_end": s.actual_end_datetime})
}
fn warning_material(code: GenerationCode, context: &Value) -> AppResult<MaterialFacts> {
    let mut facts = MaterialFacts::new("generate_schedule", code.as_str());
    facts.text(&serde_json::to_string(context).map_err(|_| arithmetic_error())?);
    Ok(facts)
}
fn open_warning(
    inputs: &RuleInputs,
    workloads: &[Workload],
    index: usize,
    exclusions: &BTreeMap<ExclusionReason, i64>,
) -> AppResult<GenerationWarning> {
    let code = GenerationCode::OpenShiftNoQualifyingCandidate;
    let slot = &inputs.slots[index];
    let s = slot.slot.structure();
    let context = json!({"slot": slot_identity(slot), "employee_id": null, "facts": {"active_employee_count": inputs.employees.len(), "exclusion_counts": exclusions}});
    let mut facts = warning_material(code, &context)?;
    facts.text(
        &serde_json::to_string(&json!({
            "minimum_rest_minutes": inputs.policy.rest,
            "quick_turnaround": inputs.policy.quick,
            "max_consecutive_workdays": inputs.policy.consecutive,
        }))
        .map_err(|_| arithmetic_error())?,
    );
    for (employee, workload) in inputs.employees.iter().zip(workloads) {
        let e = &employee.data;
        let run = workload.projected_run(slot.date);
        let ordinary_prn = e.employment_type == EmploymentType::Prn
            && s.holiday_id.is_none()
            && !matches!(slot.date.weekday(), Weekday::Saturday | Weekday::Sunday);
        let positive = e.requires_explicit_availability || ordinary_prn;
        facts.text(&employee.id);
        facts.text(&serde_json::to_string(&json!({
            "explicit_availability": e.requires_explicit_availability,
            "ordinary_prn": ordinary_prn,
            "night_capability": s.shift_night_snapshot.then_some(e.can_work_night_shift),
            "ortho_or_capability": (s.position_seed_key_snapshot.as_deref() == Some("position_ortho_or")).then_some(e.can_work_ortho_or),
            "weekend_capability": matches!(slot.date.weekday(), Weekday::Saturday | Weekday::Sunday).then_some(e.can_work_weekends),
            "weekend_pattern": matches!(slot.date.weekday(), Weekday::Saturday | Weekday::Sunday).then_some(e.weekend_rotation_pattern),
            "weekend_anchor": if matches!(slot.date.weekday(), Weekday::Saturday | Weekday::Sunday) { e.weekend_rotation_anchor_date.as_deref() } else { None },
            "holiday_capability": s.holiday_id.as_ref().map(|_| e.can_work_holidays),
            "holiday_group": slot.holiday_group.map(|_| e.holiday_group), "required_group": slot.holiday_group,
            "maximum_paid_minutes": e.maximum_paid_minutes, "maximum_period": e.maximum_period,
            "current_period_paid_minutes": workload.total(slot, e.maximum_period)?.paid,
            "candidate_paid_minutes": s.shift_paid_minutes_snapshot,
            "works_operational_date": !workload.same_day(slot.date).is_empty(),
            "projected_run": [run.0.format(), run.1.format()],
        })).map_err(|_| arithmetic_error())?);
        inputs.availability.append_hard_material(
            &employee.id,
            slot.interval,
            positive,
            &mut facts,
        )?;
        inputs.restrictions.append_hard_material(
            &employee.id,
            s.shift_category_snapshot,
            slot.interval,
            &mut facts,
        )?;
        for (blocks, target) in [
            (&inputs.template_blocks, &s.shift_template_id),
            (&inputs.position_blocks, &s.shift_position_id),
        ] {
            if let Some(ids) = blocks.get(&employee.id).and_then(|b| b.get(target)) {
                for id in ids {
                    facts.text(id);
                    facts.text(target);
                }
            }
        }
        // Neighbors decide overlap, rest and recovery.
        let (prior, next, _) = workload.neighbors(slot.interval);
        for neighbor in [prior, next].into_iter().flatten() {
            facts.text(
                &serde_json::to_string(&slot_identity(&inputs.slots[neighbor]))
                    .map_err(|_| arithmetic_error())?,
            );
        }
    }
    Ok(GenerationWarning {
        code,
        warning_id: facts.finish(),
        context,
        source_ids: Vec::new(),
        omitted_source_id_count: 0,
    })
}
fn warning(
    inputs: &RuleInputs,
    index: usize,
    employee: Option<usize>,
    code: GenerationCode,
    material: Value,
) -> AppResult<GenerationWarning> {
    let slot = &inputs.slots[index];
    let employee_id = employee.map(|i| inputs.employees[i].id.as_str());
    let context =
        json!({"slot": slot_identity(slot), "employee_id": employee_id, "facts": material});
    let mut facts = warning_material(code, &context)?;
    let (source_ids, omitted_source_id_count) = match (code, employee_id) {
        (GenerationCode::RequestedPtoAvoidanceFailed, Some(id)) => inputs
            .availability
            .material_sources(id, slot.interval, RuleCode::RequestedPtoOverlap, &mut facts)?,
        (GenerationCode::PreferredAvailabilityNotMet, Some(id)) => {
            inputs.availability.material_sources(
                id,
                slot.interval,
                RuleCode::PreferredAvailabilityNotMet,
                &mut facts,
            )?
        }
        _ => (Vec::new(), 0),
    };
    Ok(GenerationWarning {
        code,
        warning_id: facts.finish(),
        context,
        source_ids,
        omitted_source_id_count,
    })
}
fn selected_warnings(
    inputs: &RuleInputs,
    index: usize,
    candidate: &Candidate,
    exclusions: &BTreeMap<ExclusionReason, i64>,
    warnings: &mut Warnings,
) -> AppResult<()> {
    // Alphabetical code order within each already sorted planned slot.
    for code in [
        GenerationCode::CandidateEligibilityDataIncomplete,
        GenerationCode::OvertimeAvoidanceFailed,
        GenerationCode::PreferredAvailabilityNotMet,
        GenerationCode::PreferredTimeMismatch,
        GenerationCode::RequestedPtoAvoidanceFailed,
        GenerationCode::SoftRuleDataIncomplete,
    ] {
        let r = &candidate.rank;
        let applies = match code {
            GenerationCode::CandidateEligibilityDataIncomplete => {
                exclusions.contains_key(&ExclusionReason::CandidateEligibilityDataIncomplete)
            }
            GenerationCode::OvertimeAvoidanceFailed => r[6] > 0,
            GenerationCode::PreferredAvailabilityNotMet => r[13] == 2,
            GenerationCode::PreferredTimeMismatch => r[14] == 2,
            GenerationCode::RequestedPtoAvoidanceFailed => r[5] > 0,
            GenerationCode::SoftRuleDataIncomplete => r[12] > 0,
            _ => false,
        };
        if !applies {
            continue;
        }
        warnings.emit(code, || {
            let slot = &inputs.slots[index];
            let material = match code {
                GenerationCode::OvertimeAvoidanceFailed => json!({"period_start": slot.week.start.format(), "period_end_exclusive": slot.week.end_exclusive.format(), "projected_paid_minutes": r[9], "threshold_minutes": inputs.policy.overtime, "excess_minutes": r[6]}),
                GenerationCode::PreferredTimeMismatch => json!({"preferred_category": inputs.employees[candidate.employee].data.preferred_time_of_day, "assignment_category": slot.slot.structure().shift_category_snapshot}),
                GenerationCode::CandidateEligibilityDataIncomplete => json!({"excluded_candidate_count": exclusions.get(&ExclusionReason::CandidateEligibilityDataIncomplete)}),
                GenerationCode::RequestedPtoAvoidanceFailed => json!({"requested_pto_overlap": true}),
                GenerationCode::PreferredAvailabilityNotMet => json!({"has_relevant_preference": true, "has_preferred_overlap": false}),
                _ => json!({"incomplete_fact": "soft_preference"}),
            };
            warning(inputs, index, Some(candidate.employee), code, material)
        })?;
    }
    Ok(())
}
struct Selection {
    assignments: Vec<Option<usize>>,
    warnings: Warnings,
    assigned: i64,
    open: i64,
    evaluations: i64,
}
fn select(inputs: &RuleInputs, start: LocalDate, end: LocalDate) -> AppResult<Selection> {
    let mut workloads: Vec<_> = inputs
        .employees
        .iter()
        .map(|_| Workload::default())
        .collect();
    let names: Vec<_> = inputs
        .employees
        .iter()
        .map(|e| stable_text_order_key(&e.data.display_name))
        .collect();
    let mut assignments = Vec::with_capacity(inputs.slots.len());
    let mut warnings = Warnings::default();
    let mut assigned = 0;
    let mut open = 0;
    let mut evaluations = 0;
    for index in 0..inputs.slots.len() {
        let mut best: Option<Candidate> = None;
        let mut exclusions = BTreeMap::new();
        for (employee, workload) in workloads.iter().enumerate() {
            evaluations = add(evaluations, 1)?;
            match evaluate(inputs, workload, index, employee, start, end, false)? {
                Ok(candidate) => {
                    let key = (
                        &candidate.rank,
                        &names[employee],
                        &inputs.employees[employee].id,
                    );
                    if best.as_ref().is_none_or(|b| {
                        key < (
                            &b.rank,
                            &names[b.employee],
                            &inputs.employees[b.employee].id,
                        )
                    }) {
                        best = Some(candidate);
                    }
                }
                Err(reasons) => {
                    for reason in reasons {
                        let count = exclusions.entry(reason).or_default();
                        *count = add(*count, 1)?;
                    }
                }
            }
        }
        if let Some(best) = best {
            selected_warnings(inputs, index, &best, &exclusions, &mut warnings)?;
            workloads[best.employee].select(&inputs.slots[index], index)?;
            assignments.push(Some(best.employee));
            assigned = add(assigned, 1)?;
        } else {
            if exclusions.contains_key(&ExclusionReason::CandidateEligibilityDataIncomplete) {
                warnings.emit(GenerationCode::CandidateEligibilityDataIncomplete, || warning(inputs, index, None, GenerationCode::CandidateEligibilityDataIncomplete, json!({"excluded_candidate_count": exclusions.get(&ExclusionReason::CandidateEligibilityDataIncomplete)})))?;
            }
            warnings.emit(GenerationCode::OpenShiftNoQualifyingCandidate, || {
                open_warning(inputs, &workloads, index, &exclusions)
            })?;
            assignments.push(None);
            open = add(open, 1)?;
        }
    }
    Ok(Selection {
        assignments,
        warnings: warnings.finish(start, end)?,
        assigned,
        open,
        evaluations,
    })
}

pub async fn get_generation_readiness(
    pool: &SqlitePool,
    request: GenerationReadinessRequest,
) -> AppResult<generation_inputs::GenerationReadiness> {
    let (start, end) = range(&request.start_date, &request.end_date)?;
    let mut tx = pool.begin().await?;
    let result = generation_inputs::preflight(&mut tx, start, end, None).await;
    tx.rollback().await?;
    result
        .map(|i| i.readiness)
        .map_err(generation_inputs::generation_error)
}
pub async fn generate_schedule(
    pool: &SqlitePool,
    clock: &dyn MetadataClock,
    request: GenerateScheduleRequest,
) -> AppResult<GenerationResult> {
    async fn generate(
        pool: &SqlitePool,
        clock: &dyn MetadataClock,
        request: GenerateScheduleRequest,
    ) -> AppResult<GenerationResult> {
        let started = Instant::now();
        let name = normalize_required_text(&request.name)?;
        let (start, end) = range(&request.start_date, &request.end_date)?;
        if request.make_current != request.expected_current_setting_revision.is_some() {
            return Err(AppError::new(
                ErrorCode::InvalidRevision,
                "Supply the expected current-setting revision only when making this schedule current.",
            ));
        }
        if let Some(expected) = request.expected_current_setting_revision {
            crate::employees::validate_revision(expected)?;
        }
        let (_guard, mut tx) = SCHEDULE_WRITES.begin(pool).await?;
        let inputs = generation_inputs::preflight(
            &mut tx,
            start,
            end,
            request.expected_current_setting_revision,
        )
        .await?;
        let (inputs, selection) = tauri::async_runtime::spawn_blocking(move || {
            let selection = select(&inputs.rules, start, end)?;
            Ok::<_, AppError>((inputs, selection))
        })
        .await
        .map_err(|_| arithmetic_error())??;
        let count = inputs.slots.len();
        let diagnostics = GenerationDiagnostics {
            source_row_count: inputs.source_count,
            preflight_sql_read_count: inputs.sql_read_count,
            expanded_interval_count: inputs.expanded_count,
            candidate_evaluation_count: selection.evaluations,
            elapsed_milliseconds: 0,
        };
        let assignments = inputs
            .rules
            .slots
            .into_iter()
            .zip(selection.assignments)
            .map(|(slot, employee)| {
                Ok(PlannedAssignment {
                    slot: slot.slot,
                    employee: employee
                        .map(|i| GeneratedEmployee::from_employee(&inputs.rules.employees[i]))
                        .transpose()?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let schedule = store_generated_plan(
            &mut tx,
            clock,
            GeneratedSchedulePlan {
                name,
                start_date: start.format(),
                end_date: end.format(),
                notes: None,
                snapshot: inputs.snapshot,
                assignments,
            },
        )
        .await?;
        if let Some(expected) = request.expected_current_setting_revision {
            schedules::change_current_in_transaction(&mut tx, clock, Some(&schedule.id), expected)
                .await?;
        }
        // Build the complete response before commit; nothing fallible follows commit.
        let mut result = GenerationResult {
            schedule,
            assignment_count: count,
            assigned_count: selection.assigned,
            open_count: selection.open,
            made_current: request.make_current,
            warnings: selection.warnings.details,
            warning_counts: selection.warnings.counts,
            total_warning_count: selection.warnings.total,
            details_truncated: selection.warnings.truncated,
            partial_minimum_periods: inputs.readiness.partial_minimum_periods,
            diagnostics,
        };
        tx.commit().await?;
        result.diagnostics.elapsed_milliseconds = started.elapsed().as_millis();
        Ok(result)
    }
    generate(pool, clock, request)
        .await
        .map_err(generation_inputs::generation_error)
}
