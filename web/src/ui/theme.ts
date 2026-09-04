import { useEffect, useState } from "react";

export type Theme = "light" | "dark";

/** Resolved effective theme, after system preference is applied. */
export type ResolvedTheme = Theme;

const STORAGE_KEY = "allo-theme";

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && !!window.matchMedia?.("(prefers-color-scheme: dark)").matches;
}

/** Read the saved theme; `null` = follow system. */
function readSaved(): Theme | null {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === "light" || saved === "dark" ? saved : null;
}

function resolve(saved: Theme | null): Theme {
  return saved ?? (systemPrefersDark() ? "dark" : "light");
}

function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}

/** Apply the saved/OS theme immediately on boot (before React paints). */
export function applyInitialTheme() {
  applyTheme(resolve(readSaved()));
}

/**
 * Theme hook. `data-theme="dark"` on <html> drives the CSS variables.
 * The saved value may be `"light" | "dark" | null` (null = follow system);
 * `theme` is always the resolved value; `saved` exposes the raw preference.
 */
export function useTheme() {
  const [theme, setTheme] = useState<Theme>(() => resolve(readSaved()));

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  /** Cycle light → dark → light. */
  const toggleTheme = () => {
    setTheme((prev) => {
      const next: Theme = prev === "dark" ? "light" : "dark";
      localStorage.setItem(STORAGE_KEY, next);
      return next;
    });
  };

  /** Set the resolved theme directly (settings dialog segmented control). */
  const setThemeValue = (value: Theme) => {
    setTheme(value);
    localStorage.setItem(STORAGE_KEY, value);
  };

  /** Raw saved preference (`"light" | "dark" | null` = follow system). */
  const savedTheme = localStorage.getItem(STORAGE_KEY) as Theme | null;

  return { theme, toggleTheme, setTheme: setThemeValue, savedTheme };
}
