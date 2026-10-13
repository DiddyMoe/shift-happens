import { ScheduleStatusTag } from "../ScheduleBoard";
import type { Schedule } from "../lib/schedules";

/** Status tags for a plan; an archived plan also keeps its draft/final state. */
export function PlanTags({
  schedule,
  current,
}: {
  schedule: Schedule;
  current: boolean;
}) {
  return (
    <>
      {schedule.archived_at && (
        <span className="tag tag-muted">
          {schedule.status === "final" ? "Final" : "Draft"}
        </span>
      )}
      <ScheduleStatusTag schedule={schedule} />
      {current && <span className="tag tag-current">Current</span>}
    </>
  );
}
