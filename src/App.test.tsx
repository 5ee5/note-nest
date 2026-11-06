import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { MockNotesApi } from "./lib/api/mock";
import { useEditor } from "./store/editor";
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
    editorMode: "edit",
  });
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

  it("filters the note list with the search box", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.type(screen.getByPlaceholderText("Search notes"), "linux");
    expect(screen.queryByText("Rust")).toBeNull();
    expect(screen.getAllByText("Linux").length).toBeGreaterThan(0);
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
