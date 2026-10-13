import { useRef, useState } from "react";
import { useFocusAfterRender } from "../hooks/useFocusAfterRender";
import {
  Confirmation,
  LoadError,
  RecordFeedback,
  RecordFilterSelect,
} from "../RecordControls";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useRecordMutation } from "../hooks/useRecordMutation";
import type { RecordFilter } from "../lib/employees";
import {
  archiveHoliday,
  createHoliday,
  getHoliday,
  holidayData,
  listHolidayCoverageTemplates,
  listHolidays,
  previewHolidayRotation,
  readHolidayCollisions,
  updateHoliday,
  type CoverageBehavior,
  type Holiday,
  type HolidayData,
  type HolidaySet,
} from "../lib/holidays";
import { matchesFilter } from "../lib/positionsCoverage";
import { CollisionList } from "./CollisionList";
import { HolidayForm } from "./HolidayForm";
import { HolidayList } from "./HolidayList";
import { RotationPreview } from "./RotationPreview";
import { holidaySets } from "./labels";

const newHoliday: HolidayData = {
  name: "",
  holiday_set: "none",
  date_kind: "fixed",
  month: 1,
  day_of_month: 1,
  weekday: null,
  nth: null,
  exact_date: null,
  coverage_behavior: "normal",
  custom_coverage_template_id: null,
  active_status: "active",
  notes: null,
};
const coverageChoices: readonly { value: CoverageBehavior; label: string }[] = [
  { value: "normal", label: "Normal" },
  { value: "custom", label: "Custom" },
];

export function Holidays() {
  const [yearText, setYearText] = useState(() =>
    String(new Date().getFullYear()),
  );
  const year = Number(yearText);
  const validYear = Number.isInteger(year) && year >= 1 && year <= 9999;
  const [filter, setFilter] = useState<RecordFilter>("current");
  const [setChoice, setSetChoice] = useState<HolidaySet | "all">("all");
  const [coverageChoice, setCoverageChoice] = useState<
    CoverageBehavior | "all"
  >("all");
  const [search, setSearch] = useState("");
  // Reads cover the whole year and reload only when the year changes or a
  // holiday is saved; the filters and search above apply on screen.
  const [version, setVersion] = useState(0);
  const yearKey = validYear ? `${year}#${version}` : null;
  const holidays = useCommandQuery(yearKey, () => listHolidays(year));
  const collisions = useCommandQuery(yearKey, () =>
    readHolidayCollisions({ kind: "year", year }),
  );
  const rotation = useCommandQuery(validYear ? String(year) : null, () =>
    previewHolidayRotation(year),
  );
  // The custom coverage a holiday can choose; it doesn't depend on the year.
  const coverage = useCommandQuery("coverage", listHolidayCoverageTemplates);
  const [draft, setDraft] = useState<{
    row: Holiday | null;
    data: HolidayData;
  } | null>(null);
  // A fresh form for every opened record.
  const [epoch, setEpoch] = useState(0);
  const { busy, feedback, setFeedback, run } = useRecordMutation();
  const [archive, setArchive] = useState<Holiday | null>(null);
  // Saving disables, and archiving or closing removes, the focused control;
  // focus then moves to a heading instead of falling to the page.
  const listHeading = useRef<HTMLParagraphElement>(null);
  const detailHeading = useRef<HTMLHeadingElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const focusOn = (to: "list" | "detail") =>
    focusAfterRender(
      () => (to === "list" ? listHeading : detailHeading).current,
    );

  function choose(row: Holiday | null) {
    setDraft({ row, data: row ? holidayData(row) : newHoliday });
    setEpoch((v) => v + 1);
    setFeedback(null);
  }
  function saved(row: Holiday) {
    setDraft({ row, data: holidayData(row) });
    setVersion((v) => v + 1);
  }
  function archived() {
    setDraft(null);
    setVersion((v) => v + 1);
  }
  async function save(data: HolidayData) {
    const row = draft?.row ?? null;
    await run(() => (row ? updateHoliday(row, data) : createHoliday(data)), {
      success: "Holiday saved.",
      onSuccess: saved,
    });
    focusOn("detail");
  }
  async function confirmArchive() {
    if (!archive) return;
    const row = archive;
    const result = await run(() => archiveHoliday(row), {
      success: "Holiday archived.",
      onSuccess: archived,
    });
    setArchive(null);
    const succeeded =
      result?.status === "success" || result?.status === "no_change";
    focusOn(succeeded ? "list" : "detail");
  }
  async function reload() {
    const row = draft?.row;
    if (!row) return;
    await run(() => getHoliday(row.id), {
      success: "Loaded the latest saved version.",
      onSuccess: saved,
    });
    focusOn("detail");
  }

  const loadError = holidays.error || collisions.error || rotation.error;
  const refreshing = holidays.loading || collisions.loading || rotation.loading;
  const term = search.trim().toLowerCase();
  const visible = (holidays.data ?? []).filter(
    (row) =>
      matchesFilter(row, filter) &&
      (setChoice === "all" || row.holiday_set === setChoice) &&
      (coverageChoice === "all" || row.coverage_behavior === coverageChoice) &&
      row.name.toLowerCase().includes(term),
  );
  return (
    <section className="page" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">Holidays</h1>
          <p className="page-lede">
            Holiday dates, which employee group works them each year, and their
            coverage.
          </p>
        </div>
        <div className="page-actions">
          <button
            className="btn btn-primary"
            type="button"
            disabled={busy || !validYear || !holidays.data}
            onClick={() => choose(null)}
          >
            Add holiday
          </button>
        </div>
      </header>
      <div className="toolbar">
        <label className="hol-year-field">
          Year
          <input
            type="number"
            min="1"
            max="9999"
            step="1"
            value={yearText}
            disabled={busy}
            onChange={(e) => setYearText(e.target.value)}
          />
        </label>
        <RecordFilterSelect
          label="Holiday status"
          value={filter}
          onChange={setFilter}
        />
        <label>
          Holiday set
          <select
            value={setChoice}
            onChange={(e) =>
              setSetChoice(
                holidaySets.find((s) => s.value === e.target.value)?.value ??
                  "all",
              )
            }
          >
            <option value="all">All sets</option>
            {holidaySets.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Coverage
          <select
            value={coverageChoice}
            onChange={(e) =>
              setCoverageChoice(
                coverageChoices.find((c) => c.value === e.target.value)
                  ?.value ?? "all",
              )
            }
          >
            <option value="all">All coverage</option>
            {coverageChoices.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <label className="grow">
          Search names
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
      </div>
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={() => {
          void reload();
        }}
      />
      {!validYear ? (
        <div className="notice notice-error" role="alert">
          <p>Use a whole year from 1 through 9999.</p>
        </div>
      ) : loadError ? (
        <LoadError
          message={loadError}
          onRetry={() => {
            setVersion((v) => v + 1);
            rotation.reload();
          }}
        />
      ) : !holidays.data || !collisions.data || !rotation.data ? (
        <p className="status-line" role="status">
          Loading holidays…
        </p>
      ) : (
        <div
          className={refreshing ? "is-refreshing" : undefined}
          aria-busy={refreshing}
        >
          <div className="hol-year">
            <RotationPreview rotation={rotation.data} />
            <CollisionList year={year} collisions={collisions.data} />
          </div>
          <div className="split">
            <HolidayList
              rows={visible}
              selectedId={draft?.row?.id ?? null}
              busy={busy}
              headingRef={listHeading}
              onChoose={choose}
            />
            <div className="split-detail">
              {draft ? (
                <HolidayForm
                  key={epoch}
                  headingRef={detailHeading}
                  row={draft.row}
                  data={draft.data}
                  year={year}
                  templates={coverage.data}
                  templatesError={coverage.error}
                  busy={busy}
                  onChange={(data) => setDraft((d) => (d ? { ...d, data } : d))}
                  onRetryTemplates={coverage.reload}
                  onSave={(data) => {
                    void save(data);
                  }}
                  onCancel={() => {
                    setDraft(null);
                    setFeedback(null);
                    focusOn("list");
                  }}
                  onArchive={() => {
                    if (draft.row) setArchive(draft.row);
                  }}
                />
              ) : (
                <div className="split-empty">
                  <h2>No holiday selected</h2>
                  <p>Choose a holiday to edit, or add one.</p>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
      {archive && (
        <Confirmation
          title="Archive holiday?"
          confirmLabel="Archive holiday"
          busyLabel="Archiving…"
          danger
          busy={busy}
          onConfirm={() => {
            void confirmArchive();
          }}
          onCancel={() => setArchive(null)}
        >
          <p>
            Archiving can&rsquo;t be undone. Past schedules keep their copy.
          </p>
        </Confirmation>
      )}
    </section>
  );
}
