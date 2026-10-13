import { useEffect, useEffectEvent, useState } from "react";
import { readCommandResult, type CommandResult } from "../lib/ipc";
import { errorText } from "../lib/recordFeedback";

/**
 * Loads one read command for `key` (null skips loading). A new key or a
 * reload() runs it again; the previous data stays visible until the new
 * result arrives, so reloading never flashes an empty screen.
 */
export function useCommandQuery<T>(
  key: string | null,
  load: () => Promise<CommandResult<T>>,
) {
  const [version, setVersion] = useState(0);
  const [result, setResult] = useState<{
    token: string;
    data: T | null;
    error: string;
  } | null>(null);
  const token = key === null ? null : `${key}#${version}`;
  const read = useEffectEvent(load);
  useEffect(() => {
    if (token === null) return;
    let canceled = false;
    read()
      .then(readCommandResult)
      .then(
        (data) => {
          if (!canceled) setResult({ token, data, error: "" });
        },
        (error: unknown) => {
          if (!canceled)
            setResult({ token, data: null, error: errorText(error) });
        },
      );
    return () => {
      canceled = true;
    };
  }, [token]);
  const settled = result !== null && result.token === token;
  return {
    data: result?.data ?? null,
    error: settled ? result.error : "",
    loading: token !== null && !settled,
    reload: () => setVersion((v) => v + 1),
  };
}
