import { useEffect, useEffectEvent, useRef, useState } from "react";
import { readCommandResult } from "../lib/ipc";
import { errorText } from "../lib/recordFeedback";
import { validateSchedule, type ValidationResult } from "../lib/validation";

/** Plans larger than this are checked only on request. */
const AUTO_CHECK_LIMIT = 5000;

export type IssuesStatus = "idle" | "checking" | "done" | "failed";

/**
 * The plan's validation findings under current rules. Checking never changes
 * saved data. The result is checked once the plan loads and again shortly
 * after any saved change, except for very large plans, which are checked only
 * when asked.
 */
export function useIssues(
  scheduleId: string,
  ready: boolean,
  rowCount: number,
) {
  const [result, setResult] = useState<ValidationResult | null>(null);
  const [checkedAt, setCheckedAt] = useState<Date | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);
  const auto = rowCount <= AUTO_CHECK_LIMIT;
  async function run() {
    const serial = ++request.current;
    setRunning(true);
    setError("");
    setResult(null);
    try {
      const value = readCommandResult(await validateSchedule(scheduleId));
      if (request.current === serial) {
        setResult(value);
        setCheckedAt(new Date());
      }
    } catch (e) {
      if (request.current === serial) setError(errorText(e));
    } finally {
      if (request.current === serial) setRunning(false);
    }
  }
  const rerun = useEffectEvent(() => {
    if (auto) void run();
  });
  // A saved change anywhere can change findings: the shown result is dropped
  // at once and checked again after changes settle.
  useEffect(() => {
    let timer = 0;
    function invalidate() {
      request.current += 1;
      setRunning(false);
      setResult(null);
      window.clearTimeout(timer);
      timer = window.setTimeout(rerun, 500);
    }
    window.addEventListener("shifthappens:data-changed", invalidate);
    return () => {
      window.removeEventListener("shifthappens:data-changed", invalidate);
      window.clearTimeout(timer);
      request.current += 1;
    };
  }, []);
  // Only the first load of the plan starts a check on its own.
  const checkedOnce = useRef(false);
  const firstCheck = useEffectEvent(() => {
    if (checkedOnce.current) return;
    checkedOnce.current = true;
    if (auto) void run();
  });
  useEffect(() => {
    if (ready) firstCheck();
  }, [ready]);
  const shown = result?.schedule_id === scheduleId ? result : null;
  const status: IssuesStatus = running
    ? "checking"
    : error
      ? "failed"
      : shown
        ? "done"
        : auto && ready
          ? "checking"
          : "idle";
  return {
    result: shown,
    checkedAt,
    status,
    error,
    auto,
    run: () => void run(),
    /** Shows a result that was checked elsewhere, for example by Finalize. */
    seed(value: ValidationResult) {
      if (value.schedule_id !== scheduleId) return;
      request.current += 1;
      setRunning(false);
      setError("");
      setResult(value);
      setCheckedAt(new Date());
    },
  };
}
