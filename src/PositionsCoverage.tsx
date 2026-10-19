import { useState } from "react";
import { LoadError } from "./RecordControls";
import {
  RestrictionsPanel,
  type RestrictionTarget,
  type RestrictionValues,
} from "./components/RestrictionsPanel";
import { CoverageTemplates } from "./coverage/CoverageTemplates";
import { PositionsSection } from "./coverage/PositionsSection";
import { useCommandQuery } from "./hooks/useCommandQuery";
import { listShiftTemplates } from "./lib/shiftTemplates";
import {
  archivePositionRestriction,
  createPositionRestriction,
  listCoverageTemplates,
  listPositionRestrictions,
  listPositions,
  updatePositionRestriction,
  type Position,
  type PositionRestriction,
  type PositionRestrictionData,
} from "./lib/positionsCoverage";

const restrictionData = (v: RestrictionValues): PositionRestrictionData => ({
  shift_position_id: v.targetId,
  restriction_type: "cannot_work",
  active_status: v.active_status,
  notes: v.notes,
});
const positionRestrictions: RestrictionTarget<Position, PositionRestriction> = {
  noun: "position",
  sectionId: "position-restrictions",
  listTargets: () => listPositions("all"),
  list: listPositionRestrictions,
  create: (employeeId, values) =>
    createPositionRestriction(employeeId, restrictionData(values)),
  update: (row, values) =>
    updatePositionRestriction(row, restrictionData(values)),
  archive: archivePositionRestriction,
  targetOf: (row) => row.shift_position_id,
};

export function PositionsCoverage() {
  // Bumped after each change here, so every section works from fresh records.
  const [version, setVersion] = useState(0);
  const key = String(version);
  const positions = useCommandQuery(key, () => listPositions());
  const templates = useCommandQuery(key, () => listCoverageTemplates());
  const shifts = useCommandQuery(key, () => listShiftTemplates("all"));
  const error = positions.error || templates.error || shifts.error;
  const refreshSources = () => setVersion((v) => v + 1);
  return (
    <section className="page" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">Positions and coverage</h1>
          <p className="page-lede">
            The positions people work, and how many people each day needs.
          </p>
        </div>
      </header>
      {/* A failed refresh keeps the last records, and any open edits, on
          screen; Try again reloads the records without discarding edits. */}
      {error && <LoadError message={error} onRetry={refreshSources} />}
      {!positions.data || !templates.data || !shifts.data ? (
        !error && (
          <p className="status-line" role="status">
            Loading positions and coverage…
          </p>
        )
      ) : (
        <>
          <PositionsSection
            positions={positions.data}
            refreshSources={refreshSources}
          />
          <CoverageTemplates
            positions={positions.data}
            templates={templates.data}
            shifts={shifts.data}
            sourceRefresh={version}
            refreshSources={refreshSources}
          />
        </>
      )}
      <RestrictionsPanel
        target={positionRestrictions}
        sourceRefresh={version}
      />
    </section>
  );
}
