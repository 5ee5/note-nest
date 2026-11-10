import { create } from "zustand";
import { toApiError, type NoteDoc, type NoteSummary, type VaultChange } from "../lib/api";
import { useNotes } from "./notes";
import { useUi } from "./ui";

/** Delay after the last keystroke before the note is written to disk. */
export const AUTOSAVE_MS = 800;
/** Delay after the last keystroke before the text is stashed for crash recovery. */
export const DRAFT_MS = 300;

export type SaveStatus =
  | "idle"
  | "dirty"
  | "saving"
  | "saved"
  | "error"
  | "conflict"
  /** The note was deleted or moved outside NoteNest while it had unsaved text. */
  | "missing";

/** What happened to the open note after a change outside the app. */
export type ExternalOutcome =
  | "unaffected"
  /** Reloaded from disk (it had no unsaved text). */
  | "reloaded"
  /** Unsaved text met a newer version on disk: autosave is paused for a decision. */
  | "conflict"
  /** The note is gone and had no unsaved text: the editor was closed. */
  | "closed"
  /** The note is gone but has unsaved text: kept in the editor and in recovery drafts. */
  | "missing";

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
  /** Bumped whenever the text is replaced wholesale (open, reload); remounts the editor. */
  revision: number;
  /** While `status` is "missing": the note as it was before it disappeared. */
  missingNote: NoteSummary | null;

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
  /**
   * Brings the open note in line with changes made outside the app: reloads it if it has
   * no unsaved text, and never overwrites unsaved text (conflict or missing instead).
   */
  externalChange: (change: VaultChange) => Promise<ExternalOutcome>;
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
  missingNote: null as NoteSummary | null,
};
const INITIAL_REVISION = 0;

/** Autosave waits for the user's decision in these states. */
function paused(status: SaveStatus): boolean {
  return status === "conflict" || status === "missing";
}

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
      if (!s.noteId || s.baseHash === null || paused(s.status)) return;
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
      revision: get().revision + 1,
      noteId: doc.note.id,
      content: doc.content,
      savedContent: doc.content,
      baseHash: doc.contentHash,
    });
  }

  return {
    ...EMPTY,
    revision: INITIAL_REVISION,

    load: async (id) => {
      await get().flush();
      const token = ++loadToken;
      set({ ...EMPTY, revision: get().revision + 1, loading: true });
      try {
        const doc = await api().readNote(id);
        if (token !== loadToken) return;
        adopt(doc);
        useNotes.getState().applyNote(doc.note);
      } catch (e) {
        if (token !== loadToken) return;
        set({ ...EMPTY, revision: get().revision + 1, loadError: toApiError(e).message });
      }
    },

    edit: (text) => {
      const s = get();
      if (!s.noteId) return;
      const hold = paused(s.status);
      if (text === s.savedContent && !hold && !forceOverwrite) {
        clearTimers();
        set({ content: text, status: "saved", message: null });
        return;
      }
      set({
        content: text,
        status: hold ? s.status : "dirty",
        message: hold ? s.message : null,
      });
      clearTimeout(draftTimer);
      draftTimer = setTimeout(() => void stashNow(), DRAFT_MS);
      if (!hold) {
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

    externalChange: async (change) => {
      const id = get().noteId;
      if (!id || (!change.changed.includes(id) && !change.removed.includes(id))) {
        return "unaffected";
      }
      const summary = useNotes.getState().notes.find((n) => n.id === id) ?? null;
      // Let a save in progress land first, so its result is the baseline.
      while (inflight) await inflight;
      if (get().noteId !== id) return "unaffected";
      let doc: NoteDoc | null = null;
      try {
        doc = await api().readNote(id);
      } catch (e) {
        if (toApiError(e).code !== "not_found") {
          console.warn("could not re-read a note changed outside the app", e);
          return "unaffected";
        }
      }
      const s = get();
      if (s.noteId !== id) return "unaffected";
      const unsaved = s.content !== s.savedContent || forceOverwrite || paused(s.status);

      if (!doc) {
        if (!unsaved) {
          get().close();
          return "closed";
        }
        clearTimeout(saveTimer);
        forceOverwrite = false;
        set({
          status: "missing",
          message: "This note was deleted or moved outside NoteNest.",
          missingNote: summary,
        });
        await stashNow();
        return "missing";
      }
      useNotes.getState().applyNote(doc.note);
      if (doc.contentHash === s.baseHash) return "unaffected"; // nothing new on disk
      if (s.status === "conflict") return "conflict"; // already waiting for a decision
      if (!unsaved) {
        // Applied to the editor as a minimal edit, so the cursor and scroll stay put.
        clearTimers();
        set({
          content: doc.content,
          savedContent: doc.content,
          baseHash: doc.contentHash,
          status: s.status === "idle" ? "idle" : "saved",
          message: null,
        });
        return "reloaded";
      }
      clearTimeout(saveTimer);
      set({ status: "conflict", message: "Changed outside NoteNest while you were editing." });
      await stashNow();
      return "conflict";
    },

    close: () => {
      clearTimers();
      loadToken++;
      forceOverwrite = false;
      set({ ...EMPTY, revision: get().revision + 1 });
    },
  };
});
