import { useId, useRef, useState } from "react";
import {
  activeStatusLabels,
  restrictionKindLabels,
  timeOfDayLabels,
  type RestrictionData,
  type RestrictionKind,
} from "../lib/employees";
import {
  formatLocalTime,
  fromNativeTimeInput,
  parseLocalTime,
  toNativeTimeInput,
} from "../lib/localTime";
import { useRevealOnMount } from "../lib/reveal";
import { notesPlaceholder, weekdayLabels } from "./format";
import { LabelSelect } from "./LabelSelect";

function nativeTime(value: string | null) {
  if (!value) return "";
  const parsed = parseLocalTime(value);
  return parsed.ok ? toNativeTimeInput(parsed.value) : "";
}

export function RestrictionForm({
  initial,
  busy,
  onSave,
  onCancel,
}: {
  initial: RestrictionData;
  busy: boolean;
  onSave: (data: RestrictionData) => void;
  onCancel: () => void;
}) {
  const [data, setData] = useState(initial);
  const [timeError, setTimeError] = useState("");
  const endHelpId = useId();
  const root = useRef<HTMLFormElement>(null);
  useRevealOnMount(root);
  function changeKind(kind: RestrictionKind) {
    const byTime = kind === "recurring_day_time_block";
    setData((d) => ({
      ...d,
      restriction_kind: kind,
      time_of_day: byTime ? null : "day",
      weekday: byTime ? "monday" : null,
      start_time: byTime ? "0900" : null,
      end_time: byTime ? "1700" : null,
    }));
  }
  function changeTime(key: "start_time" | "end_time", value: string) {
    if (!value) {
      setData((d) => ({ ...d, [key]: null }));
      return;
    }
    const parsed = fromNativeTimeInput(value);
    if (!parsed.ok) {
      setTimeError(parsed.error.message);
      return;
    }
    setTimeError("");
    setData((d) => ({ ...d, [key]: formatLocalTime(parsed.value) }));
  }
  return (
    <form
      ref={root}
      className="panel-quiet emp-restriction-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(data);
      }}
    >
      <fieldset className="form-section" disabled={busy}>
        <legend>Restriction</legend>
        <div className="fields">
          <LabelSelect
            label="Restrict by"
            labels={restrictionKindLabels}
            value={data.restriction_kind}
            onChange={changeKind}
          />
          {data.restriction_kind === "time_of_day_block" ? (
            <LabelSelect
              label="Shift type"
              labels={timeOfDayLabels}
              value={data.time_of_day ?? "day"}
              onChange={(time) => setData((d) => ({ ...d, time_of_day: time }))}
            />
          ) : (
            <>
              <LabelSelect
                label="Weekday"
                labels={weekdayLabels}
                value={data.weekday ?? "monday"}
                onChange={(day) => setData((d) => ({ ...d, weekday: day }))}
              />
              <label>
                Start time
                <input
                  type="time"
                  step={60}
                  required
                  value={nativeTime(data.start_time)}
                  onChange={(e) => changeTime("start_time", e.target.value)}
                />
              </label>
              <div className="emp-field">
                <label>
                  End time
                  <input
                    type="time"
                    step={60}
                    required
                    aria-describedby={endHelpId}
                    value={nativeTime(data.end_time)}
                    onChange={(e) => changeTime("end_time", e.target.value)}
                  />
                </label>
                <p id={endHelpId} className="field-help">
                  An end time before the start time ends the next day.
                </p>
              </div>
            </>
          )}
          <LabelSelect
            label="Status"
            labels={activeStatusLabels}
            value={data.active_status}
            onChange={(status) =>
              setData((d) => ({ ...d, active_status: status }))
            }
          />
          <label className="span-all">
            Notes
            <textarea
              rows={2}
              maxLength={8000}
              placeholder={notesPlaceholder}
              value={data.notes ?? ""}
              onChange={(e) =>
                setData((d) => ({ ...d, notes: e.target.value || null }))
              }
            />
          </label>
        </div>
      </fieldset>
      {timeError && (
        <div role="alert" className="notice notice-error">
          <p>{timeError}</p>
        </div>
      )}
      <div className="form-actions">
        <button type="submit" className="btn btn-primary" disabled={busy}>
          {busy ? "Saving…" : "Save restriction"}
        </button>
        <button
          type="button"
          className="btn btn-quiet"
          disabled={busy}
          onClick={onCancel}
        >
          Cancel
        </button>
      </div>
    </form>
  );
}
