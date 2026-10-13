/** The saved current schedule points at a plan that is missing or archived.
 * Showing another plan never repairs it; only an explicit choice does. */
export function StaleCurrentNotice({
  busy,
  onClear,
}: {
  busy: boolean;
  onClear: () => void;
}) {
  return (
    <div className="notice notice-warn ws-stale" role="alert">
      <p>
        The saved current schedule is missing or archived. Clear the saved
        selection, or open a schedule and choose More › Set as current.
      </p>
      <button
        type="button"
        className="btn btn-sm"
        disabled={busy}
        onClick={onClear}
      >
        Clear saved selection
      </button>
    </div>
  );
}
