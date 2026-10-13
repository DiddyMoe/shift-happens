import { listEmployees } from "./employees";
import { commandOutcome, getGenerationReadiness } from "./generation";
import { readHolidayCollisions } from "./holidays";
import { getSettings, readCommandResult, type ErrorCode } from "./ipc";
import {
  addLocalDays,
  calendarMonthRange,
  currentLocalCalendarMonth,
  formatLocalDate,
  parseLocalDate,
} from "./localDate";
import { listCoverageTemplates } from "./positionsCoverage";
import { listShiftTemplates } from "./shiftTemplates";

export type SetupState = "ok" | "warn" | "blocking";
export interface SetupItem {
  readonly key:
    "settings" | "employees" | "shift_templates" | "coverage" | "holidays";
  readonly title: string;
  readonly path: string;
  readonly detail: string;
  readonly state: SetupState;
  readonly tag: string;
}

const weekdays = [
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
  "sunday",
] as const;

function localToday(): string {
  const now = new Date();
  return `${String(now.getFullYear()).padStart(4, "0")}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

/**
 * What a manager still needs before generating a useful draft. Readiness is
 * checked for the current calendar month, the default generation range;
 * holiday collisions are checked for the coming year.
 */
export async function loadSetupStatus(): Promise<readonly SetupItem[]> {
  const today = localToday();
  const parsedToday = parseLocalDate(today);
  const yearAhead = parsedToday.ok
    ? addLocalDays(parsedToday.value, 365)
    : null;
  const month = calendarMonthRange(currentLocalCalendarMonth());
  if (!month.ok || !yearAhead?.ok)
    throw new Error("Today's date could not be read.");
  const [employees, templates, coverage, readiness, collisions, settings] =
    await Promise.all([
      listEmployees("active", null, ""),
      listShiftTemplates("active"),
      listCoverageTemplates("active"),
      getGenerationReadiness(month.value.start_date, month.value.end_date),
      readHolidayCollisions({
        kind: "range",
        start_date: today,
        end_date: formatLocalDate(yearAhead.value),
      }),
      getSettings(),
    ]);
  const employeeCount = readCommandResult(employees).length;
  const templateCount = readCommandResult(templates).length;
  const defaults = new Set(
    readCommandResult(coverage)
      .filter((template) => template.is_default)
      .map((template) => template.day_type),
  );
  const defaultCount = weekdays.filter((day) => defaults.has(day)).length;
  const collisionCount = readCommandResult(collisions).collision_count;
  const anchor = readCommandResult(settings).find(
    (setting) => setting.key === "two_week_period_anchor_date",
  )?.value;
  const ready = commandOutcome(readiness);
  const blocker: ErrorCode | null = ready.error?.code ?? null;
  const anchorRequired =
    blocker === "two_week_anchor_required" ||
    (ready.data?.two_week_anchor_required ?? false);
  const coverageBlocked = blocker?.startsWith("coverage_") ?? false;
  return [
    {
      key: "settings",
      title: "Settings",
      path: "/settings",
      detail: anchor
        ? "Workweek and two-week payroll anchor are set."
        : anchorRequired
          ? "Two-week rules need a payroll anchor date."
          : "Choose the workweek and a two-week payroll anchor date.",
      state: anchor ? "ok" : anchorRequired ? "blocking" : "warn",
      tag: anchor
        ? "Ready"
        : anchorRequired
          ? "Blocks generation"
          : "Anchor not set",
    },
    {
      key: "employees",
      title: "Employees",
      path: "/employees",
      detail:
        employeeCount > 0
          ? `${employeeCount} active ${employeeCount === 1 ? "employee" : "employees"}.`
          : "Add employees; until then every slot is generated open.",
      state: employeeCount > 0 ? "ok" : "warn",
      tag: employeeCount > 0 ? "Ready" : "None active",
    },
    {
      key: "shift_templates",
      title: "Shift templates",
      path: "/shift-templates",
      detail:
        templateCount > 0
          ? `${templateCount} active ${templateCount === 1 ? "template" : "templates"}.`
          : "Add the shifts your coverage uses.",
      state: templateCount > 0 ? "ok" : "warn",
      tag: templateCount > 0 ? "Ready" : "None active",
    },
    {
      key: "coverage",
      title: "Coverage defaults",
      path: "/positions-coverage",
      detail: coverageBlocked
        ? (ready.error?.message ?? "Coverage needs attention.")
        : `${defaultCount} of 7 weekdays have a default coverage template.`,
      state: coverageBlocked ? "blocking" : defaultCount < 7 ? "warn" : "ok",
      tag: coverageBlocked ? "Blocks generation" : `${defaultCount}/7`,
    },
    {
      key: "holidays",
      title: "Holidays",
      path: "/holidays",
      detail:
        collisionCount === 0
          ? "No holiday collisions in the coming year."
          : `${collisionCount} ${collisionCount === 1 ? "date has" : "dates have"} colliding holidays in the coming year.`,
      state:
        blocker === "holiday_collision"
          ? "blocking"
          : collisionCount > 0
            ? "warn"
            : "ok",
      tag:
        blocker === "holiday_collision"
          ? "Blocks generation"
          : collisionCount === 0
            ? "No collisions"
            : `${collisionCount} ${collisionCount === 1 ? "collision" : "collisions"}`,
    },
  ];
}
