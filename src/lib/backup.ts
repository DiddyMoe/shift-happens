import { open, save } from "@tauri-apps/plugin-dialog";
import { desktopInvoke, type AppError, type CommandResult } from "./ipc";

export interface ForeignKeyResult {
  readonly violation_count: number;
  readonly details: readonly {
    readonly table: string;
    readonly parent: string;
    readonly foreign_key_index: number;
    readonly violation_count: number;
  }[];
  readonly omitted_violation_count: number;
}
export interface BackupFile {
  readonly destination_path: string;
  readonly file_size_bytes: number;
  readonly started_at: string;
  readonly completed_at: string;
  readonly integrity: "ok";
  readonly foreign_keys: ForeignKeyResult;
  readonly validation_policy: "manual_preserve" | "pre_migration_strict";
  readonly destination_policy: "manual" | "app_owned_no_overwrite";
  readonly engine: { readonly version: string; readonly source_id: string };
}
export interface BackupRun {
  readonly id: string;
  readonly backup_type: "manual" | "pre_migration";
  readonly destination_path: string;
  readonly status: "success" | "failed";
  readonly started_at: string;
  readonly completed_at: string;
  readonly file_size_bytes: number | null;
  readonly error_message: string | null;
  readonly created_at: string;
}
export interface BackupScreen {
  readonly default_filename: string;
  readonly last_manual_backup: BackupRun | null;
  readonly history_available: boolean;
}
export type BackupOutcome = CommandResult<BackupFile>;
export type OverwriteConfirmation = Extract<
  BackupOutcome,
  { status: "requires_overwrite" }
>;
export const getBackupScreen = () =>
  desktopInvoke<BackupScreen>("get_backup_screen");
export const createManualBackup = (
  destinationPath: string | null,
  overwriteToken: string | null = null,
) =>
  desktopInvoke<BackupFile>("create_manual_backup", {
    request: {
      destination_path: destinationPath,
      overwrite_token: overwriteToken,
    },
  });
export const chooseBackupDestination = (defaultFilename: string) =>
  save({
    title: "Save full SQLite backup",
    defaultPath: defaultFilename,
    filters: [{ name: "SQLite backup", extensions: ["sqlite"] }],
  });
export function backupError(error: AppError): string {
  return `${error.message} (${error.code})`;
}

export interface RestorePreview {
  readonly staged_id: string;
  readonly file_name: string;
  readonly source_path: string;
  readonly file_size_bytes: number;
  /** Highest database migration applied in the backup. */
  readonly schema_version: number;
  readonly current_schema_version: number;
  readonly will_migrate: boolean;
  readonly employee_count: number | null;
  readonly schedule_count: number | null;
  readonly latest_schedule_end_date: string | null;
  /** True when the current data is copied first; false during blocked startup,
   * when the current files are moved aside instead. */
  readonly safety_backup: boolean;
  readonly restore_backups_path: string;
}
export interface RestoreScheduled {
  readonly safety_backup: string | null;
  readonly replaced_files_folder: string;
}
export const chooseRestoreSource = () =>
  open({
    title: "Choose a Shift Happens backup",
    multiple: false,
    directory: false,
    filters: [{ name: "SQLite database", extensions: ["sqlite", "db"] }],
  });
/** Copies and checks the backup; success is `requires_confirmation`. */
export const stageDatabaseRestore = (sourcePath: string | null) =>
  desktopInvoke<RestorePreview>("stage_database_restore", {
    request: { source_path: sourcePath },
  });
/** On success (and on a failure after the data was closed) the app restarts. */
export const applyDatabaseRestore = (stagedId: string) =>
  desktopInvoke<RestoreScheduled>("apply_database_restore", {
    request: { staged_id: stagedId },
  });
