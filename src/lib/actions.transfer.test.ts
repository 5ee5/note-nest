import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const pickers = vi.hoisted(() => ({
  pickMarkdownFiles: vi.fn<() => Promise<string[]>>(),
  pickFolder: vi.fn<(title: string) => Promise<string | null>>(),
  pickSaveMarkdown: vi.fn<(name: string) => Promise<string | null>>(),
}));
vi.mock("./pickers", () => pickers);

import { MockNotesApi } from "./api/mock";
import {
  exportCurrentNote,
  exportVault,
  importMarkdownFiles,
  importMarkdownFolder,
} from "./actions";
import { useEditor } from "../store/editor";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

let api: MockNotesApi;
let id: string;

beforeEach(async () => {
  api = new MockNotesApi({ sample: false });
  useNotes.setState({ api });
  await api.createFolder("Inbox");
  id = (await api.createNote(null, "Plan")).id;
  await api.saveNote(id, "---\nid: x\ntags: [a]\n---\nPlan text", { overwrite: true });
  await useNotes.getState().load();
  useUi.setState({ notices: [], view: { kind: "all" }, exportStandard: true, selectedNoteId: id });
  await useEditor.getState().load(id);
  for (const p of Object.values(pickers)) p.mockReset();
});

afterEach(() => useEditor.getState().close());

const notices = () => useUi.getState().notices.map((n) => `${n.kind}: ${n.message}`);

describe("import", () => {
  it("imports picked files into the folder being viewed and opens a single import", async () => {
    api.disk.set("/home/me/Plan.md", "from elsewhere");
    pickers.pickMarkdownFiles.mockResolvedValue(["/home/me/Plan.md"]);
    useUi.setState({ view: { kind: "folder", value: "Inbox" } });
    await importMarkdownFiles();
    const imported = useNotes.getState().notes.find((n) => n.path === "Inbox/Plan.md");
    expect(imported).toBeTruthy();
    expect(useEditor.getState().noteId).toBe(imported!.id);
    expect(notices()).toContain("info: Imported 1 note.");
  });

  it("keeps existing notes and reports what was skipped", async () => {
    api.disk.set("/d/Plan.md", "copy");
    pickers.pickMarkdownFiles.mockResolvedValue(["/d/Plan.md", "/d/photo.jpg", "/d/gone.md"]);
    await importMarkdownFiles();
    expect(api.contentOf(id)).toContain("Plan text");
    expect(useNotes.getState().notes.map((n) => n.path)).toContain("Plan 2.md");
    expect(notices()).toContain(
      "warning: Skipped 2 items: /d/photo.jpg (not found); /d/gone.md (not found).",
    );
  });

  it("does nothing when the picker is cancelled", async () => {
    pickers.pickMarkdownFiles.mockResolvedValue([]);
    const spy = vi.spyOn(api, "importFiles");
    await importMarkdownFiles();
    expect(spy).not.toHaveBeenCalled();
    expect(notices()).toEqual([]);
  });

  it("imports a folder tree", async () => {
    api.disk.set("/src/Research/a.md", "A");
    api.disk.set("/src/Research/Sub/b.md", "B");
    api.disk.set("/src/Research/img.png", "png");
    pickers.pickFolder.mockResolvedValue("/src/Research");
    await importMarkdownFolder();
    const paths = useNotes.getState().notes.map((n) => n.path);
    expect(paths).toEqual(expect.arrayContaining(["Research/a.md", "Research/Sub/b.md"]));
    expect(notices()).toContain("info: Imported 2 notes and 1 attachment into “Research”.");
  });

  it("reports a failed import", async () => {
    pickers.pickFolder.mockResolvedValue("/x");
    vi.spyOn(api, "importFolder").mockRejectedValue(new Error("cannot be inside the vault"));
    await importMarkdownFolder();
    expect(notices()[0]).toMatch(/^error: Could not import: .*inside the vault/);
  });
});

describe("export", () => {
  it("saves unsaved text first, then exports the open note as standard Markdown", async () => {
    useEditor.getState().edit("---\nid: x\ntags: [a]\n---\nPlan text, edited");
    pickers.pickSaveMarkdown.mockResolvedValue("/out/Plan.md");
    await exportCurrentNote();
    expect(pickers.pickSaveMarkdown).toHaveBeenCalledWith("Plan.md");
    expect(api.disk.get("/out/Plan.md")).toBe("---\ntags: [a]\n---\nPlan text, edited");
    expect(notices()).toContain("info: Exported “Plan” to /out/Plan.md.");
  });

  it("exports exactly when standard Markdown is off", async () => {
    useUi.setState({ exportStandard: false });
    pickers.pickSaveMarkdown.mockResolvedValue("/out/Plan.md");
    await exportCurrentNote();
    expect(api.disk.get("/out/Plan.md")).toContain("id: x");
  });

  it("asks for a note when none is open", async () => {
    useEditor.getState().close();
    useUi.setState({ selectedNoteId: null });
    await exportCurrentNote();
    expect(pickers.pickSaveMarkdown).not.toHaveBeenCalled();
    expect(notices()).toEqual(["info: Open a note to export it."]);
  });

  it("exports the vault into the chosen folder", async () => {
    pickers.pickFolder.mockResolvedValue("/backup");
    await exportVault();
    expect(api.disk.get("/backup/Sample export/Plan.md")).toBe("---\ntags: [a]\n---\nPlan text");
    expect(notices()).toContain("info: Exported 1 note to /backup/Sample export.");
  });
});
