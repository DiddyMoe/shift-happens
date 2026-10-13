import {
  compareLocalDateTimes,
  parseLocalDateTime,
  type LocalDateTime,
} from "./localDateTime.ts";
import { localValueError, type LocalValueResult } from "./localValueResult.ts";

/** A nonempty half-open interval [start, end) of local scheduling datetimes. */
export class LocalInterval {
  readonly start: LocalDateTime;
  readonly end: LocalDateTime;

  private constructor(start: LocalDateTime, end: LocalDateTime) {
    this.start = start;
    this.end = end;
    Object.freeze(this);
  }

  static fromEndpoints(
    start: LocalDateTime,
    end: LocalDateTime,
  ): LocalValueResult<LocalInterval> {
    if (compareLocalDateTimes(start, end) >= 0) {
      return localValueError(
        "invalid_interval",
        "The interval end must be later than its start.",
      );
    }
    return { ok: true, value: new LocalInterval(start, end) };
  }
}

export function parseLocalInterval(
  start: string,
  end: string,
): LocalValueResult<LocalInterval> {
  const parsedStart = parseLocalDateTime(start);
  if (!parsedStart.ok) return parsedStart;
  const parsedEnd = parseLocalDateTime(end);
  if (!parsedEnd.ok) return parsedEnd;
  return LocalInterval.fromEndpoints(parsedStart.value, parsedEnd.value);
}

export function localIntervalsOverlap(
  left: LocalInterval,
  right: LocalInterval,
): boolean {
  return (
    compareLocalDateTimes(left.start, right.end) < 0 &&
    compareLocalDateTimes(right.start, left.end) < 0
  );
}
