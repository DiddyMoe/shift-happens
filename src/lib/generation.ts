import {
  desktopInvoke,
  unreachableResult,
  type AppError,
  type CommandResult,
} from "./ipc";
import type { Schedule } from "./schedules";

/** A read or generation result as either its data or the error to show. */
export function commandOutcome<T>(
  result: CommandResult<T>,
): { data: T; error: null } | { data: null; error: AppError } {
  switch (result.status) {
    case "success":
    case "no_change":
      return { data: result.data, error: null };
    case "canceled":
    case "requires_overwrite":
    case "requires_confirmation":
      return {
        data: null,
        error: {
          code: "invalid_acknowledgement",
          message: "This action requires explicit confirmation.",
          context: {},
        },
      };
    case "validation_error":
    case "stale_revision":
    case "blocked":
    case "failure":
      return { data: null, error: result.error };
    default:
      return unreachableResult(result);
  }
}

export interface PartialMinimumPeriod {
  readonly period: "week" | "two_week";
  readonly start_date: string;
  readonly end_exclusive: string;
  readonly employee_count: number;
}
export interface GenerationReadiness {
  readonly active_employee_count: number;
  readonly planned_slot_count: number;
  readonly candidate_evaluation_count: number;
  readonly current_setting_revision: number;
  readonly two_week_anchor_required: boolean;
  readonly two_week_anchor_date: string | null;
  readonly partial_minimum_periods: readonly PartialMinimumPeriod[];
}
export type GenerationCode =
  | "candidate_eligibility_data_incomplete"
  | "generation_warning_output_limit_exceeded"
  | "max_consecutive_avoidance_failed"
  | "minimum_rest_avoidance_failed"
  | "open_shift_no_qualifying_candidate"
  | "overtime_avoidance_failed"
  | "preferred_availability_not_met"
  | "preferred_time_mismatch"
  | "recovery_avoidance_failed"
  | "requested_pto_avoidance_failed"
  | "soft_rule_data_incomplete";
export const warningLabels: Record<GenerationCode, string> = {
  candidate_eligibility_data_incomplete:
    "Some candidates could not be proven eligible",
  generation_warning_output_limit_exceeded: "Warning details were truncated",
  max_consecutive_avoidance_failed: "Consecutive workday limit exceeded",
  minimum_rest_avoidance_failed: "Minimum rest could not be maintained",
  open_shift_no_qualifying_candidate:
    "Open slot: no eligible candidate remained at this step",
  overtime_avoidance_failed: "Weekly overtime could not be avoided",
  preferred_availability_not_met: "Preferred availability could not be met",
  preferred_time_mismatch: "Time-of-day preference could not be met",
  recovery_avoidance_failed: "Recovery gap could not be maintained",
  requested_pto_avoidance_failed: "Requested time off could not be avoided",
  soft_rule_data_incomplete: "Some preference facts could not be evaluated",
};
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };
export interface GenerationWarning {
  readonly code: GenerationCode;
  readonly warning_id: string;
  readonly context: {
    readonly slot?: {
      readonly schedule_date: string;
      readonly coverage_item_id: string;
      readonly slot_index: number;
      readonly actual_start: string;
      readonly actual_end: string;
    };
    readonly employee_id?: string | null;
    readonly facts?: { readonly [key: string]: JsonValue };
  };
  readonly source_ids: readonly string[];
  readonly omitted_source_id_count: number;
}
export interface GenerationResult {
  readonly schedule: Schedule;
  readonly assignment_count: number;
  readonly assigned_count: number;
  readonly open_count: number;
  readonly made_current: boolean;
  readonly warnings: readonly GenerationWarning[];
  readonly warning_counts: Readonly<Partial<Record<GenerationCode, number>>>;
  readonly total_warning_count: number;
  readonly details_truncated: boolean;
  readonly partial_minimum_periods: readonly PartialMinimumPeriod[];
  readonly diagnostics: {
    readonly source_row_count: number;
    readonly preflight_sql_read_count: number;
    readonly expanded_interval_count: number;
    readonly candidate_evaluation_count: number;
    readonly elapsed_milliseconds: number;
  };
}
export interface GenerateScheduleRequest {
  readonly name: string;
  readonly start_date: string;
  readonly end_date: string;
  readonly make_current: boolean;
  readonly expected_current_setting_revision: number | null;
}
export const getGenerationReadiness = (start_date: string, end_date: string) =>
  desktopInvoke<GenerationReadiness>("get_generation_readiness", {
    request: { start_date, end_date },
  });
export const generateSchedule = (request: GenerateScheduleRequest) =>
  desktopInvoke<GenerationResult>("generate_schedule", { request });
