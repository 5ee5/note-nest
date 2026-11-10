import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { MockNotesApi } from "./lib/api/mock";
import { useEditor } from "./store/editor";
import { useLinks } from "./store/links";
import { useNotes } from "./store/notes";
import { useRecovery } from "./store/recovery";
import { useUi } from "./store/ui";
import { useVault } from "./store/vault";

// CodeMirror needs a real browser layout engine; the app tests use a plain textarea with
// the same props. The real editor is covered by headless unit tests and browser checks.
vi.mock("./components/MarkdownEditor", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ value, onChange }: { value: string; onChange: (t: string) => void }) =>
      createElement("textarea", {
        "aria-label": "Note content",
        value,
        onChange: (e: { target: { value: string } }) => onChange(e.target.value),
      }),
  };
});

vi.mock("./components/FormatToolbar", () => ({ default: () => null }));

beforeAll(() => {
  // happy-dom has no ResizeObserver; react-resizable-panels needs one.
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

let api: MockNotesApi;

beforeEach(() => {
  api = new MockNotesApi();
  useNotes.setState({ api, notes: [], folders: [], trash: [], status: "idle" });
  useVault.setState({ phase: "checking" });
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  useEditor.getState().close();
  useRecovery.setState({ drafts: [] });
  useUi.setState({
    paletteOpen: false,
    settingsOpen: false,
    query: "",
    selectedNoteId: null,
    view: { kind: "all" },
    notices: [],
    prompt: null,
    linkUpdate: null,
    editorMode: "edit",
  });
  useLinks.setState({ noteId: null, data: null, error: null, open: false, tab: "backlinks" });
});

describe("App shell", () => {
  it("renders sidebar, note explorer and editor with sample notes", async () => {
    render(<App />);
    expect(await screen.findByLabelText("Sidebar")).toBeTruthy();
    expect(screen.getByLabelText("Note explorer")).toBeTruthy();
    expect(screen.getByLabelText("Editor")).toBeTruthy();
    expect(await screen.findByText("Rust")).toBeTruthy();
    expect(screen.getByText(/Showing sample data/)).toBeTruthy();
  });

  it("opens the command palette with Ctrl+K and runs a theme command", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.keyboard("{Control>}k{/Control}");
    expect(await screen.findByPlaceholderText(/Type a command/)).toBeTruthy();
    await user.click(screen.getByText("Switch theme: Dark"));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect(window.localStorage.getItem("notenest.theme")).toBe("dark");
    await waitFor(async () => expect(await api.getSetting("theme")).toBe("dark"));
  });

  it("searches note contents with the search box and highlights the match", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.type(screen.getByPlaceholderText("Search notes"), "useful");
    const results = await screen.findByLabelText("Search results");
    await waitFor(() => expect(within(results).getByText("Linux")).toBeTruthy());
    expect(within(results).queryByText("Rust")).toBeNull();
    const marks = [...results.querySelectorAll("mark")].map((m) => m.textContent?.toLowerCase());
    expect(marks).toContain("useful");

    await user.clear(screen.getByPlaceholderText("Search notes"));
    await waitFor(() => expect(screen.queryByLabelText("Search results")).toBeNull());
    expect(screen.getByText("Rust")).toBeTruthy();
  });

  it("opens settings and switches theme", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(await screen.findByLabelText("Light"));
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("notes", () => {
  it("creates a note with Ctrl+N and opens it in the editor", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.keyboard("{Control>}n{/Control}");
    const title = (await screen.findByLabelText("Note title")) as HTMLInputElement;
    expect(title.value).toBe("Untitled");
  });

  it("autosaves typed text and shows the status", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    const box = (await screen.findByLabelText("Note content")) as HTMLTextAreaElement;
    await waitFor(() => expect(box.value).toContain("Useful commands"));
    await user.type(box, " plus more");
    expect(screen.getByText("Unsaved changes")).toBeTruthy();
    await waitFor(() => expect(screen.getByText("Saved")).toBeTruthy(), { timeout: 3000 });
    const id = useEditor.getState().noteId!;
    expect(api.contentOf(id)).toBe("Useful commands and config notes. plus more");
  });

  it("renames a note from its title", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    const title = (await screen.findByLabelText("Note title")) as HTMLInputElement;
    await user.clear(title);
    await user.type(title, "Kernel{Enter}");
    // Rust links to Linux, so the app asks what to do with that link first.
    await user.click(await screen.findByRole("button", { name: "Update links" }));
    await waitFor(() =>
      expect(useNotes.getState().notes.some((n) => n.path === "Programming/Kernel.md")).toBe(true),
    );
  });

  it("deletes to the trash and restores from there", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    await user.click(await screen.findByRole("button", { name: "Delete note" }));
    await waitFor(() => expect(screen.queryByText("Linux")).toBeNull());

    await user.click(screen.getByRole("button", { name: /^Trash/ }));
    const explorer = screen.getByLabelText("Note explorer");
    expect(await within(explorer).findByText("Programming/Linux.md")).toBeTruthy();
    await user.click(within(explorer).getByRole("button", { name: "Restore" }));
    await waitFor(() =>
      expect(useNotes.getState().notes.some((n) => n.path === "Programming/Linux.md")).toBe(true),
    );
  });

  it("pins a note", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    await user.click(await screen.findByRole("button", { name: "Pin note" }));
    await waitFor(() =>
      expect(useNotes.getState().notes.find((n) => n.title === "Linux")?.pinned).toBe(true),
    );
  });
});

describe("folders", () => {
  it("creates a folder through the dialog", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.click(screen.getByRole("button", { name: "New folder" }));
    await user.type(await screen.findByLabelText("Folder name"), "Reading");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(useNotes.getState().folders).toContain("Reading"));
  });

  it("shows an error notice when a folder cannot be created", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.click(screen.getByRole("button", { name: "New folder" }));
    await user.type(await screen.findByLabelText("Folder name"), "Programming");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(screen.getByText(/Could not create the folder/)).toBeTruthy();
  });
});

describe("knowledge", () => {
  it("shows tags as a tree with counts and filters notes by a tag", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    const tags = await screen.findByLabelText("Tags", { selector: "ul" });
    expect(within(tags).getByText("programming")).toBeTruthy();
    expect(within(tags).queryByText("rust")).toBeNull(); // collapsed
    await user.click(within(tags).getByRole("button", { name: "Expand programming" }));
    await user.click(within(tags).getByRole("button", { name: /^rust/ }));
    const explorer = screen.getByLabelText("Note explorer");
    expect(within(explorer).getByText("#programming/rust")).toBeTruthy();
    expect(within(explorer).getByText("Rust")).toBeTruthy();
    expect(within(explorer).queryByText("Linux")).toBeNull();
  });

  it("adds, rejects and removes tags on a note", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    const input = await screen.findByLabelText("Add tag");

    await user.type(input, "Study{Enter}");
    expect(await screen.findByRole("button", { name: "Remove tag study" })).toBeTruthy();
    const stored = await api.listNotes();
    expect(stored.find((n) => n.title === "Linux")?.tags).toContain("study");

    await user.type(screen.getByLabelText("Add tag"), "123{Enter}");
    expect(await screen.findByText(/"123" is not a valid tag/)).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Remove tag study" }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Remove tag study" })).toBeNull(),
    );
  });

  it("shows inline #tags read-only", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("ideas"));
    expect(await screen.findByRole("button", { name: "Show notes tagged todo" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Remove tag todo" })).toBeNull();
  });

  it("lists orphaned notes and broken links", async () => {
    const user = userEvent.setup();
    const extra = await api.createNote(null, "Dangling");
    await api.saveNote(extra.id, "points at [[Missing Note]]", { overwrite: true });
    render(<App />);
    await screen.findByText("Rust");

    await user.click(screen.getByRole("button", { name: "Orphaned notes" }));
    const orphans = await screen.findByLabelText("Orphaned notes", { selector: "ul" });
    expect(within(orphans).getByText("ideas")).toBeTruthy();
    expect(within(orphans).queryByText("Rust")).toBeNull(); // linked to other notes

    await user.click(screen.getByRole("button", { name: "Broken links" }));
    const broken = await screen.findByLabelText("Broken links", { selector: "ul" });
    expect(within(broken).getByText("Dangling")).toBeTruthy();
    expect(within(broken).getByText("[[Missing Note]]")).toBeTruthy();
  });
});

describe("links", () => {
  it("shows backlinks, links and related notes for the open note", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Rust"));
    const panel = await screen.findByLabelText("Note connections");
    await waitFor(() => expect(panel.textContent).toContain("1 backlink · 2 links"));

    await user.click(within(panel).getByRole("button", { name: /1 backlink/ }));
    const backlinks = within(panel).getByLabelText("Backlinks");
    expect(within(backlinks).getByText("CodeBuddy")).toBeTruthy();
    expect(within(backlinks).getByText(/Built with \[\[Rust\]\]/)).toBeTruthy();

    await user.click(within(panel).getByRole("tab", { name: /^Links/ }));
    const links = within(panel).getByLabelText("Links in this note");
    expect(within(links).getByText("Linux", { exact: false })).toBeTruthy();

    await user.click(within(panel).getByRole("tab", { name: /^Related/ }));
    expect(
      within(within(panel).getByLabelText("Related notes")).getByText("CodeBuddy"),
    ).toBeTruthy();
  });

  it("offers to create the note a broken link points at", async () => {
    const user = userEvent.setup();
    const extra = await api.createNote(null, "Dangling");
    await api.saveNote(extra.id, "points at [[Missing Note]]", { overwrite: true });
    render(<App />);
    await user.click(await screen.findByText("Dangling"));
    const panel = await screen.findByLabelText("Note connections");
    await waitFor(() => expect(panel.textContent).toContain("1 broken"));
    await user.click(within(panel).getByRole("button", { name: /1 backlinks?|0 backlinks/ }));
    await user.click(within(panel).getByRole("tab", { name: /^Links/ }));
    await user.click(within(panel).getByRole("button", { name: "Create" }));

    const dialog = await screen.findByRole("dialog");
    const title = within(dialog).getByLabelText("Note title") as HTMLInputElement;
    expect(title.value).toBe("Missing Note");
    await user.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(async () =>
      expect((await api.listNotes()).some((n) => n.title === "Missing Note")).toBe(true),
    );
  });
});

describe("renaming a linked note", () => {
  const rename = async (user: ReturnType<typeof userEvent.setup>, to: string) => {
    await user.click(await screen.findByText("Rust"));
    const title = (await screen.findByLabelText("Note title")) as HTMLInputElement;
    await user.clear(title);
    await user.type(title, `${to}{Enter}`);
    return screen.findByRole("dialog");
  };
  const codeBuddy = async () => {
    const note = (await api.listNotes()).find((n) => n.title === "CodeBuddy")!;
    return api.contentOf(note.id);
  };

  it("asks first, then updates the links in other notes", async () => {
    const user = userEvent.setup();
    render(<App />);
    const dialog = await rename(user, "Rust Lang");
    expect(within(dialog).getByText("CodeBuddy")).toBeTruthy();
    expect(dialog.textContent).toMatch(/1 link in 1 other note would stop working/);
    expect(await codeBuddy()).toContain("[[Rust]]"); // nothing changes before the answer

    await user.click(within(dialog).getByRole("button", { name: "Update links" }));
    await waitFor(async () => expect(await codeBuddy()).toContain("[[Rust Lang]]"));
    expect(await screen.findByText(/Updated 1 link in 1 note/)).toBeTruthy();
    expect((await api.listNotes()).some((n) => n.path === "Programming/Rust Lang.md")).toBe(true);
  });

  it("can rename without touching other notes", async () => {
    const user = userEvent.setup();
    render(<App />);
    const dialog = await rename(user, "Rust Lang");
    await user.click(within(dialog).getByRole("button", { name: "Rename only" }));
    await waitFor(async () =>
      expect((await api.listNotes()).some((n) => n.path === "Programming/Rust Lang.md")).toBe(true),
    );
    expect(await codeBuddy()).toContain("[[Rust]]");
    expect((await api.listBrokenLinks()).length).toBe(1);
  });

  it("cancelling leaves everything as it was", async () => {
    const user = userEvent.setup();
    render(<App />);
    const dialog = await rename(user, "Rust Lang");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect((await api.listNotes()).some((n) => n.path === "Programming/Rust.md")).toBe(true);
    expect(await codeBuddy()).toContain("[[Rust]]");
    expect(((await screen.findByLabelText("Note title")) as HTMLInputElement).value).toBe("Rust");
  });

  it("renames silently when nothing links to the note", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("ideas"));
    const title = (await screen.findByLabelText("Note title")) as HTMLInputElement;
    await user.clear(title);
    await user.type(title, "thoughts{Enter}");
    await waitFor(async () =>
      expect((await api.listNotes()).some((n) => n.path === "Inbox/thoughts.md")).toBe(true),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("statistics", () => {
  it("shows word and character counts that follow the text", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    const bar = await screen.findByLabelText("Document statistics");
    await waitFor(() => expect(bar.textContent).toContain("5 words"));
    expect(bar.textContent).toContain("33 characters");
    await user.type(await screen.findByLabelText("Note content"), " and more");
    await waitFor(() => expect(bar.textContent).toContain("7 words"));
  });
});

describe("view modes", () => {
  it("switches between edit, split and preview and remembers the choice", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    await screen.findByLabelText("Note content");
    expect(screen.queryByLabelText("Rendered preview")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Edit and preview" }));
    expect(await screen.findByLabelText("Rendered preview")).toBeTruthy();
    expect(screen.getByLabelText("Note content")).toBeTruthy();
    await waitFor(async () => expect(await api.getSetting("editorMode")).toBe("split"));

    await user.click(screen.getByRole("button", { name: "Preview" }));
    expect(screen.getByRole("button", { name: "Preview" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    await waitFor(() =>
      expect(
        within(screen.getByLabelText("Rendered preview")).getByText(/Useful commands/),
      ).toBeTruthy(),
    );

    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.queryByLabelText("Rendered preview")).toBeNull();
  });
});

describe("safety", () => {
  it("offers a conflict choice instead of overwriting an external edit", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    const box = (await screen.findByLabelText("Note content")) as HTMLTextAreaElement;
    await waitFor(() => expect(box.value).toContain("Useful commands"));
    const id = useEditor.getState().noteId!;

    api.externalEdit(id, "changed in another editor");
    await user.type(box, " mine");
    expect(await screen.findByText(/changed outside NoteNest/, {}, { timeout: 3000 })).toBeTruthy();
    expect(api.contentOf(id)).toBe("changed in another editor");

    await user.click(screen.getByRole("button", { name: "Keep my version" }));
    await waitFor(() => expect(api.contentOf(id)).toContain("mine"));
  });

  it("offers recovered drafts at startup and restores them", async () => {
    const user = userEvent.setup();
    const linux = (await api.listNotes()).find((n) => n.title === "Linux")!;
    await api.stashDraft(linux.id, "text typed before the crash", "stale");
    render(<App />);
    expect(await screen.findByText("Recover unsaved changes")).toBeTruthy();
    expect(screen.getByText("text typed before the crash")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() =>
      expect((screen.getByLabelText("Note content") as HTMLTextAreaElement).value).toBe(
        "text typed before the crash",
      ),
    );
  });
});

describe("changes made outside NoteNest", () => {
  async function openLinux() {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByText("Linux"));
    const box = (await screen.findByLabelText("Note content")) as HTMLTextAreaElement;
    await waitFor(() => expect(box.value).toContain("Useful commands"));
    return { user, box, id: useEditor.getState().noteId! };
  }

  it("shows an outside edit of the open note right away", async () => {
    const { box, id } = await openLinux();
    api.externalEdit(id, "rewritten in another editor");
    api.emitVaultChange({ changed: [id] });
    await waitFor(() => expect(box.value).toBe("rewritten in another editor"));
    expect(screen.queryByText(/changed outside NoteNest/)).toBeNull();
  });

  it("lists notes created outside NoteNest", async () => {
    await openLinux();
    const added = api.externalCreate("Inbox/From the file manager.md", "hello");
    api.emitVaultChange({ changed: [added] });
    expect(await screen.findByText("From the file manager")).toBeTruthy();
  });

  it("closes a deleted note that had nothing unsaved", async () => {
    const { id } = await openLinux();
    api.externalDelete(id);
    api.emitVaultChange({ removed: [id] });
    expect(await screen.findByText(/“Linux” was deleted or moved outside NoteNest/)).toBeTruthy();
    await waitFor(() => expect(screen.queryByLabelText("Note content")).toBeNull());
  });

  it("follows a note renamed outside NoteNest under a new id", async () => {
    const { id } = await openLinux();
    const content = api.contentOf(id);
    api.externalDelete(id);
    const renamed = api.externalCreate("Programming/Linux tips.md", content);
    api.emitVaultChange({ changed: [renamed], removed: [id] });
    await waitFor(() => expect(useEditor.getState().noteId).toBe(renamed));
    expect(await screen.findByText(/“Linux” was renamed or moved/)).toBeTruthy();
    await waitFor(() =>
      expect((screen.getByLabelText("Note content") as HTMLTextAreaElement).value).toContain(
        "Useful commands",
      ),
    );
  });

  it("keeps unsaved text of a deleted note and saves it as a new note", async () => {
    const { user, box, id } = await openLinux();
    await user.type(box, " plus unsaved words");
    api.externalDelete(id);
    api.emitVaultChange({ removed: [id] });
    expect(await screen.findByText(/deleted or moved outside NoteNest, but you have/)).toBeTruthy();
    expect(box.value).toContain("plus unsaved words");

    await user.click(screen.getByRole("button", { name: "Save as a new note" }));
    await waitFor(() => {
      const open = useEditor.getState().noteId;
      expect(open).not.toBeNull();
      expect(open).not.toBe(id);
      expect(api.contentOf(open!)).toContain("plus unsaved words");
    });
    expect(useNotes.getState().notes.find((n) => n.id === useEditor.getState().noteId)?.title).toBe(
      "Linux",
    );
  });

  it("reconciles when the window regains focus", async () => {
    await openLinux();
    const spy = vi.spyOn(api, "reconcile");
    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));
  });
});

describe("Graph view", () => {
  it("opens from the sidebar and summarises the vault graph", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByLabelText("Sidebar");
    await user.click(screen.getByRole("button", { name: "Graph" }));
    const graph = await screen.findByRole("region", { name: "Graph" });
    expect(
      within(graph).getByRole("button", { name: "Whole vault" }).getAttribute("aria-pressed"),
    ).toBe("true");
    expect(
      await within(graph).findByRole("img", { name: /Link graph with \d+ notes/ }),
    ).toBeTruthy();
    expect(within(graph).getByText(/\d+ notes, \d+ links/)).toBeTruthy();
    expect(screen.queryByLabelText("Editor")).toBeNull();
  });

  it("is also reachable from the command palette", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByLabelText("Sidebar");
    await user.keyboard("{Control>}k{/Control}");
    await user.click(await screen.findByText("Go to Graph"));
    expect(await screen.findByRole("region", { name: "Graph" })).toBeTruthy();
  });

  it("asks for a note when the local graph has nothing to centre on", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByLabelText("Sidebar");
    await user.click(screen.getByRole("button", { name: "Graph" }));
    const graph = await screen.findByRole("region", { name: "Graph" });
    await user.click(within(graph).getByRole("button", { name: "Around this note" }));
    expect(within(graph).getByText(/Open a note to see the notes around it/)).toBeTruthy();
    await user.click(within(graph).getByRole("button", { name: "Show the whole vault" }));
    expect(await within(graph).findByRole("img", { name: /Link graph/ })).toBeTruthy();
  });

  it("shows the graph around the selected note with a depth control", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByLabelText("Sidebar");
    await waitFor(() => expect(useNotes.getState().notes.length).toBeGreaterThan(0));
    const id = useNotes.getState().notes[0]!.id;
    useUi.setState({ selectedNoteId: id });
    await user.click(screen.getByRole("button", { name: "Graph" }));
    const graph = await screen.findByRole("region", { name: "Graph" });
    expect(
      within(graph).getByRole("button", { name: "Around this note" }).getAttribute("aria-pressed"),
    ).toBe("true");
    expect(await within(graph).findByText(/\d+ of \d+ notes/)).toBeTruthy();
    await user.click(within(graph).getByRole("button", { name: "3" }));
    expect(within(graph).getByRole("button", { name: "3" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
  });

  it("will not draw a huge vault until asked", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByLabelText("Sidebar");
    await user.click(screen.getByRole("button", { name: "Graph" }));
    const graph = await screen.findByRole("region", { name: "Graph" });
    const template = useNotes.getState().notes[0]!;
    const many = Array.from({ length: 1501 }, (_, i) => ({
      ...template,
      id: `bulk-${i}`,
      title: `Bulk ${i}`,
    }));
    useNotes.setState({ notes: many });
    expect(await within(graph).findByText(/too many to draw at once/)).toBeTruthy();
    expect(within(graph).queryByRole("img", { name: /Link graph/ })).toBeNull();
    await user.click(within(graph).getByRole("button", { name: "Draw everything" }));
    expect(await within(graph).findByRole("img", { name: /Link graph/ })).toBeTruthy();
  });
});
