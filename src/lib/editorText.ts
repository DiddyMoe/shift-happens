// Plain-language text for assignment-editor findings (candidate blockers and
// warnings). Validation and finalization findings carry their own messages.
export const editorFindingLabels: Readonly<Record<string, string>> = {
  approved_pto_conflict: "Approved PTO overlaps this assignment",
  availability_hard_conflict: "Unavailable or hard-block availability overlaps",
  explicit_availability_missing: "Available time must cover the full shift",
  employee_assignment_overlap: "Overlaps another assignment in this schedule",
  employee_restriction: "Employee restriction prevents this assignment",
  shift_template_restriction: "Shift template is restricted",
  position_restriction: "Position is restricted",
  night_capability_missing: "Not set to be scheduled for night shifts",
  ortho_or_capability_missing: "Not set to be scheduled for ORTHO & OR",
  weekend_capability_missing: "Not set to be scheduled for weekends",
  holiday_capability_missing: "Not set to be scheduled for holidays",
  maximum_paid_minutes_exceeded: "Maximum paid hours would be exceeded",
  candidate_eligibility_data_incomplete: "Required eligibility data is missing",
  employee_ineligible: "Employee is missing, inactive or archived",
  assignment_locked: "Assignment is locked",
  requested_pto_overlap: "Requested PTO overlaps this assignment",
  preferred_availability_not_met: "Preferred availability is not met",
  preferred_time_mismatch: "Shift differs from preferred time of day",
  minimum_rest_violation: "Minimum rest between assignments is not met",
  recovery_rule_violation: "Required recovery after a shift is not met",
  max_consecutive_workdays: "Consecutive workday limit would be exceeded",
  projected_overtime: "Weekly overtime threshold would be exceeded",
  weekend_off_rotation: "Weekend is outside the employee’s rotation",
  holiday_group_mismatch:
    "Employee’s holiday group differs from the required group",
  prn_weekday_availability_missing:
    "PRN weekday availability does not cover the full shift",
  source_employee_minimum_shortfall_worsened:
    "Removing this assignment worsens the previous employee’s minimum shortfall",
  incomplete_data: "Some current rule data is incomplete",
};
