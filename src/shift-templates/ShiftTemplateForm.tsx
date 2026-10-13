import { useState, type RefObject } from "react";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { formatHours, hoursText } from "../lib/duration";
import type { TimeOfDay } from "../lib/employees";
import {
  formatLocalTime,
  fromNativeTimeInput,
  parseLocalTime,
  toNativeTimeInput,
} from "../lib/localTime";
import type { ShiftTemplate, ShiftTemplateData } from "../lib/shiftTemplates";
import {
  LUNCH_THRESHOLD_MINUTES,
  UNPAID_LUNCH_MINUTES,
  previewShiftTime,
} from "../lib/shiftTime";
import { BoardColorField } from "./BoardColorField";
import { PaidTimePreview } from "./PaidTimePreview";

const categories: readonly { value: TimeOfDay; label: string }[] = [
  { value: "day", label: "Day" },
  { value: "evening", label: "Evening" },
  { value: "night", label: "Night" },
];
const lunchPolicy = `Shifts of ${hoursText(LUNCH_THRESHOLD_MINUTES)} hours or more include a ${formatHours(UNPAID_LUNCH_MINUTES)} unpaid lunch; shorter shifts have none.`;

function nativeTime(value: string) {
  const parsed = parseLocalTime(value);
  return parsed.ok ? toNativeTimeInput(parsed.value) : "";
}

export function ShiftTemplateForm({
  initial,
  row,
  busy,
  headingRef,
  onSave,
  onClose,
  onArchive,
}: {
  initial: ShiftTemplateData;
  row: ShiftTemplate | null;
  busy: boolean;
  /** The form's heading, which takes focus after a save. */
  headingRef: RefObject<HTMLHeadingElement | null>;
  onSave: (data: ShiftTemplateData) => void;
  onClose: () => void;
  onArchive: () => void;
}) {
  const [data, setData] = useState(initial);
  const timesValid = previewShiftTime(data.start_time, data.end_time).ok;
  const archived = row?.archived_at != null;
  const change = <K extends keyof ShiftTemplateData>(
    key: K,
    value: ShiftTemplateData[K],
  ) => setData((d) => ({ ...d, [key]: value }));
  function changeTime(key: "start_time" | "end_time", value: string) {
    if (!value) {
      change(key, "");
      return;
    }
    const parsed = fromNativeTimeInput(value);
    if (parsed.ok) change(key, formatLocalTime(parsed.value));
  }
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (timesValid) onSave(data);
      }}
    >
      <div className="detail-header">
        <div>
          <h2 ref={headingRef} tabIndex={-1}>
            {row ? row.name : "New shift template"}
          </h2>
        </div>
        <div className="cluster">
          {row && <RecordStatusTag record={row} />}
          <button
            className="btn btn-quiet"
            type="button"
            disabled={busy}
            onClick={onClose}
          >
            Close
          </button>
        </div>
      </div>
      {archived && (
        <p className="notice notice-warn">
          This template is archived and can&rsquo;t be edited.
        </p>
      )}
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Name and status</legend>
        <div className="fields">
          <label>
            Name
            <input
              required
              maxLength={240}
              value={data.name}
              onChange={(e) => change("name", e.target.value)}
            />
          </label>
          <label>
            Display label (optional)
            <input
              maxLength={240}
              value={data.display_label ?? ""}
              onChange={(e) => change("display_label", e.target.value || null)}
            />
          </label>
          <label>
            Status
            <select
              value={data.active_status}
              onChange={(e) =>
                change(
                  "active_status",
                  e.target.value === "inactive" ? "inactive" : "active",
                )
              }
            >
              <option value="active">Active</option>
              <option value="inactive">Inactive</option>
            </select>
          </label>
        </div>
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Times</legend>
        <div className="tpl-times">
          <div className="stack">
            <div className="fields-2">
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
              <label>
                End time
                <input
                  type="time"
                  step={60}
                  required
                  value={nativeTime(data.end_time)}
                  onChange={(e) => changeTime("end_time", e.target.value)}
                />
              </label>
            </div>
            <p className="fine-print">{lunchPolicy}</p>
          </div>
          <PaidTimePreview
            start={data.start_time}
            end={data.end_time}
            savedPaidMinutes={row?.paid_minutes ?? null}
          />
        </div>
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Category and flags</legend>
        <div className="fields">
          <label>
            Category
            <select
              value={data.time_of_day}
              onChange={(e) => {
                const category = categories.find(
                  (c) => c.value === e.target.value,
                );
                if (category) change("time_of_day", category.value);
              }}
            >
              {categories.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="checks tpl-flags">
          <label>
            <input
              type="checkbox"
              checked={data.requires_recovery_after_shift}
              onChange={(e) =>
                change("requires_recovery_after_shift", e.target.checked)
              }
            />
            Requires recovery after shift
          </label>
          <label>
            <input
              type="checkbox"
              checked={data.is_night_shift}
              onChange={(e) => change("is_night_shift", e.target.checked)}
            />
            Marked as night shift
          </label>
        </div>
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Board color</legend>
        <BoardColorField
          color={data.color_hex}
          title={data.display_label ?? data.name}
          time={`${data.start_time}–${data.end_time}`}
          onChange={(color) => change("color_hex", color)}
        />
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Notes</legend>
        <label>
          <span className="visually-hidden">Notes</span>
          <textarea
            rows={3}
            maxLength={8000}
            placeholder="Scheduling notes only — no patient or medical details."
            value={data.notes ?? ""}
            onChange={(e) => change("notes", e.target.value || null)}
          />
        </label>
      </fieldset>
      {!archived && (
        <div className="form-actions">
          <button
            className="btn btn-primary"
            type="submit"
            disabled={busy || !timesValid}
          >
            {busy ? "Saving…" : "Save template"}
          </button>
          {row && (
            <>
              <span className="spacer" />
              <button
                className="btn btn-danger"
                type="button"
                disabled={busy}
                onClick={onArchive}
              >
                Archive template
              </button>
            </>
          )}
        </div>
      )}
    </form>
  );
}
