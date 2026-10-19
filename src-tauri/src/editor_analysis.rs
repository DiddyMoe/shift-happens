//! One bounded replacement view for candidate reads, edits and assignment replay.
use crate::{
    availability_pto::{AvailabilityEntry, PtoEntry},
    availability_rules::{AvailabilityPtoIndex, RuleCode},
    employee_restrictions::{EmployeeRestriction, RestrictionIndex},
    employees::{ActiveStatus, Employee, RulePeriod},
    error::{AppError, AppResult, DIAGNOSTIC_LIMIT, ErrorCode},
    generation::{self, ExclusionReason, Workload},
    generation_inputs::{
        self, PlannedSlot, Policy, RuleInputs, add, arithmetic_error, json_ids, load_counted,
    },
    generation_snapshot::ASSIGNMENT_LIMIT,
    holidays::{self, HolidaySet},
    local_values::{
        ANALYSIS_EMPLOYEE_LIMIT, ExpandedIntervalBudget, LocalDate, LocalDateTime, LocalInterval,
        SourceRowBudget, input_limit_error,
    },
    position_restrictions::PositionRestriction,
    rule_facts::MaterialFacts,
    schedule_assignments::{Assignment, ValidatedCoverageSlot},
    schedules::Schedule,
    settings::{self, SettingView},
    template_restrictions::TemplateRestriction,
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{FromRow, QueryBuilder, Sqlite, Transaction};
use std::collections::BTreeMap;

pub(crate) fn editor_error(mut e: AppError) -> AppError {
    if matches!(e.code, ErrorCode::InputLimitExceeded) {
        e.code = ErrorCode::CandidateInputLimitExceeded;
    }
    e
}
pub(crate) fn output_limit(observed: usize) -> AppError {
    let mut e = input_limit_error("diagnostics", DIAGNOSTIC_LIMIT, observed);
    e.code = ErrorCode::EditorOutputLimitExceeded;
    e
}
#[derive(Clone, Serialize)]
pub struct Finding {
    pub code: String,
    pub warning_id: String,
    pub context: Value,
    pub source_ids: Vec<String>,
    pub omitted_source_id_count: usize,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Eligible,
    Blocked,
    RequiresConfirmation,
}
#[derive(Clone, Serialize)]
pub struct Projection {
    pub employee_id: String,
    pub metric: &'static str,
    pub period_start: String,
    pub period_end_exclusive: String,
    pub complete: bool,
    pub before: i64,
    pub after: i64,
    pub required: Option<i64>,
    pub shortfall_before: i64,
    pub shortfall_after: i64,
}
#[derive(Clone, Serialize)]
pub struct Analysis {
    pub status: CandidateStatus,
    pub blockers: Vec<Finding>,
    pub warnings: Vec<Finding>,
    pub projected_paid_minutes: i64,
    pub projected_assignment_count: i64,
    pub weekly_paid_minutes: i64,
    pub weekly_overtime_minutes: i64,
    pub workweek_start: Option<String>,
    pub workweek_end_exclusive: Option<String>,
    pub workweek_complete: bool,
    pub source_paid_minutes: Option<i64>,
    pub source_assignment_count: Option<i64>,
    pub rest_checks: Vec<RestCheck>,
    pub minimum: Option<Projection>,
    pub source_minimum: Option<Projection>,
}
#[derive(Clone, Serialize)]
pub struct RestCheck {
    pub prior_assignment_id: String,
    pub next_assignment_id: String,
    pub gap_minutes: i64,
    pub required_minutes: i64,
    pub deficit_minutes: i64,
    pub recovery: bool,
    pub minimum_rest_enabled: bool,
    pub violates: bool,
}
impl Default for Analysis {
    fn default() -> Self {
        Self {
            status: CandidateStatus::Eligible,
            blockers: Vec::new(),
            warnings: Vec::new(),
            projected_paid_minutes: 0,
            projected_assignment_count: 0,
            weekly_paid_minutes: 0,
            weekly_overtime_minutes: 0,
            workweek_start: None,
            workweek_end_exclusive: None,
            workweek_complete: false,
            source_paid_minutes: None,
            source_assignment_count: None,
            rest_checks: Vec::new(),
            minimum: None,
            source_minimum: None,
        }
    }
}
// Serialized code strings are shared with generation when the rule is shared.
#[derive(Clone, Copy)]
enum WarningCode {
    RequestedPto,
    PreferredAvailability,
    PreferredTime,
    MinimumRest,
    Recovery,
    Consecutive,
    Overtime,
    SourceMinimum,
    Incomplete,
}
impl WarningCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::RequestedPto => RuleCode::RequestedPtoOverlap.as_str(),
            Self::PreferredAvailability => RuleCode::PreferredAvailabilityNotMet.as_str(),
            Self::PreferredTime => {
                crate::validation::ValidationCode::PreferredTimeMismatch.as_str()
            }
            Self::MinimumRest => crate::validation::ValidationCode::MinimumRestViolation.as_str(),
            Self::Recovery => crate::validation::ValidationCode::RecoveryRuleViolation.as_str(),
            Self::Consecutive => "max_consecutive_workdays",
            Self::Incomplete => crate::validation::ValidationCode::IncompleteData.as_str(),
            Self::Overtime => "projected_overtime",
            Self::SourceMinimum => "source_employee_minimum_shortfall_worsened",
        }
    }
}
// Owned physical sources. Candidate reads release their transaction before build/evaluate.
pub(crate) struct Sources {
    pub rows: Vec<Assignment>,
    employees: Vec<Employee>,
    settings: Vec<SettingView>,
    restrictions: Vec<EmployeeRestriction>,
    availability: Vec<AvailabilityEntry>,
    pto: Vec<PtoEntry>,
    template_blocks: Vec<TemplateRestriction>,
    position_blocks: Vec<PositionRestriction>,
}
pub(crate) async fn load(
    tx: &mut Transaction<'_, Sqlite>,
    schedule: &Schedule,
) -> AppResult<Sources> {
    let start = LocalDate::parse(&schedule.start_date)?;
    let end = LocalDate::parse(&schedule.end_date)?;
    let days = end.days_since(start) + 1;
    if days > crate::local_values::SCHEDULE_DAY_LIMIT as i64 {
        return Err(input_limit_error(
            "schedule_days",
            crate::local_values::SCHEDULE_DAY_LIMIT,
            days as usize,
        ));
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM schedule_assignments WHERE schedule_id=?")
            .bind(&schedule.id)
            .fetch_one(&mut **tx)
            .await?;
    if count > ASSIGNMENT_LIMIT as i64 {
        return Err(input_limit_error(
            "assignments",
            ASSIGNMENT_LIMIT,
            count as usize,
        ));
    }
    // Check links in one bounded query; later source archive/edit is deliberately allowed.
    let missing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM schedule_assignments a WHERE a.schedule_id=? AND (NOT EXISTS(SELECT 1 FROM coverage_templates WHERE id=a.coverage_template_id) OR NOT EXISTS(SELECT 1 FROM coverage_template_items WHERE id=a.coverage_item_id) OR NOT EXISTS(SELECT 1 FROM shift_positions WHERE id=a.shift_position_id) OR NOT EXISTS(SELECT 1 FROM shift_templates WHERE id=a.shift_template_id) OR (a.holiday_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM holidays WHERE id=a.holiday_id)) OR (a.assigned_employee_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM employees WHERE id=a.assigned_employee_id))))").bind(&schedule.id).fetch_one(&mut **tx).await?;
    if missing {
        return Err(AppError::new(
            ErrorCode::InvalidAssignment,
            "An assignment's retained source is missing.",
        ));
    }
    let rows = sqlx::query_as::<_, Assignment>("SELECT * FROM schedule_assignments WHERE schedule_id=? ORDER BY schedule_date,coverage_sort_order_snapshot,coverage_slot_index_snapshot,actual_start_datetime,id").bind(&schedule.id).fetch_all(&mut **tx).await?;
    let employee_filter = "(active_status='active' AND archived_at IS NULL) OR id IN (SELECT assigned_employee_id FROM schedule_assignments WHERE schedule_id=?1)";
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM employees WHERE (active_status='active' AND archived_at IS NULL) OR id IN (SELECT assigned_employee_id FROM schedule_assignments WHERE schedule_id=?1)").bind(&schedule.id).fetch_one(&mut **tx).await?;
    if count > ANALYSIS_EMPLOYEE_LIMIT as i64 {
        return Err(input_limit_error(
            "employee_rows",
            ANALYSIS_EMPLOYEE_LIMIT,
            count as usize,
        ));
    }
    let mut query = QueryBuilder::<Sqlite>::new("SELECT ");
    query
        .push(generation_inputs::EMPLOYEE_COLUMNS)
        .push(",")
        .push(generation_inputs::numeric_valid("employees"))
        .push(" AS numeric_valid FROM employees WHERE ")
        .push(employee_filter)
        .push(" ORDER BY id");
    let employee_rows = query
        .build()
        .bind(&schedule.id)
        .fetch_all(&mut **tx)
        .await?;
    let employees = employee_rows
        .iter()
        .map(Employee::from_row)
        .collect::<Result<Vec<_>, _>>()?;
    // Corrupt active rule data cannot produce an eligibility claim. Inactive sources
    // can be safely removed and do not contribute source-minimum warnings.
    for (e, row) in employees.iter().zip(&employee_rows) {
        if active(e) {
            generation_inputs::verify_employee_row(e, row)?;
        }
    }
    let settings = settings::read_in_transaction(tx).await?;
    let first = rows
        .iter()
        .map(|r| &r.structure.actual_start_datetime)
        .min()
        .ok_or_else(arithmetic_error)?;
    let last = rows
        .iter()
        .map(|r| &r.structure.actual_end_datetime)
        .max()
        .ok_or_else(arithmetic_error)?;
    let first = LocalDateTime::parse(first)?
        .date()
        .subtract_days(1)?
        .format();
    let last = LocalDateTime::parse(last)?.date().format();
    let ids = json_ids(
        employees
            .iter()
            .filter(|e| active(e))
            .map(|e| e.id.as_str()),
    )?;
    let shift_ids = json_ids(rows.iter().map(|r| r.structure.shift_template_id.as_str()))?;
    let position_ids = json_ids(rows.iter().map(|r| r.structure.shift_position_id.as_str()))?;
    let mut budget = SourceRowBudget::default();
    let mut reads = 0;
    let mut weekdays = std::collections::BTreeSet::new();
    let mut date = LocalDate::parse(&first)?;
    let end_date = LocalDate::parse(&last)?;
    loop {
        weekdays.insert(date.weekday().as_str());
        if date == end_date {
            break;
        }
        date = date.add_days(1)?;
    }
    let restriction_days = json_ids(weekdays)?;
    let restrictions = load_counted(
        tx,
        &mut budget,
        &mut reads,
        (
            "employee_restrictions",
            generation_inputs::RESTRICTION_COLUMNS,
            generation_inputs::RESTRICTION_FILTER,
        ),
        (&first, &last, &restriction_days),
    )
    .await?;
    let availability = load_counted(
        tx,
        &mut budget,
        &mut reads,
        (
            "availability_entries",
            generation_inputs::AVAILABILITY_COLUMNS,
            generation_inputs::AVAILABILITY_FILTER,
        ),
        (&first, &last, &ids),
    )
    .await?;
    let pto = load_counted(
        tx,
        &mut budget,
        &mut reads,
        (
            "pto_entries",
            generation_inputs::PTO_COLUMNS,
            generation_inputs::PTO_FILTER,
        ),
        (&first, &last, &ids),
    )
    .await?;
    let template_blocks = load_counted(tx, &mut budget, &mut reads, ("employee_shift_template_restrictions", "id,employee_id,shift_template_id,restriction_type,active_status,NULL AS notes,created_at,updated_at,archived_at,revision", generation_inputs::TEMPLATE_BLOCK_FILTER), (&first, &last, &shift_ids)).await?;
    let position_blocks = load_counted(tx, &mut budget, &mut reads, ("employee_position_restrictions", "id,employee_id,shift_position_id,restriction_type,active_status,NULL AS notes,created_at,updated_at,archived_at,revision", generation_inputs::POSITION_BLOCK_FILTER), (&first, &last, &position_ids)).await?;
    Ok(Sources {
        rows,
        employees,
        settings,
        restrictions,
        availability,
        pto,
        template_blocks,
        position_blocks,
    })
}
pub(crate) fn active(e: &Employee) -> bool {
    e.archived_at.is_none() && e.data.active_status == ActiveStatus::Active
}
pub(crate) struct Replacement {
    pub rules: RuleInputs,
    pub rows: Vec<Assignment>,
    pub settings_revisions: BTreeMap<String, i64>,
    workloads: BTreeMap<String, Workload>,
    index: usize,
    start: LocalDate,
    end: LocalDate,
}
impl Sources {
    pub fn build(self, schedule: &Schedule, assignment_id: &str) -> AppResult<Replacement> {
        let policy = Policy::from_settings(&self.settings)?;
        let start = LocalDate::parse(&schedule.start_date)?;
        let end = LocalDate::parse(&schedule.end_date)?;
        let index = self
            .rows
            .iter()
            .position(|r| r.id == assignment_id)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::NotFound,
                    "Select an assignment in this schedule.",
                )
            })?;
        let mut slots = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let date = LocalDate::parse(&row.structure.schedule_date)?;
            let interval = LocalInterval::new(
                LocalDateTime::parse(&row.structure.actual_start_datetime)?,
                LocalDateTime::parse(&row.structure.actual_end_datetime)?,
            )?;
            let holiday_group = match row.structure.holiday_set_snapshot {
                None | Some(HolidaySet::None) => None,
                // Both years are already validated (LocalDate, settings), so this
                // cannot fail; it only reuses the canonical parity mapping.
                Some(set) => holidays::rotation_for_year(date.year(), policy.holiday_year)?
                    .required_group(set),
            };
            slots.push(PlannedSlot {
                slot: ValidatedCoverageSlot::from_assignment(row, schedule)?,
                date,
                interval,
                week: policy.period(date, RulePeriod::Week)?,
                two_week: policy
                    .anchor
                    .map(|_| policy.period(date, RulePeriod::TwoWeek))
                    .transpose()?,
                holiday_group,
            });
        }
        let windows: Vec<_> = slots.iter().map(|s| s.interval).collect();
        let mut expanded = ExpandedIntervalBudget::default();
        let restrictions = RestrictionIndex::build(&self.restrictions, &windows, &mut expanded)?;
        let availability =
            AvailabilityPtoIndex::build(&self.availability, &self.pto, &windows, &mut expanded)?;
        let mut template_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for r in self.template_blocks {
            crate::template_restrictions::verify_restriction(&r)?;
            template_blocks
                .entry(r.employee_id)
                .or_default()
                .entry(r.data.shift_template_id)
                .or_default()
                .push(r.id);
        }
        let mut position_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for r in self.position_blocks {
            crate::position_restrictions::verify_restriction(&r)?;
            position_blocks
                .entry(r.employee_id)
                .or_default()
                .entry(r.data.shift_position_id)
                .or_default()
                .push(r.id);
        }
        let mut workloads = BTreeMap::<String, Workload>::new();
        for (i, row) in self.rows.iter().enumerate() {
            if i != index
                && let Some(id) = &row.editable.assigned_employee_id
            {
                workloads
                    .entry(id.clone())
                    .or_default()
                    .select(&slots[i], i)?;
            }
        }
        Ok(Replacement {
            rules: RuleInputs {
                employees: self.employees,
                slots,
                policy,
                restrictions,
                availability,
                template_blocks,
                position_blocks,
            },
            rows: self.rows,
            settings_revisions: self
                .settings
                .into_iter()
                .filter(|s| crate::generation_snapshot::SNAPSHOT_SETTINGS.contains(&s.key))
                .map(|s| (s.key.to_string(), s.revision))
                .collect(),
            workloads,
            index,
            start,
            end,
        })
    }
}
impl Replacement {
    fn finding(
        &self,
        operation: &str,
        code: &str,
        target: Option<&str>,
        context: Value,
        rule: Option<RuleCode>,
    ) -> AppResult<Finding> {
        let row = &self.rows[self.index];
        let slot = &self.rules.slots[self.index];
        let mut facts = MaterialFacts::new(operation, code);
        facts.text(&row.schedule_id);
        facts.text(&row.id);
        facts.text(target.unwrap_or("open"));
        facts.interval(slot.interval);
        facts.text(&serde_json::to_string(&context).map_err(|_| arithmetic_error())?);
        let (source_ids, omitted_source_id_count) = if let (Some(rule), Some(id)) = (rule, target) {
            self.rules
                .availability
                .material_sources(id, slot.interval, rule, &mut facts)?
        } else {
            (Vec::new(), 0)
        };
        let mut context = context;
        if let (Some(rule), Some(id)) = (rule, target)
            && let Some(object) = context.as_object_mut()
        {
            object.insert(
                "intervals".to_string(),
                self.rules
                    .availability
                    .display_material(id, slot.interval, rule)?,
            );
        }
        Ok(Finding {
            code: code.to_string(),
            warning_id: facts.finish(),
            context,
            source_ids,
            omitted_source_id_count,
        })
    }
    fn blocker(
        &self,
        operation: &str,
        target: &str,
        employee: &Employee,
        reason: ExclusionReason,
    ) -> AppResult<Finding> {
        let row = &self.rows[self.index];
        let slot = &self.rules.slots[self.index];
        let code = serde_json::to_value(reason)
            .map_err(|_| arithmetic_error())?
            .as_str()
            .ok_or_else(arithmetic_error)?
            .to_string();
        let mut context = json!({"employee_id":target,"assignment_id":row.id,"actual_start":slot.interval.start().format(),"actual_end":slot.interval.end().format()});
        let mut sources = Vec::new();
        let mut omitted = 0;
        let mut facts = MaterialFacts::new(operation, &code);
        facts.text(&row.schedule_id);
        facts.text(&row.id);
        facts.text(target);
        facts.interval(slot.interval);
        match reason {
            ExclusionReason::CandidateEligibilityDataIncomplete => {
                context = json!({"missing_fact": if self.rules.policy.anchor.is_none() && employee.data.maximum_period == RulePeriod::TwoWeek {"two_week_anchor"} else {"positive_availability"}, "hard_safety_unevaluable":true});
            }
            ExclusionReason::MaximumPaidMinutesExceeded => {
                let empty = Workload::default();
                let state = self.workloads.get(target).unwrap_or(&empty);
                let period = self
                    .rules
                    .policy
                    .period(slot.date, employee.data.maximum_period)?;
                let before = state.total(slot, employee.data.maximum_period)?.paid;
                context = json!({"period_start":period.start.format(),"period_end_exclusive":period.end_exclusive.format(),"maximum_paid_minutes":employee.data.maximum_paid_minutes,"before_paid_minutes":before,"after_paid_minutes":add(before,row.structure.shift_paid_minutes_snapshot)?});
            }
            ExclusionReason::EmployeeAssignmentOverlap => {
                for (i, other) in self.rows.iter().enumerate() {
                    if i != self.index
                        && other.editable.assigned_employee_id.as_deref() == Some(target)
                        && self.rules.slots[i].interval.overlaps(slot.interval)
                    {
                        facts.text(&other.id);
                        facts.interval(self.rules.slots[i].interval);
                        if sources.len() < crate::error::DIAGNOSTIC_SOURCE_ID_LIMIT {
                            sources.push(other.id.clone());
                        } else {
                            omitted += 1;
                        }
                    }
                }
            }
            ExclusionReason::EmployeeRestriction => {
                let ids = self.rules.restrictions.evaluate(
                    target,
                    row.structure.shift_category_snapshot,
                    slot.interval,
                )?;
                for id in ids {
                    if sources.len() < crate::error::DIAGNOSTIC_SOURCE_ID_LIMIT {
                        sources.push(id.to_string());
                    } else {
                        omitted += 1;
                    }
                }
                self.rules.restrictions.append_hard_material(
                    target,
                    row.structure.shift_category_snapshot,
                    slot.interval,
                    &mut facts,
                )?;
            }
            ExclusionReason::ShiftTemplateRestriction | ExclusionReason::PositionRestriction => {
                let (blocks, key) = if reason == ExclusionReason::PositionRestriction {
                    (
                        &self.rules.position_blocks,
                        &row.structure.shift_position_id,
                    )
                } else {
                    (
                        &self.rules.template_blocks,
                        &row.structure.shift_template_id,
                    )
                };
                if let Some(ids) = blocks.get(target).and_then(|b| b.get(key)) {
                    for id in ids {
                        facts.text(id);
                        facts.text(key);
                        if sources.len() < crate::error::DIAGNOSTIC_SOURCE_ID_LIMIT {
                            sources.push(id.clone());
                        } else {
                            omitted += 1;
                        }
                    }
                }
            }
            ExclusionReason::ApprovedPtoConflict
            | ExclusionReason::AvailabilityHardConflict
            | ExclusionReason::ExplicitAvailabilityMissing => {
                let rule = match reason {
                    ExclusionReason::ApprovedPtoConflict => RuleCode::ApprovedPtoConflict,
                    ExclusionReason::AvailabilityHardConflict => RuleCode::AvailabilityHardConflict,
                    _ => RuleCode::ExplicitAvailabilityMissing,
                };
                context = self
                    .rules
                    .availability
                    .display_material(target, slot.interval, rule)?;
                self.rules.availability.append_hard_material(
                    target,
                    slot.interval,
                    employee.data.requires_explicit_availability,
                    &mut facts,
                )?;
            }
            _ => {}
        }
        facts.text(&serde_json::to_string(&context).map_err(|_| arithmetic_error())?);
        Ok(Finding {
            code,
            warning_id: facts.finish(),
            context,
            source_ids: sources,
            omitted_source_id_count: omitted,
        })
    }
    fn minimum(&self, employee: &Employee, removing: bool) -> AppResult<Option<Projection>> {
        let e = &employee.data;
        if !active(employee) || e.minimum_period == RulePeriod::None {
            return Ok(None);
        }
        if e.minimum_period == RulePeriod::TwoWeek && self.rules.policy.anchor.is_none() {
            return Ok(None);
        }
        let slot = &self.rules.slots[self.index];
        let period = self.rules.policy.period(slot.date, e.minimum_period)?;
        let empty = Workload::default();
        let state = self.workloads.get(&employee.id).unwrap_or(&empty);
        let removed = state.total(slot, e.minimum_period)?;
        let included = removed.projected(slot.slot.structure().shift_paid_minutes_snapshot)?;
        let (metric, before, after, required) = if let Some(required) = e.minimum_paid_minutes {
            (
                "paid_minutes",
                if removing {
                    included.paid
                } else {
                    removed.paid
                },
                if removing {
                    removed.paid
                } else {
                    included.paid
                },
                required,
            )
        } else {
            (
                "assignment_count",
                if removing {
                    included.assignments
                } else {
                    removed.assignments
                },
                if removing {
                    removed.assignments
                } else {
                    included.assignments
                },
                e.minimum_assignment_count.ok_or_else(arithmetic_error)?,
            )
        };
        Ok(Some(Projection {
            employee_id: employee.id.clone(),
            metric,
            period_start: period.start.format(),
            period_end_exclusive: period.end_exclusive.format(),
            complete: period.start >= self.start && period.end_exclusive.days_since(self.end) <= 1,
            before,
            after,
            required: Some(required),
            shortfall_before: required
                .checked_sub(before)
                .ok_or_else(arithmetic_error)?
                .max(0),
            shortfall_after: required
                .checked_sub(after)
                .ok_or_else(arithmetic_error)?
                .max(0),
        }))
    }
    pub fn evaluate(&self, target: Option<&str>, operation: &str) -> AppResult<Analysis> {
        let row = &self.rows[self.index];
        let slot = &self.rules.slots[self.index];
        let mut result = Analysis::default();
        let same = row.editable.assigned_employee_id.as_deref() == target;
        if row.editable.locked && !same {
            result.blockers.push(self.finding(
                operation,
                "assignment_locked",
                target,
                json!({"assignment_id": row.id}),
                None,
            )?);
        }
        if !same
            && let Some(source) = row
                .editable
                .assigned_employee_id
                .as_ref()
                .and_then(|id| self.rules.employees.iter().find(|e| &e.id == id))
        {
            let empty = Workload::default();
            let state = self.workloads.get(&source.id).unwrap_or(&empty);
            result.source_paid_minutes = Some(state.whole.paid);
            result.source_assignment_count = Some(state.whole.assignments);
            result.source_minimum = self.minimum(source, true)?;
            if let Some(p) = &result.source_minimum
                && p.complete
                && p.shortfall_after > p.shortfall_before
            {
                result.warnings.push(self.finding(
                    operation,
                    WarningCode::SourceMinimum.as_str(),
                    target,
                    serde_json::to_value(p).map_err(|_| arithmetic_error())?,
                    None,
                )?);
            }
        }
        if let Some(target) = target {
            let employee_index = self
                .rules
                .employees
                .iter()
                .position(|e| e.id == target && active(e));
            if let Some(employee_index) = employee_index {
                let employee = &self.rules.employees[employee_index];
                let empty = Workload::default();
                let state = self.workloads.get(target).unwrap_or(&empty);
                let whole = state
                    .whole
                    .projected(slot.slot.structure().shift_paid_minutes_snapshot)?;
                let week = state
                    .total(slot, RulePeriod::Week)?
                    .projected(slot.slot.structure().shift_paid_minutes_snapshot)?;
                result.projected_paid_minutes = whole.paid;
                result.projected_assignment_count = whole.assignments;
                result.weekly_paid_minutes = week.paid;
                result.workweek_start = Some(slot.week.start.format());
                result.workweek_end_exclusive = Some(slot.week.end_exclusive.format());
                result.workweek_complete = slot.week.start >= self.start
                    && slot.week.end_exclusive.days_since(self.end) <= 1;
                result.weekly_overtime_minutes = week
                    .paid
                    .checked_sub(self.rules.policy.overtime)
                    .ok_or_else(arithmetic_error)?
                    .max(0);
                result.minimum = self.minimum(employee, false)?;
                let (prior, next, _) = state.neighbors(slot.interval);
                for (a, b) in prior
                    .map(|a| (a, self.index))
                    .into_iter()
                    .chain(next.map(|b| (self.index, b)))
                {
                    if let Some(gap) =
                        generation::gap_fact(a, b, &self.rules.slots, &self.rules, true)?
                    {
                        result.rest_checks.push(RestCheck {
                            prior_assignment_id: self.rows[a].id.clone(),
                            next_assignment_id: self.rows[b].id.clone(),
                            gap_minutes: gap.gap,
                            required_minutes: self.rules.policy.rest,
                            deficit_minutes: gap.deficit,
                            recovery: gap.recovery,
                            minimum_rest_enabled: self.rules.policy.quick,
                            violates: gap.violates,
                        });
                    }
                }
                if same {
                    if let Some(minimum) = &mut result.minimum {
                        minimum.before = minimum.after;
                        minimum.shortfall_before = minimum.shortfall_after;
                    }
                    return Ok(result);
                }
                match generation::evaluate(
                    &self.rules,
                    state,
                    self.index,
                    employee_index,
                    self.start,
                    self.end,
                    true,
                ) {
                    Err(e) if matches!(e.code, ErrorCode::TwoWeekAnchorRequired) => {
                        result.blockers.push(self.finding(
                            operation,
                            "candidate_eligibility_data_incomplete",
                            Some(target),
                            json!({"missing_fact":"two_week_anchor"}),
                            None,
                        )?)
                    }
                    Err(e) => return Err(e),
                    Ok(Err(_)) => return Err(arithmetic_error()),
                    Ok(Ok(candidate)) => {
                        for reason in &candidate.exclusions {
                            result
                                .blockers
                                .push(self.blocker(operation, target, employee, *reason)?);
                        }

                        for reason in candidate.overrides {
                            let (code, context) = match reason {
                                ExclusionReason::WeekendOffRotation => (
                                    "weekend_off_rotation",
                                    json!({"date": slot.date.format(), "pattern": employee.data.weekend_rotation_pattern, "anchor": employee.data.weekend_rotation_anchor_date}),
                                ),
                                ExclusionReason::HolidayGroupMismatch => (
                                    "holiday_group_mismatch",
                                    json!({"date": slot.date.format(), "holiday_set": row.structure.holiday_set_snapshot, "required_group": slot.holiday_group, "employee_group": employee.data.holiday_group}),
                                ),
                                ExclusionReason::PrnWeekdayAvailabilityMissing => (
                                    "prn_weekday_availability_missing",
                                    json!({"date": slot.date.format(), "full_coverage": false}),
                                ),
                                _ => return Err(arithmetic_error()),
                            };
                            let mut finding =
                                self.finding(operation, code, Some(target), context, None)?;
                            if reason == ExclusionReason::PrnWeekdayAvailabilityMissing {
                                let mut facts = MaterialFacts::new(operation, code);
                                facts.text(&finding.warning_id);
                                let coverage = self.rules.availability.positive_material(
                                    target,
                                    slot.interval,
                                    &mut facts,
                                )?;
                                if let Some(context) = finding.context.as_object_mut() {
                                    context.insert("coverage".to_string(), coverage);
                                }
                                finding.warning_id = facts.finish();
                            }
                            result.warnings.push(finding);
                        }
                        for gap in candidate.gaps {
                            let code = if gap.recovery {
                                WarningCode::Recovery
                            } else {
                                WarningCode::MinimumRest
                            };
                            result.warnings.push(self.finding(operation, code.as_str(), Some(target), json!({"prior_id": self.rows[gap.prior].id,"next_id":self.rows[gap.next].id,"prior_start":self.rows[gap.prior].structure.actual_start_datetime,"prior_end":self.rows[gap.prior].structure.actual_end_datetime,"next_start":self.rows[gap.next].structure.actual_start_datetime,"next_end":self.rows[gap.next].structure.actual_end_datetime,"required_minutes":self.rules.policy.rest,"gap_minutes":gap.gap,"deficit_minutes":gap.deficit,"recovery":gap.recovery,"also_minimum_rest":gap.also_minimum_rest}), None)?);
                        }
                        for (applies, code, context, rule) in [
                            (
                                candidate.rank[12] > 0,
                                WarningCode::Incomplete,
                                json!({"missing_fact":"two_week_anchor","rule":"minimum"}),
                                None,
                            ),
                            (
                                candidate.rank[4] > 0,
                                WarningCode::Consecutive,
                                json!({"run_start":candidate.run.0.format(),"run_end":candidate.run.1.format(),"limit":self.rules.policy.consecutive,"excess":candidate.rank[4]}),
                                None,
                            ),
                            (
                                candidate.rank[5] > 0,
                                WarningCode::RequestedPto,
                                json!({"requested_pto_overlap":true}),
                                Some(RuleCode::RequestedPtoOverlap),
                            ),
                            (
                                candidate.rank[6] > 0,
                                WarningCode::Overtime,
                                json!({"period_start":slot.week.start.format(),"period_end_exclusive":slot.week.end_exclusive.format(),"before_paid_minutes":state.total(slot, RulePeriod::Week)?.paid,"after_paid_minutes":week.paid,"threshold_minutes":self.rules.policy.overtime,"excess_minutes":result.weekly_overtime_minutes}),
                                None,
                            ),
                            (
                                candidate.rank[13] == 2,
                                WarningCode::PreferredAvailability,
                                json!({"has_relevant_preferred_entries":true,"has_preferred_overlap":false}),
                                Some(RuleCode::PreferredAvailabilityNotMet),
                            ),
                            (
                                candidate.rank[14] == 2,
                                WarningCode::PreferredTime,
                                json!({"preferred_category":employee.data.preferred_time_of_day,"assignment_category":row.structure.shift_category_snapshot}),
                                None,
                            ),
                        ] {
                            if applies {
                                result.warnings.push(self.finding(
                                    operation,
                                    code.as_str(),
                                    Some(target),
                                    context,
                                    rule,
                                )?);
                            }
                        }
                    }
                }
            } else {
                result.blockers.push(self.finding(
                    operation,
                    "employee_ineligible",
                    Some(target),
                    json!({"employee_id":target}),
                    None,
                )?);
            }
        }
        let count = add(result.blockers.len() as i64, result.warnings.len() as i64)?;
        if count > DIAGNOSTIC_LIMIT as i64 {
            return Err(output_limit(count as usize));
        }
        result
            .blockers
            .sort_by(|a, b| (&a.code, &a.warning_id).cmp(&(&b.code, &b.warning_id)));
        result
            .warnings
            .sort_by(|a, b| (&a.code, &a.warning_id).cmp(&(&b.code, &b.warning_id)));
        result.status = if !result.blockers.is_empty() {
            CandidateStatus::Blocked
        } else if !result.warnings.is_empty() {
            CandidateStatus::RequiresConfirmation
        } else {
            CandidateStatus::Eligible
        };
        Ok(result)
    }
}
