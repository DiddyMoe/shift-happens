import { formatHours } from "../lib/duration";
import {
  periodLabels,
  timeOfDayLabels,
  type EmploymentDefaults,
  type RestrictionData,
} from "../lib/employees";
import type { Weekday } from "../lib/ipc";

export const weekdayLabels: Record<Weekday, string> = {
  sunday: "Sunday",
  monday: "Monday",
  tuesday: "Tuesday",
  wednesday: "Wednesday",
  thursday: "Thursday",
  friday: "Friday",
  saturday: "Saturday",
};

/** Placeholder for every notes field: the app holds scheduling data only. */
export const notesPlaceholder =
  "Scheduling notes only — no patient or medical details.";

export const assignmentWord = (count: number | null) =>
  count === 1 ? "assignment" : "assignments";

/** A minimum in words for the reset preview, e.g. "40 h each workweek". */
export function minimumDetail(rule: EmploymentDefaults) {
  if (rule.minimum_period === "none") return "No minimum";
  const period = periodLabels[rule.minimum_period].toLowerCase();
  return rule.minimum_paid_minutes !== null
    ? `${formatHours(rule.minimum_paid_minutes)} ${period}`
    : `${rule.minimum_assignment_count} ${assignmentWord(rule.minimum_assignment_count)} ${period}`;
}

export const availabilityLabel = (required: boolean) =>
  required ? "Required" : "Not required";

/** True when a weekday-and-time restriction's end time falls on the next day. */
export const endsNextDay = (r: RestrictionData) =>
  Boolean(r.start_time && r.end_time && r.end_time < r.start_time);

/** A restriction in words, e.g. "No night shifts" or "Monday 0900–1700". */
export function restrictionDescription(r: RestrictionData) {
  if (r.restriction_kind === "time_of_day_block")
    return `No ${(r.time_of_day ? timeOfDayLabels[r.time_of_day] : "").toLowerCase()} shifts`;
  return `${r.weekday ? weekdayLabels[r.weekday] : ""} ${r.start_time}–${r.end_time}${endsNextDay(r) ? " (ends next day)" : ""}`;
}
