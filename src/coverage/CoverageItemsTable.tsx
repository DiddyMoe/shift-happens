import { useRef, useState } from "react";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import {
  Confirmation,
  LoadError,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useRecordMutation } from "../hooks/useRecordMutation";
import type { RecordFilter } from "../lib/employees";
import { colorStyle } from "../lib/magnetColor";
import type { ShiftTemplate } from "../lib/shiftTemplates";
import {
  actualPreview,
  archiveCoverageItem,
  coverageItemData,
  createCoverageItem,
  dayLabels,
  isActive,
  listCoverageItems,
  updateCoverageItem,
  type CoverageItem,
  type CoverageItemData,
  type CoverageTemplate,
  type Position,
} from "../lib/positionsCoverage";
import { CoverageItemForm } from "./CoverageItemForm";

export function CoverageItemsTable({
  template,
  positions,
  templates,
  shifts,
  sourceRefresh,
  refreshSources,
  reloadToken,
}: {
  template: CoverageTemplate;
  positions: readonly Position[];
  templates: readonly CoverageTemplate[];
  shifts: readonly ShiftTemplate[];
  sourceRefresh: number;
  refreshSources: () => void;
  /** Changes when the parent reloads; resets edits like a remount would, but
   * keeps the current rows visible until the fresh ones arrive. */
  reloadToken: number;
}) {
  const [filter, setFilter] = useState<RecordFilter>("current");
  const [version, setVersion] = useState(0);
  // While another template's rows load, the previous ones stay visible but
  // inert instead of blinking out.
  const items = useCommandQuery(
    JSON.stringify([template.id, filter, version, sourceRefresh, reloadToken]),
    () => listCoverageItems(template.id, filter),
  );
  const [draft, setDraft] = useState<{
    row: CoverageItem | null;
    data: CoverageItemData;
  } | null>(null);
  // A fresh form for every opened draft.
  const [epoch, setEpoch] = useState(0);
  const { busy, feedback, setFeedback, run } = useRecordMutation();
  const [confirmation, setConfirmation] = useState<
    | { kind: "archive"; row: CoverageItem }
    | { kind: "inactive"; row: CoverageItem; data: CoverageItemData }
    | null
  >(null);
  // Focus returns to the heading once a form or dialog closes, instead of
  // falling to the page when the focused control disappears.
  const heading = useRef<HTMLHeadingElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const focusHeading = () => focusAfterRender(() => heading.current);
  const resetKey = `${template.id}:${reloadToken}`;
  const [seenReset, setSeenReset] = useState(resetKey);
  if (seenReset !== resetKey) {
    setSeenReset(resetKey);
    setDraft(null);
    setFeedback(null);
    setConfirmation(null);
  }
  function openDraft(row: CoverageItem | null, data: CoverageItemData) {
    setDraft({ row, data });
    setEpoch((v) => v + 1);
    setFeedback(null);
  }
  function saved() {
    setDraft(null);
    setVersion((v) => v + 1);
  }
  async function save(data: CoverageItemData) {
    const row = draft?.row ?? null;
    if (row?.active_status === "active" && data.active_status === "inactive") {
      setConfirmation({ kind: "inactive", row, data });
      return;
    }
    await run(
      () => (row ? updateCoverageItem(row, data) : createCoverageItem(data)),
      { success: "Coverage item saved.", onSuccess: saved },
    );
    focusHeading();
  }
  async function confirm() {
    if (!confirmation) return;
    const action = confirmation;
    await run(
      () =>
        action.kind === "archive"
          ? archiveCoverageItem(action.row)
          : updateCoverageItem(action.row, action.data),
      {
        success:
          action.kind === "archive"
            ? "Coverage item archived."
            : "Coverage item made inactive.",
        onSuccess: saved,
      },
    );
    setConfirmation(null);
    focusHeading();
  }
  function add() {
    const position = positions.find(isActive),
      shift = shifts.find(isActive);
    if (!position || !shift) return;
    openDraft(null, {
      coverage_template_id: template.id,
      shift_position_id: position.id,
      shift_template_id: shift.id,
      display_label: null,
      sort_order: 10,
      required_count: 1,
      actual_start_day_offset: 0,
      actual_end_day_offset: 0,
      active_status: "active",
      notes: null,
    });
  }
  const canAdd =
    isActive(template) && positions.some(isActive) && shifts.some(isActive);
  const rows = items.data;
  const refreshing = items.loading && rows !== null;
  const headingId = `coverage-items-${template.id}`;
  return (
    <section className="section-gap" aria-labelledby={headingId}>
      <header className="section-header">
        <div>
          <h3 ref={heading} tabIndex={-1} id={headingId} className="pc-title">
            Coverage items
          </h3>
        </div>
        <div className="cluster">
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || !canAdd}
            onClick={add}
          >
            Add coverage item
          </button>
        </div>
      </header>
      <div className="toolbar">
        <RecordFilterSelect
          label="Item status"
          value={filter}
          disabled={busy}
          onChange={setFilter}
        />
      </div>
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={() => {
          setDraft(null);
          setFeedback(null);
          setVersion((v) => v + 1);
          refreshSources();
        }}
        reloadLabel="Reload and discard edits"
      />
      {items.error ? (
        <LoadError message={items.error} onRetry={items.reload} />
      ) : rows === null ? (
        <p className="status-line" role="status">
          Loading coverage items…
        </p>
      ) : rows.length === 0 ? (
        <div
          className={`empty${refreshing ? " is-refreshing" : ""}`}
          inert={refreshing}
        >
          <p>No coverage items match this filter.</p>
        </div>
      ) : (
        <div
          className={`table-wrap${refreshing ? " is-refreshing" : ""}`}
          inert={refreshing}
          aria-busy={refreshing}
          tabIndex={0}
          role="region"
          aria-label="Coverage items"
        >
          <table className="data-table pc-items-table">
            <caption>
              {template.name}, {dayLabels[template.day_type]}
            </caption>
            <thead>
              <tr>
                <th scope="col">Position</th>
                <th scope="col">Shift</th>
                <th scope="col" className="num">
                  Required
                </th>
                <th scope="col">When</th>
                <th scope="col" className="num">
                  Sort
                </th>
                <th scope="col">Status</th>
                <th scope="col" className="actions">
                  <span className="visually-hidden">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const position = positions.find(
                    (p) => p.id === row.shift_position_id,
                  ),
                  shift = shifts.find((s) => s.id === row.shift_template_id);
                return (
                  <tr key={row.id}>
                    <td>
                      {position ? (
                        <span className="display pc-position">
                          {position.display_label ?? position.name}
                        </span>
                      ) : (
                        "Position unavailable"
                      )}
                      {row.display_label && (
                        <span className="field-help">{row.display_label}</span>
                      )}
                    </td>
                    <td>
                      {shift ? (
                        <span className="pc-shift">
                          <span
                            className="swatch"
                            style={colorStyle("--swatch", shift.color_hex)}
                            aria-hidden="true"
                          />
                          <span>{shift.display_label ?? shift.name}</span>
                        </span>
                      ) : (
                        "Shift unavailable"
                      )}
                    </td>
                    <td className="num">
                      <span className="display pc-required">
                        {row.required_count}
                      </span>
                    </td>
                    <td>
                      <span className="display">
                        {actualPreview(template.day_type, row, shift)}
                      </span>
                    </td>
                    <td className="num">
                      <span className="display">{row.sort_order}</span>
                    </td>
                    <td>
                      {isActive(row) ? (
                        "Active"
                      ) : (
                        <RecordStatusTag record={row} />
                      )}
                    </td>
                    <td className="actions">
                      <button
                        type="button"
                        className="btn btn-sm"
                        disabled={busy}
                        onClick={() => openDraft(row, coverageItemData(row))}
                      >
                        {row.archived_at ? "View" : "Edit"}
                      </button>
                      {!row.archived_at && (
                        <button
                          type="button"
                          className="btn btn-sm btn-danger"
                          disabled={busy}
                          onClick={() =>
                            setConfirmation({ kind: "archive", row })
                          }
                        >
                          Archive
                        </button>
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
        <CoverageItemForm
          key={epoch}
          template={template}
          positions={positions}
          templates={templates}
          shifts={shifts}
          row={draft.row}
          initial={draft.data}
          busy={busy}
          onSave={(data) => {
            void save(data);
          }}
          onCancel={() => {
            setDraft(null);
            focusHeading();
          }}
        />
      )}
      {confirmation && (
        <Confirmation
          title={
            confirmation.kind === "archive"
              ? "Archive coverage item?"
              : "Make coverage item inactive?"
          }
          confirmLabel={
            confirmation.kind === "archive"
              ? "Archive coverage item"
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
            It stops counting toward coverage, and a default template must keep
            at least one active item.{" "}
            {confirmation.kind === "archive"
              ? "Archiving can’t be undone. Past schedules keep their copy."
              : "You can make it active again later."}
          </p>
        </Confirmation>
      )}
    </section>
  );
}
