import { desktopInvoke } from "./ipc";
import type { ActiveStatus, RecordFilter, TimeOfDay } from "./employees";
import type { ShiftTimeCalculation } from "./shiftTime";

export interface ShiftTemplateData {
  name: string;
  display_label: string | null;
  start_time: string;
  end_time: string;
  time_of_day: TimeOfDay;
  requires_recovery_after_shift: boolean;
  is_night_shift: boolean;
  color_hex: string | null;
  active_status: ActiveStatus;
  notes: string | null;
}
export interface ShiftTemplate extends ShiftTemplateData {
  readonly id: string;
  readonly seed_key: string | null;
  readonly unpaid_lunch_minutes: number;
  readonly paid_minutes: number;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export function templateData(row: ShiftTemplate): ShiftTemplateData {
  return {
    name: row.name,
    display_label: row.display_label,
    start_time: row.start_time,
    end_time: row.end_time,
    time_of_day: row.time_of_day,
    requires_recovery_after_shift: row.requires_recovery_after_shift,
    is_night_shift: row.is_night_shift,
    color_hex: row.color_hex,
    active_status: row.active_status,
    notes: row.notes,
  };
}
export const listShiftTemplates = (filter: RecordFilter, search = "") =>
  desktopInvoke<readonly ShiftTemplate[]>("list_shift_templates", {
    request: { filter, search },
  });
export const getShiftTemplate = (id: string) =>
  desktopInvoke<ShiftTemplate>("get_shift_template", { request: { id } });
export const createShiftTemplate = (template: ShiftTemplateData) =>
  desktopInvoke<ShiftTemplate>("create_shift_template", {
    request: { template },
  });
export const updateShiftTemplate = (
  row: ShiftTemplate,
  template: ShiftTemplateData,
) =>
  desktopInvoke<ShiftTemplate>("update_shift_template", {
    request: { id: row.id, expected_revision: row.revision, template },
  });
export const archiveShiftTemplate = (row: ShiftTemplate) =>
  desktopInvoke<ShiftTemplate>("archive_shift_template", {
    request: { id: row.id, expected_revision: row.revision },
  });
export const previewShiftTimeAuthoritative = (
  start_time: string,
  end_time: string,
) =>
  desktopInvoke<ShiftTimeCalculation>("preview_shift_time", {
    request: { start_time, end_time },
  });

export interface TemplateRestrictionData {
  shift_template_id: string;
  restriction_type: "cannot_work";
  active_status: ActiveStatus;
  notes: string | null;
}
export interface TemplateRestriction extends TemplateRestrictionData {
  readonly id: string;
  readonly employee_id: string;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export const listTemplateRestrictions = (
  employee_id: string,
  filter: RecordFilter,
) =>
  desktopInvoke<readonly TemplateRestriction[]>(
    "list_employee_shift_template_restrictions",
    { request: { employee_id, filter } },
  );
export const createTemplateRestriction = (
  employee_id: string,
  restriction: TemplateRestrictionData,
) =>
  desktopInvoke<TemplateRestriction>(
    "create_employee_shift_template_restriction",
    { request: { employee_id, restriction } },
  );
export const updateTemplateRestriction = (
  row: TemplateRestriction,
  restriction: TemplateRestrictionData,
) =>
  desktopInvoke<TemplateRestriction>(
    "update_employee_shift_template_restriction",
    { request: { id: row.id, expected_revision: row.revision, restriction } },
  );
export const archiveTemplateRestriction = (row: TemplateRestriction) =>
  desktopInvoke<TemplateRestriction>(
    "archive_employee_shift_template_restriction",
    { request: { id: row.id, expected_revision: row.revision } },
  );
