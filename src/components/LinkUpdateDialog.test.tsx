import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { LinkUpdateDialog } from "./LinkUpdateDialog";
import { useUi } from "../store/ui";

const note = (id: string, title: string) => ({ id, title, path: `${title}.md` });

function open() {
  const resolve = vi.fn();
  render(<LinkUpdateDialog />);
  act(() => {
    useUi.getState().openLinkUpdate({
      heading: "Rename to “New”",
      affected: [
        { note: note("a", "Alpha"), links: 2 },
        { note: note("b", "Beta"), links: 1 },
      ],
      resolve,
    });
  });
  return resolve;
}

afterEach(() => {
  cleanup();
  useUi.setState({ linkUpdate: null });
  vi.restoreAllMocks();
});

describe("LinkUpdateDialog", () => {
  it("lists what would be updated and offers the three choices", () => {
    open();
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toMatch(/3 links in 2 other notes would stop working/);
    expect(screen.getByText("Alpha")).toBeTruthy();
    for (const name of ["Update links", "Rename only", "Cancel"]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
  });

  it("does not put keyboard focus on a button when it opens", () => {
    open();
    expect(document.activeElement?.tagName).not.toBe("BUTTON");
  });

  it("ignores a key press left over from opening it, but not a real click", () => {
    const resolve = open();
    // Enter was still down when the dialog appeared: its release is a click with detail 0.
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }), { detail: 0 });
    expect(resolve).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Update links" }), { detail: 0 });
    expect(resolve).not.toHaveBeenCalled();
    // A mouse click is always honoured, immediately.
    fireEvent.click(screen.getByRole("button", { name: "Update links" }), { detail: 1 });
    expect(resolve).toHaveBeenCalledWith("update");
  });

  it("accepts keyboard activation once the guard period has passed", () => {
    const resolve = open();
    const start = performance.now();
    vi.spyOn(performance, "now").mockReturnValue(start + 1000);
    fireEvent.click(screen.getByRole("button", { name: "Rename only" }), { detail: 0 });
    expect(resolve).toHaveBeenCalledWith("keep");
  });

  it("cancels when dismissed with Escape", () => {
    const resolve = open();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(resolve).toHaveBeenCalledWith("cancel");
  });
});
