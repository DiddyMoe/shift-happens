import { useEffect, useRef } from "react";

/**
 * Shows the returned dialog as a native modal while the component is
 * mounted. Unmounting closes it and returns focus to the element that had it
 * before, if that element is still on the page.
 */
export function useModalDialog() {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    const modal = dialog.current;
    modal?.showModal();
    return () => {
      modal?.close();
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus();
    };
  }, []);
  return dialog;
}
