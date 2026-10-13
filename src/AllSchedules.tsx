import { useEffect, useState } from "react";
import { Link, Navigate, useSearchParams } from "react-router";
import { LoadError } from "./RecordControls";
import { readCommandResult } from "./lib/ipc";
import { displayLocalDate } from "./lib/localDate";
import { errorText } from "./lib/recordFeedback";
import {
  getCurrentSchedule,
  listSchedules,
  type CurrentSchedule,
  type Schedule,
  type ScheduleFilter,
} from "./lib/schedules";
import { PlanTags } from "./schedule/PlanTags";

type IndexFilter = Exclude<ScheduleFilter, "current">;
const filters: readonly { value: IndexFilter; label: string }[] = [
  { value: "non_archived", label: "Draft and final" },
  { value: "draft", label: "Draft" },
  { value: "final", label: "Final" },
  { value: "archived", label: "Archived" },
  { value: "all", label: "All" },
];

/** Every saved plan. Opening one shows it in the Schedule workspace, where
 * its actions live. An old `?id=` link opens that plan there directly. */
export function AllSchedules() {
  const [params] = useSearchParams();
  const legacyId = params.get("id");
  if (legacyId)
    return <Navigate replace to={`/?id=${encodeURIComponent(legacyId)}`} />;
  return <ScheduleIndex initialFilter={params.get("show")} />;
}

function ScheduleIndex({ initialFilter }: { initialFilter: string | null }) {
  const [filter, setFilter] = useState<IndexFilter>(
    () =>
      filters.find((item) => item.value === initialFilter)?.value ??
      "non_archived",
  );
  const [search, setSearch] = useState("");
  const [refresh, setRefresh] = useState(0);
  const [loaded, setLoaded] = useState<{
    filter: IndexFilter;
    rows: readonly Schedule[];
    current: CurrentSchedule;
  } | null>(null);
  const [error, setError] = useState("");
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
    Promise.all([listSchedules(filter), getCurrentSchedule()]).then(
      ([rows, current]) => {
        if (canceled) return;
        try {
          setLoaded({
            filter,
            rows: readCommandResult(rows),
            current: readCommandResult(current),
          });
          setError("");
        } catch (e) {
          setError(errorText(e));
        }
      },
      (e: unknown) => {
        if (!canceled) setError(errorText(e));
      },
    );
    return () => {
      canceled = true;
    };
  }, [filter, refresh]);
  const current = loaded?.current;
  const query = search.trim().toLowerCase();
  const rows = loaded?.rows.filter((s) => s.name.toLowerCase().includes(query));
  // Another filter's rows stay visible (dimmed) until this one loads.
  const stale = loaded !== null && loaded.filter !== filter;
  const currentId = current?.kind === "current" ? current.schedule.id : null;
  return (
    <section className="page" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">All schedules</h1>
          <p className="page-lede">
            Every saved schedule. Open one to edit, check, finalize, or export
            it.
          </p>
        </div>
        <div className="page-actions">
          <Link className="btn" to="/?open=generate">
            Generate…
          </Link>
        </div>
      </header>
      {current?.kind === "stale_setting" && (
        <p className="notice notice-warn">
          <span>
            The saved current schedule is missing or archived.{" "}
            <Link to="/">Review it in Schedule</Link>
          </span>
        </p>
      )}
      <div className="toolbar">
        <label className="grow">
          Find by name
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </label>
        <label>
          Show
          <select
            value={filter}
            onChange={(e) => setFilter(e.target.value as IndexFilter)}
          >
            {filters.map((item) => (
              <option key={item.value} value={item.value}>
                {item.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {error && (
        <LoadError message={error} onRetry={() => setRefresh((v) => v + 1)} />
      )}
      {!loaded && !error && (
        <p className="status-line" role="status">
          Loading schedules…
        </p>
      )}
      {rows && rows.length === 0 && !stale && (
        <p className="muted all-empty">
          {query
            ? "No schedules match this name."
            : filter === "non_archived" || filter === "all"
              ? "No schedules yet. Use Generate… to create a draft."
              : "No schedules match this filter."}
        </p>
      )}
      {rows && rows.length > 0 && (
        <div
          className={stale ? "table-wrap is-refreshing" : "table-wrap"}
          inert={stale}
          aria-busy={stale}
        >
          <table className="data-table all-table">
            <caption className="visually-hidden">
              Saved plans,{" "}
              {filters.find((f) => f.value === loaded?.filter)?.label}
            </caption>
            <thead>
              <tr>
                <th scope="col">Schedule</th>
                <th scope="col">Dates</th>
                <th scope="col">Status</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((schedule) => (
                <tr key={schedule.id}>
                  <td>
                    <Link to={`/?id=${encodeURIComponent(schedule.id)}`}>
                      {schedule.name}
                    </Link>
                  </td>
                  <td className="display all-dates">
                    {displayLocalDate(schedule.start_date)} to{" "}
                    {displayLocalDate(schedule.end_date)}
                  </td>
                  <td>
                    <span className="cluster all-tags">
                      <PlanTags
                        schedule={schedule}
                        current={schedule.id === currentId}
                      />
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
