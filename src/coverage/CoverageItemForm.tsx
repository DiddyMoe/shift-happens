import { useId, useRef, useState } from "react";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { useRevealOnMount } from "../lib/reveal";
import type { ShiftTemplate } from "../lib/shiftTemplates";
import {
  dayLabels,
  isActive,
  statusSuffix,
  type CoverageItem,
  type CoverageItemData,
  type CoverageTemplate,
  type Position,
} from "../lib/positionsCoverage";
import { IntervalPreview } from "./IntervalPreview";

/** Where an item's shift falls, relative to its operational day. */
const timings = [
  { value: "0,0", start: 0, end: 0, label: "Same day" },
  { value: "0,1", start: 0, end: 1, label: "Ends next day" },
  { value: "1,1", start: 1, end: 1, label: "Starts next day" },
] as const;

export function CoverageItemForm({
  template,
  positions,
  templates,
  shifts,
  row,
  initial,
  busy,
  onSave,
  onCancel,
}: {
  /** The template whose items are being edited. */
  template: CoverageTemplate;
  positions: readonly Position[];
  templates: readonly CoverageTemplate[];
  shifts: readonly ShiftTemplate[];
  row: CoverageItem | null;
  initial: CoverageItemData;
  busy: boolean;
  onSave: (data: CoverageItemData) => void;
  onCancel: () => void;
}) {
  const [data, setData] = useState(initial);
  const templateHelp = useId();
  const archived = row !== null && row.archived_at !== null;
  const chosenTemplate = templates.find(
    (t) => t.id === data.coverage_template_id,
  );
  const root = useRef<HTMLFormElement>(null);
  useRevealOnMount(root);
  return (
    <form
      ref={root}
      className="panel pc-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(data);
      }}
    >
      <div className="detail-header">
        <div>
          <h3 className="pc-title">
            {archived
              ? "Coverage item"
              : row
                ? "Edit coverage item"
                : "New coverage item"}
            {row && <RecordStatusTag record={row} />}
          </h3>
        </div>
      </div>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Coverage item</legend>
        <div className="fields">
          <div>
            <label>
              Template
              <select
                required
                aria-describedby={templateHelp}
                value={data.coverage_template_id}
                onChange={(e) =>
                  setData((d) => ({
                    ...d,
                    coverage_template_id: e.target.value,
                  }))
                }
              >
                {templates
                  .filter(
                    (t) => t.id === initial.coverage_template_id || isActive(t),
                  )
                  .map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name}, {dayLabels[t.day_type]}
                      {statusSuffix(t)}
                    </option>
                  ))}
              </select>
            </label>
            <p className="field-help" id={templateHelp}>
              Moving an item can&rsquo;t leave a default template empty.
            </p>
          </div>
          <label>
            Position
            <select
              required
              value={data.shift_position_id}
              onChange={(e) =>
                setData((d) => ({ ...d, shift_position_id: e.target.value }))
              }
            >
              {positions
                .filter(
                  (p) => p.id === initial.shift_position_id || isActive(p),
                )
                .map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                    {statusSuffix(p)}
                  </option>
                ))}
            </select>
          </label>
          <label>
            Shift
            <select
              required
              value={data.shift_template_id}
              onChange={(e) =>
                setData((d) => ({ ...d, shift_template_id: e.target.value }))
              }
            >
              {shifts
                .filter(
                  (s) => s.id === initial.shift_template_id || isActive(s),
                )
                .map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}, {s.start_time}–{s.end_time}
                    {statusSuffix(s)}
                  </option>
                ))}
            </select>
          </label>
          <label>
            Timing
            <select
              value={`${data.actual_start_day_offset},${data.actual_end_day_offset}`}
              onChange={(e) => {
                const timing = timings.find((t) => t.value === e.target.value);
                if (timing)
                  setData((d) => ({
                    ...d,
                    actual_start_day_offset: timing.start,
                    actual_end_day_offset: timing.end,
                  }));
              }}
            >
              {timings.map((t) => (
                <option key={t.value} value={t.value}>
                  {t.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Required count
            <input
              type="number"
              required
              min="1"
              max="100"
              step="1"
              value={data.required_count}
              onChange={(e) =>
                setData((d) => ({
                  ...d,
                  required_count: e.target.valueAsNumber,
                }))
              }
            />
          </label>
          <label>
            Sort order
            <input
              type="number"
              required
              min={-Number.MAX_SAFE_INTEGER}
              max={Number.MAX_SAFE_INTEGER}
              step="1"
              value={data.sort_order}
              onChange={(e) =>
                setData((d) => ({ ...d, sort_order: e.target.valueAsNumber }))
              }
            />
          </label>
          <label>
            Display label
            <input
              maxLength={240}
              value={data.display_label ?? ""}
              onChange={(e) =>
                setData((d) => ({
                  ...d,
                  display_label: e.target.value || null,
                }))
              }
            />
          </label>
          <label>
            Status
            <select
              value={data.active_status}
              onChange={(e) =>
                setData((d) => ({
                  ...d,
                  active_status:
                    e.target.value === "inactive" ? "inactive" : "active",
                }))
              }
            >
              <option value="active">Active</option>
              <option value="inactive">Inactive</option>
            </select>
          </label>
          <label className="span-all">
            Notes
            <textarea
              rows={3}
              maxLength={8000}
              placeholder="Scheduling notes only — no patient or medical details."
              value={data.notes ?? ""}
              onChange={(e) =>
                setData((d) => ({ ...d, notes: e.target.value || null }))
              }
            />
          </label>
        </div>
      </fieldset>
      <IntervalPreview
        shiftId={data.shift_template_id}
        startOffset={data.actual_start_day_offset}
        endOffset={data.actual_end_day_offset}
        dayType={chosenTemplate?.day_type ?? template.day_type}
        busy={busy}
      />
      <div className="form-actions">
        {!archived && (
          <button type="submit" className="btn btn-primary" disabled={busy}>
            {busy ? "Saving…" : "Save coverage item"}
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
      </div>
    </form>
  );
}
