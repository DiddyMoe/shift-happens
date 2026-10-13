import { useRef, useState } from "react";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import { Link, useSearchParams } from "react-router";
import {
  Confirmation,
  LoadError,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { RecordStatusTag } from "./RecordStatusTag";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useRecordMutation } from "../hooks/useRecordMutation";
import {
  listEmployees,
  type ActiveStatus,
  type Employee,
  type RecordFilter,
} from "../lib/employees";
import type { CommandResult } from "../lib/ipc";
import { colorStyle } from "../lib/magnetColor";
import { useRevealOnMount } from "../lib/reveal";

/** A record an employee can be kept off, such as a shift template. */
export interface RestrictionTargetRecord {
  readonly id: string;
  readonly name: string;
  readonly active_status: ActiveStatus;
  readonly archived_at: string | null;
}
export interface RestrictionRecord {
  readonly id: string;
  readonly active_status: ActiveStatus;
  readonly archived_at: string | null;
  readonly notes: string | null;
}
/** The editable part of a cannot-work restriction. */
export interface RestrictionValues {
  readonly targetId: string;
  readonly active_status: ActiveStatus;
  readonly notes: string | null;
}
/** What a restrictions panel manages and the commands it uses. Define it at
 * module level so its functions keep one identity. */
export interface RestrictionTarget<
  T extends RestrictionTargetRecord,
  R extends RestrictionRecord,
> {
  /** What the employee is kept off, in lower case: "shift template". */
  readonly noun: string;
  /** The section's id, so other screens can link to it. */
  readonly sectionId: string;
  readonly listTargets: () => Promise<CommandResult<readonly T[]>>;
  readonly list: (
    employeeId: string,
    filter: RecordFilter,
  ) => Promise<CommandResult<readonly R[]>>;
  readonly create: (
    employeeId: string,
    values: RestrictionValues,
  ) => Promise<CommandResult<R>>;
  readonly update: (
    row: R,
    values: RestrictionValues,
  ) => Promise<CommandResult<R>>;
  readonly archive: (row: R) => Promise<CommandResult<R>>;
  readonly targetOf: (row: R) => string;
  /** The board color shown beside a target's name, for targets that have one. */
  readonly swatch?: (target: T) => string | null;
}

type Mutation = ReturnType<typeof useRecordMutation>;

const isActive = (row: {
  active_status: ActiveStatus;
  archived_at: string | null;
}) => row.active_status === "active" && row.archived_at === null;
const statusSuffix = (row: {
  active_status: ActiveStatus;
  archived_at: string | null;
}) =>
  row.archived_at
    ? " (archived)"
    : row.active_status === "inactive"
      ? " (inactive)"
      : "";
const capitalize = (text: string) =>
  text.charAt(0).toUpperCase() + text.slice(1);
const notesPlaceholder =
  "Scheduling notes only — no patient or medical details.";

/**
 * Cannot-work restrictions for one employee at a time. The employee comes
 * from the `?employee=` search parameter, so other screens can link here.
 */
export function RestrictionsPanel<
  T extends RestrictionTargetRecord,
  R extends RestrictionRecord,
>({
  target,
  sourceRefresh,
}: {
  target: RestrictionTarget<T, R>;
  /** Changes when the targets were edited on this screen, to reload them. */
  sourceRefresh: number;
}) {
  const [params, setParams] = useSearchParams();
  const employeeId = params.get("employee") ?? "";
  const employees = useCommandQuery("employees", () =>
    listEmployees("all", null, ""),
  );
  const targets = useCommandQuery(`targets#${sourceRefresh}`, () =>
    target.listTargets(),
  );
  const mutation = useRecordMutation();
  const error = employees.error || targets.error;
  const employee = employees.data?.find((e) => e.id === employeeId);
  const titleId = `${target.sectionId}-title`;
  return (
    <section
      id={target.sectionId}
      tabIndex={-1}
      className="section rule-top"
      aria-labelledby={titleId}
    >
      <header className="section-header">
        <div>
          <h2 id={titleId}>Employee {target.noun} restrictions</h2>
          <p className="section-lede">
            Keep an employee from being assigned to a {target.noun}.
          </p>
        </div>
      </header>
      <div className="toolbar">
        <label className="grow">
          Employee
          <select
            value={employeeId}
            disabled={!employees.data || mutation.busy}
            onChange={(e) => {
              mutation.setFeedback(null);
              setParams(e.target.value ? { employee: e.target.value } : {});
            }}
          >
            <option value="">Choose an employee</option>
            {employeeId && employees.data && !employee && (
              <option value={employeeId}>Employee not found</option>
            )}
            {employees.data?.map((e) => (
              <option key={e.id} value={e.id}>
                {e.display_name}
                {statusSuffix(e)}
              </option>
            ))}
          </select>
        </label>
      </div>
      {error ? (
        <LoadError
          message={error}
          onRetry={() => {
            employees.reload();
            targets.reload();
          }}
        />
      ) : !employees.data || !targets.data ? (
        <p className="status-line" role="status">
          Loading employees and {target.noun}s…
        </p>
      ) : employees.data.length === 0 ? (
        <div className="empty">
          <p>
            Add an employee in <Link to="/employees">Employees</Link> to manage{" "}
            {target.noun} restrictions.
          </p>
        </div>
      ) : employeeId && !employee ? (
        <div className="notice notice-error" role="alert">
          <p>That employee wasn&rsquo;t found. Choose another employee.</p>
        </div>
      ) : !employee ? (
        <div className="empty">
          <p>Choose an employee to view or edit restrictions.</p>
        </div>
      ) : (
        <RestrictionManager
          key={employee.id}
          target={target}
          employee={employee}
          targets={targets.data}
          mutation={mutation}
        />
      )}
    </section>
  );
}

function RestrictionManager<
  T extends RestrictionTargetRecord,
  R extends RestrictionRecord,
>({
  target,
  employee,
  targets,
  mutation,
}: {
  target: RestrictionTarget<T, R>;
  employee: Employee;
  targets: readonly T[];
  mutation: Mutation;
}) {
  const { busy, feedback, setFeedback, run } = mutation;
  const [filter, setFilter] = useState<RecordFilter>("current");
  const [version, setVersion] = useState(0);
  const rows = useCommandQuery(
    JSON.stringify([employee.id, filter, version]),
    () => target.list(employee.id, filter),
  );
  const [draft, setDraft] = useState<{
    row: R | null;
    values: RestrictionValues;
  } | null>(null);
  // Each opened draft gets a fresh form, even for the same row.
  const [opened, setOpened] = useState(0);
  const [archive, setArchive] = useState<R | null>(null);
  // Focus returns to the heading once a form or dialog closes, instead of
  // falling to the page when the focused control disappears.
  const heading = useRef<HTMLHeadingElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const focusHeading = () => focusAfterRender(() => heading.current);
  const employeeActive = isActive(employee);
  const activeTargets = targets.filter(isActive);
  const findTarget = (id: string) => targets.find((t) => t.id === id);
  const targetName = (id: string) =>
    findTarget(id)?.name ?? `Unavailable ${target.noun}`;
  function inEffect(row: R) {
    const linked = findTarget(target.targetOf(row));
    if (!isActive(row)) return "No";
    if (!employeeActive) return "No — employee not active";
    if (!linked || !isActive(linked)) return `No — ${target.noun} not active`;
    return "Yes";
  }
  function open(row: R | null, values: RestrictionValues) {
    setDraft({ row, values });
    setOpened((v) => v + 1);
    setFeedback(null);
  }
  function changed() {
    setDraft(null);
    setVersion((v) => v + 1);
  }
  async function save(values: RestrictionValues) {
    const row = draft?.row ?? null;
    await run(
      () =>
        row ? target.update(row, values) : target.create(employee.id, values),
      { success: "Restriction saved.", onSuccess: changed },
    );
    focusHeading();
  }
  async function confirmArchive() {
    if (!archive) return;
    const row = archive;
    const result = await run(() => target.archive(row), {
      success: "Restriction archived.",
      onSuccess: changed,
    });
    setArchive(null);
    if (result?.status === "success" || result?.status === "no_change")
      focusHeading();
  }
  const Noun = capitalize(target.noun);
  return (
    <div className="restrictions">
      <h3 ref={heading} tabIndex={-1} className="restrictions-title">
        Restrictions for {employee.display_name}
      </h3>
      {!employeeActive && (
        <div className="notice notice-warn">
          <p>
            This employee is inactive or archived. You can make a restriction
            inactive or archive it. Adding one, changing its {target.noun}, or
            making it active again needs an active employee.
          </p>
        </div>
      )}
      <div className="toolbar">
        <RecordFilterSelect
          label="Restriction status"
          value={filter}
          disabled={busy}
          onChange={setFilter}
        />
        <div className="toolbar-end">
          <button
            className="btn"
            type="button"
            disabled={busy || !employeeActive || activeTargets.length === 0}
            onClick={() => {
              const first = activeTargets[0];
              if (first)
                open(null, {
                  targetId: first.id,
                  active_status: "active",
                  notes: null,
                });
            }}
          >
            Add restriction
          </button>
        </div>
      </div>
      {activeTargets.length === 0 && (
        <p className="fine-print restrictions-hint">
          Make a {target.noun} active before adding a restriction.
        </p>
      )}
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={() => {
          setDraft(null);
          setFeedback(null);
          setVersion((v) => v + 1);
        }}
        reloadLabel="Reload restrictions and discard edits"
      />
      {rows.error ? (
        <LoadError message={rows.error} onRetry={rows.reload} />
      ) : rows.data === null ? (
        <p className="status-line" role="status">
          Loading restrictions…
        </p>
      ) : rows.data.length === 0 ? (
        <div className="empty">
          <p>No restrictions match this filter.</p>
        </div>
      ) : (
        <div
          className={`table-wrap${rows.loading ? " is-refreshing" : ""}`}
          aria-busy={rows.loading}
          tabIndex={0}
          role="region"
          aria-label={`${Noun} restrictions for ${employee.display_name}`}
        >
          <table className="data-table restrictions-table">
            <thead>
              <tr>
                <th scope="col">Restriction</th>
                <th scope="col">Status</th>
                <th scope="col">In effect</th>
                <th scope="col">Notes</th>
                <th scope="col" className="actions">
                  <span className="visually-hidden">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.data.map((r) => {
                const targetId = target.targetOf(r);
                const linked = findTarget(targetId);
                return (
                  <tr key={r.id}>
                    <td>
                      <span className="restriction-name">
                        {linked && target.swatch && (
                          <span
                            className="swatch tpl-chip"
                            style={colorStyle(
                              "--swatch",
                              target.swatch(linked),
                            )}
                            aria-hidden="true"
                          />
                        )}
                        <span>
                          Cannot work <strong>{targetName(targetId)}</strong>
                        </span>
                      </span>
                    </td>
                    <td>
                      {isActive(r) ? "Active" : <RecordStatusTag record={r} />}
                    </td>
                    <td>{inEffect(r)}</td>
                    <td>
                      {r.notes && <span className="prewrap">{r.notes}</span>}
                    </td>
                    <td className="actions">
                      {!r.archived_at && (
                        <>
                          <button
                            className="btn btn-sm"
                            type="button"
                            disabled={busy}
                            onClick={() =>
                              open(r, {
                                targetId,
                                active_status: r.active_status,
                                notes: r.notes,
                              })
                            }
                          >
                            Edit
                          </button>
                          <button
                            className="btn btn-sm btn-danger"
                            type="button"
                            disabled={busy}
                            onClick={() => setArchive(r)}
                          >
                            Archive
                          </button>
                        </>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {draft && (
        <RestrictionForm
          key={opened}
          label={Noun}
          isNew={draft.row === null}
          initial={draft.values}
          targets={targets}
          employeeActive={employeeActive}
          busy={busy}
          onSave={(values) => {
            void save(values);
          }}
          onCancel={() => {
            setDraft(null);
            focusHeading();
          }}
        />
      )}
      {archive && (
        <Confirmation
          title="Archive restriction?"
          confirmLabel="Archive restriction"
          busyLabel="Archiving…"
          danger
          busy={busy}
          onConfirm={() => {
            void confirmArchive();
          }}
          onCancel={() => setArchive(null)}
        >
          <p>
            {employee.display_name} will no longer be kept off{" "}
            {targetName(target.targetOf(archive))}. Archiving can&rsquo;t be
            undone.
          </p>
        </Confirmation>
      )}
    </div>
  );
}

function RestrictionForm<T extends RestrictionTargetRecord>({
  label,
  isNew,
  initial,
  targets,
  employeeActive,
  busy,
  onSave,
  onCancel,
}: {
  /** Names the target field: "Shift template". */
  label: string;
  isNew: boolean;
  initial: RestrictionValues;
  targets: readonly T[];
  employeeActive: boolean;
  busy: boolean;
  onSave: (values: RestrictionValues) => void;
  onCancel: () => void;
}) {
  const [values, setValues] = useState(initial);
  // The saved target stays listed even when it is no longer active.
  const options = targets.filter(
    (t) => t.id === initial.targetId || isActive(t),
  );
  const root = useRef<HTMLFormElement>(null);
  useRevealOnMount(root);
  return (
    <form
      ref={root}
      className="panel restriction-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(values);
      }}
    >
      <fieldset className="form-section" disabled={busy}>
        <legend>{isNew ? "New restriction" : "Edit restriction"}</legend>
        <div className="fields">
          <label>
            {label}
            <select
              required
              disabled={!employeeActive}
              value={values.targetId}
              onChange={(e) =>
                setValues((v) => ({ ...v, targetId: e.target.value }))
              }
            >
              {options.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                  {statusSuffix(t)}
                </option>
              ))}
            </select>
          </label>
          <label>
            Status
            <select
              value={values.active_status}
              onChange={(e) =>
                setValues((v) => ({
                  ...v,
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
              placeholder={notesPlaceholder}
              value={values.notes ?? ""}
              onChange={(e) =>
                setValues((v) => ({ ...v, notes: e.target.value || null }))
              }
            />
          </label>
        </div>
      </fieldset>
      <div className="form-actions">
        <button className="btn btn-primary" type="submit" disabled={busy}>
          {busy ? "Saving…" : "Save restriction"}
        </button>
        <button
          className="btn btn-quiet"
          type="button"
          disabled={busy}
          onClick={onCancel}
        >
          Cancel
        </button>
      </div>
    </form>
  );
}
