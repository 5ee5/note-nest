import { describe, expect, it } from "vitest";
import { MockNotesApi } from "../lib/api/mock";
import { ApiError, type NotesApi } from "../lib/api";
import { createNotesStore } from "./notes";

describe("notes store", () => {
  it("loads notes, folders and trash from the api", async () => {
    const store = createNotesStore(new MockNotesApi());
    await store.getState().load();
    const s = store.getState();
    expect(s.status).toBe("ready");
    expect(s.notes.length).toBeGreaterThan(0);
    expect(s.folders).toContain("Programming");
    expect(s.trash).toEqual([]);
  });

  it("applyNote replaces an existing note or appends a new one", async () => {
    const store = createNotesStore(new MockNotesApi());
    await store.getState().load();
    const first = store.getState().notes[0]!;
    store.getState().applyNote({ ...first, title: "Changed" });
    expect(store.getState().notes.find((n) => n.id === first.id)?.title).toBe("Changed");
    const count = store.getState().notes.length;
    store.getState().applyNote({ ...first, id: "brand-new" });
    expect(store.getState().notes.length).toBe(count + 1);
  });

  it("surfaces backend errors instead of swallowing them", async () => {
    const failing = new MockNotesApi() as NotesApi;
    failing.listNotes = () => Promise.reject(new ApiError("io", "disk on fire"));
    const store = createNotesStore(failing);
    await store.getState().load();
    expect(store.getState().status).toBe("error");
    expect(store.getState().error).toBe("disk on fire");
  });
});
