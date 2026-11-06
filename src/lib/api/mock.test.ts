import { describe, expect, it } from "vitest";
import { MockNotesApi } from "./mock";

const empty = () => new MockNotesApi({ sample: false });

describe("MockNotesApi", () => {
  it("chooses free default names and rejects bad or taken ones", async () => {
    const api = empty();
    expect((await api.createNote()).path).toBe("Untitled.md");
    expect((await api.createNote()).path).toBe("Untitled 2.md");
    await expect(api.createNote(null, "Untitled")).rejects.toMatchObject({
      code: "already_exists",
    });
    await expect(api.createNote(null, "a/b")).rejects.toMatchObject({ code: "invalid_path" });
    await expect(api.createNote("Nope")).rejects.toMatchObject({ code: "not_found" });
  });

  it("rename, move and duplicate never overwrite", async () => {
    const api = empty();
    await api.createFolder("F");
    const a = await api.createNote(null, "A");
    await api.createNote(null, "B");
    await expect(api.renameNote(a.id, "B")).rejects.toMatchObject({ code: "already_exists" });
    expect((await api.moveNote(a.id, "F")).path).toBe("F/A.md");
    expect((await api.duplicateNote(a.id)).path).toBe("F/A copy.md");
    expect((await api.duplicateNote(a.id)).path).toBe("F/A copy 2.md");
  });

  it("delete moves to trash and restore puts it back, refusing to overwrite", async () => {
    const api = empty();
    const a = await api.createNote(null, "A");
    await api.saveNote(a.id, "text", { overwrite: true });
    const item = await api.deleteNote(a.id);
    expect(await api.listNotes()).toEqual([]);
    expect(await api.listTrash()).toHaveLength(1);
    await api.createNote(null, "A");
    await expect(api.restore(item.id)).rejects.toMatchObject({ code: "already_exists" });
  });

  it("folder rename moves the notes inside", async () => {
    const api = empty();
    await api.createFolder("Work/Deep");
    await api.createNote("Work/Deep", "N");
    await api.renameFolder("Work", "Job");
    expect(await api.listFolders()).toEqual(["Job", "Job/Deep"]);
    expect((await api.listNotes())[0]?.path).toBe("Job/Deep/N.md");
    await expect(api.moveFolder("Job", "Job/Deep")).rejects.toMatchObject({ code: "invalid_path" });
  });

  it("folder delete and restore round-trips its notes", async () => {
    const api = empty();
    await api.createFolder("F");
    await api.createNote("F", "N");
    const item = await api.deleteFolder("F");
    expect(await api.listNotes()).toEqual([]);
    await api.restore(item.id);
    expect((await api.listNotes())[0]?.path).toBe("F/N.md");
  });

  it("saves are guarded by the base hash", async () => {
    const api = empty();
    const a = await api.createNote(null, "A");
    const doc = await api.readNote(a.id);
    api.externalEdit(a.id, "elsewhere");
    await expect(api.saveNote(a.id, "mine", { baseHash: doc.contentHash })).rejects.toMatchObject({
      code: "conflict",
    });
    expect(api.contentOf(a.id)).toBe("elsewhere");
  });
});
