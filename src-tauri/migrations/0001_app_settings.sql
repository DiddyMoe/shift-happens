CREATE TABLE app_settings (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NULL,
    value_type TEXT NOT NULL CHECK (
        value_type IN ('string', 'integer', 'boolean', 'date', 'weekday', 'schedule_id')
    ),
    description TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision >= 1)
);
