import { useEffect, useEffectEvent } from "react";

interface OperationDetail {
  readonly command: string;
  readonly origin: string;
  readonly status: string;
}

/** Calls `onChange` when a change started on another screen is saved, so this
 * screen can reload what it shows. Changes made here are handled locally. */
export function useOtherScreenChanges(onChange: () => void) {
  const changed = useEffectEvent(onChange);
  useEffect(() => {
    function completed(event: Event) {
      const { origin, status } = (event as CustomEvent<OperationDetail>).detail;
      if (status === "success" && origin !== window.location.hash) changed();
    }
    window.addEventListener("shifthappens:operation", completed);
    return () =>
      window.removeEventListener("shifthappens:operation", completed);
  }, []);
}
