import { describe, expect, it } from "vitest";
import { MockNotesApi } from "../lib/api/mock";
import type { NotesApi } from "../lib/api/types";
import { createNotesStore } from "./notes";

describe("notes store", () => {
  it("loads notes and folders from the api", async () => {
    const store = createNotesStore(new MockNotesApi());
    await store.getState().load();
    expect(store.getState().status).toBe("ready");
    expect(store.getState().notes.length).toBeGreaterThan(0);
    expect(store.getState().folders).toContain("Programming");
  });

  it("creates notes with unique ids", async () => {
    const store = createNotesStore(new MockNotesApi());
    await store.getState().load();
    const before = store.getState().notes.length;
    const a = await store.getState().createNote();
    const b = await store.getState().createNote();
    expect(a && b && a.id !== b.id).toBe(true);
    expect(store.getState().notes.length).toBe(before + 2);
  });

  it("returns the same daily note when opened twice", async () => {
    const store = createNotesStore(new MockNotesApi());
    await store.getState().load();
    const a = await store.getState().openDailyNote();
    const b = await store.getState().openDailyNote();
    expect(a?.id).toBe(b?.id);
  });

  it("surfaces backend errors instead of swallowing them", async () => {
    const failing: NotesApi = {
      ...new MockNotesApi(),
      listNotes: () => Promise.reject(new Error("disk on fire")),
      listFolders: () => Promise.resolve([]),
      createNote: () => Promise.reject(new Error("cannot create")),
      openDailyNote: () => Promise.reject(new Error("cannot open")),
      markOpened: () => Promise.resolve(),
    };
    const store = createNotesStore(failing);
    await store.getState().load();
    expect(store.getState().status).toBe("error");
    expect(store.getState().error).toBe("disk on fire");
    expect(await store.getState().createNote()).toBeNull();
    expect(store.getState().error).toBe("cannot create");
  });
});
