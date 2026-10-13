import { useEffect, useRef, useState } from "react";
import { unreachableResult } from "../lib/ipc";
import { errorText, type Feedback } from "../lib/recordFeedback";
import { clearCurrentSchedule, setCurrentSchedule } from "../lib/schedules";

/**
 * Sets or clears the saved current schedule against the setting revision that
 * was read with it. `onResult` receives the message to show and whether the
 * saved selection changed.
 */
export function useCurrentSelection(
  settingRevision: number | null,
  onResult: (feedback: Feedback, changed: boolean) => void,
) {
  const [busy, setBusy] = useState(false);
  const active = useRef(false);
  // A result that arrives after the screen closes is not reported.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  function report(feedback: Feedback, changed: boolean) {
    if (mounted.current) onResult(feedback, changed);
  }
  async function change(scheduleId: string | null) {
    if (active.current || settingRevision === null) return;
    active.current = true;
    setBusy(true);
    const clear = scheduleId === null;
    try {
      const result = await (clear
        ? clearCurrentSchedule(settingRevision)
        : setCurrentSchedule(scheduleId, settingRevision));
      switch (result.status) {
        case "success":
          report(
            {
              message: clear
                ? "Saved current selection cleared."
                : "This is now the current schedule.",
              stale: false,
              failed: false,
            },
            true,
          );
          break;
        case "no_change":
          report(
            {
              message: "No changes were needed.",
              stale: false,
              failed: false,
            },
            false,
          );
          break;
        case "canceled":
          report(
            { message: "Selection canceled.", stale: false, failed: false },
            false,
          );
          break;
        case "stale_revision":
        case "blocked":
        case "validation_error":
        case "failure":
          report(
            {
              message: `${result.error.message} (${result.error.code}) Reload the current selection before retrying.`,
              stale: true,
              failed: true,
            },
            false,
          );
          break;
        case "requires_confirmation":
        case "requires_overwrite":
          report(
            {
              message: "Reload before changing this selection.",
              stale: true,
              failed: true,
            },
            false,
          );
          break;
        default:
          unreachableResult(result);
      }
    } catch (error) {
      report({ message: errorText(error), stale: true, failed: true }, false);
    } finally {
      active.current = false;
      setBusy(false);
    }
  }
  return {
    busy,
    set: (scheduleId: string) => void change(scheduleId),
    clear: () => void change(null),
  };
}
