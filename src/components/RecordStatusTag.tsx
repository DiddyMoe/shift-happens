import type { ActiveStatus } from "../lib/employees";

/** Active is the normal state, so only archived and inactive records are tagged. */
export function RecordStatusTag({
  record,
}: {
  record: { archived_at: string | null; active_status: ActiveStatus };
}) {
  if (record.archived_at)
    return <span className="tag tag-archived">Archived</span>;
  return record.active_status === "inactive" ? (
    <span className="tag tag-muted">Inactive</span>
  ) : null;
}
