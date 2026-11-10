import { create } from "zustand";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { isTauri, toApiError } from "../lib/api";
import { vaultApi } from "../lib/api/tauri";
import { afterVaultOpen } from "../lib/startup";
import { useEditor } from "./editor";
import { useUi } from "./ui";

type Phase = "checking" | "choose" | "opening" | "ready";

interface VaultState {
  phase: Phase;
  /** Open vault location (desktop app only). */
  path: string | null;
  defaultPath: string | null;
  /** Why the last attempt to open a vault failed. */
  error: string | null;

  initialize: () => Promise<void>;
  openAt: (path: string, create: boolean) => Promise<void>;
  chooseFolder: () => Promise<void>;
  /** Leaves the current vault (after saving) so another can be chosen. */
  change: () => Promise<void>;
}

export const useVault = create<VaultState>((set, get) => ({
  phase: "checking",
  path: null,
  defaultPath: null,
  error: null,

  initialize: async () => {
    if (!isTauri()) {
      await afterVaultOpen();
      set({ phase: "ready" });
      return;
    }
    try {
      const status = await vaultApi.status();
      set({ defaultPath: status.default_path });
      if (status.open && status.configured_path) {
        set({ path: status.configured_path });
        await afterVaultOpen();
        set({ phase: "ready" });
      } else if (status.configured_path) {
        await get().openAt(status.configured_path, false);
      } else {
        set({ phase: "choose" });
      }
    } catch (e) {
      set({ phase: "choose", error: toApiError(e).message });
    }
  },

  openAt: async (path, create) => {
    set({ phase: "opening", error: null });
    try {
      const opened = await vaultApi.open(path, create);
      const { notify } = useUi.getState();
      if (opened.index_rebuilt) {
        notify("warning", "The search index was damaged and has been rebuilt from your notes.");
      }
      if (opened.unreadable.length > 0) {
        notify(
          "warning",
          `${opened.unreadable.length} note(s) could not be read: ${opened.unreadable.join(", ")}`,
        );
      }
      if (opened.duplicate_ids_fixed.length > 0) {
        notify("info", `Copied notes were given new ids: ${opened.duplicate_ids_fixed.join(", ")}`);
      }
      if (!opened.watching) {
        notify(
          "warning",
          "NoteNest cannot watch this vault for changes made in other apps, so it checks for them when its window is focused.",
        );
      }
      set({ path: opened.path });
      await afterVaultOpen();
      set({ phase: "ready" });
    } catch (e) {
      set({ phase: "choose", error: toApiError(e).message });
    }
  },

  chooseFolder: async () => {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "Choose your NoteNest vault folder",
      });
      if (typeof picked === "string") await get().openAt(picked, false);
    } catch (e) {
      set({ error: toApiError(e).message });
    }
  },

  change: async () => {
    await useEditor.getState().flush();
    useEditor.getState().close();
    useUi.getState().selectNote(null);
    set({ phase: "choose", error: null });
  },
}));
