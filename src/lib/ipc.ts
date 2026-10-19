import { invoke, isTauri } from "@tauri-apps/api/core";
import { describeError, describeWarnings } from "./errors";

export type ErrorCode =
  | "database_busy"
  | "disk_full"
  | "data_folder_unwritable"
  | "change_not_allowed"
  | "finalized_schedule_retained"
  | "restore_source_invalid"
  | "restore_confirmation_expired"
  | "restore_failed"
  | "invalid_date"
  | "invalid_time"
  | "invalid_datetime"
  | "invalid_interval"
  | "invalid_text"
  | "out_of_range"
  | "invalid_metadata"
  | "invalid_setting"
  | "configuration_invalid"
  | "setting_not_editable"
  | "stale_revision"
  | "database_unavailable"
  | "database_integrity_error"
  | "startup_classification_changed"
  | "unsafe_sqlite_engine"
  | "migration_invalid"
  | "upgrade_requires_qualified_startup"
  | "wal_required"
  | "post_schema_initialization_failed"
  | "release_startup_not_qualified"
  | "invalid_schedule"
  | "invalid_assignment"
  | "invalid_generation_snapshot"
  | "schedule_read_only"
  | "reopen_confirmation_required"
  | "invalid_employee"
  | "invalid_shift_template"
  | "invalid_coverage_item"
  | "invalid_coverage_offset"
  | "invalid_required_count"
  | "invalid_coverage_default"
  | "invalid_holiday_rule"
  | "invalid_holiday_coverage"
  | "holiday_collision"
  | "duplicate_coverage_item"
  | "weekday_default_required"
  | "invalid_color"
  | "invalid_restriction"
  | "invalid_revision"
  | "not_found"
  | "record_archived"
  | "duplicate_restriction"
  | "active_dependencies"
  | "input_limit_exceeded"
  | "generation_input_limit_exceeded"
  | "planned_slot_limit_exceeded"
  | "two_week_anchor_required"
  | "coverage_missing"
  | "coverage_ambiguous"
  | "coverage_empty"
  | "coverage_source_invalid"
  | "duplicate_planned_slot"
  | "candidate_input_limit_exceeded"
  | "editor_output_limit_exceeded"
  | "assignment_locked"
  | "employee_ineligible"
  | "invalid_acknowledgement"
  | "finalization_blocked"
  | "history_stale"
  | "history_unavailable"
  | "invalid_destination"
  | "invalid_destination_extension"
  | "protected_database_destination"
  | "destination_state_changed"
  | "overwrite_authorization_invalid"
  | "overwrite_unavailable"
  | "temporary_name_exhausted"
  | "file_write_failed"
  | "file_synchronization_failed"
  | "file_publication_failed"
  | "publication_recovery_required"
  | "file_cleanup_failed"
  | "backup_source_invalid"
  | "backup_destination_in_use"
  | "backup_copy_failed"
  | "backup_copy_limit_exceeded"
  | "backup_validation_failed"
  | "backup_foreign_key_violation"
  | "foreign_key_violations_preserved"
  | "backup_metadata_unavailable"
  | "backup_in_progress"
  | "export_too_large_for_format"
  | "export_text_budget_exceeded"
  | "xlsx_text_limit_exceeded"
  | "pdf_text_not_renderable"
  | "export_structure_invalid"
  | "export_validation_failed"
  | "export_generation_failed";

export interface AppError {
  readonly code: ErrorCode;
  readonly message: string;
  readonly context: {
    readonly key?: string;
    readonly stage?: string;
    readonly current_revision?: number;
    readonly sqlite_code?: number;
    readonly cause?: ErrorCode;
    readonly id?: string;
    readonly dependency_ids?: readonly string[];
    readonly dependency_names?: readonly string[];
    readonly dependency_count?: number;
    readonly omitted_source_id_count?: number;
    readonly limit_kind?: string;
    readonly limit?: number;
    readonly observed_count?: number;
    readonly diagnostics?: unknown;
    readonly history?: unknown;
    readonly destination_path?: string;
    readonly publication_state?: string;
    readonly recovery_path?: string;
    readonly cleanup_warnings?: readonly AppError[];
  };
}

// This union mirrors the authoritative backend CommandResult exactly.
export type CommandResult<T> =
  | {
      readonly status: "success";
      readonly data: T;
      readonly warnings?: readonly AppError[];
    }
  | { readonly status: "no_change"; readonly data: T }
  | { readonly status: "requires_confirmation"; readonly data: T }
  | {
      readonly status: "requires_overwrite";
      readonly destination_path: string;
      readonly overwrite_token: string;
    }
  | { readonly status: "canceled" }
  | { readonly status: "validation_error"; readonly error: AppError }
  | { readonly status: "stale_revision"; readonly error: AppError }
  | { readonly status: "blocked"; readonly error: AppError }
  | { readonly status: "failure"; readonly error: AppError };

export type Weekday =
  | "sunday"
  | "monday"
  | "tuesday"
  | "wednesday"
  | "thursday"
  | "friday"
  | "saturday";
export type ThemePreference = "dark" | "light" | "system";
export type SettingValue =
  | { readonly kind: "string"; readonly value: string }
  | { readonly kind: "integer"; readonly value: number }
  | { readonly kind: "boolean"; readonly value: boolean }
  | { readonly kind: "date"; readonly value: string }
  | { readonly kind: "weekday"; readonly value: Weekday }
  | { readonly kind: "schedule_id"; readonly value: string };

export type ManagerSettingKey =
  | "workweek_start_weekday"
  | "two_week_period_anchor_date"
  | "max_consecutive_workdays"
  | "quick_turnaround_rule_enabled"
  | "minimum_rest_minutes_between_shifts"
  | "theme_preference";
export type SettingKey =
  | ManagerSettingKey
  | "holiday_rotation_start_year"
  | "weekly_overtime_threshold_minutes"
  | "local_workplace_time_policy"
  | "local_date_format"
  | "local_time_format"
  | "local_datetime_format"
  | "current_schedule_id";

export interface SettingView {
  readonly key: SettingKey;
  readonly value_type: SettingValue["kind"];
  readonly value: SettingValue | null;
  readonly default_value: SettingValue | null;
  readonly nullable: boolean;
  readonly editable: boolean;
  readonly owner: "settings" | "holidays" | "schedule";
  readonly description: string;
  readonly revision: number;
}

export interface DatabaseHealth {
  readonly path: string;
  readonly exists: boolean;
  readonly migrations: {
    readonly applied_versions: readonly number[];
    readonly embedded_versions: readonly number[];
    readonly current: boolean;
  };
  readonly foreign_keys: boolean;
  readonly journal_mode: string;
  readonly synchronous: string;
  readonly busy_timeout_ms: number;
  readonly max_pool_connections: number;
  readonly engine: { readonly version: string; readonly source_id: string };
  readonly health: "ok" | "attention_required";
}

export function unreachableResult(value: never): never {
  void value;
  throw new Error("Unsupported desktop result. Reload the application.");
}

export function readCommandResult<T>(result: CommandResult<T>): T {
  switch (result.status) {
    case "success":
    case "no_change":
      return result.data;
    case "canceled":
      throw new Error("The action was canceled.");
    case "requires_overwrite":
      throw new Error(
        "Review the selected file and explicitly confirm replacement.",
      );
    case "requires_confirmation":
      throw new Error(
        "Review the current warnings and explicitly confirm this action.",
      );
    case "validation_error":
    case "stale_revision":
    case "blocked":
    case "failure":
      throw new Error(describeError(result.error));
    default:
      return unreachableResult(result);
  }
}

export function desktopInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<CommandResult<T>> {
  if (!isTauri())
    return Promise.reject(
      new Error(
        "Open the Windows desktop app to manage saved data. (desktop_required)",
      ),
    );
  const origin = window.location.hash;
  // Lets a screen tell its own changes from ones begun before it opened.
  const started = performance.now();
  const mutation =
    /^(create_|update_|archive_|delete_|reset_|set_|clear_|edit_|replay_|generate_|finalize_|reopen_|export_)/.test(
      command,
    );
  return invoke<CommandResult<T>>(command, args)
    .then((result) => {
      if (mutation) {
        const request = args?.request;
        const record =
          typeof request === "object" && request !== null
            ? (request as Record<string, unknown>)
            : {};
        const outcome =
          "data" in result &&
          typeof result.data === "object" &&
          result.data !== null
            ? (result.data as Record<string, unknown>)
            : {};
        const schedule =
          typeof outcome.schedule === "object" && outcome.schedule !== null
            ? (outcome.schedule as Record<string, unknown>)
            : {};
        window.dispatchEvent(
          new CustomEvent("shifthappens:operation", {
            detail: {
              command,
              origin,
              started,
              status: result.status,
              id:
                record.schedule_id ??
                record.id ??
                record.employee_id ??
                record.key ??
                outcome.schedule_id ??
                schedule.id ??
                outcome.id ??
                record.destination_path ??
                null,
              message:
                "error" in result
                  ? describeError(result.error)
                  : result.status === "success"
                    ? describeWarnings(result.warnings) || null
                    : null,
            },
          }),
        );
        // Exports only write a file, so cached checks of saved data stay valid.
        if (result.status === "success" && !command.startsWith("export_"))
          window.dispatchEvent(new Event("shifthappens:data-changed"));
      }
      return result;
    })
    .catch(() => {
      throw new Error(
        "Shift Happens couldn't finish that. Try again, or restart the app if it keeps happening.",
      );
    });
}

export const getSettings = () =>
  desktopInvoke<readonly SettingView[]>("get_settings");
export const getDatabaseHealth = () =>
  desktopInvoke<DatabaseHealth>("get_database_health");
export const updateManagerSetting = (
  key: ManagerSettingKey,
  value: SettingValue | null,
  expectedRevision: number,
) =>
  desktopInvoke<SettingView>("update_manager_setting", {
    request: { key, value, expected_revision: expectedRevision },
  });

export function isManagerSettingKey(key: SettingKey): key is ManagerSettingKey {
  return (
    key === "workweek_start_weekday" ||
    key === "two_week_period_anchor_date" ||
    key === "max_consecutive_workdays" ||
    key === "quick_turnaround_rule_enabled" ||
    key === "minimum_rest_minutes_between_shifts" ||
    key === "theme_preference"
  );
}
