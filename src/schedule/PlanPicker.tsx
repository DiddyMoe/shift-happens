import { Link } from "react-router";
import { MenuChevron, Popover } from "../Popover";
import { displayLocalDate } from "../lib/localDate";
import type { Schedule } from "../lib/schedules";
import { PlanTags } from "./PlanTags";

/** Switches the workspace to another draft or final plan. Archived plans are
 * listed in All schedules. */
export function PlanPicker({
  schedules,
  openId,
  currentId,
  onSwitch,
}: {
  schedules: readonly Schedule[];
  openId: string | null;
  currentId: string | null;
  onSwitch: () => void;
}) {
  return (
    <Popover
      label={
        <>
          Switch schedule
          <MenuChevron />
        </>
      }
      panelLabel="Schedules"
      panelClassName="menu plan-picker"
    >
      <>
        {schedules.length === 0 && (
          <p className="menu-note">No draft or final schedules.</p>
        )}
        {schedules.length > 0 && (
          <ul className="plan-picker-list">
            {schedules.map((schedule) => (
              <li key={schedule.id}>
                <Link
                  className="plan-picker-item"
                  to={`/?id=${encodeURIComponent(schedule.id)}`}
                  aria-current={schedule.id === openId ? "true" : undefined}
                  onClick={() => {
                    if (schedule.id !== openId) onSwitch();
                  }}
                >
                  <span className="plan-picker-name">{schedule.name}</span>
                  <span className="plan-picker-meta">
                    <span className="display">
                      {displayLocalDate(schedule.start_date)} to{" "}
                      {displayLocalDate(schedule.end_date)}
                    </span>
                    <PlanTags
                      schedule={schedule}
                      current={schedule.id === currentId}
                    />
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        )}
        <hr className="menu-sep" />
        <Link className="menu-item" to="/schedules">
          All schedules
        </Link>
      </>
    </Popover>
  );
}
