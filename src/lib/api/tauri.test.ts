import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { TauriNotesApi } from "./tauri";

const record = {
  id: "11111111-1111-4111-8111-111111111111",
  path: "Programming/Rust.md",
  title: "Rust",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
  content_hash: "abc",
  pinned: true,
  archived: false,
  last_opened_at: null,
};

beforeEach(() => invoke.mockReset());

describe("TauriNotesApi", () => {
  it("maps backend records to UI summaries", async () => {
    invoke.mockResolvedValueOnce([record]);
    const [note] = await new TauriNotesApi().listNotes();
    expect(note).toMatchObject({
      id: record.id,
      title: "Rust",
      folder: "Programming",
      pinned: true,
      archived: false,
      createdAt: "2026-01-01T00:00:00Z",
      lastOpenedAt: null,
    });
    expect(invoke).toHaveBeenCalledWith("list_notes", undefined, undefined);
  });

  it("sends the right arguments when saving", async () => {
    invoke.mockResolvedValue({ record, content: "x" });
    const api = new TauriNotesApi();
    await api.saveNote("id", "text", { baseHash: "h" });
    expect(invoke).toHaveBeenLastCalledWith(
      "save_note",
      { id: "id", content: "text", baseHash: "h", overwrite: false },
      undefined,
    );
    const doc = await api.saveNote("id", "text", { overwrite: true });
    expect(invoke).toHaveBeenLastCalledWith(
      "save_note",
      { id: "id", content: "text", baseHash: null, overwrite: true },
      undefined,
    );
    expect(doc.contentHash).toBe("abc");
  });

  it("sends attachments as a raw body with metadata in headers", async () => {
    invoke.mockResolvedValueOnce({
      vault_path: "Attachments/image-001.png",
      markdown_path: "../Attachments/image-001.png",
    });
    const bytes = new Uint8Array([1, 2, 3]);
    const out = await new TauriNotesApi().addAttachment("note-id", bytes, "my pic.png");
    expect(invoke).toHaveBeenCalledWith("add_attachment", bytes, {
      headers: { "x-note-id": "note-id", "x-file-name": "my%20pic.png" },
    });
    expect(out).toEqual({
      vaultPath: "Attachments/image-001.png",
      markdownPath: "../Attachments/image-001.png",
    });
  });

  it("renders markdown through the backend", async () => {
    invoke.mockResolvedValueOnce({ html: "<p>x</p>", warnings: ["w"] });
    const out = await new TauriNotesApi().renderMarkdown("id", "x");
    expect(invoke).toHaveBeenCalledWith("render_markdown", { id: "id", content: "x" }, undefined);
    expect(out).toEqual({ html: "<p>x</p>", warnings: ["w"] });
  });

  it("lists tags from both sources", async () => {
    invoke.mockResolvedValueOnce([{ ...record, tags: ["a"], inline_tags: ["b"] }]);
    const [note] = await new TauriNotesApi().listNotes();
    expect(note).toMatchObject({ tags: ["a", "b"], inlineTags: ["b"] });
  });

  it("maps search hits, links and the graph to camelCase", async () => {
    const api = new TauriNotesApi();
    invoke.mockResolvedValueOnce([
      {
        id: "i",
        title: "T",
        path: "T.md",
        archived: false,
        title_parts: [{ text: "T", matched: true }],
        snippet: [],
      },
    ]);
    const hits = await api.searchNotes("t", 5);
    expect(invoke).toHaveBeenLastCalledWith("search_notes", { query: "t", limit: 5 }, undefined);
    expect(hits[0]).toMatchObject({ titleParts: [{ text: "T", matched: true }] });

    const ref = { id: "x", title: "X", path: "X.md" };
    invoke.mockResolvedValueOnce({
      backlinks: [{ source: ref, line: 2, context: "c", target_text: "T", display: null }],
      outgoing: [{ target_text: "Y", display: "y", line: 1, context: "c", target: null }],
      related: [{ note: ref, shared_tags: 2, linked: true }],
    });
    expect(await api.getNoteLinks("i")).toEqual({
      backlinks: [{ source: ref, line: 2, context: "c", targetText: "T", display: null }],
      outgoing: [{ targetText: "Y", display: "y", line: 1, context: "c", target: null }],
      related: [{ note: ref, sharedTags: 2, linked: true }],
    });

    invoke.mockResolvedValueOnce({ nodes: [], edges: [["a", "b"]], total_notes: 7 });
    expect(await api.getGraph("c", 2)).toEqual({ nodes: [], edges: [["a", "b"]], totalNotes: 7 });
    expect(invoke).toHaveBeenLastCalledWith("graph", { center: "c", depth: 2 }, undefined);
  });

  it("calls the link-updating commands with the arguments Rust expects", async () => {
    const api = new TauriNotesApi();
    invoke.mockResolvedValueOnce([]);
    await api.previewRenameNote("i", "New");
    expect(invoke).toHaveBeenLastCalledWith(
      "preview_rename_note",
      { id: "i", newTitle: "New" },
      undefined,
    );

    invoke.mockResolvedValueOnce({
      record,
      links: { updated: [], skipped: [], backup_dir: ".notenest/recovery/link-updates/1" },
    });
    const out = await api.renameNoteWithLinks("i", "New");
    expect(invoke).toHaveBeenLastCalledWith(
      "rename_note_with_links",
      { id: "i", newTitle: "New" },
      undefined,
    );
    expect(out.links.backupDir).toBe(".notenest/recovery/link-updates/1");
    expect(out.note.title).toBe("Rust");

    invoke.mockResolvedValueOnce({ updated: [], skipped: [], backup_dir: null });
    expect((await api.moveFolderWithLinks("A", null)).backupDir).toBeNull();
    expect(invoke).toHaveBeenLastCalledWith(
      "move_folder_with_links",
      { path: "A", newParent: null },
      undefined,
    );

    invoke.mockResolvedValueOnce(record);
    await api.setTags("i", ["a"]);
    expect(invoke).toHaveBeenLastCalledWith("set_tags", { id: "i", tags: ["a"] }, undefined);
  });

  it("turns Tauri rejections into ApiError with a code", async () => {
    invoke.mockRejectedValueOnce({ code: "conflict", message: "changed on disk" });
    await expect(new TauriNotesApi().readNote("id")).rejects.toMatchObject({
      name: "ApiError",
      code: "conflict",
      message: "changed on disk",
    });
    invoke.mockRejectedValueOnce("plain string");
    await expect(new TauriNotesApi().listFolders()).rejects.toMatchObject({ code: "unknown" });
  });

  it("maps trash entries and drafts", async () => {
    invoke.mockResolvedValueOnce([
      { id: "t", original_path: "A.md", trashed_at: "2026-01-01T00:00:00Z", is_dir: false },
    ]);
    expect(await new TauriNotesApi().listTrash()).toEqual([
      { id: "t", originalPath: "A.md", trashedAt: "2026-01-01T00:00:00Z", isDir: false },
    ]);
    invoke.mockResolvedValueOnce([
      {
        note_id: "n",
        path: "A.md",
        base_hash: "h",
        content: "c",
        stashed_at: "2026-01-01T00:00:00Z",
        note_missing: false,
        disk_changed: true,
      },
    ]);
    expect((await new TauriNotesApi().listDrafts())[0]).toMatchObject({
      noteId: "n",
      baseHash: "h",
      diskChanged: true,
    });
  });
});
