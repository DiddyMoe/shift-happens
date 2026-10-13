import { useId, useRef, useState, type RefObject } from "react";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import {
  Confirmation,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { useRecordMutation } from "../hooks/useRecordMutation";
import type { RecordFilter } from "../lib/employees";
import {
  archivePosition,
  createPosition,
  getPosition,
  matchesFilter,
  positionData,
  updatePosition,
  type Position,
  type PositionData,
} from "../lib/positionsCoverage";

const newPosition: PositionData = {
  name: "",
  display_label: null,
  sort_order: 10,
  active_status: "active",
  notes: null,
};

export function PositionsSection({
  positions,
  refreshSources,
}: {
  positions: readonly Position[];
  refreshSources: () => void;
}) {
  const [filter, setFilter] = useState<RecordFilter>("current");
  const [draft, setDraft] = useState<{
    row: Position | null;
    data: PositionData;
  } | null>(null);
  // A fresh form for every opened or saved record.
  const [epoch, setEpoch] = useState(0);
  const { busy, feedback, setFeedback, run } = useRecordMutation();
  const [confirmation, setConfirmation] = useState<
    | { kind: "archive"; row: Position }
    | { kind: "inactive"; row: Position; data: PositionData }
    | null
  >(null);
  // Saving disables, and archiving or closing removes, the focused control;
  // focus then moves to a heading instead of falling to the page.
  const listHeading = useRef<HTMLParagraphElement>(null);
  const detailHeading = useRef<HTMLHeadingElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const focusOn = (to: "list" | "detail") =>
    focusAfterRender(
      () => (to === "list" ? listHeading : detailHeading).current,
    );

  function open(row: Position | null, data: PositionData) {
    setDraft({ row, data });
    setEpoch((v) => v + 1);
  }
  function saved(row: Position) {
    open(row, positionData(row));
    refreshSources();
  }
  function archived() {
    setDraft(null);
    refreshSources();
  }
  async function save(data: PositionData) {
    const row = draft?.row ?? null;
    if (row?.active_status === "active" && data.active_status === "inactive") {
      setConfirmation({ kind: "inactive", row, data });
      return;
    }
    await run(() => (row ? updatePosition(row, data) : createPosition(data)), {
      success: "Position saved.",
      onSuccess: saved,
    });
    focusOn("detail");
  }
  async function confirm() {
    if (!confirmation) return;
    const action = confirmation;
    const result =
      action.kind === "archive"
        ? await run(() => archivePosition(action.row), {
            success: "Position archived.",
            onSuccess: archived,
          })
        : await run(() => updatePosition(action.row, action.data), {
            success: "Position made inactive.",
            onSuccess: saved,
          });
    setConfirmation(null);
    const succeeded =
      result?.status === "success" || result?.status === "no_change";
    focusOn(action.kind === "archive" && succeeded ? "list" : "detail");
  }
  async function reload() {
    const row = draft?.row;
    if (!row) return;
    await run(() => getPosition(row.id), {
      success: "Loaded the latest saved version.",
      onSuccess: saved,
    });
    focusOn("detail");
  }
  const visible = positions.filter((row) => matchesFilter(row, filter));
  return (
    <section className="section" aria-labelledby="positions-title">
      <header className="section-header">
        <div>
          <h2 id="positions-title">Positions</h2>
          <p className="section-lede">Listed in board order.</p>
        </div>
        <div className="cluster">
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy}
            onClick={() => {
              setFeedback(null);
              open(null, newPosition);
            }}
          >
            Add position
          </button>
        </div>
      </header>
      <div className="toolbar">
        <RecordFilterSelect
          label="Position status"
          value={filter}
          disabled={busy}
          onChange={setFilter}
        />
      </div>
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={() => {
          void reload();
        }}
      />
      <div className="split">
        <div className="split-list">
          <p ref={listHeading} tabIndex={-1} className="split-list-header">
            <span>
              {visible.length === 0
                ? "No positions match this filter"
                : visible.length === 1
                  ? "1 position"
                  : `${visible.length} positions`}
            </span>
          </p>
          {visible.length > 0 && (
            <ul className="record-list" aria-label="Positions">
              {visible.map((row) => {
                const selected = draft?.row?.id === row.id;
                return (
                  <li key={row.id}>
                    <button
                      type="button"
                      className={`record-item${selected ? " is-selected" : ""}`}
                      aria-current={selected ? "true" : undefined}
                      disabled={busy}
                      onClick={() => {
                        setFeedback(null);
                        open(row, positionData(row));
                      }}
                    >
                      <span className="record-title">
                        <span className="display">
                          {row.display_label ?? row.name}
                        </span>
                        <RecordStatusTag record={row} />
                      </span>
                      {row.display_label !== null && (
                        <span className="record-meta">{row.name}</span>
                      )}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        <div className="split-detail">
          {draft ? (
            <PositionForm
              key={epoch}
              headingRef={detailHeading}
              row={draft.row}
              initial={draft.data}
              busy={busy}
              onSave={(data) => {
                void save(data);
              }}
              onCancel={() => {
                setDraft(null);
                setFeedback(null);
                focusOn("list");
              }}
              onArchive={() => {
                if (draft.row)
                  setConfirmation({ kind: "archive", row: draft.row });
              }}
            />
          ) : (
            <div className="split-empty">
              <h3 className="pc-title">No position chosen</h3>
              <p>Choose a position to edit, or add one.</p>
            </div>
          )}
        </div>
      </div>
      {confirmation && (
        <Confirmation
          title={
            confirmation.kind === "archive"
              ? "Archive position?"
              : "Make position inactive?"
          }
          confirmLabel={
            confirmation.kind === "archive"
              ? "Archive position"
              : "Make inactive"
          }
          busyLabel={confirmation.kind === "archive" ? "Archiving…" : "Saving…"}
          danger={confirmation.kind === "archive"}
          busy={busy}
          onConfirm={() => {
            void confirm();
          }}
          onCancel={() => setConfirmation(null)}
        >
          <p>
            {confirmation.kind === "archive"
              ? "Archive or move this position’s active coverage first. Archiving can’t be undone."
              : "You can make it active again later."}
          </p>
        </Confirmation>
      )}
    </section>
  );
}

function PositionForm({
  row,
  initial,
  busy,
  headingRef,
  onSave,
  onCancel,
  onArchive,
}: {
  row: Position | null;
  initial: PositionData;
  busy: boolean;
  /** The form's heading, which takes focus after a save. */
  headingRef: RefObject<HTMLHeadingElement | null>;
  onSave: (data: PositionData) => void;
  onCancel: () => void;
  onArchive: () => void;
}) {
  const [data, setData] = useState(initial);
  const statusHelp = useId();
  const archived = row !== null && row.archived_at !== null;
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        onSave(data);
      }}
    >
      <div className="detail-header">
        <div>
          <h3 ref={headingRef} tabIndex={-1} className="pc-title">
            {row ? row.name : "New position"}
            {row && <RecordStatusTag record={row} />}
          </h3>
        </div>
      </div>
      <fieldset className="form-section" disabled={busy || archived}>
        <legend>Position details</legend>
        <div className="fields">
          <label>
            Name
            <input
              required
              maxLength={240}
              value={data.name}
              onChange={(e) => setData((d) => ({ ...d, name: e.target.value }))}
            />
          </label>
          <label>
            Display label
            <input
              maxLength={240}
              value={data.display_label ?? ""}
              onChange={(e) =>
                setData((d) => ({
                  ...d,
                  display_label: e.target.value || null,
                }))
              }
            />
          </label>
          <label>
            Sort order
            <input
              type="number"
              required
              step="1"
              min={-Number.MAX_SAFE_INTEGER}
              max={Number.MAX_SAFE_INTEGER}
              value={data.sort_order}
              onChange={(e) =>
                setData((d) => ({ ...d, sort_order: e.target.valueAsNumber }))
              }
            />
          </label>
          <div>
            <label>
              Status
              <select
                value={data.active_status}
                aria-describedby={statusHelp}
                onChange={(e) =>
                  setData((d) => ({
                    ...d,
                    active_status:
                      e.target.value === "inactive" ? "inactive" : "active",
                  }))
                }
              >
                <option value="active">Active</option>
                <option value="inactive">Inactive</option>
              </select>
            </label>
            <p className="field-help" id={statusHelp}>
              A position with active coverage can&rsquo;t be made inactive or
              archived.
            </p>
          </div>
          <label className="span-all">
            Notes
            <textarea
              rows={3}
              maxLength={8000}
              placeholder="Scheduling notes only — no patient or medical details."
              value={data.notes ?? ""}
              onChange={(e) =>
                setData((d) => ({ ...d, notes: e.target.value || null }))
              }
            />
          </label>
        </div>
      </fieldset>
      <div className="form-actions">
        {!archived && (
          <button type="submit" className="btn btn-primary" disabled={busy}>
            {busy ? "Saving…" : "Save position"}
          </button>
        )}
        <button
          type="button"
          className="btn btn-quiet"
          disabled={busy}
          onClick={onCancel}
        >
          {archived ? "Close" : "Cancel"}
        </button>
        {row && !archived && (
          <>
            <span className="spacer" />
            <button
              type="button"
              className="btn btn-danger"
              disabled={busy}
              onClick={onArchive}
            >
              Archive position
            </button>
          </>
        )}
      </div>
    </form>
  );
}
