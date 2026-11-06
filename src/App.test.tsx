import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { useUi } from "./store/ui";

beforeAll(() => {
  // happy-dom has no ResizeObserver; react-resizable-panels needs one.
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  useUi.setState({ paletteOpen: false, settingsOpen: false, query: "", selectedNoteId: null });
});

describe("App shell", () => {
  it("renders sidebar, note explorer and editor with sample notes", async () => {
    render(<App />);
    expect(screen.getByLabelText("Sidebar")).toBeTruthy();
    expect(screen.getByLabelText("Note explorer")).toBeTruthy();
    expect(screen.getByLabelText("Editor")).toBeTruthy();
    expect(await screen.findByText("Rust")).toBeTruthy();
  });

  it("opens the command palette with Ctrl+K and runs a theme command", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.keyboard("{Control>}k{/Control}");
    expect(await screen.findByPlaceholderText(/Type a command/)).toBeTruthy();
    await user.click(screen.getByText("Switch theme: Dark"));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect(window.localStorage.getItem("notenest.theme")).toBe("dark");
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
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(await screen.findByLabelText("Light"));
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("creates a note with Ctrl+N and shows it in the editor", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Rust");
    await user.keyboard("{Control>}n{/Control}");
    expect(await screen.findByRole("heading", { level: 1, name: "Untitled" })).toBeTruthy();
  });
});
