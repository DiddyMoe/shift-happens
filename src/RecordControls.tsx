import { useEffect, useId, useRef, type ReactNode } from "react";
import { useModalDialog } from "./hooks/useModalDialog";
import { trapDialogFocus } from "./lib/dialogFocus";
import type { RecordFilter } from "./lib/employees";
import type { Feedback } from "./lib/recordFeedback";

export function RecordFeedback({
  feedback,
  busy,
  onReload,
  reloadLabel = "Reload latest record and discard edits",
}: {
  feedback: Feedback | null;
  busy: boolean;
  onReload: () => void;
  reloadLabel?: string;
}) {
  return feedback ? (
    <FeedbackNotice
      feedback={feedback}
      busy={busy}
      onReload={onReload}
      reloadLabel={reloadLabel}
    />
  ) : null;
}

/** A result message that scrolls into view when it appears, so the outcome
 * of a Save or Archive at the bottom of a long form is never off-screen. */
function FeedbackNotice({
  feedback,
  busy,
  onReload,
  reloadLabel,
}: {
  feedback: Feedback;
  busy: boolean;
  onReload: () => void;
  reloadLabel: string;
}) {
  const notice = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const reduce = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    notice.current?.scrollIntoView({
      block: "nearest",
      behavior: reduce ? "auto" : "smooth",
    });
  }, [feedback]);
  return (
    <div
      ref={notice}
      className={`notice ${feedback.failed ? "notice-error" : "notice-success"}`}
      role={feedback.failed ? "alert" : "status"}
    >
      <p>{feedback.message}</p>
      {feedback.stale && (
        <button
          className="btn btn-sm"
          type="button"
          disabled={busy}
          onClick={onReload}
        >
          {reloadLabel}
        </button>
      )}
    </div>
  );
}

export function RecordFilterSelect({
  value,
  onChange,
  label,
  disabled = false,
}: {
  value: RecordFilter;
  onChange: (v: RecordFilter) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label>
      {label}
      <select
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as RecordFilter)}
      >
        <option value="current">Active and inactive</option>
        <option value="active">Active</option>
        <option value="inactive">Inactive</option>
        <option value="archived">Archived</option>
        <option value="all">All records</option>
      </select>
    </label>
  );
}
export function Confirmation({
  title,
  children,
  busy,
  onConfirm,
  onCancel,
  confirmLabel = "Confirm",
  busyLabel = "Working…",
  danger = false,
}: {
  title: string;
  children: ReactNode;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  /** Names the action, for example "Archive employee". */
  confirmLabel?: string;
  /** Shown on the confirm button while the action runs, e.g. "Archiving…". */
  busyLabel?: string;
  /** Styles the confirm action as destructive. */
  danger?: boolean;
}) {
  const dialog = useModalDialog();
  const titleId = useId();
  return (
    <dialog
      ref={dialog}
      className="dialog"
      aria-labelledby={titleId}
      onKeyDown={trapDialogFocus}
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onCancel();
      }}
    >
      <h2 id={titleId}>{title}</h2>
      {children}
      <div className="dialog-actions">
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={onCancel}
        >
          Cancel
        </button>
        <button
          type="button"
          className={`btn ${danger ? "btn-danger" : "btn-primary"}`}
          disabled={busy}
          onClick={onConfirm}
        >
          {busy ? busyLabel : confirmLabel}
        </button>
      </div>
    </dialog>
  );
}

/** A failed read, with the one action that helps: trying it again. */
export function LoadError({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => void;
}) {
  return (
    <div className="notice notice-error" role="alert">
      <p>{message}</p>
      <button type="button" className="btn btn-sm" onClick={onRetry}>
        Try again
      </button>
    </div>
  );
}

/** Background rules for a section, available on request instead of always
 * filling the screen. */
export function HowItWorks({
  children,
  label = "How this works",
}: {
  children: ReactNode;
  label?: string;
}) {
  return (
    <details className="how-it-works">
      <summary>{label}</summary>
      <div className="how-it-works-body">{children}</div>
    </details>
  );
}
