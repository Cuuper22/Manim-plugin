import { useCallback, useState } from "react";
import { readPreference, writePreference } from "../preferences.ts";

export type ThemeChoice = "system" | "light" | "dark";

const ORDER: readonly ThemeChoice[] = ["system", "light", "dark"];

export function storedTheme(): ThemeChoice {
  const value = readPreference("theme");
  return value === "light" || value === "dark" ? value : "system";
}

/** `system` follows `prefers-color-scheme`; the tokens read `data-theme` otherwise. */
export function applyTheme(theme: ThemeChoice): void {
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
}

/** The color theme: the OS setting unless chosen here; `next` cycles system → light → dark. */
export function useTheme(): { theme: ThemeChoice; next: () => void } {
  const [theme, setTheme] = useState(storedTheme);
  const next = useCallback(() => {
    const chosen = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length]!;
    applyTheme(chosen);
    writePreference("theme", chosen === "system" ? null : chosen);
    setTheme(chosen);
  }, [theme]);
  return { theme, next };
}
