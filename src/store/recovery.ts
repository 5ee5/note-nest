import { create } from "zustand";
import type { Draft } from "../lib/api";
import { useNotes } from "./notes";

interface RecoveryState {
  drafts: Draft[];
  /** Looks for unsaved text left behind by an unexpected exit. */
  load: () => Promise<void>;
  /** Removes a draft from the list (after it was restored or discarded). */
  remove: (noteId: string) => void;
}

export const useRecovery = create<RecoveryState>((set) => ({
  drafts: [],
  load: async () => {
    try {
      set({ drafts: await useNotes.getState().api.listDrafts() });
    } catch (e) {
      console.warn("could not list recovery drafts", e);
    }
  },
  remove: (noteId) => set((s) => ({ drafts: s.drafts.filter((d) => d.noteId !== noteId) })),
}));
