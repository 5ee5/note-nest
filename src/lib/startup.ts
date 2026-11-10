import { isTheme } from "./theme";
import { useNotes } from "../store/notes";
import { useRecovery } from "../store/recovery";
import { isEditorMode, useUi } from "../store/ui";

/** Loads everything the UI needs once a vault is available. */
export async function afterVaultOpen(): Promise<void> {
  const { api, load } = useNotes.getState();
  await load();
  try {
    const theme = await api.getSetting("theme");
    if (isTheme(theme)) useUi.getState().setTheme(theme);
    useUi.getState().setLastBackup(await api.getSetting("last_backup"));
    const mode = await api.getSetting("editorMode");
    if (isEditorMode(mode)) useUi.getState().setEditorMode(mode);
  } catch (e) {
    console.warn("could not read saved settings", e);
  }
  await useRecovery.getState().load();
}
