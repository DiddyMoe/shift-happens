import type { KeyboardEvent } from "react";

// Elements Tab can stop on in these dialogs. A summary is a stop only as its
// details' toggle, which `tabIndex` reports.
const CANDIDATES =
  "a[href], button, input, select, textarea, summary, [tabindex]";

/** Whether Tab can stop on `control`, as the browser decides it. */
function tabbable(control: HTMLElement, dialog: HTMLElement) {
  if (
    control.tabIndex < 0 ||
    // Also true for every control in a disabled fieldset.
    control.matches(":disabled") ||
    control.closest("[inert]") !== null ||
    // Hidden, or inside a closed details.
    !control.checkVisibility({ visibilityProperty: true })
  )
    return false;
  // A radio group with a checked button is one stop, at that button.
  return !(
    control instanceof HTMLInputElement &&
    control.type === "radio" &&
    !control.checked &&
    control.name &&
    dialog.querySelector(
      `input[type="radio"][name="${CSS.escape(control.name)}"]:checked`,
    )
  );
}

/**
 * Keep keyboard navigation inside the modal, including WebView focus
 * boundaries: Tab past the last stop wraps to the first, and Shift+Tab before
 * the first wraps to the last. Elsewhere the browser moves focus itself.
 */
export function trapDialogFocus(event: KeyboardEvent<HTMLDialogElement>) {
  if (event.key !== "Tab" || event.defaultPrevented) return;
  const dialog = event.currentTarget;
  const active = document.activeElement ?? dialog;
  const controls = [...dialog.querySelectorAll<HTMLElement>(CANDIDATES)].filter(
    (control) => tabbable(control, dialog),
  );
  const direction = event.shiftKey
    ? Node.DOCUMENT_POSITION_PRECEDING
    : Node.DOCUMENT_POSITION_FOLLOWING;
  if (
    controls.some(
      (control) => active.compareDocumentPosition(control) & direction,
    )
  )
    return;
  event.preventDefault();
  const target = event.shiftKey ? controls.at(-1) : controls[0];
  if (target) target.focus();
  else dialog.focus();
}
