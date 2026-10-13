CREATE TABLE shift_templates (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    seed_key TEXT NULL CHECK (seed_key IS NULL OR length(trim(seed_key)) BETWEEN 1 AND 120),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    name_order_key TEXT NOT NULL,
    display_label TEXT NULL CHECK (display_label IS NULL OR length(trim(display_label)) BETWEEN 1 AND 120),
    start_time TEXT NOT NULL CHECK (length(CAST(start_time AS BLOB)) = 4 AND start_time GLOB '[0-2][0-9][0-5][0-9]' AND start_time <= '2359'),
    end_time TEXT NOT NULL CHECK (length(CAST(end_time AS BLOB)) = 4 AND end_time GLOB '[0-2][0-9][0-5][0-9]' AND end_time <= '2359'),
    time_of_day TEXT NOT NULL CHECK (time_of_day IN ('day', 'evening', 'night')),
    unpaid_lunch_minutes INTEGER NOT NULL CHECK (typeof(unpaid_lunch_minutes) = 'integer' AND unpaid_lunch_minutes IN (0, 30)),
    paid_minutes INTEGER NOT NULL CHECK (typeof(paid_minutes) = 'integer' AND paid_minutes BETWEEN 1 AND 1439),
    requires_recovery_after_shift INTEGER NOT NULL CHECK (typeof(requires_recovery_after_shift) = 'integer' AND requires_recovery_after_shift IN (0, 1)),
    is_night_shift INTEGER NOT NULL CHECK (typeof(is_night_shift) = 'integer' AND is_night_shift IN (0, 1)),
    color_hex TEXT NULL CHECK (color_hex IS NULL OR (length(color_hex) = 7 AND length(CAST(color_hex AS BLOB)) = 7 AND substr(color_hex, 1, 1) = '#' AND substr(color_hex, 2) NOT GLOB '*[^0-9A-F]*')),
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (start_time <> end_time),
    -- Canonical HHMM clocks guarantee a duration in 1..1439 after one crossing.
    CHECK (unpaid_lunch_minutes = CASE WHEN
        ((CAST(substr(end_time, 1, 2) AS INTEGER) * 60 + CAST(substr(end_time, 3, 2) AS INTEGER)
        - CAST(substr(start_time, 1, 2) AS INTEGER) * 60 - CAST(substr(start_time, 3, 2) AS INTEGER) + 1440) % 1440) >= 480
        THEN 30 ELSE 0 END),
    CHECK (paid_minutes =
        ((CAST(substr(end_time, 1, 2) AS INTEGER) * 60 + CAST(substr(end_time, 3, 2) AS INTEGER)
        - CAST(substr(start_time, 1, 2) AS INTEGER) * 60 - CAST(substr(start_time, 3, 2) AS INTEGER) + 1440) % 1440)
        - unpaid_lunch_minutes),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);

CREATE UNIQUE INDEX shift_templates_seed_key ON shift_templates(seed_key) WHERE seed_key IS NOT NULL;
CREATE INDEX shift_templates_status_order ON shift_templates(archived_at, active_status, name_order_key, id);
CREATE INDEX shift_templates_order ON shift_templates(name_order_key, id);

CREATE TRIGGER shift_templates_immutable_seed_key
BEFORE UPDATE OF seed_key ON shift_templates
WHEN OLD.seed_key IS NOT NEW.seed_key
BEGIN
    SELECT RAISE(ABORT, 'Shift template seed keys are immutable');
END;

CREATE TABLE employee_shift_template_restrictions (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    employee_id TEXT NOT NULL REFERENCES employees(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    shift_template_id TEXT NOT NULL REFERENCES shift_templates(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    restriction_type TEXT NOT NULL CHECK (restriction_type = 'cannot_work'),
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);

CREATE INDEX employee_shift_template_restrictions_employee_status
    ON employee_shift_template_restrictions(employee_id, archived_at, active_status, shift_template_id, id);
CREATE INDEX employee_shift_template_restrictions_template
    ON employee_shift_template_restrictions(shift_template_id, id);
CREATE UNIQUE INDEX employee_shift_template_restrictions_active
    ON employee_shift_template_restrictions(employee_id, shift_template_id)
    WHERE active_status = 'active' AND archived_at IS NULL;

-- New links and reactivations must use current sources. Retained exclusions may
-- remain after either source retires, without blocking historical updates/archive.
CREATE TRIGGER employee_shift_template_restrictions_insert_sources
BEFORE INSERT ON employee_shift_template_restrictions
WHEN NOT EXISTS (SELECT 1 FROM employees WHERE id = NEW.employee_id AND active_status = 'active' AND archived_at IS NULL)
    OR NOT EXISTS (SELECT 1 FROM shift_templates WHERE id = NEW.shift_template_id AND active_status = 'active' AND archived_at IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'New template restrictions require active sources');
END;

CREATE TRIGGER employee_shift_template_restrictions_update_sources
BEFORE UPDATE ON employee_shift_template_restrictions
WHEN (NEW.employee_id IS NOT OLD.employee_id OR NEW.shift_template_id IS NOT OLD.shift_template_id
    OR (OLD.active_status = 'inactive' AND NEW.active_status = 'active'))
    AND (NOT EXISTS (SELECT 1 FROM employees WHERE id = NEW.employee_id AND active_status = 'active' AND archived_at IS NULL)
        OR NOT EXISTS (SELECT 1 FROM shift_templates WHERE id = NEW.shift_template_id AND active_status = 'active' AND archived_at IS NULL))
BEGIN
    SELECT RAISE(ABORT, 'Template restriction changes require active sources');
END;
