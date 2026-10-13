import { useEffect, type RefObject } from "react";

/**
 * Scrolls an inline form into view when it opens off-screen, so pressing an
 * Add or Edit button never opens a form below the fold where it looks like
 * nothing happened. A form that is already partly visible (for example one
 * re-rendered after a save) stays put, so result messages remain in view.
 */
export function useRevealOnMount(ref: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const box = element.getBoundingClientRect();
    const scroller = element.closest(".main-content, .sheet-body");
    const view = scroller?.getBoundingClientRect() ?? {
      top: 0,
      bottom: window.innerHeight,
    };
    if (box.top < view.bottom - 48 && box.bottom > view.top) return;
    const reduce = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    element.scrollIntoView({
      block: "nearest",
      behavior: reduce ? "auto" : "smooth",
    });
  }, [ref]);
}
