import { EditorState, StateField, type Text } from "@codemirror/state";
import { codeFolding, foldEffect, foldService } from "@codemirror/language";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
import { External } from "./text";

/** A note's leading `---` block, located in editor coordinates. */
export interface FrontmatterBlock {
  /** Start of the block (always 0). */
  from: number;
  /** End of the closing delimiter line. */
  to: number;
  /** Start of the body after the block. */
  after: number;
  /** Line number of the closing delimiter. */
  closeLine: number;
  /** The note's id if the block has a valid `id:` key. */
  id: string | null;
  /** The `id:` line (valid or not), if the block has one. */
  idLine: { from: number; to: number } | null;
}

const MAX_SCAN_LINES = 500;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Mirrors the backend's frontmatter rules (see crates/notenest-core/src/frontmatter.rs). */
export function findFrontmatter(doc: Text): FrontmatterBlock | null {
  if (doc.lines < 2 || doc.line(1).text.replace(/^\uFEFF/, "") !== "---") return null;
  const last = Math.min(doc.lines, MAX_SCAN_LINES);
  for (let n = 2; n <= last; n++) {
    const close = doc.line(n);
    if (close.text !== "---" && close.text !== "...") continue;
    let id: string | null = null;
    let idLine: FrontmatterBlock["idLine"] = null;
    for (let m = 2; m < n; m++) {
      const line = doc.line(m);
      const match = /^id:\s*(.*)$/.exec(line.text);
      if (!match) continue;
      const value = (match[1] ?? "").trim().replace(/^["']|["']$/g, "");
      id = UUID.test(value) ? value.toLowerCase() : null;
      idLine = { from: line.from, to: line.to };
      break;
    }
    return {
      from: 0,
      to: close.to,
      after: Math.min(close.to + 1, doc.length),
      closeLine: n,
      id,
      idLine,
    };
  }
  return null;
}

/**
 * NoteNest owns a note's `id`. If an edit would drop or damage it (select-all and
 * paste, deleting the block, editing the line), it is put back, so the buffer always
 * matches what the backend would write anyway.
 */
export const frontmatterIdGuard = EditorState.transactionFilter.of((tr) => {
  if (!tr.docChanged || tr.annotation(External)) return tr;
  const before = findFrontmatter(tr.startState.doc);
  if (!before?.id) return tr;
  const after = findFrontmatter(tr.newDoc);
  if (after?.id === before.id) return tr;

  const nl = tr.startState.lineBreak;
  const line = `id: ${before.id}`;
  const change = after?.idLine
    ? { from: after.idLine.from, to: after.idLine.to, insert: line }
    : after
      ? { from: tr.newDoc.line(after.closeLine).from, insert: line + nl }
      : { from: 0, insert: `---${nl}${line}${nl}---${nl}` };
  return [tr, { changes: change, sequential: true }];
});

const frontmatterLine = Decoration.line({ class: "cm-frontmatter" });

const frontmatterDecorations = StateField.define<DecorationSet>({
  create: (state) => decorate(state),
  update: (value, tr) => (tr.docChanged ? decorate(tr.state) : value),
  provide: (f) => EditorView.decorations.from(f),
});

function decorate(state: EditorState): DecorationSet {
  const fm = findFrontmatter(state.doc);
  if (!fm) return Decoration.none;
  const ranges = [];
  for (let n = 1; n <= fm.closeLine; n++)
    ranges.push(frontmatterLine.range(state.doc.line(n).from));
  return Decoration.set(ranges);
}

/** The frontmatter block can be folded into a single "Properties" chip. */
export const frontmatterFolding = [
  foldService.of((state, lineStart) => {
    if (lineStart !== 0) return null;
    const fm = findFrontmatter(state.doc);
    return fm ? { from: fm.from, to: fm.to } : null;
  }),
  codeFolding({
    preparePlaceholder: (state, range) =>
      range.from === 0 && findFrontmatter(state.doc)?.to === range.to ? "Properties" : "…",
    placeholderDOM: (_view, onclick, prepared: string) => {
      const el = document.createElement("span");
      el.className = "cm-foldPlaceholder";
      el.textContent = prepared === "Properties" ? "Properties ▸" : "…";
      el.setAttribute("role", "button");
      el.title = "Show";
      el.onclick = onclick;
      return el;
    },
  }),
  frontmatterDecorations,
];

/** Folds the block and moves the cursor to the body (call once after creating the view). */
export function foldFrontmatter(view: EditorView): void {
  const fm = findFrontmatter(view.state.doc);
  if (!fm) return;
  view.dispatch({
    effects: foldEffect.of({ from: fm.from, to: fm.to }),
    selection: { anchor: fm.after },
  });
}
