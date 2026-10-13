-- No supported predecessor contains recurrence_rule_json; V1 has no recurrence column.

CREATE TABLE availability_entries (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) BETWEEN 1 AND 128),
    employee_id TEXT NOT NULL REFERENCES employees(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    entry_type TEXT NOT NULL CHECK (entry_type IN ('available', 'unavailable', 'preferred', 'hard_block')),
    start_date TEXT NOT NULL CHECK (
        start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        AND start_date BETWEEN '0001-01-01' AND '9999-12-31'
        AND date(start_date, '+0 days') IS NOT NULL AND date(start_date, '+0 days') = start_date),
    end_date TEXT NOT NULL CHECK (
        end_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        AND end_date BETWEEN '0001-01-01' AND '9999-12-31'
        AND date(end_date, '+0 days') IS NOT NULL AND date(end_date, '+0 days') = end_date),
    is_full_day INTEGER NOT NULL CHECK (typeof(is_full_day) = 'integer' AND is_full_day IN (0, 1)),
    start_time TEXT NULL,
    end_time TEXT NULL,
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (start_date <= end_date),
    CHECK ((is_full_day = 1 AND start_time IS NULL AND end_time IS NULL)
        OR (is_full_day = 0 AND start_time IS NOT NULL AND end_time IS NOT NULL
            AND start_time GLOB '[0-2][0-9][0-5][0-9]' AND start_time <= '2359'
            AND end_time GLOB '[0-2][0-9][0-5][0-9]' AND end_time <= '2359'
            AND start_time <> end_time)),
    CHECK (end_date < '9999-12-31' OR (is_full_day = 0 AND end_time > start_time)),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);

CREATE INDEX availability_entries_employee_dates ON availability_entries (employee_id, archived_at, active_status, start_date, end_date, entry_type, id);
CREATE INDEX availability_entries_dates_type_status ON availability_entries (archived_at, active_status, entry_type, start_date, end_date, id);

CREATE TABLE pto_entries (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) BETWEEN 1 AND 128),
    employee_id TEXT NOT NULL REFERENCES employees(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    status TEXT NOT NULL CHECK (status IN ('requested', 'approved', 'denied', 'canceled')),
    start_date TEXT NOT NULL CHECK (
        start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        AND start_date BETWEEN '0001-01-01' AND '9999-12-31'
        AND date(start_date, '+0 days') IS NOT NULL AND date(start_date, '+0 days') = start_date),
    end_date TEXT NOT NULL CHECK (
        end_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        AND end_date BETWEEN '0001-01-01' AND '9999-12-31'
        AND date(end_date, '+0 days') IS NOT NULL AND date(end_date, '+0 days') = end_date),
    is_full_day INTEGER NOT NULL CHECK (typeof(is_full_day) = 'integer' AND is_full_day IN (0, 1)),
    start_time TEXT NULL,
    end_time TEXT NULL,
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (start_date <= end_date),
    CHECK ((is_full_day = 1 AND start_time IS NULL AND end_time IS NULL)
        OR (is_full_day = 0 AND start_time IS NOT NULL AND end_time IS NOT NULL
            AND start_time GLOB '[0-2][0-9][0-5][0-9]' AND start_time <= '2359'
            AND end_time GLOB '[0-2][0-9][0-5][0-9]' AND end_time <= '2359'
            AND start_time <> end_time)),
    CHECK (end_date < '9999-12-31' OR (is_full_day = 0 AND end_time > start_time)),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);

CREATE INDEX pto_entries_employee_dates ON pto_entries (employee_id, archived_at, active_status, start_date, end_date, status, id);
CREATE INDEX pto_entries_dates_type_status ON pto_entries (archived_at, active_status, status, start_date, end_date, id);
