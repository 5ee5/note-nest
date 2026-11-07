import { describe, expect, it } from "vitest";
import { buildTagTree, normalizeTag } from "./tags";

describe("normalizeTag", () => {
  it("accepts valid names and folds them", () => {
    expect(normalizeTag("Rust")).toBe("rust");
    expect(normalizeTag("#Programming/Rust")).toBe("programming/rust");
    expect(normalizeTag("  my tag ")).toBe("my-tag");
    expect(normalizeTag("a//b/")).toBe("a/b");
    expect(normalizeTag("日本語")).toBe("日本語");
  });

  it("rejects invalid names", () => {
    for (const bad of [
      "",
      "#",
      "123",
      "2026/10",
      "c++",
      "a.b",
      "tag!",
      "-",
      "/",
      "a".repeat(101),
    ]) {
      expect(normalizeTag(bad)).toBeNull();
    }
  });
});

describe("buildTagTree", () => {
  it("nests tags under their parents and keeps the counts", () => {
    const tree = buildTagTree([
      { name: "programming", count: 3 },
      { name: "programming/rust", count: 2 },
      { name: "programming/python", count: 1 },
      { name: "home", count: 1 },
    ]);
    expect(tree.map((n) => n.full)).toEqual(["home", "programming"]);
    const programming = tree[1]!;
    expect(programming.count).toBe(3);
    expect(programming.children.map((c) => [c.name, c.full, c.count])).toEqual([
      ["python", "programming/python", 1],
      ["rust", "programming/rust", 2],
    ]);
  });

  it("handles deep nesting and an empty list", () => {
    expect(buildTagTree([])).toEqual([]);
    const tree = buildTagTree([
      { name: "a", count: 1 },
      { name: "a/b", count: 1 },
      { name: "a/b/c", count: 1 },
    ]);
    expect(tree[0]!.children[0]!.children[0]!.full).toBe("a/b/c");
  });
});
