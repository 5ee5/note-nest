import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MockNotesApi } from "../lib/api/mock";
import { ApiError } from "../lib/api";
import { useNotes } from "../store/notes";
import { LARGE_PREVIEW_CHARS, MarkdownPreview, PREVIEW_DEBOUNCE_MS } from "./MarkdownPreview";

const followPreviewLink = vi.fn();
vi.mock("../lib/actions", () => ({
  followPreviewLink: (...args: unknown[]) => followPreviewLink(...args),
}));

const openExternal = vi.fn();
vi.mock("../lib/openExternal", () => ({ openExternal: (u: string) => openExternal(u) }));

let api: MockNotesApi;

beforeEach(() => {
  vi.useFakeTimers();
  openExternal.mockReset();
  followPreviewLink.mockReset();
  api = new MockNotesApi({ sample: false });
  useNotes.setState({ api });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const settle = (ms = 0) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));

describe("MarkdownPreview", () => {
  it("renders the backend's HTML immediately on mount", async () => {
    vi.spyOn(api, "renderMarkdown").mockResolvedValue({ html: "<h1>Hello</h1>", warnings: [] });
    render(<MarkdownPreview noteId="n" content="# Hello" />);
    await settle();
    expect(screen.getByRole("heading", { name: "Hello" })).toBeTruthy();
  });

  it("re-renders after a debounce and only for the latest text", async () => {
    const spy = vi.spyOn(api, "renderMarkdown").mockImplementation(async (_id, text) => ({
      html: `<p>${text}</p>`,
      warnings: [],
    }));
    const { rerender } = render(<MarkdownPreview noteId="n" content="a" />);
    await settle();
    for (const t of ["ab", "abc", "abcd"]) {
      rerender(<MarkdownPreview noteId="n" content={t} />);
      await settle(PREVIEW_DEBOUNCE_MS / 2);
    }
    expect(spy).toHaveBeenCalledTimes(1);
    await settle(PREVIEW_DEBOUNCE_MS);
    expect(spy).toHaveBeenCalledTimes(2);
    expect(screen.getByText("abcd")).toBeTruthy();
  });

  it("ignores a slow response that was superseded", async () => {
    let releaseOld!: () => void;
    const spy = vi.spyOn(api, "renderMarkdown");
    spy.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          releaseOld = () => resolve({ html: "<p>OLD</p>", warnings: [] });
        }),
    );
    spy.mockResolvedValue({ html: "<p>NEW</p>", warnings: [] });
    const { rerender } = render(<MarkdownPreview noteId="n" content="1" />);
    await settle();
    rerender(<MarkdownPreview noteId="n" content="2" />);
    await settle(PREVIEW_DEBOUNCE_MS + 10);
    releaseOld();
    await settle();
    expect(screen.getByText("NEW")).toBeTruthy();
    expect(screen.queryByText("OLD")).toBeNull();
  });

  it("shows errors and image warnings instead of failing silently", async () => {
    const spy = vi.spyOn(api, "renderMarkdown");
    spy.mockResolvedValueOnce({ html: "<p>x</p>", warnings: ["pic.png: file not found"] });
    const { rerender } = render(<MarkdownPreview noteId="n" content="a" />);
    await settle();
    expect(screen.getByText(/1 image\(s\) could not be shown/)).toBeTruthy();
    expect(screen.getByText("pic.png: file not found")).toBeTruthy();

    spy.mockRejectedValueOnce(new ApiError("too_large", "note is too large"));
    rerender(<MarkdownPreview noteId="n" content="b" />);
    await settle(PREVIEW_DEBOUNCE_MS + 10);
    expect(screen.getByRole("alert").textContent).toContain("note is too large");
  });

  it("opens external links through the opener and never navigates", async () => {
    vi.spyOn(api, "renderMarkdown").mockResolvedValue({
      html: '<p><a href="https://example.com">web</a> <a href="Other.md">note</a> <a href="#top">frag</a></p>',
      warnings: [],
    });
    render(<MarkdownPreview noteId="n" content="x" />);
    await settle();
    for (const name of ["web", "note", "frag"]) {
      const click = new MouseEvent("click", { bubbles: true, cancelable: true });
      screen.getByText(name).dispatchEvent(click);
      expect(click.defaultPrevented).toBe(true); // the webview never follows a link
    }
    expect(openExternal.mock.calls.map((c) => c[0])).toEqual([
      "https://example.com",
      "Other.md",
      "#top",
    ]);
  });

  it("routes note links to the app and never to the system opener", async () => {
    vi.spyOn(api, "renderMarkdown").mockResolvedValue({
      html: '<p><a href="#nn-note=11111111-1111-4111-8111-111111111111">note</a> <a href="#nn-missing" title="Note not found: Gone">gone</a> <a href="https://example.com">web</a></p>',
      warnings: [],
    });
    followPreviewLink.mockImplementation((href: string) => href.startsWith("#nn-"));
    render(<MarkdownPreview noteId="n" content="x" />);
    await settle();
    for (const name of ["note", "gone", "web"]) {
      const click = new MouseEvent("click", { bubbles: true, cancelable: true });
      screen.getByText(name).dispatchEvent(click);
      expect(click.defaultPrevented).toBe(true);
    }
    expect(followPreviewLink.mock.calls.map((c) => [c[0], c[1]])).toEqual([
      ["#nn-note=11111111-1111-4111-8111-111111111111", null],
      ["#nn-missing", "Note not found: Gone"],
      ["https://example.com", null],
    ]);
    expect(openExternal.mock.calls.map((c) => c[0])).toEqual(["https://example.com"]);
  });

  it("renders large notes only on request", async () => {
    const spy = vi
      .spyOn(api, "renderMarkdown")
      .mockResolvedValue({ html: "<p>big</p>", warnings: [] });
    const big = "x".repeat(LARGE_PREVIEW_CHARS + 1);
    render(<MarkdownPreview noteId="n" content={big} />);
    await settle(PREVIEW_DEBOUNCE_MS * 2);
    expect(spy).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /Render preview/ }));
    await settle();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(screen.getByText("big")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Refresh/ })).toBeTruthy();
  });
});
