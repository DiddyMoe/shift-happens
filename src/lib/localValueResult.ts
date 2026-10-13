export type LocalValueErrorCode =
  | "invalid_date"
  | "invalid_time"
  | "invalid_datetime"
  | "invalid_interval"
  | "invalid_day_offset"
  | "out_of_range";

export type LocalValueResult<T> =
  | { readonly ok: true; readonly value: T }
  | {
      readonly ok: false;
      readonly error: {
        readonly code: LocalValueErrorCode;
        readonly message: string;
      };
    };

export type Ordering = -1 | 0 | 1;

export function localValueError(
  code: LocalValueErrorCode,
  message: string,
): LocalValueResult<never> {
  return { ok: false, error: { code, message } };
}
