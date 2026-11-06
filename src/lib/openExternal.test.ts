import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const openUrl = vi.fn();
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: (u: string) => openUrl(u) }));

import { openExternal } from "./openExternal";

beforeEach(() => {
  openUrl.mockReset();
  vi.spyOn(window, "open").mockImplementation(() => null);
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.restoreAllMocks();
});

describe("openExternal", () => {
  it("opens only http, https and mailto links", async () => {
    for (const url of [
      "https://example.com",
      "http://example.com/a?b=1",
      "mailto:a@b.c",
      " HTTPS://X.org",
    ]) {
      expect(await openExternal(url)).toBe(true);
    }
    for (const url of [
      "javascript:alert(1)",
      "data:text/html,x",
      "file:///etc/passwd",
      "#frag",
      "Other.md",
      "",
      "ftp://x",
    ]) {
      expect(await openExternal(url)).toBe(false);
    }
    expect(window.open).toHaveBeenCalledTimes(4);
  });

  it("uses noopener in the browser", async () => {
    await openExternal("https://example.com");
    expect(window.open).toHaveBeenCalledWith(
      "https://example.com",
      "_blank",
      "noopener,noreferrer",
    );
  });

  it("goes through the opener plugin on desktop", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    await openExternal("https://example.com");
    expect(openUrl).toHaveBeenCalledWith("https://example.com");
    expect(window.open).not.toHaveBeenCalled();
  });
});
