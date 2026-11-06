/**
 * Application interface between the UI and the notes backend.
 *
 * Two implementations exist: `TauriNotesApi` (the real vault on disk) and
 * `MockNotesApi` (in memory, used in the browser and in tests). Components only
 * depend on this interface.
 */

export interface NoteSummary {
  id: string;
  title: string;
  /** Path relative to the vault root, e.g. `Programming/Rust.md`. */
  path: string;
  /** Folder path relative to the vault root; empty string for the vault root. */
  folder: string;
  tags: string[];
  pinned: boolean;
  archived: boolean;
  preview: string;
  createdAt: string; // ISO 8601
  updatedAt: string; // ISO 8601
  lastOpenedAt: string | null; // ISO 8601
}

/** A note together with its full file content. */
export interface NoteDoc {
  note: NoteSummary;
  /** Entire file, including any frontmatter. */
  content: string;
  /** Hash of `content` as stored on disk; pass back as `baseHash` when saving. */
  contentHash: string;
}

export type SaveMode = { baseHash: string } | { overwrite: true };

export interface TrashItem {
  id: string;
  originalPath: string;
  trashedAt: string;
  isDir: boolean;
}

/** Unsaved editor text recovered after an unexpected exit. */
export interface Draft {
  noteId: string;
  path: string;
  baseHash: string;
  content: string;
  stashedAt: string;
  /** The note no longer exists at `path`. */
  noteMissing: boolean;
  /** The file changed on disk after the draft was based on it. */
  diskChanged: boolean;
}

export interface NotesApi {
  /** True for sample data that is never written to disk. */
  readonly isSample: boolean;

  listNotes(): Promise<NoteSummary[]>;
  listFolders(): Promise<string[]>;

  readNote(id: string): Promise<NoteDoc>;
  /** Rejects with code `conflict` if the file changed since `baseHash`. */
  saveNote(id: string, content: string, mode: SaveMode): Promise<NoteDoc>;
  createNote(folder?: string | null, title?: string | null): Promise<NoteSummary>;
  renameNote(id: string, title: string): Promise<NoteSummary>;
  moveNote(id: string, folder: string | null): Promise<NoteSummary>;
  duplicateNote(id: string): Promise<NoteSummary>;
  setPinned(id: string, pinned: boolean): Promise<NoteSummary>;
  setArchived(id: string, archived: boolean): Promise<NoteSummary>;
  deleteNote(id: string): Promise<TrashItem>;
  markOpened(id: string): Promise<void>;

  createFolder(path: string): Promise<void>;
  renameFolder(path: string, newName: string): Promise<void>;
  moveFolder(path: string, newParent: string | null): Promise<void>;
  deleteFolder(path: string): Promise<TrashItem>;

  listTrash(): Promise<TrashItem[]>;
  restore(trashId: string): Promise<string>;

  stashDraft(id: string, content: string, baseHash: string): Promise<void>;
  listDrafts(): Promise<Draft[]>;
  discardDraft(id: string): Promise<void>;

  getSetting(key: string): Promise<string | null>;
  setSetting(key: string, value: string): Promise<void>;
}
