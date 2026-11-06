import { EditorState, Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { findFrontmatter, frontmatterIdGuard } from "./frontmatter";
import { External } from "./text";

const ID = "123e4567-e89b-12d3-a456-426614174000";
const NOTE = `---\ntitle: Keep me\nid: ${ID}\n---\n# Body\n`;

const state = (doc: string, crlf = false) =>
  EditorState.create({
    doc,
    extensions: [frontmatterIdGuard, ...(crlf ? [EditorState.lineSeparator.of("\r\n")] : [])],
  });

describe("findFrontmatter", () => {
  it("locates the block, the id and the body start", () => {
    const fm = findFrontmatter(Text.of(NOTE.split("\n")))!;
    expect(fm.id).toBe(ID);
    expect(fm.closeLine).toBe(4);
    expect(NOTE.slice(fm.after)).toBe("# Body\n");
    expect(NOTE.slice(fm.idLine!.from, fm.idLine!.to)).toBe(`id: ${ID}`);
  });

  it("accepts quoted ids, BOM and the ... closer", () => {
    const fm = findFrontmatter(Text.of(["﻿---", `id: "${ID.toUpperCase()}"`, "...", "x"]))!;
    expect(fm.id).toBe(ID);
  });

  it("returns null without a closed block", () => {
    for (const doc of ["# Hi", "---\nno closer", "---", "", "text\n---\nid: x\n---"]) {
      expect(findFrontmatter(Text.of(doc.split("\n")))).toBeNull();
    }
  });

  it("reports an invalid id line without a usable id", () => {
    const fm = findFrontmatter(Text.of(["---", "id: not-a-uuid", "---"]))!;
    expect(fm.id).toBeNull();
    expect(fm.idLine).not.toBeNull();
  });
});

describe("frontmatterIdGuard", () => {
  const apply = (s: EditorState, from: number, to: number, insert: string) =>
    s.update({ changes: { from, to, insert }, userEvent: "input" }).state.doc.toString();

  it("lets ordinary edits through untouched", () => {
    const s = state(NOTE);
    expect(apply(s, NOTE.length, NOTE.length, "more")).toBe(NOTE + "more");
    expect(apply(s, 0, 0, "")).toBe(NOTE);
  });

  it("restores the id after select-all and paste", () => {
    const out = apply(state(NOTE), 0, NOTE.length, "replaced everything");
    expect(out).toBe(`---\nid: ${ID}\n---\nreplaced everything`);
  });

  it("restores a damaged or edited id line", () => {
    const s = state(NOTE);
    const idStart = NOTE.indexOf(`id: ${ID}`);
    expect(apply(s, idStart + 4, idStart + 8, "ZZZZ")).toBe(NOTE);
    // Deleting just the text leaves the user's blank line; the id is put back after it.
    expect(apply(s, idStart, idStart + `id: ${ID}`.length, "")).toBe(
      `---\ntitle: Keep me\n\nid: ${ID}\n---\n# Body\n`,
    );
  });

  it("re-adds a deleted id inside a surviving block and keeps other keys", () => {
    const doc = `---\ntitle: x\nid: ${ID}\n---\nB`;
    const lineStart = doc.indexOf("id:");
    const out = apply(state(doc), lineStart, lineStart + `id: ${ID}\n`.length, "");
    expect(out).toBe(`---\ntitle: x\nid: ${ID}\n---\nB`);
  });

  it("uses the document's line breaks when it must insert", () => {
    const doc = `---\r\nid: ${ID}\r\n---\r\nBody`;
    const s = state(doc, true);
    const out = s.update({ changes: { from: 0, to: s.doc.length, insert: "x" } }).state;
    expect(out.sliceDoc(0)).toBe(`---\r\nid: ${ID}\r\n---\r\nx`);
  });

  it("leaves notes without an id alone", () => {
    const plain = state("---\ntitle: x\n---\nBody");
    expect(apply(plain, 0, plain.doc.length, "free text")).toBe("free text");
    const none = state("just text");
    expect(apply(none, 0, 9, "other")).toBe("other");
  });

  it("does not interfere with programmatic (external) replacements", () => {
    const s = state(NOTE);
    const out = s.update({
      changes: { from: 0, to: s.doc.length, insert: "from disk" },
      annotations: External.of(true),
    }).state;
    expect(out.doc.toString()).toBe("from disk");
  });
});
