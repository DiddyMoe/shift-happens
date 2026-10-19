import { useState } from "react";
import { Link } from "react-router";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { useCommandQuery } from "../hooks/useCommandQuery";
import { useOtherScreenChanges } from "../hooks/useOtherScreenChanges";
import type { EntryFilters } from "../lib/availabilityPto";
import { employmentLabels, listEmployees } from "../lib/employees";
import { LoadError, RecordFilterSelect } from "../RecordControls";
import { EntryPanel } from "./EntryPanel";

export function AvailabilityPto() {
  const employees = useCommandQuery("employees", () =>
    listEmployees("all", null, ""),
  );
  // An employee saved elsewhere shows here with its current name and status.
  useOtherScreenChanges(employees.reload);
  const [filters, setFilters] = useState<EntryFilters>({
    employee_id: null,
    filter: "current",
    start_date: null,
    end_date: null,
  });
  const list = employees.data;
  const chosen =
    filters.employee_id === null
      ? null
      : list?.find((e) => e.id === filters.employee_id);
  const allSelected = filters.employee_id === null;
  return (
    <section className="page" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">Availability and PTO</h1>
          <p className="page-lede">
            When employees can and can&rsquo;t work, and their time off.
          </p>
        </div>
        <div className="page-actions">
          <Link className="btn" to="/employees">
            Manage employees
          </Link>
        </div>
      </header>
      {/* A failed refresh keeps the last list, and any open entry. */}
      {employees.error && (
        <LoadError message={employees.error} onRetry={employees.reload} />
      )}
      {list === null ? (
        !employees.error && (
          <p className="status-line" role="status">
            Loading employees…
          </p>
        )
      ) : (
        <>
          {list.length === 0 && (
            <div className="notice notice-warn">
              <p>Add an employee before entering availability or PTO.</p>
            </div>
          )}
          <div className="toolbar" role="group" aria-label="Filter both lists">
            <label>
              From
              <input
                type="date"
                min="0001-01-01"
                max="9999-12-31"
                value={filters.start_date ?? ""}
                onChange={(e) =>
                  setFilters((f) => ({
                    ...f,
                    start_date: e.target.value || null,
                  }))
                }
              />
            </label>
            <label>
              To
              <input
                type="date"
                min="0001-01-01"
                max="9999-12-31"
                value={filters.end_date ?? ""}
                onChange={(e) =>
                  setFilters((f) => ({
                    ...f,
                    end_date: e.target.value || null,
                  }))
                }
              />
            </label>
            <RecordFilterSelect
              label="Entry status"
              value={filters.filter}
              onChange={(filter) => setFilters((f) => ({ ...f, filter }))}
            />
          </div>
          <div className="split">
            <div className="split-list">
              <p className="split-list-header">
                <span>
                  <span className="display">{list.length}</span>{" "}
                  {list.length === 1 ? "employee" : "employees"}
                </span>
              </p>
              <ul className="record-list" aria-label="Show entries for">
                <li>
                  <button
                    className={`record-item${allSelected ? " is-selected" : ""}`}
                    type="button"
                    aria-current={allSelected ? "true" : undefined}
                    onClick={() =>
                      setFilters((f) => ({ ...f, employee_id: null }))
                    }
                  >
                    <span className="record-title">All employees</span>
                    <span className="record-meta">
                      Entries for every employee
                    </span>
                  </button>
                </li>
                {list.map((e) => {
                  const selected = filters.employee_id === e.id;
                  return (
                    <li key={e.id}>
                      <button
                        className={`record-item${selected ? " is-selected" : ""}`}
                        type="button"
                        aria-current={selected ? "true" : undefined}
                        onClick={() =>
                          setFilters((f) => ({ ...f, employee_id: e.id }))
                        }
                      >
                        <span className="record-title">
                          {e.display_name}
                          <RecordStatusTag record={e} />
                        </span>
                        <span className="record-meta">
                          {employmentLabels[e.employment_type]}
                          {e.requires_explicit_availability &&
                            ", needs availability entered"}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
            <div className="split-detail">
              <div className="detail-header">
                <div>
                  <h2>
                    {chosen === null
                      ? "All employees"
                      : (chosen?.display_name ?? "Employee unavailable")}
                  </h2>
                </div>
                {chosen && <RecordStatusTag record={chosen} />}
              </div>
              <EntryPanel
                kind="availability"
                employees={list}
                filters={filters}
              />
              <EntryPanel kind="pto" employees={list} filters={filters} />
            </div>
          </div>
        </>
      )}
    </section>
  );
}
