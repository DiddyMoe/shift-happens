import { displayLocalDate } from "./lib/localDate";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import {
  Confirmation as RecordConfirmation,
  HowItWorks,
} from "./RecordControls";
import { ScheduleStatusTag } from "./ScheduleBoard";
import { trapDialogFocus } from "./lib/dialogFocus";
import {
  buildPrintDto,
  cellText,
  chooseExportDestination,
  compactHeaders,
  exportHeaders,
  exportSchedule,
  type ExportDiagnostic,
  type ExportFile,
  type ExportFormat,
  type ExportSnapshot,
} from "./lib/exports";
import {
  unreachableResult,
  type AppError,
  type CommandResult,
} from "./lib/ipc";
import type { Schedule } from "./lib/schedules";
import { errorText } from "./lib/recordFeedback";

interface Confirmation {
  scheduleId: string;
  format: ExportFormat;
  path: string;
  token: string;
}
function Diagnostics({
  values,
  omitted,
}: {
  values: readonly ExportDiagnostic[];
  omitted: number;
}) {
  if (!values.length && !omitted) return null;
  return (
    <div className="notice notice-warn export-controls" role="status">
      <p>Output diagnostics (saved data was unchanged):</p>
      <ul className="bullets">
        {values.map((d, i) => (
          <li key={i}>
            {d.row ? `Assignment ${d.row}: ` : ""}
            {d.field}, {d.code.replaceAll("_", " ")}
          </li>
        ))}
      </ul>
      {omitted > 0 && <p>{omitted} additional diagnostics omitted.</p>}
    </div>
  );
}
function Preview({ snapshot }: { snapshot: ExportSnapshot }) {
  return (
    <article
      className="schedule-print-preview"
      aria-label="Read-only print preview"
    >
      <header className="schedule-print-header">
        <h2>{snapshot.schedule_name}</h2>
        <p>ID: {snapshot.schedule_id}</p>
        <p>
          {displayLocalDate(snapshot.start_date)} to{" "}
          {displayLocalDate(snapshot.end_date)}
        </p>
        <p>
          Status: {snapshot.status},{" "}
          {snapshot.archived_at ? "archived" : "not archived"}
        </p>
        <p>Captured schedule revision: {snapshot.revision}</p>
        <p>Export metadata (UTC): {snapshot.exported_at_utc}</p>
      </header>
      {!snapshot.rows.length && <p>No assignments in this schedule.</p>}
      <div
        className="print-table-scroll"
        tabIndex={0}
        role="region"
        aria-label="Scrollable print preview table"
      >
        <table className="schedule-print-table">
          <thead>
            <tr>
              {compactHeaders.map((h) => (
                <th key={h} scope="col">
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {snapshot.rows.map((row) => (
              <tr key={row.detail_key}>
                {row.compact.map((cell, i) => (
                  <td key={i}>{cell}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <section className="schedule-print-details">
        <h2>Assignment details</h2>
        <h3>Schedule notes</h3>
        <p className="literal-text">
          {snapshot.schedule_notes || "No schedule notes."}
        </p>
        {snapshot.rows.map((row, index) => (
          <section key={row.detail_key} className="print-assignment-detail">
            <h3>
              Detail {index + 1}: {cellText(row.cells[0])} /{" "}
              {cellText(row.cells[4])} / {cellText(row.cells[5])}
            </h3>
            <dl>
              {row.cells.map((cell, i) => (
                <div key={i}>
                  <dt>{exportHeaders[i]}</dt>
                  <dd>{cellText(cell)}</dd>
                </div>
              ))}
              {row.details.map((field, i) => (
                <div key={`detail-${i}`}>
                  <dt>{field.label}</dt>
                  <dd>{field.value}</dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </section>
    </article>
  );
}
function Failure({ error }: { error: AppError }) {
  const context = error.context;
  const detail = context.diagnostics;
  const alternative =
    typeof detail === "object" &&
    detail !== null &&
    "xlsx_alternative" in detail &&
    detail.xlsx_alternative === true;
  return (
    <div className="notice notice-error export-controls" role="alert">
      <p>
        {error.message} ({error.code})
      </p>
      {context.observed_count !== undefined && (
        <p>
          Observed:{" "}
          <span className="display">
            {context.observed_count.toLocaleString()}
          </span>
          ; limit:{" "}
          <span className="display">{context.limit?.toLocaleString()}</span>
        </p>
      )}
      {alternative && (
        <p>
          This captured schedule is within the Excel limits. Export it as an
          Excel workbook to preserve the full text.
        </p>
      )}
      {error.code === "pdf_text_not_renderable" && (
        <p>
          PDF supports bundled Noto Sans Latin text and selected punctuation.
          Other scripts, remaining combining marks, and bidirectional text
          require an Excel workbook.
        </p>
      )}
      {context.destination_path && (
        <p className="literal">{context.destination_path}</p>
      )}
      {context.publication_state && (
        <p>Publication state: {context.publication_state}</p>
      )}
      {context.recovery_path && (
        <p>
          Prior file recovery:{" "}
          <span className="literal">{context.recovery_path}</span>
        </p>
      )}
      {context.cleanup_warnings?.map((w, i) => (
        <p key={i}>
          {w.message}{" "}
          <span className="literal">{w.context.destination_path}</span>
        </p>
      ))}
    </div>
  );
}

/** A modal opened from a schedule's action row. It renders at the document
 * root so print rules can show it without the app shell around it. */
function OutputDialog({
  className,
  labelledBy,
  busy,
  onClose,
  children,
}: {
  className: string;
  labelledBy: string;
  busy: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    const modal = dialog.current;
    modal?.showModal();
    return () => {
      modal?.close();
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus();
    };
  }, []);
  return createPortal(
    <dialog
      ref={dialog}
      className={className}
      aria-labelledby={labelledBy}
      onKeyDown={trapDialogFocus}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
    >
      {children}
    </dialog>,
    document.body,
  );
}

function Subject({ schedule }: { schedule: Schedule }) {
  return (
    <p className="out-subject">
      <strong>{schedule.name}</strong>
      <span className="display">
        {displayLocalDate(schedule.start_date)} to{" "}
        {displayLocalDate(schedule.end_date)}
      </span>
      <ScheduleStatusTag schedule={schedule} />
    </p>
  );
}

const formats: readonly {
  value: ExportFormat;
  title: string;
  detail: string;
  limit: string;
}[] = [
  {
    value: "xlsx",
    title: "Excel workbook (.xlsx)",
    detail: "Keeps every character exactly as entered.",
    limit: "100,000",
  },
  {
    value: "pdf",
    title: "PDF document (.pdf)",
    detail:
      "Latin letters and common punctuation only. Use Excel for other scripts.",
    limit: "10,000",
  },
];

function ExportDialog({
  schedule,
  onClose,
}: {
  schedule: Schedule;
  onClose: () => void;
}) {
  const [format, setFormat] = useState<ExportFormat>("xlsx");
  const [busy, setBusy] = useState(false);
  const active = useRef(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [failure, setFailure] = useState<AppError | null>(null);
  const [warnings, setWarnings] = useState<readonly AppError[]>([]);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [file, setFile] = useState<ExportFile | null>(null);
  // Results that arrive after the dialog closes are discarded.
  const sequence = useRef(0);
  useLayoutEffect(() => {
    sequence.current += 1;
    return () => {
      sequence.current += 1;
    };
  }, []);
  function showResult(
    result: CommandResult<ExportFile>,
    scheduleId: string,
    format: ExportFormat,
  ) {
    setConfirmation(null);
    switch (result.status) {
      case "success":
        setFile(result.data);
        setWarnings(result.warnings ?? []);
        setNotice("Export saved and independently parsed.");
        break;
      case "requires_overwrite":
        setConfirmation({
          scheduleId,
          format,
          path: result.destination_path,
          token: result.overwrite_token,
        });
        break;
      case "canceled":
        setNotice("Export canceled. No file was written.");
        break;
      case "failure":
      case "blocked":
      case "validation_error":
      case "stale_revision":
        setFailure(result.error);
        break;
      case "no_change":
      case "requires_confirmation":
        setNotice(
          "Unexpected export outcome. Inspect the selected destination before retrying.",
        );
        break;
      default:
        unreachableResult(result);
    }
  }
  async function start(format: ExportFormat, confirmed?: Confirmation) {
    if (active.current) return;
    const serial = sequence.current;
    active.current = true;
    setBusy(true);
    setFailure(null);
    setNotice(null);
    setWarnings([]);
    setFile(null);
    const id = confirmed?.scheduleId ?? schedule.id;
    try {
      const path =
        confirmed?.path ??
        (await chooseExportDestination(format, schedule.name));
      if (sequence.current !== serial) return;
      const result = await exportSchedule(
        id,
        format,
        path,
        confirmed?.token ?? null,
      );
      if (sequence.current === serial) showResult(result, id, format);
    } catch {
      if (sequence.current !== serial) return;
      setConfirmation(null);
      setNotice(
        "The desktop did not return an export result. Inspect the selected destination before retrying.",
      );
    } finally {
      active.current = false;
      setBusy(false);
    }
  }
  const label = (f: ExportFormat) => (f === "xlsx" ? "Excel" : "PDF");
  return (
    <>
      <OutputDialog
        className="dialog out-dialog"
        labelledBy="export-title"
        busy={busy}
        onClose={onClose}
      >
        <h2 id="export-title">Export schedule</h2>
        <Subject schedule={schedule} />
        <fieldset className="out-formats" disabled={busy || !!confirmation}>
          <legend>Format</legend>
          {formats.map((f) => (
            <label key={f.value} className="out-format">
              <input
                type="radio"
                name="export-format"
                value={f.value}
                checked={format === f.value}
                onChange={() => setFormat(f.value)}
              />
              <span>
                <span className="out-format-title">{f.title}</span>
                <span className="out-format-detail">
                  {f.detail} Up to <span className="display">{f.limit}</span>{" "}
                  rows.
                </span>
              </span>
            </label>
          ))}
        </fieldset>
        <HowItWorks label="About exported files">
          <p>
            Exported files are ordinary unencrypted local documents; the
            selected folder and Windows account permissions protect them. Every
            format is limited to <span className="display">64</span> MiB of
            text.
          </p>
          <p>
            Each export captures the schedule as it is at that moment. Edits
            made between exports can change what the next one contains. Paid
            time is scheduled clock time, without daylight-saving adjustment.
          </p>
        </HowItWorks>
        {busy && (
          <p className="status-line" role="status">
            Generating and checking the local document… Keep this window open.
          </p>
        )}
        {notice && (
          <p className="notice notice-info" role="status">
            {notice}
          </p>
        )}
        {failure && <Failure error={failure} />}
        {warnings.length > 0 && (
          <div className="notice notice-warn">
            {warnings.map((w, i) => (
              <p key={i}>
                {w.message}{" "}
                <span className="literal">{w.context.destination_path}</span>
              </p>
            ))}
          </div>
        )}
        {file && (
          <div className="out-result">
            <h3>{label(file.format)} file saved</h3>
            <dl className="facts">
              <dt>File</dt>
              <dd className="literal">{file.destination_path}</dd>
              <dt>Size</dt>
              <dd>
                <span className="display">
                  {file.file_size_bytes.toLocaleString()}
                </span>{" "}
                bytes
              </dd>
              <dt>Assignments</dt>
              <dd className="display">
                {file.assignment_count.toLocaleString()}
              </dd>
              <dt>Captured revision</dt>
              <dd className="display">{file.captured_revision}</dd>
              <dt>Export metadata (UTC)</dt>
              <dd className="display">{file.exported_at_utc}</dd>
            </dl>
            <Diagnostics
              values={file.diagnostics}
              omitted={file.omitted_diagnostic_count}
            />
          </div>
        )}
        <div className="dialog-actions">
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={onClose}
          >
            {file ? "Done" : "Cancel"}
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || !!confirmation}
            onClick={() => void start(format)}
          >
            {busy ? "Exporting…" : `Export as ${label(format)}…`}
          </button>
        </div>
      </OutputDialog>
      {confirmation && (
        <RecordConfirmation
          title={`Replace this existing ${label(confirmation.format)} file?`}
          confirmLabel={`Replace ${label(confirmation.format)} file`}
          danger
          busy={busy}
          onConfirm={() => void start(confirmation.format, confirmation)}
          onCancel={() => {
            setConfirmation(null);
            setNotice("Export canceled. No file was written.");
          }}
        >
          <p>Schedule: {schedule.name}</p>
          <p className="literal">{confirmation.path}</p>
          <p>
            Confirmation applies to this exact observed file. If it or its
            folder changes, choose the destination again. The retry captures
            this schedule’s current revision.
          </p>
        </RecordConfirmation>
      )}
    </>
  );
}

function PrintDialog({
  schedule,
  onClose,
}: {
  schedule: Schedule;
  onClose: () => void;
}) {
  const [preview, setPreview] = useState<ExportSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState<AppError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let alive = true;
    buildPrintDto(schedule.id)
      .then((result) => {
        if (!alive) return;
        if (result.status === "success") {
          // Backend enforces this before loading; guard the DOM boundary too.
          if (result.data.rows.length > 10_000)
            throw new Error("The print preview exceeds its row limit.");
          setPreview(result.data);
        } else if (
          result.status === "failure" ||
          result.status === "blocked" ||
          result.status === "validation_error" ||
          result.status === "stale_revision"
        )
          setFailure(result.error);
        else setNotice("Print preview could not be loaded.");
      })
      .catch((e: unknown) => {
        if (alive) setNotice(errorText(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [schedule.id, attempt]);
  return (
    <OutputDialog
      className="print-dialog"
      labelledBy="print-title"
      busy={false}
      onClose={onClose}
    >
      <div className="print-dialog-bar">
        <div>
          <h2 id="print-title">Print preview</h2>
          <Subject schedule={schedule} />
        </div>
        <div className="cluster">
          <button type="button" className="btn" onClick={onClose}>
            Close
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!preview}
            onClick={() => window.print()}
          >
            Print…
          </button>
        </div>
      </div>
      <div className="print-dialog-body">
        {loading && (
          <p className="status-line" role="status">
            Preparing a read-only snapshot of this schedule…
          </p>
        )}
        {failure && <Failure error={failure} />}
        {notice && (
          <div className="notice notice-error export-controls" role="alert">
            <p>{notice}</p>
            <button
              type="button"
              className="btn btn-sm"
              onClick={() => {
                setLoading(true);
                setNotice(null);
                setAttempt((n) => n + 1);
              }}
            >
              Try again
            </button>
          </div>
        )}
        {preview && (
          <>
            <Diagnostics
              values={preview.diagnostics}
              omitted={preview.omitted_diagnostic_count}
            />
            <Preview snapshot={preview} />
          </>
        )}
      </div>
    </OutputDialog>
  );
}

export type OutputDialogKind = "export" | "print";

/** Export and Print for one explicit schedule. Export saves an Excel or PDF
 * file; Print opens a read-only preview that prints through the OS dialog.
 * Pass `open` and `onOpenChange` to open a dialog from elsewhere, for
 * example a keyboard shortcut. */
export function ScheduleOutputActions({
  schedule,
  disabled = false,
  open: controlledOpen,
  onOpenChange,
  primary,
}: {
  schedule: Schedule;
  disabled?: boolean;
  open?: OutputDialogKind | null | undefined;
  onOpenChange?: ((open: OutputDialogKind | null) => void) | undefined;
  /** Makes Print… the main action of its row. */
  primary?: "print" | undefined;
}) {
  const [ownOpen, setOwnOpen] = useState<OutputDialogKind | null>(null);
  const open = onOpenChange ? (controlledOpen ?? null) : ownOpen;
  const setOpen = onOpenChange ?? setOwnOpen;
  return (
    <>
      <button
        type="button"
        className="btn"
        disabled={disabled}
        onClick={() => setOpen("export")}
      >
        Export…
      </button>
      <button
        type="button"
        className={primary === "print" ? "btn btn-primary" : "btn"}
        disabled={disabled}
        aria-keyshortcuts={onOpenChange ? "Control+P" : undefined}
        title={onOpenChange ? "Print preview (Ctrl+P)" : undefined}
        onClick={() => setOpen("print")}
      >
        Print…
      </button>
      {open === "export" && (
        <ExportDialog
          key={schedule.id}
          schedule={schedule}
          onClose={() => setOpen(null)}
        />
      )}
      {open === "print" && (
        <PrintDialog
          key={schedule.id}
          schedule={schedule}
          onClose={() => setOpen(null)}
        />
      )}
    </>
  );
}
