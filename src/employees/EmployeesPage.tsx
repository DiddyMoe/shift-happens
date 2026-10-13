import { useEffect, useRef, useState } from "react";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useRecordMutation } from "../hooks/useRecordMutation";
import { formatHours } from "../lib/duration";
import {
  archiveEmployee,
  createEmployee,
  employeeData,
  employmentLabels,
  getEmployee,
  getEmploymentDefaults,
  listEmployees,
  periodLabels,
  previewEmploymentDefaults,
  resetEmploymentDefaults,
  updateEmployee,
  type Employee,
  type EmployeeData,
  type EmploymentDefaults,
  type EmploymentType,
  type RecordFilter,
  type ResetPreview,
} from "../lib/employees";
import { readCommandResult } from "../lib/ipc";
import { errorText, mutationFeedback } from "../lib/recordFeedback";
import {
  Confirmation,
  LoadError,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { EmployeeForm } from "./EmployeeForm";
import { EmployeeRestrictions } from "./EmployeeRestrictions";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import { assignmentWord, availabilityLabel, minimumDetail } from "./format";
import { LabelSelect } from "./LabelSelect";

const typeFilterLabels = { all: "All types", ...employmentLabels };

/** The value after it has stopped changing for `delay` ms, so typing in the
 * search box runs one query per pause instead of one per keystroke. */
function useSettledValue<T>(value: T, delay: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(value), delay);
    return () => window.clearTimeout(timer);
  }, [value, delay]);
  return settled;
}

/** The minimum for a list row, e.g. "minimum 40 h each workweek". */
function MinimumPhrase({ rule }: { rule: EmploymentDefaults }) {
  if (rule.minimum_period === "none") return <>no minimum</>;
  const period = periodLabels[rule.minimum_period].toLowerCase();
  return rule.minimum_paid_minutes !== null ? (
    <>
      minimum{" "}
      <span className="display">{formatHours(rule.minimum_paid_minutes)}</span>{" "}
      {period}
    </>
  ) : (
    <>
      minimum <span className="display">{rule.minimum_assignment_count}</span>{" "}
      {assignmentWord(rule.minimum_assignment_count)} {period}
    </>
  );
}

function newEmployee(defaults: EmploymentDefaults): EmployeeData {
  return {
    first_name: "",
    last_name: "",
    display_name: "",
    employment_type: "full_time",
    active_status: "active",
    holiday_group: "none",
    can_work_weekends: true,
    can_work_holidays: true,
    can_work_night_shift: false,
    can_work_ortho_or: false,
    preferred_time_of_day: null,
    ...defaults,
    maximum_paid_minutes: null,
    maximum_period: "none",
    weekend_rotation_pattern: "none",
    weekend_rotation_anchor_date: null,
    notes: null,
  };
}

export function Employees() {
  const [filter, setFilter] = useState<RecordFilter>("current");
  const [employmentType, setEmploymentType] = useState<EmploymentType | "all">(
    "all",
  );
  const [search, setSearch] = useState("");
  const searchQuery = useSettledValue(search, 200);
  const list = useCommandQuery(
    JSON.stringify([filter, employmentType, searchQuery]),
    () =>
      listEmployees(
        filter,
        employmentType === "all" ? null : employmentType,
        searchQuery,
      ),
  );
  const [selection, setSelection] = useState<{
    employee: Employee | null;
    draft: EmployeeData;
  } | null>(null);
  // Changes only when a different record is opened, not when one is saved.
  const [formKey, setFormKey] = useState(0);
  const [reading, setReading] = useState(false);
  const mutation = useRecordMutation();
  const { feedback, setFeedback, run } = mutation;
  const busy = mutation.busy || reading;
  const [confirmation, setConfirmation] = useState<
    | { kind: "archive"; employee: Employee }
    | { kind: "reset"; preview: ResetPreview }
    | null
  >(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const focusAfterRender = useFocusAfterRender();

  function open(employee: Employee | null, draft: EmployeeData) {
    setSelection({ employee, draft });
    setFormKey((k) => k + 1);
    setFeedback(null);
  }
  function choose(employee: Employee) {
    const current = selection?.employee;
    // Choosing the open employee again keeps any unsaved edits, unless the
    // list holds a newer saved version.
    if (current?.id === employee.id && employee.revision <= current.revision)
      return;
    open(employee, employeeData(employee));
  }
  function show(employee: Employee) {
    setSelection({ employee, draft: employeeData(employee) });
    list.reload();
  }
  function close() {
    const id = selection?.employee?.id;
    setSelection(null);
    setFeedback(null);
    focusAfterRender(
      () =>
        listRef.current?.querySelector<HTMLElement>(
          `[data-employee="${CSS.escape(id ?? "")}"]`,
        ) ?? addButton.current,
    );
  }
  async function read(action: () => Promise<void>) {
    setReading(true);
    setFeedback(null);
    try {
      await action();
    } catch (error) {
      setFeedback({ message: errorText(error), stale: false, failed: true });
    } finally {
      setReading(false);
    }
  }
  function addEmployee() {
    void read(async () => {
      const defaults = readCommandResult(
        await getEmploymentDefaults("full_time"),
      );
      open(null, newEmployee(defaults));
    });
  }
  function reloadSelected() {
    const employee = selection?.employee;
    if (!employee) return;
    void read(async () => {
      show(readCommandResult(await getEmployee(employee.id)));
      focusAfterRender(() => heading.current, true);
      setFeedback({
        message: "Loaded the latest saved version.",
        stale: false,
        failed: false,
      });
    });
  }
  function previewReset(employee: Employee) {
    void read(async () => {
      const result = await previewEmploymentDefaults(employee);
      if (result.status === "success" || result.status === "no_change")
        setConfirmation({ kind: "reset", preview: result.data });
      else mutationFeedback(result, () => {}, setFeedback, "");
    });
  }
  function save(data: EmployeeData) {
    const employee = selection?.employee ?? null;
    void run(
      () => (employee ? updateEmployee(employee, data) : createEmployee(data)),
      {
        success: "Employee saved.",
        onSuccess: (saved) => {
          show(saved);
          focusAfterRender(() => heading.current, true);
        },
      },
    );
  }
  function archive(employee: Employee) {
    void run(() => archiveEmployee(employee), {
      success: "Employee archived.",
      onSuccess: (saved) => {
        show(saved);
        setConfirmation(null);
        focusAfterRender(() => heading.current);
      },
      onFailure: () => setConfirmation(null),
    });
  }
  function reset(preview: ResetPreview) {
    void run(() => resetEmploymentDefaults(preview), {
      success: `Reset to ${employmentLabels[preview.employment_type]} defaults.`,
      onSuccess: (saved) => {
        show(saved);
        setConfirmation(null);
      },
      onFailure: () => setConfirmation(null),
    });
  }

  return (
    <section aria-labelledby="page-title" className="page">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">Employees</h1>
          <p className="page-lede">
            Who can be scheduled, their hours, and what they can&rsquo;t work.
          </p>
        </div>
        <div className="page-actions">
          <button
            ref={addButton}
            className="btn btn-primary"
            type="button"
            disabled={busy}
            onClick={addEmployee}
          >
            Add employee
          </button>
        </div>
      </header>
      <div className="toolbar">
        <label className="grow">
          Search names
          <input
            type="search"
            maxLength={240}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
        <RecordFilterSelect
          label="Status"
          value={filter}
          onChange={setFilter}
        />
        <LabelSelect
          label="Employment type"
          labels={typeFilterLabels}
          value={employmentType}
          onChange={setEmploymentType}
        />
      </div>
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={reloadSelected}
        reloadLabel="Reload latest employee and discard edits"
      />
      <div className="split">
        <div className="split-list">
          {list.error ? (
            <div className="emp-list-state">
              <LoadError message={list.error} onRetry={list.reload} />
            </div>
          ) : list.data === null ? (
            <div className="emp-list-state">
              <p role="status" className="status-line">
                Loading employees…
              </p>
            </div>
          ) : list.data.length === 0 ? (
            <div className="emp-list-state">
              <p className="muted">No employees match these filters.</p>
            </div>
          ) : (
            <>
              <p className="split-list-header">
                <span>
                  <span className="display">{list.data.length}</span>{" "}
                  {list.data.length === 1 ? "employee" : "employees"}
                </span>
              </p>
              <ul
                ref={listRef}
                className="record-list"
                aria-label="Employees"
                aria-busy={list.loading}
              >
                {list.data.map((employee) => {
                  const selected = selection?.employee?.id === employee.id;
                  return (
                    <li key={employee.id}>
                      <button
                        type="button"
                        disabled={busy}
                        className={`record-item${selected ? " is-selected" : ""}`}
                        aria-current={selected ? "true" : undefined}
                        data-employee={employee.id}
                        onClick={() => choose(employee)}
                      >
                        <span className="record-title">
                          {employee.display_name}
                          <RecordStatusTag record={employee} />
                        </span>
                        <span className="record-meta">
                          {employmentLabels[employee.employment_type]},{" "}
                          <MinimumPhrase rule={employee} />
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </>
          )}
        </div>
        <div className="split-detail">
          {selection ? (
            <>
              <EmployeeForm
                key={formKey}
                initial={selection.draft}
                employee={selection.employee}
                busy={busy}
                headingRef={heading}
                onSave={save}
                onClose={close}
                onArchive={() => {
                  if (selection.employee)
                    setConfirmation({
                      kind: "archive",
                      employee: selection.employee,
                    });
                }}
                onReset={() => {
                  if (selection.employee) previewReset(selection.employee);
                }}
              />
              {selection.employee && (
                <EmployeeRestrictions
                  key={selection.employee.id}
                  employee={selection.employee}
                />
              )}
            </>
          ) : (
            <div className="split-empty">
              <h2>Choose an employee</h2>
              <p>Select an employee to edit, or add one.</p>
            </div>
          )}
        </div>
      </div>
      {confirmation?.kind === "archive" && (
        <Confirmation
          title={`Archive ${confirmation.employee.display_name}?`}
          confirmLabel="Archive"
          busyLabel="Archiving…"
          danger
          busy={busy}
          onConfirm={() => archive(confirmation.employee)}
          onCancel={() => setConfirmation(null)}
        >
          <p>
            {confirmation.employee.display_name} won&rsquo;t be available for
            new schedules. Archiving can&rsquo;t be undone. Past schedules keep
            their copy.
          </p>
        </Confirmation>
      )}
      {confirmation?.kind === "reset" && (
        <Confirmation
          title={`Reset to ${employmentLabels[confirmation.preview.employment_type]} defaults?`}
          confirmLabel="Reset"
          busyLabel="Resetting…"
          busy={busy}
          onConfirm={() => reset(confirmation.preview)}
          onCancel={() => setConfirmation(null)}
        >
          <dl className="facts emp-reset-facts">
            <div>
              <dt>Current minimum</dt>
              <dd>{minimumDetail(confirmation.preview.before)}</dd>
            </div>
            <div>
              <dt>New minimum</dt>
              <dd>{minimumDetail(confirmation.preview.after)}</dd>
            </div>
            <div>
              <dt>Current availability rule</dt>
              <dd>
                {availabilityLabel(
                  confirmation.preview.before.requires_explicit_availability,
                )}
              </dd>
            </div>
            <div>
              <dt>New availability rule</dt>
              <dd>
                {availabilityLabel(
                  confirmation.preview.after.requires_explicit_availability,
                )}
              </dd>
            </div>
          </dl>
          <p>
            Only the minimum and the availability rule change. Unsaved edits to
            this employee are discarded.
          </p>
        </Confirmation>
      )}
    </section>
  );
}
