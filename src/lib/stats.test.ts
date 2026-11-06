import { describe, expect, it } from "vitest";
import { FAST_PATH_CHARS, noteStats, stripFrontmatter, textStats } from "./stats";

describe("stripFrontmatter", () => {
  it("removes a leading block and a BOM", () => {
    expect(stripFrontmatter("---\nid: x\n---\nBody")).toBe("Body");
    expect(stripFrontmatter("﻿---\r\nid: x\r\n...\r\nBody")).toBe("Body");
    expect(stripFrontmatter("---\n---\n")).toBe("");
  });

  it("leaves everything else alone", () => {
    expect(stripFrontmatter("# Title\n---\nnot front\n---")).toBe("# Title\n---\nnot front\n---");
    expect(stripFrontmatter("---\nunclosed\nbody")).toBe("---\nunclosed\nbody");
    expect(stripFrontmatter("")).toBe("");
  });
});

describe("textStats", () => {
  it("counts words, ignoring Markdown punctuation", () => {
    expect(textStats("").words).toBe(0);
    expect(textStats("   \n\t ").words).toBe(0);
    expect(textStats("Hello world").words).toBe(2);
    expect(textStats("# Title\n\nSome **bold** text.").words).toBe(4);
    expect(textStats("don't stop").words).toBe(2);
    expect(textStats("- one\n- two\n- three").words).toBe(3);
  });

  it("counts languages without spaces as several words", () => {
    expect(textStats("日本語のテキストです").words).toBeGreaterThan(1);
  });

  it("counts characters as code points, whitespace included", () => {
    expect(textStats("abc").characters).toBe(3);
    expect(textStats("a b\nc").characters).toBe(5);
    expect(textStats("😀😀").characters).toBe(2); // not 4 UTF-16 units
    expect(textStats("").characters).toBe(0);
  });

  it("switches to a fast whitespace count for very large texts", () => {
    const big = "word ".repeat(FAST_PATH_CHARS / 5 + 10);
    const stats = textStats(big);
    expect(stats.words).toBe(FAST_PATH_CHARS / 5 + 10);
    expect(stats.characters).toBe(big.length);
  });
});

describe("noteStats", () => {
  it("ignores the frontmatter block", () => {
    const note = "---\nid: 123e4567-e89b-12d3-a456-426614174000\ntags: [a, b]\n---\nOne two three";
    expect(noteStats(note)).toEqual({ words: 3, characters: 13 });
  });
});

describe("fast path consistency", () => {
  it("counts code points and words like the normal path would for plain text", () => {
    const text = "word 😀 other\n".repeat(FAST_PATH_CHARS / 10);
    const fast = textStats(text);
    const per = textStats("word 😀 other\n");
    const n = FAST_PATH_CHARS / 10;
    expect(fast.characters).toBe(per.characters * n);
    expect(fast.words).toBeGreaterThanOrEqual(2 * n); // whitespace words; the emoji counts as one
  });
});
