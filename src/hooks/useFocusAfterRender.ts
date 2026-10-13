import { useCallback, useEffect, useRef } from "react";

type Target = () => HTMLElement | null | undefined;

/**
 * Moves focus after the next render, once the screen shows the outcome of an
 * action. Use it when the focused control is about to disappear (a form that
 * closes, a button that hides), so focus lands somewhere sensible instead of
 * falling back to the page.
 *
 * With `onlyIfLost`, focus that is still on a visible element is left alone.
 */
export function useFocusAfterRender() {
  const pending = useRef<{ target: Target; onlyIfLost: boolean } | null>(null);
  useEffect(() => {
    const request = pending.current;
    if (!request) return;
    pending.current = null;
    const active = document.activeElement;
    const lost =
      !active || active === document.body || !active.checkVisibility();
    if (request.onlyIfLost && !lost) return;
    request.target()?.focus();
  });
  return useCallback((target: Target, onlyIfLost = false) => {
    pending.current = { target, onlyIfLost };
  }, []);
}
