-- No retained migration introduced a day-label column. Weekdays are derived in Rust.
CREATE TABLE schedules (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) BETWEEN 1 AND 128),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    start_date TEXT NOT NULL CHECK (length(start_date) = 10 AND start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND start_date BETWEEN '0001-01-01' AND '9999-12-31' AND date(start_date, '+0 days') IS NOT NULL AND date(start_date, '+0 days') = start_date),
    end_date TEXT NOT NULL CHECK (length(end_date) = 10 AND end_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND end_date BETWEEN '0001-01-01' AND '9999-12-31' AND date(end_date, '+0 days') IS NOT NULL AND date(end_date, '+0 days') = end_date),
    status TEXT NOT NULL CHECK (status IN ('draft', 'final')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    generation_settings_snapshot TEXT NOT NULL CHECK (CASE WHEN json_valid(generation_settings_snapshot) THEN
        json(generation_settings_snapshot) = generation_settings_snapshot
        AND json_type(generation_settings_snapshot) = 'object'
        AND json_type(generation_settings_snapshot, '$.version') IS 'integer'
        AND json_extract(generation_settings_snapshot, '$.version') = 1
        AND json_type(generation_settings_snapshot, '$.expected_assignment_count') IS 'integer'
        AND json_extract(generation_settings_snapshot, '$.expected_assignment_count') BETWEEN 1 AND 100000
        ELSE 0 END),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (start_date <= end_date)
);
CREATE INDEX schedules_visibility_order ON schedules(archived_at, status, created_at, id);
CREATE INDEX schedules_created_order ON schedules(created_at, id);
CREATE INDEX schedules_range ON schedules(start_date, end_date, id);

CREATE TRIGGER schedules_immutable_structure BEFORE UPDATE ON schedules
WHEN NEW.id IS NOT OLD.id OR NEW.start_date IS NOT OLD.start_date OR NEW.end_date IS NOT OLD.end_date
    OR NEW.generation_settings_snapshot IS NOT OLD.generation_settings_snapshot OR NEW.created_at IS NOT OLD.created_at
BEGIN SELECT RAISE(ABORT, 'Schedule structure is immutable'); END;
CREATE TRIGGER schedules_readonly_metadata BEFORE UPDATE ON schedules
WHEN (OLD.archived_at IS NOT NULL)
    OR ((NEW.name IS NOT OLD.name OR NEW.notes IS NOT OLD.notes) AND OLD.status <> 'draft')
BEGIN SELECT RAISE(ABORT, 'Only non-archived drafts have editable metadata'); END;
CREATE TRIGGER schedules_no_delete BEFORE DELETE ON schedules
BEGIN SELECT RAISE(ABORT, 'Schedules must be archived'); END;

CREATE TABLE schedule_assignments (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) BETWEEN 1 AND 128),
    schedule_id TEXT NOT NULL REFERENCES schedules(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    schedule_date TEXT NOT NULL CHECK (length(schedule_date) = 10 AND schedule_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND schedule_date BETWEEN '0001-01-01' AND '9999-12-31' AND date(schedule_date, '+0 days') IS NOT NULL AND date(schedule_date, '+0 days') = schedule_date),
    actual_start_datetime TEXT NOT NULL CHECK (length(actual_start_datetime) = 19 AND actual_start_datetime GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-2][0-9]:[0-5][0-9]:00' AND substr(actual_start_datetime,1,10) BETWEEN '0001-01-01' AND '9999-12-31' AND substr(actual_start_datetime,12,2) <= '23' AND datetime(actual_start_datetime, '+0 days') IS NOT NULL AND datetime(actual_start_datetime, '+0 days') = replace(actual_start_datetime, 'T', ' ')),
    actual_end_datetime TEXT NOT NULL CHECK (length(actual_end_datetime) = 19 AND actual_end_datetime GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-2][0-9]:[0-5][0-9]:00' AND substr(actual_end_datetime,1,10) BETWEEN '0001-01-01' AND '9999-12-31' AND substr(actual_end_datetime,12,2) <= '23' AND datetime(actual_end_datetime, '+0 days') IS NOT NULL AND datetime(actual_end_datetime, '+0 days') = replace(actual_end_datetime, 'T', ' ')),
    coverage_template_id TEXT NOT NULL REFERENCES coverage_templates(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    coverage_item_id TEXT NOT NULL REFERENCES coverage_template_items(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    shift_position_id TEXT NOT NULL REFERENCES shift_positions(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    shift_template_id TEXT NOT NULL REFERENCES shift_templates(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    coverage_template_seed_key_snapshot TEXT NULL CHECK (coverage_template_seed_key_snapshot IS NULL OR length(trim(coverage_template_seed_key_snapshot)) BETWEEN 1 AND 120),
    coverage_template_name_snapshot TEXT NOT NULL CHECK (length(trim(coverage_template_name_snapshot)) BETWEEN 1 AND 120),
    coverage_day_type_snapshot TEXT NOT NULL CHECK (coverage_day_type_snapshot IN ('monday','tuesday','wednesday','thursday','friday','saturday','sunday','holiday_custom')),
    coverage_item_seed_key_snapshot TEXT NULL CHECK (coverage_item_seed_key_snapshot IS NULL OR length(trim(coverage_item_seed_key_snapshot)) BETWEEN 1 AND 120),
    coverage_item_display_snapshot TEXT NULL CHECK (coverage_item_display_snapshot IS NULL OR length(trim(coverage_item_display_snapshot)) BETWEEN 1 AND 120),
    coverage_sort_order_snapshot INTEGER NOT NULL CHECK (typeof(coverage_sort_order_snapshot) = 'integer' AND coverage_sort_order_snapshot BETWEEN -9007199254740991 AND 9007199254740991),
    coverage_required_count_snapshot INTEGER NOT NULL CHECK (typeof(coverage_required_count_snapshot) = 'integer' AND coverage_required_count_snapshot BETWEEN 1 AND 100),
    coverage_slot_index_snapshot INTEGER NOT NULL CHECK (typeof(coverage_slot_index_snapshot) = 'integer' AND coverage_slot_index_snapshot BETWEEN 1 AND coverage_required_count_snapshot),
    coverage_start_offset_snapshot INTEGER NOT NULL CHECK (typeof(coverage_start_offset_snapshot) = 'integer' AND coverage_start_offset_snapshot IN (0,1)),
    coverage_end_offset_snapshot INTEGER NOT NULL CHECK (typeof(coverage_end_offset_snapshot) = 'integer' AND coverage_end_offset_snapshot IN (0,1)),
    position_seed_key_snapshot TEXT NULL CHECK (position_seed_key_snapshot IS NULL OR length(trim(position_seed_key_snapshot)) BETWEEN 1 AND 120),
    position_name_snapshot TEXT NOT NULL CHECK (length(trim(position_name_snapshot)) BETWEEN 1 AND 120),
    position_display_snapshot TEXT NULL CHECK (position_display_snapshot IS NULL OR length(trim(position_display_snapshot)) BETWEEN 1 AND 120),
    shift_seed_key_snapshot TEXT NULL CHECK (shift_seed_key_snapshot IS NULL OR length(trim(shift_seed_key_snapshot)) BETWEEN 1 AND 120),
    shift_name_snapshot TEXT NOT NULL CHECK (length(trim(shift_name_snapshot)) BETWEEN 1 AND 120),
    shift_display_snapshot TEXT NULL CHECK (shift_display_snapshot IS NULL OR length(trim(shift_display_snapshot)) BETWEEN 1 AND 120),
    shift_start_snapshot TEXT NOT NULL CHECK (length(CAST(shift_start_snapshot AS BLOB)) = 4 AND shift_start_snapshot GLOB '[0-2][0-9][0-5][0-9]' AND shift_start_snapshot <= '2359'),
    shift_end_snapshot TEXT NOT NULL CHECK (length(CAST(shift_end_snapshot AS BLOB)) = 4 AND shift_end_snapshot GLOB '[0-2][0-9][0-5][0-9]' AND shift_end_snapshot <= '2359'),
    shift_category_snapshot TEXT NOT NULL CHECK (shift_category_snapshot IN ('day','evening','night')),
    shift_color_snapshot TEXT NULL CHECK (shift_color_snapshot IS NULL OR (length(CAST(shift_color_snapshot AS BLOB)) = 7 AND substr(shift_color_snapshot,1,1) = '#' AND substr(shift_color_snapshot,2) NOT GLOB '*[^0-9A-F]*')),
    shift_lunch_minutes_snapshot INTEGER NOT NULL CHECK (typeof(shift_lunch_minutes_snapshot) = 'integer' AND shift_lunch_minutes_snapshot IN (0,30)),
    shift_paid_minutes_snapshot INTEGER NOT NULL CHECK (typeof(shift_paid_minutes_snapshot) = 'integer' AND shift_paid_minutes_snapshot BETWEEN 1 AND 1439),
    shift_recovery_snapshot INTEGER NOT NULL CHECK (typeof(shift_recovery_snapshot) = 'integer' AND shift_recovery_snapshot IN (0,1)),
    shift_night_snapshot INTEGER NOT NULL CHECK (typeof(shift_night_snapshot) = 'integer' AND shift_night_snapshot IN (0,1)),
    holiday_id TEXT NULL REFERENCES holidays(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    holiday_seed_key_snapshot TEXT NULL CHECK (holiday_seed_key_snapshot IS NULL OR length(trim(holiday_seed_key_snapshot)) BETWEEN 1 AND 120),
    holiday_name_snapshot TEXT NULL CHECK (holiday_name_snapshot IS NULL OR length(trim(holiday_name_snapshot)) BETWEEN 1 AND 120),
    holiday_set_snapshot TEXT NULL CHECK (holiday_set_snapshot IN ('a','b','none')),
    holiday_coverage_behavior_snapshot TEXT NULL CHECK (holiday_coverage_behavior_snapshot IN ('normal','custom')),
    assigned_employee_id TEXT NULL REFERENCES employees(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    employee_first_name_snapshot TEXT NULL CHECK (employee_first_name_snapshot IS NULL OR length(trim(employee_first_name_snapshot)) BETWEEN 1 AND 120),
    employee_last_name_snapshot TEXT NULL CHECK (employee_last_name_snapshot IS NULL OR length(trim(employee_last_name_snapshot)) BETWEEN 1 AND 120),
    employee_display_name_snapshot TEXT NULL CHECK (employee_display_name_snapshot IS NULL OR length(trim(employee_display_name_snapshot)) BETWEEN 1 AND 120),
    locked INTEGER NOT NULL CHECK (typeof(locked) = 'integer' AND locked IN (0,1)),
    open_shift INTEGER NOT NULL CHECK (typeof(open_shift) = 'integer' AND open_shift IN (0,1)),
    manually_edited INTEGER NOT NULL CHECK (typeof(manually_edited) = 'integer' AND manually_edited IN (0,1)),
    assignment_source TEXT NOT NULL CHECK (assignment_source IN ('generated','manual')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (actual_end_datetime > actual_start_datetime),
    CHECK (shift_start_snapshot <> shift_end_snapshot),
    CHECK ((coverage_start_offset_snapshot = 0 AND coverage_end_offset_snapshot IN (0,1)) OR (coverage_start_offset_snapshot = 1 AND coverage_end_offset_snapshot = 1)),
    CHECK (coverage_end_offset_snapshot - coverage_start_offset_snapshot = CASE WHEN shift_end_snapshot < shift_start_snapshot THEN 1 ELSE 0 END),
    CHECK (actual_start_datetime = strftime('%Y-%m-%dT%H:%M:%S', schedule_date || 'T' || substr(shift_start_snapshot,1,2) || ':' || substr(shift_start_snapshot,3,2) || ':00', '+' || coverage_start_offset_snapshot || ' days')),
    CHECK (actual_end_datetime = strftime('%Y-%m-%dT%H:%M:%S', schedule_date || 'T' || substr(shift_end_snapshot,1,2) || ':' || substr(shift_end_snapshot,3,2) || ':00', '+' || coverage_end_offset_snapshot || ' days')),
    CHECK ((open_shift = 1 AND assigned_employee_id IS NULL AND employee_first_name_snapshot IS NULL AND employee_last_name_snapshot IS NULL AND employee_display_name_snapshot IS NULL) OR (open_shift = 0 AND assigned_employee_id IS NOT NULL AND employee_first_name_snapshot IS NOT NULL AND employee_last_name_snapshot IS NOT NULL AND employee_display_name_snapshot IS NOT NULL)),
    CHECK ((holiday_id IS NULL AND holiday_seed_key_snapshot IS NULL AND holiday_name_snapshot IS NULL AND holiday_set_snapshot IS NULL AND holiday_coverage_behavior_snapshot IS NULL) OR (holiday_id IS NOT NULL AND holiday_name_snapshot IS NOT NULL AND holiday_set_snapshot IS NOT NULL AND holiday_coverage_behavior_snapshot IS NOT NULL)),
    CHECK (shift_lunch_minutes_snapshot = CASE WHEN ((CAST(substr(shift_end_snapshot,1,2) AS INTEGER)*60+CAST(substr(shift_end_snapshot,3,2) AS INTEGER)-CAST(substr(shift_start_snapshot,1,2) AS INTEGER)*60-CAST(substr(shift_start_snapshot,3,2) AS INTEGER)+1440)%1440) >= 480 THEN 30 ELSE 0 END),
    CHECK (shift_paid_minutes_snapshot = ((CAST(substr(shift_end_snapshot,1,2) AS INTEGER)*60+CAST(substr(shift_end_snapshot,3,2) AS INTEGER)-CAST(substr(shift_start_snapshot,1,2) AS INTEGER)*60-CAST(substr(shift_start_snapshot,3,2) AS INTEGER)+1440)%1440) - shift_lunch_minutes_snapshot)
);
CREATE UNIQUE INDEX schedule_assignments_slot ON schedule_assignments(schedule_id,schedule_date,coverage_item_id,coverage_slot_index_snapshot);
CREATE INDEX schedule_assignments_stable_order ON schedule_assignments(schedule_id,schedule_date,coverage_sort_order_snapshot,coverage_slot_index_snapshot,actual_start_datetime,id);
CREATE INDEX schedule_assignments_employee_interval ON schedule_assignments(assigned_employee_id,actual_start_datetime,actual_end_datetime,schedule_id,id);
CREATE INDEX schedule_assignments_operational_date ON schedule_assignments(schedule_date,schedule_id,id);
CREATE INDEX schedule_assignments_coverage_template_id ON schedule_assignments(coverage_template_id,id);
CREATE INDEX schedule_assignments_coverage_item_id ON schedule_assignments(coverage_item_id,id);
CREATE INDEX schedule_assignments_shift_position_id ON schedule_assignments(shift_position_id,id);
CREATE INDEX schedule_assignments_shift_template_id ON schedule_assignments(shift_template_id,id);
CREATE INDEX schedule_assignments_holiday_id ON schedule_assignments(holiday_id,id);

CREATE TRIGGER schedule_assignments_immutable_structure BEFORE UPDATE ON schedule_assignments
WHEN NEW.id IS NOT OLD.id
    OR NEW.schedule_id IS NOT OLD.schedule_id
    OR NEW.schedule_date IS NOT OLD.schedule_date
    OR NEW.actual_start_datetime IS NOT OLD.actual_start_datetime
    OR NEW.actual_end_datetime IS NOT OLD.actual_end_datetime
    OR NEW.coverage_template_id IS NOT OLD.coverage_template_id
    OR NEW.coverage_item_id IS NOT OLD.coverage_item_id
    OR NEW.shift_position_id IS NOT OLD.shift_position_id
    OR NEW.shift_template_id IS NOT OLD.shift_template_id
    OR NEW.coverage_template_seed_key_snapshot IS NOT OLD.coverage_template_seed_key_snapshot
    OR NEW.coverage_template_name_snapshot IS NOT OLD.coverage_template_name_snapshot
    OR NEW.coverage_day_type_snapshot IS NOT OLD.coverage_day_type_snapshot
    OR NEW.coverage_item_seed_key_snapshot IS NOT OLD.coverage_item_seed_key_snapshot
    OR NEW.coverage_item_display_snapshot IS NOT OLD.coverage_item_display_snapshot
    OR NEW.coverage_sort_order_snapshot IS NOT OLD.coverage_sort_order_snapshot
    OR NEW.coverage_required_count_snapshot IS NOT OLD.coverage_required_count_snapshot
    OR NEW.coverage_slot_index_snapshot IS NOT OLD.coverage_slot_index_snapshot
    OR NEW.coverage_start_offset_snapshot IS NOT OLD.coverage_start_offset_snapshot
    OR NEW.coverage_end_offset_snapshot IS NOT OLD.coverage_end_offset_snapshot
    OR NEW.position_seed_key_snapshot IS NOT OLD.position_seed_key_snapshot
    OR NEW.position_name_snapshot IS NOT OLD.position_name_snapshot
    OR NEW.position_display_snapshot IS NOT OLD.position_display_snapshot
    OR NEW.shift_seed_key_snapshot IS NOT OLD.shift_seed_key_snapshot
    OR NEW.shift_name_snapshot IS NOT OLD.shift_name_snapshot
    OR NEW.shift_display_snapshot IS NOT OLD.shift_display_snapshot
    OR NEW.shift_start_snapshot IS NOT OLD.shift_start_snapshot
    OR NEW.shift_end_snapshot IS NOT OLD.shift_end_snapshot
    OR NEW.shift_category_snapshot IS NOT OLD.shift_category_snapshot
    OR NEW.shift_color_snapshot IS NOT OLD.shift_color_snapshot
    OR NEW.shift_lunch_minutes_snapshot IS NOT OLD.shift_lunch_minutes_snapshot
    OR NEW.shift_paid_minutes_snapshot IS NOT OLD.shift_paid_minutes_snapshot
    OR NEW.shift_recovery_snapshot IS NOT OLD.shift_recovery_snapshot
    OR NEW.shift_night_snapshot IS NOT OLD.shift_night_snapshot
    OR NEW.holiday_id IS NOT OLD.holiday_id
    OR NEW.holiday_seed_key_snapshot IS NOT OLD.holiday_seed_key_snapshot
    OR NEW.holiday_name_snapshot IS NOT OLD.holiday_name_snapshot
    OR NEW.holiday_set_snapshot IS NOT OLD.holiday_set_snapshot
    OR NEW.holiday_coverage_behavior_snapshot IS NOT OLD.holiday_coverage_behavior_snapshot
    OR NEW.created_at IS NOT OLD.created_at
BEGIN SELECT RAISE(ABORT, 'Assignment structure and snapshots are immutable'); END;

CREATE TRIGGER schedule_assignments_insert_parent BEFORE INSERT ON schedule_assignments BEGIN
    SELECT RAISE(ABORT, 'Assignments require a draft parent and an in-range operational date')
    WHERE NOT EXISTS (SELECT 1 FROM schedules WHERE id = NEW.schedule_id AND status = 'draft' AND archived_at IS NULL AND NEW.schedule_date BETWEEN start_date AND end_date);
END;
CREATE TRIGGER schedule_assignments_insert_sources BEFORE INSERT ON schedule_assignments BEGIN
    SELECT RAISE(ABORT, 'New slots require linked active sources')
    WHERE NOT EXISTS (SELECT 1 FROM coverage_template_items i
        JOIN coverage_templates c ON c.id = i.coverage_template_id
        JOIN shift_positions p ON p.id = i.shift_position_id
        JOIN shift_templates s ON s.id = i.shift_template_id
        WHERE i.id = NEW.coverage_item_id AND c.id = NEW.coverage_template_id AND p.id = NEW.shift_position_id AND s.id = NEW.shift_template_id
        AND i.active_status = 'active' AND i.archived_at IS NULL AND c.active_status = 'active' AND c.archived_at IS NULL
        AND p.active_status = 'active' AND p.archived_at IS NULL AND s.active_status = 'active' AND s.archived_at IS NULL)
        OR (NEW.holiday_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM holidays WHERE id = NEW.holiday_id AND active_status = 'active' AND archived_at IS NULL))
        OR (NEW.assigned_employee_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM employees WHERE id = NEW.assigned_employee_id AND active_status = 'active' AND archived_at IS NULL));
END;
-- Later writes validate stored structure, never mutable current source fields.
CREATE TRIGGER schedule_assignments_update_parent BEFORE UPDATE ON schedule_assignments
WHEN NOT EXISTS (SELECT 1 FROM schedules WHERE id = OLD.schedule_id AND status = 'draft' AND archived_at IS NULL AND NEW.schedule_date BETWEEN start_date AND end_date)
BEGIN SELECT RAISE(ABORT, 'Only non-archived drafts have editable assignments'); END;
CREATE TRIGGER schedule_assignments_no_delete BEFORE DELETE ON schedule_assignments
BEGIN SELECT RAISE(ABORT, 'Structural slots cannot be deleted'); END;
