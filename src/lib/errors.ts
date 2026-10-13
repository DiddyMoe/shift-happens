import type { AppError } from "./ipc";

/** A backend error in plain words. Codes and record IDs stay out of the text;
 * records that block the change are named. */
export function describeError(error: AppError): string {
  const names = error.context.dependency_names ?? [];
  if (!names.length) return error.message;
  const total = Math.max(
    error.context.dependency_count ?? names.length,
    names.length,
  );
  const shown = names.slice(0, 3);
  const more = total - shown.length;
  return `${error.message} Used by: ${shown.join(", ")}${more > 0 ? ` and ${more} more` : ""}.`;
}

export function describeWarnings(warnings?: readonly AppError[]): string {
  return (warnings ?? []).map(describeError).join(" ");
}
