import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { foldKeymap } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, type Extension, type Text } from "@codemirror/state";
import { drawSelection, EditorView, highlightSpecialChars, keymap } from "@codemirror/view";
import { formatKeymap } from "./format";
import { imageHandlers } from "./images";
import { frontmatterFolding, frontmatterIdGuard } from "./frontmatter";
import { detectLineBreak } from "./text";
import { editorHighlighting, editorTheme } from "./theme";
import { wikiLinkExtensions, type WikiLinkOptions } from "./wikilinks";

/** Notes larger than this are edited as plain text (the Markdown parser would slow typing). */
export const MAX_HIGHLIGHT_CHARS = 8_000_000;
/**
 * The longest run of non-blank lines. Markdown re-parses a whole paragraph on every
 * edit inside it, so one enormous unbroken block (a pasted log, say) freezes typing.
 */
export const MAX_BLOCK_CHARS = 100_000;
/** A single edit inserting at least this much triggers a re-check while editing. */
const RECHECK_INSERT_CHARS = 20_000;

/** Length of the longest paragraph-like block (consecutive non-blank lines). */
export function longestBlock(doc: Text): number {
  let longest = 0;
  let current = 0;
  for (let n = 1; n <= doc.lines; n++) {
    const line = doc.line(n);
    if (line.text.trim() === "") {
      current = 0;
    } else {
      current += line.length + 1;
      if (current > longest) longest = current;
    }
  }
  return longest;
}

/** Whether Markdown parsing and highlighting is safe for this document. */
export function shouldUseMarkdown(doc: Text): boolean {
  return doc.length <= MAX_HIGHLIGHT_CHARS && longestBlock(doc) <= MAX_BLOCK_CHARS;
}

const language = new Compartment();
const markdownSupport: Extension = markdown({ base: markdownLanguage, codeLanguages: languages });

export interface EditorOptions {
  /** Mod-S: save right now (autosave still runs). */
  onSave: () => void;
  /** Called once if the note switched to plain text because it is too large or unbroken. */
  onPlainMode?: () => void;
  /** Wiki-link support: highlighting, following, completion. */
  wikiLinks?: WikiLinkOptions;
  /** Extra extensions (formatting keymap, update listener, ...). */
  extensions?: Extension[];
}

/** Builds the editor state for a note's full file text. */
export function createEditorState(doc: string, options: EditorOptions): EditorState {
  const lineBreak = detectLineBreak(doc);
  const text = EditorState.create({ doc }).doc; // only to measure; line breaks do not matter here
  const markdownOk = shouldUseMarkdown(text);
  if (!markdownOk) options.onPlainMode?.();
  return EditorState.create({
    doc,
    extensions: [
      ...(lineBreak === "\r\n" ? [EditorState.lineSeparator.of("\r\n")] : []),
      keymap.of([
        {
          key: "Mod-s",
          preventDefault: true,
          run: () => {
            options.onSave();
            return true;
          },
        },
      ]),
      ...(options.extensions ?? []),
      history(),
      drawSelection(),
      highlightSpecialChars(),
      EditorView.lineWrapping,
      EditorView.contentAttributes.of({ "aria-label": "Note content", spellcheck: "true" }),
      language.of(markdownOk ? markdownSupport : []),
      guardLargeEdits(options),
      frontmatterFolding,
      frontmatterIdGuard,
      imageHandlers,
      ...(options.wikiLinks ? [wikiLinkExtensions(options.wikiLinks)] : []),
      search({ top: true }),
      keymap.of(formatKeymap),
      keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, ...foldKeymap]),
      editorTheme,
      editorHighlighting,
    ],
  });
}

/**
 * If a big paste or edit makes the note unsafe for the Markdown parser, switch it to
 * plain text (once) instead of letting every keystroke freeze the editor.
 */
function guardLargeEdits(options: EditorOptions): Extension {
  return EditorView.updateListener.of((update) => {
    if (!update.docChanged || language.get(update.state) !== markdownSupport) return;
    let inserted = 0;
    update.changes.iterChanges((_fa, _ta, _fb, _tb, text) => {
      inserted += text.length;
    });
    if (inserted < RECHECK_INSERT_CHARS || shouldUseMarkdown(update.state.doc)) return;
    // Not allowed to dispatch while an update is being processed.
    queueMicrotask(() => {
      update.view.dispatch({ effects: language.reconfigure([]) });
      options.onPlainMode?.();
    });
  });
}
