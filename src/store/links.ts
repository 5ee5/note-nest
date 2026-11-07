import { create } from "zustand";
import { toApiError, type NoteLinks } from "../lib/api";
import { useNotes } from "./notes";

export type LinksTab = "backlinks" | "links" | "related";

interface LinksState {
  /** The note the data belongs to. */
  noteId: string | null;
  data: NoteLinks | null;
  error: string | null;
  open: boolean;
  tab: LinksTab;

  /** Loads backlinks, outgoing links and related notes for a note (null clears). */
  refresh: (noteId: string | null) => Promise<void>;
  setOpen: (open: boolean) => void;
  setTab: (tab: LinksTab) => void;
}

let ticket = 0;

export const useLinks = create<LinksState>((set) => ({
  noteId: null,
  data: null,
  error: null,
  open: false,
  tab: "backlinks",

  refresh: async (noteId) => {
    const mine = ++ticket;
    if (noteId === null) {
      set({ noteId: null, data: null, error: null });
      return;
    }
    try {
      const data = await useNotes.getState().api.getNoteLinks(noteId);
      if (mine === ticket) set({ noteId, data, error: null });
    } catch (e) {
      if (mine === ticket) set({ noteId, data: null, error: toApiError(e).message });
    }
  },
  setOpen: (open) => set({ open }),
  setTab: (tab) => set({ tab }),
}));
