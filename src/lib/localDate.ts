import {
  localValueError,
  type LocalValueResult,
  type Ordering,
} from "./localValueResult.ts";

export type Weekday = 0 | 1 | 2 | 3 | 4 | 5 | 6;

export function isLeapYear(year: number): boolean {
  return year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
}

function daysInMonth(year: number, month: number): number {
  if (month === 2) return isLeapYear(year) ? 29 : 28;
  return month === 4 || month === 6 || month === 9 || month === 11 ? 30 : 31;
}

/** A validated civil date in the inclusive range 0001-01-01..9999-12-31. */
export class LocalDate {
  readonly year: number;
  readonly month: number;
  readonly day: number;

  private constructor(year: number, month: number, day: number) {
    this.year = year;
    this.month = month;
    this.day = day;
    Object.freeze(this);
  }

  static fromParts(
    year: number,
    month: number,
    day: number,
  ): LocalValueResult<LocalDate> {
    if (
      !Number.isInteger(year) ||
      year < 1 ||
      year > 9999 ||
      !Number.isInteger(month) ||
      month < 1 ||
      month > 12 ||
      !Number.isInteger(day) ||
      day < 1 ||
      day > daysInMonth(year, month)
    ) {
      return localValueError(
        "invalid_date",
        "Use a valid date from 0001-01-01 through 9999-12-31.",
      );
    }
    return { ok: true, value: new LocalDate(year, month, day) };
  }
}

export function parseLocalDate(value: string): LocalValueResult<LocalDate> {
  if (value.length !== 10 || !/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(value)) {
    return localValueError(
      "invalid_date",
      "Use a local date in YYYY-MM-DD format.",
    );
  }
  return LocalDate.fromParts(
    Number(value.slice(0, 4)),
    Number(value.slice(5, 7)),
    Number(value.slice(8, 10)),
  );
}

export function formatLocalDate(date: LocalDate): string {
  return `${String(date.year).padStart(4, "0")}-${String(date.month).padStart(2, "0")}-${String(date.day).padStart(2, "0")}`;
}

export function compareLocalDates(left: LocalDate, right: LocalDate): Ordering {
  const a = localDateOrdinal(left);
  const b = localDateOrdinal(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

function daysBeforeYear(year: number): number {
  const previous = year - 1;
  return (
    365 * previous +
    Math.floor(previous / 4) -
    Math.floor(previous / 100) +
    Math.floor(previous / 400)
  );
}

/** Zero-based Gregorian day index, with 0001-01-01 equal to zero. */
export function localDateOrdinal(date: LocalDate): number {
  let ordinal = daysBeforeYear(date.year) + date.day - 1;
  for (let month = 1; month < date.month; month += 1)
    ordinal += daysInMonth(date.year, month);
  return ordinal;
}

export function addLocalDays(
  date: LocalDate,
  days: number,
): LocalValueResult<LocalDate> {
  if (!Number.isSafeInteger(days)) {
    return localValueError(
      "invalid_day_offset",
      "The day offset must be a safe integer.",
    );
  }
  const ordinal = localDateOrdinal(date);
  const lastOrdinal = daysBeforeYear(10000) - 1;
  // Check the offset before adding so even a very large safe integer cannot overflow.
  if (days < -ordinal || days > lastOrdinal - ordinal) {
    return localValueError(
      "out_of_range",
      "Date arithmetic exceeds 0001-01-01 through 9999-12-31.",
    );
  }
  const target = ordinal + days;
  let firstYear = 1;
  let lastYear = 9999;
  while (firstYear < lastYear) {
    const midpoint = Math.ceil((firstYear + lastYear) / 2);
    if (daysBeforeYear(midpoint) <= target) firstYear = midpoint;
    else lastYear = midpoint - 1;
  }
  let dayOfYear = target - daysBeforeYear(firstYear);
  let month = 1;
  while (dayOfYear >= daysInMonth(firstYear, month)) {
    dayOfYear -= daysInMonth(firstYear, month);
    month += 1;
  }
  return LocalDate.fromParts(firstYear, month, dayOfYear + 1);
}

/** Sunday = 0, Monday = 1, ... Saturday = 6. */
export function localWeekday(date: LocalDate): Weekday {
  // 0001-01-01 was a Monday in the proleptic Gregorian calendar.
  switch ((localDateOrdinal(date) + 1) % 7) {
    case 0:
      return 0;
    case 1:
      return 1;
    case 2:
      return 2;
    case 3:
      return 3;
    case 4:
      return 4;
    case 5:
      return 5;
    default:
      return 6;
  }
}

export function fromNativeDateInput(
  value: string,
): LocalValueResult<LocalDate> {
  return parseLocalDate(value);
}

export function toNativeDateInput(date: LocalDate): string {
  return formatLocalDate(date);
}

/** Display a wire-format civil value without timezone conversion. */
export function displayLocalDate(value: string): string {
  const parsed = parseLocalDate(value);
  return parsed.ok ? formatLocalDate(parsed.value) : "Invalid local date";
}

/** Month arithmetic stays civil; only the current clock uses native local getters. */
export function calendarMonthRange(month: string): LocalValueResult<{
  start_date: string;
  end_date: string;
}> {
  if (!/^[0-9]{4}-[0-9]{2}$/.test(month)) {
    return localValueError("invalid_date", "Choose a month in YYYY-MM format.");
  }
  const year = Number(month.slice(0, 4));
  const monthNumber = Number(month.slice(5, 7));
  const first = LocalDate.fromParts(year, monthNumber, 1);
  if (!first.ok) return first;
  const last = LocalDate.fromParts(
    year,
    monthNumber,
    daysInMonth(year, monthNumber),
  );
  if (!last.ok) return last;
  return {
    ok: true,
    value: {
      start_date: formatLocalDate(first.value),
      end_date: formatLocalDate(last.value),
    },
  };
}

export function currentLocalCalendarMonth(clock: Date = new Date()): string {
  return `${String(clock.getFullYear()).padStart(4, "0")}-${String(clock.getMonth() + 1).padStart(2, "0")}`;
}
