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
    expect(invoke).toHaveBeenCalledWith("list_notes", undefined);
  });

  it("sends the right arguments when saving", async () => {
    invoke.mockResolvedValue({ record, content: "x" });
    const api = new TauriNotesApi();
    await api.saveNote("id", "text", { baseHash: "h" });
    expect(invoke).toHaveBeenLastCalledWith("save_note", {
      id: "id",
      content: "text",
      baseHash: "h",
      overwrite: false,
    });
    const doc = await api.saveNote("id", "text", { overwrite: true });
    expect(invoke).toHaveBeenLastCalledWith("save_note", {
      id: "id",
      content: "text",
      baseHash: null,
      overwrite: true,
    });
    expect(doc.contentHash).toBe("abc");
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
