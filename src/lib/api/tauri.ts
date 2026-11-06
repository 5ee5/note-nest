import { invoke } from "@tauri-apps/api/core";
import { toApiError } from "./errors";
import type { Draft, NoteDoc, NoteSummary, NotesApi, SaveMode, TrashItem } from "./types";

// Shapes returned by the Rust commands (snake_case, see crates/notenest-core).
interface NoteRecord {
  id: string;
  path: string;
  title: string;
  created_at: string;
  updated_at: string;
  content_hash: string;
  pinned: boolean;
  archived: boolean;
  last_opened_at: string | null;
}
interface NoteContent {
  record: NoteRecord;
  content: string;
}
interface TrashEntry {
  id: string;
  original_path: string;
  trashed_at: string;
  is_dir: boolean;
}
interface DraftDto {
  note_id: string;
  path: string;
  base_hash: string;
  content: string;
  stashed_at: string;
  note_missing: boolean;
  disk_changed: boolean;
}

export function toSummary(r: NoteRecord): NoteSummary {
  const slash = r.path.lastIndexOf("/");
  return {
    id: r.id,
    title: r.title,
    path: r.path,
    folder: slash === -1 ? "" : r.path.slice(0, slash),
    tags: [], // tags arrive with the knowledge-management milestone
    pinned: r.pinned,
    archived: r.archived,
    preview: "", // previews arrive with search
    createdAt: r.created_at,
    updatedAt: r.updated_at,
    lastOpenedAt: r.last_opened_at,
  };
}

function toDoc(c: NoteContent): NoteDoc {
  return { note: toSummary(c.record), content: c.content, contentHash: c.record.content_hash };
}

function toTrash(t: TrashEntry): TrashItem {
  return { id: t.id, originalPath: t.original_path, trashedAt: t.trashed_at, isDir: t.is_dir };
}

function toDraft(d: DraftDto): Draft {
  return {
    noteId: d.note_id,
    path: d.path,
    baseHash: d.base_hash,
    content: d.content,
    stashedAt: d.stashed_at,
    noteMissing: d.note_missing,
    diskChanged: d.disk_changed,
  };
}

/** Calls a Rust command, normalizing rejections into `ApiError`. */
export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toApiError(e);
  }
}

/** The real vault on disk, through the Tauri commands. */
export class TauriNotesApi implements NotesApi {
  readonly isSample = false;

  async listNotes(): Promise<NoteSummary[]> {
    return (await call<NoteRecord[]>("list_notes")).map(toSummary);
  }

  listFolders(): Promise<string[]> {
    return call("list_folders");
  }

  async readNote(id: string): Promise<NoteDoc> {
    return toDoc(await call<NoteContent>("read_note", { id }));
  }

  async saveNote(id: string, content: string, mode: SaveMode): Promise<NoteDoc> {
    const args =
      "overwrite" in mode
        ? { id, content, baseHash: null, overwrite: true }
        : { id, content, baseHash: mode.baseHash, overwrite: false };
    return toDoc(await call<NoteContent>("save_note", args));
  }

  async createNote(folder: string | null = null, title: string | null = null) {
    return toSummary(await call<NoteRecord>("create_note", { folder, title }));
  }

  async renameNote(id: string, title: string) {
    return toSummary(await call<NoteRecord>("rename_note", { id, newTitle: title }));
  }

  async moveNote(id: string, folder: string | null) {
    return toSummary(await call<NoteRecord>("move_note", { id, folder }));
  }

  async duplicateNote(id: string) {
    return toSummary(await call<NoteRecord>("duplicate_note", { id }));
  }

  async setPinned(id: string, pinned: boolean) {
    return toSummary(await call<NoteRecord>("set_pinned", { id, pinned }));
  }

  async setArchived(id: string, archived: boolean) {
    return toSummary(await call<NoteRecord>("set_archived", { id, archived }));
  }

  async deleteNote(id: string) {
    return toTrash(await call<TrashEntry>("delete_note", { id }));
  }

  markOpened(id: string): Promise<void> {
    return call("mark_opened", { id });
  }

  createFolder(path: string): Promise<void> {
    return call("create_folder", { path });
  }

  renameFolder(path: string, newName: string): Promise<void> {
    return call("rename_folder", { path, newName });
  }

  moveFolder(path: string, newParent: string | null): Promise<void> {
    return call("move_folder", { path, newParent });
  }

  async deleteFolder(path: string) {
    return toTrash(await call<TrashEntry>("delete_folder", { path }));
  }

  async listTrash(): Promise<TrashItem[]> {
    return (await call<TrashEntry[]>("list_trash")).map(toTrash);
  }

  restore(trashId: string): Promise<string> {
    return call("restore", { trashId });
  }

  stashDraft(id: string, content: string, baseHash: string): Promise<void> {
    return call("stash_draft", { id, content, baseHash });
  }

  async listDrafts(): Promise<Draft[]> {
    return (await call<DraftDto[]>("list_drafts")).map(toDraft);
  }

  discardDraft(id: string): Promise<void> {
    return call("discard_draft", { id });
  }

  getSetting(key: string): Promise<string | null> {
    return call("get_setting", { key });
  }

  setSetting(key: string, value: string): Promise<void> {
    return call("set_setting", { key, value });
  }
}

// ---- vault lifecycle (not part of NotesApi: it happens before a vault exists) ----

export interface VaultStatus {
  configured_path: string | null;
  default_path: string | null;
  open: boolean;
}

export interface OpenedVault {
  path: string;
  index_rebuilt: boolean;
  index_backup: string | null;
  notes: number;
  unreadable: string[];
  skipped_invalid: string[];
  duplicate_ids_fixed: string[];
}

export const vaultApi = {
  status: () => call<VaultStatus>("vault_status"),
  open: (path: string, create: boolean) => call<OpenedVault>("open_vault", { path, create }),
};
