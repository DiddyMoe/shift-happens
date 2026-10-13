import { save } from "@tauri-apps/plugin-dialog";
import { desktopInvoke } from "./ipc";

export type ExportFormat = "xlsx" | "pdf";
export type ExportCell =
  | { readonly kind: "text"; readonly value: string }
  | { readonly kind: "integer"; readonly value: number };
export interface ExportDiagnostic {
  readonly code: string;
  readonly row: number | null;
  readonly field: string;
}
export interface DetailField {
  readonly label: string;
  readonly value: string;
}
export interface ExportRow {
  readonly detail_key: string;
  readonly cells: readonly ExportCell[];
  readonly compact: readonly string[];
  readonly details: readonly DetailField[];
}
export interface ExportSnapshot {
  readonly schedule_id: string;
  readonly schedule_name: string;
  readonly start_date: string;
  readonly end_date: string;
  readonly status: "draft" | "final";
  readonly archived_at: string | null;
  readonly revision: number;
  readonly exported_at_utc: string;
  readonly schedule_notes: string;
  readonly rows: readonly ExportRow[];
  readonly diagnostics: readonly ExportDiagnostic[];
  readonly omitted_diagnostic_count: number;
  readonly text_bytes: number;
  readonly xlsx_available: boolean;
}
export interface ExportFile {
  readonly format: ExportFormat;
  readonly destination_path: string;
  readonly file_size_bytes: number;
  readonly schedule_id: string;
  readonly captured_revision: number;
  readonly exported_at_utc: string;
  readonly assignment_count: number;
  readonly diagnostics: readonly ExportDiagnostic[];
  readonly omitted_diagnostic_count: number;
}
export const exportHeaders = [
  "Schedule Date",
  "Operational Day",
  "Holiday",
  "Holiday Set",
  "Coverage",
  "Slot",
  "Position",
  "Shift",
  "Shift Time",
  "Actual Start",
  "Actual End",
  "Employee",
  "State",
  "Paid Minutes",
  "Paid H:MM",
  "Lunch Minutes",
  "Locked",
  "Source",
  "Manually Edited",
  "Notes",
] as const;
export const compactHeaders = [
  "Date",
  "Day",
  "Holiday",
  "Slot",
  "Position",
  "Shift / Time",
  "Actual Time",
  "Employee / State",
  "Paid",
  "Flags",
] as const;
export const cellText = (cell: ExportCell | undefined) =>
  cell ? String(cell.value) : "";
export const buildPrintDto = (scheduleId: string) =>
  desktopInvoke<ExportSnapshot>("build_print_dto", {
    request: { schedule_id: scheduleId },
  });
export const exportSchedule = (
  scheduleId: string,
  format: ExportFormat,
  destinationPath: string | null,
  overwriteToken: string | null = null,
) =>
  desktopInvoke<ExportFile>(
    format === "xlsx" ? "export_schedule_xlsx" : "export_schedule_pdf",
    {
      request: {
        schedule_id: scheduleId,
        destination_path: destinationPath,
        overwrite_token: overwriteToken,
      },
    },
  );
/** The schedule's name as a Windows file name: reserved and control
 * characters become spaces, and trailing dots or spaces are dropped. */
function fileName(scheduleName: string) {
  const name = [...scheduleName]
    .map((c) => (c < " " || '<>:"/\\|?*'.includes(c) ? " " : c))
    .join("")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[. ]+$/, "");
  return name || "schedule";
}
export const chooseExportDestination = (
  format: ExportFormat,
  scheduleName: string,
) =>
  save({
    title: `Save schedule as ${format === "xlsx" ? "Excel workbook" : "PDF"}`,
    defaultPath: `${fileName(scheduleName)}.${format}`,
    filters: [
      {
        name: format === "xlsx" ? "Excel workbook" : "PDF document",
        extensions: [format],
      },
    ],
  });
