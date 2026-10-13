export const managerRoutes = [
  {
    path: "/",
    key: "schedule",
    label: "Schedule",
    detail:
      "The open plan on the board: edit, check issues, finalize, export and print",
  },
  {
    path: "/schedules",
    key: "schedules",
    label: "All schedules",
    detail: "Every saved plan, to find and open one",
  },
  {
    path: "/employees",
    key: "employees",
    label: "Employees",
    detail: "Employee records, employment rules, and recurring restrictions",
  },
  {
    path: "/availability-pto",
    key: "availability_pto",
    label: "Availability and PTO",
    detail: "Availability and time-off records",
  },
  {
    path: "/shift-templates",
    key: "shift_templates",
    label: "Shift templates",
    detail: "Shift templates and paid-time calculations",
  },
  {
    path: "/positions-coverage",
    key: "positions_coverage",
    label: "Positions and coverage",
    detail: "Positions and daily coverage templates",
  },
  {
    path: "/holidays",
    key: "holidays",
    label: "Holidays",
    detail: "Holiday rules, yearly rotation, and collision previews",
  },
  {
    path: "/backup",
    key: "backup",
    label: "Backup",
    detail: "Consistent local database backup",
  },
  {
    path: "/settings",
    key: "settings",
    label: "Settings",
    detail: "Workweek, payroll anchor, rest rules, and theme",
  },
  {
    path: "/about",
    key: "about",
    label: "About",
    detail: "Version, startup state, database health, and V1 limitations",
  },
] as const;

export type ManagerRoute = (typeof managerRoutes)[number];

/** Addresses of screens merged into Schedule, kept so old links still work. */
export const legacyRoutes = [
  "/schedule-editor",
  "/validation",
  "/generate-schedule",
] as const;
