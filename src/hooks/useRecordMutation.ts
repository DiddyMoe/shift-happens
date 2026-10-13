import { useCallback, useRef, useState } from "react";
import type { CommandResult } from "../lib/ipc";
import {
  errorText,
  mutationFeedback,
  type Feedback,
} from "../lib/recordFeedback";

export interface MutationOptions<T> {
  /** Shown when the change is saved. */
  success: string;
  onSuccess?: (data: T) => void;
  /** Called when the change did not go through, e.g. to close a dialog. */
  onFailure?: () => void;
  /** Returns false to drop a result that arrives after the user moved on. */
  stillRelevant?: () => boolean;
}

/** Runs one record change at a time and keeps its outcome as feedback. */
export function useRecordMutation() {
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const inFlight = useRef(false);
  const run = useCallback(
    async <T>(
      operation: () => Promise<CommandResult<T>>,
      options: MutationOptions<T>,
    ): Promise<CommandResult<T> | null> => {
      if (inFlight.current) return null;
      inFlight.current = true;
      setBusy(true);
      setFeedback(null);
      const relevant = options.stillRelevant ?? (() => true);
      try {
        const result = await operation();
        if (!relevant()) return result;
        mutationFeedback(
          result,
          (data) => options.onSuccess?.(data),
          setFeedback,
          options.success,
        );
        if (result.status !== "success" && result.status !== "no_change")
          options.onFailure?.();
        return result;
      } catch (error) {
        if (relevant()) {
          options.onFailure?.();
          setFeedback({
            message: errorText(error),
            stale: false,
            failed: true,
          });
        }
        return null;
      } finally {
        inFlight.current = false;
        setBusy(false);
      }
    },
    [],
  );
  return { busy, feedback, setFeedback, run };
}
