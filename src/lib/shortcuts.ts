export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || "");
}

/** True when the platform modifier (Cmd on macOS, Ctrl elsewhere) is held. */
export function hasMod(e: Pick<KeyboardEvent, "ctrlKey" | "metaKey">, mac = isMac()): boolean {
  return mac ? e.metaKey : e.ctrlKey;
}

export type ShortcutAction = "palette" | "search" | "newNote";

export function matchShortcut(
  e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">,
  mac = isMac(),
): ShortcutAction | null {
  if (!hasMod(e, mac) || e.altKey || e.shiftKey) return null;
  switch (e.key.toLowerCase()) {
    case "k":
      return "palette";
    case "p":
      return "search";
    case "n":
      return "newNote";
    default:
      return null;
  }
}

export function modLabel(mac = isMac()): string {
  return mac ? "⌘" : "Ctrl";
}
