import { desktopInvoke, type Weekday } from "./ipc";

export type EmploymentType = "full_time" | "part_time" | "prn";
export type ActiveStatus = "active" | "inactive";
export type TimeOfDay = "day" | "evening" | "night";
export type RulePeriod = "week" | "two_week" | "none";
export type RecordFilter =
  "current" | "active" | "inactive" | "archived" | "all";
export type HolidayGroup = "a" | "b" | "none";
export type WeekendPattern = "none" | "every_other_weekend";
export type RestrictionKind = "time_of_day_block" | "recurring_day_time_block";
export interface EmployeeData {
  first_name: string;
  last_name: string;
  display_name: string;
  employment_type: EmploymentType;
  active_status: ActiveStatus;
  holiday_group: HolidayGroup;
  can_work_weekends: boolean;
  can_work_holidays: boolean;
  can_work_night_shift: boolean;
  can_work_ortho_or: boolean;
  preferred_time_of_day: TimeOfDay | null;
  requires_explicit_availability: boolean;
  minimum_paid_minutes: number | null;
  minimum_assignment_count: number | null;
  minimum_period: RulePeriod;
  maximum_paid_minutes: number | null;
  maximum_period: RulePeriod;
  weekend_rotation_pattern: WeekendPattern;
  weekend_rotation_anchor_date: string | null;
  notes: string | null;
}
export interface Employee extends EmployeeData {
  readonly id: string;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export type EmploymentDefaults = Pick<
  EmployeeData,
  | "minimum_paid_minutes"
  | "minimum_assignment_count"
  | "minimum_period"
  | "requires_explicit_availability"
>;
export interface ResetPreview {
  readonly id: string;
  readonly revision: number;
  readonly employment_type: EmploymentType;
  readonly before: EmploymentDefaults;
  readonly after: EmploymentDefaults;
}
export interface RestrictionData {
  restriction_kind: RestrictionKind;
  time_of_day: TimeOfDay | null;
  weekday: Weekday | null;
  start_time: string | null;
  end_time: string | null;
  active_status: ActiveStatus;
  notes: string | null;
}
export interface EmployeeRestriction extends RestrictionData {
  readonly id: string;
  readonly employee_id: string;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export const employmentLabels: Record<EmploymentType, string> = {
  full_time: "Full-time",
  part_time: "Part-time",
  prn: "PRN",
};
export const activeStatusLabels: Record<ActiveStatus, string> = {
  active: "Active",
  inactive: "Inactive",
};
export const timeOfDayLabels: Record<TimeOfDay, string> = {
  day: "Day",
  evening: "Evening",
  night: "Night",
};
/** The periods a minimum or maximum can count over. */
export const periodLabels: Record<Exclude<RulePeriod, "none">, string> = {
  week: "Each workweek",
  two_week: "Each two-week pay period",
};
export const holidayGroupLabels: Record<HolidayGroup, string> = {
  none: "None",
  a: "Group A",
  b: "Group B",
};
export const weekendPatternLabels: Record<WeekendPattern, string> = {
  none: "No rotation",
  every_other_weekend: "Every other weekend",
};
export const restrictionKindLabels: Record<RestrictionKind, string> = {
  time_of_day_block: "Shift type",
  recurring_day_time_block: "Weekday and time",
};

/** A label map's keys in their declared order, for rendering select options. */
export function labelKeys<T extends string>(labels: Record<T, string>): T[] {
  return Object.keys(labels) as T[];
}
/** The key a select reported, or undefined when it isn't one of `labels`. */
export function labelKey<T extends string>(
  labels: Record<T, string>,
  value: string,
): T | undefined {
  return labelKeys(labels).find((key) => key === value);
}
export function employeeData(employee: Employee): EmployeeData {
  // Explicit typed replacement: metadata never becomes editable request input.
  return {
    first_name: employee.first_name,
    last_name: employee.last_name,
    display_name: employee.display_name,
    employment_type: employee.employment_type,
    active_status: employee.active_status,
    holiday_group: employee.holiday_group,
    can_work_weekends: employee.can_work_weekends,
    can_work_holidays: employee.can_work_holidays,
    can_work_night_shift: employee.can_work_night_shift,
    can_work_ortho_or: employee.can_work_ortho_or,
    preferred_time_of_day: employee.preferred_time_of_day,
    requires_explicit_availability: employee.requires_explicit_availability,
    minimum_paid_minutes: employee.minimum_paid_minutes,
    minimum_assignment_count: employee.minimum_assignment_count,
    minimum_period: employee.minimum_period,
    maximum_paid_minutes: employee.maximum_paid_minutes,
    maximum_period: employee.maximum_period,
    weekend_rotation_pattern: employee.weekend_rotation_pattern,
    weekend_rotation_anchor_date: employee.weekend_rotation_anchor_date,
    notes: employee.notes,
  };
}
export function restrictionData(r: EmployeeRestriction): RestrictionData {
  return {
    restriction_kind: r.restriction_kind,
    time_of_day: r.time_of_day,
    weekday: r.weekday,
    start_time: r.start_time,
    end_time: r.end_time,
    active_status: r.active_status,
    notes: r.notes,
  };
}
export const getEmploymentDefaults = (employmentType: EmploymentType) =>
  desktopInvoke<EmploymentDefaults>("get_employment_defaults", {
    employmentType,
  });
export const listEmployees = (
  filter: RecordFilter,
  employmentType: EmploymentType | null,
  search: string,
) =>
  desktopInvoke<readonly Employee[]>("list_employees", {
    request: { filter, employment_type: employmentType, search },
  });
export const getEmployee = (id: string) =>
  desktopInvoke<Employee>("get_employee", { request: { id } });
export function createEmployee(d: EmployeeData) {
  return desktopInvoke<Employee>("create_employee", {
    request: {
      first_name: d.first_name,
      last_name: d.last_name,
      display_name: d.display_name,
      employment_type: d.employment_type,
      options: {
        active_status: d.active_status,
        holiday_group: d.holiday_group,
        can_work_weekends: d.can_work_weekends,
        can_work_holidays: d.can_work_holidays,
        can_work_night_shift: d.can_work_night_shift,
        can_work_ortho_or: d.can_work_ortho_or,
        preferred_time_of_day: d.preferred_time_of_day,
        requires_explicit_availability: d.requires_explicit_availability,
        minimum: {
          minimum_paid_minutes: d.minimum_paid_minutes,
          minimum_assignment_count: d.minimum_assignment_count,
          minimum_period: d.minimum_period,
        },
        maximum: {
          maximum_paid_minutes: d.maximum_paid_minutes,
          maximum_period: d.maximum_period,
        },
        weekend: {
          weekend_rotation_pattern: d.weekend_rotation_pattern,
          weekend_rotation_anchor_date: d.weekend_rotation_anchor_date,
        },
        notes: d.notes,
      },
    },
  });
}
export const updateEmployee = (e: Employee, employee: EmployeeData) =>
  desktopInvoke<Employee>("update_employee", {
    request: { id: e.id, expected_revision: e.revision, employee },
  });
export const archiveEmployee = (e: Employee) =>
  desktopInvoke<Employee>("archive_employee", {
    request: { id: e.id, expected_revision: e.revision },
  });
export const previewEmploymentDefaults = (e: Employee) =>
  desktopInvoke<ResetPreview>("preview_employment_defaults", {
    request: { id: e.id, expected_revision: e.revision },
  });
export const resetEmploymentDefaults = (preview: ResetPreview) =>
  desktopInvoke<Employee>("reset_employment_defaults", {
    request: { id: preview.id, expected_revision: preview.revision },
  });
export const listRestrictions = (employeeId: string, filter: RecordFilter) =>
  desktopInvoke<readonly EmployeeRestriction[]>("list_employee_restrictions", {
    request: { employee_id: employeeId, filter },
  });
export const createRestriction = (
  employeeId: string,
  restriction: RestrictionData,
) =>
  desktopInvoke<EmployeeRestriction>("create_employee_restriction", {
    request: { employee_id: employeeId, restriction },
  });
export const updateRestriction = (
  r: EmployeeRestriction,
  restriction: RestrictionData,
) =>
  desktopInvoke<EmployeeRestriction>("update_employee_restriction", {
    request: { id: r.id, expected_revision: r.revision, restriction },
  });
export const archiveRestriction = (r: EmployeeRestriction) =>
  desktopInvoke<EmployeeRestriction>("archive_employee_restriction", {
    request: { id: r.id, expected_revision: r.revision },
  });
