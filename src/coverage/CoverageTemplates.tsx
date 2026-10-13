import { useRef, useState } from "react";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import {
  Confirmation,
  HowItWorks,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { useRecordMutation } from "../hooks/useRecordMutation";
import type { RecordFilter } from "../lib/employees";
import type { ShiftTemplate } from "../lib/shiftTemplates";
import {
  archiveCoverageTemplate,
  coverageTemplateData,
  createCoverageTemplate,
  dayLabels,
  dayTypes,
  isActive,
  matchesFilter,
  setDefaultCoverageTemplate,
  statusSuffix,
  updateCoverageTemplate,
  type CoverageTemplate,
  type CoverageTemplateData,
  type DayType,
  type Position,
} from "../lib/positionsCoverage";
import { CoverageItemsTable } from "./CoverageItemsTable";
import { CoverageTemplateForm } from "./CoverageTemplateForm";

type Sources = {
  positions: readonly Position[];
  templates: readonly CoverageTemplate[];
  shifts: readonly ShiftTemplate[];
  sourceRefresh: number;
  refreshSources: () => void;
};
export function CoverageTemplates(props: Sources) {
  const [day, setDay] = useState<DayType>("monday");
  return (
    <section className="section rule-top" aria-labelledby="coverage-title">
      <header className="section-header">
        <div>
          <h2 id="coverage-title">Coverage</h2>
          <p className="section-lede">
            How many people each operational day needs, by position and shift.
          </p>
          <HowItWorks label="How coverage works">
            <p>
              Each weekday uses its default template. To change a day&rsquo;s
              coverage, add an alternative, give it items, then set it as the
              default.
            </p>
            <p>
              An item&rsquo;s timing places its shift on the operational day or
              the next, so a night shift can end the next morning.
            </p>
            <p>
              Holiday custom templates are used only by holidays that choose
              them on the Holidays screen.
            </p>
          </HowItWorks>
        </div>
      </header>
      <div className="pc-day-row">
        <span className="pc-day-label" id="pc-day-choice-label">
          Operational day
        </span>
        <div
          className="segmented pc-days"
          role="group"
          aria-labelledby="pc-day-choice-label"
        >
          {dayTypes.map((d) => (
            <button
              key={d}
              type="button"
              className={d === "holiday_custom" ? "pc-day-holiday" : undefined}
              aria-pressed={d === day}
              onClick={() => setDay(d)}
            >
              {dayLabels[d]}
            </button>
          ))}
        </div>
      </div>
      <DayCoverage day={day} {...props} />
    </section>
  );
}

function DayCoverage({
  day,
  positions,
  templates,
  shifts,
  sourceRefresh,
  refreshSources,
}: Sources & { day: DayType }) {
  const [filter, setFilter] = useState<RecordFilter>("current");
  const [selectedId, setSelectedId] = useState("");
  const [draft, setDraft] = useState<{
    row: CoverageTemplate | null;
    data: CoverageTemplateData;
  } | null>(null);
  // A fresh form for every opened draft; also resets the coverage items.
  const [epoch, setEpoch] = useState(0);
  const { busy, feedback, setFeedback, run } = useRecordMutation();
  const [confirmation, setConfirmation] = useState<
    | { kind: "archive"; row: CoverageTemplate }
    | { kind: "inactive"; row: CoverageTemplate; data: CoverageTemplateData }
    | { kind: "default"; row: CoverageTemplate; previous: CoverageTemplate }
    | null
  >(null);
  // Focus moves to the template heading (or the template choice when no
  // template is shown) once a form or dialog closes.
  const detailHeading = useRef<HTMLHeadingElement>(null);
  const templateSelect = useRef<HTMLSelectElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const focusHeading = () =>
    focusAfterRender(() => detailHeading.current ?? templateSelect.current);
  // Switching day starts the day's view fresh, as a remount would, but keeps
  // the coverage table mounted so it never blinks out between days.
  const [seenDay, setSeenDay] = useState(day);
  if (seenDay !== day) {
    setSeenDay(day);
    setFilter("current");
    setSelectedId("");
    setDraft(null);
    setFeedback(null);
    setConfirmation(null);
  }
  const visible = templates.filter(
    (t) => t.day_type === day && matchesFilter(t, filter),
  );
  const current = templates.find(
    (t) => t.day_type === day && t.is_default && isActive(t),
  );
  // A chosen template that is no longer listed (for example just archived)
  // falls back to the day's default instead of leaving the view empty.
  const selected =
    (selectedId ? visible.find((t) => t.id === selectedId) : undefined) ??
    visible.find((t) => t.is_default) ??
    visible[0];
  const weekday = day !== "holiday_custom";
  function openDraft(row: CoverageTemplate | null, data: CoverageTemplateData) {
    setDraft({ row, data });
    setEpoch((v) => v + 1);
    setFeedback(null);
  }
  function saved(row: CoverageTemplate) {
    setSelectedId(row.id);
    setDraft(null);
    setEpoch((v) => v + 1);
    refreshSources();
  }
  async function save(data: CoverageTemplateData) {
    const row = draft?.row ?? null;
    if (row?.active_status === "active" && data.active_status === "inactive") {
      setConfirmation({ kind: "inactive", row, data });
      return;
    }
    await run(
      () =>
        row ? updateCoverageTemplate(row, data) : createCoverageTemplate(data),
      { success: "Template saved.", onSuccess: saved },
    );
    focusHeading();
  }
  async function confirm() {
    if (!confirmation) return;
    const action = confirmation;
    await run(
      () =>
        action.kind === "archive"
          ? archiveCoverageTemplate(action.row)
          : action.kind === "inactive"
            ? updateCoverageTemplate(action.row, action.data)
            : setDefaultCoverageTemplate(action.row, action.previous),
      {
        success:
          action.kind === "default"
            ? "Default changed."
            : action.kind === "archive"
              ? "Template archived."
              : "Template made inactive.",
        onSuccess: saved,
      },
    );
    setConfirmation(null);
    focusHeading();
  }
  return (
    <div>
      <p
        className={weekday && !current ? "notice notice-warn" : "status-line"}
        role="status"
      >
        {!weekday ? (
          "Holidays with custom coverage use one of these templates."
        ) : current ? (
          <>
            Default for {dayLabels[day]}: <strong>{current.name}</strong>
          </>
        ) : (
          `${dayLabels[day]} has no active default template.`
        )}
      </p>
      <div className="toolbar">
        <RecordFilterSelect
          label="Template status"
          value={filter}
          disabled={busy}
          onChange={(v) => {
            setFilter(v);
            setSelectedId("");
            setDraft(null);
            setFeedback(null);
          }}
        />
        <label className="grow">
          Template
          <select
            ref={templateSelect}
            value={selected?.id ?? ""}
            disabled={busy}
            onChange={(e) => {
              setSelectedId(e.target.value);
              setDraft(null);
              setFeedback(null);
            }}
          >
            <option value="">Choose a template</option>
            {visible.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
                {weekday && t.is_default ? " (default)" : ""}
                {statusSuffix(t)}
              </option>
            ))}
          </select>
        </label>
        <div className="toolbar-end">
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() =>
              openDraft(null, {
                name: "",
                day_type: day,
                active_status: "active",
                notes: null,
              })
            }
          >
            {weekday ? "Add alternative template" : "Add template"}
          </button>
        </div>
      </div>
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={() => {
          setDraft(null);
          setFeedback(null);
          setEpoch((v) => v + 1);
          refreshSources();
        }}
        reloadLabel="Reload templates and discard edits"
      />
      <div className="rule-top">
        {draft ? (
          <CoverageTemplateForm
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
              focusHeading();
            }}
          />
        ) : selected ? (
          <div>
            <div className="detail-header">
              <div>
                <h3 ref={detailHeading} tabIndex={-1} className="pc-title">
                  {selected.name}
                  {weekday &&
                    (selected.is_default ? (
                      <span className="tag tag-current">Default</span>
                    ) : (
                      <span className="tag">Alternative</span>
                    ))}
                  <RecordStatusTag record={selected} />
                </h3>
              </div>
              {selected.archived_at === null && (
                <div className="cluster">
                  <button
                    type="button"
                    className="btn"
                    disabled={busy}
                    onClick={() =>
                      openDraft(selected, coverageTemplateData(selected))
                    }
                  >
                    Edit template
                  </button>
                  {weekday && !selected.is_default && (
                    <button
                      type="button"
                      className="btn"
                      disabled={busy || !isActive(selected) || !current}
                      onClick={() => {
                        if (current)
                          setConfirmation({
                            kind: "default",
                            row: selected,
                            previous: current,
                          });
                      }}
                    >
                      Set as default
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn btn-danger"
                    disabled={busy}
                    onClick={() =>
                      setConfirmation({ kind: "archive", row: selected })
                    }
                  >
                    Archive template
                  </button>
                </div>
              )}
            </div>
            {selected.notes && (
              <p className="prewrap pc-notes">{selected.notes}</p>
            )}
          </div>
        ) : (
          <div className="empty">
            <p>
              {visible.length > 0
                ? "Choose a template above to see its coverage."
                : "No templates match this day and status. Add one or change the filter."}
            </p>
          </div>
        )}
        {selected && (
          <CoverageItemsTable
            reloadToken={epoch}
            template={selected}
            positions={positions}
            templates={templates}
            shifts={shifts}
            sourceRefresh={sourceRefresh}
            refreshSources={refreshSources}
          />
        )}
      </div>
      {confirmation && (
        <Confirmation
          title={
            confirmation.kind === "default"
              ? "Change the default?"
              : confirmation.kind === "archive"
                ? "Archive coverage template?"
                : "Make coverage template inactive?"
          }
          confirmLabel={
            confirmation.kind === "default"
              ? "Set as default"
              : confirmation.kind === "archive"
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
          {confirmation.kind === "default" ? (
            <p>
              {confirmation.row.name} will replace {confirmation.previous.name}{" "}
              as the default for {dayLabels[day]}. It needs active items.
            </p>
          ) : (
            <p>
              Archive or move its active items first. A default template needs a
              replacement first.{" "}
              {confirmation.kind === "archive"
                ? "Archiving can’t be undone. Past schedules keep their copy."
                : "You can make it active again later."}
            </p>
          )}
        </Confirmation>
      )}
    </div>
  );
}
