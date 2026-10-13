import {
  desktopInvoke,
  readCommandResult,
  type CommandResult,
  type Weekday,
} from "./ipc";
import type { ActiveStatus, EmployeeData, RecordFilter } from "./employees";
import {
  listCoverageItems,
  listCoverageTemplates,
  type CoverageTemplate,
} from "./positionsCoverage";

export type HolidayGroup = EmployeeData["holiday_group"];
export type HolidaySet = "a" | "b" | "none";
export type DateKind = "fixed" | "nth_weekday" | "last_weekday" | "exact_date";
export type CoverageBehavior = "normal" | "custom";
export interface HolidayData {
  name: string;
  holiday_set: HolidaySet;
  date_kind: DateKind;
  month: number | null;
  day_of_month: number | null;
  weekday: Weekday | null;
  nth: number | null;
  exact_date: string | null;
  coverage_behavior: CoverageBehavior;
  custom_coverage_template_id: string | null;
  active_status: ActiveStatus;
  notes: string | null;
}
export interface Holiday extends HolidayData {
  readonly id: string;
  readonly seed_key: string | null;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export interface HolidayListEntry extends Holiday {
  readonly resolved_date: string | null;
}
export interface DatePreview {
  readonly year: number;
  readonly resolved_date: string | null;
}
export interface RotationPreview {
  readonly year: number;
  readonly start_year: number;
  readonly set_a_group: HolidayGroup;
  readonly set_b_group: HolidayGroup;
}
export type HolidayWindow =
  | { kind: "date"; date: string }
  | { kind: "year"; year: number }
  | { kind: "range"; start_date: string; end_date: string };
export interface CollisionPreview {
  readonly start_date: string;
  readonly end_date: string;
  readonly collision_count: number;
  readonly collisions: readonly {
    readonly date: string;
    readonly holiday_count: number;
    readonly holidays: readonly {
      readonly id: string;
      readonly name: string;
    }[];
    readonly omitted_source_id_count: number;
  }[];
  readonly omitted_diagnostic_count: number;
}
export const holidayData = (row: Holiday): HolidayData => ({
  name: row.name,
  holiday_set: row.holiday_set,
  date_kind: row.date_kind,
  month: row.month,
  day_of_month: row.day_of_month,
  weekday: row.weekday,
  nth: row.nth,
  exact_date: row.exact_date,
  coverage_behavior: row.coverage_behavior,
  custom_coverage_template_id: row.custom_coverage_template_id,
  active_status: row.active_status,
  notes: row.notes,
});
/** Every holiday, with its date in `year`; screens filter the rows. */
export const listHolidays = (year: number) =>
  desktopInvoke<readonly HolidayListEntry[]>("list_holidays", {
    request: {
      year,
      filter: "all" satisfies RecordFilter,
      holiday_set: null,
      coverage_behavior: null,
    },
  });
export const getHoliday = (id: string) =>
  desktopInvoke<Holiday>("get_holiday", { request: { id } });
export const createHoliday = (holiday: HolidayData) =>
  desktopInvoke<Holiday>("create_holiday", { request: { holiday } });
export const updateHoliday = (row: Holiday, holiday: HolidayData) =>
  desktopInvoke<Holiday>("update_holiday", {
    request: { id: row.id, expected_revision: row.revision, holiday },
  });
export const archiveHoliday = (row: Holiday) =>
  desktopInvoke<Holiday>("archive_holiday", {
    request: { id: row.id, expected_revision: row.revision },
  });
export const previewHolidayDate = (holiday: HolidayData, year: number) =>
  desktopInvoke<DatePreview>("preview_holiday_date", {
    request: { holiday, year },
  });
export const previewHolidayRotation = (year: number) =>
  desktopInvoke<RotationPreview>("preview_holiday_rotation", {
    request: { year },
  });
export const readHolidayCollisions = (request: HolidayWindow) =>
  desktopInvoke<CollisionPreview>("read_holiday_collisions", { request });

/**
 * The custom coverage a holiday can choose: active holiday-custom templates
 * that have at least one active item. No read returns item counts, so each
 * template's items are listed; the backend rechecks the choice on save.
 */
export async function listHolidayCoverageTemplates(): Promise<
  CommandResult<readonly CoverageTemplate[]>
> {
  const templates = readCommandResult(
    await listCoverageTemplates("active", "holiday_custom"),
  );
  const populated = await Promise.all(
    templates.map(
      async (t) =>
        readCommandResult(await listCoverageItems(t.id, "active")).length > 0,
    ),
  );
  return {
    status: "success",
    data: templates.filter((_, i) => populated[i]),
  };
}
