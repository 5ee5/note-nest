import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./lib/api";
import { AppLayout } from "./components/AppLayout";
import { CommandPalette } from "./components/CommandPalette";
import { LinkUpdateDialog } from "./components/LinkUpdateDialog";
import { Notices } from "./components/Notices";
import { PromptDialog } from "./components/PromptDialog";
import { RecoveryDialog } from "./components/RecoveryDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { VaultGate } from "./components/VaultGate";
import { useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import { usePreventFileNavigation } from "./hooks/usePreventFileNavigation";
import { useThemeSync } from "./hooks/useThemeSync";

export default function App() {
  useThemeSync();
  useGlobalShortcuts();
  usePreventFileNavigation();

  useEffect(() => {
    if (isTauri()) {
      invoke<string>("app_info")
        .then((info) => console.info(info))
        .catch((e) => console.error("app_info failed", e));
    }
  }, []);

  return (
    <div className="h-screen bg-bg text-fg">
      <VaultGate>
        <AppLayout />
        <CommandPalette />
        <SettingsDialog />
        <PromptDialog />
        <LinkUpdateDialog />
        <RecoveryDialog />
      </VaultGate>
      <Notices />
    </div>
  );
}
