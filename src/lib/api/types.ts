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
  /** All tags: the frontmatter `tags:` list first, then tags that appear only inline (#tag). */
  tags: string[];
  /** The subset of `tags` that exist only as #tag in the text (not removable from the UI). */
  inlineTags: string[];
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

/** Sanitized preview HTML plus images that could not be shown. */
export interface RenderedMarkdown {
  html: string;
  warnings: string[];
}

/** A stored image attachment. */
export interface Attachment {
  /** Path relative to the vault root. */
  vaultPath: string;
  /** Path relative to the note that references it, ready for `![](...)`. */
  markdownPath: string;
}

/** A fragment of text; `matched` parts are what the search found. */
export interface SnippetPart {
  text: string;
  matched: boolean;
}

export interface SearchHit {
  id: string;
  title: string;
  path: string;
  archived: boolean;
  titleParts: SnippetPart[];
  snippet: SnippetPart[];
}

/** A tag with the number of notes using it (nested tags roll up into their parents). */
export interface TagCount {
  name: string;
  count: number;
}

export interface NoteRef {
  id: string;
  title: string;
  path: string;
}

/** A link in another note that points at this one. */
export interface Backlink {
  source: NoteRef;
  line: number;
  context: string;
  targetText: string;
  display: string | null;
}

/** A link written in a note; `target` is null when it points at nothing. */
export interface OutgoingLink {
  targetText: string;
  display: string | null;
  line: number;
  context: string;
  target: NoteRef | null;
}

export interface BrokenLink {
  source: NoteRef;
  targetText: string;
  line: number;
  context: string;
}

export interface RelatedNote {
  note: NoteRef;
  sharedTags: number;
  linked: boolean;
}

export interface NoteLinks {
  backlinks: Backlink[];
  outgoing: OutgoingLink[];
  related: RelatedNote[];
}

export interface GraphNode {
  id: string;
  title: string;
  path: string;
  archived: boolean;
  /** Number of distinct notes this one is linked to or from. */
  degree: number;
}

export interface Graph {
  nodes: GraphNode[];
  /** Directed [from, to] note ids. */
  edges: [string, string][];
  /** Notes in the whole vault (a local graph shows only some). */
  totalNotes: number;
}

/** A note whose links would be (or were) rewritten by a rename or move. */
export interface AffectedNote {
  note: NoteRef;
  links: number;
}

export interface LinkUpdateReport {
  updated: AffectedNote[];
  skipped: { note: NoteRef; reason: string }[];
  /** Where the originals were copied before being changed (relative to the vault). */
  backupDir: string | null;
}

export interface RenameOutcome {
  note: NoteSummary;
  links: LinkUpdateReport;
}

/** What changed in the vault outside NoteNest (another editor, the file manager, sync tools). */
export interface VaultChange {
  /** Notes added or modified, including ones moved or renamed together with their id. */
  changed: string[];
  /** Notes that no longer exist under their id (deleted, or renamed without an id). */
  removed: string[];
  /** Folders were added, renamed or removed. */
  folders: boolean;
  /** Files other than notes (such as attachments) changed. */
  files: boolean;
}

/** Something that was not imported or exported, and why. */
export interface SkippedItem {
  path: string;
  reason: string;
}

export interface ImportReport {
  /** Notes created, as vault paths. */
  notes: string[];
  /** Other files copied into the vault (attachments). */
  files: number;
  /** Imported notes given a new id because theirs was already used in the vault. */
  newIds: number;
  /** For a folder import: the folder created in the vault. */
  folder: string | null;
  skipped: SkippedItem[];
}

export interface ExportReport {
  /** The file or folder written. */
  destination: string;
  notes: number;
  files: number;
  skipped: SkippedItem[];
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

  /** Renders Markdown to sanitized HTML. `noteId` anchors relative image paths. */
  renderMarkdown(noteId: string | null, content: string): Promise<RenderedMarkdown>;
  /** Stores an image; the content decides the file type, `suggestedName` only the stem. */
  addAttachment(
    noteId: string,
    data: Uint8Array,
    suggestedName?: string | null,
  ): Promise<Attachment>;

  searchNotes(query: string, limit?: number): Promise<SearchHit[]>;
  listTags(): Promise<TagCount[]>;
  getNoteLinks(id: string): Promise<NoteLinks>;
  getGraph(center?: string | null, depth?: number): Promise<Graph>;
  listOrphans(): Promise<NoteRef[]>;
  listBrokenLinks(): Promise<BrokenLink[]>;
  /** Which note a `[[target]]` written in note `sourceId` points at, if any. */
  resolveLink(sourceId: string, target: string): Promise<NoteRef | null>;
  /** Replaces the frontmatter tags list; inline #tags in the text are untouched. */
  setTags(id: string, tags: string[]): Promise<NoteSummary>;

  /** Notes whose links would be rewritten by the operation (empty = nothing to confirm). */
  previewRenameNote(id: string, title: string): Promise<AffectedNote[]>;
  previewMoveNote(id: string, folder: string | null): Promise<AffectedNote[]>;
  previewRenameFolder(path: string, newName: string): Promise<AffectedNote[]>;
  previewMoveFolder(path: string, newParent: string | null): Promise<AffectedNote[]>;
  /** Like renameNote/moveNote/renameFolder/moveFolder, and also fixes links that stopped resolving. */
  renameNoteWithLinks(id: string, title: string): Promise<RenameOutcome>;
  moveNoteWithLinks(id: string, folder: string | null): Promise<RenameOutcome>;
  renameFolderWithLinks(path: string, newName: string): Promise<LinkUpdateReport>;
  moveFolderWithLinks(path: string, newParent: string | null): Promise<LinkUpdateReport>;

  getSetting(key: string): Promise<string | null>;
  setSetting(key: string, value: string): Promise<void>;

  /**
   * Copies Markdown files (absolute paths outside the vault) into vault folder `folder`
   * (the root if null), with the files they reference. Never overwrites anything.
   */
  importFiles(paths: string[], folder: string | null): Promise<ImportReport>;
  /** Copies a folder tree into vault folder `folder` as a new folder. */
  importFolder(path: string, folder: string | null): Promise<ImportReport>;
  /**
   * Writes a note to `path` (outside the vault) with its attachments. `standard` removes
   * NoteNest's frontmatter keys and turns wiki links into plain text.
   */
  exportNote(id: string, path: string, standard: boolean): Promise<ExportReport>;
  /**
   * Copies the vault into a new folder inside `path`. `standard` removes NoteNest's
   * frontmatter keys and turns wiki links into relative Markdown links.
   */
  exportVault(path: string, standard: boolean): Promise<ExportReport>;

  /** Brings the index in line with the files now and resolves to what changed outside the app. */
  reconcile(): Promise<VaultChange>;
  /**
   * Calls `listener` whenever the backend notices that files changed outside the app
   * (the index is already updated by then). Returns a function that stops listening.
   */
  onVaultChanged(listener: (change: VaultChange) => void): () => void;
}
