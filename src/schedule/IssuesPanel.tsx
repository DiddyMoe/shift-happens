import { useState, type RefObject } from "react";
import { HowItWorks, LoadError } from "../RecordControls";
import { trapDialogFocus } from "../lib/dialogFocus";
import { factEntries } from "../lib/findings";
import { displayLocalDate } from "../lib/localDate";
import type { Finding, Severity, ValidationResult } from "../lib/validation";
import type { IssuesStatus } from "./useIssues";

const clock = (value: Date) =>
  `${String(value.getHours()).padStart(2, "0")}:${String(value.getMinutes()).padStart(2, "0")}`;

/**
 * The plan's errors and warnings in a side panel, grouped by severity, then by
 * operational date. Opening a finding's shift closes the panel and shows that
 * shift's sheet.
 */
export function IssuesPanel({
  dialogRef,
  result,
  status,
  error,
  checkedAt,
  employeeNames,
  onRun,
  onOpenAssignment,
  onClose,
}: {
  dialogRef: RefObject<HTMLDialogElement | null>;
  result: ValidationResult | null;
  status: IssuesStatus;
  error: string;
  checkedAt: Date | null;
  /** Display names of the plan's employees, by id. */
  employeeNames: ReadonlyMap<string, string>;
  onRun: () => void;
  /** `returnTo` names the control to focus when the panel opens again. */
  onOpenAssignment: (returnTo: string, assignmentId: string) => void;
  onClose: () => void;
}) {
  const [severity, setSeverity] = useState<"all" | Severity>("all");
  const [code, setCode] = useState("");
  const [employee, setEmployee] = useState("");
  const employeeIds = [
    ...new Set(
      result?.findings.flatMap((f) => (f.employee_id ? [f.employee_id] : [])) ??
        [],
    ),
  ].sort((a, b) =>
    (employeeNames.get(a) ?? a).localeCompare(employeeNames.get(b) ?? b),
  );
  const filtered =
    result?.findings.filter(
      (f) =>
        (severity === "all" || f.severity === severity) &&
        (!code || f.code === code) &&
        (!employee || f.employee_id === employee),
    ) ?? [];
  // Preserve the backend's canonical order within severity/date groups.
  const groups = new Map<
    string,
    { severity: Severity; date: string | null; findings: Finding[] }
  >();
  for (const finding of filtered) {
    const key = `${finding.severity}/${finding.operational_date ?? ""}`;
    const group = groups.get(key) ?? {
      severity: finding.severity,
      date: finding.operational_date,
      findings: [],
    };
    group.findings.push(finding);
    groups.set(key, group);
  }
  const sections = (["error", "warning"] as const)
    .map((tone) => ({
      tone,
      groups: [...groups.values()].filter((g) => g.severity === tone),
    }))
    .filter((section) => section.groups.length > 0);
  const openShiftCount =
    result?.findings.filter((f) => f.code === "open_shift").length ?? 0;
  return (
    <dialog
      ref={dialogRef}
      className="sheet issues-sheet"
      aria-labelledby="issues-title"
      onKeyDown={trapDialogFocus}
      onClose={onClose}
    >
      <div className="sheet-header">
        <div>
          <h2 id="issues-title" tabIndex={-1}>
            Issues
          </h2>
          <p className="iss-checked">
            {status === "checking"
              ? "Checking current rules…"
              : checkedAt && result
                ? `Last checked ${clock(checkedAt)}`
                : "Not checked yet"}
          </p>
        </div>
        <button
          type="button"
          className="btn btn-quiet"
          onClick={() => dialogRef.current?.close()}
        >
          Close
        </button>
      </div>
      <div className="sheet-body">
        {status === "failed" && <LoadError message={error} onRetry={onRun} />}
        {status === "idle" && (
          <div className="iss-idle">
            <p className="muted">
              This schedule is large, so it is checked only when you ask.
            </p>
            <button type="button" className="btn btn-primary" onClick={onRun}>
              Check now
            </button>
          </div>
        )}
        {status === "checking" && (
          <p className="status-line" role="status">
            Reading the schedule and current rules…
          </p>
        )}
        {result && (
          <>
            <div className="iss-summary" role="status">
              <dl className="counts">
                <div className={result.error_count > 0 ? "iss-has-errors" : ""}>
                  <dt>{result.error_count === 1 ? "error" : "errors"}</dt>
                  <dd>{result.error_count}</dd>
                </div>
                <div
                  className={result.warning_count > 0 ? "iss-has-warnings" : ""}
                >
                  <dt>{result.warning_count === 1 ? "warning" : "warnings"}</dt>
                  <dd>{result.warning_count}</dd>
                </div>
                <div>
                  <dt>{openShiftCount === 1 ? "open shift" : "open shifts"}</dt>
                  <dd>{openShiftCount}</dd>
                </div>
              </dl>
              {!result.complete && (
                <p className="notice notice-error">
                  The check is incomplete, so finalizing is blocked.
                </p>
              )}
              {result.truncated && (
                <p className="fine-print">
                  Counts describe the findings kept in this list only.
                </p>
              )}
            </div>
            {result.findings.length > 0 && (
              <div className="iss-filters">
                <label>
                  Severity
                  <select
                    value={severity}
                    onChange={(e) =>
                      setSeverity(e.target.value as typeof severity)
                    }
                  >
                    <option value="all">All</option>
                    <option value="error">Errors</option>
                    <option value="warning">Warnings</option>
                  </select>
                </label>
                <label>
                  Finding
                  <select
                    value={code}
                    onChange={(e) => setCode(e.target.value)}
                  >
                    <option value="">All findings</option>
                    {result.codes
                      .filter((c) =>
                        result.findings.some((f) => f.code === c.code),
                      )
                      .map((c) => (
                        <option key={c.code} value={c.code}>
                          {c.code.replaceAll("_", " ")}
                        </option>
                      ))}
                  </select>
                </label>
                {employeeIds.length > 0 && (
                  <label>
                    Employee
                    <select
                      value={employee}
                      onChange={(e) => setEmployee(e.target.value)}
                    >
                      <option value="">All employees</option>
                      {employeeIds.map((id) => (
                        <option key={id} value={id}>
                          {employeeNames.get(id) ?? id}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
              </div>
            )}
            {result.complete && result.findings.length === 0 && (
              <p className="notice notice-success">
                No errors or warnings under the current rules.
              </p>
            )}
            {result.findings.length > 0 && filtered.length === 0 && (
              <p className="status-line">No findings match these filters.</p>
            )}
            {sections.map(({ tone, groups: toneGroups }) => (
              <section
                key={tone}
                className="iss-section"
                aria-labelledby={`iss-${tone}-title`}
              >
                <h3 id={`iss-${tone}-title`}>
                  {tone === "error" ? "Errors" : "Warnings"}
                </h3>
                {toneGroups.map((group) => (
                  <div key={group.date ?? ""} className="iss-day">
                    <h4>
                      {group.date ? (
                        <span className="display">
                          {displayLocalDate(group.date)}
                        </span>
                      ) : (
                        "Schedule and settings"
                      )}
                    </h4>
                    <ul
                      className={`finding-list ${tone === "error" ? "is-blocker" : "is-warning"}`}
                    >
                      {group.findings.map((f) => (
                        <li key={f.id}>
                          <strong>{f.message}</strong>
                          {f.actual_datetime && (
                            <p className="display iss-when">
                              {f.actual_datetime.replace("T", " ")}
                            </p>
                          )}
                          {(f.assignment_id || f.counterpart_assignment_id) && (
                            <p className="iss-links">
                              {f.assignment_id && (
                                <button
                                  type="button"
                                  className="btn-link"
                                  data-finding-id={f.id}
                                  onClick={() =>
                                    f.assignment_id &&
                                    onOpenAssignment(f.id, f.assignment_id)
                                  }
                                >
                                  Open shift
                                </button>
                              )}
                              {f.counterpart_assignment_id && (
                                <button
                                  type="button"
                                  className="btn-link"
                                  data-finding-id={`${f.id}:related`}
                                  onClick={() =>
                                    f.counterpart_assignment_id &&
                                    onOpenAssignment(
                                      `${f.id}:related`,
                                      f.counterpart_assignment_id,
                                    )
                                  }
                                >
                                  Open related shift
                                </button>
                              )}
                            </p>
                          )}
                          <details>
                            <summary>Technical context</summary>
                            <dl className="facts">
                              <div>
                                <dt>Rule</dt>
                                <dd className="literal">{f.code}</dd>
                              </div>
                              {f.employee_id && (
                                <div>
                                  <dt>Employee</dt>
                                  <dd>
                                    {employeeNames.get(f.employee_id) ??
                                      "Unknown employee"}
                                  </dd>
                                </div>
                              )}
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
                  </div>
                ))}
              </section>
            ))}
            <details className="iss-reference">
              <summary>Finding reference</summary>
              <ul className="ruled-list">
                {result.codes.map((c) => (
                  <li key={c.code}>
                    <span
                      className={`tag ${c.severity === "error" ? "tag-danger" : "tag-warn"}`}
                    >
                      {c.severity === "error" ? "Error" : "Warning"}
                    </span>{" "}
                    <strong>{c.code.replaceAll("_", " ")}</strong>
                    <p className="iss-code-message">{c.message}</p>
                  </li>
                ))}
              </ul>
            </details>
          </>
        )}
        <HowItWorks label="What the check can and cannot prove">
          <p>
            Checking never changes saved data. Policy changes can change
            findings without rewriting the schedule. Workload uses only this
            plan; other plans and outside systems are not combined.
          </p>
          <p>
            Clean overlap, rest, recovery and consecutive-day results do not
            prove facts beyond this plan&rsquo;s dates. Minimums apply only to
            complete covered periods; exceedances can be conclusive in partial
            periods. Paid time is scheduled clock time, without daylight-saving
            adjustment.
          </p>
        </HowItWorks>
      </div>
    </dialog>
  );
}
