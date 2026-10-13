import { MenuChevron, Popover } from "../Popover";
import type { Schedule } from "../lib/schedules";

/** Less frequent actions for the open plan. What it lists depends on the
 * plan's status: an archived plan shows its details, and Delete… when it
 * was never finalized. */
export function PlanMenu({
  schedule,
  isCurrent,
  canDelete,
  disabled,
  onSetCurrent,
  onClearCurrent,
  onDetails,
  onArchive,
  onReopen,
  onDelete,
}: {
  schedule: Schedule;
  isCurrent: boolean;
  /** Only a never-finalized draft can be deleted. */
  canDelete: boolean;
  disabled: boolean;
  onSetCurrent: () => void;
  onClearCurrent: () => void;
  onDetails: () => void;
  onArchive: () => void;
  onReopen: () => void;
  onDelete: () => void;
}) {
  const archived = schedule.archived_at !== null;
  const draft = schedule.status === "draft" && !archived;
  const final = schedule.status === "final" && !archived;
  const item = (
    label: string,
    action: () => void,
    options: { danger?: boolean; dialog?: boolean } = {},
  ) => (
    <button
      type="button"
      className={options.danger ? "menu-item is-danger" : "menu-item"}
      disabled={disabled}
      aria-haspopup={options.dialog ? "dialog" : undefined}
      onClick={action}
    >
      {label}
    </button>
  );
  return (
    <Popover
      label={
        <>
          More
          <MenuChevron />
        </>
      }
      ariaLabel="More schedule actions"
      panelLabel="More schedule actions"
      panelClassName="menu"
    >
      {final && item("Reopen as draft…", onReopen, { dialog: true })}
      {!archived &&
        (isCurrent
          ? item("Clear current", onClearCurrent)
          : item("Set as current", onSetCurrent))}
      {draft
        ? item("Rename and notes…", onDetails, { dialog: true })
        : item("Schedule details…", onDetails, { dialog: true })}
      {(!archived || canDelete) && <hr className="menu-sep" />}
      {!archived && item("Archive…", onArchive, { danger: true, dialog: true })}
      {canDelete && item("Delete…", onDelete, { danger: true, dialog: true })}
    </Popover>
  );
}
