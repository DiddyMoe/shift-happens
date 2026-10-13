import type { EntryData } from "../lib/availabilityPto";
import {
  addLocalDays,
  formatLocalDate,
  parseLocalDate,
} from "../lib/localDate";

/**
 * What an entry covers, in words, or what to fix before it can be saved.
 * A full day runs to midnight; a partial day repeats the same hours on every
 * date in the range, and an end time before the start time ends the next day.
 */
export function intervalPreview(data: EntryData): string {
  const first = parseLocalDate(data.start_date);
  const last = parseLocalDate(data.end_date);
  if (!first.ok || !last.ok || data.start_date > data.end_date)
    return "Choose a first and last date. The last date can't be before the first.";
  const oneDay = data.start_date === data.end_date;
  const dayAfterLast = addLocalDays(last.value, 1);
  if (data.is_full_day) {
    if (!dayAfterLast.ok) return "Choose a last date before 9999-12-31.";
    return oneDay
      ? `All day on ${data.start_date}.`
      : `All day from ${data.start_date} through ${data.end_date}.`;
  }
  const { start_time: start, end_time: end } = data;
  if (!start || !end || start === end)
    return "Choose a start and end time. They can't be the same.";
  const dates = oneDay
    ? `on ${data.start_date}`
    : `on each date from ${data.start_date} through ${data.end_date}`;
  if (end > start) return `${start}–${end} ${dates}.`;
  if (!dayAfterLast.ok) return "Choose a last date before 9999-12-31.";
  return oneDay
    ? `${start}–${end} overnight, starting ${data.start_date} and ending ${formatLocalDate(dayAfterLast.value)}.`
    : `${start}–${end} overnight, starting ${dates}. The last one ends ${formatLocalDate(dayAfterLast.value)}.`;
}
