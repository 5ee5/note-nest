import { redo, undo } from "@codemirror/commands";
import { EditorSelection, type EditorState, type Line, type StateCommand } from "@codemirror/state";
import type { EditorView, KeyBinding } from "@codemirror/view";

/*
 * Formatting commands. Each is a CodeMirror `StateCommand`: it reads a state and
 * dispatches a transaction, so it can be bound to a key, called from the toolbar, and
 * unit-tested without a browser. Line breaks are always taken from the state
 * (`state.lineBreak`) so CRLF notes stay CRLF.
 */

// ---------------------------------------------------------------- inline marks

function countAround(state: EditorState, from: number, to: number, ch: string): [number, number] {
  let before = 0;
  while (from - before > 0 && state.sliceDoc(from - before - 1, from - before) === ch) before++;
  let after = 0;
  while (to + after < state.doc.length && state.sliceDoc(to + after, to + after + 1) === ch)
    after++;
  return [before, after];
}

/** Range a mark applies to: the selection without edge spaces, or the word at the cursor. */
function markTarget(
  state: EditorState,
  from: number,
  to: number,
): { from: number; to: number } | null {
  if (from === to) {
    const word = state.wordAt(from);
    return word ? { from: word.from, to: word.to } : null;
  }
  while (from < to && /\s/.test(state.sliceDoc(from, from + 1))) from++;
  while (to > from && /\s/.test(state.sliceDoc(to - 1, to))) to--;
  return from < to ? { from, to } : null;
}

interface Emphasis {
  /** Asterisks on the leading side that belong to this emphasis (0-3). */
  n: number;
  lead: number;
  trail: number;
  core: string;
  outerFrom: number;
  outerTo: number;
}

function emphasisAt(state: EditorState, from: number, to: number): Emphasis {
  const text = state.sliceDoc(from, to);
  const allStars = /^\*+$/.test(text);
  const a = allStars ? 0 : (/^\*+/.exec(text)?.[0].length ?? 0);
  const b = allStars ? 0 : (/\*+$/.exec(text)?.[0].length ?? 0);
  const [p, q] = countAround(state, from, to, "*");
  const lead = a + p;
  const trail = b + q;
  return {
    n: Math.min(lead, trail, 3),
    lead,
    trail,
    core: text.slice(a, text.length - b),
    outerFrom: from - p,
    outerTo: to + q,
  };
}

/** Bold (`weight` 2) or italic (1) with asterisks, aware of `**` and `***` runs. */
function toggleAsterisks(weight: 1 | 2): StateCommand {
  return ({ state, dispatch }) => {
    const tr = state.changeByRange((range) => {
      const target = markTarget(state, range.from, range.to);
      if (!target) {
        const marks = "*".repeat(weight);
        return {
          changes: { from: range.from, to: range.to, insert: marks + marks },
          range: EditorSelection.cursor(range.from + marks.length),
        };
      }
      const e = emphasisAt(state, target.from, target.to);
      const next =
        weight === 2 ? (e.n >= 2 ? e.n - 2 : e.n + 2) : e.n % 2 === 1 ? e.n - 1 : e.n + 1;
      const delta = next - e.n;
      const leadOut = e.lead + delta;
      const insert = "*".repeat(leadOut) + e.core + "*".repeat(e.trail + delta);
      return {
        changes: { from: e.outerFrom, to: e.outerTo, insert },
        range: EditorSelection.range(e.outerFrom + leadOut, e.outerFrom + leadOut + e.core.length),
      };
    });
    dispatch(state.update(tr, { scrollIntoView: true, userEvent: "input.format" }));
    return true;
  };
}

/** Marks that must match exactly on both sides (`~~strike~~`, `` `code` ``). */
function toggleLiteral(marker: string): StateCommand {
  const m = marker.length;
  return ({ state, dispatch }) => {
    const tr = state.changeByRange((range) => {
      const target = markTarget(state, range.from, range.to);
      if (!target) {
        return {
          changes: { from: range.from, to: range.to, insert: marker + marker },
          range: EditorSelection.cursor(range.from + m),
        };
      }
      const { from, to } = target;
      const text = state.sliceDoc(from, to);
      if (text.length >= 2 * m && text.startsWith(marker) && text.endsWith(marker)) {
        const inner = text.slice(m, text.length - m);
        return {
          changes: { from, to, insert: inner },
          range: EditorSelection.range(from, from + inner.length),
        };
      }
      if (state.sliceDoc(from - m, from) === marker && state.sliceDoc(to, to + m) === marker) {
        return {
          changes: { from: from - m, to: to + m, insert: text },
          range: EditorSelection.range(from - m, from - m + text.length),
        };
      }
      return {
        changes: { from, to, insert: marker + text + marker },
        range: EditorSelection.range(from + m, from + m + text.length),
      };
    });
    dispatch(state.update(tr, { scrollIntoView: true, userEvent: "input.format" }));
    return true;
  };
}

export const toggleBold = toggleAsterisks(2);
export const toggleItalic = toggleAsterisks(1);
export const toggleStrike = toggleLiteral("~~");
export const toggleInlineCode = toggleLiteral("`");

// ---------------------------------------------------------------- line formats

export type LineKind = "bullet" | "number" | "check" | "quote";

const LINE_PATTERNS: [LineKind, RegExp][] = [
  ["check", /^(\s*)[-*+] \[[ xX]\] /],
  ["bullet", /^(\s*)[-*+] (?!\[[ xX]\] )/],
  ["number", /^(\s*)\d+[.)] /],
  ["quote", /^(\s*)> ?/],
];

export function lineKind(text: string): LineKind | null {
  for (const [kind, re] of LINE_PATTERNS) if (re.test(text)) return kind;
  return null;
}

function selectedLines(state: EditorState): Line[] {
  const seen = new Set<number>();
  const lines: Line[] = [];
  for (const r of state.selection.ranges) {
    const last = state.doc.lineAt(r.to).number;
    for (let n = state.doc.lineAt(r.from).number; n <= last; n++) {
      if (!seen.has(n)) {
        seen.add(n);
        lines.push(state.doc.line(n));
      }
    }
  }
  return lines;
}

/**
 * Applies line-prefix changes so that a cursor at the start of a line ends up after the
 * new prefix (CodeMirror would otherwise leave it before the inserted text).
 */
function withCursorAfterPrefix(
  state: EditorState,
  changes: { from: number; to?: number; insert: string }[],
) {
  const set = state.changes(changes);
  return state.update({
    changes: set,
    selection: state.selection.map(set, 1),
    scrollIntoView: true,
    userEvent: "input.format",
  });
}

function toggleLineKind(kind: LineKind): StateCommand {
  return ({ state, dispatch }) => {
    let lines = selectedLines(state);
    const nonEmpty = lines.filter((l) => l.text.trim() !== "");
    if (nonEmpty.length > 0) lines = nonEmpty;
    const remove = lines.every((l) => lineKind(l.text) === kind);
    let counter = 0;
    const changes = lines.map((line) => {
      const indent = /^\s*/.exec(line.text)?.[0] ?? "";
      const existing = LINE_PATTERNS.map(([, re]) => re.exec(line.text)).find(Boolean);
      const oldLen = existing ? existing[0].length : indent.length;
      counter += 1;
      const marker = { bullet: "- ", check: "- [ ] ", quote: "> ", number: `${counter}. ` }[kind];
      return { from: line.from, to: line.from + oldLen, insert: remove ? indent : indent + marker };
    });
    dispatch(withCursorAfterPrefix(state, changes));
    return true;
  };
}

export const toggleBulletList = toggleLineKind("bullet");
export const toggleNumberedList = toggleLineKind("number");
export const toggleChecklist = toggleLineKind("check");
export const toggleQuote = toggleLineKind("quote");

/** Sets the heading level of the selected lines; the same level again removes it. */
export function setHeading(level: 0 | 1 | 2 | 3 | 4 | 5 | 6): StateCommand {
  return ({ state, dispatch }) => {
    const lines = selectedLines(state);
    const changes = lines.map((line) => {
      const m = /^(#{1,6}) +/.exec(line.text);
      const current = m ? (m[1] ?? "").length : 0;
      const prefix = level === 0 || current === level ? "" : `${"#".repeat(level)} `;
      return { from: line.from, to: line.from + (m ? m[0].length : 0), insert: prefix };
    });
    dispatch(withCursorAfterPrefix(state, changes));
    return true;
  };
}

// ------------------------------------------------------------------ list indent

function markerWidth(text: string): number {
  const m = /^(\s*)(?:[-*+] (?:\[[ xX]\] )?|\d+[.)] )/.exec(text);
  if (!m) return 0;
  const body = m[0].slice((m[1] ?? "").length);
  // A nested item must line up under its parent's text; `- [ ] ` nests like `- `.
  return /^[-*+] /.test(body) ? 2 : body.length;
}

function indentLines(direction: 1 | -1): StateCommand {
  return ({ state, dispatch }) => {
    const lines = selectedLines(state).filter((l) => markerWidth(l.text) > 0);
    if (lines.length === 0) return false; // not in a list: let Tab move focus
    const changes = lines.map((line) => {
      const width = markerWidth(line.text);
      if (direction === 1) return { from: line.from, insert: " ".repeat(width) };
      const lead = /^[ \t]*/.exec(line.text)?.[0] ?? "";
      const remove = lead.startsWith("\t") ? 1 : Math.min(width, lead.length);
      return { from: line.from, to: line.from + remove, insert: "" };
    });
    dispatch(state.update({ changes, userEvent: "input.indent" }));
    return true;
  };
}

export const indentListItem = indentLines(1);
export const outdentListItem = indentLines(-1);

// ----------------------------------------------------------------------- blocks

export const insertCodeBlock: StateCommand = ({ state, dispatch }) => {
  const nl = state.lineBreak;
  const r = state.selection.main;
  const first = state.doc.lineAt(r.from);
  const last = state.doc.lineAt(r.to);
  if (r.empty && first.text.trim() === "") {
    const insert = `\`\`\`${nl}${nl}\`\`\``;
    dispatch(
      state.update({
        changes: { from: first.from, to: first.to, insert },
        selection: EditorSelection.cursor(first.from + 3 + nl.length),
        scrollIntoView: true,
        userEvent: "input.format",
      }),
    );
    return true;
  }
  const from = r.empty ? first.to : first.from;
  const to = r.empty ? first.to : last.to;
  const body = r.empty ? "" : state.sliceDoc(from, to);
  const lead = r.empty ? nl + nl : "";
  const insert = `${lead}\`\`\`${nl}${body}${nl}\`\`\``;
  const bodyStart = from + lead.length + 3 + nl.length;
  dispatch(
    state.update({
      changes: { from, to, insert },
      selection: EditorSelection.range(bodyStart, bodyStart + body.length),
      scrollIntoView: true,
      userEvent: "input.format",
    }),
  );
  return true;
};

export const insertHorizontalRule: StateCommand = ({ state, dispatch }) => {
  const nl = state.lineBreak;
  const line = state.doc.lineAt(state.selection.main.to);
  const blank = line.text.trim() === "";
  // On an empty line the rule replaces it; otherwise it goes after the current line,
  // separated by a blank line so it is not read as a setext heading underline.
  const insert = blank ? `---${nl}` : `${nl}${nl}---${nl}`;
  const from = blank ? line.from : line.to;
  dispatch(
    state.update({
      changes: { from, to: line.to, insert },
      selection: EditorSelection.cursor(from + insert.length),
      scrollIntoView: true,
      userEvent: "input.format",
    }),
  );
  return true;
};

export const insertTable: StateCommand = ({ state, dispatch }) => {
  const nl = state.lineBreak;
  const line = state.doc.lineAt(state.selection.main.to);
  const blank = line.text.trim() === "";
  const lead = blank ? "" : nl + nl;
  const header = "Column 1";
  const table = [`| ${header} | Column 2 |`, "| --- | --- |", "|  |  |"].join(nl);
  const from = blank ? line.from : line.to;
  dispatch(
    state.update({
      changes: { from, to: line.to, insert: lead + table },
      selection: EditorSelection.range(
        from + lead.length + 2,
        from + lead.length + 2 + header.length,
      ),
      scrollIntoView: true,
      userEvent: "input.format",
    }),
  );
  return true;
};

/** `[text](url)`: wraps the selection as link text, or uses a selected URL as the target. */
export const insertLink: StateCommand = ({ state, dispatch }) => {
  const r = state.selection.main;
  const selected = state.sliceDoc(r.from, r.to);
  if (/^https?:\/\/\S+$/.test(selected)) {
    const insert = `[link text](${selected})`;
    dispatch(
      state.update({
        changes: { from: r.from, to: r.to, insert },
        selection: EditorSelection.range(r.from + 1, r.from + 1 + "link text".length),
        userEvent: "input.format",
      }),
    );
    return true;
  }
  const text = selected === "" ? "text" : selected;
  const insert = `[${text}](url)`;
  const urlStart = r.from + text.length + 3;
  dispatch(
    state.update({
      changes: { from: r.from, to: r.to, insert },
      selection: EditorSelection.range(urlStart, urlStart + 3),
      userEvent: "input.format",
    }),
  );
  return true;
};

/** Inserts text (e.g. an image reference) at the cursor, replacing any selection. */
export function insertText(text: string): StateCommand {
  return ({ state, dispatch }) => {
    const r = state.selection.main;
    const insert = text.replace(/\n/g, state.lineBreak);
    dispatch(
      state.update({
        changes: { from: r.from, to: r.to, insert },
        selection: EditorSelection.cursor(r.from + insert.length),
        scrollIntoView: true,
        userEvent: "input.paste",
      }),
    );
    return true;
  };
}

// ------------------------------------------------------------------ active state

export interface ActiveFormats {
  bold: boolean;
  italic: boolean;
  strike: boolean;
  code: boolean;
  heading: 0 | 1 | 2 | 3 | 4 | 5 | 6;
  list: LineKind | null;
}

export const NO_FORMATS: ActiveFormats = {
  bold: false,
  italic: false,
  strike: false,
  code: false,
  heading: 0,
  list: null,
};

/** Which formats apply at the main selection (for pressed states in the toolbar). */
export function activeFormats(state: EditorState): ActiveFormats {
  const r = state.selection.main;
  const line = state.doc.lineAt(r.from);
  const heading = /^(#{1,6}) /.exec(line.text);
  const target = markTarget(state, r.from, r.to);
  let bold = false;
  let italic = false;
  let strike = false;
  let code = false;
  if (target) {
    const e = emphasisAt(state, target.from, target.to);
    bold = e.n >= 2;
    italic = e.n % 2 === 1;
    const text = state.sliceDoc(target.from, target.to);
    const around = (m: string) =>
      (text.length >= 2 * m.length && text.startsWith(m) && text.endsWith(m)) ||
      (state.sliceDoc(target.from - m.length, target.from) === m &&
        state.sliceDoc(target.to, target.to + m.length) === m);
    strike = around("~~");
    code = around("`");
  }
  return {
    bold,
    italic,
    strike,
    code,
    heading: heading ? ((heading[1] ?? "").length as ActiveFormats["heading"]) : 0,
    list: lineKind(line.text),
  };
}

// ----------------------------------------------------------------------- keymap

/** Shortcuts for the editor. Mod-K is deliberately not used: the app owns it (command palette). */
export const formatKeymap: KeyBinding[] = [
  { key: "Mod-b", run: toggleBold, preventDefault: true },
  { key: "Mod-i", run: toggleItalic, preventDefault: true },
  { key: "Mod-Shift-x", run: toggleStrike, preventDefault: true },
  { key: "Mod-e", run: toggleInlineCode, preventDefault: true },
  { key: "Mod-Alt-1", run: setHeading(1), preventDefault: true },
  { key: "Mod-Alt-2", run: setHeading(2), preventDefault: true },
  { key: "Mod-Alt-3", run: setHeading(3), preventDefault: true },
  { key: "Mod-Shift-7", run: toggleNumberedList, preventDefault: true },
  { key: "Mod-Shift-8", run: toggleBulletList, preventDefault: true },
  { key: "Mod-Shift-9", run: toggleChecklist, preventDefault: true },
  { key: "Tab", run: indentListItem },
  { key: "Shift-Tab", run: outdentListItem },
];

/** Runs a command against a live view. */
export function runCommand(view: EditorView, command: StateCommand): boolean {
  return command({ state: view.state, dispatch: view.dispatch });
}

export { redo, undo };
