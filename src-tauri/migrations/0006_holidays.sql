CREATE TABLE holidays (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) BETWEEN 1 AND 128),
    seed_key TEXT NULL CHECK (seed_key IS NULL OR length(trim(seed_key)) BETWEEN 1 AND 120),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    name_order_key TEXT NOT NULL,
    holiday_set TEXT NOT NULL CHECK (holiday_set IN ('a', 'b', 'none')),
    date_kind TEXT NOT NULL CHECK (date_kind IN ('fixed', 'nth_weekday', 'last_weekday', 'exact_date')),
    month INTEGER NULL CHECK (month IS NULL OR (typeof(month) = 'integer' AND month BETWEEN 1 AND 12)),
    day_of_month INTEGER NULL CHECK (day_of_month IS NULL OR (typeof(day_of_month) = 'integer' AND day_of_month BETWEEN 1 AND 31)),
    weekday TEXT NULL CHECK (weekday IS NULL OR weekday IN ('sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday')),
    nth INTEGER NULL CHECK (nth IS NULL OR (typeof(nth) = 'integer' AND nth BETWEEN 1 AND 4)),
    exact_date TEXT NULL CHECK (exact_date IS NULL OR (
        exact_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
        AND exact_date BETWEEN '0001-01-01' AND '9999-12-31'
        AND date(exact_date, '+0 days') IS NOT NULL AND date(exact_date, '+0 days') = exact_date)),
    coverage_behavior TEXT NOT NULL CHECK (coverage_behavior IN ('normal', 'custom')),
    custom_coverage_template_id TEXT NULL REFERENCES coverage_templates(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (archived_at IS NULL OR active_status = 'inactive'),
    CHECK (
        (date_kind = 'fixed' AND month IS NOT NULL AND day_of_month IS NOT NULL
            AND weekday IS NULL AND nth IS NULL AND exact_date IS NULL
            AND day_of_month <= CASE month WHEN 2 THEN 28 WHEN 4 THEN 30 WHEN 6 THEN 30 WHEN 9 THEN 30 WHEN 11 THEN 30 ELSE 31 END)
        OR (date_kind = 'nth_weekday' AND month IS NOT NULL AND weekday IS NOT NULL AND nth IS NOT NULL AND day_of_month IS NULL AND exact_date IS NULL)
        OR (date_kind = 'last_weekday' AND month IS NOT NULL AND weekday IS NOT NULL AND day_of_month IS NULL AND nth IS NULL AND exact_date IS NULL)
        OR (date_kind = 'exact_date' AND exact_date IS NOT NULL AND month IS NULL AND day_of_month IS NULL AND weekday IS NULL AND nth IS NULL)),
    CHECK ((coverage_behavior = 'normal' AND custom_coverage_template_id IS NULL)
        OR (coverage_behavior = 'custom' AND custom_coverage_template_id IS NOT NULL))
);
CREATE UNIQUE INDEX holidays_seed_key ON holidays(seed_key) WHERE seed_key IS NOT NULL;
CREATE INDEX holidays_status_order ON holidays(archived_at, active_status, name_order_key, id);
CREATE INDEX holidays_active_rule ON holidays(date_kind, exact_date, month, day_of_month, id)
    WHERE active_status = 'active' AND archived_at IS NULL;
CREATE INDEX holidays_custom_coverage ON holidays(custom_coverage_template_id, active_status, archived_at, id);
CREATE TRIGGER holidays_immutable_seed_key BEFORE UPDATE OF seed_key ON holidays
WHEN OLD.seed_key IS NOT NEW.seed_key BEGIN
    SELECT RAISE(ABORT, 'Holiday seed keys are immutable');
END;

-- Retained inactive/archived references require only the restrictive FK. Active
-- configuration additionally requires a currently usable populated custom source.
CREATE TRIGGER holidays_insert_custom_source BEFORE INSERT ON holidays
WHEN NEW.active_status = 'active' AND NEW.archived_at IS NULL AND NEW.coverage_behavior = 'custom' BEGIN
    SELECT RAISE(ABORT, 'Active holidays require populated active custom coverage')
    WHERE NOT EXISTS (SELECT 1 FROM coverage_templates t WHERE t.id = NEW.custom_coverage_template_id
        AND t.day_type = 'holiday_custom' AND t.active_status = 'active' AND t.archived_at IS NULL
        AND EXISTS (SELECT 1 FROM coverage_template_items i WHERE i.coverage_template_id = t.id AND i.active_status = 'active' AND i.archived_at IS NULL));
END;
CREATE TRIGGER holidays_update_custom_source BEFORE UPDATE ON holidays
WHEN NEW.active_status = 'active' AND NEW.archived_at IS NULL AND NEW.coverage_behavior = 'custom' BEGIN
    SELECT RAISE(ABORT, 'Active holidays require populated active custom coverage')
    WHERE NOT EXISTS (SELECT 1 FROM coverage_templates t WHERE t.id = NEW.custom_coverage_template_id
        AND t.day_type = 'holiday_custom' AND t.active_status = 'active' AND t.archived_at IS NULL
        AND EXISTS (SELECT 1 FROM coverage_template_items i WHERE i.coverage_template_id = t.id AND i.active_status = 'active' AND i.archived_at IS NULL));
END;
CREATE TRIGGER coverage_templates_active_holidays_guard BEFORE UPDATE ON coverage_templates
WHEN NEW.day_type <> 'holiday_custom' OR NEW.active_status <> 'active' OR NEW.archived_at IS NOT NULL BEGIN
    SELECT RAISE(ABORT, 'Active holidays prevent this coverage change')
    WHERE EXISTS (SELECT 1 FROM holidays h WHERE h.custom_coverage_template_id = OLD.id AND h.active_status = 'active' AND h.archived_at IS NULL);
END;
CREATE TRIGGER coverage_items_active_holidays_update_guard AFTER UPDATE ON coverage_template_items BEGIN
    SELECT RAISE(ABORT, 'Active holidays prevent empty custom coverage')
    WHERE EXISTS (SELECT 1 FROM holidays h WHERE h.custom_coverage_template_id = OLD.coverage_template_id AND h.active_status = 'active' AND h.archived_at IS NULL)
        AND NOT EXISTS (SELECT 1 FROM coverage_template_items i WHERE i.coverage_template_id = OLD.coverage_template_id AND i.active_status = 'active' AND i.archived_at IS NULL);
END;
CREATE TRIGGER coverage_items_active_holidays_delete_guard AFTER DELETE ON coverage_template_items BEGIN
    SELECT RAISE(ABORT, 'Active holidays prevent empty custom coverage')
    WHERE EXISTS (SELECT 1 FROM holidays h WHERE h.custom_coverage_template_id = OLD.coverage_template_id AND h.active_status = 'active' AND h.archived_at IS NULL)
        AND NOT EXISTS (SELECT 1 FROM coverage_template_items i WHERE i.coverage_template_id = OLD.coverage_template_id AND i.active_status = 'active' AND i.archived_at IS NULL);
END;
