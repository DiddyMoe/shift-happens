import { useEffect, useId, useState } from "react";
import {
  previewHolidayDate,
  type DateKind,
  type HolidayData,
} from "../lib/holidays";
import { readCommandResult, type Weekday } from "../lib/ipc";
import { dayLabels } from "../lib/positionsCoverage";
import { errorText } from "../lib/recordFeedback";

const dateRules: readonly { value: DateKind; label: string }[] = [
  { value: "fixed", label: "Same date every year" },
  { value: "nth_weekday", label: "Weekday of the month" },
  { value: "last_weekday", label: "Last weekday of the month" },
  { value: "exact_date", label: "One-time date" },
];
const weekdays: readonly Weekday[] = [
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
  "sunday",
];
const months = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];
const occurrences = ["First", "Second", "Third", "Fourth"];

/** The holiday's date rule, with the date it falls on in the chosen year. */
export function HolidayDateFields({
  data,
  year,
  onChange,
}: {
  data: HolidayData;
  year: number;
  onChange: (data: HolidayData) => void;
}) {
  const helpId = useId();
  const [preview, setPreview] = useState<{
    input: HolidayData;
    year: number;
    date: string | null;
    error: string;
  } | null>(null);
  // The date check validates the whole holiday, so a blank new form waits
  // for a name instead of opening with an error.
  const unnamed = data.name.trim() === "";
  useEffect(() => {
    if (unnamed) return;
    let canceled = false;
    const timer = window.setTimeout(() => {
      previewHolidayDate(data, year)
        .then(readCommandResult)
        .then(
          (value) => {
            if (!canceled)
              setPreview({
                input: data,
                year,
                date: value.resolved_date,
                error: "",
              });
          },
          (error: unknown) => {
            if (!canceled)
              setPreview({
                input: data,
                year,
                date: null,
                error: errorText(error),
              });
          },
        );
    }, 250);
    return () => {
      canceled = true;
      window.clearTimeout(timer);
    };
  }, [data, year, unnamed]);
  function changeKind(kind: DateKind) {
    onChange({
      ...data,
      date_kind: kind,
      month: kind === "exact_date" ? null : 1,
      day_of_month: kind === "fixed" ? 1 : null,
      weekday:
        kind === "nth_weekday" || kind === "last_weekday" ? "monday" : null,
      nth: kind === "nth_weekday" ? 1 : null,
      exact_date:
        kind === "exact_date" ? `${String(year).padStart(4, "0")}-01-01` : null,
    });
  }
  const current =
    !unnamed && preview?.input === data && preview.year === year
      ? preview
      : null;
  return (
    <>
      <div className="fields">
        <label>
          Date rule
          <select
            value={data.date_kind}
            aria-describedby={helpId}
            onChange={(e) => {
              const rule = dateRules.find((r) => r.value === e.target.value);
              if (rule) changeKind(rule.value);
            }}
          >
            {dateRules.map((r) => (
              <option key={r.value} value={r.value}>
                {r.label}
              </option>
            ))}
          </select>
        </label>
        {data.date_kind !== "exact_date" && (
          <label>
            Month
            <select
              value={data.month ?? ""}
              onChange={(e) =>
                onChange({ ...data, month: Number(e.target.value) })
              }
            >
              {months.map((m, i) => (
                <option key={m} value={i + 1}>
                  {m}
                </option>
              ))}
            </select>
          </label>
        )}
        {data.date_kind === "fixed" && (
          <label>
            Day of month
            <input
              required
              type="number"
              min="1"
              max="31"
              step="1"
              value={data.day_of_month ?? ""}
              onChange={(e) =>
                onChange({
                  ...data,
                  day_of_month:
                    e.target.value === "" ? null : Number(e.target.value),
                })
              }
            />
          </label>
        )}
        {(data.date_kind === "nth_weekday" ||
          data.date_kind === "last_weekday") && (
          <label>
            Weekday
            <select
              value={data.weekday ?? ""}
              onChange={(e) => {
                const weekday = weekdays.find((w) => w === e.target.value);
                if (weekday) onChange({ ...data, weekday });
              }}
            >
              {weekdays.map((w) => (
                <option key={w} value={w}>
                  {dayLabels[w]}
                </option>
              ))}
            </select>
          </label>
        )}
        {data.date_kind === "nth_weekday" && (
          <label>
            Occurrence
            <select
              value={data.nth ?? ""}
              onChange={(e) =>
                onChange({ ...data, nth: Number(e.target.value) })
              }
            >
              {occurrences.map((label, i) => (
                <option key={label} value={i + 1}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}
        {data.date_kind === "exact_date" && (
          <label>
            Date
            <input
              required
              type="date"
              min="0001-01-01"
              max="9999-12-31"
              value={data.exact_date ?? ""}
              onChange={(e) =>
                onChange({ ...data, exact_date: e.target.value || null })
              }
            />
          </label>
        )}
      </div>
      <p className="field-help" id={helpId}>
        For February 29, use a one-time date. Observed or substitute days
        aren&rsquo;t added.
      </p>
      {current?.error ? (
        <div className="notice notice-error" role="alert">
          <p>{current.error}</p>
        </div>
      ) : (
        <div className="notice notice-info" role="status">
          <p>
            {unnamed ? (
              <>Add a name to see the date in {year}.</>
            ) : !current ? (
              <>Finding the date in {year}…</>
            ) : current.date ? (
              <>
                In {year}: <strong className="display">{current.date}</strong>
              </>
            ) : (
              <>No date in {year}.</>
            )}
          </p>
        </div>
      )}
    </>
  );
}
