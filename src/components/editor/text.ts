import { Annotation, Transaction, type EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

/** Marks transactions that come from outside the user's typing (loading, resync). */
export const External = Annotation.define<boolean>();

/**
 * CodeMirror normalizes line breaks, so a CRLF file would silently become LF on the
 * first save. A note that uses CRLF throughout keeps it by configuring the editor
 * with that separator; anything else uses LF.
 */
export function detectLineBreak(text: string): "\r\n" | "\n" {
  return text.includes("\r\n") && !/(^|[^\r])\n/.test(text) ? "\r\n" : "\n";
}

/** The file text exactly as it should be written (the state's own line breaks). */
export function docText(state: EditorState): string {
  return state.sliceDoc(0);
}

const toLf = (s: string) => s.replace(/\r\n/g, "\n");

/** Smallest single replacement turning `a` into `b` (strings use `\n` line breaks). */
export function diffChange(
  a: string,
  b: string,
): { from: number; to: number; insert: string } | null {
  if (a === b) return null;
  let start = 0;
  const max = Math.min(a.length, b.length);
  while (start < max && a.charCodeAt(start) === b.charCodeAt(start)) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a.charCodeAt(endA - 1) === b.charCodeAt(endB - 1)) {
    endA--;
    endB--;
  }
  // Never cut a surrogate pair in half.
  const isHigh = (s: string, i: number) => {
    const c = s.charCodeAt(i);
    return c >= 0xd800 && c <= 0xdbff;
  };
  if (start > 0 && isHigh(a, start - 1)) start--;
  if (
    endA < a.length &&
    !isHigh(a, endA - 1) &&
    a.charCodeAt(endA) >= 0xdc00 &&
    a.charCodeAt(endA) <= 0xdfff
  ) {
    endA++;
    endB++;
  }
  return { from: start, to: endA, insert: b.slice(start, endB) };
}

/**
 * Brings the editor's text to `text` with the smallest possible change, so the
 * cursor, scroll position and selection survive. Not added to the undo history and
 * not reported back as a user edit.
 */
export function syncDocument(view: EditorView, text: string): void {
  const current = toLf(docText(view.state));
  const next = toLf(text);
  const change = diffChange(current, next);
  if (!change) return;
  view.dispatch({
    changes: {
      from: change.from,
      to: change.to,
      insert: change.insert.replace(/\n/g, view.state.lineBreak),
    },
    annotations: [External.of(true), Transaction.addToHistory.of(false)],
  });
}
