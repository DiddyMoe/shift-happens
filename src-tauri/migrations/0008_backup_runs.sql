-- Operational history; file publication never depends on this table.
CREATE TABLE backup_runs (
    id TEXT NOT NULL PRIMARY KEY,
    backup_type TEXT NOT NULL CHECK (backup_type IN ('manual', 'pre_migration')),
    destination_path TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('success', 'failed')),
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    file_size_bytes INTEGER NULL CHECK (
        file_size_bytes IS NULL OR (typeof(file_size_bytes) = 'integer' AND file_size_bytes > 0)
    ),
    error_message TEXT NULL,
    created_at TEXT NOT NULL,
    CHECK ((status = 'success' AND file_size_bytes IS NOT NULL AND error_message IS NULL)
        OR (status = 'failed' AND file_size_bytes IS NULL AND error_message IS NOT NULL))
);
-- Callers parse RFC 3339 instants before chronological ordering; this index
-- supports the type-filtered history scan, not raw-text chronological semantics.
CREATE INDEX backup_runs_type_completed ON backup_runs (backup_type, completed_at, id);
CREATE INDEX backup_runs_completed ON backup_runs (completed_at, id);
