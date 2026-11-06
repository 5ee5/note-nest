import { EditorSelection, EditorState, type StateCommand } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  activeFormats,
  indentListItem,
  insertCodeBlock,
  insertHorizontalRule,
  insertLink,
  insertTable,
  insertText,
  lineKind,
  outdentListItem,
  setHeading,
  toggleBold,
  toggleBulletList,
  toggleChecklist,
  toggleInlineCode,
  toggleItalic,
  toggleNumberedList,
  toggleQuote,
  toggleStrike,
} from "./format";

/**
 * Tests describe documents with markers: `[text]` is the selection, `|` a cursor.
 * `run` applies a command and returns the result in the same notation.
 */
function parse(marked: string): { doc: string; from: number; to: number } {
  const open = marked.indexOf("[");
  if (open !== -1) {
    const close = marked.indexOf("]", open);
    const doc = marked.slice(0, open) + marked.slice(open + 1, close) + marked.slice(close + 1);
    return { doc, from: open, to: close - 1 };
  }
  const bar = marked.indexOf("|");
  return { doc: marked.slice(0, bar) + marked.slice(bar + 1), from: bar, to: bar };
}

function show(state: EditorState): string {
  const { from, to } = state.selection.main;
  const doc = state.sliceDoc(0);
  return from === to
    ? `${doc.slice(0, from)}|${doc.slice(from)}`
    : `${doc.slice(0, from)}[${doc.slice(from, to)}]${doc.slice(to)}`;
}

function stateOf(marked: string, crlf = false): EditorState {
  const { doc, from, to } = parse(marked);
  return EditorState.create({
    // Positions count a line break as one character, CRLF or not.
    doc: crlf ? doc.replace(/\n/g, "\r\n") : doc,
    selection: EditorSelection.single(from, to),
    extensions: crlf ? [EditorState.lineSeparator.of("\r\n")] : [],
  });
}

/** For documents that contain `[` themselves: explicit selection offsets. */
function runAt(command: StateCommand, doc: string, from: number, to = from): string {
  let state = EditorState.create({ doc, selection: EditorSelection.single(from, to) });
  command({ state, dispatch: (tr) => (state = tr.state) });
  return show(state);
}

function run(command: StateCommand, marked: string, crlf = false): string {
  let state = stateOf(marked, crlf);
  command({ state, dispatch: (tr) => (state = tr.state) });
  return show(state);
}

describe("bold and italic", () => {
  it("wraps a selection and unwraps it again", () => {
    expect(run(toggleBold, "say [hello] now")).toBe("say **[hello]** now");
    expect(run(toggleBold, "say **[hello]** now")).toBe("say [hello] now");
  });

  it("unwraps when the markers are part of the selection", () => {
    expect(run(toggleBold, "say [**hello**] now")).toBe("say [hello] now");
  });

  it("uses the word at the cursor", () => {
    expect(run(toggleBold, "say hel|lo now")).toBe("say **[hello]** now");
  });

  it("inserts an empty pair away from words", () => {
    expect(run(toggleBold, "a | b".replace("| ", "|"))).toBe("a **[b]**"); // next word
    expect(run(toggleItalic, "|")).toBe("*|*");
    expect(run(toggleBold, "|")).toBe("**|**");
  });

  it("keeps edge spaces outside the marks", () => {
    expect(run(toggleBold, "a[ hello ]b")).toBe("a **[hello]** b");
  });

  it("combines italic and bold without confusing the asterisks", () => {
    expect(run(toggleItalic, "**[hi]**")).toBe("***[hi]***");
    expect(run(toggleBold, "***[hi]***")).toBe("*[hi]*");
    expect(run(toggleItalic, "*[hi]*")).toBe("[hi]");
    expect(run(toggleItalic, "***[hi]***")).toBe("**[hi]**");
    expect(run(toggleBold, "*[hi]*")).toBe("***[hi]***");
  });

  it("works on several selections at once", () => {
    let state = EditorState.create({
      doc: "one two",
      selection: EditorSelection.create([EditorSelection.range(0, 3), EditorSelection.range(4, 7)]),
      extensions: [EditorState.allowMultipleSelections.of(true)],
    });
    toggleBold({ state, dispatch: (tr) => (state = tr.state) });
    expect(state.sliceDoc(0)).toBe("**one** **two**");
  });
});

describe("strikethrough and inline code", () => {
  it("toggle exactly their own marks", () => {
    expect(run(toggleStrike, "[gone]")).toBe("~~[gone]~~");
    expect(run(toggleStrike, "~~[gone]~~")).toBe("[gone]");
    expect(run(toggleStrike, "[~~gone~~]")).toBe("[gone]");
    expect(run(toggleInlineCode, "use [cargo] now")).toBe("use `[cargo]` now");
    expect(run(toggleInlineCode, "use `[cargo]` now")).toBe("use [cargo] now");
  });

  it("do not mistake other marks for their own", () => {
    expect(run(toggleStrike, "**[x]**")).toBe("**~~[x]~~**");
    expect(run(toggleInlineCode, "~~[x]~~")).toBe("~~`[x]`~~");
  });
});

describe("headings", () => {
  it("sets, switches and removes the level", () => {
    expect(run(setHeading(2), "|Title")).toBe("## |Title");
    expect(run(setHeading(1), "## T|itle")).toBe("# T|itle");
    expect(run(setHeading(1), "# |Title")).toBe("|Title");
    expect(run(setHeading(0), "### Ti|tle")).toBe("Ti|tle");
  });

  it("applies to every selected line", () => {
    const state = stateOf("[a\nb\nc]");
    let out = state;
    setHeading(3)({ state, dispatch: (tr) => (out = tr.state) });
    expect(out.sliceDoc(0)).toBe("### a\n### b\n### c");
  });
});

describe("lists and quotes", () => {
  const doc = (marked: string, cmd: StateCommand, crlf = false) => {
    let state = stateOf(marked, crlf);
    cmd({ state, dispatch: (tr) => (state = tr.state) });
    return state.sliceDoc(0);
  };

  it("toggles bullets on and off", () => {
    expect(doc("[a\nb]", toggleBulletList)).toBe("- a\n- b");
    expect(doc("[- a\n- b]", toggleBulletList)).toBe("a\nb");
  });

  it("numbers lines sequentially and converts other kinds", () => {
    expect(doc("[a\nb\nc]", toggleNumberedList)).toBe("1. a\n2. b\n3. c");
    expect(doc("[- a\n- b]", toggleNumberedList)).toBe("1. a\n2. b");
    expect(doc("[1. a\n2. b]", toggleBulletList)).toBe("- a\n- b");
    expect(doc("[1. a\n2. b]", toggleNumberedList)).toBe("a\nb");
  });

  it("makes checklists and keeps indentation", () => {
    expect(doc("[a\n  b]", toggleChecklist)).toBe("- [ ] a\n  - [ ] b");
    expect(runAt(toggleChecklist, "- [x] a", 7)).toBe("a|");
    expect(runAt(toggleChecklist, "- [ ] a", 7)).toBe("a|");
    expect(doc("[- a]", toggleChecklist)).toBe("- [ ] a");
  });

  it("quotes lines", () => {
    expect(doc("[a\nb]", toggleQuote)).toBe("> a\n> b");
    expect(doc("[> a\n> b]", toggleQuote)).toBe("a\nb");
  });

  it("skips blank lines inside a selection", () => {
    expect(doc("[a\n\nb]", toggleBulletList)).toBe("- a\n\n- b");
  });

  it("adds a marker to an empty line", () => {
    expect(doc("|", toggleBulletList)).toBe("- ");
  });

  it("recognizes line kinds", () => {
    expect(lineKind("- a")).toBe("bullet");
    expect(lineKind("* a")).toBe("bullet");
    expect(lineKind("- [ ] a")).toBe("check");
    expect(lineKind("12. a")).toBe("number");
    expect(lineKind("> a")).toBe("quote");
    expect(lineKind("plain")).toBeNull();
    expect(lineKind("-not a list")).toBeNull();
  });

  it("leaves CRLF documents CRLF", () => {
    expect(doc("[a\nb]", toggleBulletList, true)).toBe("- a\r\n- b");
  });
});

describe("list indentation", () => {
  const doc = (marked: string, cmd: StateCommand) => {
    let state = stateOf(marked);
    const handled = cmd({ state, dispatch: (tr) => (state = tr.state) });
    return handled ? state.sliceDoc(0) : null;
  };

  it("nests and un-nests list items under their parent's text", () => {
    expect(doc("- a\n- [b]", indentListItem)).toBe("- a\n  - b");
    expect(doc("1. a\n1. [b]", indentListItem)).toBe("1. a\n   1. b");
    expect(doc("- a\n  - [b]", outdentListItem)).toBe("- a\n- b");
    expect(runAt(indentListItem, "- [ ] task", 6)).toBe("  - [ ] |task");
  });

  it("does nothing outside lists so Tab can still move focus", () => {
    expect(doc("plain [text]", indentListItem)).toBeNull();
    expect(doc("not a list|", outdentListItem)).toBeNull();
    expect(doc("- a|", outdentListItem)).toBe("- a"); // already at the top: handled, unchanged
  });
});

describe("blocks and inserts", () => {
  it("inserts a code block on an empty line with the cursor inside", () => {
    expect(run(insertCodeBlock, "|")).toBe("```\n|\n```");
  });

  it("fences selected lines and keeps them selected", () => {
    expect(run(insertCodeBlock, "x\n[let a = 1;\nlet b = 2;]\ny")).toBe(
      "x\n```\n[let a = 1;\nlet b = 2;]\n```\ny",
    );
  });

  it("adds a code block after a non-empty line", () => {
    expect(run(insertCodeBlock, "text|")).toBe("text\n\n```\n|\n```");
  });

  it("uses CRLF inside CRLF notes", () => {
    let state = stateOf("|", true);
    insertCodeBlock({ state, dispatch: (tr) => (state = tr.state) });
    expect(state.sliceDoc(0)).toBe("```\r\n\r\n```");
  });

  it("inserts a horizontal rule separated from the text above", () => {
    expect(run(insertHorizontalRule, "text|")).toBe("text\n\n---\n|");
    expect(run(insertHorizontalRule, "|")).toBe("---\n|");
  });

  it("inserts a table with the first header selected", () => {
    expect(run(insertTable, "|")).toBe("| [Column 1] | Column 2 |\n| --- | --- |\n|  |  |");
    const out = run(insertTable, "para|");
    expect(out.startsWith("para\n\n| [Column 1]")).toBe(true);
  });

  it("builds links around the selection or a pasted URL", () => {
    expect(run(insertLink, "[docs]")).toBe("[docs]([url])");
    expect(run(insertLink, "|")).toBe("[text]([url])");
    expect(run(insertLink, "[https://example.com/a]")).toBe("[[link text]](https://example.com/a)");
  });

  it("inserts arbitrary text at the cursor", () => {
    expect(run(insertText("![](Attachments/image-001.png)"), "a [b] c")).toBe(
      "a ![](Attachments/image-001.png)| c",
    );
  });
});

describe("activeFormats", () => {
  const at = (marked: string) => activeFormats(stateOf(marked));

  it("reports marks around the selection or word", () => {
    expect(at("**[x]**")).toMatchObject({ bold: true, italic: false });
    expect(at("***[x]***")).toMatchObject({ bold: true, italic: true });
    expect(at("*[x]*")).toMatchObject({ bold: false, italic: true });
    expect(at("~~[x]~~")).toMatchObject({ strike: true });
    expect(at("`[x]`")).toMatchObject({ code: true });
    expect(at("plain wo|rd")).toMatchObject({
      bold: false,
      italic: false,
      strike: false,
      code: false,
    });
  });

  it("reports the heading level and list kind of the line", () => {
    expect(at("## ti|tle")).toMatchObject({ heading: 2, list: null });
    expect(
      activeFormats(EditorState.create({ doc: "- [ ] item", selection: { anchor: 8 } })),
    ).toMatchObject({
      heading: 0,
      list: "check",
    });
    expect(at("> qu|ote")).toMatchObject({ list: "quote" });
  });
});
