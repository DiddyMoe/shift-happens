import { useState, type RefObject } from "react";
import {
  dayLabels,
  dayTypes,
  type CoverageTemplate,
  type CoverageTemplateData,
} from "../lib/positionsCoverage";

export function CoverageTemplateForm({
  row,
  initial,
  busy,
  headingRef,
  onSave,
  onCancel,
}: {
  row: CoverageTemplate | null;
  initial: CoverageTemplateData;
  busy: boolean;
  /** The form's heading, which takes focus after a failed save. */
  headingRef: RefObject<HTMLHeadingElement | null>;
  onSave: (data: CoverageTemplateData) => void;
  onCancel: () => void;
}) {
  const [data, setData] = useState(initial);
  return (
    <form
      className="panel"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(data);
      }}
    >
      <div className="detail-header">
        <div>
          <h3 ref={headingRef} tabIndex={-1} className="pc-title">
            {row ? "Edit coverage template" : "New coverage template"}
          </h3>
          {row?.is_default && (
            <p className="fine-print pc-form-note">
              A default template keeps its day and stays active until another
              template replaces it.
            </p>
          )}
        </div>
      </div>
      <fieldset className="form-section" disabled={busy}>
        <legend>Template details</legend>
        <div className="fields">
          <label>
            Name
            <input
              required
              maxLength={240}
              value={data.name}
              onChange={(e) => setData((d) => ({ ...d, name: e.target.value }))}
            />
          </label>
          <label>
            Operational day
            <select
              value={data.day_type}
              disabled={row?.is_default}
              onChange={(e) => {
                const day = dayTypes.find((d) => d === e.target.value);
                if (day) setData((d) => ({ ...d, day_type: day }));
              }}
            >
              {dayTypes.map((day) => (
                <option key={day} value={day}>
                  {dayLabels[day]}
                </option>
              ))}
            </select>
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
      <div className="form-actions">
        <button type="submit" className="btn btn-primary" disabled={busy}>
          {busy ? "Saving…" : "Save template"}
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
