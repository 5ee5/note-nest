/**
 * Application interface between the UI and the notes backend.
 *
 * Milestone 1 ships an in-memory mock. Milestone 2 replaces it with a
 * Tauri-backed implementation; components only depend on this interface.
 */

export interface NoteSummary {
  id: string;
  title: string;
  /** Path relative to the vault root, e.g. `Programming/Rust.md`. */
  path: string;
  /** Folder path relative to the vault root; empty string for the vault root. */
  folder: string;
  tags: string[];
  favorite: boolean;
  preview: string;
  createdAt: string; // ISO 8601
  updatedAt: string; // ISO 8601
  lastOpenedAt: string | null; // ISO 8601
}

export interface NotesApi {
  listNotes(): Promise<NoteSummary[]>;
  listFolders(): Promise<string[]>;
  createNote(): Promise<NoteSummary>;
  /** Returns today's daily note, creating it if needed. */
  openDailyNote(): Promise<NoteSummary>;
  markOpened(id: string): Promise<void>;
}
