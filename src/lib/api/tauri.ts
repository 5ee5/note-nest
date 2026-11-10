import { invoke, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toApiError } from "./errors";
import type {
  AffectedNote,
  Attachment,
  Backlink,
  BrokenLink,
  Graph,
  LinkUpdateReport,
  NoteLinks,
  NoteRef,
  RenameOutcome,
  SearchHit,
  TagCount,
  Draft,
  NoteDoc,
  NoteSummary,
  NotesApi,
  RenderedMarkdown,
  SaveMode,
  TrashItem,
  VaultChange,
  ImportReport,
  ExportReport,
} from "./types";

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
  tags?: string[];
  inline_tags?: string[];
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
    tags: [...(r.tags ?? []), ...(r.inline_tags ?? [])],
    inlineTags: r.inline_tags ?? [],
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

// ---- knowledge DTOs (snake_case from Rust) ----
interface PartDto {
  text: string;
  matched: boolean;
}
interface HitDto {
  id: string;
  title: string;
  path: string;
  archived: boolean;
  title_parts: PartDto[];
  snippet: PartDto[];
}
interface BacklinkDto {
  source: NoteRef;
  line: number;
  context: string;
  target_text: string;
  display: string | null;
}
interface OutgoingDto {
  target_text: string;
  display: string | null;
  line: number;
  context: string;
  target: NoteRef | null;
}
interface BrokenDto {
  source: NoteRef;
  target_text: string;
  line: number;
  context: string;
}
interface RelatedDto {
  note: NoteRef;
  shared_tags: number;
  linked: boolean;
}
interface NoteLinksDto {
  backlinks: BacklinkDto[];
  outgoing: OutgoingDto[];
  related: RelatedDto[];
}
interface GraphDto {
  nodes: Graph["nodes"];
  edges: [string, string][];
  total_notes: number;
}
interface ReportDto {
  updated: AffectedNote[];
  skipped: { note: NoteRef; reason: string }[];
  backup_dir: string | null;
}

const toReport = (r: ReportDto): LinkUpdateReport => ({
  updated: r.updated,
  skipped: r.skipped,
  backupDir: r.backup_dir,
});

/** Calls a Rust command, normalizing rejections into `ApiError`. */
export async function call<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  try {
    return await invoke<T>(cmd, args, options);
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

  renderMarkdown(noteId: string | null, content: string): Promise<RenderedMarkdown> {
    return call("render_markdown", { id: noteId, content });
  }

  async addAttachment(
    noteId: string,
    data: Uint8Array,
    suggestedName?: string | null,
  ): Promise<Attachment> {
    // Raw body (not a JSON array); metadata travels in headers.
    const headers: Record<string, string> = { "x-note-id": noteId };
    if (suggestedName) headers["x-file-name"] = encodeURIComponent(suggestedName);
    const r = await call<{ vault_path: string; markdown_path: string }>("add_attachment", data, {
      headers,
    });
    return { vaultPath: r.vault_path, markdownPath: r.markdown_path };
  }

  async searchNotes(query: string, limit = 50): Promise<SearchHit[]> {
    return (await call<HitDto[]>("search_notes", { query, limit })).map((h) => ({
      id: h.id,
      title: h.title,
      path: h.path,
      archived: h.archived,
      titleParts: h.title_parts,
      snippet: h.snippet,
    }));
  }

  listTags(): Promise<TagCount[]> {
    return call("list_tags");
  }

  async getNoteLinks(id: string): Promise<NoteLinks> {
    const r = await call<NoteLinksDto>("note_links", { id });
    return {
      backlinks: r.backlinks.map((b): Backlink => ({
        source: b.source,
        line: b.line,
        context: b.context,
        targetText: b.target_text,
        display: b.display,
      })),
      outgoing: r.outgoing.map((o) => ({
        targetText: o.target_text,
        display: o.display,
        line: o.line,
        context: o.context,
        target: o.target,
      })),
      related: r.related.map((x) => ({
        note: x.note,
        sharedTags: x.shared_tags,
        linked: x.linked,
      })),
    };
  }

  async getGraph(center: string | null = null, depth = 1): Promise<Graph> {
    const g = await call<GraphDto>("graph", { center, depth });
    return { nodes: g.nodes, edges: g.edges, totalNotes: g.total_notes };
  }

  listOrphans(): Promise<NoteRef[]> {
    return call("orphans");
  }

  async listBrokenLinks(): Promise<BrokenLink[]> {
    return (await call<BrokenDto[]>("broken_links")).map((b) => ({
      source: b.source,
      targetText: b.target_text,
      line: b.line,
      context: b.context,
    }));
  }

  resolveLink(sourceId: string, target: string): Promise<NoteRef | null> {
    return call("resolve_link", { source: sourceId, target });
  }

  async setTags(id: string, tags: string[]) {
    return toSummary(await call<NoteRecord>("set_tags", { id, tags }));
  }

  previewRenameNote(id: string, title: string): Promise<AffectedNote[]> {
    return call("preview_rename_note", { id, newTitle: title });
  }

  previewMoveNote(id: string, folder: string | null): Promise<AffectedNote[]> {
    return call("preview_move_note", { id, folder });
  }

  previewRenameFolder(path: string, newName: string): Promise<AffectedNote[]> {
    return call("preview_rename_folder", { path, newName });
  }

  previewMoveFolder(path: string, newParent: string | null): Promise<AffectedNote[]> {
    return call("preview_move_folder", { path, newParent });
  }

  async renameNoteWithLinks(id: string, title: string): Promise<RenameOutcome> {
    const r = await call<{ record: NoteRecord; links: ReportDto }>("rename_note_with_links", {
      id,
      newTitle: title,
    });
    return { note: toSummary(r.record), links: toReport(r.links) };
  }

  async moveNoteWithLinks(id: string, folder: string | null): Promise<RenameOutcome> {
    const r = await call<{ record: NoteRecord; links: ReportDto }>("move_note_with_links", {
      id,
      folder,
    });
    return { note: toSummary(r.record), links: toReport(r.links) };
  }

  async renameFolderWithLinks(path: string, newName: string): Promise<LinkUpdateReport> {
    return toReport(await call<ReportDto>("rename_folder_with_links", { path, newName }));
  }

  async moveFolderWithLinks(path: string, newParent: string | null): Promise<LinkUpdateReport> {
    return toReport(await call<ReportDto>("move_folder_with_links", { path, newParent }));
  }

  getSetting(key: string): Promise<string | null> {
    return call("get_setting", { key });
  }

  setSetting(key: string, value: string): Promise<void> {
    return call("set_setting", { key, value });
  }

  async importFiles(paths: string[], folder: string | null): Promise<ImportReport> {
    return toImport(await call<ImportDto>("import_files", { paths, folder }));
  }

  async importFolder(path: string, folder: string | null): Promise<ImportReport> {
    return toImport(await call<ImportDto>("import_folder", { path, folder }));
  }

  exportNote(id: string, path: string, standard: boolean): Promise<ExportReport> {
    return call("export_note", { id, path, standard });
  }

  exportVault(path: string, standard: boolean): Promise<ExportReport> {
    return call("export_vault", { path, standard });
  }

  backupVault(path: string): Promise<ExportReport> {
    return call("backup_vault", { path });
  }

  reconcile(): Promise<VaultChange> {
    return call("reconcile");
  }

  onVaultChanged(listener: (change: VaultChange) => void): () => void {
    let stopped = false;
    let unlisten: (() => void) | null = null;
    listen<VaultChange>(VAULT_CHANGED, (event) => listener(event.payload))
      .then((un) => {
        if (stopped) un();
        else unlisten = un;
      })
      .catch((e) => console.warn("cannot listen for vault changes", e));
    return () => {
      stopped = true;
      unlisten?.();
    };
  }
}

interface ImportDto {
  notes: string[];
  files: number;
  new_ids: number;
  folder: string | null;
  skipped: { path: string; reason: string }[];
}

function toImport(d: ImportDto): ImportReport {
  return {
    notes: d.notes,
    files: d.files,
    newIds: d.new_ids,
    folder: d.folder,
    skipped: d.skipped,
  };
}

/** Event the backend emits after it reconciled changes made outside the app (see commands.rs). */
export const VAULT_CHANGED = "vault-changed";

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
  /** False if outside changes are only picked up on window focus (no OS watcher). */
  watching: boolean;
}

export const vaultApi = {
  status: () => call<VaultStatus>("vault_status"),
  open: (path: string, create: boolean) => call<OpenedVault>("open_vault", { path, create }),
};
