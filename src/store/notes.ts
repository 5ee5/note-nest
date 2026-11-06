import { create } from "zustand";
import {
  createDefaultApi,
  toApiError,
  type NoteSummary,
  type NotesApi,
  type TrashItem,
} from "../lib/api";

interface NotesState {
  api: NotesApi;
  notes: NoteSummary[];
  folders: string[];
  trash: TrashItem[];
  status: "idle" | "loading" | "ready" | "error";
  error: string | null;

  /** Reloads notes, folders and trash from the backend. */
  load: () => Promise<void>;
  /** Replaces one note in the list without a full reload. */
  applyNote: (note: NoteSummary) => void;
}

export function createNotesStore(api: NotesApi) {
  return create<NotesState>((set, get) => ({
    api,
    notes: [],
    folders: [],
    trash: [],
    status: "idle",
    error: null,

    load: async () => {
      set({ status: "loading" });
      try {
        const current = get().api;
        const [notes, folders, trash] = await Promise.all([
          current.listNotes(),
          current.listFolders(),
          current.listTrash(),
        ]);
        set({ notes, folders, trash, status: "ready", error: null });
      } catch (e) {
        set({ status: "error", error: toApiError(e).message });
      }
    },

    applyNote: (note) =>
      set((s) => ({
        notes: s.notes.some((n) => n.id === note.id)
          ? s.notes.map((n) => (n.id === note.id ? note : n))
          : [...s.notes, note],
      })),
  }));
}

/** The app-wide store. Tests replace the backend with `useNotes.setState({ api })`. */
export const useNotes = createNotesStore(createDefaultApi());
