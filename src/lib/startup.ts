import { desktopInvoke, type AppError } from "./ipc";
import type { BackupFile } from "./backup";

export type StartupClassification =
  | "new_database"
  | "uninitialized_empty_database"
  | "current"
  | "pending"
  | "database_file_set_inconsistent"
  | "database_access_error"
  | "migration_metadata_missing"
  | "failed_or_dirty_migration"
  | "checksum_mismatch"
  | "newer_or_unknown_migration"
  | "database_integrity_error";

export interface StartupReport {
  readonly version: string;
  readonly database_path: string | null;
  readonly classification: StartupClassification;
  readonly reasons: readonly StartupClassification[];
  readonly ready: boolean;
  readonly error: AppError | null;
  readonly pre_migration_backup: {
    readonly status: "not_attempted" | "success" | "failed";
    readonly path: string | null;
    readonly error: AppError | null;
  };
  readonly preservation_available: boolean;
  readonly preservation_unavailable_reason: string | null;
  readonly preservation_filename: string | null;
  readonly warnings: readonly AppError[];
  /** Outcome of a restore that this startup handled, if one was pending. */
  readonly restore: RestoreOutcome | null;
}

export interface RestoreOutcome {
  readonly status: "applied" | "abandoned";
  readonly source_name: string | null;
  /** Copy of the previous data saved before restoring (running app only). */
  readonly safety_backup: string | null;
  /** Folder holding the previous database files after they were moved aside. */
  readonly replaced_dir: string | null;
  /** The backup was upgraded to this version's database layout. */
  readonly migrated: boolean;
  readonly completed_at: string | null;
}

export const classificationLabels: Record<StartupClassification, string> = {
  new_database: "New database",
  uninitialized_empty_database: "Uninitialized empty database",
  current: "Current database",
  pending: "Pending database upgrade",
  database_file_set_inconsistent: "Unexpected or changed database files",
  database_access_error: "Database cannot be safely read",
  migration_metadata_missing: "Migration history is missing",
  failed_or_dirty_migration: "Migration history is incomplete or inconsistent",
  checksum_mismatch: "Migration checksums do not match this application",
  newer_or_unknown_migration: "Database belongs to a newer or unknown build",
  database_integrity_error: "Database integrity or foreign keys need attention",
};

export const getStartupStatus = () =>
  desktopInvoke<StartupReport>("get_startup_status");
export const savePreservationBackup = (
  destinationPath: string | null,
  overwriteToken: string | null = null,
) =>
  desktopInvoke<BackupFile>("save_preservation_backup", {
    request: {
      destination_path: destinationPath,
      overwrite_token: overwriteToken,
    },
  });
