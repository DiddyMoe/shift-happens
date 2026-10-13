import { useEffect, useRef, useState, type FormEvent } from "react";
import { formatHours } from "../lib/duration";
import { Link } from "react-router";
import { HowItWorks, LoadError } from "../RecordControls";
import { trapDialogFocus } from "../lib/dialogFocus";
import {
  commandOutcome,
  generateSchedule,
  getGenerationReadiness,
  warningLabels,
  type GenerationReadiness,
  type GenerationResult,
  type GenerationWarning,
  type PartialMinimumPeriod,
} from "../lib/generation";
import type { AppError } from "../lib/ipc";
import {
  calendarMonthRange,
  currentLocalCalendarMonth,
  displayLocalDate,
  localDateOrdinal,
  parseLocalDate,
} from "../lib/localDate";
import { errorText } from "../lib/recordFeedback";

function PartialPeriods({
  periods,
}: {
  periods: readonly PartialMinimumPeriod[];
}) {
  if (!periods.length)
    return (
      <p className="gen-check-detail">
        Stored minimums apply for complete periods in this range.
      </p>
    );
  return (
    <details className="gen-check-detail">
      <summary>Some minimum periods extend past this range</summary>
      <p>
        These periods are not fully inside the schedule. Their full minimum
        targets will not be forced into the visible dates. Maximum exceedances
        still block assignments.
      </p>
      <ul className="bullets gen-periods">
        {periods.map((p) => (
          <li key={`${p.period}/${displayLocalDate(p.start_date)}`}>
            {p.period === "week" ? "Workweek" : "Two-week period"}:{" "}
            <span className="display">
              {displayLocalDate(p.start_date)} to{" "}
              {displayLocalDate(p.end_exclusive)}
            </span>{" "}
            (end exclusive), <span className="display">{p.employee_count}</span>{" "}
            employee rule
            {p.employee_count === 1 ? "" : "s"}.
          </li>
        ))}
      </ul>
    </details>
  );
}
function FatalDetails({ error }: { error: AppError }) {
  const c = error.context;
  return (
    <div className="notice notice-error" role="alert">
      <p>
        {error.message} ({error.code})
      </p>
      {c.key && <p>Setting or operational date: {c.key}</p>}
      {c.id && (
        <p>
          Relevant record: <span className="literal">{c.id}</span>
        </p>
      )}
      {c.limit_kind && (
        <p>
          {c.limit_kind.replaceAll("_", " ")}:{" "}
          <span className="display">{c.observed_count}</span> observed; limit{" "}
          <span className="display">{c.limit}</span>.
        </p>
      )}
      {c.dependency_ids && (
        <ul className="bullets">
          {c.dependency_ids.map((id, i) => (
            <li key={id}>{c.dependency_names?.[i] ?? id}</li>
          ))}
        </ul>
      )}
      {c.omitted_source_id_count ? (
        <p>
          <span className="display">{c.omitted_source_id_count}</span> further
          source records omitted.
        </p>
      ) : null}
      {error.code === "two_week_anchor_required" ? (
        <Link className="btn btn-sm" to="/settings">
          Set the two-week payroll anchor in Settings
        </Link>
      ) : error.code === "holiday_collision" ? (
        <Link className="btn btn-sm" to="/holidays">
          Resolve holiday collisions in Holidays
        </Link>
      ) : (
        <div className="cluster">
          <Link className="btn btn-sm" to="/positions-coverage">
            Review coverage
          </Link>
          <Link className="btn btn-sm" to="/employees">
            Review employee rules
          </Link>
          <Link className="btn btn-sm" to="/settings">
            Review settings
          </Link>
        </div>
      )}
    </div>
  );
}
function WarningDetail({ warning }: { warning: GenerationWarning }) {
  const { slot, facts } = warning.context;
  const deficit = facts?.deficit_minutes;
  const excessMinutes = facts?.excess_minutes;
  const excessDays = facts?.excess_days;
  return (
    <li>
      <strong>{warningLabels[warning.code]}</strong>
      {slot && (
        <p>
          <span className="display">{slot.schedule_date}</span>, slot{" "}
          <span className="display">{slot.slot_index}</span>,{" "}
          <span className="display">
            {slot.actual_start.replace("T", " ")} –{" "}
            {slot.actual_end.replace("T", " ")}
          </span>
        </p>
      )}
      {typeof deficit === "number" && (
        <p>
          Rest short by <span className="display">{formatHours(deficit)}</span>.
        </p>
      )}
      {typeof excessMinutes === "number" && (
        <p>
          Over by <span className="display">{formatHours(excessMinutes)}</span>{" "}
          paid.
        </p>
      )}
      {typeof excessMinutes !== "number" && typeof excessDays === "number" && (
        <p>
          Over by <span className="display">{excessDays}</span>{" "}
          {excessDays === 1 ? "day" : "days"}.
        </p>
      )}
      {facts?.exclusion_counts &&
        typeof facts.exclusion_counts === "object" &&
        !Array.isArray(facts.exclusion_counts) && (
          <details>
            <summary>Why candidates were excluded</summary>
            <ul className="bullets">
              {Object.entries(facts.exclusion_counts).map(([code, count]) => (
                <li key={code}>
                  {code.replaceAll("_", " ")}:{" "}
                  <span className="display">{String(count)}</span>
                </li>
              ))}
            </ul>
          </details>
        )}
    </li>
  );
}

/**
 * Generates a complete draft in a modal. On success the workspace opens the
 * new draft underneath while this dialog shows the result; "Open draft"
 * closes it and "Show issues" opens the draft's issues.
 */
export function GenerateDialog({
  onCreated,
  onClose,
}: {
  onCreated: (result: GenerationResult) => void;
  /** `next` is what follows the dialog: the new draft, its issues, or
   * nothing (canceled). */
  onClose: (next: "draft" | "issues" | null) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [month, setMonth] = useState(currentLocalCalendarMonth);
  const [mode, setMode] = useState<"month" | "custom">("month");
  const [name, setName] = useState("");
  const [customStart, setCustomStart] = useState("");
  const [customEnd, setCustomEnd] = useState("");
  const [makeCurrent, setMakeCurrent] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const [readiness, setReadiness] = useState<{
    key: string;
    data: GenerationReadiness | null;
    error: AppError | null;
    message: string;
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const submitting = useRef(false);
  const mounted = useRef(true);
  const outcome = useRef<HTMLDivElement>(null);
  const [fatal, setFatal] = useState<AppError | null>(null);
  const [message, setMessage] = useState("");
  const [result, setResult] = useState<GenerationResult | null>(null);
  const [warningPage, setWarningPage] = useState(0);
  const monthRange = calendarMonthRange(month);
  const start =
    mode === "month" && monthRange.ok
      ? monthRange.value.start_date
      : mode === "custom"
        ? customStart
        : "";
  const end =
    mode === "month" && monthRange.ok
      ? monthRange.value.end_date
      : mode === "custom"
        ? customEnd
        : "";
  const parsedStart = parseLocalDate(start);
  const parsedEnd = parseLocalDate(end);
  const dayCount =
    parsedStart.ok && parsedEnd.ok
      ? localDateOrdinal(parsedEnd.value) -
        localDateOrdinal(parsedStart.value) +
        1
      : 0;
  const validRange = dayCount >= 1 && dayCount <= 370;
  const key = `${start}/${end}/${refresh}`;
  // Generation needs readiness for exactly this range and reload.
  const activeReadiness = readiness?.key === key ? readiness : null;
  const ready = activeReadiness?.data;
  // The last result stays on screen while a recheck runs, dimmed if it is
  // slow and inert so its fix links match what is shown.
  const shownReadiness = validRange ? readiness : null;
  const readinessStale =
    shownReadiness !== null && shownReadiness !== activeReadiness;
  const shownReady = shownReadiness?.data;
  useEffect(() => {
    const modal = dialog.current;
    modal?.showModal();
    return () => modal?.close();
  }, []);
  // Bring a failure into view; it renders below the form.
  useEffect(() => {
    if (!fatal && !message) return;
    const reduce = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    outcome.current?.scrollIntoView({
      block: "nearest",
      behavior: reduce ? "auto" : "smooth",
    });
  }, [fatal, message]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    let canceled = false;
    if (!validRange) return;
    // Readiness is a read-only preview. Generation repeats it in its committing view.
    const timer = window.setTimeout(() => {
      void getGenerationReadiness(start, end)
        .then((response) => {
          if (!canceled) {
            const outcome = commandOutcome(response);
            setReadiness({ key, ...outcome, message: "" });
          }
        })
        .catch((error: unknown) => {
          if (!canceled)
            setReadiness({
              key,
              data: null,
              error: null,
              message: errorText(error),
            });
        });
    }, 250);
    return () => {
      canceled = true;
      window.clearTimeout(timer);
    };
  }, [start, end, key, validRange]);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (submitting.current || !ready || !name.trim() || !validRange) return;
    submitting.current = true;
    setBusy(true);
    setFatal(null);
    setMessage("");
    setResult(null);
    setWarningPage(0);
    try {
      const outcome = commandOutcome(
        await generateSchedule({
          name,
          start_date: start,
          end_date: end,
          make_current: makeCurrent,
          expected_current_setting_revision: makeCurrent
            ? ready.current_setting_revision
            : null,
        }),
      );
      if (!mounted.current) return;
      if (outcome.error) setFatal(outcome.error);
      else {
        setResult(outcome.data);
        onCreated(outcome.data);
      }
    } catch (error) {
      if (mounted.current) setMessage(errorText(error));
    } finally {
      submitting.current = false;
      if (mounted.current) {
        setBusy(false);
        setRefresh((v) => v + 1);
      }
    }
  }
  const details =
    result?.warnings.filter(
      (w) => w.code !== "generation_warning_output_limit_exceeded",
    ) ?? [];
  const pageCount = Math.ceil(details.length / 50);
  return (
    <dialog
      ref={dialog}
      className="dialog gen-dialog"
      aria-labelledby="gen-title"
      onKeyDown={trapDialogFocus}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose(result ? "draft" : null);
      }}
    >
      <h2 id="gen-title">{result ? "Draft created" : "Generate a draft"}</h2>
      {result ? (
        <section className="gen-result" aria-label="Generation result">
          <div className="notice notice-success" role="status">
            <p>
              <strong>{result.schedule.name}</strong> was saved and is open
              behind this dialog.
              {result.made_current
                ? " It is now your current schedule."
                : " Your current schedule selection is unchanged."}
            </p>
            <dl className="counts">
              <div>
                <dt>slots</dt>
                <dd>{result.assignment_count}</dd>
              </div>
              <div>
                <dt>assigned</dt>
                <dd>{result.assigned_count}</dd>
              </div>
              <div>
                <dt>open</dt>
                <dd>{result.open_count}</dd>
              </div>
              <div>
                <dt>warnings</dt>
                <dd>{result.total_warning_count}</dd>
              </div>
            </dl>
          </div>
          {result.details_truncated && (
            <p className="notice notice-error" role="alert">
              Warning details were truncated at the 10,000-item response limit.
              Counts include all warnings; assignment choices were not changed.
              Issues shows authoritative findings under current policy.
            </p>
          )}
          <p className="fine-print">
            Generation warnings describe choices made during the greedy run.
            Issues shows authoritative findings under current policy.
          </p>
          {result.total_warning_count > 0 && (
            <div className="gen-warnings">
              <h3>Warning summary</h3>
              <ul className="ruled-list gen-warning-counts">
                {Object.entries(result.warning_counts).map(([code, count]) => (
                  <li key={code}>
                    <span>
                      {warningLabels[code as keyof typeof warningLabels]}
                    </span>{" "}
                    <span className="display">{count}</span>
                  </li>
                ))}
              </ul>
              <details>
                <summary>Retained warning details ({details.length})</summary>
                <ul className="finding-list is-warning">
                  {details
                    .slice(warningPage * 50, (warningPage + 1) * 50)
                    .map((warning) => (
                      <WarningDetail
                        key={warning.warning_id}
                        warning={warning}
                      />
                    ))}
                </ul>
                {pageCount > 1 && (
                  <div className="cluster gen-pager">
                    <button
                      type="button"
                      className="btn btn-sm"
                      disabled={warningPage === 0}
                      onClick={() => setWarningPage((v) => v - 1)}
                    >
                      Previous
                    </button>
                    <span className="muted">
                      Page <span className="display">{warningPage + 1}</span> of{" "}
                      <span className="display">{pageCount}</span>
                    </span>
                    <button
                      type="button"
                      className="btn btn-sm"
                      disabled={warningPage + 1 >= pageCount}
                      onClick={() => setWarningPage((v) => v + 1)}
                    >
                      Next
                    </button>
                  </div>
                )}
              </details>
            </div>
          )}
        </section>
      ) : (
        <form
          id="gen-form"
          className="gen-layout"
          onSubmit={(event) => void submit(event)}
          aria-busy={busy}
        >
          <div className="gen-form">
            <fieldset disabled={busy}>
              <legend className="visually-hidden">Draft details</legend>
              <div className="stack gen-fields">
                <label>
                  Name
                  <input
                    required
                    autoFocus
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    placeholder="Name this draft"
                  />
                </label>
                <div className="gen-mode">
                  <span className="gen-mode-label" id="gen-mode-label">
                    Dates
                  </span>
                  <div
                    className="segmented"
                    role="group"
                    aria-labelledby="gen-mode-label"
                  >
                    {(
                      [
                        ["month", "Calendar month"],
                        ["custom", "Custom range"],
                      ] as const
                    ).map(([value, text]) => (
                      <button
                        key={value}
                        type="button"
                        aria-pressed={mode === value}
                        onClick={() => {
                          setMode(value);
                          // A blank custom range starts from the chosen month,
                          // so it only needs adjusting rather than filling in.
                          if (
                            value === "custom" &&
                            !customStart &&
                            !customEnd &&
                            monthRange.ok
                          ) {
                            setCustomStart(monthRange.value.start_date);
                            setCustomEnd(monthRange.value.end_date);
                          }
                        }}
                      >
                        {text}
                      </button>
                    ))}
                  </div>
                </div>
                {mode === "month" ? (
                  <label className="gen-month">
                    Calendar month
                    <input
                      type="month"
                      min="0001-01"
                      max="9999-12"
                      required
                      value={month}
                      onChange={(event) => setMonth(event.target.value)}
                    />
                  </label>
                ) : (
                  <div className="fields-2">
                    <label>
                      Start date
                      <input
                        type="date"
                        min="0001-01-01"
                        max="9999-12-31"
                        required
                        value={customStart}
                        onChange={(event) => setCustomStart(event.target.value)}
                      />
                    </label>
                    <label>
                      End date
                      <input
                        type="date"
                        min="0001-01-01"
                        max="9999-12-31"
                        required
                        value={customEnd}
                        onChange={(event) => setCustomEnd(event.target.value)}
                      />
                    </label>
                  </div>
                )}
                <p className="gen-range" aria-live="polite">
                  Inclusive range:{" "}
                  <strong className="display">
                    {start || "Choose a start"} – {end || "Choose an end"}
                  </strong>
                  {validRange ? (
                    <>
                      , <strong className="display">{dayCount}</strong>{" "}
                      {dayCount === 1 ? "day" : "days"}
                    </>
                  ) : (
                    ". Choose dates spanning 1–370 days."
                  )}
                </p>
                <label>
                  <input
                    type="checkbox"
                    checked={makeCurrent}
                    onChange={(event) => setMakeCurrent(event.target.checked)}
                  />
                  Make this my current schedule
                </label>
              </div>
            </fieldset>
            {busy && (
              <p className="status-line" role="status">
                Checking coverage, choosing employees and saving the complete
                schedule. Keep this window open until the result appears.
              </p>
            )}
            <div ref={outcome} className="gen-outcome">
              {fatal && <FatalDetails error={fatal} />}
              {message && (
                <p className="notice notice-error" role="alert">
                  {message}
                </p>
              )}
            </div>
          </div>
          <div className="gen-readiness panel-quiet" aria-live="polite">
            <div className="gen-readiness-header">
              <h3>Readiness</h3>
            </div>
            {validRange && !shownReadiness && (
              <p className="status-line" role="status">
                Checking settings, coverage, holidays and employee rules…
              </p>
            )}
            {!validRange && (
              <p className="muted">Choose a valid range to check readiness.</p>
            )}
            <div
              className={readinessStale ? "is-refreshing" : undefined}
              inert={readinessStale}
              aria-busy={readinessStale}
            >
              {shownReadiness?.error && (
                <FatalDetails error={shownReadiness.error} />
              )}
              {shownReadiness?.message && (
                <LoadError
                  message={shownReadiness.message}
                  onRetry={() => setRefresh((v) => v + 1)}
                />
              )}
              {shownReady && (
                <ul className="gen-checklist">
                  <li>
                    <div className="gen-check-head">
                      <h4>Coverage</h4>
                      {shownReady.planned_slot_count > 0 ? (
                        <span className="tag tag-ok">Ready</span>
                      ) : (
                        <span className="tag tag-warn">No slots</span>
                      )}
                    </div>
                    <p className="gen-check-detail">
                      <span className="display">
                        {shownReady.planned_slot_count}
                      </span>{" "}
                      required slots.
                    </p>
                    <Link className="gen-check-link" to="/positions-coverage">
                      Positions and coverage
                    </Link>
                  </li>
                  <li>
                    <div className="gen-check-head">
                      <h4>Employees</h4>
                      {shownReady.active_employee_count > 0 ? (
                        <span className="tag tag-ok">Ready</span>
                      ) : (
                        <span className="tag tag-warn">None active</span>
                      )}
                    </div>
                    <p className="gen-check-detail">
                      <span className="display">
                        {shownReady.active_employee_count}
                      </span>{" "}
                      active employees.
                      {shownReady.active_employee_count === 0 &&
                        " All slots will be open until employees are added."}
                    </p>
                    <Link className="gen-check-link" to="/employees">
                      Employees
                    </Link>
                  </li>
                  {shownReady.two_week_anchor_required && (
                    <li>
                      <div className="gen-check-head">
                        <h4>Two-week payroll anchor</h4>
                        {shownReady.two_week_anchor_date ? (
                          <span className="tag tag-ok">Set</span>
                        ) : (
                          <span className="tag tag-warn">Not set</span>
                        )}
                      </div>
                      <p className="gen-check-detail">
                        Anchor date:{" "}
                        <span className="display">
                          {shownReady.two_week_anchor_date}
                        </span>
                        .
                      </p>
                      <Link className="gen-check-link" to="/settings">
                        Settings
                      </Link>
                    </li>
                  )}
                  <li>
                    <div className="gen-check-head">
                      <h4>Minimum periods</h4>
                      {shownReady.partial_minimum_periods.length > 0 ? (
                        <span className="tag tag-warn">Partial</span>
                      ) : (
                        <span className="tag tag-ok">Complete</span>
                      )}
                    </div>
                    <PartialPeriods
                      periods={shownReady.partial_minimum_periods}
                    />
                  </li>
                </ul>
              )}
            </div>
          </div>
        </form>
      )}
      <div className="gen-scope">
        <HowItWorks label="What a generated draft can and cannot tell you">
          <p>
            Minimum, maximum, and overtime totals use only this schedule.
            Alternate schedules and external payroll systems are not included.
            Minimum targets can exceed the work available in coverage;
            generation does not guarantee every target will be met.
          </p>
          <p>
            Overlap, rest, recovery, and consecutive-day checks cannot prove
            facts about assignments immediately outside this range, in another
            schedule, or in another system. Scheduling uses workplace wall-clock
            minutes without daylight-saving adjustment.
          </p>
          <p>
            An open slot means no eligible employee remained at that greedy
            step. A different assignment order may allow a fully staffed
            arrangement.
          </p>
          <p>
            Requested time off and preferences are soft priorities. When a soft
            conflict cannot be avoided, the best eligible candidate is assigned
            with warnings.
          </p>
        </HowItWorks>
      </div>
      <div className="dialog-actions">
        {result ? (
          <>
            <button
              type="button"
              className="btn"
              aria-haspopup="dialog"
              onClick={() => onClose("issues")}
            >
              Show issues
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => onClose("draft")}
            >
              Open draft
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="btn"
              disabled={busy}
              onClick={() => onClose(null)}
            >
              Cancel
            </button>
            <button
              type="submit"
              form="gen-form"
              className="btn btn-primary"
              disabled={busy || !ready || !name.trim() || !validRange}
            >
              {busy ? "Generating…" : "Generate draft"}
            </button>
          </>
        )}
      </div>
    </dialog>
  );
}
