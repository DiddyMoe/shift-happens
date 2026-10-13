import type { ThemePreference } from "./ipc";

// The saved theme lives in the database, which loads after the first paint.
// Remembering the last one locally lets that first paint use it too.
const KEY = "shifthappens.theme";

export function rememberedTheme(): ThemePreference {
  try {
    const value = window.localStorage.getItem(KEY);
    if (value === "dark" || value === "light" || value === "system")
      return value;
  } catch {
    // Unavailable storage just means the default theme until settings load.
  }
  return "dark";
}

export function rememberTheme(theme: ThemePreference) {
  try {
    window.localStorage.setItem(KEY, theme);
  } catch {
    // Remembering is a convenience; the saved setting still applies.
  }
}

/** Sets the page theme; "system" follows Windows' light or dark mode. */
export function applyTheme(theme: ThemePreference) {
  document.documentElement.dataset.theme =
    theme === "system"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme;
}
