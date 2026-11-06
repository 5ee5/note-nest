import { isTauri } from "./api";

const ALLOWED = /^(https?:|mailto:)/i;

/**
 * Opens a link in the user's browser or mail client. Only http, https and mailto are
 * ever opened; anything else is ignored. On desktop this goes through the opener
 * plugin (whose capability is scoped to the same schemes).
 */
export async function openExternal(url: string): Promise<boolean> {
  if (!ALLOWED.test(url.trim())) return false;
  if (isTauri()) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
  return true;
}
