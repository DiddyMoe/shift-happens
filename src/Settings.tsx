import { useRef, useState, type SubmitEvent } from "react";
import { useFocusAfterRender } from "./hooks/useFocusAfterRender";
import { useCommandQuery } from "./hooks/useCommandQuery";
import { useOtherScreenChanges } from "./hooks/useOtherScreenChanges";
import { useRecordMutation } from "./hooks/useRecordMutation";
import {
  formatHours,
  minutesToHoursInput,
  parseHoursInput,
} from "./lib/duration";
import {
  getSettings,
  isManagerSettingKey,
  updateManagerSetting,
  type SettingValue,
  type SettingView,
  type ThemePreference,
  type Weekday,
} from "./lib/ipc";
import { Confirmation, LoadError, RecordFeedback } from "./RecordControls";

const weekdays: readonly Weekday[] = [
  "sunday",
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
];
const themes: readonly ThemePreference[] = ["dark", "light", "system"];
const capitalized = (text: string) =>
  text.charAt(0).toUpperCase() + text.slice(1);
const labels: Record<SettingView["key"], string> = {
  workweek_start_weekday: "Workweek starts on",
  two_week_period_anchor_date: "Two-week pay period start",
  max_consecutive_workdays: "Consecutive workday limit",
  quick_turnaround_rule_enabled: "Quick turnaround rule",
  minimum_rest_minutes_between_shifts: "Minimum rest between shifts (hours)",
  theme_preference: "Theme preference",
  holiday_rotation_start_year: "Holiday rotation start year",
  weekly_overtime_threshold_minutes: "Weekly overtime threshold",
  local_workplace_time_policy: "Workplace time policy",
  local_date_format: "Date format",
  local_time_format: "Time format",
  local_datetime_format: "Datetime format",
  current_schedule_id: "Current schedule",
};
/** The longest rest the backend accepts: 48 h. */
const maxRestMinutes = 2880;
// Editable settings are grouped by topic. Every other setting (except the
// current schedule, which is chosen on Schedules) is listed as a read-only fact.
const settingGroups: readonly {
  title: string;
  keys: readonly SettingView["key"][];
}[] = [
  {
    title: "Workweek and payroll",
    keys: ["workweek_start_weekday", "two_week_period_anchor_date"],
  },
  {
    title: "Rest rules",
    keys: [
      "max_consecutive_workdays",
      "quick_turnaround_rule_enabled",
      "minimum_rest_minutes_between_shifts",
    ],
  },
  {
    title: "Appearance",
    keys: ["theme_preference"],
  },
];
const groupedKeys = new Set(settingGroups.flatMap((group) => group.keys));
function isReadOnlyFact(setting: SettingView): boolean {
  return setting.key !== "current_schedule_id" && !groupedKeys.has(setting.key);
}

function displayValue(setting: SettingView): string {
  const value = setting.value;
  if (value === null) return "Not set";
  if (
    value.kind === "integer" &&
    setting.key === "weekly_overtime_threshold_minutes"
  )
    return formatHours(value.value);
  if (
    setting.key === "local_workplace_time_policy" &&
    value.value === "wall_clock"
  )
    return "Wall clock (no daylight-saving change)";
  return String(value.value);
}

/** Durations are edited in hours; every other setting edits its raw value. */
const inHours = (setting: SettingView) =>
  setting.key === "minimum_rest_minutes_between_shifts";

/** The text an editor starts from for a saved value. */
function draftText(setting: SettingView): string {
  const value = setting.value;
  if (value === null) return "";
  if (inHours(setting) && value.kind === "integer")
    return minutesToHoursInput(value.value);
  return String(value.value);
}

type Parsed =
  { ok: true; value: SettingValue | null } | { ok: false; message: string };
function parseDraft(setting: SettingView, draft: string): Parsed {
  switch (setting.value_type) {
    case "integer": {
      if (inHours(setting)) {
        const minutes = parseHoursInput(draft);
        if (minutes === null)
          return { ok: false, message: "Enter hours, like 10 or 10.5." };
        if (minutes > maxRestMinutes)
          return { ok: false, message: "Enter 48 hours or less." };
        return { ok: true, value: { kind: "integer", value: minutes } };
      }
      const integer = Number(draft);
      if (!/^-?[0-9]+$/.test(draft) || !Number.isSafeInteger(integer))
        return { ok: false, message: "Enter a whole number." };
      return { ok: true, value: { kind: "integer", value: integer } };
    }
    case "boolean":
      return { ok: true, value: { kind: "boolean", value: draft === "true" } };
    case "date":
      return {
        ok: true,
        value: draft === "" ? null : { kind: "date", value: draft },
      };
    case "weekday": {
      const weekday = weekdays.find((v) => v === draft);
      return weekday
        ? { ok: true, value: { kind: "weekday", value: weekday } }
        : { ok: false, message: "Select a weekday." };
    }
    case "string":
      return { ok: true, value: { kind: "string", value: draft } };
    case "schedule_id":
      return { ok: false, message: "This setting is chosen on Schedules." };
  }
}
const sameValue = (a: SettingValue | null, b: SettingValue | null) =>
  a === null || b === null ? a === b : a.value === b.value;

function SettingForm({
  setting,
  reload,
}: {
  setting: SettingView;
  reload: () => void;
}) {
  const [current, setCurrent] = useState(setting);
  const [draft, setDraft] = useState(() => draftText(setting));
  const [anchorChange, setAnchorChange] = useState<{
    value: SettingValue | null;
  } | null>(null);
  const { busy, feedback, setFeedback, run } = useRecordMutation();
  const form = useRef<HTMLFormElement>(null);
  const focusAfterRender = useFocusAfterRender();
  const editable = current.editable && isManagerSettingKey(current.key);
  const id = `setting-${current.key}`;
  const parsed = parseDraft(current, draft);
  // Save and Cancel only appear once this setting has an unsaved change.
  const changed = !parsed.ok || !sameValue(parsed.value, current.value);
  // Save and Cancel hide once the draft matches the saved value, so focus
  // goes back to the setting's own control.
  const focusControl = () =>
    focusAfterRender(() =>
      form.current?.querySelector<HTMLElement>(
        ".set-item-control input, .set-item-control select, .set-item-control [aria-pressed='true']",
      ),
    );
  function edit(text: string) {
    setDraft(text);
    setFeedback(null);
  }

  function save(value: SettingValue | null) {
    const key = current.key;
    if (!isManagerSettingKey(key)) return;
    void run(() => updateManagerSetting(key, value, current.revision), {
      success: "Saved.",
      onSuccess: (saved) => {
        setCurrent(saved);
        setDraft(draftText(saved));
        setAnchorChange(null);
        focusControl();
      },
      onFailure: () => setAnchorChange(null),
    });
  }
  function submit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editable || busy) return;
    if (!parsed.ok) {
      setFeedback({ message: parsed.message, stale: false, failed: true });
      return;
    }
    // Moving an existing pay-period start shifts every two-week period, so it
    // is confirmed; setting it for the first time is not.
    if (
      current.key === "two_week_period_anchor_date" &&
      current.value !== null &&
      !sameValue(parsed.value, current.value)
    ) {
      setAnchorChange({ value: parsed.value });
      return;
    }
    save(parsed.value);
  }

  return (
    <form ref={form} className="set-item" onSubmit={submit} aria-busy={busy}>
      <div className="set-item-head">
        {editable && current.key === "theme_preference" ? (
          // The theme choice is a button group, which a <label> cannot name.
          <span id={`${id}-label`} className="set-label">
            {labels[current.key]}
          </span>
        ) : (
          <label htmlFor={editable ? id : undefined}>
            {labels[current.key]}
          </label>
        )}
        {!editable && <span className="tag tag-muted">Read-only</span>}
      </div>
      <p id={`${id}-description`} className="field-help">
        {current.description}
      </p>
      {!editable ? (
        <p className="set-value">{displayValue(current)}</p>
      ) : (
        <div className="set-item-control">
          {current.value_type === "boolean" ? (
            <label>
              <input
                id={id}
                type="checkbox"
                checked={draft === "true"}
                disabled={busy}
                aria-describedby={`${id}-description`}
                onChange={(e) => edit(String(e.target.checked))}
              />
              Enabled
            </label>
          ) : current.value_type === "weekday" ? (
            <select
              id={id}
              value={draft}
              disabled={busy}
              aria-describedby={`${id}-description`}
              onChange={(e) => edit(e.target.value)}
            >
              {weekdays.map((v) => (
                <option key={v} value={v}>
                  {capitalized(v)}
                </option>
              ))}
            </select>
          ) : current.key === "theme_preference" ? (
            <div
              id={id}
              className="segmented"
              role="group"
              aria-labelledby={`${id}-label`}
              aria-describedby={`${id}-description`}
            >
              {themes.map((v) => (
                <button
                  key={v}
                  type="button"
                  aria-pressed={draft === v}
                  disabled={busy}
                  onClick={() => edit(v)}
                >
                  {capitalized(v)}
                </button>
              ))}
            </div>
          ) : current.value_type === "date" ? (
            <input
              id={id}
              type="date"
              value={draft}
              disabled={busy}
              aria-describedby={`${id}-description`}
              min="0001-01-01"
              max="9999-12-31"
              onChange={(e) => edit(e.target.value)}
            />
          ) : inHours(current) ? (
            <input
              id={id}
              type="number"
              inputMode="decimal"
              value={draft}
              disabled={busy}
              aria-describedby={`${id}-description`}
              min={0}
              max={maxRestMinutes / 60}
              step="any"
              onChange={(e) => edit(e.target.value)}
            />
          ) : (
            <input
              id={id}
              type="number"
              value={draft}
              disabled={busy}
              aria-describedby={`${id}-description`}
              min={1}
              max={31}
              step={1}
              onChange={(e) => edit(e.target.value)}
            />
          )}
          <div className="cluster" hidden={!changed && !busy}>
            <button
              type="submit"
              className="btn btn-primary btn-sm"
              disabled={busy || !changed || feedback?.stale === true}
            >
              {busy ? "Saving…" : "Save"}
            </button>
            <button
              type="button"
              className="btn btn-quiet btn-sm"
              disabled={busy || !changed}
              onClick={() => {
                setDraft(draftText(current));
                if (!feedback?.stale) setFeedback(null);
                focusControl();
              }}
            >
              Cancel changes
            </button>
          </div>
        </div>
      )}
      {current.key === "two_week_period_anchor_date" &&
        current.value === null && (
          <p className="notice notice-warn">
            Set the first day of any two-week pay period. Until it&rsquo;s set,
            schedules for employees with two-week minimums or maximums
            can&rsquo;t be generated. Your payroll contact can confirm the date.
          </p>
        )}
      <RecordFeedback
        feedback={feedback}
        busy={busy}
        onReload={reload}
        reloadLabel="Reload saved settings"
      />
      {anchorChange && (
        <Confirmation
          title="Change the pay-period start?"
          confirmLabel="Change pay-period start"
          busyLabel="Saving…"
          busy={busy}
          onConfirm={() => save(anchorChange.value)}
          onCancel={() => setAnchorChange(null)}
        >
          <p>
            Changing the pay-period start changes overtime and two-week rule
            calculations for every employee. Only change it to match a permanent
            change in your hospital&rsquo;s pay period.
          </p>
        </Confirmation>
      )}
    </form>
  );
}

export function Settings() {
  const settings = useCommandQuery("settings", getSettings);
  // A setting saved elsewhere (for example the theme) is shown here too.
  useOtherScreenChanges(settings.reload);
  const data = settings.data;
  return (
    <section className="page page-narrow" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">Settings</h1>
          <p className="page-lede">
            Rules used to build and check schedules, and the app theme. Set
            these to match your hospital policy and state law.
          </p>
        </div>
      </header>
      {settings.error ? (
        <LoadError message={settings.error} onRetry={settings.reload} />
      ) : data === null ? (
        <p className="status-line" role="status">
          Loading saved settings…
        </p>
      ) : (
        <>
          {settingGroups.map((group) => {
            const rows = group.keys.flatMap((key) =>
              data.filter((setting) => setting.key === key),
            );
            return (
              rows.length > 0 && (
                <section key={group.title} className="set-group">
                  <h2>{group.title}</h2>
                  {rows.map((setting) => (
                    // A newer saved revision starts the form fresh.
                    <SettingForm
                      key={`${setting.key}:${setting.revision}`}
                      setting={setting}
                      reload={settings.reload}
                    />
                  ))}
                </section>
              )
            );
          })}
          {data.some(isReadOnlyFact) && (
            <section className="set-group">
              <h2>Read-only settings</h2>
              <dl className="facts set-readonly">
                {data.filter(isReadOnlyFact).map((setting) => (
                  <div key={setting.key}>
                    <dt>{labels[setting.key]}</dt>
                    <dd>
                      {displayValue(setting)}
                      <span className="field-help">{setting.description}</span>
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          )}
        </>
      )}
    </section>
  );
}
