import { toApiError } from "./api";
import { localDateString } from "./api/mock";
import type { Theme } from "./theme";
import type { EditorMode } from "../store/ui";
import type { AffectedNote, Draft, LinkUpdateReport, NoteSummary } from "./api";
import { altText, isImageFile, MAX_ATTACHMENT_BYTES } from "./images";
import { normalizeTag } from "./tags";
import { useEditor } from "../store/editor";
import { useRecovery } from "../store/recovery";
import { useNotes } from "../store/notes";
import { useUi, type LinkChoice } from "../store/ui";

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

/** Shows the "update links?" dialog and waits for the user's choice. */
function askLinkUpdate(heading: string, affected: AffectedNote[]): Promise<LinkChoice> {
  return new Promise((resolve) => {
    ui().openLinkUpdate({
      heading,
      affected,
      resolve: (choice) => {
        ui().closeLinkUpdate();
        resolve(choice);
      },
    });
  });
}

/** Tells the user what a link update did, including anything it did not touch. */
function reportLinkUpdate(report: LinkUpdateReport): void {
  const notes = report.updated.length;
  if (notes > 0) {
    const links = report.updated.reduce((n, u) => n + u.links, 0);
    const backup = report.backupDir ? ` Originals were backed up in ${report.backupDir}.` : "";
    ui().notify(
      "info",
      `Updated ${links} link${links === 1 ? "" : "s"} in ${notes} note${notes === 1 ? "" : "s"}.${backup}`,
    );
  }
  if (report.skipped.length > 0) {
    ui().notify(
      "warning",
      `Some links were not updated: ${report.skipped.map((s) => `${s.note.title} (${s.reason})`).join("; ")}`,
    );
  }
}

/**
 * Renames a note. If other notes link to it by a name that would stop working, asks
 * whether to update those links first; cancelling leaves everything as it was.
 */
export async function renameNote(id: string, title: string): Promise<boolean> {
  let affected: AffectedNote[] = [];
  const checked = await attempt("Could not rename the note", async () => {
    affected = await api().previewRenameNote(id, title);
    return true;
  });
  if (!checked) return false;
  let choice: LinkChoice = "keep";
  if (affected.length > 0) {
    choice = await askLinkUpdate(`Rename to “${title}”`, affected);
    if (choice === "cancel") return false;
  }
  let ok = false;
  await attempt("Could not rename the note", () =>
    withEditorSettled(async () => {
      let note: NoteSummary;
      if (choice === "update") {
        const outcome = await api().renameNoteWithLinks(id, title);
        note = outcome.note;
        reportLinkUpdate(outcome.links);
      } else {
        note = await api().renameNote(id, title);
      }
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
  let affected: AffectedNote[] = [];
  const checked = await attempt("Could not rename the folder", async () => {
    affected = await api().previewRenameFolder(path, newName);
    return true;
  });
  if (!checked) return false;
  let choice: LinkChoice = "keep";
  if (affected.length > 0) {
    choice = await askLinkUpdate(`Rename folder to “${newName}”`, affected);
    if (choice === "cancel") return false;
  }
  let ok = false;
  await attempt("Could not rename the folder", async () => {
    await useEditor.getState().flush();
    if (choice === "update") reportLinkUpdate(await api().renameFolderWithLinks(path, newName));
    else await api().renameFolder(path, newName);
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

/** Switches between edit, split and preview, and remembers the choice in the vault's settings. */
export function changeEditorMode(mode: EditorMode): void {
  ui().setEditorMode(mode);
  void attempt("Could not save the view mode", () => api().setSetting("editorMode", mode));
}

/**
 * Stores image files as attachments of the open note and returns the Markdown that
 * references them (`![alt](path)`, one per file, blank-line separated), or null if none
 * could be stored. Problems are reported as notices, per file, so one bad file does not
 * lose the others.
 */
export async function storeImageFiles(files: File[]): Promise<string | null> {
  const noteId = useEditor.getState().noteId;
  if (!noteId) {
    ui().notify("error", "Open a note before adding images.");
    return null;
  }
  const refs: string[] = [];
  for (const file of files) {
    const label = file.name || "the image";
    if (!isImageFile(file)) {
      ui().notify("error", `${label} is not an image, so it was not added.`);
      continue;
    }
    if (file.size > MAX_ATTACHMENT_BYTES) {
      ui().notify("error", `${label} is larger than 25 MB, so it was not added.`);
      continue;
    }
    const attachment = await attempt(`Could not attach ${label}`, async () =>
      api().addAttachment(noteId, new Uint8Array(await file.arrayBuffer()), file.name || null),
    );
    if (attachment) refs.push(`![${altText(file.name)}](${attachment.markdownPath})`);
  }
  return refs.length > 0 ? refs.join("\n\n") : null;
}

// ------------------------------------------------------------------ tags

/** Replaces the note's frontmatter tags, keeping the editor consistent. */
async function setNoteTags(id: string, tags: string[]): Promise<boolean> {
  let ok = false;
  await attempt("Could not change the tags", () =>
    withEditorSettled(async () => {
      await api().setTags(id, tags);
      ok = true;
      await reloadOpenNote(id);
    }),
  );
  return ok;
}

/** The tags stored in the note's frontmatter (inline #tags are not editable from the UI). */
function editableTags(note: NoteSummary): string[] {
  return note.tags.filter((t) => !note.inlineTags.includes(t));
}

export async function addTag(note: NoteSummary, raw: string): Promise<boolean> {
  const tag = normalizeTag(raw);
  if (!tag) {
    ui().notify(
      "error",
      `"${raw.trim()}" is not a valid tag. Use letters, numbers, - _ and / (for nesting), with at least one letter.`,
    );
    return false;
  }
  if (note.tags.includes(tag)) return true;
  return setNoteTags(note.id, [...editableTags(note), tag]);
}

export async function removeTag(note: NoteSummary, tag: string): Promise<boolean> {
  return setNoteTags(
    note.id,
    editableTags(note).filter((t) => t !== tag),
  );
}

/** Puts text in the explorer's search box and focuses it. */
export function searchFor(query: string): void {
  ui().setQuery(query);
  ui().focusSearch();
}

// ----------------------------------------------------------------- wiki links

/** Creates a note with the given title (in `folder` if it exists) and opens it. */
async function createNoteWithTitle(folder: string | null, title: string): Promise<void> {
  const parent = folder && useNotes.getState().folders.includes(folder) ? folder : null;
  const note = await attempt("Could not create the note", () => api().createNote(parent, title));
  if (!note) return;
  await useNotes.getState().load();
  await openNote(note.id);
}

/** Asks whether to create the note a broken `[[link]]` points at. */
export function offerCreateNote(target: string): void {
  const base = (target.split(/[#^]/)[0] ?? "").trim().replace(/\.md$/i, "");
  const parts = base.split("/").filter(Boolean);
  const title = parts.pop() ?? "";
  if (title === "") return;
  const sourceId = useEditor.getState().noteId;
  const source = useNotes.getState().notes.find((n) => n.id === sourceId);
  const folder = parts.length > 0 ? parts.join("/") : (source?.folder ?? "") || null;
  ui().openPrompt({
    title: "Create note",
    label: "Note title",
    initial: title,
    confirmLabel: "Create",
    onSubmit: (t) => createNoteWithTitle(folder, t),
  });
}

/** Opens the note a `[[link]]` in the open note points at, or offers to create it. */
export async function followLink(target: string): Promise<void> {
  const sourceId = useEditor.getState().noteId;
  if (!sourceId) return;
  const hit = await attempt("Could not follow the link", () => api().resolveLink(sourceId, target));
  if (hit === undefined) return;
  if (hit) await openNote(hit.id);
  else offerCreateNote(target);
}

/** A click on a link inside the rendered preview (`#nn-note=<id>` / `#nn-missing`). Returns whether it was handled. */
export function followPreviewLink(href: string, title: string | null): boolean {
  const id = /^#nn-note=([0-9a-f-]{36})$/i.exec(href)?.[1];
  if (id) {
    void openNote(id);
    return true;
  }
  if (href === "#nn-missing") {
    offerCreateNote((title ?? "").replace(/^Note not found: /, ""));
    return true;
  }
  return false;
}
