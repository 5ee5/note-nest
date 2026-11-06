import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AppLayout } from "./components/AppLayout";
import { CommandPalette } from "./components/CommandPalette";
import { SettingsDialog } from "./components/SettingsDialog";
import { useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import { useThemeSync } from "./hooks/useThemeSync";
import { useNotes } from "./store/notes";

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export default function App() {
  useThemeSync();
  useGlobalShortcuts();

  useEffect(() => {
    void useNotes.getState().load();
    if (isTauri()) {
      invoke<string>("app_info")
        .then((info) => console.info(info))
        .catch((e) => console.error("app_info failed", e));
    }
  }, []);

  return (
    <div className="h-screen bg-bg text-fg">
      <AppLayout />
      <CommandPalette />
      <SettingsDialog />
    </div>
  );
}
