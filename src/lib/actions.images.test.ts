import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MockNotesApi } from "./api/mock";
import { storeImageFiles } from "./actions";
import { useEditor } from "../store/editor";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);
const file = (name: string, type: string, bytes: Uint8Array = PNG) =>
  new File([bytes.buffer as ArrayBuffer], name, { type });

let api: MockNotesApi;

beforeEach(async () => {
  api = new MockNotesApi({ sample: false });
  useNotes.setState({ api });
  await api.createFolder("Notes");
  const note = await api.createNote("Notes", "N");
  useEditor.setState({ noteId: note.id });
  useUi.setState({ notices: [] });
});

afterEach(() => {
  useEditor.getState().close();
});

const messages = () => useUi.getState().notices.map((n) => n.message);

describe("storeImageFiles", () => {
  it("stores images and returns Markdown with paths relative to the note", async () => {
    const md = await storeImageFiles([file("Shot 1.png", "image/png"), file("b.png", "image/png")]);
    expect(md).toBe("![Shot 1](../Attachments/Shot-1-001.png)\n\n![b](../Attachments/b-002.png)");
    expect(messages()).toEqual([]);
  });

  it("reports non-images and oversized files without losing the good ones", async () => {
    const big = file("huge.png", "image/png");
    Object.defineProperty(big, "size", { value: 26 * 1024 * 1024 });
    const md = await storeImageFiles([
      file("notes.md", "text/markdown"),
      big,
      file("ok.png", "image/png"),
    ]);
    expect(md).toBe("![ok](../Attachments/ok-001.png)");
    expect(messages().some((m) => m.includes("notes.md is not an image"))).toBe(true);
    expect(messages().some((m) => m.includes("huge.png is larger than 25 MB"))).toBe(true);
  });

  it("surfaces backend rejections (wrong content behind an image name)", async () => {
    const md = await storeImageFiles([file("fake.png", "image/png", new Uint8Array([1, 2, 3, 4]))]);
    expect(md).toBeNull();
    expect(messages().some((m) => m.includes("Could not attach fake.png"))).toBe(true);
  });

  it("refuses when no note is open", async () => {
    useEditor.setState({ noteId: null });
    expect(await storeImageFiles([file("a.png", "image/png")])).toBeNull();
    expect(messages()).toEqual(["Open a note before adding images."]);
  });
});
