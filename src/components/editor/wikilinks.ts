import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import {
  type EditorState,
  type Extension,
  RangeSetBuilder,
  StateEffect,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  keymap,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";

/** A `[[wiki link]]` in the text. `target` is the part before any `|display`. */
export interface LinkSpan {
  from: number;
  to: number;
  target: string;
}

const WIKILINK = /\[\[([^[\]|\n]+?)(?:\|[^[\]\n]*)?\]\]/g;

/** Wiki links in one line of text (positions are offset by `offset`). */
export function wikiLinksIn(text: string, offset = 0): LinkSpan[] {
  const spans: LinkSpan[] = [];
  for (const m of text.matchAll(WIKILINK)) {
    const target = (m[1] ?? "").trim();
    if (target === "") continue; // `[[ ]]` points nowhere (the backend ignores it too)
    const from = offset + (m.index ?? 0);
    spans.push({ from, to: from + m[0].length, target });
  }
  return spans;
}

/** True if `pos` is inside inline code or a code block (links and tags do not count there). */
export function inCode(state: EditorState, pos: number): boolean {
  for (let node = syntaxTree(state).resolveInner(pos, 1); node.parent; node = node.parent) {
    if (/Code/.test(node.name)) return true;
  }
  return false;
}

/** The wiki link under (or touching) `pos`, outside code. */
export function linkAt(state: EditorState, pos: number): LinkSpan | null {
  const line = state.doc.lineAt(pos);
  const hit = wikiLinksIn(line.text, line.from).find((s) => pos >= s.from && pos <= s.to);
  return hit && !inCode(state, hit.from) ? hit : null;
}

// ------------------------------------------------------------------ highlighting

/** Link targets (as written) known to point at no note; styled as broken. */
export const setBrokenTargets = StateEffect.define<ReadonlySet<string>>();

const brokenField = StateField.define<ReadonlySet<string>>({
  create: () => new Set(),
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setBrokenTargets)) return e.value;
    return value;
  },
});

const normal = Decoration.mark({ class: "cm-wikilink" });
const broken = Decoration.mark({ class: "cm-wikilink cm-wikilink-broken" });

function decorate(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const known = view.state.field(brokenField);
  for (const { from, to } of view.visibleRanges) {
    for (let pos = from; pos <= to;) {
      const line = view.state.doc.lineAt(pos);
      for (const s of wikiLinksIn(line.text, line.from)) {
        if (inCode(view.state, s.from)) continue;
        builder.add(s.from, s.to, known.has(s.target) ? broken : normal);
      }
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

const highlighter = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = decorate(view);
    }
    update(u: ViewUpdate) {
      const changed = u.transactions.some((tr) => tr.effects.some((e) => e.is(setBrokenTargets)));
      if (u.docChanged || u.viewportChanged || changed) this.decorations = decorate(u.view);
    }
  },
  { decorations: (v) => v.decorations },
);

// -------------------------------------------------------------------- completion

export interface NoteName {
  title: string;
  path: string;
}

/** Suggestions after `[[`: note titles (path-qualified when two notes share a title). */
export function wikiLinkSource(getNotes: () => readonly NoteName[]) {
  return (ctx: CompletionContext): CompletionResult | null => {
    const m = ctx.matchBefore(/\[\[[^[\]\n|]*/);
    if (!m) return null;
    const notes = getNotes();
    const count = new Map<string, number>();
    for (const n of notes)
      count.set(n.title.toLowerCase(), (count.get(n.title.toLowerCase()) ?? 0) + 1);
    const closing = ctx.state.sliceDoc(ctx.pos, ctx.pos + 2) === "]]" ? "" : "]]";
    const options: Completion[] = notes.map((n) => {
      const ambiguous = (count.get(n.title.toLowerCase()) ?? 0) > 1;
      const label = ambiguous ? n.path.replace(/\.md$/i, "") : n.title;
      return {
        label,
        detail: ambiguous ? undefined : folderOf(n.path),
        apply: label + closing,
        type: "text",
      };
    });
    return { from: m.from + 2, options, validFor: /^[^[\]\n|]*$/ };
  };
}

const folderOf = (path: string) =>
  path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : undefined;

/** Suggestions after `#`: existing tags. Needs a typed character, so `# Heading` stays quiet. */
export function tagSource(getTags: () => readonly string[]) {
  return (ctx: CompletionContext): CompletionResult | null => {
    const m = ctx.matchBefore(/(?:^|[\s([{"'])#[\p{L}\p{N}_/-]*/u);
    if (!m) return null;
    const hash = m.text.lastIndexOf("#");
    if (m.text.length - hash - 1 === 0 && !ctx.explicit) return null;
    if (inCode(ctx.state, ctx.pos)) return null;
    return {
      from: m.from + hash + 1,
      options: getTags().map((name) => ({ label: name, type: "keyword" })),
      validFor: /^[\p{L}\p{N}_/-]*$/u,
    };
  };
}

// ------------------------------------------------------------------------ wiring

export interface WikiLinkOptions {
  /** Open the note a link points at (or offer to create it). */
  onFollow: (target: string) => void;
  getNotes: () => readonly NoteName[];
  getTags: () => readonly string[];
}

/** Highlighting, Mod-click / Mod-Enter navigation and `[[` / `#` completion. */
export function wikiLinkExtensions(options: WikiLinkOptions): Extension {
  return [
    brokenField,
    highlighter,
    EditorView.domEventHandlers({
      mousedown(event, view) {
        if (!(event.ctrlKey || event.metaKey) || event.button !== 0) return false;
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
        const link = pos === null ? null : linkAt(view.state, pos);
        if (!link) return false;
        event.preventDefault();
        options.onFollow(link.target);
        return true;
      },
    }),
    keymap.of([
      {
        key: "Mod-Enter",
        run(view) {
          const link = linkAt(view.state, view.state.selection.main.head);
          if (!link) return false;
          options.onFollow(link.target);
          return true;
        },
      },
    ]),
    autocompletion({
      override: [wikiLinkSource(options.getNotes), tagSource(options.getTags)],
      icons: false,
    }),
  ];
}
