import { useId, useRef, useState, type ReactNode } from "react";

/**
 * A button that shows a small panel of choices in the top layer. It uses the
 * native `popover` attribute, so Escape and a click outside close it. The
 * panel opens under the button, aligned to its right edge, and focus returns
 * to the button when it closes.
 *
 * Choosing a link or button in the panel closes it before the choice's own
 * click handler runs: focus moves to the button first, so a dialog opened by
 * the choice restores focus to the button rather than to a hidden item.
 */
export function Popover({
  label,
  ariaLabel,
  className = "btn",
  disabled = false,
  panelLabel,
  panelClassName,
  children,
}: {
  label: ReactNode;
  ariaLabel?: string | undefined;
  className?: string | undefined;
  disabled?: boolean | undefined;
  /** Names the panel for assistive technology. */
  panelLabel: string;
  panelClassName?: string | undefined;
  children: ReactNode;
}) {
  const id = useId();
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(false);
  function place() {
    const anchor = button.current?.getBoundingClientRect();
    const element = panel.current;
    if (!anchor || !element) return;
    element.style.top = `${Math.round(anchor.bottom + 4)}px`;
    element.style.left = "auto";
    element.style.right = `${Math.max(8, Math.round(window.innerWidth - anchor.right))}px`;
  }
  // Keeps the open panel inside the window: above the button when there is
  // no room below it, and clear of the left edge.
  function fit() {
    const anchor = button.current?.getBoundingClientRect();
    const element = panel.current;
    if (!anchor || !element) return;
    const box = element.getBoundingClientRect();
    if (box.bottom > window.innerHeight - 8 && anchor.top - box.height - 4 > 8)
      element.style.top = `${Math.round(anchor.top - box.height - 4)}px`;
    if (box.left < 8) {
      element.style.left = "8px";
      element.style.right = "auto";
    }
  }
  function close() {
    button.current?.focus();
    if (panel.current?.matches(":popover-open")) panel.current.hidePopover();
  }
  return (
    <>
      <button
        ref={button}
        type="button"
        className={className}
        disabled={disabled}
        aria-label={ariaLabel}
        aria-expanded={expanded}
        aria-controls={id}
        popoverTarget={id}
      >
        {label}
      </button>
      <div
        ref={panel}
        id={id}
        popover="auto"
        role="group"
        aria-label={panelLabel}
        className={panelClassName ? `popover ${panelClassName}` : "popover"}
        onClickCapture={(event) => {
          const item =
            event.target instanceof Element
              ? event.target.closest("a[href], button")
              : null;
          if (item && panel.current?.contains(item)) close();
        }}
        onBeforeToggle={(event) => {
          if (event.newState === "open") place();
        }}
        onToggle={(event) => {
          const open = event.newState === "open";
          setExpanded(open);
          if (open) {
            fit();
            panel.current
              ?.querySelector<HTMLElement>("a[href], button:not(:disabled)")
              ?.focus();
          } else if (
            !document.activeElement ||
            document.activeElement === document.body ||
            panel.current?.contains(document.activeElement)
          )
            button.current?.focus();
        }}
      >
        {children}
      </div>
    </>
  );
}

/** The downward chevron on a button that opens a popover. */
export function MenuChevron() {
  return (
    <svg className="icon" viewBox="0 0 16 16" aria-hidden="true">
      <path
        d="m4 6 4 4 4-4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
