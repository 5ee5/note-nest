import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MockNotesApi } from "./api/mock";
import { followLink, followPreviewLink, offerCreateNote } from "./actions";
import { useEditor } from "../store/editor";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";

let api: MockNotesApi;
let ids: Record<string, string>;

beforeEach(async () => {
  api = new MockNotesApi({ sample: false });
  useNotes.setState({ api });
  await api.createFolder("Docs");
  const a = await api.createNote("Docs", "A");
  const b = await api.createNote("Docs", "B");
  await api.saveNote(a.id, "to [[B]] and [[Nowhere]]", { overwrite: true });
  ids = { a: a.id, b: b.id };
  await useNotes.getState().load();
  useUi.setState({ notices: [], prompt: null });
  await useEditor.getState().load(a.id);
});

afterEach(() => {
  useEditor.getState().close();
});

describe("following links", () => {
  it("opens the note a link resolves to", async () => {
    await followLink("B");
    expect(useEditor.getState().noteId).toBe(ids.b);
  });

  it("offers to create a note when the link is broken, in the same folder", async () => {
    await followLink("Nowhere");
    const prompt = useUi.getState().prompt;
    expect(prompt?.title).toBe("Create note");
    expect(prompt?.initial).toBe("Nowhere");
    await prompt!.onSubmit("Nowhere");
    const created = (await api.listNotes()).find((n) => n.title === "Nowhere");
    expect(created?.path).toBe("Docs/Nowhere.md");
    expect(useEditor.getState().noteId).toBe(created?.id);
  });

  it("creates path-qualified targets where they say, and ignores empty ones", async () => {
    offerCreateNote("Docs/Sub/Deep#Heading");
    expect(useUi.getState().prompt?.initial).toBe("Deep");
    useUi.getState().closePrompt();
    offerCreateNote("#only-a-heading");
    expect(useUi.getState().prompt).toBeNull();
  });
});

describe("followPreviewLink", () => {
  it("opens notes for #nn-note links and offers creation for #nn-missing ones", async () => {
    expect(followPreviewLink(`#nn-note=${"11111111-1111-4111-8111-111111111111"}`, null)).toBe(
      true,
    );
    expect(followPreviewLink("#nn-missing", "Note not found: Ghost")).toBe(true);
    expect(useUi.getState().prompt?.initial).toBe("Ghost");
  });

  it("leaves every other link alone", () => {
    for (const href of ["https://example.com", "#top", "Other.md", "#nn-note=not-an-id", ""]) {
      expect(followPreviewLink(href, null)).toBe(false);
    }
  });
});
