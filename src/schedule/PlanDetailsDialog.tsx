import { useEffect, useRef, useState, type FormEvent } from "react";
import { trapDialogFocus } from "../lib/dialogFocus";
import { displayLocalDate } from "../lib/localDate";
import type { Schedule } from "../lib/schedules";
import { PlanTags } from "./PlanTags";

/** A plan's name, notes and record dates. A draft's name and notes can be
 * edited here; final and archived plans are shown read-only. */
export function PlanDetailsDialog({
  schedule,
  isCurrent,
  busy,
  onSave,
  onClose,
}: {
  schedule: Schedule;
  isCurrent: boolean;
  busy: boolean;
  onSave: (name: string, notes: string | null) => void;
  onClose: () => void;
}) {
  const editable = schedule.status === "draft" && !schedule.archived_at;
  const [name, setName] = useState(schedule.name);
  const [notes, setNotes] = useState(schedule.notes ?? "");
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    const modal = dialog.current;
    modal?.showModal();
    return () => {
      modal?.close();
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus();
    };
  }, []);
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editable || busy || !name.trim()) return;
    onSave(name, notes.trim() ? notes : null);
  }
  const unchanged = name === schedule.name && notes === (schedule.notes ?? "");
  return (
    <dialog
      ref={dialog}
      className="dialog plan-details"
      aria-labelledby="plan-details-title"
      onKeyDown={trapDialogFocus}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      <h2 id="plan-details-title">
        {editable ? "Rename and notes" : "Schedule details"}
      </h2>
      <form id="plan-details-form" onSubmit={submit}>
        {editable ? (
          <div className="stack">
            <label>
              Name
              <input
                required
                maxLength={120}
                value={name}
                disabled={busy}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <label>
              Notes
              <textarea
                maxLength={4000}
                value={notes}
                disabled={busy}
                onChange={(e) => setNotes(e.target.value)}
              />
            </label>
            <p className="field-help">
              Dates and the settings captured at generation stay fixed.
            </p>
          </div>
        ) : (
          <dl className="facts">
            <dt>Name</dt>
            <dd>{schedule.name}</dd>
            <dt>Notes</dt>
            <dd className="prewrap">{schedule.notes || "No notes."}</dd>
          </dl>
        )}
      </form>
      <dl className="facts plan-details-facts">
        <dt>Status</dt>
        <dd className="cluster">
          <PlanTags schedule={schedule} current={isCurrent} />
        </dd>
        <dt>Dates</dt>
        <dd className="display">
          {displayLocalDate(schedule.start_date)} to{" "}
          {displayLocalDate(schedule.end_date)}
        </dd>
        <dt>Created</dt>
        <dd className="display">{schedule.created_at}</dd>
        <dt>Updated</dt>
        <dd className="display">{schedule.updated_at}</dd>
        {schedule.archived_at && (
          <>
            <dt>Archived</dt>
            <dd className="display">{schedule.archived_at}</dd>
          </>
        )}
      </dl>
      <div className="dialog-actions">
        <button
          type="button"
          className={editable ? "btn" : "btn btn-primary"}
          disabled={busy}
          onClick={onClose}
        >
          {editable ? "Cancel" : "Close"}
        </button>
        {editable && (
          <button
            type="submit"
            form="plan-details-form"
            className="btn btn-primary"
            disabled={busy || !name.trim() || unchanged}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        )}
      </div>
    </dialog>
  );
}
