import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { foldKeymap } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { search, searchKeymap } from "@codemirror/search";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightSpecialChars, keymap } from "@codemirror/view";
import { formatKeymap } from "./format";
import { imageHandlers } from "./images";
import { frontmatterFolding, frontmatterIdGuard } from "./frontmatter";
import { detectLineBreak } from "./text";
import { editorHighlighting, editorTheme } from "./theme";

export interface EditorOptions {
  /** Mod-S: save right now (autosave still runs). */
  onSave: () => void;
  /** Extra extensions (formatting keymap, update listener, ...). */
  extensions?: Extension[];
}

/** Builds the editor state for a note's full file text. */
export function createEditorState(doc: string, options: EditorOptions): EditorState {
  const lineBreak = detectLineBreak(doc);
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
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      frontmatterFolding,
      frontmatterIdGuard,
      imageHandlers,
      search({ top: true }),
      keymap.of(formatKeymap),
      keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, ...foldKeymap]),
      editorTheme,
      editorHighlighting,
    ],
  });
}
