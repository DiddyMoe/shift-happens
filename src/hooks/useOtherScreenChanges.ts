import { useEffect, useEffectEvent } from "react";

interface OperationDetail {
  readonly command: string;
  readonly origin: string;
  /** performance.now() when the change was sent. */
  readonly started: number;
  readonly status: string;
}

/** Calls `onChange` when a change started on another screen is saved, so this
 * screen can reload what it shows. Changes made here are handled locally; one
 * started here before this screen opened (you left and came back) counts as
 * another screen's, because the screen that started it is gone. */
export function useOtherScreenChanges(onChange: () => void) {
  const changed = useEffectEvent(onChange);
  useEffect(() => {
    const opened = performance.now();
    function completed(event: Event) {
      const { origin, started, status } = (
        event as CustomEvent<OperationDetail>
      ).detail;
      if (
        status === "success" &&
        (origin !== window.location.hash || started < opened)
      )
        changed();
    }
    window.addEventListener("shifthappens:operation", completed);
    return () =>
      window.removeEventListener("shifthappens:operation", completed);
  }, []);
}
