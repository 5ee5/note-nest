import { create } from "zustand";
import { toApiError } from "../lib/api";
import { useNotes } from "./notes";
import { useUi } from "./ui";

/** Delay after the last keystroke before the note is written to disk. */
export const AUTOSAVE_MS = 800;
/** Delay after the last keystroke before the text is stashed for crash recovery. */
export const DRAFT_MS = 300;

export type SaveStatus = "idle" | "dirty" | "saving" | "saved" | "error" | "conflict";

interface EditorState {
  noteId: string | null;
  /** What is in the text box. */
  content: string;
  /** The last content known to be on disk. */
  savedContent: string;
  /** Hash of the file when it was last read or written by this editor. */
  baseHash: string | null;
  status: SaveStatus;
  message: string | null;
  loading: boolean;
  loadError: string | null;

  load: (id: string) => Promise<void>;
  edit: (text: string) => void;
  /** Saves pending changes now and waits for any save in progress. */
  flush: () => Promise<void>;
  /** Conflict: overwrite the file with my text (the disk version is backed up first). */
  keepMine: () => Promise<void>;
  /** Conflict: keep my text in recovery drafts and show the version on disk. */
  loadTheirs: () => Promise<void>;
  /** Puts recovered text into the editor; `overwrite` replaces newer disk content (backed up). */
  restoreContent: (text: string, overwrite: boolean) => void;
  /** Re-reads the open note after its location may have changed; closes it if gone. */
  reopen: () => Promise<void>;
  close: () => void;
}

const api = () => useNotes.getState().api;

let saveTimer: ReturnType<typeof setTimeout> | undefined;
let draftTimer: ReturnType<typeof setTimeout> | undefined;
let inflight: Promise<void> | null = null;
let resave = false;
let forceOverwrite = false;
let loadToken = 0;

const EMPTY = {
  noteId: null,
  content: "",
  savedContent: "",
  baseHash: null,
  status: "idle" as SaveStatus,
  message: null,
  loading: false,
  loadError: null,
};

function clearTimers(): void {
  clearTimeout(saveTimer);
  clearTimeout(draftTimer);
}

export const useEditor = create<EditorState>((set, get) => {
  /** Keeps the user's unsaved text safe in the recovery area. Never throws. */
  async function stashNow(): Promise<void> {
    const s = get();
    if (!s.noteId || s.baseHash === null || s.content === s.savedContent) return;
    try {
      await api().stashDraft(s.noteId, s.content, s.baseHash);
    } catch (e) {
      console.warn("could not stash draft", e);
    }
  }

  async function doSave(): Promise<void> {
    do {
      resave = false;
      const s = get();
      if (!s.noteId || s.baseHash === null || s.status === "conflict") return;
      if (s.content === s.savedContent && !forceOverwrite) {
        if (s.status === "dirty") set({ status: "saved" });
        return;
      }
      const id = s.noteId;
      const sent = s.content;
      const overwrite = forceOverwrite;
      set({ status: "saving", message: null });
      try {
        const doc = await api().saveNote(
          id,
          sent,
          overwrite ? { overwrite: true } : { baseHash: s.baseHash },
        );
        forceOverwrite = false;
        if (get().noteId !== id) return; // the user moved on; nothing to apply
        const unchanged = get().content === sent;
        set({
          noteId: doc.note.id,
          savedContent: doc.content,
          baseHash: doc.contentHash,
          // The backend may add the note's id to the frontmatter; adopt what was written.
          content: unchanged ? doc.content : get().content,
          status: unchanged ? "saved" : "dirty",
        });
        if (doc.note.id === id) useNotes.getState().applyNote(doc.note);
        else {
          useUi.getState().selectNote(doc.note.id);
          void useNotes.getState().load();
        }
        if (!unchanged) resave = true;
      } catch (e) {
        const err = toApiError(e);
        set({ status: err.code === "conflict" ? "conflict" : "error", message: err.message });
        await stashNow();
        return;
      }
    } while (resave);
  }

  function save(): Promise<void> {
    if (inflight) {
      resave = true;
      return inflight;
    }
    inflight = doSave().finally(() => {
      inflight = null;
    });
    return inflight;
  }

  function adopt(doc: { note: { id: string }; content: string; contentHash: string }): void {
    set({
      ...EMPTY,
      noteId: doc.note.id,
      content: doc.content,
      savedContent: doc.content,
      baseHash: doc.contentHash,
    });
  }

  return {
    ...EMPTY,

    load: async (id) => {
      await get().flush();
      const token = ++loadToken;
      set({ ...EMPTY, loading: true });
      try {
        const doc = await api().readNote(id);
        if (token !== loadToken) return;
        adopt(doc);
        useNotes.getState().applyNote(doc.note);
      } catch (e) {
        if (token !== loadToken) return;
        set({ ...EMPTY, loadError: toApiError(e).message });
      }
    },

    edit: (text) => {
      const s = get();
      if (!s.noteId) return;
      const conflict = s.status === "conflict";
      if (text === s.savedContent && !conflict && !forceOverwrite) {
        clearTimers();
        set({ content: text, status: "saved", message: null });
        return;
      }
      set({
        content: text,
        status: conflict ? "conflict" : "dirty",
        message: conflict ? s.message : null,
      });
      clearTimeout(draftTimer);
      draftTimer = setTimeout(() => void stashNow(), DRAFT_MS);
      if (!conflict) {
        clearTimeout(saveTimer);
        saveTimer = setTimeout(() => void save(), AUTOSAVE_MS);
      }
    },

    flush: async () => {
      clearTimers();
      await save();
      await stashNow(); // no-op unless unsaved text remains (failed save or conflict)
    },

    keepMine: async () => {
      forceOverwrite = true;
      set({ status: "dirty", message: null });
      await save();
    },

    loadTheirs: async () => {
      await stashNow();
      const id = get().noteId;
      if (!id) return;
      try {
        adopt(await api().readNote(id));
        useUi.getState().notify("info", "Your version was kept in recovery drafts.");
      } catch (e) {
        set({ status: "error", message: toApiError(e).message });
      }
    },

    restoreContent: (text, overwrite) => {
      forceOverwrite = overwrite;
      get().edit(text);
      if (overwrite) {
        clearTimeout(saveTimer);
        saveTimer = setTimeout(() => void save(), AUTOSAVE_MS);
      }
    },

    reopen: async () => {
      const id = get().noteId;
      if (!id) return;
      try {
        const doc = await api().readNote(id);
        adopt(doc);
      } catch {
        get().close();
        useUi.getState().selectNote(null);
      }
    },

    close: () => {
      clearTimers();
      loadToken++;
      forceOverwrite = false;
      set({ ...EMPTY });
    },
  };
});
