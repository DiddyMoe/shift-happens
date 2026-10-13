import { useId, useRef, useState, type RefObject } from "react";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { LabelSelect } from "../employees/LabelSelect";
import { notesPlaceholder } from "../employees/format";
import {
  availabilityLabels,
  ptoLabels,
  type Entry,
  type EntryDraft,
} from "../lib/availabilityPto";
import { activeStatusLabels, type Employee } from "../lib/employees";
import {
  formatLocalTime,
  fromNativeTimeInput,
  parseLocalTime,
  toNativeTimeInput,
} from "../lib/localTime";
import { useRevealOnMount } from "../lib/reveal";
import { intervalPreview } from "./intervalPreview";

const lengthLabels = { full: "Full day", partial: "Partial day" };

function nativeTime(value: string | null) {
  const parsed = parseLocalTime(value ?? "");
  return parsed.ok ? toNativeTimeInput(parsed.value) : "";
}
function canonicalTime(value: string) {
  const parsed = fromNativeTimeInput(value);
  return parsed.ok ? formatLocalTime(parsed.value) : null;
}

export function EntryForm({
  noun,
  row,
  initial,
  employees,
  busy,
  headingRef,
  onSave,
  onCancel,
  onArchive,
}: {
  noun: string;
  row: Entry | null;
  initial: EntryDraft;
  employees: readonly Employee[];
  busy: boolean;
  headingRef: RefObject<HTMLHeadingElement | null>;
  onSave: (data: EntryDraft) => void;
  onCancel: () => void;
  onArchive: () => void;
}) {
  const [source, setSource] = useState(initial);
  const [data, setData] = useState(initial);
  // A saved or reloaded entry replaces the form's values in place. The form
  // stays mounted, so the button that was pressed keeps focus.
  if (source !== initial) {
    setSource(initial);
    setData(initial);
  }
  const archived = row !== null && row.archived_at !== null;
  const employee = employees.find((e) => e.id === data.employee_id);
  const unavailable =
    employee &&
    (employee.archived_at !== null || employee.active_status !== "active")
      ? employee
      : null;
  const lengthHelpId = useId();
  const root = useRef<HTMLFormElement>(null);
  useRevealOnMount(root);
  return (
    <form
      ref={root}
      className="panel avail-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(data);
      }}
    >
      <div className="detail-header">
        <div>
          <h3 ref={headingRef} tabIndex={-1}>
            {row ? `Edit ${noun} entry` : `New ${noun} entry`}
          </h3>
        </div>
        {row && <RecordStatusTag record={row} />}
      </div>
      {unavailable && !archived && (
        <div className="notice notice-warn">
          <p>
            {unavailable.display_name} is inactive or archived, so this entry
            can only be saved as inactive.
          </p>
        </div>
      )}
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Details</legend>
        <div className="fields">
          <label>
            Employee
            <select
              required
              value={data.employee_id}
              onChange={(e) =>
                setData((d) => ({ ...d, employee_id: e.target.value }))
              }
            >
              <option value="">Choose an employee</option>
              {employees.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.display_name}
                  {e.archived_at
                    ? ", archived"
                    : e.active_status === "inactive"
                      ? ", inactive"
                      : ""}
                </option>
              ))}
            </select>
          </label>
          {"entry_type" in data ? (
            <LabelSelect
              label="Type"
              labels={availabilityLabels}
              value={data.entry_type}
              onChange={(type) =>
                setData((d) =>
                  "entry_type" in d ? { ...d, entry_type: type } : d,
                )
              }
            />
          ) : (
            <LabelSelect
              label="PTO status"
              labels={ptoLabels}
              value={data.status}
              onChange={(status) =>
                setData((d) => ("status" in d ? { ...d, status } : d))
              }
            />
          )}
          <LabelSelect
            label="Entry status"
            labels={activeStatusLabels}
            value={data.active_status}
            onChange={(status) =>
              setData((d) => ({ ...d, active_status: status }))
            }
          />
        </div>
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Dates and times</legend>
        <div className="fields">
          <label>
            First date
            <input
              required
              type="date"
              min="0001-01-01"
              max="9999-12-31"
              value={data.start_date}
              onChange={(e) =>
                setData((d) => ({ ...d, start_date: e.target.value }))
              }
            />
          </label>
          <label>
            Last date
            <input
              required
              type="date"
              min={data.start_date || "0001-01-01"}
              max="9999-12-31"
              value={data.end_date}
              onChange={(e) =>
                setData((d) => ({ ...d, end_date: e.target.value }))
              }
            />
          </label>
          <LabelSelect
            label="Length"
            labels={lengthLabels}
            value={data.is_full_day ? "full" : "partial"}
            describedBy={lengthHelpId}
            onChange={(length) =>
              setData((d) => ({
                ...d,
                is_full_day: length === "full",
                start_time: null,
                end_time: null,
              }))
            }
          />
          {!data.is_full_day && (
            <>
              <label>
                Start time
                <input
                  required
                  type="time"
                  step={60}
                  value={nativeTime(data.start_time)}
                  onChange={(e) =>
                    setData((d) => ({
                      ...d,
                      start_time: canonicalTime(e.target.value),
                    }))
                  }
                />
              </label>
              <label>
                End time
                <input
                  required
                  type="time"
                  step={60}
                  value={nativeTime(data.end_time)}
                  onChange={(e) =>
                    setData((d) => ({
                      ...d,
                      end_time: canonicalTime(e.target.value),
                    }))
                  }
                />
              </label>
            </>
          )}
        </div>
        <p id={lengthHelpId} className="field-help">
          A partial day repeats the same hours on each date in the range. An end
          time before the start time ends the next day.
        </p>
        <p className="panel-quiet avail-preview" role="status">
          {intervalPreview(data)}
        </p>
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Notes</legend>
        <textarea
          aria-label="Notes"
          rows={3}
          maxLength={8000}
          placeholder={notesPlaceholder}
          value={data.notes ?? ""}
          onChange={(e) =>
            setData((d) => ({ ...d, notes: e.target.value || null }))
          }
        />
      </fieldset>
      <div className="form-actions">
        {!archived && (
          <button type="submit" className="btn btn-primary" disabled={busy}>
            {busy ? "Saving…" : `Save ${noun} entry`}
          </button>
        )}
        <button
          type="button"
          className="btn btn-quiet"
          disabled={busy}
          onClick={onCancel}
        >
          {archived ? "Close" : "Cancel"}
        </button>
        {row && !archived && (
          <>
            <span className="spacer" />
            <button
              type="button"
              className="btn btn-danger"
              disabled={busy}
              onClick={onArchive}
            >
              Archive entry
            </button>
          </>
        )}
      </div>
    </form>
  );
}
