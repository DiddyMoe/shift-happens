import { desktopInvoke, type CommandResult } from "./ipc";
import type { ActiveStatus, RecordFilter } from "./employees";

export type AvailabilityType =
  "available" | "unavailable" | "preferred" | "hard_block";
export type PtoStatus = "requested" | "approved" | "denied" | "canceled";
export interface EntryData {
  employee_id: string;
  start_date: string;
  end_date: string;
  is_full_day: boolean;
  start_time: string | null;
  end_time: string | null;
  active_status: ActiveStatus;
  notes: string | null;
}
export interface AvailabilityData extends EntryData {
  entry_type: AvailabilityType;
}
export interface PtoData extends EntryData {
  status: PtoStatus;
}
export interface EntryMetadata {
  readonly id: string;
  readonly created_at: string;
  readonly updated_at: string;
  readonly archived_at: string | null;
  readonly revision: number;
}
export type AvailabilityEntry = AvailabilityData & EntryMetadata;
export type PtoEntry = PtoData & EntryMetadata;
export type Entry = AvailabilityEntry | PtoEntry;
export type EntryDraft = AvailabilityData | PtoData;
export interface EntryFilters {
  employee_id: string | null;
  filter: RecordFilter;
  start_date: string | null;
  end_date: string | null;
}
export const availabilityLabels: Record<AvailabilityType, string> = {
  available: "Available",
  preferred: "Preferred",
  unavailable: "Unavailable",
  hard_block: "Hard block",
};
export const ptoLabels: Record<PtoStatus, string> = {
  requested: "Requested",
  approved: "Approved",
  denied: "Denied",
  canceled: "Canceled",
};
export function entryData(row: Entry): EntryDraft {
  const data: EntryData = {
    employee_id: row.employee_id,
    start_date: row.start_date,
    end_date: row.end_date,
    is_full_day: row.is_full_day,
    start_time: row.start_time,
    end_time: row.end_time,
    active_status: row.active_status,
    notes: row.notes,
  };
  return "entry_type" in row
    ? { ...data, entry_type: row.entry_type }
    : { ...data, status: row.status };
}
export const listAvailability = (
  filters: EntryFilters,
  entryType: AvailabilityType | null,
) =>
  desktopInvoke<readonly AvailabilityEntry[]>("list_availability_entries", {
    request: { ...filters, entry_type: entryType },
  });
export const listPto = (filters: EntryFilters, status: PtoStatus | null) =>
  desktopInvoke<readonly PtoEntry[]>("list_pto_entries", {
    request: { ...filters, status },
  });
export const getAvailability = (id: string) =>
  desktopInvoke<AvailabilityEntry>("get_availability_entry", {
    request: { id },
  });
export const getPto = (id: string) =>
  desktopInvoke<PtoEntry>("get_pto_entry", { request: { id } });
export const createAvailability = (entry: AvailabilityData) =>
  desktopInvoke<AvailabilityEntry>("create_availability_entry", {
    request: { entry },
  });
export const createPto = (entry: PtoData) =>
  desktopInvoke<PtoEntry>("create_pto_entry", { request: { entry } });
export const updateAvailability = (
  row: EntryMetadata,
  entry: AvailabilityData,
) =>
  desktopInvoke<AvailabilityEntry>("update_availability_entry", {
    request: { id: row.id, expected_revision: row.revision, entry },
  });
export const updatePto = (row: EntryMetadata, entry: PtoData) =>
  desktopInvoke<PtoEntry>("update_pto_entry", {
    request: { id: row.id, expected_revision: row.revision, entry },
  });
export const archiveAvailability = (row: EntryMetadata) =>
  desktopInvoke<AvailabilityEntry>("archive_availability_entry", {
    request: { id: row.id, expected_revision: row.revision },
  });
export const archivePto = (row: EntryMetadata) =>
  desktopInvoke<PtoEntry>("archive_pto_entry", {
    request: { id: row.id, expected_revision: row.revision },
  });

/** Creates or updates an availability or PTO entry, whichever `data` is. */
export function saveEntry(
  row: Entry | null,
  data: EntryDraft,
): Promise<CommandResult<Entry>> {
  if ("entry_type" in data)
    return row ? updateAvailability(row, data) : createAvailability(data);
  return row ? updatePto(row, data) : createPto(data);
}
export const archiveEntry = (row: Entry): Promise<CommandResult<Entry>> =>
  "entry_type" in row ? archiveAvailability(row) : archivePto(row);
export const getEntry = (row: Entry): Promise<CommandResult<Entry>> =>
  "entry_type" in row ? getAvailability(row.id) : getPto(row.id);
