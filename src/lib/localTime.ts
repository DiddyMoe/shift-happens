import {
  localValueError,
  type LocalValueResult,
  type Ordering,
} from "./localValueResult.ts";

/** A validated local clock time with minute precision. */
export class LocalTime {
  readonly hour: number;
  readonly minute: number;

  private constructor(hour: number, minute: number) {
    this.hour = hour;
    this.minute = minute;
    Object.freeze(this);
  }

  static fromParts(hour: number, minute: number): LocalValueResult<LocalTime> {
    if (
      !Number.isInteger(hour) ||
      hour < 0 ||
      hour > 23 ||
      !Number.isInteger(minute) ||
      minute < 0 ||
      minute > 59
    ) {
      return localValueError(
        "invalid_time",
        "Use a time from 0000 through 2359.",
      );
    }
    return { ok: true, value: new LocalTime(hour, minute) };
  }
}

export function parseLocalTime(value: string): LocalValueResult<LocalTime> {
  if (value.length !== 4 || !/^[0-9]{4}$/.test(value)) {
    return localValueError("invalid_time", "Use a local time in HHMM format.");
  }
  return LocalTime.fromParts(
    Number(value.slice(0, 2)),
    Number(value.slice(2, 4)),
  );
}

export function formatLocalTime(time: LocalTime): string {
  return `${String(time.hour).padStart(2, "0")}${String(time.minute).padStart(2, "0")}`;
}

export function localTimeMinutes(time: LocalTime): number {
  return time.hour * 60 + time.minute;
}

export function compareLocalTimes(left: LocalTime, right: LocalTime): Ordering {
  const a = localTimeMinutes(left);
  const b = localTimeMinutes(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

export function fromNativeTimeInput(
  value: string,
): LocalValueResult<LocalTime> {
  if (value.length !== 5 || !/^[0-9]{2}:[0-9]{2}$/.test(value)) {
    return localValueError(
      "invalid_time",
      "Use a native time in HH:MM format.",
    );
  }
  return parseLocalTime(value.slice(0, 2) + value.slice(3, 5));
}

export function toNativeTimeInput(time: LocalTime): string {
  const canonical = formatLocalTime(time);
  return canonical.slice(0, 2) + ":" + canonical.slice(2, 4);
}
