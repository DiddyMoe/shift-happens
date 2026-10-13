import { displayLocalDate } from "./lib/localDate";
import { displayLocalDateTime } from "./lib/localDateTime";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Confirmation } from "./RecordControls";
import { unreachableResult } from "./lib/ipc";
import { errorText } from "./lib/recordFeedback";
import type { Schedule } from "./lib/schedules";
import { factEntries } from "./lib/findings";
import {
  finalizeSchedule,
  type FinalizationOutcome,
  type Finding,
  type ValidationResult,
} from "./lib/validation";

function Findings({ findings }: { findings: readonly Finding[] }) {
  return (
    <ul className="finding-list">
      {findings.map((finding) => (
        <li
          key={finding.id}
          className={
            finding.severity === "error" ? "is-error" : "is-warning-item"
          }
        >
          <strong>
            {finding.code === "open_shift" ? "Open shift" : finding.message}
          </strong>
          {finding.operational_date && (
            <p className="display">
              {displayLocalDate(finding.operational_date)}
              {finding.actual_datetime
                ? `, ${displayLocalDateTime(finding.actual_datetime)}`
                : ""}
            </p>
          )}
          {factEntries(finding.context).length > 0 && (
            <details>
              <summary>Rule details</summary>
              <dl className="facts">
                {factEntries(finding.context).map(([label, text]) => (
                  <div key={label}>
                    <dt>{label}</dt>
                    <dd>{text}</dd>
                  </div>
                ))}
              </dl>
            </details>
          )}
        </li>
      ))}
    </ul>
  );
}
/** Finalize for one plan. `children` places the Finalize… button (null when
 * the plan cannot be finalized) inside the plan's action row; outcome
 * messages and the confirmation render below that row. */
export function Finalization({
  schedule,
  disabled = false,
  onChanged,
  onShowIssues,
  onValidation,
  children,
}: {
  schedule: Schedule;
  disabled?: boolean;
  onChanged: (schedule: Schedule, historyUnavailable?: boolean) => void;
  /** Opens the plan's issues list, for example from a blocked attempt. */
  onShowIssues?: (() => void) | undefined;
  /** Receives the validation result that finalization checked. */
  onValidation?: ((result: ValidationResult) => void) | undefined;
  children: (finalizeButton: ReactNode) => ReactNode;
}) {
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  const sequence = useRef(0);
  const [review, setReview] = useState<{
    schedule: Schedule;
    outcome: FinalizationOutcome | null;
  } | null>(null);
  const [blocked, setBlocked] = useState<ValidationResult | null>(null);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);
  const [celebrate, setCelebrate] = useState(false);
  useEffect(
    () => () => {
      sequence.current += 1;
    },
    [],
  );
  useEffect(() => {
    if (!celebrate) return;
    const timer = window.setTimeout(() => setCelebrate(false), 3500);
    return () => window.clearTimeout(timer);
  }, [celebrate]);
  async function submit() {
    if (!review || inFlight.current || disabled) return;
    const captured = review.schedule;
    const ids =
      review.outcome === null
        ? null
        : (review.outcome.validation?.findings
            .filter((f) => f.severity === "warning")
            .map((f) => f.id) ?? []);
    const serial = ++sequence.current;
    inFlight.current = true;
    setBusy(true);
    setMessage("");
    setFailed(false);
    setBlocked(null);
    try {
      const result = await finalizeSchedule(
        captured.id,
        captured.revision,
        ids,
      );
      if (sequence.current !== serial) return;
      switch (result.status) {
        case "success": {
          const value = result.data;
          const validation = value.validation;
          setReview(null);
          setMessage(
            `${captured.name}: ${validation?.warning_count ? "Finalized with acknowledged warnings" : "Finalized"}: ${validation?.error_count ?? 0} errors, ${validation?.warning_count ?? 0} warnings, ${value.open_shift_count} open shifts. ${result.warnings?.map((w) => `${w.message} (${w.code})`).join(" ") ?? ""}`,
          );
          setCelebrate(
            validation !== null &&
              validation.error_count === 0 &&
              validation.warning_count === 0 &&
              validation.complete,
          );
          onChanged(
            value.schedule,
            result.warnings?.some(
              (warning) => warning.code === "history_unavailable",
            ) ?? false,
          );
          break;
        }
        case "no_change":
          setReview(null);
          setMessage(
            `${captured.name} is already final. No changes were needed; its revision is unchanged.`,
          );
          onChanged(result.data.schedule);
          break;
        case "requires_confirmation":
          setReview({ schedule: result.data.schedule, outcome: result.data });
          setMessage(
            "Review the complete current warning list before confirming. Earlier acknowledgements do not authorize changed facts.",
          );
          if (result.data.validation) onValidation?.(result.data.validation);
          break;
        case "blocked":
          setReview(null);
          setFailed(true);
          setMessage(`${result.error.message} (${result.error.code})`);
          if (result.error.code === "finalization_blocked") {
            setBlocked(result.error.context.diagnostics as ValidationResult);
            onValidation?.(
              result.error.context.diagnostics as ValidationResult,
            );
          }
          break;
        case "stale_revision":
          setReview(null);
          setFailed(true);
          setMessage(
            `${result.error.message} (${result.error.code}) Reload this schedule, then review again.`,
          );
          onChanged(captured);
          break;
        case "validation_error":
        case "failure":
          setReview(null);
          setFailed(true);
          setMessage(`${result.error.message} (${result.error.code})`);
          break;
        case "canceled":
          setReview(null);
          setMessage("Finalization canceled.");
          break;
        case "requires_overwrite":
          setReview(null);
          setFailed(true);
          setMessage(
            "Unexpected file confirmation from finalization. Reload the application.",
          );
          break;
        default:
          unreachableResult(result);
      }
    } catch (error) {
      if (sequence.current === serial) {
        setReview(null);
        setFailed(true);
        setMessage(errorText(error));
      }
    } finally {
      inFlight.current = false;
      if (sequence.current === serial) setBusy(false);
    }
  }
  const finalizeButton = schedule.status === "draft" &&
    !schedule.archived_at && (
      <button
        type="button"
        className="btn btn-primary"
        disabled={disabled || busy || review !== null}
        onClick={() => setReview({ schedule, outcome: null })}
      >
        Finalize…
      </button>
    );
  return (
    <div className="finalization-panel">
      {children(finalizeButton || null)}
      {message && (
        <p
          className={`notice ${failed ? "notice-error" : "notice-success"}`}
          role={failed ? "alert" : "status"}
        >
          {message}
        </p>
      )}
      {celebrate && (
        <div className="celebration" aria-hidden="true">
          <span>✦</span>
          <span>✧</span>
          <span>✦</span>
          <span>✧</span>
          <span>✦</span>
        </div>
      )}
      {blocked && (
        <div className="finalize-blocked">
          <h3>Errors cannot be overridden</h3>
          <Findings findings={blocked.findings} />
          {onShowIssues && (
            <p className="cluster">
              <button
                type="button"
                className="btn btn-sm"
                aria-haspopup="dialog"
                onClick={onShowIssues}
              >
                Show issues
              </button>
            </p>
          )}
        </div>
      )}
      {review && (
        <Confirmation
          title={
            review.outcome === null
              ? "Finalize this schedule?"
              : "Acknowledge current warnings and finalize?"
          }
          busy={busy}
          onConfirm={() => void submit()}
          onCancel={() => {
            setReview(null);
            setMessage("Finalization canceled.");
          }}
          confirmLabel={
            review.outcome === null
              ? "Finalize schedule"
              : "Acknowledge and finalize"
          }
          busyLabel="Finalizing…"
        >
          <p>
            <strong>{review.schedule.name}</strong>,{" "}
            <span className="display">
              {displayLocalDate(review.schedule.start_date)} to{" "}
              {displayLocalDate(review.schedule.end_date)}
            </span>
            , revision {review.schedule.revision}
          </p>
          <p>
            Finalization locks editing until you explicitly reopen this
            schedule. It does not mean all slots are filled. The backend checks
            current policy again before saving.
          </p>
          {review.outcome !== null && (
            <>
              <p>
                <strong>
                  {review.outcome.open_shift_count} open shifts,{" "}
                  {review.outcome.validation?.warning_count ?? 0} warnings.
                </strong>{" "}
                Confirm accepts exactly the material facts below, including
                every listed open shift.
              </p>
              <Findings findings={review.outcome.validation?.findings ?? []} />
            </>
          )}
        </Confirmation>
      )}
    </div>
  );
}
