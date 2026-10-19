import { useId, useRef, useState } from "react";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { LabelSelect } from "../employees/LabelSelect";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useRecordMutation } from "../hooks/useRecordMutation";
import {
  archiveEntry,
  availabilityLabels,
  entryData,
  getEntry,
  listAvailability,
  listPto,
  ptoLabels,
  saveEntry,
  type AvailabilityType,
  type Entry,
  type EntryData,
  type EntryDraft,
  type EntryFilters,
  type PtoStatus,
} from "../lib/availabilityPto";
import type { Employee } from "../lib/employees";
import { readCommandResult } from "../lib/ipc";
import { errorText } from "../lib/recordFeedback";
import {
  Confirmation,
  HowItWorks,
  LoadError,
  RecordFeedback,
} from "../RecordControls";
import { EntryForm } from "./EntryForm";

const typeFilterLabels = { "": "All types", ...availabilityLabels };
const statusFilterLabels = { "": "All statuses", ...ptoLabels };
// Tags follow scheduling effect: ok supplies coverage, plain is a soft
// preference, warn is a soft avoidance, danger prohibits overlapping work, and
// muted has no scheduling effect.
const effectTags: Record<AvailabilityType | PtoStatus, string> = {
  available: "tag tag-ok",
  preferred: "tag",
  unavailable: "tag tag-danger",
  hard_block: "tag tag-danger",
  requested: "tag tag-warn",
  approved: "tag tag-danger",
  denied: "tag tag-muted",
  canceled: "tag tag-muted",
};

export function EntryPanel({
  kind,
  employees,
  filters,
}: {
  kind: "availability" | "pto";
  employees: readonly Employee[];
  filters: EntryFilters;
}) {
  const [type, setType] = useState<AvailabilityType | "">("");
  const [status, setStatus] = useState<PtoStatus | "">("");
  const entries = useCommandQuery<readonly Entry[]>(
    JSON.stringify({ kind, filters, type, status }),
    () =>
      kind === "availability"
        ? listAvailability(filters, type || null)
        : listPto(filters, status || null),
  );
  const [draft, setDraft] = useState<{
    row: Entry | null;
    data: EntryDraft;
  } | null>(null);
  // Changes only when a different entry is opened, not when one is saved.
  const [formKey, setFormKey] = useState(0);
  const [reading, setReading] = useState(false);
  const mutation = useRecordMutation();
  const { feedback, setFeedback, run } = mutation;
  const busy = mutation.busy || reading;
  const [confirmation, setConfirmation] = useState<Entry | null>(null);
  const titleId = useId();
  const heading = useRef<HTMLHeadingElement>(null);
  const formHeading = useRef<HTMLHeadingElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const title = kind === "availability" ? "Availability" : "PTO";
  const noun = kind === "availability" ? "availability" : "PTO";
  const employeeName = (id: string) =>
    employees.find((e) => e.id === id)?.display_name ?? "Employee unavailable";

  function choose(row: Entry | null) {
    const employee =
      employees.find((e) => e.id === filters.employee_id) ??
      employees.find((e) => !e.archived_at && e.active_status === "active");
    const data: EntryData = {
      employee_id: employee?.id ?? "",
      start_date: filters.start_date ?? "",
      end_date: filters.end_date ?? filters.start_date ?? "",
      is_full_day: true,
      start_time: null,
      end_time: null,
      active_status: "active",
      notes: null,
    };
    setDraft({
      row,
      data: row
        ? entryData(row)
        : kind === "availability"
          ? { ...data, entry_type: "available" }
          : { ...data, status: "requested" },
    });
    setFormKey((k) => k + 1);
    setFeedback(null);
  }
  function show(row: Entry) {
    setDraft({ row, data: entryData(row) });
    entries.reload();
  }
  function close() {
    setDraft(null);
    setFeedback(null);
    focusAfterRender(() => heading.current);
  }
  function save(data: EntryDraft) {
    const row = draft?.row ?? null;
    void run(() => saveEntry(row, data), {
      success: `${title} entry saved.`,
      onSuccess: (saved) => {
        show(saved);
        focusAfterRender(() => formHeading.current, true);
      },
    });
  }
  function archive(row: Entry) {
    void run(() => archiveEntry(row), {
      success: `${title} entry archived.`,
      onSuccess: () => {
        setConfirmation(null);
        setDraft(null);
        entries.reload();
        focusAfterRender(() => heading.current);
      },
      onFailure: () => setConfirmation(null),
    });
  }
  async function reload() {
    const row = draft?.row;
    if (!row) return;
    setReading(true);
    try {
      show(readCommandResult(await getEntry(row)));
      focusAfterRender(() => formHeading.current, true);
      setFeedback({
        message: "Loaded the latest saved version.",
        stale: false,
        failed: false,
      });
    } catch (e) {
      setFeedback({ message: errorText(e), stale: false, failed: true });
    } finally {
      setReading(false);
    }
  }

  const showEmployee = filters.employee_id === null;
  const rows = entries.data;
  return (
    <section className="avail-panel" aria-labelledby={titleId}>
      <h2 id={titleId} ref={heading} tabIndex={-1}>
        {title}
      </h2>
      <HowItWorks
        label={
          kind === "availability"
            ? "What each availability type means"
            : "What each PTO status means"
        }
      >
        {kind === "availability" ? (
          <>
            <p>
              Available adds coverage. Preferred is a soft preference.
              Unavailable and Hard block both prevent overlapping work. For an
              employee who must have availability entered, back-to-back
              available times combine to cover a whole shift.
            </p>
            <p>
              Hard blocks and approved PTO win over available and preferred
              times. The date filters match an entry&rsquo;s first and last
              dates; overnight times can run into the next day.
            </p>
          </>
        ) : (
          <p>
            Requested is a soft avoidance. Approved prevents overlapping work.
            Denied and Canceled have no scheduling effect. Entry status is
            separate from PTO status.
          </p>
        )}
      </HowItWorks>
      <div className="toolbar">
        {kind === "availability" ? (
          <LabelSelect
            label="Availability type"
            labels={typeFilterLabels}
            value={type}
            disabled={busy}
            onChange={setType}
          />
        ) : (
          <LabelSelect
            label="PTO status"
            labels={statusFilterLabels}
            value={status}
            disabled={busy}
            onChange={setStatus}
          />
        )}
        <div className="toolbar-end">
          <button
            className="btn btn-primary"
            type="button"
            disabled={busy || employees.length === 0}
            onClick={() => choose(null)}
          >
            Add {noun} entry
          </button>
        </div>
      </div>
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={() => {
          void reload();
        }}
      />
      {entries.error ? (
        <LoadError message={entries.error} onRetry={entries.reload} />
      ) : rows === null ? (
        <p className="status-line" role="status">
          Loading {noun} entries…
        </p>
      ) : rows.length === 0 ? (
        <div
          className={`empty avail-empty${entries.loading ? " is-refreshing" : ""}`}
          aria-busy={entries.loading}
        >
          <p>No {noun} entries match these filters.</p>
        </div>
      ) : (
        // Rows for another employee or filter stay visible until these load,
        // but can't be opened from the keyboard under the new heading.
        <div
          className={`table-wrap${entries.loading ? " is-refreshing" : ""}`}
          inert={entries.stale}
          aria-busy={entries.loading}
        >
          <table className="data-table avail-table">
            <caption className="visually-hidden">{title} entries</caption>
            <thead>
              <tr>
                {showEmployee && <th scope="col">Employee</th>}
                <th scope="col">
                  {kind === "availability" ? "Type" : "PTO status"}
                </th>
                <th scope="col">Dates</th>
                <th scope="col">Time</th>
                <th scope="col">Entry status</th>
                <th scope="col" className="actions">
                  <span className="visually-hidden">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const selected = draft?.row?.id === row.id;
                const name = employeeName(row.employee_id);
                const label =
                  "entry_type" in row
                    ? availabilityLabels[row.entry_type]
                    : ptoLabels[row.status];
                const overnight =
                  row.start_time !== null &&
                  row.end_time !== null &&
                  row.end_time < row.start_time;
                return (
                  <tr
                    key={row.id}
                    className={selected ? "is-selected" : undefined}
                    aria-current={selected ? "true" : undefined}
                  >
                    {showEmployee && <td>{name}</td>}
                    <td>
                      <span
                        className={
                          effectTags[
                            "entry_type" in row ? row.entry_type : row.status
                          ]
                        }
                      >
                        {label}
                      </span>
                    </td>
                    <td>
                      <span className="display">{row.start_date}</span> through{" "}
                      <span className="display">{row.end_date}</span>
                    </td>
                    <td>
                      {row.is_full_day ? (
                        "Full day"
                      ) : (
                        <span className="cluster avail-window">
                          <span className="display">
                            {row.start_time}–{row.end_time}
                          </span>
                          {overnight && <span className="tag">Overnight</span>}
                        </span>
                      )}
                    </td>
                    <td>
                      {row.archived_at || row.active_status === "inactive" ? (
                        <RecordStatusTag record={row} />
                      ) : (
                        "Active"
                      )}
                    </td>
                    <td className="actions">
                      <button
                        className="btn btn-sm"
                        type="button"
                        disabled={busy}
                        onClick={() => choose(row)}
                      >
                        {row.archived_at ? "View" : "Edit"}
                        <span className="visually-hidden">
                          {" "}
                          {label} entry for {name}, {row.start_date} through{" "}
                          {row.end_date}
                        </span>
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {draft && (
        <EntryForm
          key={formKey}
          noun={noun}
          row={draft.row}
          initial={draft.data}
          employees={employees}
          busy={busy}
          headingRef={formHeading}
          onSave={save}
          onCancel={close}
          onArchive={() => {
            if (draft.row) setConfirmation(draft.row);
          }}
        />
      )}
      {confirmation && (
        <Confirmation
          title={`Archive ${noun} entry?`}
          confirmLabel="Archive"
          busyLabel="Archiving…"
          danger
          busy={busy}
          onConfirm={() => archive(confirmation)}
          onCancel={() => setConfirmation(null)}
        >
          <p>
            The entry for {employeeName(confirmation.employee_id)},{" "}
            {confirmation.start_date} through {confirmation.end_date}, will stop
            affecting schedules. Archiving can&rsquo;t be undone. Past schedules
            keep their copy.
          </p>
        </Confirmation>
      )}
    </section>
  );
}
