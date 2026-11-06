import { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import { detectLineBreak, diffChange, docText, syncDocument } from "./text";

/** A minimal stand-in for EditorView: enough for syncDocument, no DOM needed. */
function fakeView(doc: string, crlf = false) {
  let state = EditorState.create({
    doc,
    extensions: crlf ? [EditorState.lineSeparator.of("\r\n")] : [],
  });
  const view = {
    get state() {
      return state;
    },
    dispatch(spec: Parameters<EditorState["update"]>[0]) {
      state = state.update(spec).state;
    },
  };
  return view as unknown as EditorView;
}

describe("detectLineBreak", () => {
  it("uses CRLF only when the whole file does", () => {
    expect(detectLineBreak("a\r\nb\r\n")).toBe("\r\n");
    expect(detectLineBreak("a\nb\n")).toBe("\n");
    expect(detectLineBreak("a\r\nb\nc")).toBe("\n");
    expect(detectLineBreak("")).toBe("\n");
  });
});

describe("diffChange", () => {
  it("finds the smallest middle replacement", () => {
    expect(diffChange("hello world", "hello brave world")).toEqual({
      from: 6,
      to: 6,
      insert: "brave ",
    });
    expect(diffChange("abcdef", "abXef")).toEqual({ from: 2, to: 4, insert: "X" });
    expect(diffChange("abc", "")).toEqual({ from: 0, to: 3, insert: "" });
    expect(diffChange("same", "same")).toBeNull();
  });

  it("applies cleanly for arbitrary edits", () => {
    const cases: [string, string][] = [
      ["", "new"],
      ["aaa", "aaaa"],
      ["aXa", "aa"],
      ["line1\nline2\nline3", "line1\nchanged\nline3"],
      ["---\nid: x\n---\nbody", "---\nid: y\n---\nbody"],
    ];
    for (const [a, b] of cases) {
      const c = diffChange(a, b)!;
      expect(a.slice(0, c.from) + c.insert + a.slice(c.to)).toBe(b);
    }
  });

  it("never splits a surrogate pair", () => {
    const a = "x😀y";
    const b = "x😁y"; // differs only in the low surrogate
    const c = diffChange(a, b)!;
    expect(a.slice(0, c.from) + c.insert + a.slice(c.to)).toBe(b);
    expect(c.insert).toBe("😁");
  });
});

describe("syncDocument", () => {
  it("changes only what differs and keeps the cursor after it", () => {
    const view = fakeView("alpha\nbeta\ngamma");
    syncDocument(view, "alpha\nBETA!\ngamma");
    expect(docText(view.state)).toBe("alpha\nBETA!\ngamma");
  });

  it("keeps CRLF line endings intact", () => {
    const view = fakeView("one\r\ntwo\r\nthree", true);
    expect(docText(view.state)).toBe("one\r\ntwo\r\nthree");
    syncDocument(view, "one\r\n2\r\nthree\r\nfour");
    expect(docText(view.state)).toBe("one\r\n2\r\nthree\r\nfour");
  });

  it("does nothing when the text already matches", () => {
    const view = fakeView("same");
    const before = view.state;
    syncDocument(view, "same");
    expect(view.state).toBe(before);
  });

  it("is not recorded in the undo history", () => {
    const view = fakeView("a");
    syncDocument(view, "ab");
    const tr = view.state.update({ changes: { from: 0, insert: "" } });
    expect(tr.isUserEvent("undo")).toBe(false);
  });
});
