import { useEffect, useRef, useState } from "react";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import {
  Confirmation,
  LoadError,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { RecordStatusTag } from "../components/RecordStatusTag";
import {
  RestrictionsPanel,
  type RestrictionTarget,
  type RestrictionValues,
} from "../components/RestrictionsPanel";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useRecordMutation } from "../hooks/useRecordMutation";
import { formatHours } from "../lib/duration";
import type { RecordFilter } from "../lib/employees";
import { colorStyle } from "../lib/magnetColor";
import {
  archiveShiftTemplate,
  archiveTemplateRestriction,
  createShiftTemplate,
  createTemplateRestriction,
  getShiftTemplate,
  listShiftTemplates,
  listTemplateRestrictions,
  templateData,
  updateShiftTemplate,
  updateTemplateRestriction,
  type ShiftTemplate,
  type ShiftTemplateData,
  type TemplateRestriction,
  type TemplateRestrictionData,
} from "../lib/shiftTemplates";
import { ShiftTemplateForm } from "./ShiftTemplateForm";

const newTemplate: ShiftTemplateData = {
  name: "",
  display_label: null,
  start_time: "0900",
  end_time: "1700",
  time_of_day: "day",
  requires_recovery_after_shift: false,
  is_night_shift: false,
  color_hex: null,
  active_status: "active",
  notes: null,
};

const restrictionData = (v: RestrictionValues): TemplateRestrictionData => ({
  shift_template_id: v.targetId,
  restriction_type: "cannot_work",
  active_status: v.active_status,
  notes: v.notes,
});
const templateRestrictions: RestrictionTarget<
  ShiftTemplate,
  TemplateRestriction
> = {
  noun: "shift template",
  sectionId: "template-restrictions",
  listTargets: () => listShiftTemplates("all"),
  list: listTemplateRestrictions,
  create: (employeeId, values) =>
    createTemplateRestriction(employeeId, restrictionData(values)),
  update: (row, values) =>
    updateTemplateRestriction(row, restrictionData(values)),
  archive: archiveTemplateRestriction,
  targetOf: (row) => row.shift_template_id,
  swatch: (template) => template.color_hex,
};

export function ShiftTemplates() {
  const [filter, setFilter] = useState<RecordFilter>("current");
  const [search, setSearch] = useState("");
  // The list follows the search box after a pause in typing, not per key.
  const [query, setQuery] = useState("");
  useEffect(() => {
    const timer = window.setTimeout(() => setQuery(search), 200);
    return () => window.clearTimeout(timer);
  }, [search]);
  // Bumped after each change here, so the list and restrictions reload.
  const [version, setVersion] = useState(0);
  const list = useCommandQuery(JSON.stringify([filter, query, version]), () =>
    listShiftTemplates(filter, query),
  );
  const [selection, setSelection] = useState<{
    row: ShiftTemplate | null;
    data: ShiftTemplateData;
  } | null>(null);
  // A fresh form for every opened or saved record.
  const [editorEpoch, setEditorEpoch] = useState(0);
  const { busy, feedback, setFeedback, run } = useRecordMutation();
  const [confirmation, setConfirmation] = useState<
    | { kind: "archive"; row: ShiftTemplate }
    | { kind: "inactive"; row: ShiftTemplate; data: ShiftTemplateData }
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

  function open(row: ShiftTemplate | null, data: ShiftTemplateData) {
    setSelection({ row, data });
    setEditorEpoch((v) => v + 1);
  }
  function saved(row: ShiftTemplate) {
    open(row, templateData(row));
    setVersion((v) => v + 1);
  }
  function archived() {
    setSelection(null);
    setVersion((v) => v + 1);
  }
  async function save(data: ShiftTemplateData) {
    const row = selection?.row ?? null;
    if (row?.active_status === "active" && data.active_status === "inactive") {
      setConfirmation({ kind: "inactive", row, data });
      return;
    }
    await run(
      () => (row ? updateShiftTemplate(row, data) : createShiftTemplate(data)),
      { success: "Template saved.", onSuccess: saved },
    );
    focusOn("detail");
  }
  async function confirm() {
    if (!confirmation) return;
    const action = confirmation;
    const result =
      action.kind === "archive"
        ? await run(() => archiveShiftTemplate(action.row), {
            success: "Template archived.",
            onSuccess: archived,
          })
        : await run(() => updateShiftTemplate(action.row, action.data), {
            success: "Template made inactive.",
            onSuccess: saved,
          });
    setConfirmation(null);
    const succeeded =
      result?.status === "success" || result?.status === "no_change";
    focusOn(action.kind === "archive" && succeeded ? "list" : "detail");
  }
  async function reloadSelected() {
    const row = selection?.row;
    if (!row) return;
    await run(() => getShiftTemplate(row.id), {
      success: "Loaded the latest saved version.",
      onSuccess: saved,
    });
    focusOn("detail");
  }

  const rows = list.data;
  return (
    <section className="page" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">Shift templates</h1>
          <p className="page-lede">
            Shift times, paid hours, and the color each shift has on the board.
          </p>
        </div>
        <div className="page-actions">
          <button
            className="btn btn-primary"
            type="button"
            disabled={busy}
            onClick={() => {
              setFeedback(null);
              open(null, newTemplate);
            }}
          >
            Add template
          </button>
        </div>
      </header>
      <div className="toolbar">
        <label className="grow">
          Search name or label
          <input
            type="search"
            maxLength={240}
            disabled={busy}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
        <RecordFilterSelect
          label="Template status"
          value={filter}
          disabled={busy}
          onChange={setFilter}
        />
      </div>
      <div className="split">
        <div className="split-list">
          {list.error ? (
            <div className="tpl-list-state">
              <LoadError message={list.error} onRetry={list.reload} />
            </div>
          ) : rows === null ? (
            <div className="tpl-list-state">
              <p className="status-line" role="status">
                Loading templates…
              </p>
            </div>
          ) : (
            <>
              <p ref={listHeading} tabIndex={-1} className="split-list-header">
                <span>
                  {rows.length === 0
                    ? "No templates match these filters"
                    : rows.length === 1
                      ? "1 template"
                      : `${rows.length} templates`}
                </span>
              </p>
              {rows.length > 0 && (
                <ul
                  className={`record-list tpl-list${list.loading ? " is-refreshing" : ""}`}
                  aria-label="Shift templates"
                  aria-busy={list.loading}
                >
                  {rows.map((row) => {
                    const selected = selection?.row?.id === row.id;
                    const overnight = row.end_time < row.start_time;
                    return (
                      <li key={row.id}>
                        <button
                          className={`record-item${selected ? " is-selected" : ""}`}
                          type="button"
                          disabled={busy}
                          aria-current={selected ? "true" : undefined}
                          onClick={() => {
                            setFeedback(null);
                            open(row, templateData(row));
                          }}
                        >
                          <span className="record-title">
                            <span
                              className="swatch tpl-chip"
                              style={colorStyle("--swatch", row.color_hex)}
                              aria-hidden="true"
                            />
                            <span className="tpl-item-name">
                              {row.display_label ?? row.name}
                            </span>
                            <RecordStatusTag record={row} />
                          </span>
                          {row.display_label && (
                            <span className="record-meta">{row.name}</span>
                          )}
                          <span className="record-meta tpl-item-meta">
                            <span className="display">
                              {row.start_time}–{row.end_time}
                            </span>
                            , {row.time_of_day},{" "}
                            <span className="display">
                              {formatHours(row.paid_minutes)}
                            </span>{" "}
                            paid
                          </span>
                          {(overnight ||
                            row.requires_recovery_after_shift ||
                            row.is_night_shift) && (
                            <span className="cluster tpl-item-flags">
                              {overnight && (
                                <span className="tag">Overnight</span>
                              )}
                              {row.is_night_shift && (
                                <span className="tag">Night</span>
                              )}
                              {row.requires_recovery_after_shift && (
                                <span className="tag">Recovery after</span>
                              )}
                            </span>
                          )}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </>
          )}
        </div>
        <div className="split-detail">
          <RecordFeedback
            feedback={feedback}
            busy={busy}
            onReload={() => {
              void reloadSelected();
            }}
          />
          {selection ? (
            <ShiftTemplateForm
              key={editorEpoch}
              headingRef={detailHeading}
              initial={selection.data}
              row={selection.row}
              busy={busy}
              onSave={(data) => {
                void save(data);
              }}
              onClose={() => {
                setSelection(null);
                setFeedback(null);
                focusOn("list");
              }}
              onArchive={() => {
                if (selection.row)
                  setConfirmation({ kind: "archive", row: selection.row });
              }}
            />
          ) : (
            <div className="split-empty">
              <h2>Choose a template</h2>
              <p>Select a template to edit, or add one.</p>
            </div>
          )}
        </div>
      </div>
      <RestrictionsPanel
        target={templateRestrictions}
        sourceRefresh={version}
      />
      {confirmation && (
        <Confirmation
          title={
            confirmation.kind === "archive"
              ? "Archive template?"
              : "Make template inactive?"
          }
          confirmLabel={
            confirmation.kind === "archive"
              ? "Archive template"
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
            {confirmation.row.name} won&rsquo;t be offered for new coverage or
            restrictions. Move or archive its active coverage items first.
          </p>
          <p>
            {confirmation.kind === "archive"
              ? "Archiving can’t be undone. Past schedules keep their copy."
              : "You can make it active again later."}
          </p>
        </Confirmation>
      )}
    </section>
  );
}
