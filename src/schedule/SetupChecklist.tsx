import { useEffect, useId, useState } from "react";
import { Link } from "react-router";
import { LoadError } from "../RecordControls";
import { errorText } from "../lib/recordFeedback";
import { loadSetupStatus, type SetupItem } from "../lib/setup";

const HIDDEN_KEY = "shifthappens.setupHidden";

function hiddenFor(): string | null {
  try {
    return window.localStorage.getItem(HIDDEN_KEY);
  } catch {
    return null;
  }
}
function saveHidden(value: string) {
  try {
    window.localStorage.setItem(HIDDEN_KEY, value);
  } catch {
    // Hiding is a convenience; the banner simply shows again.
  }
}

const tagClass: Readonly<Record<SetupItem["state"], string>> = {
  ok: "tag tag-ok",
  warn: "tag tag-warn",
  blocking: "tag tag-danger",
};

function Items({
  items,
  onNavigate,
}: {
  items: readonly SetupItem[];
  onNavigate?: (() => void) | undefined;
}) {
  return (
    <ul className="setup-list">
      {items.map((item) => (
        <li key={item.key}>
          <div className="setup-item-head">
            <Link to={item.path} onClick={onNavigate}>
              {item.title}
            </Link>
            <span className={tagClass[item.state]}>{item.tag}</span>
          </div>
          <p className="setup-detail">{item.detail}</p>
        </li>
      ))}
    </ul>
  );
}

/**
 * Setup status before generating. `full` is the checklist shown when no plans
 * exist; `banner` is a collapsed reminder above a plan that can be hidden
 * until the set of unfinished items changes.
 */
export function SetupChecklist({
  variant,
  refresh,
  heading = "Get set up",
  level = 2,
  onNavigate,
}: {
  variant: "full" | "banner";
  refresh: number;
  /** The full checklist's title; Help shows it under its own heading. */
  heading?: string;
  /** Heading level for the title, so it nests under the page or Help. */
  level?: 2 | 4;
  /** Called when an item's link is followed, e.g. to close Help. */
  onNavigate?: (() => void) | undefined;
}) {
  const titleId = useId();
  const [items, setItems] = useState<readonly SetupItem[] | null>(null);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [hidden, setHidden] = useState(hiddenFor);
  useEffect(() => {
    let canceled = false;
    loadSetupStatus().then(
      (value) => {
        if (canceled) return;
        setItems(value);
        setError("");
      },
      (e: unknown) => {
        if (!canceled) setError(errorText(e));
      },
    );
    return () => {
      canceled = true;
    };
  }, [refresh, retry]);
  const unfinished = items?.filter((item) => item.state !== "ok") ?? [];
  const unfinishedKey = unfinished.map((item) => item.key).join(",");
  if (variant === "banner") {
    // A reminder only; a failed status read is not worth an error here.
    if (!items || unfinished.length === 0 || hidden === unfinishedKey)
      return null;
    return (
      <div className="setup-banner">
        <details>
          <summary>
            Finish setting up ({items.length - unfinished.length} of{" "}
            {items.length} done)
          </summary>
          <Items items={items} />
        </details>
        <button
          type="button"
          className="btn btn-quiet btn-sm"
          onClick={() => {
            saveHidden(unfinishedKey);
            setHidden(unfinishedKey);
          }}
        >
          Hide
        </button>
      </div>
    );
  }
  return (
    <section className="setup-checklist" aria-labelledby={titleId}>
      {level === 2 ? (
        <h2 id={titleId}>{heading}</h2>
      ) : (
        <h4 id={titleId}>{heading}</h4>
      )}
      {error && (
        <LoadError message={error} onRetry={() => setRetry((v) => v + 1)} />
      )}
      {!items && !error && (
        <p className="status-line" role="status">
          Checking setup…
        </p>
      )}
      {items && <Items items={items} onNavigate={onNavigate} />}
    </section>
  );
}
