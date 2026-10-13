import { useEffect, useState } from "react";
import { formatHours } from "../lib/duration";
import { readCommandResult } from "../lib/ipc";
import { errorText } from "../lib/recordFeedback";
import { previewShiftTimeAuthoritative } from "../lib/shiftTemplates";
import { previewShiftTime, type ShiftTimeCalculation } from "../lib/shiftTime";

const sameCalculation = (a: ShiftTimeCalculation, b: ShiftTimeCalculation) =>
  a.elapsed_minutes === b.elapsed_minutes &&
  a.unpaid_lunch_minutes === b.unpaid_lunch_minutes &&
  a.paid_minutes === b.paid_minutes &&
  a.overnight === b.overnight;

/**
 * Paid time for the entered clock times. A quick local result shows at once;
 * the app's own calculation replaces it after a short pause in typing.
 */
export function PaidTimePreview({
  start,
  end,
  savedPaidMinutes,
}: {
  start: string;
  end: string;
  /** The saved template's paid time, shown when the entered times differ. */
  savedPaidMinutes: number | null;
}) {
  const local = previewShiftTime(start, end);
  const [checked, setChecked] = useState<{
    start: string;
    end: string;
    calculation: ShiftTimeCalculation | null;
    error: string;
  } | null>(null);
  useEffect(() => {
    if (!previewShiftTime(start, end).ok) return;
    let canceled = false;
    const timer = window.setTimeout(() => {
      previewShiftTimeAuthoritative(start, end)
        .then(readCommandResult)
        .then(
          (calculation) => {
            if (!canceled) setChecked({ start, end, calculation, error: "" });
          },
          (error: unknown) => {
            if (!canceled)
              setChecked({
                start,
                end,
                calculation: null,
                error: errorText(error),
              });
          },
        );
    }, 200);
    return () => {
      canceled = true;
      window.clearTimeout(timer);
    };
  }, [start, end]);
  if (!local.ok)
    return (
      <div className="panel-quiet tpl-preview">
        <p className="notice notice-error" role="alert">
          {local.error.message}
        </p>
      </div>
    );
  const current =
    checked?.start === start && checked.end === end ? checked : null;
  const calculation = current?.calculation ?? local.value;
  const progress = current?.calculation
    ? sameCalculation(current.calculation, local.value)
      ? null
      : "Updated to the saved calculation."
    : current?.error
      ? null
      : "Calculating…";
  return (
    <div className="panel-quiet tpl-preview" role="status">
      <div className="tpl-calc">
        <p className="tpl-calc-paid display">
          {formatHours(calculation.paid_minutes)} paid
        </p>
        <p className="tpl-calc-detail">
          Elapsed {formatHours(calculation.elapsed_minutes)} · unpaid lunch{" "}
          {formatHours(calculation.unpaid_lunch_minutes)}
          {calculation.overnight && " · ends the next day"}
        </p>
        {progress && <p className="fine-print">{progress}</p>}
        {savedPaidMinutes !== null &&
          savedPaidMinutes !== calculation.paid_minutes && (
            <p className="fine-print">
              Saved template: {formatHours(savedPaidMinutes)} paid.
            </p>
          )}
      </div>
      {current?.error && (
        <p className="notice notice-error" role="alert">
          {current.error}
        </p>
      )}
    </div>
  );
}
