import { MockNotesApi } from "./mock";
import { TauriNotesApi } from "./tauri";
import type { NotesApi } from "./types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** The real backend inside the desktop app, sample data in a plain browser. */
export function createDefaultApi(): NotesApi {
  return isTauri() ? new TauriNotesApi() : new MockNotesApi();
}

export type * from "./types";
export { ApiError, toApiError } from "./errors";
