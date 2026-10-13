import { useEffect, useState } from "react";
import { getSettings, readCommandResult, type Weekday } from "./ipc";
import type { Assignment } from "./schedules";

export const positionLabel = (a: Assignment) =>
  a.position_display_snapshot ?? a.position_name_snapshot;
export const employeeLabel = (a: Assignment) =>
  a.employee_display_name_snapshot ??
  ([a.employee_first_name_snapshot, a.employee_last_name_snapshot]
    .filter(Boolean)
    .join(" ") ||
    "Assigned employee");
/** HHMM–HHMM, the clock span printed on a shift magnet. */
export const shiftClock = (a: Assignment) =>
  `${a.shift_start_snapshot}–${a.shift_end_snapshot}`;
export const isOpen = (a: Assignment) =>
  a.open_shift || !a.assigned_employee_id;

/**
 * The saved workweek start, used to align board weeks. Until it loads (or if
 * it cannot be read) the board starts weeks on the schedule's first date.
 */
export function useWorkweekStart(): Weekday | null {
  const [value, setValue] = useState<Weekday | null>(null);
  useEffect(() => {
    let canceled = false;
    void getSettings()
      .then(readCommandResult)
      .then(
        (settings) => {
          const setting = settings.find(
            (item) => item.key === "workweek_start_weekday",
          )?.value;
          if (!canceled && setting?.kind === "weekday") setValue(setting.value);
        },
        () => {
          // Alignment is cosmetic; the board still shows every date.
        },
      );
    return () => {
      canceled = true;
    };
  }, []);
  return value;
}
