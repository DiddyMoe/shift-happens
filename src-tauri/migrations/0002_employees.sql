CREATE TABLE employees (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    first_name TEXT NOT NULL CHECK (length(trim(first_name)) BETWEEN 1 AND 120),
    last_name TEXT NOT NULL CHECK (length(trim(last_name)) BETWEEN 1 AND 120),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 120),
    display_name_order_key TEXT NOT NULL,
    employment_type TEXT NOT NULL CHECK (employment_type IN ('full_time', 'part_time', 'prn')),
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    holiday_group TEXT NOT NULL CHECK (holiday_group IN ('a', 'b', 'none')),
    can_work_weekends INTEGER NOT NULL CHECK (typeof(can_work_weekends) = 'integer' AND can_work_weekends IN (0, 1)),
    can_work_holidays INTEGER NOT NULL CHECK (typeof(can_work_holidays) = 'integer' AND can_work_holidays IN (0, 1)),
    can_work_night_shift INTEGER NOT NULL CHECK (typeof(can_work_night_shift) = 'integer' AND can_work_night_shift IN (0, 1)),
    can_work_ortho_or INTEGER NOT NULL CHECK (typeof(can_work_ortho_or) = 'integer' AND can_work_ortho_or IN (0, 1)),
    preferred_time_of_day TEXT NULL CHECK (preferred_time_of_day IN ('day', 'evening', 'night')),
    requires_explicit_availability INTEGER NOT NULL CHECK (typeof(requires_explicit_availability) = 'integer' AND requires_explicit_availability IN (0, 1)),
    minimum_paid_minutes INTEGER NULL CHECK (minimum_paid_minutes IS NULL OR (typeof(minimum_paid_minutes) = 'integer' AND minimum_paid_minutes BETWEEN 1 AND 1000000)),
    minimum_assignment_count INTEGER NULL CHECK (minimum_assignment_count IS NULL OR (typeof(minimum_assignment_count) = 'integer' AND minimum_assignment_count BETWEEN 1 AND 1000000)),
    minimum_period TEXT NOT NULL CHECK (minimum_period IN ('week', 'two_week', 'none')),
    maximum_paid_minutes INTEGER NULL CHECK (maximum_paid_minutes IS NULL OR (typeof(maximum_paid_minutes) = 'integer' AND maximum_paid_minutes BETWEEN 1 AND 1000000)),
    maximum_period TEXT NOT NULL CHECK (maximum_period IN ('week', 'two_week', 'none')),
    weekend_rotation_pattern TEXT NOT NULL CHECK (weekend_rotation_pattern IN ('none', 'every_other_weekend')),
    weekend_rotation_anchor_date TEXT NULL,
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK (
        (minimum_period = 'none' AND minimum_paid_minutes IS NULL AND minimum_assignment_count IS NULL)
        OR (minimum_period IN ('week', 'two_week') AND (
            (minimum_paid_minutes IS NOT NULL AND minimum_assignment_count IS NULL)
            OR (minimum_assignment_count IS NOT NULL AND minimum_paid_minutes IS NULL)
        ))
    ),
    CHECK ((maximum_period = 'none' AND maximum_paid_minutes IS NULL)
        OR (maximum_period IN ('week', 'two_week') AND maximum_paid_minutes IS NOT NULL)),
    CHECK (minimum_paid_minutes IS NULL OR maximum_paid_minutes IS NULL
        OR minimum_period <> maximum_period OR minimum_paid_minutes <= maximum_paid_minutes),
    CHECK ((weekend_rotation_pattern = 'none' AND weekend_rotation_anchor_date IS NULL)
        OR (weekend_rotation_pattern = 'every_other_weekend' AND weekend_rotation_anchor_date IS NOT NULL
            AND weekend_rotation_anchor_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
            AND weekend_rotation_anchor_date BETWEEN '0001-01-01' AND '9999-12-31'
            AND date(weekend_rotation_anchor_date, '+0 days') IS NOT NULL
            AND date(weekend_rotation_anchor_date, '+0 days') = weekend_rotation_anchor_date
            AND strftime('%w', weekend_rotation_anchor_date) IS NOT NULL
            AND strftime('%w', weekend_rotation_anchor_date) = '6')),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);

CREATE INDEX employees_status_order ON employees (archived_at, active_status, display_name_order_key, id);
CREATE INDEX employees_order ON employees (display_name_order_key, id);

CREATE TABLE employee_restrictions (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(trim(id)) > 0),
    employee_id TEXT NOT NULL REFERENCES employees(id) ON DELETE RESTRICT ON UPDATE RESTRICT,
    restriction_kind TEXT NOT NULL CHECK (restriction_kind IN ('time_of_day_block', 'recurring_day_time_block')),
    time_of_day TEXT NULL CHECK (time_of_day IN ('day', 'evening', 'night')),
    weekday TEXT NULL CHECK (weekday IN ('sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday')),
    start_time TEXT NULL,
    end_time TEXT NULL,
    active_status TEXT NOT NULL CHECK (active_status IN ('active', 'inactive')),
    notes TEXT NULL CHECK (notes IS NULL OR length(notes) <= 4000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (typeof(revision) = 'integer' AND revision BETWEEN 1 AND 9007199254740991),
    CHECK ((restriction_kind = 'time_of_day_block' AND time_of_day IS NOT NULL
            AND weekday IS NULL AND start_time IS NULL AND end_time IS NULL)
        OR (restriction_kind = 'recurring_day_time_block' AND time_of_day IS NULL
            AND weekday IS NOT NULL AND start_time IS NOT NULL AND end_time IS NOT NULL
            AND start_time GLOB '[0-2][0-9][0-5][0-9]' AND start_time <= '2359'
            AND end_time GLOB '[0-2][0-9][0-5][0-9]' AND end_time <= '2359'
            AND start_time <> end_time)),
    CHECK (archived_at IS NULL OR active_status = 'inactive')
);

CREATE INDEX employee_restrictions_employee_status ON employee_restrictions (employee_id, archived_at, active_status, id);
CREATE UNIQUE INDEX employee_restrictions_active_category ON employee_restrictions (employee_id, time_of_day)
    WHERE restriction_kind = 'time_of_day_block' AND active_status = 'active' AND archived_at IS NULL;
CREATE UNIQUE INDEX employee_restrictions_active_recurring ON employee_restrictions (employee_id, weekday, start_time, end_time)
    WHERE restriction_kind = 'recurring_day_time_block' AND active_status = 'active' AND archived_at IS NULL;
