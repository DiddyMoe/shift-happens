// One offline content model for Help, the printed quick reference, and About.

/** One-time setup, in the order it is usually done. */
export const setupSteps = [
  {
    path: "/settings",
    title: "Check settings",
    detail: "Set the workweek, the two-week pay period start, and rest rules.",
  },
  {
    path: "/employees",
    title: "Add employees",
    detail: "Include hours limits, what they can be scheduled for, and rules.",
  },
  {
    path: "/shift-templates",
    title: "Review shift templates",
    detail: "Check times, paid hours, and recovery.",
  },
  {
    path: "/positions-coverage",
    title: "Review positions and coverage",
    detail: "Give each weekday a default coverage template.",
  },
  {
    path: "/availability-pto",
    title: "Enter availability and PTO",
    detail: "Approved PTO blocks a shift; requested PTO only warns.",
  },
  {
    path: "/holidays",
    title: "Review holidays",
    detail: "Resolve holiday collisions before generating.",
  },
] as const;

/** The routine for each schedule period. */
export const monthlySteps = [
  {
    path: "/?open=generate",
    title: "Generate a draft",
    detail: "Generate… on Schedule fills a calendar month or a custom range.",
  },
  {
    path: "/",
    title: "Adjust the board",
    detail:
      "Select a shift to assign someone, open it, add a note, or lock it.",
  },
  {
    path: "/?open=issues",
    title: "Review issues",
    detail: "Issues lists errors and warnings and updates after each change.",
  },
  {
    path: "/",
    title: "Finalize",
    detail: "Finalize… locks the schedule. Errors must be fixed first.",
  },
  {
    path: "/",
    title: "Print or export",
    detail: "Print… and Export… are in the Schedule toolbar.",
  },
  {
    path: "/backup",
    title: "Back up",
    detail: "Save a copy of all your data to a file.",
  },
] as const;

export const howTo = [
  {
    title: "Build a schedule",
    text: [
      "On Schedule, choose Generate…, pick a calendar month or a date range, and name the draft. Shift Happens fills each required shift with someone who can work it, following your rules.",
      "It never gives anyone a second shift on the same day, less than the minimum rest or recovery time, or more consecutive workdays than your limit. A shift it can't fill within those rules stays open.",
      "An open shift doesn't mean no one could work it, only that this pass found no one. You can assign it yourself; if that breaks a rest or workday rule, you're asked to confirm first.",
    ],
  },
  {
    title: "Fix issues",
    text: [
      "Issues shows errors and warnings for the open schedule. Errors, such as a double booking or approved PTO, must be fixed before finalizing. Warnings, such as overtime or a missed preference, can be accepted when you finalize.",
      "Select an issue to open its shift on the board, change it, and Issues checks again automatically.",
    ],
  },
  {
    title: "Finalize or reopen",
    text: [
      "Finalize… locks the schedule once there are no errors; you confirm any warnings and open shifts. To change a final schedule, choose More › Reopen as draft.",
    ],
  },
  {
    title: "Print or export",
    text: [
      "Print… shows a preview, then opens the Windows print dialog. Export… saves an Excel workbook or a PDF. Both use the schedule as it is at that moment.",
      "Use Excel for names in non-Latin scripts; PDF supports Latin letters and common punctuation.",
    ],
  },
  {
    title: "Back up and restore",
    text: [
      "Backup saves a complete copy of your data to one file. Back up after finalizing, and keep one copy on a department network drive.",
      "Restore from backup… replaces all current data with a backup file. Your current data is saved first, and the app restarts.",
    ],
  },
  {
    title: "Archive or delete a schedule",
    text: [
      "Archive hides a schedule from everyday lists but keeps it; archived schedules can still be viewed, printed, and exported.",
      "Delete removes a draft that was never finalized, such as a scratch copy. Schedules that were finalized are kept as records and can only be archived.",
    ],
  },
  {
    title: "What the labels mean",
    text: [
      "Draft: editable. Final: locked until reopened. Archived: kept for reference and read-only. Current: the schedule that opens first. Most recent: shown when no schedule is set as current.",
      "Hours are planned hours from the schedule, not hours actually worked.",
    ],
  },
] as const;

export const shortcuts = [
  { keys: "F1", action: "Open Help" },
  { keys: "Ctrl+Z", action: "Undo the last shift change (Schedule)" },
  { keys: "Ctrl+Y or Ctrl+Shift+Z", action: "Redo (Schedule)" },
  { keys: "Ctrl+P", action: "Print preview of the open schedule" },
  { keys: "Escape", action: "Close a dialog, panel, or menu" },
  { keys: "Tab / Shift+Tab", action: "Move between controls" },
  { keys: "Enter / Space", action: "Activate a button or toggle a checkbox" },
  { keys: "Alt+Left / Alt+Right", action: "Go back or forward" },
] as const;

export const privacy = [
  {
    title: "Where your data is",
    text: "Everything stays on this computer, in your Windows profile. Shift Happens has no accounts, makes no internet connections, and sends no usage data. The exact folder is shown on About.",
  },
  {
    title: "No patient or medical details",
    text: "Notes are for scheduling only. Don't enter patient information, diagnoses, or reasons for medical leave; those belong in clinical and HR systems.",
  },
  {
    title: "Keeping it protected",
    text: "Your Windows sign-in and drive encryption (BitLocker on hospital-managed computers) protect the data. Backups, exports, and printouts are ordinary files and paper; keep them in hospital-approved places.",
  },
  {
    title: "Records",
    text: "Finalized schedules can't be deleted, only archived. Follow your hospital's retention policy; federal wage-and-hour rules expect work schedules to be kept for at least two years.",
  },
] as const;

export const limitations = [
  "Planning for one manager on one Windows computer. No employee access, cloud sync, or automatic updates.",
  "Totals, minimums, maximums, and overtime count only the schedule you're viewing, not other schedules or shifts worked elsewhere. Minimums are checked fully only for periods the schedule covers completely.",
  "Rest, recovery, overlap, and consecutive-day checks can't see shifts before or after the schedule's dates.",
  "Shift Happens supplements your hospital's official scheduling, timekeeping, and payroll systems; those remain the system of record.",
] as const;
