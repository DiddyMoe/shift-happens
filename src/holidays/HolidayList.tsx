import type { RefObject } from "react";
import { RecordStatusTag } from "../components/RecordStatusTag";
import type { HolidayListEntry, HolidaySet } from "../lib/holidays";
import { holidaySets } from "./labels";

function HolidaySetTag({ set }: { set: HolidaySet }) {
  const label = holidaySets.find((s) => s.value === set)?.label ?? set;
  return (
    <span className={set === "none" ? "tag tag-muted" : "tag"}>{label}</span>
  );
}

export function HolidayList({
  rows,
  selectedId,
  busy,
  headingRef,
  onChoose,
}: {
  rows: readonly HolidayListEntry[];
  selectedId: string | null;
  busy: boolean;
  /** The list's count line, which takes focus after an archive. */
  headingRef: RefObject<HTMLParagraphElement | null>;
  onChoose: (row: HolidayListEntry) => void;
}) {
  return (
    <div className="split-list">
      <p ref={headingRef} tabIndex={-1} className="split-list-header">
        <span>
          {rows.length === 0
            ? "No holidays match these filters"
            : rows.length === 1
              ? "1 holiday"
              : `${rows.length} holidays`}
        </span>
      </p>
      {rows.length > 0 && (
        <ul className="record-list" aria-label="Holidays">
          {rows.map((row) => {
            const selected = row.id === selectedId;
            return (
              <li key={row.id}>
                <button
                  type="button"
                  className={`record-item${selected ? " is-selected" : ""}`}
                  aria-current={selected ? "true" : undefined}
                  disabled={busy}
                  onClick={() => onChoose(row)}
                >
                  <span className="record-title">
                    {row.name}
                    <RecordStatusTag record={row} />
                  </span>
                  <span className="record-meta hol-item-meta">
                    {row.resolved_date ? (
                      <span className="display">{row.resolved_date}</span>
                    ) : (
                      "No date this year"
                    )}
                  </span>
                  <span className="record-meta hol-item-meta">
                    <HolidaySetTag set={row.holiday_set} />
                    {row.coverage_behavior === "custom"
                      ? "Custom coverage"
                      : "Normal coverage"}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
