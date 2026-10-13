import { desktopInvoke } from "./ipc";
import type { DayType } from "./positionsCoverage";
import type { TimeOfDay } from "./employees";
import type { CoverageBehavior, HolidaySet } from "./holidays";

export type ScheduleStatus = "draft" | "final";
export type ScheduleFilter =
  "non_archived" | "draft" | "final" | "archived" | "current" | "all";
export interface Schedule {
  readonly id: string;
  readonly name: string;
  readonly start_date: string;
  readonly end_date: string;
  readonly status: ScheduleStatus;
  readonly notes: string | null;
  readonly generation_settings_snapshot: string;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
  /** Set by finalization and kept after reopening. */
  readonly finalized_once: boolean;
}
export type CurrentSchedule =
  | { readonly kind: "none"; readonly setting_revision: number }
  | {
      readonly kind: "current";
      readonly setting_revision: number;
      readonly schedule: Schedule;
    }
  | {
      readonly kind: "stale_setting";
      readonly setting_revision: number;
      readonly schedule_id: string;
    };
export interface Assignment {
  readonly id: string;
  readonly schedule_id: string;
  readonly schedule_date: string;
  readonly operational_day_label: string;
  readonly actual_start_datetime: string;
  readonly actual_end_datetime: string;
  readonly coverage_template_id: string;
  readonly coverage_template_seed_key_snapshot: string | null;
  readonly coverage_template_name_snapshot: string;
  readonly coverage_day_type_snapshot: DayType;
  readonly coverage_item_id: string;
  readonly coverage_item_seed_key_snapshot: string | null;
  readonly coverage_item_display_snapshot: string | null;
  readonly coverage_sort_order_snapshot: number;
  readonly coverage_required_count_snapshot: number;
  readonly coverage_slot_index_snapshot: number;
  readonly coverage_start_offset_snapshot: number;
  readonly coverage_end_offset_snapshot: number;
  readonly shift_position_id: string;
  readonly position_seed_key_snapshot: string | null;
  readonly position_name_snapshot: string;
  readonly position_display_snapshot: string | null;
  readonly shift_template_id: string;
  readonly shift_seed_key_snapshot: string | null;
  readonly shift_name_snapshot: string;
  readonly shift_display_snapshot: string | null;
  readonly shift_start_snapshot: string;
  readonly shift_end_snapshot: string;
  readonly shift_category_snapshot: TimeOfDay;
  readonly shift_color_snapshot: string | null;
  readonly shift_lunch_minutes_snapshot: number;
  readonly shift_paid_minutes_snapshot: number;
  readonly shift_recovery_snapshot: boolean;
  readonly shift_night_snapshot: boolean;
  readonly holiday_id: string | null;
  readonly holiday_seed_key_snapshot: string | null;
  readonly holiday_name_snapshot: string | null;
  readonly holiday_set_snapshot: HolidaySet | null;
  readonly holiday_coverage_behavior_snapshot: CoverageBehavior | null;
  readonly assigned_employee_id: string | null;
  readonly employee_first_name_snapshot: string | null;
  readonly employee_last_name_snapshot: string | null;
  readonly employee_display_name_snapshot: string | null;
  readonly locked: boolean;
  readonly open_shift: boolean;
  readonly manually_edited: boolean;
  readonly assignment_source: "generated" | "manual";
  readonly notes: string | null;
  readonly created_at: string;
  readonly updated_at: string;
  readonly revision: number;
}
export const listSchedules = (filter: ScheduleFilter = "non_archived") =>
  desktopInvoke<readonly Schedule[]>("list_schedules", { request: { filter } });
export const getSchedule = (id: string) =>
  desktopInvoke<Schedule>("get_schedule", { request: { id } });
export const listAssignments = (id: string) =>
  desktopInvoke<readonly Assignment[]>("list_schedule_assignments", {
    request: { id },
  });
export const getCurrentSchedule = () =>
  desktopInvoke<CurrentSchedule>("get_current_schedule");
export const setCurrentSchedule = (
  scheduleId: string,
  expectedSettingRevision: number,
) =>
  desktopInvoke<CurrentSchedule>("set_current_schedule", {
    request: {
      schedule_id: scheduleId,
      expected_setting_revision: expectedSettingRevision,
    },
  });
export const clearCurrentSchedule = (expectedSettingRevision: number) =>
  desktopInvoke<CurrentSchedule>("clear_current_schedule", {
    request: { expected_setting_revision: expectedSettingRevision },
  });
export const updateSchedule = (
  id: string,
  expectedRevision: number,
  name: string,
  notes: string | null,
) =>
  desktopInvoke<Schedule>("update_schedule", {
    request: { id, expected_revision: expectedRevision, name, notes },
  });
export const reopenSchedule = (id: string, expectedRevision: number) =>
  desktopInvoke<Schedule>("reopen_schedule", {
    request: { id, expected_revision: expectedRevision, confirmed: true },
  });
export const archiveSchedule = (id: string, expectedRevision: number) =>
  desktopInvoke<Schedule>("archive_schedule", {
    request: { id, expected_revision: expectedRevision },
  });

/** Only a draft that was never finalized can be deleted. Final schedules, and
 * drafts reopened after finalizing, are kept as records; archive those. */
export const canDeleteSchedule = (schedule: Schedule) =>
  schedule.status === "draft" && !schedule.finalized_once;
/** The preview (`requires_confirmation`, nothing changed) and the committed
 * deletion (`success`) share this shape. */
export interface ScheduleDeletion {
  readonly id: string;
  readonly name: string;
  readonly assignment_count: number;
  readonly is_current: boolean;
  /** True only after a committed deletion that cleared the current selection. */
  readonly cleared_current: boolean;
}
/** Call with `confirmed: false` first for the preview, then `true` to delete.
 * A blocked result carries "Schedules that were finalized are kept as records.
 * Archive it instead." */
export const deleteSchedule = (
  id: string,
  expectedRevision: number,
  confirmed: boolean,
) =>
  desktopInvoke<ScheduleDeletion>("delete_schedule", {
    request: { id, expected_revision: expectedRevision, confirmed },
  });

export interface ScheduleSummary {
  readonly schedule: Schedule;
  readonly open_shift_count: number;
}
export interface DashboardData {
  readonly current: CurrentSchedule;
  readonly shown: ScheduleSummary | null;
  readonly recent: readonly ScheduleSummary[];
}
export const getScheduleDashboard = () =>
  desktopInvoke<DashboardData>("get_schedule_dashboard");
