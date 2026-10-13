import {
  addLocalDays,
  formatLocalDate,
  parseLocalDate,
  type LocalDate,
} from "./localDate.ts";
import {
  parseLocalTime,
  toNativeTimeInput,
  type LocalTime,
} from "./localTime.ts";
import {
  localValueError,
  type LocalValueResult,
  type Ordering,
} from "./localValueResult.ts";

/** A validated local datetime; it has no timezone or offset. */
export class LocalDateTime {
  readonly date: LocalDate;
  readonly time: LocalTime;
  readonly second: number;

  private constructor(date: LocalDate, time: LocalTime, second: number) {
    this.date = date;
    this.time = time;
    this.second = second;
    Object.freeze(this);
  }

  static fromParts(
    date: LocalDate,
    time: LocalTime,
    second = 0,
  ): LocalValueResult<LocalDateTime> {
    if (!Number.isInteger(second) || second < 0 || second > 59) {
      return localValueError(
        "invalid_datetime",
        "Seconds must be an integer from 00 through 59.",
      );
    }
    return { ok: true, value: new LocalDateTime(date, time, second) };
  }
}

export function parseLocalDateTime(
  value: string,
): LocalValueResult<LocalDateTime> {
  if (
    value.length !== 19 ||
    !/^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}$/.test(value)
  ) {
    return localValueError(
      "invalid_datetime",
      "Use YYYY-MM-DDTHH:MM:SS without a timezone.",
    );
  }
  const date = parseLocalDate(value.slice(0, 10));
  const time = parseLocalTime(value.slice(11, 13) + value.slice(14, 16));
  if (!date.ok || !time.ok) {
    return localValueError(
      "invalid_datetime",
      "Use a valid local date and time.",
    );
  }
  return LocalDateTime.fromParts(
    date.value,
    time.value,
    Number(value.slice(17, 19)),
  );
}

export function formatLocalDateTime(datetime: LocalDateTime): string {
  return `${formatLocalDate(datetime.date)}T${toNativeTimeInput(datetime.time)}:${String(datetime.second).padStart(2, "0")}`;
}

/** Display a wire-format civil value without timezone conversion. */
export function displayLocalDateTime(value: string): string {
  const parsed = parseLocalDateTime(value);
  return parsed.ok
    ? formatLocalDateTime(parsed.value)
    : "Invalid local datetime";
}

export function compareLocalDateTimes(
  left: LocalDateTime,
  right: LocalDateTime,
): Ordering {
  // Fixed-width validated local values compare chronologically without timezone conversion.
  const a = formatLocalDateTime(left);
  const b = formatLocalDateTime(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

export function addLocalDateTimeDays(
  datetime: LocalDateTime,
  days: number,
): LocalValueResult<LocalDateTime> {
  const date = addLocalDays(datetime.date, days);
  if (!date.ok) return date;
  return LocalDateTime.fromParts(date.value, datetime.time, datetime.second);
}

/** Native controls may omit seconds; omitted seconds are explicitly zero. */
export function fromNativeDateTimeInput(
  value: string,
): LocalValueResult<LocalDateTime> {
  return parseLocalDateTime(value.length === 16 ? value + ":00" : value);
}

/** Use step="1" on the control to preserve second precision. */
export function toNativeDateTimeInput(datetime: LocalDateTime): string {
  return formatLocalDateTime(datetime);
}
