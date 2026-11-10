import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockNotesApi } from "../lib/api/mock";
import { ApiError } from "../lib/api";
import { AUTOSAVE_MS, DRAFT_MS, useEditor } from "./editor";
import { useNotes } from "./notes";
import { useUi } from "./ui";

let api: MockNotesApi;
let id: string;

beforeEach(async () => {
  vi.useFakeTimers();
  api = new MockNotesApi({ sample: false });
  useNotes.setState({ api, notes: [], folders: [], trash: [] });
  id = (await api.createNote(null, "A")).id;
  await useNotes.getState().load();
  useUi.setState({ notices: [], selectedNoteId: id });
  await useEditor.getState().load(id);
});

afterEach(() => {
  useEditor.getState().close();
  vi.useRealTimers();
});

const typeText = (t: string) => useEditor.getState().edit(t);

describe("editor autosave", () => {
  it("loads the note and starts idle", () => {
    const s = useEditor.getState();
    expect(s.noteId).toBe(id);
    expect(s.status).toBe("idle");
  });

  it("saves after the debounce and reports saved", async () => {
    typeText("hello");
    expect(useEditor.getState().status).toBe("dirty");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS - 10);
    expect(api.contentOf(id)).toBe("");
    await vi.advanceTimersByTimeAsync(20);
    expect(api.contentOf(id)).toBe("hello");
    expect(useEditor.getState().status).toBe("saved");
  });

  it("keeps resetting the debounce while typing and saves once", async () => {
    const spy = vi.spyOn(api, "saveNote");
    for (const t of ["h", "he", "hel", "hell", "hello"]) {
      typeText(t);
      await vi.advanceTimersByTimeAsync(AUTOSAVE_MS / 2);
    }
    expect(spy).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(api.contentOf(id)).toBe("hello");
  });

  it("stashes a draft quickly so a crash loses little", async () => {
    typeText("unsaved");
    await vi.advanceTimersByTimeAsync(DRAFT_MS + 10);
    const drafts = await api.listDrafts();
    expect(drafts.map((d) => d.content)).toEqual(["unsaved"]);
  });

  it("flush saves immediately", async () => {
    typeText("now");
    await useEditor.getState().flush();
    expect(api.contentOf(id)).toBe("now");
    expect(useEditor.getState().status).toBe("saved");
  });

  it("edits made during a save are saved afterwards", async () => {
    const original = api.saveNote.bind(api);
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    vi.spyOn(api, "saveNote").mockImplementationOnce(async (...args) => {
      await gate;
      return original(...args);
    });
    typeText("one");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    expect(useEditor.getState().status).toBe("saving");
    typeText("one two");
    release();
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 2);
    expect(api.contentOf(id)).toBe("one two");
    expect(useEditor.getState().status).toBe("saved");
  });

  it("reverting to the saved text clears the dirty state", () => {
    typeText("x");
    typeText("");
    expect(useEditor.getState().status).toBe("saved");
  });
});

describe("editor failures", () => {
  it("shows an error, keeps the text and a draft, and recovers on the next edit", async () => {
    const original = api.saveNote.bind(api);
    vi.spyOn(api, "saveNote").mockRejectedValueOnce(new ApiError("io", "disk full"));
    typeText("precious");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    let s = useEditor.getState();
    expect(s.status).toBe("error");
    expect(s.message).toBe("disk full");
    expect(s.content).toBe("precious");
    expect((await api.listDrafts())[0]?.content).toBe("precious");

    vi.spyOn(api, "saveNote").mockImplementation(original);
    typeText("precious!");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    s = useEditor.getState();
    expect(s.status).toBe("saved");
    expect(api.contentOf(id)).toBe("precious!");
  });

  it("detects an external change, pauses autosave and writes nothing", async () => {
    api.externalEdit(id, "edited in VS Code");
    typeText("my edit");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    expect(useEditor.getState().status).toBe("conflict");
    expect(api.contentOf(id)).toBe("edited in VS Code");

    typeText("my edit, more");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 3);
    expect(api.contentOf(id)).toBe("edited in VS Code");
    expect(useEditor.getState().status).toBe("conflict");
  });

  it("keepMine overwrites the file", async () => {
    api.externalEdit(id, "theirs");
    typeText("mine");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    await useEditor.getState().keepMine();
    expect(api.contentOf(id)).toBe("mine");
    expect(useEditor.getState().status).toBe("saved");
  });

  it("loadTheirs shows the disk version but keeps mine as a recovery draft", async () => {
    api.externalEdit(id, "theirs");
    typeText("mine");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    await useEditor.getState().loadTheirs();
    expect(useEditor.getState().content).toBe("theirs");
    expect(useEditor.getState().status).toBe("idle");
    expect((await api.listDrafts()).map((d) => d.content)).toContain("mine");
    expect(useUi.getState().notices.some((n) => n.message.includes("recovery"))).toBe(true);
  });

  it("restoreContent with overwrite replaces newer disk content", async () => {
    api.externalEdit(id, "newer on disk");
    useEditor.getState().restoreContent("recovered", true);
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    expect(api.contentOf(id)).toBe("recovered");
  });

  it("a note that cannot be opened reports why", async () => {
    vi.spyOn(api, "readNote").mockRejectedValueOnce(new ApiError("corrupt", "not valid UTF-8"));
    await useEditor.getState().load(id);
    const s = useEditor.getState();
    expect(s.loadError).toBe("not valid UTF-8");
    expect(s.noteId).toBeNull();
  });
});

describe("changes made outside the app", () => {
  const change = (over: { changed?: string[]; removed?: string[] }) => ({
    changed: [],
    removed: [],
    folders: false,
    files: false,
    ...over,
  });

  it("reloads a note without unsaved text in place, keeping the editor mounted", async () => {
    const revision = useEditor.getState().revision;
    api.externalEdit(id, "edited in another app");
    const outcome = await useEditor.getState().externalChange(change({ changed: [id] }));
    expect(outcome).toBe("reloaded");
    const s = useEditor.getState();
    expect(s.content).toBe("edited in another app");
    expect(s.savedContent).toBe("edited in another app");
    expect(s.revision).toBe(revision); // applied as an edit, so the cursor survives
    // Typing afterwards saves on top of the new version without a conflict.
    typeText("edited in another app, and here");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    expect(useEditor.getState().status).toBe("saved");
    expect(api.contentOf(id)).toBe("edited in another app, and here");
  });

  it("never overwrites unsaved text: it pauses autosave and keeps a draft", async () => {
    typeText("mine");
    api.externalEdit(id, "theirs");
    const outcome = await useEditor.getState().externalChange(change({ changed: [id] }));
    expect(outcome).toBe("conflict");
    expect(useEditor.getState().status).toBe("conflict");
    expect(useEditor.getState().content).toBe("mine");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 2);
    expect(api.contentOf(id)).toBe("theirs");
    expect((await api.listDrafts()).map((d) => d.content)).toEqual(["mine"]);
  });

  it("ignores changes to other notes and reports of its own saves", async () => {
    const other = (await api.createNote(null, "B")).id;
    const spy = vi.spyOn(api, "readNote");
    expect(await useEditor.getState().externalChange(change({ changed: [other] }))).toBe(
      "unaffected",
    );
    expect(spy).not.toHaveBeenCalled();

    typeText("saved by me");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 10);
    expect(await useEditor.getState().externalChange(change({ changed: [id] }))).toBe("unaffected");
    expect(useEditor.getState().status).toBe("saved");
  });

  it("waits for a save in progress before comparing", async () => {
    typeText("about to be saved");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS + 1); // save starts
    const outcome = await useEditor.getState().externalChange(change({ changed: [id] }));
    expect(outcome).toBe("unaffected");
    expect(useEditor.getState().status).toBe("saved");
  });

  it("closes a deleted note that had no unsaved text", async () => {
    api.externalDelete(id);
    expect(await useEditor.getState().externalChange(change({ removed: [id] }))).toBe("closed");
    expect(useEditor.getState().noteId).toBeNull();
  });

  it("keeps unsaved text of a deleted note and stops autosaving it", async () => {
    typeText("precious");
    await vi.advanceTimersByTimeAsync(DRAFT_MS + 10); // a draft exists
    api.externalDelete(id);
    expect(await useEditor.getState().externalChange(change({ removed: [id] }))).toBe("missing");
    const s = useEditor.getState();
    expect(s.status).toBe("missing");
    expect(s.content).toBe("precious");
    expect(s.missingNote?.title).toBe("A");

    const save = vi.spyOn(api, "saveNote");
    typeText("precious, and more");
    await vi.advanceTimersByTimeAsync(AUTOSAVE_MS * 2);
    expect(save).not.toHaveBeenCalled();
    expect(useEditor.getState().status).toBe("missing");
    const drafts = await api.listDrafts();
    expect(drafts.map((d) => d.content)).toEqual(["precious, and more"]);
  });
});
