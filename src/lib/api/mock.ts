import { ApiError } from "./errors";
import {
  affected,
  applyEdits,
  brokenLinks,
  graph as buildGraph,
  linkKey,
  MockResolver,
  noteLinks,
  orphans as findOrphans,
  planLinkUpdates,
  search as searchIndex,
  setFrontmatterTags,
  tagCounts,
  tagsOf,
  type KNote,
  type PlannedEdit,
} from "./mockKnowledge";
import type {
  AffectedNote,
  Attachment,
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
} from "./types";

export function localDateString(date = new Date()): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** Cheap non-cryptographic hash; only the mock uses it. */
export function mockHash(text: string): string {
  let h = 5381;
  for (let i = 0; i < text.length; i++) h = ((h << 5) + h + text.charCodeAt(i)) | 0;
  return (h >>> 0).toString(16).padStart(8, "0");
}

const iso = (daysAgo: number, hour = 10): string => {
  const d = new Date();
  d.setDate(d.getDate() - daysAgo);
  d.setHours(hour, 0, 0, 0);
  return d.toISOString();
};

interface Stored {
  note: NoteSummary;
  content: string;
  /** Tags that exist regardless of the content (sample data). */
  seedTags: string[];
}

interface Trashed {
  item: TrashItem;
  /** Notes (and their contents) that were removed together with the item. */
  notes: Stored[];
  folders: string[];
}

function parts(path: string): { folder: string; title: string } {
  const i = path.lastIndexOf("/");
  const name = i === -1 ? path : path.slice(i + 1);
  return { folder: i === -1 ? "" : path.slice(0, i), title: name.replace(/\.md$/i, "") };
}

function validName(name: string): void {
  if (name.trim() === "" || /[/\\:*?"<>|]/.test(name) || name.startsWith(".")) {
    throw new ApiError("invalid_path", `"${name}" is not a valid name`);
  }
}

/** Extension for the image types the app accepts, decided from the leading bytes. */
function sniffImage(b: Uint8Array): string | null {
  const starts = (...bytes: number[]) => bytes.every((v, i) => b[i] === v);
  if (starts(0x89, 0x50, 0x4e, 0x47)) return "png";
  if (starts(0xff, 0xd8, 0xff)) return "jpg";
  if (starts(0x47, 0x49, 0x46, 0x38)) return "gif";
  if (starts(0x52, 0x49, 0x46, 0x46) && b[8] === 0x57 && b[9] === 0x45) return "webp";
  const head = new TextDecoder().decode(b.slice(0, 256)).trimStart().toLowerCase();
  return head.startsWith("<svg") || (head.startsWith("<?xml") && head.includes("<svg"))
    ? "svg"
    : null;
}

function previewOf(content: string): string {
  const body = content.replace(/^---\n[\s\S]*?\n---\n/, "");
  return body.replace(/\s+/g, " ").trim().slice(0, 120);
}

/** In-memory stand-in for the real backend. Nothing is written to disk. */
export class MockNotesApi implements NotesApi {
  readonly isSample = true;
  private notes = new Map<string, Stored>();
  private folders = new Set<string>();
  private trash: Trashed[] = [];
  private drafts = new Map<string, Draft>();
  private settings = new Map<string, string>();
  private counter = 0;
  private attachments = 0;

  constructor(options: { sample?: boolean } = {}) {
    if (options.sample ?? true) this.seed();
  }

  private nextId(): string {
    this.counter += 1;
    return `mock-${this.counter}`;
  }

  private add(path: string, content: string, over: Partial<NoteSummary> = {}): NoteSummary {
    const { folder, title } = parts(path);
    this.ensureFolder(folder);
    const now = new Date().toISOString();
    const note: NoteSummary = {
      id: this.nextId(),
      title,
      path,
      folder,
      tags: [],
      inlineTags: [],
      pinned: false,
      archived: false,
      preview: previewOf(content),
      createdAt: now,
      updatedAt: now,
      lastOpenedAt: null,
      ...over,
    };
    const stored: Stored = { note, content, seedTags: over.tags ?? [] };
    this.notes.set(note.id, stored);
    this.refreshTags(stored);
    return stored.note;
  }

  private ensureFolder(folder: string): void {
    if (folder === "") return;
    const segs = folder.split("/");
    for (let i = 1; i <= segs.length; i++) this.folders.add(segs.slice(0, i).join("/"));
  }

  private seed(): void {
    this.add("Inbox/ideas.md", "Things to look into later: a Markdown linter, a tiny CLI…", {
      updatedAt: iso(0),
    });
    this.add("Programming/Rust.md", "# Rust\n\nOwnership, borrowing and lifetimes…", {
      pinned: true,
      tags: ["programming/rust"],
      updatedAt: iso(1),
      lastOpenedAt: iso(0, 9),
    });
    this.add("Programming/Linux.md", "Useful commands and config notes.", {
      tags: ["programming/linux"],
      updatedAt: iso(3),
    });
    this.add("Projects/CodeBuddy.md", "Project goals and open questions.", {
      tags: ["research/security"],
      updatedAt: iso(5),
      lastOpenedAt: iso(1, 15),
    });
    this.add(`Journal/${localDateString()}.md`, "Today's plan and reflections.", {
      tags: ["personal/journal"],
      updatedAt: iso(0),
    });
    this.add(
      "Welcome.md",
      "---\nid: 6e6f7465-6e65-4737-8000-000000000001\ntags: [sample]\n---\n\nThis is sample data. Nothing you type here is saved to disk.\n",
      {
        pinned: true,
        updatedAt: iso(20),
      },
    );
  }

  private get(id: string): Stored {
    const s = this.notes.get(id);
    if (!s) throw new ApiError("not_found", `note ${id} not found`);
    return s;
  }

  private pathTaken(path: string, ignoreId?: string): boolean {
    return [...this.notes.values()].some((s) => s.note.path === path && s.note.id !== ignoreId);
  }

  /** Test hook: change a note's content behind the app's back, like an external editor. */
  externalEdit(id: string, content: string): void {
    this.get(id).content = content;
  }

  /** Test hook: current stored content. */
  contentOf(id: string): string {
    return this.get(id).content;
  }

  async listNotes(): Promise<NoteSummary[]> {
    return Promise.resolve([...this.notes.values()].map((s) => ({ ...s.note })));
  }

  async listFolders(): Promise<string[]> {
    return Promise.resolve([...this.folders].sort());
  }

  async readNote(id: string): Promise<NoteDoc> {
    const s = this.get(id);
    return Promise.resolve({
      note: { ...s.note },
      content: s.content,
      contentHash: mockHash(s.content),
    });
  }

  async saveNote(id: string, content: string, mode: SaveMode): Promise<NoteDoc> {
    const s = this.get(id);
    if ("baseHash" in mode && mode.baseHash !== mockHash(s.content)) {
      return Promise.reject(
        new ApiError("conflict", `${s.note.path} changed on disk since it was loaded`),
      );
    }
    s.content = content;
    s.note = {
      ...s.note,
      preview: previewOf(content),
      updatedAt: new Date().toISOString(),
    };
    this.refreshTags(s);
    return Promise.resolve({ note: { ...s.note }, content, contentHash: mockHash(content) });
  }

  async createNote(
    folder: string | null = null,
    title: string | null = null,
  ): Promise<NoteSummary> {
    if (folder && !this.folders.has(folder)) {
      return Promise.reject(new ApiError("not_found", `folder ${folder} not found`));
    }
    const dir = folder ? `${folder}/` : "";
    let name = title;
    if (name === null) {
      name = "Untitled";
      for (let n = 2; this.pathTaken(`${dir}${name}.md`); n++) name = `Untitled ${n}`;
    } else {
      validName(name);
      if (this.pathTaken(`${dir}${name}.md`)) {
        return Promise.reject(new ApiError("already_exists", `${dir}${name}.md already exists`));
      }
    }
    return Promise.resolve({ ...this.add(`${dir}${name}.md`, "") });
  }

  private relocate(id: string, path: string): NoteSummary {
    const s = this.get(id);
    if (this.pathTaken(path, id)) throw new ApiError("already_exists", `${path} already exists`);
    const { folder, title } = parts(path);
    this.ensureFolder(folder);
    s.note = { ...s.note, path, folder, title };
    return { ...s.note };
  }

  async renameNote(id: string, title: string): Promise<NoteSummary> {
    try {
      validName(title);
      const { folder } = this.get(id).note;
      return Promise.resolve(this.relocate(id, `${folder ? `${folder}/` : ""}${title}.md`));
    } catch (e) {
      return Promise.reject(e);
    }
  }

  async moveNote(id: string, folder: string | null): Promise<NoteSummary> {
    try {
      if (folder && !this.folders.has(folder)) throw new ApiError("not_found", `folder ${folder}`);
      const name = `${this.get(id).note.title}.md`;
      return Promise.resolve(this.relocate(id, `${folder ? `${folder}/` : ""}${name}`));
    } catch (e) {
      return Promise.reject(e);
    }
  }

  async duplicateNote(id: string): Promise<NoteSummary> {
    const s = this.get(id);
    const dir = s.note.folder ? `${s.note.folder}/` : "";
    let name = `${s.note.title} copy`;
    for (let n = 2; this.pathTaken(`${dir}${name}.md`); n++) name = `${s.note.title} copy ${n}`;
    return Promise.resolve({ ...this.add(`${dir}${name}.md`, s.content) });
  }

  async setPinned(id: string, pinned: boolean): Promise<NoteSummary> {
    const s = this.get(id);
    s.note = { ...s.note, pinned };
    return Promise.resolve({ ...s.note });
  }

  async setArchived(id: string, archived: boolean): Promise<NoteSummary> {
    const s = this.get(id);
    s.note = { ...s.note, archived };
    return Promise.resolve({ ...s.note });
  }

  async deleteNote(id: string): Promise<TrashItem> {
    const s = this.get(id);
    this.notes.delete(id);
    const item: TrashItem = {
      id: this.nextId(),
      originalPath: s.note.path,
      trashedAt: new Date().toISOString(),
      isDir: false,
    };
    this.trash.push({ item, notes: [s], folders: [] });
    return Promise.resolve(item);
  }

  async markOpened(id: string): Promise<void> {
    const s = this.get(id);
    s.note = { ...s.note, lastOpenedAt: new Date().toISOString() };
    return Promise.resolve();
  }

  async createFolder(path: string): Promise<void> {
    path.split("/").forEach(validName);
    if (this.folders.has(path)) {
      return Promise.reject(new ApiError("already_exists", `${path} already exists`));
    }
    this.ensureFolder(path);
    return Promise.resolve();
  }

  private moveFolderTo(from: string, to: string): void {
    if (this.folders.has(to)) throw new ApiError("already_exists", `${to} already exists`);
    if (to === from || to.startsWith(`${from}/`)) {
      throw new ApiError("invalid_path", `cannot move ${from} into itself`);
    }
    const rewrite = (p: string) => (p === from ? to : `${to}${p.slice(from.length)}`);
    const under = (p: string) => p === from || p.startsWith(`${from}/`);
    this.folders = new Set([...this.folders].map((f) => (under(f) ? rewrite(f) : f)));
    for (const s of this.notes.values()) {
      if (under(s.note.path)) {
        const path = rewrite(s.note.path);
        s.note = { ...s.note, path, folder: parts(path).folder };
      }
    }
  }

  async renameFolder(path: string, newName: string): Promise<void> {
    try {
      validName(newName);
      const { folder } = parts(path);
      this.moveFolderTo(path, `${folder ? `${folder}/` : ""}${newName}`);
      return Promise.resolve();
    } catch (e) {
      return Promise.reject(e);
    }
  }

  async moveFolder(path: string, newParent: string | null): Promise<void> {
    try {
      const name = path.slice(path.lastIndexOf("/") + 1);
      this.moveFolderTo(path, `${newParent ? `${newParent}/` : ""}${name}`);
      return Promise.resolve();
    } catch (e) {
      return Promise.reject(e);
    }
  }

  async deleteFolder(path: string): Promise<TrashItem> {
    if (!this.folders.has(path)) {
      return Promise.reject(new ApiError("not_found", `folder ${path} not found`));
    }
    const under = (p: string) => p === path || p.startsWith(`${path}/`);
    const notes = [...this.notes.values()].filter((s) => under(s.note.path));
    const folders = [...this.folders].filter(under);
    notes.forEach((s) => this.notes.delete(s.note.id));
    folders.forEach((f) => this.folders.delete(f));
    const item: TrashItem = {
      id: this.nextId(),
      originalPath: path,
      trashedAt: new Date().toISOString(),
      isDir: true,
    };
    this.trash.push({ item, notes, folders });
    return Promise.resolve(item);
  }

  async listTrash(): Promise<TrashItem[]> {
    return Promise.resolve(this.trash.map((t) => ({ ...t.item })).reverse());
  }

  async restore(trashId: string): Promise<string> {
    const i = this.trash.findIndex((t) => t.item.id === trashId);
    if (i === -1) return Promise.reject(new ApiError("not_found", `trash entry ${trashId}`));
    const t = this.trash[i]!;
    const taken = t.item.isDir
      ? this.folders.has(t.item.originalPath)
      : this.pathTaken(t.item.originalPath);
    if (taken) {
      return Promise.reject(
        new ApiError("already_exists", `${t.item.originalPath} already exists`),
      );
    }
    t.folders.forEach((f) => this.folders.add(f));
    t.notes.forEach((s) => {
      this.ensureFolder(s.note.folder);
      this.notes.set(s.note.id, s);
    });
    this.trash.splice(i, 1);
    return Promise.resolve(t.item.originalPath);
  }

  async stashDraft(id: string, content: string, baseHash: string): Promise<void> {
    const s = this.get(id);
    this.drafts.set(id, {
      noteId: id,
      path: s.note.path,
      baseHash,
      content,
      stashedAt: new Date().toISOString(),
      noteMissing: false,
      diskChanged: false,
    });
    return Promise.resolve();
  }

  async listDrafts(): Promise<Draft[]> {
    const out: Draft[] = [];
    for (const d of this.drafts.values()) {
      const s = this.notes.get(d.noteId);
      if (s && s.content === d.content) continue;
      out.push({
        ...d,
        path: s ? s.note.path : d.path,
        noteMissing: !s,
        diskChanged: s ? mockHash(s.content) !== d.baseHash : false,
      });
    }
    return Promise.resolve(out);
  }

  async discardDraft(id: string): Promise<void> {
    this.drafts.delete(id);
    return Promise.resolve();
  }

  /** The mock has no Markdown engine: it shows the text escaped, which is always safe. */
  async renderMarkdown(_noteId: string | null, content: string): Promise<RenderedMarkdown> {
    const escaped = content.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    return { html: `<pre>${escaped}</pre>`, warnings: [] };
  }

  async addAttachment(
    noteId: string,
    data: Uint8Array,
    suggestedName?: string | null,
  ): Promise<Attachment> {
    const note = this.get(noteId).note;
    const kind = sniffImage(data);
    if (!kind) {
      throw new ApiError(
        "unsupported_file",
        "only PNG, JPEG, GIF, WebP and SVG images can be attached",
      );
    }
    this.attachments += 1;
    const stem =
      (suggestedName ?? "")
        .replace(/\.[^.]*$/, "")
        .replace(/[^A-Za-z0-9_]+/g, "-")
        .replace(/^-+|-+$/g, "") || "image";
    const name = `${stem}-${String(this.attachments).padStart(3, "0")}.${kind}`;
    const depth = note.path.split("/").length - 1;
    return {
      vaultPath: `Attachments/${name}`,
      markdownPath: `${"../".repeat(depth)}Attachments/${name}`,
    };
  }

  // ------------------------------------------------------------- knowledge

  /** Recomputes a note's tags from its content (and sample tags). */
  private refreshTags(s: Stored): void {
    const { tags, inline } = tagsOf(this.knote(s));
    s.note = { ...s.note, tags, inlineTags: inline };
  }

  private knote(s: Stored): KNote {
    return {
      id: s.note.id,
      path: s.note.path,
      title: s.note.title,
      content: s.content,
      archived: s.note.archived,
      seedTags: s.seedTags,
    };
  }

  private knowledge(): KNote[] {
    return [...this.notes.values()].map((s) => this.knote(s));
  }

  async searchNotes(query: string, limit = 50): Promise<SearchHit[]> {
    return searchIndex(this.knowledge(), query, limit);
  }

  async listTags(): Promise<TagCount[]> {
    return tagCounts(this.knowledge());
  }

  async getNoteLinks(id: string): Promise<NoteLinks> {
    return noteLinks(this.knowledge(), id);
  }

  async getGraph(center: string | null = null, depth = 1): Promise<Graph> {
    return buildGraph(this.knowledge(), center, depth);
  }

  async listOrphans(): Promise<NoteRef[]> {
    return findOrphans(this.knowledge());
  }

  async listBrokenLinks(): Promise<BrokenLink[]> {
    return brokenLinks(this.knowledge());
  }

  async resolveLink(sourceId: string, target: string): Promise<NoteRef | null> {
    const notes = this.knowledge();
    const source = notes.find((n) => n.id === sourceId);
    if (!source) return null;
    const hit = new MockResolver(notes).resolve(source.path, linkKey(target));
    return hit ? { id: hit.id, title: hit.title, path: hit.path } : null;
  }

  async setTags(id: string, tags: string[]): Promise<NoteSummary> {
    const s = this.get(id);
    s.seedTags = [];
    s.content = setFrontmatterTags(s.content, tags);
    s.note = { ...s.note, updatedAt: new Date().toISOString() };
    this.refreshTags(s);
    return { ...s.note };
  }

  private noteMoves(id: string, newPath: string) {
    return new Map([[id, { path: newPath, title: parts(newPath).title }]]);
  }

  private folderMoves(from: string, to: string) {
    const moves = new Map<string, { path: string; title: string }>();
    for (const s of this.notes.values()) {
      const p = s.note.path;
      if (p.startsWith(`${from}/`)) {
        const path = `${to}${p.slice(from.length)}`;
        moves.set(s.note.id, { path, title: parts(path).title });
      }
    }
    return moves;
  }

  private newNotePath(id: string, title: string): string {
    validName(title);
    const { folder } = this.get(id).note;
    return `${folder ? `${folder}/` : ""}${title}.md`;
  }

  private movedNotePath(id: string, folder: string | null): string {
    if (folder && !this.folders.has(folder)) throw new ApiError("not_found", `folder ${folder}`);
    return `${folder ? `${folder}/` : ""}${this.get(id).note.title}.md`;
  }

  private affectedBy(moves: Map<string, { path: string; title: string }>): AffectedNote[] {
    const notes = this.knowledge();
    return affected(notes, planLinkUpdates(notes, moves));
  }

  async previewRenameNote(id: string, title: string): Promise<AffectedNote[]> {
    return this.affectedBy(this.noteMoves(id, this.newNotePath(id, title)));
  }

  async previewMoveNote(id: string, folder: string | null): Promise<AffectedNote[]> {
    return this.affectedBy(this.noteMoves(id, this.movedNotePath(id, folder)));
  }

  async previewRenameFolder(path: string, newName: string): Promise<AffectedNote[]> {
    validName(newName);
    const { folder } = parts(path);
    return this.affectedBy(this.folderMoves(path, `${folder ? `${folder}/` : ""}${newName}`));
  }

  async previewMoveFolder(path: string, newParent: string | null): Promise<AffectedNote[]> {
    const name = path.slice(path.lastIndexOf("/") + 1);
    return this.affectedBy(this.folderMoves(path, `${newParent ? `${newParent}/` : ""}${name}`));
  }

  /** Applies planned link edits to the notes' contents and reports what happened. */
  private applyPlan(plan: Map<string, PlannedEdit[]>): LinkUpdateReport {
    const report: LinkUpdateReport = { updated: [], skipped: [], backupDir: null };
    for (const [id, edits] of plan) {
      const s = this.notes.get(id);
      if (!s) continue;
      const { text, applied } = applyEdits(s.content, edits);
      const note = { id, title: s.note.title, path: s.note.path };
      if (applied > 0) {
        s.content = text;
        s.note = { ...s.note, preview: previewOf(text), updatedAt: new Date().toISOString() };
        this.refreshTags(s);
        report.updated.push({ note, links: applied });
        report.backupDir = ".notenest/recovery/link-updates/mock";
      }
      if (applied < edits.length) {
        report.skipped.push({
          note,
          reason: `${edits.length - applied} link(s) were left unchanged`,
        });
      }
    }
    return report;
  }

  private planFor(moves: Map<string, { path: string; title: string }>) {
    return planLinkUpdates(this.knowledge(), moves);
  }

  async renameNoteWithLinks(id: string, title: string): Promise<RenameOutcome> {
    const plan = this.planFor(this.noteMoves(id, this.newNotePath(id, title)));
    const note = await this.renameNote(id, title);
    return { note, links: this.applyPlan(plan) };
  }

  async moveNoteWithLinks(id: string, folder: string | null): Promise<RenameOutcome> {
    const plan = this.planFor(this.noteMoves(id, this.movedNotePath(id, folder)));
    const note = await this.moveNote(id, folder);
    return { note, links: this.applyPlan(plan) };
  }

  async renameFolderWithLinks(path: string, newName: string): Promise<LinkUpdateReport> {
    validName(newName);
    const { folder } = parts(path);
    const plan = this.planFor(this.folderMoves(path, `${folder ? `${folder}/` : ""}${newName}`));
    await this.renameFolder(path, newName);
    return this.applyPlan(plan);
  }

  async moveFolderWithLinks(path: string, newParent: string | null): Promise<LinkUpdateReport> {
    const name = path.slice(path.lastIndexOf("/") + 1);
    const plan = this.planFor(this.folderMoves(path, `${newParent ? `${newParent}/` : ""}${name}`));
    await this.moveFolder(path, newParent);
    return this.applyPlan(plan);
  }

  async getSetting(key: string): Promise<string | null> {
    return Promise.resolve(this.settings.get(key) ?? null);
  }

  async setSetting(key: string, value: string): Promise<void> {
    this.settings.set(key, value);
    return Promise.resolve();
  }
}
