import { useMemo, useState } from "react";
import { employeeLabel, isOpen, positionLabel, shiftClock } from "./lib/board";
import type { Weekday as SettingWeekday } from "./lib/ipc";
import {
  addLocalDays,
  formatLocalDate,
  localDateOrdinal,
  localWeekday,
  parseLocalDate,
} from "./lib/localDate";
import { magnetStyle } from "./lib/magnetColor";
import type { Assignment, Schedule } from "./lib/schedules";

export type BoardView = "employees" | "positions";

export function ScheduleStatusTag({ schedule }: { schedule: Schedule }) {
  if (schedule.archived_at)
    return <span className="tag tag-archived">Archived</span>;
  return schedule.status === "final" ? (
    <span className="tag tag-final">Final</span>
  ) : (
    <span className="tag tag-draft">Draft</span>
  );
}

const weekdayNumbers: Readonly<Record<SettingWeekday, number>> = {
  sunday: 0,
  monday: 1,
  tuesday: 2,
  wednesday: 3,
  thursday: 4,
  friday: 5,
  saturday: 6,
};
const dayNames = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;
const VIEW_KEY = "shifthappens.boardView";

function savedView(): BoardView {
  try {
    return window.localStorage.getItem(VIEW_KEY) === "positions"
      ? "positions"
      : "employees";
  } catch {
    return "employees";
  }
}
function saveView(view: BoardView) {
  try {
    window.localStorage.setItem(VIEW_KEY, view);
  } catch {
    // The view choice is a convenience; the board works without storage.
  }
}

/** Today's local civil date, used only to mark the matching column. */
function localToday(): string {
  const now = new Date();
  return `${String(now.getFullYear()).padStart(4, "0")}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

const sortKey = (text: string) => text.normalize("NFC").toLowerCase();

interface Row {
  key: string;
  label: string;
  open: boolean;
  order: number;
  items: Assignment[];
}

function LockIcon() {
  return (
    <svg className="icon" viewBox="0 0 12 12" aria-hidden="true">
      <rect x="2" y="5.2" width="8" height="5.8" rx="1.2" fill="currentColor" />
      <path
        d="M3.9 5.2V3.9a2.1 2.1 0 0 1 4.2 0v1.3"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.3"
      />
    </svg>
  );
}

export function Magnet({
  assignment,
  view,
  selected,
  disabled,
  onSelect,
}: {
  assignment: Assignment;
  view: BoardView;
  selected: boolean;
  disabled?: boolean | undefined;
  onSelect?: ((assignment: Assignment) => void) | undefined;
}) {
  const open = isOpen(assignment);
  const title =
    view === "employees"
      ? positionLabel(assignment)
      : open
        ? "Open"
        : employeeLabel(assignment);
  const description = [
    positionLabel(assignment),
    `${assignment.shift_name_snapshot} ${shiftClock(assignment)}`,
    `${dayNames[weekdayOf(assignment.schedule_date)] ?? ""} ${assignment.schedule_date}`,
    open ? "open slot" : employeeLabel(assignment),
    assignment.locked ? "locked" : null,
  ]
    .filter(Boolean)
    .join(", ");
  const content = (
    <>
      <span className="magnet-title">
        <span>{title}</span>
        {assignment.locked && <LockIcon />}
      </span>
      <span className="magnet-time">{shiftClock(assignment)}</span>
    </>
  );
  const className = `magnet${open ? " is-open" : ""}`;
  const style = open ? undefined : magnetStyle(assignment.shift_color_snapshot);
  if (!onSelect)
    return (
      <span className={className} style={style}>
        {content}
      </span>
    );
  return (
    <button
      type="button"
      className={className}
      style={style}
      aria-pressed={selected}
      aria-label={description}
      disabled={disabled}
      onClick={() => onSelect(assignment)}
    >
      {content}
    </button>
  );
}

function weekdayOf(date: string): number {
  const parsed = parseLocalDate(date);
  return parsed.ok ? localWeekday(parsed.value) : -1;
}

/**
 * The schedule as a magnetic staffing board: one week at a time, rows are
 * employees (with open slots on top) or positions, columns are operational
 * dates. Read-only unless `onSelect` is supplied.
 */
export function ScheduleBoard({
  assignments,
  startDate,
  endDate,
  weekStart,
  selectedId = "",
  focusDate,
  onSelect,
  disabled = false,
  label,
}: {
  assignments: readonly Assignment[];
  startDate: string;
  endDate: string;
  weekStart?: SettingWeekday | null;
  selectedId?: string;
  /** A date to show first, for example a selected assignment's date. */
  focusDate?: string | null;
  onSelect?: (assignment: Assignment) => void;
  disabled?: boolean;
  label: string;
}) {
  const [view, setView] = useState<BoardView>(savedView);
  const today = localToday();
  const range = useMemo(() => {
    const start = parseLocalDate(startDate);
    const end = parseLocalDate(endDate);
    if (!start.ok || !end.ok) return null;
    const first =
      weekStart === undefined || weekStart === null
        ? localWeekday(start.value)
        : weekdayNumbers[weekStart];
    // Weeks begin on the workweek start, up to six days before the schedule.
    // Near 0001-01-01 those days may not exist, so weeks are counted as
    // ordinals and only real dates are shown.
    const back = (localWeekday(start.value) - first + 7) % 7;
    const origin = localDateOrdinal(start.value) - back;
    const span = localDateOrdinal(end.value) - origin;
    if (span < 0) return null;
    return {
      origin,
      back,
      startDay: start.value,
      weeks: Math.floor(span / 7) + 1,
      start: startDate,
      end: endDate,
    };
  }, [startDate, endDate, weekStart]);

  const weekOf = (date: string | null | undefined) => {
    if (!range || !date) return null;
    const parsed = parseLocalDate(date);
    if (!parsed.ok) return null;
    const offset = localDateOrdinal(parsed.value) - range.origin;
    const week = Math.floor(offset / 7);
    return week >= 0 && week < range.weeks ? week : null;
  };
  // The shown week is remembered as a date inside it, so it stays put when the
  // saved workweek start arrives and realigns the weeks.
  const [anchor, setAnchor] = useState(() =>
    focusDate && focusDate >= startDate && focusDate <= endDate
      ? focusDate
      : today >= startDate && today <= endDate
        ? today
        : startDate,
  );
  const [followed, setFollowed] = useState(focusDate);
  if (focusDate !== followed) {
    setFollowed(focusDate);
    if (focusDate && focusDate >= startDate && focusDate <= endDate)
      setAnchor(focusDate);
  }
  const shownWeek = range
    ? Math.min(Math.max(weekOf(anchor) ?? 0, 0), range.weeks - 1)
    : 0;
  const moveWeek = (step: number) => {
    if (!range) return;
    // The first week's anchor is the schedule's first date.
    const value = addLocalDays(
      range.startDay,
      Math.max((shownWeek + step) * 7 - range.back, 0),
    );
    if (value.ok) setAnchor(formatLocalDate(value.value));
  };

  const days = useMemo(() => {
    if (!range) return [];
    const result: { date: string; weekday: number; inRange: boolean }[] = [];
    for (let d = 0; d < 7; d += 1) {
      const value = addLocalDays(
        range.startDay,
        shownWeek * 7 + d - range.back,
      );
      if (!value.ok) continue;
      const date = formatLocalDate(value.value);
      result.push({
        date,
        weekday: localWeekday(value.value),
        inRange: date >= range.start && date <= range.end,
      });
    }
    return result;
  }, [range, shownWeek]);

  const rows = useMemo(() => {
    const visible = new Set(days.map((d) => d.date));
    const map = new Map<string, Row>();
    const openRow: Row = {
      key: "open",
      label: "Open slots",
      open: true,
      order: -1,
      items: [],
    };
    for (const a of assignments) {
      if (view === "employees" && isOpen(a)) {
        openRow.items.push(a);
        continue;
      }
      const key =
        view === "employees"
          ? (a.assigned_employee_id ?? "")
          : a.shift_position_id;
      const label = view === "employees" ? employeeLabel(a) : positionLabel(a);
      const row = map.get(key) ?? {
        key,
        label,
        open: false,
        order: a.coverage_sort_order_snapshot,
        items: [],
      };
      row.order = Math.min(row.order, a.coverage_sort_order_snapshot);
      row.items.push(a);
      map.set(key, row);
    }
    const sorted = [...map.values()].sort((x, y) => {
      if (view === "positions" && x.order !== y.order) return x.order - y.order;
      const a = sortKey(x.label);
      const b = sortKey(y.label);
      return a < b ? -1 : a > b ? 1 : x.key < y.key ? -1 : 1;
    });
    const all = view === "employees" ? [openRow, ...sorted] : sorted;
    return all.map((row) => ({
      ...row,
      weekItems: row.items.filter((a) => visible.has(a.schedule_date)),
    }));
  }, [assignments, view, days]);

  const holidays = useMemo(() => {
    const names = new Map<string, string>();
    for (const a of assignments)
      if (a.holiday_name_snapshot && !names.has(a.schedule_date))
        names.set(a.schedule_date, a.holiday_name_snapshot);
    return names;
  }, [assignments]);

  if (!range)
    return (
      <p className="notice notice-error" role="alert">
        This schedule's date range could not be read.
      </p>
    );

  const first = days[0]?.date ?? "";
  const last = days.at(-1)?.date ?? "";
  const cellItems = (row: { weekItems: Assignment[] }, date: string) =>
    row.weekItems
      .filter((a) => a.schedule_date === date)
      .sort((a, b) =>
        a.actual_start_datetime !== b.actual_start_datetime
          ? a.actual_start_datetime < b.actual_start_datetime
            ? -1
            : 1
          : a.coverage_sort_order_snapshot !== b.coverage_sort_order_snapshot
            ? a.coverage_sort_order_snapshot - b.coverage_sort_order_snapshot
            : a.coverage_slot_index_snapshot - b.coverage_slot_index_snapshot,
      );
  const openThisWeek = assignments.filter(
    (a) => isOpen(a) && a.schedule_date >= first && a.schedule_date <= last,
  ).length;

  return (
    <div className="board">
      <div className="board-toolbar">
        <div className="week-pager">
          <button
            type="button"
            className="btn"
            disabled={shownWeek === 0}
            onClick={() => moveWeek(-1)}
            aria-label="Previous week"
          >
            <svg className="icon" viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="M10 3 5 8l5 5"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
          <p className="week-label" aria-live="polite">
            <span className="visually-hidden">
              Week {shownWeek + 1} of {range.weeks}:{" "}
            </span>
            {first} to {last}
          </p>
          <button
            type="button"
            className="btn"
            disabled={shownWeek >= range.weeks - 1}
            onClick={() => moveWeek(1)}
            aria-label="Next week"
          >
            <svg className="icon" viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="m6 3 5 5-5 5"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
          {range.weeks > 1 && (
            <span className="board-week-count">
              Week {shownWeek + 1} of {range.weeks}
            </span>
          )}
        </div>
        <div className="segmented" role="group" aria-label="Board rows">
          {(
            [
              ["employees", "By employee"],
              ["positions", "By position"],
            ] as const
          ).map(([value, text]) => (
            <button
              key={value}
              type="button"
              aria-pressed={view === value}
              onClick={() => {
                setView(value);
                saveView(value);
              }}
            >
              {text}
            </button>
          ))}
        </div>
      </div>
      <div className="board-wrap" tabIndex={0} aria-label={label} role="region">
        <table className="board-table">
          <caption className="visually-hidden">
            {label}, {first} to {last},{" "}
            {view === "employees" ? "rows by employee" : "rows by position"}
          </caption>
          <colgroup>
            <col className="board-row-col" />
            {days.map((d) => (
              <col key={d.date} />
            ))}
          </colgroup>
          <thead>
            <tr>
              <th scope="col" className="board-corner">
                {view === "employees" ? "Employee" : "Position"}
              </th>
              {days.map((d) => (
                <th
                  key={d.date}
                  scope="col"
                  className={[
                    d.weekday === 0 || d.weekday === 6 ? "is-weekend" : "",
                    d.date === today ? "is-today" : "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  aria-label={`${dayNames[d.weekday]} ${d.date}${d.date === today ? ", today" : ""}${holidays.has(d.date) ? `, ${holidays.get(d.date)}` : ""}`}
                >
                  <span className="board-day">
                    <span className="board-day-name">
                      {dayNames[d.weekday]}
                    </span>
                    <span className="board-day-num">{d.date.slice(8)}</span>
                  </span>
                  {holidays.has(d.date) && (
                    <span className="board-day-note">
                      {holidays.get(d.date)}
                    </span>
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.key} className={row.open ? "is-open-row" : ""}>
                <th scope="row">
                  {row.label}
                  <span className="board-row-meta">
                    {row.open
                      ? `${openThisWeek} this week`
                      : `${row.weekItems.length} ${row.weekItems.length === 1 ? "shift" : "shifts"} this week`}
                  </span>
                </th>
                {days.map((d) => {
                  const items = cellItems(row, d.date);
                  return (
                    <td
                      key={d.date}
                      className={
                        !d.inRange
                          ? "is-outside"
                          : d.weekday === 0 || d.weekday === 6
                            ? "is-weekend"
                            : undefined
                      }
                    >
                      {items.length > 0 && (
                        <div className="board-cell">
                          {items.map((a) => (
                            <Magnet
                              key={a.id}
                              assignment={a}
                              view={view}
                              selected={a.id === selectedId}
                              disabled={disabled}
                              onSelect={onSelect}
                            />
                          ))}
                        </div>
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {assignments.length === 0 && (
        <p className="status-line">This schedule has no stored assignments.</p>
      )}
    </div>
  );
}
