import { localValueError, type LocalValueResult } from "./localValueResult";
import { localTimeMinutes, parseLocalTime } from "./localTime";

export interface ShiftTimeCalculation {
  readonly elapsed_minutes: number;
  readonly unpaid_lunch_minutes: number;
  readonly paid_minutes: number;
  readonly overnight: boolean;
}

/** Shifts at least this long include an unpaid lunch. */
export const LUNCH_THRESHOLD_MINUTES = 480;
/** The unpaid lunch deducted from those shifts. */
export const UNPAID_LUNCH_MINUTES = 30;

/** Immediate usability preview. Rust remains authoritative for previews and writes. */
export function previewShiftTime(
  start: string,
  end: string,
): LocalValueResult<ShiftTimeCalculation> {
  const a = parseLocalTime(start);
  if (!a.ok) return a;
  const b = parseLocalTime(end);
  if (!b.ok) return b;
  const difference = localTimeMinutes(b.value) - localTimeMinutes(a.value);
  if (difference === 0)
    return localValueError(
      "invalid_interval",
      "Start and end times must be different.",
    );
  const overnight = difference < 0;
  const elapsed_minutes = difference + (overnight ? 1440 : 0);
  const unpaid_lunch_minutes =
    elapsed_minutes >= LUNCH_THRESHOLD_MINUTES ? UNPAID_LUNCH_MINUTES : 0;
  return {
    ok: true,
    value: {
      elapsed_minutes,
      unpaid_lunch_minutes,
      paid_minutes: elapsed_minutes - unpaid_lunch_minutes,
      overnight,
    },
  };
}
