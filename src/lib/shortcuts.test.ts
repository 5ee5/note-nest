import { describe, expect, it } from "vitest";
import { matchShortcut } from "./shortcuts";

const key = (k: string, mods: Partial<KeyboardEvent> = {}) => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});

describe("matchShortcut", () => {
  it("uses Ctrl on non-Mac platforms", () => {
    expect(matchShortcut(key("k", { ctrlKey: true }), false)).toBe("palette");
    expect(matchShortcut(key("p", { ctrlKey: true }), false)).toBe("search");
    expect(matchShortcut(key("n", { ctrlKey: true }), false)).toBe("newNote");
    expect(matchShortcut(key("k", { metaKey: true }), false)).toBeNull();
  });

  it("uses Cmd on macOS", () => {
    expect(matchShortcut(key("K", { metaKey: true }), true)).toBe("palette");
    expect(matchShortcut(key("k", { ctrlKey: true }), true)).toBeNull();
  });

  it("ignores unmodified keys and extra modifiers", () => {
    expect(matchShortcut(key("k"), false)).toBeNull();
    expect(matchShortcut(key("k", { ctrlKey: true, shiftKey: true }), false)).toBeNull();
    expect(matchShortcut(key("x", { ctrlKey: true }), false)).toBeNull();
  });
});
