import { Confirmation } from "./RecordControls";
import { useEffect, useRef, useState } from "react";
import {
  applyDatabaseRestore,
  backupError,
  chooseBackupDestination,
  chooseRestoreSource,
  createManualBackup,
  getBackupScreen,
  stageDatabaseRestore,
  type BackupFile,
  type BackupOutcome,
  type BackupRun,
  type BackupScreen,
  type OverwriteConfirmation,
  type RestorePreview,
} from "./lib/backup";
import { readCommandResult, unreachableResult, type AppError } from "./lib/ipc";
import { displayLocalDate } from "./lib/localDate";
import { errorText } from "./lib/recordFeedback";
import {
  getStartupStatus,
  savePreservationBackup,
  type RestoreOutcome,
} from "./lib/startup";

function completion(value: string) {
  // Backup metadata is a UTC instant, separate from civil scheduling values.
  return new Date(value).toLocaleString();
}
function size(bytes: number) {
  return bytes >= 1024 * 1024
    ? `${(bytes / 1024 / 1024).toFixed(1)} MB`
    : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}
function FileResult({ file }: { file: BackupFile }) {
  return (
    <div className="panel bk-result" aria-label="Completed backup">
      <h2>Backup saved</h2>
      <dl className="facts">
        <dt>File</dt>
        <dd className="literal">{file.destination_path}</dd>
        <dt>Size</dt>
        <dd className="display">{size(file.file_size_bytes)}</dd>
        <dt>Completed</dt>
        <dd className="display">{completion(file.completed_at)}</dd>
        <dt>Check</dt>
        <dd>
          <span className="tag tag-ok">Passed</span> The copy was read back and
          is complete.
        </dd>
      </dl>
      {file.foreign_keys.violation_count > 0 && (
        <p className="notice notice-error">
          This physically valid preservation copy includes existing foreign-key
          problems. Keep it for recovery.
        </p>
      )}
    </div>
  );
}
function History({ run }: { run: BackupRun }) {
  return (
    <>
      <dl className="facts">
        <dt>Result</dt>
        <dd>
          <span
            className={`tag ${run.status === "success" ? "tag-ok" : "tag-danger"}`}
          >
            {run.status === "success" ? "Saved" : "Failed"}
          </span>
        </dd>
        <dt>Completed</dt>
        <dd className="display">{completion(run.completed_at)}</dd>
        <dt>File</dt>
        <dd className="literal">{run.destination_path}</dd>
        {run.file_size_bytes !== null && (
          <>
            <dt>Size</dt>
            <dd className="display">{size(run.file_size_bytes)}</dd>
          </>
        )}
      </dl>
      {run.error_message && (
        <p className="notice notice-error">{run.error_message}</p>
      )}
    </>
  );
}
export function Backup({
  preservationFilename,
}: {
  preservationFilename?: string;
}) {
  const createBackup = preservationFilename
    ? savePreservationBackup
    : createManualBackup;
  const [screen, setScreen] = useState<BackupScreen | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const active = useRef(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [failure, setFailure] = useState<AppError | null>(null);
  const [warnings, setWarnings] = useState<readonly AppError[]>([]);
  const [file, setFile] = useState<BackupFile | null>(null);
  const [confirmation, setConfirmation] =
    useState<OverwriteConfirmation | null>(null);
  useEffect(() => {
    let alive = true;
    getBackupScreen()
      .then((result) => {
        const data = readCommandResult(result);
        if (alive) {
          setScreen(data);
          if (result.status === "success") setWarnings(result.warnings ?? []);
        }
      })
      .catch((error: unknown) => {
        if (alive) setNotice(errorText(error));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);
  async function refreshHistory() {
    try {
      setScreen(readCommandResult(await getBackupScreen()));
    } catch {
      setWarnings((current) => [
        ...current,
        {
          code: "backup_metadata_unavailable",
          message:
            "The backup result is unchanged, but history could not be refreshed.",
          context: {},
        },
      ]);
    }
  }
  function showResult(result: BackupOutcome) {
    setConfirmation(null);
    switch (result.status) {
      case "success":
        setFile(result.data);
        setWarnings(result.warnings ?? []);
        setNotice("The full local backup was saved and independently checked.");
        break;
      case "requires_overwrite":
        setConfirmation(result);
        setNotice(null);
        break;
      case "canceled":
        setNotice("Backup canceled. No file was written.");
        break;
      case "failure":
      case "blocked":
      case "validation_error":
      case "stale_revision":
        setFailure(result.error);
        setNotice(null);
        break;
      case "no_change":
      case "requires_confirmation":
        setNotice(
          "The desktop returned an unexpected backup result. Inspect the selected file before retrying.",
        );
        break;
      default:
        unreachableResult(result);
    }
  }
  async function start(confirmed?: OverwriteConfirmation) {
    if (active.current || !screen) return;
    active.current = true;
    setBusy(true);
    setFailure(null);
    setNotice(null);
    setWarnings([]);
    setFile(null);
    try {
      const path =
        confirmed?.destination_path ??
        (await chooseBackupDestination(
          preservationFilename ?? screen.default_filename,
        ));
      const result = await createBackup(
        path,
        confirmed?.overwrite_token ?? null,
      );
      showResult(result);
      if (
        result.status !== "requires_overwrite" &&
        result.status !== "canceled"
      )
        await refreshHistory();
    } catch {
      setConfirmation(null);
      setNotice(
        "The desktop could not return the backup result. Inspect the selected destination before retrying.",
      );
    } finally {
      active.current = false;
      setBusy(false);
    }
  }
  async function cancelConfirmation() {
    if (active.current) return;
    active.current = true;
    setBusy(true);
    try {
      showResult(await createBackup(null));
    } catch {
      setConfirmation(null);
      setNotice("Backup canceled. No backup attempt started.");
    } finally {
      active.current = false;
      setBusy(false);
    }
  }
  return (
    <section
      className={
        preservationFilename
          ? "startup-card bk-preservation"
          : "page page-narrow"
      }
      aria-labelledby={
        preservationFilename ? "preservation-title" : "page-title"
      }
    >
      {preservationFilename ? (
        <div className="bk-intro">
          <h2 id="preservation-title">Save a preservation backup</h2>
          <p className="muted">
            Create a full local SQLite backup of your saved application data.
            The application remains blocked after saving; no migrations or
            repairs run.
          </p>
        </div>
      ) : (
        <header className="page-header">
          <div className="page-heading">
            <h1 id="page-title">Backup</h1>
            <p className="page-lede">
              Save a complete copy of your data to one file, or restore your
              data from one.
            </p>
          </div>
        </header>
      )}
      {screen && (
        <div className="bk-block">
          <h2>Last backup</h2>
          {screen.last_manual_backup ? (
            <History run={screen.last_manual_backup} />
          ) : (
            <p className="muted">
              {screen.history_available
                ? "No backup has been made yet."
                : "Backup history is unavailable during this startup. File preservation is still available."}
            </p>
          )}
        </div>
      )}
      <div className="bk-block bk-action">
        {!preservationFilename && <h2>Back up now</h2>}
        <p className="muted">
          Saves a complete copy of your data to a file you choose. The copy is
          checked before it&rsquo;s saved.
        </p>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy || loading || !screen || !!confirmation}
          onClick={() => void start()}
        >
          {busy
            ? "Backing up…"
            : preservationFilename
              ? "Save a preservation backup…"
              : "Back up…"}
        </button>
        <p className="fine-print">
          Backup files aren&rsquo;t encrypted. Save them only in
          hospital-approved places, such as a department network drive.
        </p>
        {loading && (
          <p className="status-line" role="status">
            Loading backup details…
          </p>
        )}
        {busy && (
          <p className="status-line" role="status">
            Backing up and checking the copy… Keep Shift Happens open.
          </p>
        )}
      </div>
      {confirmation && (
        <Confirmation
          title="Replace this existing backup file?"
          confirmLabel="Replace backup file"
          danger
          busy={busy}
          onConfirm={() => void start(confirmation)}
          onCancel={() => void cancelConfirmation()}
        >
          <p>
            This replaces the selected file after the backup passes its checks.
            If the file or folder changes, choose the destination again.
          </p>
          <p className="literal">{confirmation.destination_path}</p>
        </Confirmation>
      )}
      {notice && (
        <p
          className={`notice ${file ? "notice-success" : "notice-info"}`}
          role="status"
        >
          {notice}
        </p>
      )}
      {failure && (
        <div className="notice notice-error" role="alert">
          <p>{backupError(failure)}</p>
          {failure.context.destination_path && (
            <p className="literal">{failure.context.destination_path}</p>
          )}
          {failure.context.publication_state && (
            <p>Publication state: {failure.context.publication_state}</p>
          )}
          {failure.context.recovery_path && (
            <p>
              Prior file recovery:{" "}
              <span className="literal">{failure.context.recovery_path}</span>
            </p>
          )}
          {(failure.context.cleanup_warnings ?? []).map((warning, index) => (
            <p key={index}>
              {backupError(warning)}{" "}
              <span className="literal">
                {warning.context.destination_path}
              </span>
            </p>
          ))}
        </div>
      )}
      {warnings.length > 0 && (
        <div className="notice notice-warn" role="status">
          {warnings.map((warning, index) => (
            <p key={index}>
              {backupError(warning)}{" "}
              <span className="literal">
                {warning.context.destination_path}
              </span>
            </p>
          ))}
        </div>
      )}
      {file && <FileResult file={file} />}
      {!preservationFilename && <RestoreSection />}
    </section>
  );
}

function counted(count: number, noun: string) {
  return `${count.toLocaleString()} ${noun}${count === 1 ? "" : "s"}`;
}
function contents(preview: RestorePreview) {
  const counts = [
    preview.employee_count === null
      ? null
      : counted(preview.employee_count, "employee"),
    preview.schedule_count === null
      ? null
      : counted(preview.schedule_count, "schedule"),
  ]
    .filter(Boolean)
    .join(", ");
  const latest = preview.latest_schedule_end_date
    ? `latest schedule ends ${displayLocalDate(preview.latest_schedule_end_date)}`
    : null;
  return [counts, latest].filter(Boolean).join("; ");
}

/** Shown after a confirmed restore until the window closes for the restart. */
function RestoringDialog() {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const modal = dialog.current;
    modal?.showModal();
    return () => modal?.close();
  }, []);
  return (
    <dialog
      ref={dialog}
      className="dialog"
      aria-labelledby="restoring-title"
      onCancel={(event) => event.preventDefault()}
    >
      <h2 id="restoring-title">Restoring…</h2>
      <p role="status">Shift Happens will restart.</p>
    </dialog>
  );
}

function RestoreOutcomeNotice({ outcome }: { outcome: RestoreOutcome }) {
  const file = outcome.source_name ?? "a backup";
  if (outcome.status === "abandoned")
    return (
      <p className="notice notice-warn" role="status">
        The restore from {file} could not be applied. Your data was not changed.
      </p>
    );
  const saved = outcome.safety_backup ?? outcome.replaced_dir;
  return (
    <div className="notice notice-success" role="status">
      <p>
        Restored from {file}
        {outcome.completed_at ? ` on ${completion(outcome.completed_at)}` : ""}.
        {outcome.migrated ? " It was upgraded for this version." : ""}
      </p>
      {saved && (
        <p>
          Your previous data was saved to{" "}
          <span className="literal">{saved}</span>.
        </p>
      )}
    </div>
  );
}

/** Replace all data with a backup file. The backend checks a copy first, saves
 * the current data on confirmation, and the next startup installs the copy. */
export function RestoreSection({
  blocked = false,
  preservationFirst = false,
}: {
  /** Startup is blocked: the current files are moved aside, not copied. */
  blocked?: boolean;
  /** Suggest saving a preservation backup before restoring. */
  preservationFirst?: boolean;
}) {
  const [phase, setPhase] = useState<
    "idle" | "checking" | "confirm" | "restoring"
  >("idle");
  const [preview, setPreview] = useState<RestorePreview | null>(null);
  const [failure, setFailure] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<RestoreOutcome | null>(null);
  const active = useRef(false);
  useEffect(() => {
    let alive = true;
    getStartupStatus()
      .then((result) => {
        if (alive && result.status === "success")
          setOutcome(result.data.restore);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  async function choose() {
    if (active.current) return;
    active.current = true;
    setFailure(null);
    setNotice(null);
    try {
      const path = await chooseRestoreSource();
      if (!path) return;
      setPhase("checking");
      const result = await stageDatabaseRestore(path);
      switch (result.status) {
        case "requires_confirmation":
          setPreview(result.data);
          setPhase("confirm");
          return;
        case "canceled":
          break;
        case "failure":
        case "blocked":
        case "validation_error":
        case "stale_revision":
          setFailure(result.error);
          break;
        case "success":
        case "no_change":
        case "requires_overwrite":
          setNotice(
            "The desktop returned an unexpected restore result. Nothing was changed.",
          );
          break;
        default:
          unreachableResult(result);
      }
      setPhase("idle");
    } catch (error) {
      setNotice(errorText(error));
      setPhase("idle");
    } finally {
      active.current = false;
    }
  }
  async function confirm() {
    if (!preview || active.current) return;
    active.current = true;
    setPhase("restoring");
    try {
      const result = await applyDatabaseRestore(preview.staged_id);
      // Success keeps the restoring state; the backend restarts the app.
      if (result.status === "success") return;
      setPreview(null);
      setPhase("idle");
      if ("error" in result) setFailure(result.error);
      else
        setNotice(
          "The desktop returned an unexpected restore result. Nothing was changed.",
        );
    } catch (error) {
      setPreview(null);
      setPhase("idle");
      setNotice(errorText(error));
    } finally {
      active.current = false;
    }
  }
  const summary = preview ? contents(preview) : "";
  const body = (
    <>
      {outcome && <RestoreOutcomeNotice outcome={outcome} />}
      <button
        type="button"
        className="btn"
        disabled={phase !== "idle"}
        onClick={() => void choose()}
      >
        {phase === "checking" ? "Checking the backup…" : "Restore from backup…"}
      </button>
      {failure && (
        <div className="notice notice-error" role="alert">
          <p>{failure.message}</p>
          {failure.context.recovery_path && (
            <p className="literal">{failure.context.recovery_path}</p>
          )}
        </div>
      )}
      {notice && (
        <p className="notice notice-info" role="status">
          {notice}
        </p>
      )}
      {phase === "confirm" && preview && (
        <Confirmation
          title="Replace all current data with this backup?"
          confirmLabel="Restore and restart"
          danger
          busy={false}
          onConfirm={() => void confirm()}
          onCancel={() => {
            setPreview(null);
            setPhase("idle");
          }}
        >
          <dl className="facts bk-restore-facts">
            <dt>File</dt>
            <dd className="literal">{preview.file_name}</dd>
            <dt>Size</dt>
            <dd className="display">{size(preview.file_size_bytes)}</dd>
            {summary && (
              <>
                <dt>Contains</dt>
                <dd>{summary}</dd>
              </>
            )}
            {preview.will_migrate && (
              <>
                <dt>Version</dt>
                <dd>
                  Made with an earlier version. It will be upgraded when Shift
                  Happens restarts.
                </dd>
              </>
            )}
          </dl>
          <p>
            {preview.safety_backup
              ? "Your current data will be saved to "
              : "Your current database files will be moved to "}
            <span className="literal">{preview.restore_backups_path}</span>{" "}
            before restoring.
          </p>
        </Confirmation>
      )}
      {phase === "restoring" && <RestoringDialog />}
    </>
  );
  return blocked ? (
    <section
      className="startup-card bk-preservation"
      aria-labelledby="restore-title"
    >
      <div className="bk-intro">
        <h2 id="restore-title">Restore from a backup</h2>
        <p className="muted">
          Replaces the blocked database with a backup file. The current files
          are kept, and the app restarts.
          {preservationFirst
            ? " Save a preservation backup first if you may need them."
            : ""}
        </p>
      </div>
      <div className="bk-block bk-action">{body}</div>
    </section>
  ) : (
    <div className="bk-block bk-action bk-restore">
      <h2>Restore from a backup</h2>
      <p className="muted">
        Replaces all current data with a backup file. Your current data is saved
        first, and the app restarts.
      </p>
      {body}
    </div>
  );
}
