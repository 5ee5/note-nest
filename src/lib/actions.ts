import { toApiError } from "./api";
import { localDateString } from "./api/mock";
import type { Theme } from "./theme";
import type { Draft } from "./api";
import { useEditor } from "../store/editor";
import { useRecovery } from "../store/recovery";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

/**
 * User-facing operations shared by shortcuts, the command palette and the UI.
 * Each one reports failures as a visible notice instead of failing silently.
 */

const api = () => useNotes.getState().api;
const ui = () => useUi.getState();

async function attempt<T>(what: string, fn: () => Promise<T>): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    ui().notify("error", `${what}: ${toApiError(e).message}`);
    return undefined;
  }
}

/** Opens a note in the editor, saving whatever was being edited first. */
export async function openNote(id: string): Promise<void> {
  await useEditor.getState().flush();
  ui().selectNote(id);
  await useEditor.getState().load(id);
  await attempt("Could not record the opened note", async () => {
    await api().markOpened(id);
    const note = useNotes.getState().notes.find((n) => n.id === id);
    if (note) useNotes.getState().applyNote({ ...note, lastOpenedAt: new Date().toISOString() });
  });
}

export async function createNote(folder?: string | null): Promise<void> {
  const view = ui().view;
  const target = folder !== undefined ? folder : view.kind === "folder" ? view.value : null;
  const note = await attempt("Could not create the note", () => api().createNote(target));
  if (!note) return;
  await useNotes.getState().load();
  if (view.kind !== "folder") ui().setView({ kind: "all" });
  await openNote(note.id);
}

export async function openDailyNote(): Promise<void> {
  const title = localDateString();
  const path = `Journal/${title}.md`;
  const id = await attempt("Could not open the daily note", async () => {
    await useNotes.getState().load();
    let existing = useNotes.getState().notes.find((n) => n.path === path);
    if (!existing) {
      if (!useNotes.getState().folders.includes("Journal")) await api().createFolder("Journal");
      existing = await api().createNote("Journal", title);
    }
    return existing.id;
  });
  if (!id) return;
  await useNotes.getState().load();
  ui().setView({ kind: "daily" });
  await openNote(id);
}

/** Runs a structural change on the open note's file, keeping the editor consistent. */
async function withEditorSettled(fn: () => Promise<void>): Promise<void> {
  await useEditor.getState().flush();
  await fn();
  await useNotes.getState().load();
}

export async function renameNote(id: string, title: string): Promise<boolean> {
  let ok = false;
  await attempt("Could not rename the note", () =>
    withEditorSettled(async () => {
      const note = await api().renameNote(id, title);
      ok = true;
      if (ui().selectedNoteId === id) {
        ui().selectNote(note.id);
        await useEditor.getState().load(note.id);
      }
    }),
  );
  return ok;
}

export async function deleteNote(id: string): Promise<void> {
  await attempt("Could not delete the note", () =>
    withEditorSettled(async () => {
      await api().deleteNote(id);
      if (ui().selectedNoteId === id) {
        ui().selectNote(null);
        useEditor.getState().close();
      }
    }),
  );
}

export async function duplicateNote(id: string): Promise<void> {
  const copy = await attempt("Could not duplicate the note", async () => {
    await useEditor.getState().flush();
    return api().duplicateNote(id);
  });
  if (!copy) return;
  await useNotes.getState().load();
  await openNote(copy.id);
}

async function reloadOpenNote(id: string): Promise<void> {
  if (ui().selectedNoteId === id) await useEditor.getState().load(id);
}

export async function setPinned(id: string, pinned: boolean): Promise<void> {
  await attempt("Could not change the pin", () =>
    withEditorSettled(async () => {
      await api().setPinned(id, pinned);
      await reloadOpenNote(id);
    }),
  );
}

export async function setArchived(id: string, archived: boolean): Promise<void> {
  await attempt("Could not change the archive state", () =>
    withEditorSettled(async () => {
      await api().setArchived(id, archived);
      await reloadOpenNote(id);
    }),
  );
}

export async function createFolder(path: string): Promise<boolean> {
  let ok = false;
  await attempt("Could not create the folder", async () => {
    await api().createFolder(path);
    ok = true;
    await useNotes.getState().load();
  });
  return ok;
}

async function afterFolderChange(): Promise<void> {
  await useNotes.getState().load();
  await useEditor.getState().reopen();
}

export async function renameFolder(path: string, newName: string): Promise<boolean> {
  let ok = false;
  await attempt("Could not rename the folder", async () => {
    await useEditor.getState().flush();
    await api().renameFolder(path, newName);
    ok = true;
    const view = ui().view;
    if (view.kind === "folder" && (view.value === path || view.value.startsWith(`${path}/`))) {
      ui().setView({ kind: "all" });
    }
    await afterFolderChange();
  });
  return ok;
}

export async function deleteFolder(path: string): Promise<void> {
  await attempt("Could not delete the folder", async () => {
    await useEditor.getState().flush();
    await api().deleteFolder(path);
    const view = ui().view;
    if (view.kind === "folder" && (view.value === path || view.value.startsWith(`${path}/`))) {
      ui().setView({ kind: "all" });
    }
    await afterFolderChange();
  });
}

export async function restoreFromTrash(trashId: string): Promise<void> {
  await attempt("Could not restore the item", async () => {
    await api().restore(trashId);
    await afterFolderChange();
  });
}

/** Changes the theme now and remembers it in the vault's settings. */
export function changeTheme(theme: Theme): void {
  ui().setTheme(theme);
  void attempt("Could not save the theme setting", () => api().setSetting("theme", theme));
}

/** Puts recovered unsaved text back into the editor (it is saved by the normal autosave). */
export async function restoreDraft(draft: Draft): Promise<void> {
  await attempt("Could not restore the draft", async () => {
    if (draft.noteMissing) {
      // The note is gone: recover the text into a new note next to where it was.
      const slash = draft.path.lastIndexOf("/");
      const folder = slash === -1 ? null : draft.path.slice(0, slash);
      const title = draft.path.slice(slash + 1).replace(/\.md$/i, "");
      await useNotes.getState().load();
      const parent = folder && useNotes.getState().folders.includes(folder) ? folder : null;
      let note;
      try {
        note = await api().createNote(parent, title);
      } catch (e) {
        if (toApiError(e).code !== "already_exists") throw e;
        note = await api().createNote(parent);
      }
      await useNotes.getState().load();
      await openNote(note.id);
      useEditor.getState().restoreContent(draft.content, true);
      await useEditor.getState().flush();
      await api().discardDraft(draft.noteId);
    } else {
      await openNote(draft.noteId);
      useEditor.getState().restoreContent(draft.content, draft.diskChanged);
    }
    useRecovery.getState().remove(draft.noteId);
  });
}

export async function discardDraft(draft: Draft): Promise<void> {
  await attempt("Could not discard the draft", async () => {
    await api().discardDraft(draft.noteId);
    useRecovery.getState().remove(draft.noteId);
  });
}
