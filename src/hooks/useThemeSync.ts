import { useEffect } from "react";
import { applyTheme } from "../lib/theme";
import { useUi } from "../store/ui";

/** Applies the theme and re-applies it when the OS color scheme changes (for "system"). */
export function useThemeSync(): void {
  const theme = useUi((s) => s.theme);
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system" || typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);
}
