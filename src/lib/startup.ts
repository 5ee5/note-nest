import { isTheme } from "./theme";
import { useNotes } from "../store/notes";
import { useRecovery } from "../store/recovery";
import { useUi } from "../store/ui";

/** Loads everything the UI needs once a vault is available. */
export async function afterVaultOpen(): Promise<void> {
  const { api, load } = useNotes.getState();
  await load();
  try {
    const theme = await api.getSetting("theme");
    if (isTheme(theme)) useUi.getState().setTheme(theme);
  } catch (e) {
    console.warn("could not read the theme setting", e);
  }
  await useRecovery.getState().load();
}
