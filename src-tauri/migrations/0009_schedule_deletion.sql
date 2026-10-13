-- Finalization is remembered after reopening. A draft that was never final may be
-- deleted; anything once final stays as a record and can only be archived.
ALTER TABLE schedules ADD COLUMN finalized_once INTEGER NOT NULL DEFAULT 0
    CHECK (typeof(finalized_once) = 'integer' AND finalized_once IN (0, 1));

-- Archived rows are otherwise read-only. Recording existing final rows is the one backfill.
DROP TRIGGER schedules_readonly_metadata;
UPDATE schedules SET finalized_once = 1 WHERE status = 'final';
CREATE TRIGGER schedules_readonly_metadata BEFORE UPDATE ON schedules
WHEN (OLD.archived_at IS NOT NULL)
    OR ((NEW.name IS NOT OLD.name OR NEW.notes IS NOT OLD.notes) AND OLD.status <> 'draft')
BEGIN SELECT RAISE(ABORT, 'Only non-archived drafts have editable metadata'); END;

CREATE TRIGGER schedules_finalized_once_recorded BEFORE UPDATE ON schedules
WHEN (NEW.status = 'final' AND NEW.finalized_once IS NOT 1)
    OR (OLD.finalized_once = 1 AND NEW.finalized_once IS NOT 1)
BEGIN SELECT RAISE(ABORT, 'Finalization history is retained'); END;

DROP TRIGGER schedules_no_delete;
CREATE TRIGGER schedules_no_delete BEFORE DELETE ON schedules
WHEN NOT (OLD.status = 'draft' AND OLD.archived_at IS NOT NULL AND OLD.finalized_once = 0)
BEGIN SELECT RAISE(ABORT, 'Only archived drafts that were never finalized can be deleted'); END;

DROP TRIGGER schedule_assignments_no_delete;
CREATE TRIGGER schedule_assignments_no_delete BEFORE DELETE ON schedule_assignments
WHEN NOT EXISTS (SELECT 1 FROM schedules WHERE id = OLD.schedule_id AND status = 'draft'
    AND archived_at IS NOT NULL AND finalized_once = 0)
BEGIN SELECT RAISE(ABORT, 'Structural slots cannot be deleted'); END;
