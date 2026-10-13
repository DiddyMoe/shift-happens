CREATE TABLE shift_positions (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    seed_key TEXT NULL CHECK (seed_key IS NULL OR length(trim(seed_key)) BETWEEN 1 AND 120),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    name_order_key TEXT NOT NULL,
    display_label TEXT NULL CHECK (display_label IS NULL OR length(trim(display_label)) BETWEEN 1 AND 120),
    sort_order INTEGER NOT NULL CHECK (typeof(sort_order) = 'integer' AND sort_order BETWEEN -9007199254740991 AND 9007199254740991),
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);
CREATE UNIQUE INDEX shift_positions_seed_key ON shift_positions(seed_key) WHERE seed_key IS NOT NULL;
CREATE INDEX shift_positions_status_order ON shift_positions(archived_at, active_status, sort_order, name_order_key, id);
CREATE TRIGGER shift_positions_immutable_seed_key BEFORE UPDATE OF seed_key ON shift_positions
WHEN OLD.seed_key IS NOT NEW.seed_key BEGIN
    SELECT RAISE(ABORT, 'Position seed keys are immutable');
END;

CREATE TABLE coverage_templates (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    seed_key TEXT NULL CHECK (seed_key IS NULL OR length(trim(seed_key)) BETWEEN 1 AND 120),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    name_order_key TEXT NOT NULL,
    day_type TEXT NOT NULL CHECK (day_type IN ('monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday', 'holiday_custom')),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (typeof(is_default) = 'integer' AND is_default IN (0, 1)),
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (archived_at IS NULL OR active_status = 'inactive'),
    CHECK (is_default = 0 OR (day_type <> 'holiday_custom' AND active_status = 'active' AND archived_at IS NULL))
);
CREATE UNIQUE INDEX coverage_templates_seed_key ON coverage_templates(seed_key) WHERE seed_key IS NOT NULL;
CREATE UNIQUE INDEX coverage_templates_weekday_default ON coverage_templates(day_type)
    WHERE is_default = 1 AND active_status = 'active' AND archived_at IS NULL AND day_type <> 'holiday_custom';
CREATE INDEX coverage_templates_day_status_order ON coverage_templates(day_type, archived_at, active_status, name_order_key, id);
CREATE TRIGGER coverage_templates_immutable_seed_key BEFORE UPDATE OF seed_key ON coverage_templates
WHEN OLD.seed_key IS NOT NEW.seed_key BEGIN
    SELECT RAISE(ABORT, 'Coverage template seed keys are immutable');
END;

CREATE TABLE coverage_template_items (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    seed_key TEXT NULL CHECK (seed_key IS NULL OR length(trim(seed_key)) BETWEEN 1 AND 120),
    coverage_template_id TEXT NOT NULL REFERENCES coverage_templates(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    shift_position_id TEXT NOT NULL REFERENCES shift_positions(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    shift_template_id TEXT NOT NULL REFERENCES shift_templates(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    display_label TEXT NULL CHECK (display_label IS NULL OR length(trim(display_label)) BETWEEN 1 AND 120),
    sort_order INTEGER NOT NULL CHECK (typeof(sort_order) = 'integer' AND sort_order BETWEEN -9007199254740991 AND 9007199254740991),
    required_count INTEGER NOT NULL CHECK (typeof(required_count) = 'integer' AND required_count BETWEEN 1 AND 100),
    actual_start_day_offset INTEGER NOT NULL CHECK (typeof(actual_start_day_offset) = 'integer'),
    actual_end_day_offset INTEGER NOT NULL CHECK (typeof(actual_end_day_offset) = 'integer'),
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK ((actual_start_day_offset = 0 AND actual_end_day_offset IN (0, 1)) OR (actual_start_day_offset = 1 AND actual_end_day_offset = 1)),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);
CREATE UNIQUE INDEX coverage_template_items_seed_key ON coverage_template_items(seed_key) WHERE seed_key IS NOT NULL;
CREATE UNIQUE INDEX coverage_template_items_active_structure
    ON coverage_template_items(coverage_template_id, shift_position_id, shift_template_id, actual_start_day_offset, actual_end_day_offset)
    WHERE active_status = 'active' AND archived_at IS NULL;
CREATE INDEX coverage_template_items_template_status_order ON coverage_template_items(coverage_template_id, archived_at, active_status, sort_order, id);
CREATE INDEX coverage_template_items_position_status ON coverage_template_items(shift_position_id, archived_at, active_status, id);
CREATE INDEX coverage_template_items_shift_status ON coverage_template_items(shift_template_id, archived_at, active_status, id);
CREATE TRIGGER coverage_template_items_immutable_seed_key BEFORE UPDATE OF seed_key ON coverage_template_items
WHEN OLD.seed_key IS NOT NEW.seed_key BEGIN
    SELECT RAISE(ABORT, 'Coverage item seed keys are immutable');
END;

-- Current-source checks apply to active configuration only. Retained historical
-- items are deliberately unaffected by later source time/status changes.
CREATE TRIGGER coverage_template_items_insert_sources BEFORE INSERT ON coverage_template_items
WHEN NEW.active_status = 'active' AND NEW.archived_at IS NULL BEGIN
    SELECT RAISE(ABORT, 'Active coverage requires active sources and valid explicit offsets')
    WHERE NOT EXISTS (SELECT 1 FROM coverage_templates WHERE id = NEW.coverage_template_id AND active_status = 'active' AND archived_at IS NULL)
        OR NOT EXISTS (SELECT 1 FROM shift_positions WHERE id = NEW.shift_position_id AND active_status = 'active' AND archived_at IS NULL)
        OR NOT EXISTS (SELECT 1 FROM shift_templates WHERE id = NEW.shift_template_id AND active_status = 'active' AND archived_at IS NULL
            AND NEW.actual_end_day_offset - NEW.actual_start_day_offset = CASE WHEN end_time < start_time THEN 1 ELSE 0 END);
END;
CREATE TRIGGER coverage_template_items_update_sources BEFORE UPDATE ON coverage_template_items
WHEN NEW.active_status = 'active' AND NEW.archived_at IS NULL BEGIN
    SELECT RAISE(ABORT, 'Active coverage requires active sources and valid explicit offsets')
    WHERE NOT EXISTS (SELECT 1 FROM coverage_templates WHERE id = NEW.coverage_template_id AND active_status = 'active' AND archived_at IS NULL)
        OR NOT EXISTS (SELECT 1 FROM shift_positions WHERE id = NEW.shift_position_id AND active_status = 'active' AND archived_at IS NULL)
        OR NOT EXISTS (SELECT 1 FROM shift_templates WHERE id = NEW.shift_template_id AND active_status = 'active' AND archived_at IS NULL
            AND NEW.actual_end_day_offset - NEW.actual_start_day_offset = CASE WHEN end_time < start_time THEN 1 ELSE 0 END);
END;
CREATE TRIGGER shift_templates_active_coverage_guard BEFORE UPDATE ON shift_templates BEGIN
    SELECT RAISE(ABORT, 'Active coverage prevents this shift change')
    WHERE EXISTS (SELECT 1 FROM coverage_template_items WHERE shift_template_id = OLD.id AND active_status = 'active' AND archived_at IS NULL
        AND (NEW.active_status <> 'active' OR NEW.archived_at IS NOT NULL
            OR actual_end_day_offset - actual_start_day_offset <> CASE WHEN NEW.end_time < NEW.start_time THEN 1 ELSE 0 END));
END;
CREATE TRIGGER shift_positions_active_coverage_guard BEFORE UPDATE ON shift_positions
WHEN NEW.active_status <> 'active' OR NEW.archived_at IS NOT NULL BEGIN
    SELECT RAISE(ABORT, 'Active coverage prevents position retirement')
    WHERE EXISTS (SELECT 1 FROM coverage_template_items WHERE shift_position_id = OLD.id AND active_status = 'active' AND archived_at IS NULL);
END;
CREATE TRIGGER coverage_templates_active_items_guard BEFORE UPDATE ON coverage_templates
WHEN NEW.active_status <> 'active' OR NEW.archived_at IS NOT NULL BEGIN
    SELECT RAISE(ABORT, 'Active items prevent coverage template retirement')
    WHERE EXISTS (SELECT 1 FROM coverage_template_items WHERE coverage_template_id = OLD.id AND active_status = 'active' AND archived_at IS NULL);
END;

CREATE TABLE employee_position_restrictions (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    employee_id TEXT NOT NULL REFERENCES employees(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    shift_position_id TEXT NOT NULL REFERENCES shift_positions(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    restriction_type TEXT NOT NULL CHECK (restriction_type = 'cannot_work'),
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);
CREATE INDEX employee_position_restrictions_employee_status ON employee_position_restrictions(employee_id, archived_at, active_status, shift_position_id, id);
CREATE INDEX employee_position_restrictions_position ON employee_position_restrictions(shift_position_id, id);
CREATE UNIQUE INDEX employee_position_restrictions_active ON employee_position_restrictions(employee_id, shift_position_id)
    WHERE active_status = 'active' AND archived_at IS NULL;
CREATE TRIGGER employee_position_restrictions_insert_sources BEFORE INSERT ON employee_position_restrictions BEGIN
    SELECT RAISE(ABORT, 'New position restrictions require active sources')
    WHERE NOT EXISTS (SELECT 1 FROM employees WHERE id = NEW.employee_id AND active_status = 'active' AND archived_at IS NULL)
        OR NOT EXISTS (SELECT 1 FROM shift_positions WHERE id = NEW.shift_position_id AND active_status = 'active' AND archived_at IS NULL);
END;
CREATE TRIGGER employee_position_restrictions_update_sources BEFORE UPDATE ON employee_position_restrictions
WHEN NEW.shift_position_id IS NOT OLD.shift_position_id OR NEW.employee_id IS NOT OLD.employee_id
    OR (OLD.active_status = 'inactive' AND NEW.active_status = 'active') BEGIN
    SELECT RAISE(ABORT, 'Position restriction changes require active sources')
    WHERE NOT EXISTS (SELECT 1 FROM employees WHERE id = NEW.employee_id AND active_status = 'active' AND archived_at IS NULL)
        OR NOT EXISTS (SELECT 1 FROM shift_positions WHERE id = NEW.shift_position_id AND active_status = 'active' AND archived_at IS NULL);
END;
