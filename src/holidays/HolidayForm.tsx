import type { RefObject } from "react";
import { Link } from "react-router";
import { LoadError } from "../RecordControls";
import { RecordStatusTag } from "../components/RecordStatusTag";
import type { Holiday, HolidayData } from "../lib/holidays";
import type { CoverageTemplate } from "../lib/positionsCoverage";
import { HolidayDateFields } from "./HolidayDateFields";
import { holidaySets } from "./labels";

export function HolidayForm({
  row,
  data,
  year,
  templates,
  templatesError,
  busy,
  headingRef,
  onChange,
  onRetryTemplates,
  onSave,
  onCancel,
  onArchive,
}: {
  row: Holiday | null;
  /** The edited values; the page keeps them. */
  data: HolidayData;
  year: number;
  /** Custom coverage a holiday can use, or null while it loads. */
  templates: readonly CoverageTemplate[] | null;
  templatesError: string;
  busy: boolean;
  /** The form's heading, which takes focus after a save. */
  headingRef: RefObject<HTMLHeadingElement | null>;
  onChange: (data: HolidayData) => void;
  onRetryTemplates: () => void;
  onSave: (data: HolidayData) => void;
  onCancel: () => void;
  onArchive: () => void;
}) {
  const archived = row?.archived_at != null;
  const custom = data.coverage_behavior === "custom";
  // A template the holiday still points at but that can no longer be chosen.
  const previousTemplate =
    templates !== null &&
    data.custom_coverage_template_id !== null &&
    !templates.some((t) => t.id === data.custom_coverage_template_id);
  return (
    <form
      className="hol-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(data);
      }}
    >
      <div className="detail-header">
        <div>
          <h2 ref={headingRef} tabIndex={-1}>
            {row ? row.name : "New holiday"}
          </h2>
        </div>
        {row && (
          <div className="cluster">
            {row.seed_key && <span className="tag">Built-in</span>}
            <RecordStatusTag record={row} />
          </div>
        )}
      </div>
      {archived && (
        <div className="notice notice-info">
          <p>This holiday is archived and can&rsquo;t be edited.</p>
        </div>
      )}
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Holiday</legend>
        <div className="fields">
          <label>
            Name
            <input
              required
              value={data.name}
              onChange={(e) => onChange({ ...data, name: e.target.value })}
            />
          </label>
          <label>
            Holiday set
            <select
              value={data.holiday_set}
              onChange={(e) => {
                const set = holidaySets.find((s) => s.value === e.target.value);
                if (set) onChange({ ...data, holiday_set: set.value });
              }}
            >
              {holidaySets.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Status
            <select
              value={data.active_status}
              onChange={(e) =>
                onChange({
                  ...data,
                  active_status:
                    e.target.value === "inactive" ? "inactive" : "active",
                })
              }
            >
              <option value="active">Active</option>
              <option value="inactive">Inactive</option>
            </select>
          </label>
        </div>
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Date</legend>
        <HolidayDateFields data={data} year={year} onChange={onChange} />
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Coverage</legend>
        <div className="fields">
          <label>
            Coverage
            <select
              value={data.coverage_behavior}
              onChange={(e) => {
                const choice = e.target.value === "custom";
                onChange({
                  ...data,
                  coverage_behavior: choice ? "custom" : "normal",
                  custom_coverage_template_id: choice
                    ? (templates?.[0]?.id ?? null)
                    : null,
                });
              }}
            >
              <option value="normal">Normal weekday coverage</option>
              <option value="custom">Custom holiday coverage</option>
            </select>
          </label>
          {custom && (
            <label>
              Custom coverage template
              <select
                required
                value={data.custom_coverage_template_id ?? ""}
                onChange={(e) =>
                  onChange({
                    ...data,
                    custom_coverage_template_id: e.target.value || null,
                  })
                }
              >
                <option value="">
                  {templates === null && !templatesError
                    ? "Loading templates…"
                    : "Choose a template"}
                </option>
                {previousTemplate && (
                  <option
                    value={data.custom_coverage_template_id ?? ""}
                    disabled
                  >
                    Previous template (no longer available)
                  </option>
                )}
                {templates?.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
        {custom && templatesError && (
          <LoadError message={templatesError} onRetry={onRetryTemplates} />
        )}
        {custom && templates?.length === 0 && (
          <p className="field-help">
            No holiday custom templates with items yet. Add one in{" "}
            <Link to="/positions-coverage">Positions and coverage</Link>.
          </p>
        )}
        {custom && previousTemplate && (
          <p className="field-help">
            Choose a current template before making this holiday active.
          </p>
        )}
      </fieldset>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Notes</legend>
        <label>
          <span className="visually-hidden">Notes</span>
          <textarea
            rows={3}
            placeholder="Scheduling notes only — no patient or medical details."
            value={data.notes ?? ""}
            onChange={(e) =>
              onChange({ ...data, notes: e.target.value || null })
            }
          />
        </label>
      </fieldset>
      <div className="form-actions">
        {!archived && (
          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? "Saving…" : "Save holiday"}
          </button>
        )}
        <button
          className="btn btn-quiet"
          type="button"
          disabled={busy}
          onClick={onCancel}
        >
          {archived ? "Close" : "Cancel"}
        </button>
        {row && !archived && (
          <>
            <span className="spacer" />
            <button
              className="btn btn-danger"
              type="button"
              disabled={busy}
              onClick={onArchive}
            >
              Archive holiday
            </button>
          </>
        )}
      </div>
    </form>
  );
}
