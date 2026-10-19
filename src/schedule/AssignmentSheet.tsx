import { Link } from "react-router";
import { LoadError, RecordFeedback } from "../RecordControls";
import { editorFindingLabels } from "../lib/editorText";
import { formatHours } from "../lib/duration";
import { factEntries } from "../lib/findings";
import { displayLocalDate } from "../lib/localDate";
import { displayLocalDateTime } from "../lib/localDateTime";
import type { Feedback } from "../lib/recordFeedback";
import type { Assignment } from "../lib/schedules";
import type {
  Analysis,
  Candidate,
  Candidates,
  EditorAction,
  Finding,
  Projection,
} from "../lib/scheduleEditor";

export function Findings({
  findings,
  tone,
}: {
  findings: readonly Finding[];
  tone: "blocker" | "warning";
}) {
  return (
    <ul className={`finding-list is-${tone}`}>
      {findings.map((f) => (
        <li key={f.warning_id}>
          <strong>{editorFindingLabels[f.code] ?? f.code}</strong>
          <details>
            <summary>Current rule details</summary>
            <dl className="facts">
              {factEntries(f.context).map(([label, text]) => (
                <div key={label}>
                  <dt>{label}</dt>
                  <dd>{text}</dd>
                </div>
              ))}
            </dl>
          </details>
        </li>
      ))}
    </ul>
  );
}
export function Minimum({
  value,
  source = false,
}: {
  value: Projection | null;
  source?: boolean;
}) {
  if (!value) return null;
  const amount = (n: number) =>
    value.metric === "paid_minutes" ? formatHours(n) : String(n);
  return (
    <p>
      {source ? "Previous employee minimum" : "Candidate minimum"}:{" "}
      {amount(value.after)} of{" "}
      {value.required === null ? "no set minimum" : amount(value.required)}
      {value.metric === "paid_minutes" ? " paid" : " assignments"},{" "}
      {displayLocalDate(value.period_start)} to{" "}
      {value.period_end_exclusive &&
        displayLocalDate(value.period_end_exclusive)}{" "}
      (end excluded).
      {value.complete
        ? ` Complete period; shortfall ${amount(value.shortfall_before)} → ${amount(value.shortfall_after)}.`
        : " Partial period; minimum progress is not authoritative."}
    </p>
  );
}
function AnalysisDetails({ value }: { value: Analysis }) {
  return (
    <div className="analysis">
      <p>
        Projected schedule total: {formatHours(value.projected_paid_minutes)}{" "}
        paid in {value.projected_assignment_count} assignments. This workweek:{" "}
        {formatHours(value.weekly_paid_minutes)} paid,{" "}
        {formatHours(value.weekly_overtime_minutes)} overtime.
      </p>
      {value.workweek_start && (
        <p>
          Workweek: {displayLocalDate(value.workweek_start)} to{" "}
          {value.workweek_end_exclusive &&
            displayLocalDate(value.workweek_end_exclusive)}{" "}
          (end excluded).{" "}
          {value.workweek_complete
            ? "Complete period."
            : "Partial boundary period."}
        </p>
      )}
      {value.source_paid_minutes !== null && (
        <p>
          Previous employee after replacement:{" "}
          {formatHours(value.source_paid_minutes)} paid in{" "}
          {value.source_assignment_count} assignments.
        </p>
      )}
      {value.rest_checks.length > 0 && (
        <details>
          <summary>Rest and recovery between nearby assignments</summary>
          <ul>
            {value.rest_checks.map((check) => (
              <li
                key={`${check.prior_assignment_id}:${check.next_assignment_id}`}
              >
                {check.gap_minutes < 0
                  ? "Assignments overlap."
                  : `${formatHours(check.gap_minutes)} between shifts; ${formatHours(check.required_minutes)} required${check.recovery ? " for recovery" : check.minimum_rest_enabled ? " for minimum rest" : " (minimum rest check is off)"}. ${check.violates ? `${formatHours(check.deficit_minutes)} short.` : "No rest or recovery warning."}`}
              </li>
            ))}
          </ul>
        </details>
      )}
      <Minimum value={value.minimum} />
      <Minimum value={value.source_minimum} source />
      <p className="fine-print">
        Totals, overtime, minimums, overlaps, rest and consecutive days use this
        schedule only; work in other schedules or systems is not included.
      </p>
      {value.blockers.length > 0 && (
        <>
          <h3>Hard blockers</h3>
          <Findings findings={value.blockers} tone="blocker" />
        </>
      )}
      {value.warnings.length > 0 && (
        <>
          <h3>Warnings requiring acknowledgement</h3>
          <Findings findings={value.warnings} tone="warning" />
        </>
      )}
    </div>
  );
}

/** The contents of the assignment sheet for the selected shift. */
export function AssignmentSheet({
  row,
  readOnly,
  disabled,
  busy,
  candidateRead,
  candidateLoading,
  candidateError,
  candidateId,
  onCandidateChange,
  candidate,
  notes,
  onNotesChange,
  feedback,
  onEdit,
  onReload,
  closeLabel,
  onClose,
}: {
  row: Assignment;
  readOnly: boolean;
  disabled: boolean;
  busy: boolean;
  candidateRead: Candidates | null;
  candidateLoading: boolean;
  candidateError: string;
  candidateId: string;
  onCandidateChange: (id: string) => void;
  candidate: Candidate | undefined;
  notes: string;
  onNotesChange: (notes: string) => void;
  feedback: Feedback | null;
  onEdit: (action: EditorAction) => void;
  onReload: () => void;
  closeLabel: string;
  onClose: () => void;
}) {
  const when = {
    day: row.operational_day_label,
    date: displayLocalDate(row.schedule_date),
    start: displayLocalDateTime(row.actual_start_datetime),
    end: displayLocalDateTime(row.actual_end_datetime),
  };
  return (
    <>
      <div className="sheet-header">
        <div>
          <h2 id="sheet-title">
            {row.position_display_snapshot ?? row.position_name_snapshot}, slot{" "}
            {row.coverage_slot_index_snapshot}
          </h2>
          <p className="sheet-when">
            {when.day} {when.date}, {row.shift_name_snapshot}
          </p>
        </div>
        <button type="button" className="btn btn-quiet" onClick={onClose}>
          {closeLabel}
        </button>
      </div>
      <div className="sheet-body">
        <div className="sheet-section">
          <dl className="facts">
            <dt>Assigned</dt>
            <dd>
              {row.employee_display_name_snapshot ?? (
                <span className="tag tag-muted">Open shift</span>
              )}
            </dd>
            <dt>Starts</dt>
            <dd className="display">{when.start}</dd>
            <dt>Ends</dt>
            <dd className="display">{when.end}</dd>
            <dt>Paid time</dt>
            <dd className="display">
              {formatHours(row.shift_paid_minutes_snapshot)}
            </dd>
            <dt>Lock</dt>
            <dd>{row.locked ? "Locked" : "Unlocked"}</dd>
          </dl>
          <details className="sheet-more">
            <summary>Slot details</summary>
            <dl className="facts">
              <dt>Slot</dt>
              <dd>
                {row.coverage_slot_index_snapshot} of{" "}
                {row.coverage_required_count_snapshot}
              </dd>
              <dt>Coverage</dt>
              <dd>
                {row.coverage_template_name_snapshot},{" "}
                {row.coverage_day_type_snapshot}
                {row.coverage_item_display_snapshot
                  ? `, ${row.coverage_item_display_snapshot}`
                  : ""}
              </dd>
              <dt>Holiday</dt>
              <dd>
                {row.holiday_id
                  ? `${row.holiday_name_snapshot}, set ${row.holiday_set_snapshot?.toUpperCase()}, ${row.holiday_coverage_behavior_snapshot} coverage`
                  : "None"}
              </dd>
              <dt>Shift policy</dt>
              <dd>
                {row.shift_category_snapshot},{" "}
                {row.shift_night_snapshot ? "night" : "not night"},{" "}
                {row.shift_recovery_snapshot
                  ? "recovery required"
                  : "no recovery flag"}
              </dd>
              <dt>Unpaid lunch</dt>
              <dd>
                <span className="display">
                  {formatHours(row.shift_lunch_minutes_snapshot)}
                </span>
              </dd>
              <dt>Source</dt>
              <dd>
                {row.assignment_source},{" "}
                {row.manually_edited ? "manually edited" : "no manual edits"}
              </dd>
            </dl>
          </details>
          <div className="cluster sheet-actions">
            <button
              className="btn"
              disabled={disabled || readOnly}
              onClick={() => onEdit({ kind: row.locked ? "unlock" : "lock" })}
            >
              {row.locked ? "Unlock assignment" : "Lock assignment"}
            </button>
            <button
              className="btn"
              disabled={
                disabled ||
                readOnly ||
                row.locked ||
                row.open_shift ||
                candidateLoading ||
                !candidateRead ||
                candidateRead.open.status === "blocked"
              }
              onClick={() => onEdit({ kind: "open" })}
            >
              Mark open
            </button>
          </div>
          {candidateRead?.open.source_minimum && (
            <Minimum value={candidateRead.open.source_minimum} source />
          )}
        </div>
        {!readOnly && (
          <div className="sheet-section">
            <h3>Candidate employees</h3>
            {candidateLoading && (
              <p className="status-line" role="status">
                Evaluating current rules…
              </p>
            )}
            {candidateError && (
              <LoadError message={candidateError} onRetry={onReload} />
            )}
            {candidateRead && (
              <>
                <label>
                  Employee
                  <select
                    value={candidateId}
                    disabled={disabled || row.locked || candidateLoading}
                    onChange={(e) => onCandidateChange(e.target.value)}
                  >
                    <option value="">Choose an employee</option>
                    {candidateRead.candidates.map((e) => (
                      <option key={e.employee_id} value={e.employee_id}>
                        {e.display_name},{" "}
                        {e.status === "requires_confirmation"
                          ? "warnings to review"
                          : e.status}
                      </option>
                    ))}
                  </select>
                </label>
                {candidateRead.candidates.length === 0 && (
                  <p className="muted">
                    No active employees. Add employees in{" "}
                    <Link to="/employees">Employees</Link>.
                  </p>
                )}
                {candidate && (
                  <>
                    <AnalysisDetails value={candidate} />
                    <div className="cluster sheet-actions">
                      <button
                        className="btn btn-primary"
                        disabled={
                          disabled ||
                          row.locked ||
                          candidate.status === "blocked"
                        }
                        onClick={() =>
                          onEdit({
                            kind: "assign",
                            employee_id: candidate.employee_id,
                          })
                        }
                      >
                        {candidate.employee_id === row.assigned_employee_id
                          ? "Already assigned"
                          : "Assign employee"}
                      </button>
                    </div>
                  </>
                )}
              </>
            )}
          </div>
        )}
        <form
          className="sheet-section"
          onSubmit={(e) => {
            e.preventDefault();
            onEdit({ kind: "note", notes: notes || null });
          }}
        >
          <label>
            Assignment notes
            <textarea
              value={notes}
              maxLength={4000}
              disabled={disabled || readOnly || row.locked}
              onChange={(e) => onNotesChange(e.target.value)}
            />
          </label>
          <div className="cluster sheet-actions">
            <button
              className="btn"
              disabled={disabled || readOnly || row.locked}
            >
              Save note
            </button>
          </div>
        </form>
        <RecordFeedback
          feedback={feedback}
          busy={busy}
          onReload={onReload}
          reloadLabel="Reload schedule"
        />
      </div>
    </>
  );
}
