import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { LoadError, RecordFeedback } from "../RecordControls";
import type { GenerationResult } from "../lib/generation";
import { readCommandResult } from "../lib/ipc";
import { errorText, type Feedback } from "../lib/recordFeedback";
import {
  getScheduleDashboard,
  listSchedules,
  type DashboardData,
  type Schedule,
} from "../lib/schedules";
import { GenerateDialog } from "./GenerateDialog";
import { PlanWorkspace } from "./PlanWorkspace";
import { SetupChecklist } from "./SetupChecklist";
import { StaleCurrentNotice } from "./StaleCurrentNotice";
import { useCurrentSelection } from "./useCurrentSelection";

/**
 * The Schedule screen. It opens the plan named by `?id`, or else the current
 * plan, or else the most recent one; that fallback never changes the saved
 * current selection. `?open=generate` and `?open=issues` open those dialogs
 * once. With no plans it shows what to set up before generating.
 */
export function ScheduleWorkspace() {
  const [params, setParams] = useSearchParams();
  const navigate = useNavigate();
  const requestedId = params.get("id");
  const openRequest = params.get("open");
  const [refresh, setRefresh] = useState(0);
  const [loaded, setLoaded] = useState<{
    dashboard: DashboardData;
    schedules: readonly Schedule[];
    archivedCount: number;
  } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState<Feedback | null>(null);
  const [generating, setGenerating] = useState(false);
  const [generated, setGenerated] = useState<Schedule | null>(null);
  const [seenOpenRequest, setSeenOpenRequest] = useState<string | null>(null);
  if (openRequest !== seenOpenRequest) {
    setSeenOpenRequest(openRequest);
    if (openRequest === "generate") setGenerating(true);
  }
  // What follows the Generate dialog once it closes; undefined while open.
  const afterGenerate = useRef<"draft" | "issues" | null | undefined>(
    undefined,
  );
  // Where focus goes once the next plan (or the empty state) is on screen.
  const pendingFocus = useRef<"switch" | "removed" | null>(null);
  const live = useRef<HTMLParagraphElement>(null);
  useEffect(() => {
    function completed(event: Event) {
      const { origin, status } = (
        event as CustomEvent<{ origin: string; status: string }>
      ).detail;
      if (status === "success" && origin !== window.location.hash)
        setRefresh((value) => value + 1);
    }
    window.addEventListener("shifthappens:operation", completed);
    return () =>
      window.removeEventListener("shifthappens:operation", completed);
  }, []);
  useEffect(() => {
    let canceled = false;
    async function load() {
      setLoading(true);
      try {
        const [dashboard, schedules] = await Promise.all([
          getScheduleDashboard(),
          listSchedules("non_archived"),
        ]);
        const value = {
          dashboard: readCommandResult(dashboard),
          schedules: readCommandResult(schedules),
        };
        // With no open plans, the empty state points to archived ones.
        const archivedCount =
          !value.dashboard.shown && value.schedules.length === 0
            ? readCommandResult(await listSchedules("archived")).length
            : 0;
        if (!canceled) {
          setLoaded({ ...value, archivedCount });
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
  }, [refresh]);
  const dashboard = loaded?.dashboard;
  const current = dashboard?.current ?? null;
  const resolvedId = requestedId ?? dashboard?.shown?.schedule.id ?? null;
  // One-time address requests are removed once the plan they apply to (or
  // the empty state) is on screen, which has read them by then.
  const settled = requestedId !== null || loaded !== null || error !== "";
  useEffect(() => {
    if (!settled || (!params.has("open") && !params.has("assignment"))) return;
    const next = new URLSearchParams(params);
    next.delete("open");
    next.delete("assignment");
    setParams(next, { replace: true });
  }, [settled, params, setParams]);
  // After switching plans, focus moves to the new plan's name. After a plan
  // is deleted, it moves to the heading of whatever replaces it.
  useEffect(() => {
    const pending = pendingFocus.current;
    if (pending === null || (pending === "removed" ? !loaded : !resolvedId))
      return;
    pendingFocus.current = null;
    const heading = document.getElementById("page-title");
    heading?.focus();
    if (pending === "switch" && live.current)
      live.current.textContent = `Opened ${heading?.textContent ?? "schedule"}`;
  }, [resolvedId, loaded]);
  useEffect(() => {
    if (generating || afterGenerate.current === undefined) return;
    const next = afterGenerate.current;
    afterGenerate.current = undefined;
    if (next === "draft") document.getElementById("page-title")?.focus();
    else if (next === null) document.getElementById("ws-generate")?.focus();
  }, [generating]);
  const selection = useCurrentSelection(
    current?.setting_revision ?? null,
    (result, changed) => {
      setNotice(result);
      if (changed) setRefresh((v) => v + 1);
    },
  );
  function created(result: GenerationResult) {
    setGenerated(result.schedule);
    setRefresh((v) => v + 1);
    void navigate(`/?id=${encodeURIComponent(result.schedule.id)}`);
  }
  function deleted(schedule: Schedule) {
    setNotice({
      message: `Deleted ${schedule.name}.`,
      stale: false,
      failed: false,
    });
    // The list still names the deleted plan, so the workspace waits for a
    // fresh one before choosing what to show.
    pendingFocus.current = "removed";
    setLoaded(null);
    setRefresh((v) => v + 1);
    void navigate("/", { replace: true });
  }
  function closeGenerate(next: "draft" | "issues" | null) {
    afterGenerate.current = next;
    setGenerating(false);
    if (next === "issues" && generated)
      setParams({ id: generated.id, open: "issues" }, { replace: true });
  }
  const schedules = loaded?.schedules ?? [];
  const shown =
    dashboard?.shown?.schedule.id === resolvedId ? dashboard.shown : null;
  const initial =
    schedules.find((s) => s.id === resolvedId) ??
    shown?.schedule ??
    (generated?.id === resolvedId ? generated : null);
  const isCurrent =
    current?.kind === "current" && current.schedule.id === resolvedId;
  return (
    <section
      className="page page-workspace"
      aria-labelledby="page-title"
      aria-busy={loading && !loaded}
    >
      {current?.kind === "stale_setting" && (
        <StaleCurrentNotice busy={selection.busy} onClear={selection.clear} />
      )}
      <RecordFeedback
        feedback={notice}
        busy={selection.busy}
        onReload={() => {
          setNotice(null);
          setRefresh((v) => v + 1);
        }}
        reloadLabel="Reload"
      />
      {error && (
        <LoadError message={error} onRetry={() => setRefresh((v) => v + 1)} />
      )}
      {loaded && resolvedId && <SetupChecklist variant="banner" refresh={0} />}
      {resolvedId ? (
        <PlanWorkspace
          key={resolvedId}
          scheduleId={resolvedId}
          initial={initial}
          initialOpenShifts={
            shown?.open_shift_count ??
            dashboard?.recent.find((r) => r.schedule.id === resolvedId)
              ?.open_shift_count ??
            null
          }
          kind={
            isCurrent
              ? "current"
              : requestedId === null && current?.kind !== "current"
                ? "most_recent"
                : null
          }
          current={current}
          schedules={schedules}
          onPlansChanged={() => setRefresh((v) => v + 1)}
          onDeleted={deleted}
          onGenerate={() => setGenerating(true)}
          onSwitch={() => {
            pendingFocus.current = "switch";
          }}
        />
      ) : (
        <>
          <header className="page-header">
            <div className="page-heading">
              <h1 id="page-title" tabIndex={-1} className="ws-title">
                Schedule
              </h1>
              {loaded && (
                <p className="page-lede">
                  No draft or final schedules yet. Finish setting up, then
                  generate a complete draft.
                </p>
              )}
            </div>
            {loaded && (
              <div className="page-actions">
                <button
                  id="ws-generate"
                  type="button"
                  className="btn btn-primary"
                  aria-haspopup="dialog"
                  onClick={() => setGenerating(true)}
                >
                  Generate…
                </button>
              </div>
            )}
          </header>
          {loading && !loaded && !error && (
            <p className="status-line" role="status">
              Loading schedules…
            </p>
          )}
          {loaded && (
            <>
              <SetupChecklist variant="full" refresh={refresh} />
              {loaded.archivedCount > 0 && (
                <p className="ws-archived-note">
                  <Link to="/schedules?show=archived">
                    {loaded.archivedCount}{" "}
                    {loaded.archivedCount === 1
                      ? "archived schedule"
                      : "archived schedules"}{" "}
                    in All schedules
                  </Link>
                </p>
              )}
            </>
          )}
        </>
      )}
      {generating && (
        <GenerateDialog onCreated={created} onClose={closeGenerate} />
      )}
      <p ref={live} className="visually-hidden" aria-live="polite" />
    </section>
  );
}
