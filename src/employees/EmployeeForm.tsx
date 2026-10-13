import {
  useEffect,
  useId,
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
  type SubmitEvent,
} from "react";
import { RecordStatusTag } from "../components/RecordStatusTag";
import { minutesToHoursInput, parseHoursInput } from "../lib/duration";
import {
  activeStatusLabels,
  employmentLabels,
  getEmploymentDefaults,
  holidayGroupLabels,
  periodLabels,
  timeOfDayLabels,
  weekendPatternLabels,
  type Employee,
  type EmployeeData,
  type EmploymentType,
} from "../lib/employees";
import { readCommandResult } from "../lib/ipc";
import {
  addLocalDays,
  formatLocalDate,
  localWeekday,
  parseLocalDate,
} from "../lib/localDate";
import { errorText } from "../lib/recordFeedback";
import { notesPlaceholder } from "./format";
import { LabelSelect } from "./LabelSelect";

type Change = <K extends keyof EmployeeData>(
  key: K,
  value: EmployeeData[K],
) => void;

type MinimumRule = "none" | "hours" | "assignments";
const minimumRuleLabels: Record<MinimumRule, string> = {
  none: "No minimum",
  hours: "Paid hours",
  assignments: "Assignment count",
};
const maximumRuleLabels: Record<"none" | "hours", string> = {
  none: "No maximum",
  hours: "Paid hours",
};
const preferenceLabels = { none: "No preference", ...timeOfDayLabels };
const schedulableFor = [
  ["can_work_weekends", "Weekends"],
  ["can_work_holidays", "Holidays"],
  ["can_work_night_shift", "Night shifts"],
  ["can_work_ortho_or", "ORTHO & OR"],
] as const;
/** 40 h, the starting amount when a paid-hours rule is first chosen. */
const defaultPaidMinutes = 2400;
const prnExplanation =
  "PRN employees have no default minimum. On ordinary weekdays they're only added to shifts their entered availability fully covers. Weekend and holiday rotation works as usual.";

const minimumRuleOf = (data: EmployeeData): MinimumRule =>
  data.minimum_period === "none"
    ? "none"
    : data.minimum_assignment_count !== null
      ? "assignments"
      : "hours";

export function EmployeeForm({
  initial,
  employee,
  busy,
  headingRef,
  onSave,
  onClose,
  onArchive,
  onReset,
}: {
  initial: EmployeeData;
  employee: Employee | null;
  busy: boolean;
  headingRef: RefObject<HTMLHeadingElement | null>;
  onSave: (data: EmployeeData) => void;
  onClose: () => void;
  onArchive: () => void;
  onReset: () => void;
}) {
  const [source, setSource] = useState(initial);
  const [data, setData] = useState(initial);
  const [minimumRule, setMinimumRule] = useState(() => minimumRuleOf(initial));
  // Type defaults fill in a new employee's rules until the manager edits them.
  const [customized, setCustomized] = useState({
    minimum: false,
    availability: false,
  });
  const [typeBusy, setTypeBusy] = useState(false);
  const [typeError, setTypeError] = useState("");
  // A saved or reloaded record replaces the form's values in place. The form
  // stays mounted, so the button that was pressed keeps focus.
  if (source !== initial) {
    setSource(initial);
    setData(initial);
    setMinimumRule(minimumRuleOf(initial));
    setCustomized({ minimum: false, availability: false });
    setTypeError("");
  }
  const archived = employee !== null && employee.archived_at !== null;
  const locked = busy || typeBusy || archived;
  const change: Change = (key, value) =>
    setData((d) => ({ ...d, [key]: value }));

  async function changeType(type: EmploymentType) {
    if (employee) {
      change("employment_type", type);
      return;
    }
    setTypeBusy(true);
    setTypeError("");
    try {
      const defaults = readCommandResult(await getEmploymentDefaults(type));
      if (!customized.minimum)
        setMinimumRule(defaults.minimum_period === "none" ? "none" : "hours");
      setData((d) => ({
        ...d,
        employment_type: type,
        ...(!customized.minimum
          ? {
              minimum_paid_minutes: defaults.minimum_paid_minutes,
              minimum_assignment_count: defaults.minimum_assignment_count,
              minimum_period: defaults.minimum_period,
            }
          : {}),
        requires_explicit_availability: customized.availability
          ? d.requires_explicit_availability
          : defaults.requires_explicit_availability,
      }));
    } catch (error) {
      setTypeError(errorText(error));
    } finally {
      setTypeBusy(false);
    }
  }
  function chooseMinimumRule(rule: MinimumRule) {
    setCustomized((c) => ({ ...c, minimum: true }));
    setMinimumRule(rule);
    setData((d) => ({
      ...d,
      minimum_period:
        rule === "none"
          ? "none"
          : d.minimum_period === "none"
            ? "week"
            : d.minimum_period,
      minimum_paid_minutes:
        rule === "hours"
          ? (d.minimum_paid_minutes ?? defaultPaidMinutes)
          : null,
      minimum_assignment_count:
        rule === "assignments" ? (d.minimum_assignment_count ?? 1) : null,
    }));
  }
  function submit(e: SubmitEvent<HTMLFormElement>) {
    e.preventDefault();
    onSave(data);
  }

  return (
    <form onSubmit={submit}>
      <div className="detail-header">
        <div>
          <h2 ref={headingRef} tabIndex={-1}>
            {employee ? employee.display_name : "New employee"}
          </h2>
        </div>
        <div className="cluster">
          {employee && <RecordStatusTag record={employee} />}
          <button
            type="button"
            className="btn btn-quiet"
            disabled={busy || typeBusy}
            onClick={onClose}
          >
            Close
          </button>
        </div>
      </div>
      {archived && (
        <div className="notice notice-info">
          <p>Archived employees can&rsquo;t be edited.</p>
        </div>
      )}
      {typeError && (
        <div role="alert" className="notice notice-error">
          <p>{typeError}</p>
        </div>
      )}
      <DetailsSection
        data={data}
        change={change}
        onType={(type) => {
          void changeType(type);
        }}
        saved={employee !== null}
        disabled={locked}
      />
      <HoursSection
        data={data}
        setData={setData}
        minimumRule={minimumRule}
        onMinimumRule={chooseMinimumRule}
        onMinimumEdit={() => setCustomized((c) => ({ ...c, minimum: true }))}
        onAvailabilityEdit={() =>
          setCustomized((c) => ({ ...c, availability: true }))
        }
        disabled={locked}
      />
      <fieldset className="form-section" disabled={locked}>
        <legend>Can be scheduled for</legend>
        <div className="checks">
          {schedulableFor.map(([key, label]) => (
            <label key={key}>
              <input
                type="checkbox"
                checked={data[key]}
                onChange={(e) => change(key, e.target.checked)}
              />
              {label}
            </label>
          ))}
        </div>
      </fieldset>
      <fieldset className="form-section" disabled={locked}>
        <legend>Preferences</legend>
        <div className="fields">
          <LabelSelect
            label="Preferred time of day"
            labels={preferenceLabels}
            value={data.preferred_time_of_day ?? "none"}
            onChange={(time) =>
              change("preferred_time_of_day", time === "none" ? null : time)
            }
          />
        </div>
      </fieldset>
      <RotationSection data={data} change={change} disabled={locked} />
      <fieldset className="form-section" disabled={locked}>
        <legend>Notes</legend>
        <textarea
          aria-label="Notes"
          rows={3}
          maxLength={8000}
          placeholder={notesPlaceholder}
          value={data.notes ?? ""}
          onChange={(e) => change("notes", e.target.value || null)}
        />
      </fieldset>
      {!archived && (
        <div className="form-actions">
          <button
            className="btn btn-primary"
            type="submit"
            disabled={busy || typeBusy}
          >
            {busy ? "Saving…" : "Save employee"}
          </button>
          {employee && (
            <>
              <button
                className="btn"
                type="button"
                disabled={busy || typeBusy}
                onClick={onReset}
              >
                Reset to defaults…
              </button>
              <span className="spacer" />
              <button
                className="btn btn-danger"
                type="button"
                disabled={busy || typeBusy}
                onClick={onArchive}
              >
                Archive employee
              </button>
            </>
          )}
        </div>
      )}
    </form>
  );
}

function DetailsSection({
  data,
  change,
  onType,
  saved,
  disabled,
}: {
  data: EmployeeData;
  change: Change;
  onType: (type: EmploymentType) => void;
  saved: boolean;
  disabled: boolean;
}) {
  const helpId = useId();
  const prn = data.employment_type === "prn";
  return (
    <fieldset className="form-section" disabled={disabled}>
      <legend>Details</legend>
      <div className="fields">
        {(
          [
            ["first_name", "First name"],
            ["last_name", "Last name"],
            ["display_name", "Display name"],
          ] as const
        ).map(([key, label]) => (
          <label key={key}>
            {label}
            <input
              required
              maxLength={240}
              value={data[key]}
              onChange={(e) => change(key, e.target.value)}
            />
          </label>
        ))}
        <LabelSelect
          label="Employment type"
          labels={employmentLabels}
          value={data.employment_type}
          onChange={onType}
          describedBy={saved || prn ? helpId : undefined}
        />
        <LabelSelect
          label="Status"
          labels={activeStatusLabels}
          value={data.active_status}
          onChange={(status) => change("active_status", status)}
        />
      </div>
      {(saved || prn) && (
        <div id={helpId} className="emp-help">
          {saved && (
            <p className="field-help">
              Changing the type keeps this employee&rsquo;s hour rules. Save,
              then use Reset to defaults to apply the new type&rsquo;s rules.
            </p>
          )}
          {prn && <p className="field-help">{prnExplanation}</p>}
        </div>
      )}
    </fieldset>
  );
}

function HoursSection({
  data,
  setData,
  minimumRule,
  onMinimumRule,
  onMinimumEdit,
  onAvailabilityEdit,
  disabled,
}: {
  data: EmployeeData;
  setData: Dispatch<SetStateAction<EmployeeData>>;
  minimumRule: MinimumRule;
  onMinimumRule: (rule: MinimumRule) => void;
  onMinimumEdit: () => void;
  onAvailabilityEdit: () => void;
  disabled: boolean;
}) {
  const change: Change = (key, value) =>
    setData((d) => ({ ...d, [key]: value }));
  return (
    <fieldset className="form-section" disabled={disabled}>
      <legend>Hours and limits</legend>
      <div className="fields">
        <LabelSelect
          label="Minimum"
          labels={minimumRuleLabels}
          value={minimumRule}
          onChange={onMinimumRule}
        />
        {minimumRule === "hours" && (
          <HoursInput
            label="Minimum paid hours"
            minutes={data.minimum_paid_minutes}
            onChange={(minutes) => {
              onMinimumEdit();
              change("minimum_paid_minutes", minutes);
            }}
          />
        )}
        {minimumRule === "assignments" && (
          <label>
            Minimum assignments
            <input
              type="number"
              required
              min={1}
              max={1_000_000}
              step={1}
              value={data.minimum_assignment_count ?? ""}
              onChange={(e) => {
                onMinimumEdit();
                change(
                  "minimum_assignment_count",
                  e.target.value === "" ? null : Number(e.target.value),
                );
              }}
            />
          </label>
        )}
        {minimumRule !== "none" && (
          <LabelSelect
            label="Minimum period"
            labels={periodLabels}
            value={
              data.minimum_period === "none" ? "week" : data.minimum_period
            }
            onChange={(period) => {
              onMinimumEdit();
              change("minimum_period", period);
            }}
          />
        )}
        <LabelSelect
          label="Maximum"
          labels={maximumRuleLabels}
          value={data.maximum_period === "none" ? "none" : "hours"}
          onChange={(rule) =>
            setData((d) => ({
              ...d,
              maximum_period: rule === "none" ? "none" : "week",
              maximum_paid_minutes:
                rule === "none"
                  ? null
                  : (d.maximum_paid_minutes ?? defaultPaidMinutes),
            }))
          }
        />
        {data.maximum_period !== "none" && (
          <>
            <HoursInput
              label="Maximum paid hours"
              minutes={data.maximum_paid_minutes}
              onChange={(minutes) => change("maximum_paid_minutes", minutes)}
            />
            <LabelSelect
              label="Maximum period"
              labels={periodLabels}
              value={data.maximum_period}
              onChange={(period) => change("maximum_period", period)}
            />
          </>
        )}
        <label className="span-all">
          <input
            type="checkbox"
            checked={data.requires_explicit_availability}
            onChange={(e) => {
              onAvailabilityEdit();
              change("requires_explicit_availability", e.target.checked);
            }}
          />
          Only schedule when entered availability covers the whole shift
        </label>
      </div>
      {(data.minimum_period === "two_week" ||
        data.maximum_period === "two_week") && (
        <p className="field-help">
          Two-week periods count from the pay-period start in Settings.
        </p>
      )}
    </fieldset>
  );
}

function RotationSection({
  data,
  change,
  disabled,
}: {
  data: EmployeeData;
  change: Change;
  disabled: boolean;
}) {
  const anchorHelpId = useId();
  return (
    <fieldset className="form-section" disabled={disabled}>
      <legend>Weekend and holiday rotation</legend>
      <div className="fields">
        <LabelSelect
          label="Holiday group"
          labels={holidayGroupLabels}
          value={data.holiday_group}
          onChange={(group) => change("holiday_group", group)}
        />
        <LabelSelect
          label="Weekend rotation"
          labels={weekendPatternLabels}
          value={data.weekend_rotation_pattern}
          onChange={(pattern) => {
            change("weekend_rotation_pattern", pattern);
            if (pattern === "none")
              change("weekend_rotation_anchor_date", null);
          }}
        />
        {data.weekend_rotation_pattern === "every_other_weekend" && (
          <div className="emp-field">
            <label>
              A weekend they work
              <input
                type="date"
                min="0001-01-01"
                max="9999-12-31"
                required
                aria-describedby={anchorHelpId}
                value={data.weekend_rotation_anchor_date ?? ""}
                onChange={(e) =>
                  change("weekend_rotation_anchor_date", e.target.value || null)
                }
              />
            </label>
            <p id={anchorHelpId} className="field-help">
              {anchorGuidance(data.weekend_rotation_anchor_date)}
            </p>
          </div>
        )}
      </div>
    </fieldset>
  );
}

function anchorGuidance(anchor: string | null) {
  const parsed = anchor ? parseLocalDate(anchor) : null;
  if (!parsed?.ok) return "Pick the Saturday or Sunday of a weekend they work.";
  const weekday = localWeekday(parsed.value);
  if (weekday === 0) {
    const saturday = addLocalDays(parsed.value, -1);
    return saturday.ok
      ? `Saved as Saturday ${formatLocalDate(saturday.value)}, the start of that weekend.`
      : saturday.error.message;
  }
  if (weekday === 6)
    return `They work the weekend starting Saturday ${formatLocalDate(parsed.value)}, then every other weekend.`;
  return "This is a weekday. Pick a Saturday or Sunday.";
}

/**
 * Paid time entered in hours ("40", "7.5") and saved as whole minutes, so
 * 40 h is exactly 2400 minutes. The typed text is kept as typed; it is only
 * replaced when the saved value changes underneath it.
 */
function HoursInput({
  label,
  minutes,
  onChange,
}: {
  label: string;
  minutes: number | null;
  onChange: (minutes: number | null) => void;
}) {
  const [text, setText] = useState(() => minutesToHoursInput(minutes));
  const [shown, setShown] = useState(minutes);
  if (shown !== minutes) {
    setShown(minutes);
    if (parseHoursInput(text) !== minutes)
      setText(minutesToHoursInput(minutes));
  }
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const parsed = parseHoursInput(text);
    input.current?.setCustomValidity(
      text !== "" && parsed === null
        ? "Enter hours as a number, like 40 or 7.5."
        : parsed === 0
          ? "Enter more than 0 hours."
          : "",
    );
  }, [text]);
  return (
    <label>
      {label}
      <input
        ref={input}
        type="number"
        inputMode="decimal"
        min={0}
        max={9999}
        step="any"
        required
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          onChange(parseHoursInput(e.target.value));
        }}
      />
    </label>
  );
}
