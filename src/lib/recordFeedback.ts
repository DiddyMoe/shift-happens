import { describeError, describeWarnings } from "./errors";
import { unreachableResult, type CommandResult } from "./ipc";

export const errorText = (error: unknown) =>
  error instanceof Error
    ? error.message
    : "Shift Happens couldn't finish that.";

export interface Feedback {
  message: string;
  stale: boolean;
  failed: boolean;
}
export function mutationFeedback<T>(
  result: CommandResult<T>,
  success: (data: T) => void,
  setFeedback: (value: Feedback) => void,
  message: string,
) {
  switch (result.status) {
    case "success":
      success(result.data);
      setFeedback({
        message: [message, describeWarnings(result.warnings)]
          .filter(Boolean)
          .join(" "),
        stale: false,
        failed: false,
      });
      break;
    case "no_change":
      success(result.data);
      setFeedback({
        message: "No changes to save.",
        stale: false,
        failed: false,
      });
      break;
    case "canceled":
      setFeedback({ message: "Canceled.", stale: false, failed: false });
      break;
    case "requires_overwrite":
    case "requires_confirmation":
      setFeedback({
        message: "Review the warnings and confirm this change.",
        stale: false,
        failed: false,
      });
      break;
    case "stale_revision":
      setFeedback({
        message: describeError(result.error),
        stale: true,
        failed: true,
      });
      break;
    case "validation_error":
    case "blocked":
    case "failure":
      setFeedback({
        message: describeError(result.error),
        stale: false,
        failed: true,
      });
      break;
    default:
      unreachableResult(result);
  }
}
