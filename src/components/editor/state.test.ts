import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { longestBlock, MAX_BLOCK_CHARS, MAX_HIGHLIGHT_CHARS, shouldUseMarkdown } from "./state";

const doc = (s: string) => Text.of(s.split("\n"));

describe("longestBlock", () => {
  it("measures runs of non-blank lines", () => {
    expect(longestBlock(doc(""))).toBe(0);
    expect(longestBlock(doc("abc"))).toBe(4);
    expect(longestBlock(doc("ab\ncd\n\nlonger line here"))).toBe(17);
    expect(longestBlock(doc("a\n\n\n\nb"))).toBe(2);
  });

  it("treats whitespace-only lines as blank", () => {
    expect(longestBlock(doc("aaaa\n   \nbb"))).toBe(5);
  });
});

describe("shouldUseMarkdown", () => {
  it("accepts ordinary and long-but-structured notes", () => {
    expect(shouldUseMarkdown(doc("# Title\n\nSome text."))).toBe(true);
    const structured = Array.from({ length: 5000 }, () => "para line\n\n").join("");
    expect(shouldUseMarkdown(doc(structured))).toBe(true);
  });

  it("refuses one enormous unbroken block", () => {
    const log = Array.from({ length: MAX_BLOCK_CHARS / 10 + 10 }, () => "log line..").join("\n");
    expect(shouldUseMarkdown(doc(log))).toBe(false);
  });

  it("refuses documents beyond the size limit", () => {
    const line = "x".repeat(99) + "\n\n";
    const huge = line.repeat(Math.ceil(MAX_HIGHLIGHT_CHARS / line.length) + 1);
    expect(shouldUseMarkdown(doc(huge))).toBe(false);
  });
});
