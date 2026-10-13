// Rule context attached to findings, as people read it: record identifiers
// are left out and durations, stored as whole minutes, read in hours.
import { formatHours } from "./duration";

type Context = Readonly<Record<string, unknown>>;

/** Keys that hold paid minutes only when their finding measures paid time. */
const METRIC_AMOUNTS = new Set([
  "total",
  "threshold",
  "excess",
  "shortfall",
  "required",
  "after",
  "before",
]);

const isIdentifier = (key: string) =>
  key.endsWith("_id") || key.endsWith("_ids");
const isMinutes = (key: string, siblings: Context) =>
  key.endsWith("_minutes") ||
  (METRIC_AMOUNTS.has(key) && siblings.metric === "paid_minutes");

/** "required_rest_minutes" → "Required rest", "metric" → "Metric". */
export function factLabel(key: string): string {
  const words = key.replace(/_minutes$/, "").replaceAll("_", " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** One context value as text, with minute amounts shown in hours. */
export function factValue(
  key: string,
  value: unknown,
  siblings: Context = {},
): string {
  if (value === null || value === undefined) return "None";
  if (typeof value === "number" && isMinutes(key, siblings))
    return formatHours(value);
  if (value === "paid_minutes") return "paid hours";
  if (Array.isArray(value))
    return value.map((item) => factValue(key, item, siblings)).join("; ");
  if (typeof value === "object") {
    const nested = value as Context;
    return factEntries(nested)
      .map(([label, text]) => `${label}: ${text}`)
      .join(", ");
  }
  return String(value);
}

/** Label/text pairs for a finding's context, without record identifiers. */
export function factEntries(context: Context): [string, string][] {
  return Object.entries(context)
    .filter(([key]) => !isIdentifier(key))
    .map(([key, value]) => [factLabel(key), factValue(key, value, context)]);
}
