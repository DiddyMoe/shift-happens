import type { HolidayGroup, HolidaySet } from "../lib/holidays";

export const holidaySets: readonly { value: HolidaySet; label: string }[] = [
  { value: "a", label: "Set A" },
  { value: "b", label: "Set B" },
  { value: "none", label: "No set" },
];

export const groupLabel = (group: HolidayGroup) =>
  group === "none" ? "none" : group.toUpperCase();
