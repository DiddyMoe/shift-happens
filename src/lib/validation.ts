import { desktopInvoke } from "./ipc";
import type { Schedule } from "./schedules";
export type ValidationCode =
  | "invalid_schedule_state"
  | "corrupt_assignment_state"
  | "duplicate_schedule_slot"
  | "incomplete_schedule_slots"
  | "assignment_outside_schedule_range"
  | "invalid_assignment_interval"
  | "assignment_paid_time_mismatch"
  | "lunch_policy_violation"
  | "assignment_overlap_conflict"
  | "missing_reference"
  | "hard_rule_data_incomplete"
  | "approved_pto_conflict"
  | "availability_hard_conflict"
  | "explicit_availability_missing"
  | "employee_restriction_conflict"
  | "shift_template_restriction_conflict"
  | "position_restriction_conflict"
  | "night_shift_ineligible"
  | "ortho_or_ineligible"
  | "inactive_or_archived_employee_assigned"
  | "weekend_ineligible"
  | "holiday_ineligible"
  | "maximum_paid_minutes_exceeded"
  | "validation_input_limit_exceeded"
  | "validation_output_limit_exceeded"
  | "open_shift"
  | "requested_pto_overlap"
  | "preferred_time_mismatch"
  | "preferred_availability_not_met"
  | "weekend_rotation_mismatch"
  | "holiday_group_mismatch"
  | "prn_weekday_availability_missing"
  | "overtime_week_exceeded"
  | "full_time_week_minimum_not_met"
  | "part_time_two_week_minimum_not_met"
  | "custom_employee_minimum_not_met"
  | "consecutive_workdays_exceeded"
  | "minimum_rest_violation"
  | "recovery_rule_violation"
  | "two_week_anchor_missing"
  | "incomplete_data";
export type Severity = "error" | "warning";
export interface Finding {
  readonly id: string;
  readonly code: ValidationCode;
  readonly severity: Severity;
  readonly schedule_id: string;
  readonly employee_id: string | null;
  readonly assignment_id: string | null;
  readonly counterpart_assignment_id: string | null;
  readonly operational_date: string | null;
  readonly actual_datetime: string | null;
  readonly message: string;
  readonly context: Readonly<Record<string, unknown>>;
}
export interface ValidationResult {
  readonly schedule_id: string;
  readonly findings: readonly Finding[];
  readonly error_count: number;
  readonly warning_count: number;
  readonly truncated: boolean;
  readonly complete: boolean;
  readonly codes: readonly {
    readonly code: ValidationCode;
    readonly severity: Severity;
    readonly message: string;
  }[];
}
export const validateSchedule = (id: string) =>
  desktopInvoke<ValidationResult>("validate_schedule", { request: { id } });

export interface FinalizationOutcome {
  readonly schedule: Schedule;
  readonly validation: ValidationResult | null;
  readonly open_shift_count: number;
}
export const finalizeSchedule = (
  scheduleId: string,
  expectedRevision: number,
  acknowledgedWarningIds: readonly string[] | null,
) =>
  desktopInvoke<FinalizationOutcome>("finalize_schedule", {
    request: {
      schedule_id: scheduleId,
      expected_revision: expectedRevision,
      acknowledged_warning_ids: acknowledgedWarningIds,
    },
  });
