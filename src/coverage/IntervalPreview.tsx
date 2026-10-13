import { useState } from "react";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { formatHours } from "../lib/duration";
import { formatLocalDate } from "../lib/localDate";
import { parseLocalDateTime } from "../lib/localDateTime";
import { formatLocalTime } from "../lib/localTime";
import {
  dayLabels,
  previewCoverageInterval,
  referenceDate,
  type CoverageIntervalPreview,
  type DayType,
} from "../lib/positionsCoverage";

/** "Monday 2026-10-05" and "0700" for a wire-format local datetime. */
function dateAndTime(value: string, weekday: string) {
  const parsed = parseLocalDateTime(value);
  return parsed.ok
    ? {
        date: `${weekday} ${formatLocalDate(parsed.value.date)}`,
        time: formatLocalTime(parsed.value.time),
      }
    : { date: weekday, time: value };
}

function describe(preview: CoverageIntervalPreview) {
  const start = dateAndTime(
    preview.actual_start,
    dayLabels[preview.actual_start_weekday],
  );
  const end = dateAndTime(
    preview.actual_end,
    dayLabels[preview.actual_end_weekday],
  );
  const span =
    start.date === end.date
      ? `${start.date}, ${start.time}–${end.time}`
      : `${start.date} ${start.time} → ${end.date} ${end.time}`;
  return `Works ${span} · ${formatHours(preview.elapsed_minutes)}`;
}

/** When a coverage item's shift actually runs for one operational date. */
export function IntervalPreview({
  shiftId,
  startOffset,
  endOffset,
  dayType,
  busy,
}: {
  shiftId: string;
  startOffset: number;
  endOffset: number;
  /** The item's operational day; the preview date starts on one. */
  dayType: DayType;
  busy: boolean;
}) {
  const [date, setDate] = useState(() => referenceDate(dayType));
  const [seenDay, setSeenDay] = useState(dayType);
  if (seenDay !== dayType) {
    setSeenDay(dayType);
    setDate(referenceDate(dayType));
  }
  const preview = useCommandQuery(
    JSON.stringify([shiftId, date, startOffset, endOffset]),
    () => previewCoverageInterval(shiftId, date, startOffset, endOffset),
  );
  return (
    <div className="panel-quiet pc-preview">
      <div className="toolbar">
        <label>
          Operational date
          <input
            type="date"
            min="0001-01-01"
            max="9999-12-31"
            value={date}
            disabled={busy}
            onChange={(e) => setDate(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="btn btn-quiet"
          disabled={busy}
          onClick={preview.reload}
        >
          Refresh preview
        </button>
      </div>
      {preview.error ? (
        <div className="notice notice-error" role="alert">
          <p>{preview.error}</p>
        </div>
      ) : preview.loading || !preview.data ? (
        <p className="muted" role="status">
          Calculating…
        </p>
      ) : (
        <p role="status">{describe(preview.data)}</p>
      )}
    </div>
  );
}
