import { create } from "zustand";
import type { NoteSummary, NotesApi } from "../lib/api/types";
import { MockNotesApi } from "../lib/api/mock";

interface NotesState {
  api: NotesApi;
  notes: NoteSummary[];
  folders: string[];
  status: "idle" | "loading" | "ready" | "error";
  error: string | null;

  load: () => Promise<void>;
  /** Creates a note and returns it, or null on failure (error is stored). */
  createNote: () => Promise<NoteSummary | null>;
  openDailyNote: () => Promise<NoteSummary | null>;
  markOpened: (id: string) => Promise<void>;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function createNotesStore(api: NotesApi) {
  return create<NotesState>((set, get) => {
    const refresh = async () => {
      const [notes, folders] = await Promise.all([api.listNotes(), api.listFolders()]);
      set({ notes, folders, status: "ready", error: null });
    };
    return {
      api,
      notes: [],
      folders: [],
      status: "idle",
      error: null,

      load: async () => {
        set({ status: "loading" });
        try {
          await refresh();
        } catch (e) {
          set({ status: "error", error: message(e) });
        }
      },
      createNote: async () => {
        try {
          const note = await get().api.createNote();
          await refresh();
          return note;
        } catch (e) {
          set({ error: message(e) });
          return null;
        }
      },
      openDailyNote: async () => {
        try {
          const note = await get().api.openDailyNote();
          await refresh();
          return note;
        } catch (e) {
          set({ error: message(e) });
          return null;
        }
      },
      markOpened: async (id) => {
        try {
          await get().api.markOpened(id);
          await refresh();
        } catch (e) {
          set({ error: message(e) });
        }
      },
    };
  });
}

export const useNotes = createNotesStore(new MockNotesApi());
