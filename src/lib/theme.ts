import { safeStorage } from "./safeStorage";

export type Theme = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

export const THEMES: readonly Theme[] = ["light", "dark", "system"];
const KEY = "notenest.theme";

export function isTheme(value: unknown): value is Theme {
  return value === "light" || value === "dark" || value === "system";
}

export function loadTheme(): Theme {
  const stored = safeStorage.getItem(KEY);
  return isTheme(stored) ? stored : "system";
}

export function saveTheme(theme: Theme): void {
  safeStorage.setItem(KEY, theme);
}

export function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches
    : false;
}

export function resolveTheme(theme: Theme, prefersDark: boolean): ResolvedTheme {
  if (theme === "system") return prefersDark ? "dark" : "light";
  return theme;
}

/** Applies the resolved theme to the document through `data-theme`. */
export function applyTheme(theme: Theme): ResolvedTheme {
  const resolved = resolveTheme(theme, systemPrefersDark());
  document.documentElement.dataset.theme = resolved;
  return resolved;
}
