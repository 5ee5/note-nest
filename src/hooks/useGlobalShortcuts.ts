import { useEffect } from "react";
import { matchShortcut } from "../lib/shortcuts";
import { createNote } from "../lib/actions";
import { useUi } from "../store/ui";

/** Registers the app-wide shortcuts (palette, search, new note). */
export function useGlobalShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const action = matchShortcut(e);
      if (!action) return;
      e.preventDefault();
      const ui = useUi.getState();
      switch (action) {
        case "palette":
          ui.setPaletteOpen(!ui.paletteOpen);
          break;
        case "search":
          ui.setPaletteOpen(false);
          ui.focusSearch();
          break;
        case "newNote":
          void createNote();
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
