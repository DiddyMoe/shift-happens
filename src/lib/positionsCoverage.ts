import { desktopInvoke, type Weekday } from "./ipc";
import type { ActiveStatus, RecordFilter } from "./employees";
import type { ShiftTemplate } from "./shiftTemplates";
import {
  addLocalDays,
  formatLocalDate,
  parseLocalDate,
  localWeekday,
} from "./localDate";

export type DayType = Weekday | "holiday_custom";
export const dayTypes: readonly DayType[] = [
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
  "sunday",
  "holiday_custom",
];
export const dayLabels: Record<DayType, string> = {
  monday: "Monday",
  tuesday: "Tuesday",
  wednesday: "Wednesday",
  thursday: "Thursday",
  friday: "Friday",
  saturday: "Saturday",
  sunday: "Sunday",
  holiday_custom: "Holiday custom",
};
interface SourceRecord {
  readonly id: string;
  readonly seed_key: string | null;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export interface PositionData {
  name: string;
  display_label: string | null;
  sort_order: number;
  active_status: ActiveStatus;
  notes: string | null;
}
export interface Position extends PositionData, SourceRecord {}
export const positionData = (row: Position): PositionData => ({
  name: row.name,
  display_label: row.display_label,
  sort_order: row.sort_order,
  active_status: row.active_status,
  notes: row.notes,
});
export interface CoverageTemplateData {
  name: string;
  day_type: DayType;
  active_status: ActiveStatus;
  notes: string | null;
}
export interface CoverageTemplate extends CoverageTemplateData, SourceRecord {
  readonly is_default: boolean;
}
export const coverageTemplateData = (
  row: CoverageTemplate,
): CoverageTemplateData => ({
  name: row.name,
  day_type: row.day_type,
  active_status: row.active_status,
  notes: row.notes,
});
export interface CoverageItemData {
  coverage_template_id: string;
  shift_position_id: string;
  shift_template_id: string;
  display_label: string | null;
  sort_order: number;
  required_count: number;
  actual_start_day_offset: number;
  actual_end_day_offset: number;
  active_status: ActiveStatus;
  notes: string | null;
}
export interface CoverageItem extends CoverageItemData, SourceRecord {}
export const coverageItemData = (row: CoverageItem): CoverageItemData => ({
  coverage_template_id: row.coverage_template_id,
  shift_position_id: row.shift_position_id,
  shift_template_id: row.shift_template_id,
  display_label: row.display_label,
  sort_order: row.sort_order,
  required_count: row.required_count,
  actual_start_day_offset: row.actual_start_day_offset,
  actual_end_day_offset: row.actual_end_day_offset,
  active_status: row.active_status,
  notes: row.notes,
});
export interface CoverageIntervalPreview {
  readonly operational_date: string;
  readonly operational_weekday: Weekday;
  readonly actual_start: string;
  readonly actual_end: string;
  readonly actual_start_weekday: Weekday;
  readonly actual_end_weekday: Weekday;
  readonly elapsed_minutes: number;
}
export const listPositions = (filter: RecordFilter = "all") =>
  desktopInvoke<readonly Position[]>("list_shift_positions", {
    request: { filter },
  });
export const getPosition = (id: string) =>
  desktopInvoke<Position>("get_shift_position", { request: { id } });
export const createPosition = (position: PositionData) =>
  desktopInvoke<Position>("create_shift_position", { request: { position } });
export const updatePosition = (row: Position, position: PositionData) =>
  desktopInvoke<Position>("update_shift_position", {
    request: { id: row.id, expected_revision: row.revision, position },
  });
export const archivePosition = (row: Position) =>
  desktopInvoke<Position>("archive_shift_position", {
    request: { id: row.id, expected_revision: row.revision },
  });
export const listCoverageTemplates = (
  filter: RecordFilter = "all",
  day_type: DayType | null = null,
) =>
  desktopInvoke<readonly CoverageTemplate[]>("list_coverage_templates", {
    request: { filter, day_type },
  });
export const createCoverageTemplate = (template: CoverageTemplateData) =>
  desktopInvoke<CoverageTemplate>("create_coverage_template", {
    request: { template },
  });
export const updateCoverageTemplate = (
  row: CoverageTemplate,
  template: CoverageTemplateData,
) =>
  desktopInvoke<CoverageTemplate>("update_coverage_template", {
    request: { id: row.id, expected_revision: row.revision, template },
  });
export const archiveCoverageTemplate = (row: CoverageTemplate) =>
  desktopInvoke<CoverageTemplate>("archive_coverage_template", {
    request: { id: row.id, expected_revision: row.revision },
  });
export const setDefaultCoverageTemplate = (
  row: CoverageTemplate,
  previous: CoverageTemplate,
) =>
  desktopInvoke<CoverageTemplate>("set_default_coverage_template", {
    request: {
      id: row.id,
      expected_revision: row.revision,
      day_type: row.day_type,
      previous_default_id: previous.id,
      previous_default_revision: previous.revision,
    },
  });
export const listCoverageItems = (
  coverage_template_id: string,
  filter: RecordFilter,
) =>
  desktopInvoke<readonly CoverageItem[]>("list_coverage_items", {
    request: { coverage_template_id, filter },
  });
export const createCoverageItem = (item: CoverageItemData) =>
  desktopInvoke<CoverageItem>("create_coverage_item", { request: { item } });
export const updateCoverageItem = (row: CoverageItem, item: CoverageItemData) =>
  desktopInvoke<CoverageItem>("update_coverage_item", {
    request: { id: row.id, expected_revision: row.revision, item },
  });
export const archiveCoverageItem = (row: CoverageItem) =>
  desktopInvoke<CoverageItem>("archive_coverage_item", {
    request: { id: row.id, expected_revision: row.revision },
  });
export const previewCoverageInterval = (
  shift_template_id: string,
  operational_date: string,
  actual_start_day_offset: number,
  actual_end_day_offset: number,
) =>
  desktopInvoke<CoverageIntervalPreview>("preview_coverage_interval", {
    request: {
      shift_template_id,
      operational_date,
      actual_start_day_offset,
      actual_end_day_offset,
    },
  });
export function matchesFilter(
  row: { active_status: ActiveStatus; archived_at: string | null },
  filter: RecordFilter,
): boolean {
  switch (filter) {
    case "all":
      return true;
    case "archived":
      return row.archived_at !== null;
    case "current":
      return row.archived_at === null;
    case "active":
      return row.archived_at === null && row.active_status === "active";
    case "inactive":
      return row.archived_at === null && row.active_status === "inactive";
  }
}
export const isActive = (row: {
  active_status: ActiveStatus;
  archived_at: string | null;
}) => row.active_status === "active" && row.archived_at === null;
/** Marks an inactive or archived record in a choice list: " (inactive)". */
export const statusSuffix = (row: {
  active_status: ActiveStatus;
  archived_at: string | null;
}) =>
  row.archived_at
    ? " (archived)"
    : row.active_status === "inactive"
      ? " (inactive)"
      : "";
export function referenceDate(day: DayType): string {
  const date = parseLocalDate("2026-10-05");
  if (!date.ok) throw new Error(date.error.message);
  const shifted = addLocalDays(
    date.value,
    day === "holiday_custom" ? 0 : dayTypes.indexOf(day),
  );
  if (!shifted.ok) throw new Error(shifted.error.message);
  return formatLocalDate(shifted.value);
}
// Table display uses the stored offsets verbatim. The backend owns interval validation.
export function actualPreview(
  day: DayType,
  item: CoverageItemData,
  shift: ShiftTemplate | undefined,
): string {
  if (!shift) return "Shift unavailable";
  const date = parseLocalDate(referenceDate(day));
  if (!date.ok) return date.error.message;
  const start = addLocalDays(date.value, item.actual_start_day_offset),
    end = addLocalDays(date.value, item.actual_end_day_offset);
  if (!start.ok || !end.ok) return "Date out of range";
  const weekdayLabels = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
  ] as const;
  const startLabel = weekdayLabels[localWeekday(start.value)],
    endLabel = weekdayLabels[localWeekday(end.value)];
  return startLabel === endLabel
    ? `${startLabel} ${shift.start_time}–${shift.end_time}`
    : `${startLabel} ${shift.start_time} → ${endLabel} ${shift.end_time}`;
}
export interface PositionRestrictionData {
  shift_position_id: string;
  restriction_type: "cannot_work";
  active_status: ActiveStatus;
  notes: string | null;
}
export interface PositionRestriction extends PositionRestrictionData {
  readonly id: string;
  readonly employee_id: string;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export const listPositionRestrictions = (
  employee_id: string,
  filter: RecordFilter,
) =>
  desktopInvoke<readonly PositionRestriction[]>(
    "list_employee_position_restrictions",
    { request: { employee_id, filter } },
  );
export const createPositionRestriction = (
  employee_id: string,
  restriction: PositionRestrictionData,
) =>
  desktopInvoke<PositionRestriction>("create_employee_position_restriction", {
    request: { employee_id, restriction },
  });
export const updatePositionRestriction = (
  row: PositionRestriction,
  restriction: PositionRestrictionData,
) =>
  desktopInvoke<PositionRestriction>("update_employee_position_restriction", {
    request: { id: row.id, expected_revision: row.revision, restriction },
  });
export const archivePositionRestriction = (row: PositionRestriction) =>
  desktopInvoke<PositionRestriction>("archive_employee_position_restriction", {
    request: { id: row.id, expected_revision: row.revision },
  });
