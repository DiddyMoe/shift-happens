import { useId, useRef, useState } from "react";
import { Link } from "react-router";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useRecordMutation } from "../hooks/useRecordMutation";
import {
  archiveRestriction,
  createRestriction,
  listRestrictions,
  restrictionData,
  updateRestriction,
  type Employee,
  type EmployeeRestriction,
  type RecordFilter,
  type RestrictionData,
} from "../lib/employees";
import {
  Confirmation,
  LoadError,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import { endsNextDay, restrictionDescription, weekdayLabels } from "./format";
import { RestrictionForm } from "./RestrictionForm";

const newRestriction: RestrictionData = {
  restriction_kind: "time_of_day_block",
  time_of_day: "day",
  weekday: null,
  start_time: null,
  end_time: null,
  active_status: "active",
  notes: null,
};

/** The restriction in words, with the clock range in the display face. */
function RestrictionSummary({
  restriction: r,
}: {
  restriction: RestrictionData;
}) {
  if (r.restriction_kind === "time_of_day_block")
    return <>{restrictionDescription(r)}</>;
  return (
    <>
      {r.weekday ? weekdayLabels[r.weekday] : ""}{" "}
      <span className="display">
        {r.start_time}–{r.end_time}
      </span>
      {endsNextDay(r) ? " (ends next day)" : ""}
    </>
  );
}

export function EmployeeRestrictions({ employee }: { employee: Employee }) {
  const [filter, setFilter] = useState<RecordFilter>("current");
  // A saved employee revision refreshes the rows without remounting.
  const rows = useCommandQuery(
    `${employee.id}:${employee.revision}:${filter}`,
    () => listRestrictions(employee.id, filter),
  );
  const [draft, setDraft] = useState<{
    restriction: EmployeeRestriction | null;
    data: RestrictionData;
  } | null>(null);
  const [archive, setArchive] = useState<EmployeeRestriction | null>(null);
  const { busy, feedback, setFeedback, run } = useRecordMutation();
  const titleId = useId();
  const heading = useRef<HTMLHeadingElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const focusHeading = (onlyIfLost = false) =>
    focusAfterRender(() => heading.current, onlyIfLost);
  // A new employee revision (saved, archived or reset) starts the restriction
  // panel fresh, as a remount would, while the rows stay on screen.
  const [seenRevision, setSeenRevision] = useState(employee.revision);
  if (seenRevision !== employee.revision) {
    setSeenRevision(employee.revision);
    setDraft(null);
    setArchive(null);
    setFeedback(null);
  }
  const archived = employee.archived_at !== null;

  function edit(restriction: EmployeeRestriction | null) {
    setFeedback(null);
    setDraft({
      restriction,
      data: restriction ? restrictionData(restriction) : newRestriction,
    });
  }
  function save(data: RestrictionData) {
    const restriction = draft?.restriction ?? null;
    void run(
      () =>
        restriction
          ? updateRestriction(restriction, data)
          : createRestriction(employee.id, data),
      {
        success: "Restriction saved.",
        onSuccess: () => {
          setDraft(null);
          rows.reload();
          focusHeading(true);
        },
      },
    );
  }
  function confirmArchive(restriction: EmployeeRestriction) {
    void run(() => archiveRestriction(restriction), {
      success: "Restriction archived.",
      onSuccess: () => {
        setArchive(null);
        setDraft(null);
        rows.reload();
        focusHeading();
      },
      onFailure: () => setArchive(null),
    });
  }

  return (
    <section className="rule-top" aria-labelledby={titleId}>
      <div className="section-header">
        <div>
          <h2 id={titleId} ref={heading} tabIndex={-1}>
            Restrictions
          </h2>
          <p className="section-lede">
            Times or shift types this employee can&rsquo;t work.
          </p>
        </div>
      </div>
      <div className="cluster emp-related">
        <Link
          to={`/shift-templates?employee=${encodeURIComponent(employee.id)}`}
        >
          Shift template restrictions
        </Link>
        <Link
          to={`/positions-coverage?employee=${encodeURIComponent(employee.id)}`}
        >
          Position restrictions
        </Link>
      </div>
      <div className="toolbar">
        <RecordFilterSelect
          label="Status"
          value={filter}
          onChange={setFilter}
        />
        <div className="toolbar-end">
          <button
            className="btn"
            type="button"
            disabled={busy || archived}
            onClick={() => edit(null)}
          >
            Add restriction
          </button>
        </div>
      </div>
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={() => {
          setDraft(null);
          setFeedback(null);
          rows.reload();
        }}
        reloadLabel="Reload restrictions and discard edits"
      />
      {rows.error ? (
        <LoadError message={rows.error} onRetry={rows.reload} />
      ) : rows.data === null ? (
        <p role="status" className="status-line">
          Loading restrictions…
        </p>
      ) : rows.data.length === 0 ? (
        <p className="muted">No restrictions match this filter.</p>
      ) : (
        <ul className="ruled-list" aria-labelledby={titleId}>
          {rows.data.map((r) => (
            <li key={r.id} className="emp-restriction">
              <div className="emp-restriction-body">
                <p className="emp-restriction-title">
                  <span>
                    <RestrictionSummary restriction={r} />
                  </span>
                  <RecordStatusTag record={r} />
                </p>
                {r.notes && <p className="prewrap emp-note">{r.notes}</p>}
              </div>
              {!archived && r.archived_at === null && (
                <div className="cluster">
                  <button
                    className="btn btn-sm"
                    type="button"
                    disabled={busy}
                    onClick={() => edit(r)}
                  >
                    Edit
                    <span className="visually-hidden">
                      {" "}
                      {restrictionDescription(r)}
                    </span>
                  </button>
                  <button
                    className="btn btn-sm btn-danger"
                    type="button"
                    disabled={busy}
                    onClick={() => setArchive(r)}
                  >
                    Archive
                    <span className="visually-hidden">
                      {" "}
                      {restrictionDescription(r)}
                    </span>
                  </button>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
      {draft && (
        <RestrictionForm
          key={`${draft.restriction?.id ?? "new"}:${draft.restriction?.revision ?? 0}`}
          initial={draft.data}
          busy={busy}
          onSave={save}
          onCancel={() => {
            setDraft(null);
            setFeedback(null);
            focusHeading();
          }}
        />
      )}
      {archive && (
        <Confirmation
          title="Archive restriction?"
          confirmLabel="Archive"
          busyLabel="Archiving…"
          danger
          busy={busy}
          onConfirm={() => confirmArchive(archive)}
          onCancel={() => setArchive(null)}
        >
          <p>
            {restrictionDescription(archive)} will stop applying to{" "}
            {employee.display_name}. Archiving can&rsquo;t be undone. Past
            schedules keep their copy.
          </p>
        </Confirmation>
      )}
    </section>
  );
}
