import {
  useEffect,
  useEffectEvent,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useSearchParams } from "react-router";
import { Finalization } from "../Finalization";
import { Confirmation, LoadError, RecordFeedback } from "../RecordControls";
import { ScheduleBoard } from "../ScheduleBoard";
import {
  ScheduleOutputActions,
  type OutputDialogKind,
} from "../ScheduleOutput";
import { employeeLabel, useWorkweekStart } from "../lib/board";
import { trapDialogFocus } from "../lib/dialogFocus";
import {
  readCommandResult,
  unreachableResult,
  type CommandResult,
} from "../lib/ipc";
import { displayLocalDate } from "../lib/localDate";
import { displayLocalDateTime } from "../lib/localDateTime";
import {
  errorText,
  mutationFeedback,
  type Feedback,
} from "../lib/recordFeedback";
import {
  archiveSchedule,
  canDeleteSchedule,
  deleteSchedule,
  getSchedule,
  listAssignments,
  reopenSchedule,
  updateSchedule,
  type Assignment,
  type CurrentSchedule,
  type Schedule,
  type ScheduleDeletion,
} from "../lib/schedules";
import {
  editAssignment,
  getCandidates,
  getHistory,
  replayAssignment,
  type ActionRequest,
  type Candidates,
  type EditorAction,
  type EditorOutcome,
  type Head,
  type HistoryState,
  type ReplayRequest,
} from "../lib/scheduleEditor";
import { AssignmentSheet, Findings, Minimum } from "./AssignmentSheet";
import { IssuesPanel } from "./IssuesPanel";
import { PlanDetailsDialog } from "./PlanDetailsDialog";
import { PlanMenu } from "./PlanMenu";
import { PlanPicker } from "./PlanPicker";
import { PlanTags } from "./PlanTags";
import { useCurrentSelection } from "./useCurrentSelection";
import { useIssues } from "./useIssues";

type Pending =
  | { kind: "edit"; request: ActionRequest; outcome: EditorOutcome }
  | { kind: "replay"; request: ReplayRequest; outcome: EditorOutcome };
type Lifecycle =
  | { kind: "archive" | "reopen" | "details"; schedule: Schedule }
  | { kind: "delete"; schedule: Schedule; preview: ScheduleDeletion };

const plural = (count: number, one: string, many: string) =>
  `${count} ${count === 1 ? one : many}`;

/**
 * One open plan: its header, actions, board, assignment sheet and issues.
 * The workspace keys this by plan, so switching plans starts fresh.
 */
export function PlanWorkspace({
  scheduleId,
  initial,
  initialOpenShifts,
  kind,
  current,
  schedules,
  onPlansChanged,
  onDeleted,
  onGenerate,
  onSwitch,
}: {
  scheduleId: string;
  /** The plan as listed, shown until its details load. */
  initial: Schedule | null;
  initialOpenShifts: number | null;
  /** Why this plan is open without an explicit choice, if it is. */
  kind: "current" | "most_recent" | null;
  current: CurrentSchedule | null;
  /** Draft and final plans, for switching. */
  schedules: readonly Schedule[];
  onPlansChanged: () => void;
  /** The plan no longer exists; the workspace moves on to another. */
  onDeleted: (schedule: Schedule) => void;
  onGenerate: () => void;
  onSwitch: () => void;
}) {
  const [params, setParams] = useSearchParams();
  // A linked assignment (for example from an old Schedule Editor link) opens
  // once; the workspace removes it from the address.
  const [linkedAssignmentId] = useState(() => params.get("assignment"));
  const linked = useRef(linkedAssignmentId);
  const [refresh, setRefresh] = useState(0);
  const [loaded, setLoaded] = useState<{
    schedule: Schedule;
    rows: readonly Assignment[];
    history: HistoryState;
  } | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [candidateSnapshot, setCandidateRead] = useState<Candidates | null>(
    null,
  );
  const [candidateId, setCandidateId] = useState("");
  // The open shift's unsaved note. It survives that shift reloading after
  // other actions; choosing a shift starts again from its saved note.
  const [noteDraft, setNoteDraft] = useState<{
    id: string;
    text: string;
  } | null>(null);
  const [loading, setLoading] = useState(true);
  const [candidateLoading, setCandidateLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [candidateError, setCandidateError] = useState("");
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [lifecycle, setLifecycle] = useState<Lifecycle | null>(null);
  // The assignment the sheet was opened for. It shows only while that exact
  // assignment is selected, so a stale link never opens a different shift.
  const [sheetFor, setSheetFor] = useState<string | null>(linkedAssignmentId);
  const [issuesOpen, setIssuesOpen] = useState(false);
  // The finding control to focus when Issues opens again after its shift.
  const [issuesFocus, setIssuesFocus] = useState<string | null>(null);
  const [returnTo, setReturnTo] = useState<string | null>(null);
  const [output, setOutput] = useState<OutputDialogKind | null>(null);
  const [titleFocus, setTitleFocus] = useState(0);
  // `?open=issues` opens Issues each time it appears in the address.
  const issuesRequested = params.get("open") === "issues";
  const [seenIssuesRequest, setSeenIssuesRequest] = useState(false);
  if (issuesRequested !== seenIssuesRequest) {
    setSeenIssuesRequest(issuesRequested);
    if (issuesRequested) {
      setSheetFor(null);
      setReturnTo(null);
      setIssuesFocus(null);
      setIssuesOpen(true);
    }
  }
  const sheet = useRef<HTMLDialogElement>(null);
  const issuesDialog = useRef<HTMLDialogElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const issuesButton = useRef<HTMLButtonElement>(null);
  const weekStart = useWorkweekStart();
  const operation = useRef(0);
  const mutating = useRef(false);
  useEffect(() => {
    function completed(event: Event) {
      const { origin, status } = (
        event as CustomEvent<{ origin: string; status: string }>
      ).detail;
      if (status === "success" && origin !== window.location.hash) {
        setCandidateRead(null);
        setRefresh((value) => value + 1);
      }
    }
    window.addEventListener("shifthappens:operation", completed);
    return () =>
      window.removeEventListener("shifthappens:operation", completed);
  }, []);
  useLayoutEffect(() => {
    operation.current += 1;
    return () => {
      operation.current += 1;
    };
  }, [scheduleId, selectedId]);
  useEffect(() => {
    let canceled = false;
    async function load() {
      setLoading(true);
      setPending(null);
      setLifecycle(null);
      try {
        // Sequential detail reads use parent revision checks below to detect an
        // intervening write before displaying candidate controls.
        const schedule = readCommandResult(await getSchedule(scheduleId));
        const assignments = readCommandResult(
          await listAssignments(scheduleId),
        );
        const history = readCommandResult(await getHistory(scheduleId));
        const latest = readCommandResult(await getSchedule(scheduleId));
        if (latest.revision !== schedule.revision)
          throw new Error(
            "The schedule changed while loading. Reload its latest revision.",
          );
        if (!canceled) {
          const linkedId = linked.current;
          linked.current = null;
          setLoaded({ schedule, rows: assignments, history });
          setSelectedId((id) =>
            linkedId && assignments.some((r) => r.id === linkedId)
              ? linkedId
              : assignments.some((r) => r.id === id)
                ? id
                : "",
          );
          setError("");
        }
      } catch (e) {
        if (!canceled) {
          setLoaded(null);
          setError(errorText(e));
        }
      } finally {
        if (!canceled) setLoading(false);
      }
    }
    void load();
    return () => {
      canceled = true;
    };
  }, [scheduleId, refresh]);
  const detail = loaded?.schedule.id === scheduleId ? loaded : null;
  const row = detail?.rows.find((r) => r.id === selectedId);
  const candidateRead =
    candidateSnapshot?.schedule_id === detail?.schedule.id &&
    candidateSnapshot?.assignment_id === row?.id &&
    candidateSnapshot?.assignment_revision === row?.revision &&
    candidateSnapshot?.schedule_revision === detail?.schedule.revision
      ? candidateSnapshot
      : null;
  const savedNotes = row?.notes ?? "";
  // A draft that matches the saved note is no longer unsaved, so a later
  // change to the saved note (for example Undo) shows.
  if (noteDraft && noteDraft.id === row?.id && noteDraft.text === savedNotes)
    setNoteDraft(null);
  const notes =
    noteDraft && noteDraft.id === row?.id ? noteDraft.text : savedNotes;
  useEffect(() => {
    let canceled = false;
    if (!row || !detail) return;
    const selectedRow = row;
    const selectedSchedule = detail.schedule;
    async function load() {
      setCandidateRead(null);
      setCandidateId("");
      setCandidateError("");
      if (selectedSchedule.status !== "draft" || selectedSchedule.archived_at)
        return;
      setCandidateLoading(true);
      try {
        const result = readCommandResult(
          await getCandidates(selectedSchedule.id, selectedRow.id),
        );
        if (
          result.assignment_revision !== selectedRow.revision ||
          result.schedule_revision !== selectedSchedule.revision
        )
          throw new Error(
            "Candidates refer to a newer schedule revision. Reload the schedule.",
          );
        if (!canceled) setCandidateRead(result);
      } catch (e) {
        if (!canceled) setCandidateError(errorText(e));
      } finally {
        if (!canceled) setCandidateLoading(false);
      }
    }
    void load();
    return () => {
      canceled = true;
    };
  }, [row, detail]);
  const issues = useIssues(
    scheduleId,
    detail !== null,
    detail?.rows.length ?? 0,
  );
  const employeeNames = useMemo(() => {
    const names = new Map<string, string>();
    for (const assignment of detail?.rows ?? [])
      if (assignment.assigned_employee_id)
        names.set(assignment.assigned_employee_id, employeeLabel(assignment));
    return names;
  }, [detail]);
  /** After a plan-level change, keep this plan open even if the workspace's
   * default plan changes, then refresh the plan list. */
  function planChanged() {
    if (!params.has("id")) setParams({ id: scheduleId }, { replace: true });
    onPlansChanged();
  }
  const selection = useCurrentSelection(
    current?.setting_revision ?? null,
    (result, changed) => {
      setFeedback(result);
      if (changed) planChanged();
    },
  );
  function reload() {
    setPending(null);
    setFeedback(null);
    setRefresh((v) => v + 1);
  }
  function outcome(
    result: CommandResult<EditorOutcome>,
    request: ActionRequest | ReplayRequest,
  ) {
    switch (result.status) {
      case "success":
      case "no_change":
        setPending(null);
        setFeedback({
          message: [
            result.status === "success" ? "Saved." : "No changes were needed.",
            ...(result.status === "success"
              ? (result.warnings ?? []).map((w) => w.message)
              : []),
          ].join(" "),
          stale: false,
          failed: false,
        });
        // Show the note as saved until the reload confirms it.
        if ("action" in request && request.action.kind === "note")
          setNoteDraft({
            id: result.data.assignment.id,
            text: result.data.assignment.notes ?? "",
          });
        setSelectedId(result.data.assignment.id);
        setRefresh((v) => v + 1);
        return;
      case "canceled":
      case "requires_overwrite":
        setPending(null);
        setFeedback({
          message:
            result.status === "canceled"
              ? "Action canceled."
              : "Review the selected file before confirming replacement.",
          stale: false,
          failed: false,
        });
        return;
      case "requires_confirmation":
        setPending(
          "action" in request
            ? { kind: "edit", request, outcome: result.data }
            : { kind: "replay", request, outcome: result.data },
        );
        return;
      case "stale_revision":
      case "blocked":
      case "validation_error":
      case "failure":
        setPending(null);
        setFeedback({
          message: `${result.error.message} (${result.error.code})`,
          stale: true,
          failed: true,
        });
        return;
      default:
        unreachableResult(result);
    }
  }
  async function submit(request: ActionRequest | ReplayRequest) {
    if (mutating.current) return;
    mutating.current = true;
    const serial = operation.current;
    setBusy(true);
    setFeedback(null);
    try {
      const result = await ("action" in request
        ? editAssignment(request)
        : replayAssignment(request));
      if (operation.current === serial) outcome(result, request);
    } catch (e) {
      if (operation.current === serial) {
        setPending(null);
        setFeedback({ message: errorText(e), stale: true, failed: true });
      }
    } finally {
      mutating.current = false;
      setBusy(false);
    }
  }
  function edit(action: EditorAction) {
    if (!row || !detail) return;
    void submit({
      schedule_id: detail.schedule.id,
      assignment_id: row.id,
      expected_assignment_revision: row.revision,
      expected_schedule_revision: detail.schedule.revision,
      action,
      acknowledged_warning_ids: null,
    });
  }
  function replay(head: Head | null) {
    if (head && detail) {
      const { action: _action, ...request } = head;
      void _action;
      void submit({
        schedule_id: detail.schedule.id,
        ...request,
        acknowledged_warning_ids: null,
      });
    }
  }
  function confirm() {
    if (
      !pending ||
      pending.request.schedule_id !== detail?.schedule.id ||
      pending.request.expected_schedule_revision !== detail.schedule.revision
    )
      return;
    // The request's head, target and revisions remain exactly those reviewed.
    void submit({
      ...pending.request,
      acknowledged_warning_ids: pending.outcome.analysis.warnings.map(
        (w) => w.warning_id,
      ),
    });
  }
  async function reopen() {
    if (lifecycle?.kind !== "reopen" || mutating.current) return;
    const captured = lifecycle.schedule;
    const serial = operation.current;
    mutating.current = true;
    setBusy(true);
    try {
      const result = await reopenSchedule(captured.id, captured.revision);
      if (operation.current !== serial) return;
      readCommandResult(result);
      setLifecycle(null);
      setRefresh((v) => v + 1);
      planChanged();
      setFeedback({
        message:
          result.status === "no_change"
            ? "No changes were needed. This schedule is already draft."
            : `Reopened as draft. Undo and redo start empty. ${result.status === "success" ? (result.warnings?.map((warning) => `${warning.message} (${warning.code})`).join(" ") ?? "") : ""}`,
        stale: false,
        failed: false,
      });
    } catch (e) {
      if (operation.current === serial) {
        setLifecycle(null);
        setFeedback({ message: errorText(e), stale: true, failed: true });
      }
    } finally {
      mutating.current = false;
      setBusy(false);
    }
  }
  /** Archive or rename: plan-level changes reported as one message. */
  async function mutatePlan<T>(
    run: () => Promise<CommandResult<T>>,
    message: string,
    focusTitle: boolean,
  ) {
    if (mutating.current) return;
    mutating.current = true;
    const serial = operation.current;
    setBusy(true);
    setFeedback(null);
    try {
      const result = await run();
      if (operation.current !== serial) return;
      mutationFeedback(
        result,
        () => {
          setLifecycle(null);
          setRefresh((v) => v + 1);
          planChanged();
          if (focusTitle) setTitleFocus((v) => v + 1);
        },
        setFeedback,
        message,
      );
      // A failed confirmation must close so the error and its reload are visible.
      if (result.status !== "success" && result.status !== "no_change")
        setLifecycle(null);
    } catch (e) {
      if (operation.current === serial) {
        setLifecycle(null);
        setFeedback({ message: errorText(e), stale: false, failed: true });
      }
    } finally {
      mutating.current = false;
      setBusy(false);
    }
  }
  /** Asks the backend what deleting would remove, then confirms. */
  async function previewDelete(target: Schedule) {
    if (mutating.current) return;
    mutating.current = true;
    const serial = operation.current;
    setBusy(true);
    setFeedback(null);
    try {
      const result = await deleteSchedule(target.id, target.revision, false);
      if (operation.current !== serial) return;
      if (result.status === "requires_confirmation")
        setLifecycle({
          kind: "delete",
          schedule: target,
          preview: result.data,
        });
      else
        setFeedback(
          "error" in result
            ? {
                message: `${result.error.message} (${result.error.code})`,
                stale: result.status === "stale_revision",
                failed: true,
              }
            : {
                message: "Reload this schedule before deleting it.",
                stale: true,
                failed: true,
              },
        );
    } catch (e) {
      if (operation.current === serial)
        setFeedback({ message: errorText(e), stale: false, failed: true });
    } finally {
      mutating.current = false;
      setBusy(false);
    }
  }
  async function confirmDelete() {
    if (lifecycle?.kind !== "delete" || mutating.current) return;
    const target = lifecycle.schedule;
    mutating.current = true;
    const serial = operation.current;
    setBusy(true);
    try {
      const result = await deleteSchedule(target.id, target.revision, true);
      if (operation.current !== serial) return;
      readCommandResult(result);
      setLifecycle(null);
      onDeleted(target);
    } catch (e) {
      if (operation.current === serial) {
        setLifecycle(null);
        setFeedback({ message: errorText(e), stale: true, failed: true });
      }
    } finally {
      mutating.current = false;
      setBusy(false);
    }
  }
  const disabled = busy || loading || pending !== null || lifecycle !== null;
  const readOnly =
    !detail ||
    detail.schedule.status !== "draft" ||
    detail.schedule.archived_at !== null;
  const candidate = candidateRead?.candidates.find(
    (e) => e.employee_id === candidateId,
  );
  const sheetVisible = sheetFor !== null && row?.id === sheetFor;
  // Issues and the sheet are both modal and never open together. Closing
  // first keeps a closing dialog from returning focus after the other opens.
  useEffect(() => {
    const sheetModal = sheet.current;
    const issuesModal = issuesDialog.current;
    if (!sheetVisible && sheetModal?.open) sheetModal.close();
    if (!issuesOpen && issuesModal?.open) issuesModal.close();
    if (sheetVisible && sheetModal && !sheetModal.open) sheetModal.showModal();
    if (issuesOpen && issuesModal && !issuesModal.open) {
      issuesModal.showModal();
      const target = issuesFocus
        ? issuesModal.querySelector<HTMLElement>(
            `[data-finding-id="${CSS.escape(issuesFocus)}"]`,
          )
        : null;
      (
        target ?? issuesModal.querySelector<HTMLElement>("#issues-title")
      )?.focus();
    }
  }, [sheetVisible, issuesOpen, issuesFocus]);
  useEffect(() => {
    if (titleFocus > 0) title.current?.focus();
  }, [titleFocus]);
  const plan = detail?.schedule ?? initial;
  const final =
    plan !== null && plan.status === "final" && plan.archived_at === null;
  const isCurrent =
    current?.kind === "current" && current.schedule.id === scheduleId;
  function undo() {
    if (disabled || readOnly || !detail?.history.undo) return;
    replay(detail.history.undo);
  }
  function redo() {
    if (disabled || readOnly || !detail?.history.redo) return;
    replay(detail.history.redo);
  }
  function showIssues() {
    setSheetFor(null);
    setReturnTo(null);
    setIssuesFocus(null);
    setIssuesOpen(true);
    if (issues.status === "idle") issues.run();
  }
  function openFromIssues(focusKey: string, assignmentId: string) {
    const target = detail?.rows.find((r) => r.id === assignmentId);
    if (!target) return;
    setIssuesOpen(false);
    setReturnTo(focusKey);
    setNoteDraft(null);
    setSelectedId(target.id);
    setSheetFor(target.id);
  }
  const shortcut = useEffectEvent((event: KeyboardEvent) => {
    if (
      event.altKey ||
      event.metaKey ||
      !event.ctrlKey ||
      event.defaultPrevented ||
      event.repeat ||
      document.querySelector("dialog[open]")
    )
      return;
    const target = event.target;
    if (
      target instanceof HTMLElement &&
      (target.isContentEditable ||
        target.closest("input, textarea, select") !== null)
    )
      return;
    const key = event.key.toLowerCase();
    if (key === "p" && !event.shiftKey) {
      event.preventDefault();
      if (plan && !disabled) setOutput("print");
    } else if (key === "z" && !event.shiftKey) {
      event.preventDefault();
      undo();
    } else if (key === "y" || (key === "z" && event.shiftKey)) {
      event.preventDefault();
      redo();
    }
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => shortcut(event);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
  const openShifts = detail
    ? detail.rows.filter((a) => a.open_shift).length
    : initialOpenShifts;
  const result = issues.result;
  const issuesLabel =
    issues.status === "checking"
      ? "Issues: checking"
      : issues.status === "failed"
        ? "Issues: check failed"
        : issues.status === "idle"
          ? "Issues: not checked"
          : result && result.error_count + result.warning_count === 0
            ? "Issues: none found"
            : `Issues: ${plural(result?.error_count ?? 0, "error", "errors")}, ${plural(result?.warning_count ?? 0, "warning", "warnings")}`;
  function toolbar(finalizeButton: ReactNode) {
    if (!plan) return null;
    return (
      <div className="ws-toolbar" role="group" aria-label="Schedule actions">
        <div className="ws-toolbar-group">
          {detail && !readOnly && (
            <>
              <button
                type="button"
                className="btn"
                disabled={disabled || !detail.history.undo}
                aria-label={`Undo (${detail.history.undo_count})`}
                aria-keyshortcuts="Control+Z"
                title="Undo (Ctrl+Z)"
                onClick={undo}
              >
                Undo
                {detail.history.undo_count > 0 && (
                  <span className="ws-count">{detail.history.undo_count}</span>
                )}
              </button>
              <button
                type="button"
                className="btn"
                disabled={disabled || !detail.history.redo}
                aria-label={`Redo (${detail.history.redo_count})`}
                aria-keyshortcuts="Control+Y Control+Shift+Z"
                title="Redo (Ctrl+Y)"
                onClick={redo}
              >
                Redo
                {detail.history.redo_count > 0 && (
                  <span className="ws-count">{detail.history.redo_count}</span>
                )}
              </button>
            </>
          )}
          <button
            ref={issuesButton}
            type="button"
            className="btn ws-issues"
            aria-haspopup="dialog"
            aria-label={issuesLabel}
            onClick={showIssues}
          >
            Issues
            <span className="ws-issues-status" aria-hidden="true">
              {issues.status === "checking" && "checking…"}
              {issues.status === "failed" && "check failed"}
              {issues.status === "idle" && "check"}
              {issues.status === "done" &&
                result &&
                (result.error_count + result.warning_count === 0 ? (
                  <span className="ws-issue is-ok">none</span>
                ) : (
                  <>
                    {result.error_count > 0 && (
                      <span className="ws-issue is-error">
                        {plural(result.error_count, "error", "errors")}
                      </span>
                    )}
                    {result.warning_count > 0 && (
                      <span className="ws-issue is-warn">
                        {plural(result.warning_count, "warning", "warnings")}
                      </span>
                    )}
                  </>
                ))}
            </span>
          </button>
        </div>
        <div className="ws-toolbar-group ws-toolbar-end">
          <ScheduleOutputActions
            schedule={plan}
            disabled={disabled}
            open={output}
            onOpenChange={setOutput}
            primary={final ? "print" : undefined}
          />
          {finalizeButton}
          <PlanMenu
            schedule={plan}
            isCurrent={isCurrent}
            canDelete={detail !== null && canDeleteSchedule(detail.schedule)}
            disabled={disabled || selection.busy || !detail}
            onSetCurrent={() => selection.set(scheduleId)}
            onClearCurrent={selection.clear}
            onDetails={() => setLifecycle({ kind: "details", schedule: plan })}
            onArchive={() => setLifecycle({ kind: "archive", schedule: plan })}
            onReopen={() => setLifecycle({ kind: "reopen", schedule: plan })}
            onDelete={() => void previewDelete(plan)}
          />
        </div>
      </div>
    );
  }
  return (
    <>
      <header className="page-header ws-header">
        <div className="ws-heading">
          {(kind || plan) && (
            <p className="ws-kind">
              {kind === "current" && (
                <span className="tag tag-current">Current</span>
              )}
              {kind === "most_recent" && (
                <span className="tag">Most recent</span>
              )}
              {plan && <PlanTags schedule={plan} current={false} />}
            </p>
          )}
          <h1 id="page-title" ref={title} tabIndex={-1} className="ws-title">
            {plan?.name ?? "Schedule"}
          </h1>
          {plan && (
            <p className="ws-facts">
              <span className="display">
                {displayLocalDate(plan.start_date)} to{" "}
                {displayLocalDate(plan.end_date)}
              </span>
              {openShifts !== null && (
                <span
                  className={openShifts > 0 ? "ws-open has-open" : "ws-open"}
                >
                  <span className="display">{openShifts}</span>{" "}
                  {openShifts === 1 ? "open shift" : "open shifts"}
                </span>
              )}
            </p>
          )}
        </div>
        <div className="page-actions">
          <PlanPicker
            schedules={schedules}
            openId={scheduleId}
            currentId={current?.kind === "current" ? current.schedule.id : null}
            onSwitch={onSwitch}
          />
          <button
            id="ws-generate"
            type="button"
            className="btn"
            aria-haspopup="dialog"
            onClick={onGenerate}
          >
            Generate…
          </button>
        </div>
      </header>
      {detail ? (
        <Finalization
          schedule={detail.schedule}
          disabled={disabled}
          onShowIssues={showIssues}
          onValidation={issues.seed}
          onChanged={(schedule, historyUnavailable) => {
            setPending(null);
            setCandidateRead(null);
            if (schedule.status === "draft") {
              reload();
              setFeedback({
                message:
                  "The schedule changed. Review the refreshed schedule before finalizing.",
                stale: true,
                failed: true,
              });
              return;
            }
            // The finalization result replaces any earlier edit message.
            setFeedback(null);
            setLoaded((value) =>
              value?.schedule.id === schedule.id
                ? {
                    ...value,
                    schedule,
                    history: {
                      undo: null,
                      redo: null,
                      undo_count: 0,
                      redo_count: 0,
                      unavailable:
                        historyUnavailable ?? value.history.unavailable,
                    },
                  }
                : value,
            );
            planChanged();
          }}
        >
          {toolbar}
        </Finalization>
      ) : (
        toolbar(null)
      )}
      {!sheetVisible && (
        <RecordFeedback
          feedback={feedback}
          busy={busy}
          onReload={() => {
            reload();
            onPlansChanged();
          }}
          reloadLabel="Reload schedule"
        />
      )}
      {loading && !detail && !error && (
        <p className="status-line" role="status">
          Loading saved schedule…
        </p>
      )}
      {error && <LoadError message={error} onRetry={reload} />}
      {detail && (
        <>
          {detail.history.unavailable && (
            <p className="notice notice-error">
              Undo and redo are unavailable. Saved database changes remain
              saved.
            </p>
          )}
          {final && (
            <p className="notice notice-info">
              This final schedule is read-only. Use More › Reopen as draft to
              make changes.
            </p>
          )}
          {detail.schedule.archived_at && (
            <p className="notice notice-info">
              This archived schedule is read-only.
            </p>
          )}
          {detail.rows.length > 0 ? (
            <ScheduleBoard
              assignments={detail.rows}
              startDate={detail.schedule.start_date}
              endDate={detail.schedule.end_date}
              weekStart={weekStart}
              selectedId={sheetVisible ? selectedId : ""}
              focusDate={sheetVisible ? (row?.schedule_date ?? null) : null}
              disabled={disabled}
              label={`${detail.schedule.name} assignments`}
              onSelect={(item) => {
                setReturnTo(null);
                setNoteDraft(null);
                setSelectedId(item.id);
                setSheetFor(item.id);
              }}
            />
          ) : (
            <div className="empty">
              <h2>No assignments are stored in this schedule.</h2>
            </div>
          )}
        </>
      )}
      {/* The sheet stays mounted while the schedule reloads after a save, so
          it remains open and shows the result. */}
      <dialog
        ref={sheet}
        className="sheet"
        aria-labelledby="sheet-title"
        onKeyDown={trapDialogFocus}
        onClose={() => {
          setSheetFor(null);
          if (returnTo) {
            setIssuesFocus(returnTo);
            setReturnTo(null);
            setIssuesOpen(true);
          }
        }}
      >
        {row && (
          <AssignmentSheet
            row={row}
            readOnly={readOnly}
            disabled={disabled}
            busy={busy}
            candidateRead={candidateRead}
            candidateLoading={candidateLoading}
            candidateError={candidateError}
            candidateId={candidateId}
            onCandidateChange={setCandidateId}
            candidate={candidate}
            notes={notes}
            onNotesChange={(text) => setNoteDraft({ id: row.id, text })}
            feedback={feedback}
            onEdit={edit}
            onReload={reload}
            closeLabel={returnTo ? "Back to issues" : "Close"}
            onClose={() => sheet.current?.close()}
          />
        )}
      </dialog>
      <IssuesPanel
        dialogRef={issuesDialog}
        result={issues.result}
        status={issues.status}
        error={issues.error}
        checkedAt={issues.checkedAt}
        employeeNames={employeeNames}
        onRun={issues.run}
        onOpenAssignment={openFromIssues}
        onClose={() => {
          setIssuesOpen(false);
          // Opened from a dialog that is gone (for example Generate), the
          // panel has nowhere to return focus to.
          if (
            !document.activeElement ||
            document.activeElement === document.body
          )
            issuesButton.current?.focus();
        }}
      />
      {pending && (
        <Confirmation
          title="Review current assignment warnings"
          busy={busy}
          onConfirm={confirm}
          onCancel={() => setPending(null)}
          confirmLabel="Acknowledge and apply"
          busyLabel="Applying…"
        >
          <p>
            {pending.kind === "replay"
              ? `${pending.request.direction === "undo" ? "Undo" : "Redo"} the displayed history action`
              : `Apply ${pending.request.action.kind}`}{" "}
            for {pending.outcome.assignment.position_name_snapshot},{" "}
            {displayLocalDateTime(
              pending.outcome.assignment.actual_start_datetime,
            )}
            .
          </p>
          <p>
            {pending.outcome.analysis.warnings.length
              ? "Confirming acknowledges every warning below for this exact action and revision."
              : "Warnings changed and are now empty. Confirm the refreshed result to apply the action."}
          </p>
          <Findings
            findings={pending.outcome.analysis.warnings}
            tone="warning"
          />
          <Minimum value={pending.outcome.analysis.source_minimum} source />
        </Confirmation>
      )}
      {lifecycle?.kind === "reopen" && (
        <Confirmation
          title="Reopen this final schedule?"
          busy={busy}
          onConfirm={() => void reopen()}
          onCancel={() => setLifecycle(null)}
          confirmLabel="Reopen as draft"
          busyLabel="Reopening…"
        >
          <p>
            {lifecycle.schedule.name} will become an editable draft. Its history
            starts empty.
          </p>
        </Confirmation>
      )}
      {lifecycle?.kind === "archive" && (
        <Confirmation
          title={`Archive ${lifecycle.schedule.name}?`}
          busy={busy}
          danger
          onConfirm={() => {
            const target = lifecycle.schedule;
            void mutatePlan(
              () => archiveSchedule(target.id, target.revision),
              "Schedule archived. Find it in All schedules under Archived.",
              true,
            );
          }}
          onCancel={() => setLifecycle(null)}
          confirmLabel="Archive schedule"
          busyLabel="Archiving…"
        >
          <p>
            <strong>{lifecycle.schedule.name}</strong>,{" "}
            <span className="display">
              {displayLocalDate(lifecycle.schedule.start_date)} to{" "}
              {displayLocalDate(lifecycle.schedule.end_date)}
            </span>
          </p>
          <p>
            The schedule and all its shifts stay available, read-only, under
            Archived in All schedules. Archiving cannot be undone.
            {isCurrent && " It also stops being the current schedule."}
          </p>
        </Confirmation>
      )}
      {lifecycle?.kind === "delete" && (
        <Confirmation
          title="Delete draft schedule?"
          busy={busy}
          danger
          onConfirm={() => void confirmDelete()}
          onCancel={() => setLifecycle(null)}
          confirmLabel="Delete draft"
          busyLabel="Deleting…"
        >
          <p>
            “{lifecycle.preview.name}” and its{" "}
            {plural(lifecycle.preview.assignment_count, "shift", "shifts")} will
            be permanently deleted. This can’t be undone.
          </p>
          {lifecycle.preview.is_current && (
            <p>
              It’s your current schedule; the current selection will be cleared.
            </p>
          )}
        </Confirmation>
      )}
      {lifecycle?.kind === "details" && (
        <PlanDetailsDialog
          schedule={lifecycle.schedule}
          isCurrent={isCurrent}
          busy={busy}
          onClose={() => setLifecycle(null)}
          onSave={(name, notes) => {
            const target = lifecycle.schedule;
            void mutatePlan(
              () => updateSchedule(target.id, target.revision, name, notes),
              "Name and notes saved.",
              false,
            );
          }}
        />
      )}
    </>
  );
}
