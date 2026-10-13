// Durations are stored and calculated as whole minutes, so totals stay exact.
// People read and enter them in hours; these helpers are the only conversion.

const MINUTES_PER_HOUR = 60;
const hourFormat = new Intl.NumberFormat(undefined, {
  maximumFractionDigits: 2,
});

/** Hours as a plain number for text: 2400 → "40", 450 → "7.5", 505 → "8.42". */
export function hoursText(minutes: number): string {
  return hourFormat.format(minutes / MINUTES_PER_HOUR);
}

/** A duration in hours: 2400 → "40 h", 450 → "7.5 h", 30 → "0.5 h". */
export function formatHours(minutes: number): string {
  return `${hoursText(minutes)} h`;
}

/** The text an hours input starts with: null → "", 450 → "7.5". */
export function minutesToHoursInput(minutes: number | null): string {
  return minutes === null
    ? ""
    : String(Number((minutes / MINUTES_PER_HOUR).toFixed(2)));
}

/**
 * Whole minutes from an hours input, or null when the text isn't a
 * non-negative duration. Accepts decimal hours ("7.5") and hours:minutes
 * ("7:30"). Two decimal places round-trip every whole minute exactly.
 */
export function parseHoursInput(text: string): number | null {
  const value = text.trim();
  const clock = /^(\d{1,4}):([0-5]\d)$/.exec(value);
  if (clock) return Number(clock[1]) * MINUTES_PER_HOUR + Number(clock[2]);
  if (!/^(\d{1,4}(\.\d*)?|\.\d+)$/.test(value)) return null;
  return Math.round(Number(value) * MINUTES_PER_HOUR);
}
