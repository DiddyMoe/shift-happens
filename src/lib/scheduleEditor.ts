import { desktopInvoke } from "./ipc";
import type { Assignment, Schedule } from "./schedules";
export type EditorAction =
  | { readonly kind: "assign"; readonly employee_id: string }
  | { readonly kind: "open" }
  | { readonly kind: "note"; readonly notes: string | null }
  | { readonly kind: "lock" }
  | { readonly kind: "unlock" };
export type Direction = "undo" | "redo";
export interface Finding {
  readonly code: string;
  readonly warning_id: string;
  readonly context: Readonly<Record<string, unknown>>;
  readonly source_ids: readonly string[];
  readonly omitted_source_id_count: number;
}
export interface Projection {
  readonly employee_id: string;
  readonly metric: "paid_minutes" | "assignment_count";
  readonly period_start: string;
  readonly period_end_exclusive: string;
  readonly complete: boolean;
  readonly before: number;
  readonly after: number;
  readonly required: number | null;
  readonly shortfall_before: number;
  readonly shortfall_after: number;
}
export interface Analysis {
  readonly status: "eligible" | "blocked" | "requires_confirmation";
  readonly blockers: readonly Finding[];
  readonly warnings: readonly Finding[];
  readonly projected_paid_minutes: number;
  readonly projected_assignment_count: number;
  readonly weekly_paid_minutes: number;
  readonly weekly_overtime_minutes: number;
  readonly workweek_start: string | null;
  readonly workweek_end_exclusive: string | null;
  readonly workweek_complete: boolean;
  readonly source_paid_minutes: number | null;
  readonly source_assignment_count: number | null;
  readonly rest_checks: readonly {
    readonly prior_assignment_id: string;
    readonly next_assignment_id: string;
    readonly gap_minutes: number;
    readonly required_minutes: number;
    readonly deficit_minutes: number;
    readonly recovery: boolean;
    readonly minimum_rest_enabled: boolean;
    readonly violates: boolean;
  }[];
  readonly minimum: Projection | null;
  readonly source_minimum: Projection | null;
}
export interface Candidate extends Analysis {
  readonly employee_id: string;
  readonly display_name: string;
}
export interface Candidates {
  readonly schedule_id: string;
  readonly assignment_id: string;
  readonly assignment_revision: number;
  readonly schedule_revision: number;
  readonly setting_revisions: Readonly<Record<string, number>>;
  readonly candidates: readonly Candidate[];
  readonly open: Analysis;
}
export interface Head {
  readonly entry_id: string;
  readonly assignment_id: string;
  readonly direction: Direction;
  readonly expected_assignment_revision: number;
  readonly expected_schedule_revision: number;
  readonly action: EditorAction["kind"];
}
export interface HistoryState {
  readonly undo: Head | null;
  readonly redo: Head | null;
  readonly undo_count: number;
  readonly redo_count: number;
  readonly unavailable: boolean;
}
export interface ActionRequest {
  readonly schedule_id: string;
  readonly assignment_id: string;
  readonly expected_assignment_revision: number;
  readonly expected_schedule_revision: number;
  readonly action: EditorAction;
  readonly acknowledged_warning_ids: readonly string[] | null;
}
export interface ReplayRequest extends Omit<Head, "action"> {
  readonly schedule_id: string;
  readonly acknowledged_warning_ids: readonly string[] | null;
}
export interface EditorOutcome {
  readonly schedule: Schedule;
  readonly assignment: Omit<Assignment, "operational_day_label">;
  readonly history: HistoryState;
  readonly analysis: Analysis;
  readonly action: EditorAction | null;
  readonly replay: {
    readonly entry_id: string;
    readonly direction: Direction;
    readonly target_employee_id: string | null;
    readonly target_open: boolean;
    readonly expected_assignment_revision: number;
    readonly expected_schedule_revision: number;
  } | null;
}
export const getCandidates = (scheduleId: string, assignmentId: string) =>
  desktopInvoke<Candidates>("get_assignment_candidates", {
    request: { schedule_id: scheduleId, assignment_id: assignmentId },
  });
export const getHistory = (scheduleId: string) =>
  desktopInvoke<HistoryState>("get_editor_history", {
    request: { schedule_id: scheduleId },
  });
export const editAssignment = (request: ActionRequest) =>
  desktopInvoke<EditorOutcome>("edit_schedule_assignment", { request });
export const replayAssignment = (request: ReplayRequest) =>
  desktopInvoke<EditorOutcome>("replay_schedule_assignment", { request });
