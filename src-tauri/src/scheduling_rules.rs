//! Canonical assignment eligibility and rest facts for generation, editor and validation.
use crate::{
    availability_rules::{AvailabilityAssessment, AvailabilityPtoIndex},
    employee_restrictions::RestrictionIndex,
    employees::{
        self, Employee, EmploymentType, TimeOfDay, WeekendPattern, weekend_is_on_rotation,
    },
    error::AppResult,
    generation::ExclusionReason,
    generation_inputs::{Policy, arithmetic_error},
    local_values::{LocalDate, LocalInterval, Weekday, wall_clock_minutes},
};
use std::collections::BTreeMap;

pub(crate) struct SlotRules<'a> {
    pub date: LocalDate,
    pub interval: LocalInterval,
    pub category: TimeOfDay,
    pub shift_id: &'a str,
    pub position_id: &'a str,
    pub night: bool,
    pub ortho: bool,
    pub is_holiday: bool,
    pub holiday_group: Option<employees::HolidayGroup>,
}
pub(crate) struct RuleIndexes {
    pub restrictions: RestrictionIndex,
    pub availability: AvailabilityPtoIndex,
    pub template_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    pub position_blocks: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}
pub(crate) struct IndexRefs<'a> {
    pub restrictions: &'a RestrictionIndex,
    pub availability: &'a AvailabilityPtoIndex,
    pub template_blocks: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
    pub position_blocks: &'a BTreeMap<String, BTreeMap<String, Vec<String>>>,
}
impl RuleIndexes {
    pub fn refs(&self) -> IndexRefs<'_> {
        IndexRefs {
            restrictions: &self.restrictions,
            availability: &self.availability,
            template_blocks: &self.template_blocks,
            position_blocks: &self.position_blocks,
        }
    }
}
pub(crate) fn evaluate_slot(
    slot: SlotRules<'_>,
    employee: &Employee,
    indexes: IndexRefs<'_>,
) -> AppResult<(Vec<ExclusionReason>, AvailabilityAssessment)> {
    let e = &employee.data;
    let availability = indexes.availability.assess(&employee.id, slot.interval)?;
    let mut exclusions = Vec::new();
    if availability.approved_pto {
        exclusions.push(ExclusionReason::ApprovedPtoConflict);
    }
    if availability.hard_block {
        exclusions.push(ExclusionReason::AvailabilityHardConflict);
    }
    let requires_positive = e.requires_explicit_availability
        || (e.employment_type == EmploymentType::Prn
            && !slot.is_holiday
            && !matches!(slot.date.weekday(), Weekday::Saturday | Weekday::Sunday));
    if requires_positive && !availability.facts.has_full_available_coverage {
        exclusions.push(if availability.facts.positive_availability_incomplete {
            ExclusionReason::CandidateEligibilityDataIncomplete
        } else if e.requires_explicit_availability {
            ExclusionReason::ExplicitAvailabilityMissing
        } else {
            ExclusionReason::PrnWeekdayAvailabilityMissing
        });
    }
    if !indexes
        .restrictions
        .evaluate(&employee.id, slot.category, slot.interval)?
        .is_empty()
    {
        exclusions.push(ExclusionReason::EmployeeRestriction);
    }
    if indexes
        .template_blocks
        .get(&employee.id)
        .is_some_and(|blocks| blocks.contains_key(slot.shift_id))
    {
        exclusions.push(ExclusionReason::ShiftTemplateRestriction);
    }
    if indexes
        .position_blocks
        .get(&employee.id)
        .is_some_and(|blocks| blocks.contains_key(slot.position_id))
    {
        exclusions.push(ExclusionReason::PositionRestriction);
    }
    if slot.night && !e.can_work_night_shift {
        exclusions.push(ExclusionReason::NightCapabilityMissing);
    }
    if slot.ortho && !e.can_work_ortho_or {
        exclusions.push(ExclusionReason::OrthoOrCapabilityMissing);
    }
    if matches!(slot.date.weekday(), Weekday::Saturday | Weekday::Sunday) {
        if !e.can_work_weekends {
            exclusions.push(ExclusionReason::WeekendCapabilityMissing);
        }
        if e.weekend_rotation_pattern == WeekendPattern::EveryOtherWeekend {
            let anchor = LocalDate::parse(
                e.weekend_rotation_anchor_date
                    .as_deref()
                    .ok_or_else(arithmetic_error)?,
            )?;
            if weekend_is_on_rotation(slot.date, anchor)? == Some(false) {
                exclusions.push(ExclusionReason::WeekendOffRotation);
            }
        }
    }
    if slot.is_holiday && !e.can_work_holidays {
        exclusions.push(ExclusionReason::HolidayCapabilityMissing);
    }
    if slot
        .holiday_group
        .is_some_and(|group| group != e.holiday_group)
    {
        exclusions.push(ExclusionReason::HolidayGroupMismatch);
    }
    Ok((exclusions, availability))
}

pub(crate) fn rest_fact(
    prior: LocalInterval,
    next: LocalInterval,
    recovery: bool,
    policy: &Policy,
) -> AppResult<(i64, i64, bool, bool)> {
    let gap = wall_clock_minutes(prior.end(), next.start())?;
    let violates = gap >= 0 && gap < policy.rest && (recovery || policy.quick);
    Ok((
        gap,
        policy
            .rest
            .checked_sub(gap)
            .ok_or_else(arithmetic_error)?
            .max(0),
        violates,
        recovery && policy.quick,
    ))
}
