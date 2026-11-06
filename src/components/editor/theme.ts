import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

const MONO =
  'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace';

/** Layout and chrome. Colors come from the app's CSS variables, so themes just work. */
export const editorTheme = EditorView.theme({
  "&": { height: "100%", color: "var(--fg)", backgroundColor: "transparent", fontSize: "15px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-sans)", lineHeight: "1.75", overflow: "auto" },
  ".cm-content": {
    maxWidth: "48rem",
    margin: "0 auto",
    padding: "0.75rem 1.75rem 6rem",
    caretColor: "var(--fg)",
  },
  ".cm-line": { padding: "0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--fg)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
    { backgroundColor: "var(--selection)" },
  ".cm-activeLine": { backgroundColor: "transparent" },
  ".cm-frontmatter": { fontFamily: MONO, fontSize: "0.85em", color: "var(--fg-muted)" },
  ".cm-foldPlaceholder": {
    backgroundColor: "var(--surface)",
    border: "1px solid var(--border)",
    borderRadius: "0.375rem",
    color: "var(--fg-muted)",
    padding: "0 0.5rem",
    fontSize: "0.8em",
    cursor: "pointer",
  },
  ".cm-panels": {
    backgroundColor: "var(--surface)",
    color: "var(--fg)",
    borderColor: "var(--border)",
    fontFamily: "var(--font-sans)",
  },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--border)" },
  ".cm-panel.cm-search": { padding: "0.5rem 0.75rem" },
  ".cm-textfield": {
    backgroundColor: "var(--bg)",
    border: "1px solid var(--border)",
    borderRadius: "0.375rem",
    color: "var(--fg)",
    padding: "0.2rem 0.4rem",
  },
  ".cm-button": {
    backgroundImage: "none",
    backgroundColor: "var(--surface-hover)",
    border: "1px solid var(--border)",
    borderRadius: "0.375rem",
    color: "var(--fg)",
  },
  ".cm-searchMatch": { backgroundColor: "var(--selection)", outline: "1px solid var(--ring)" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--surface-active)" },
});

const highlightStyle = HighlightStyle.define([
  { tag: t.heading1, fontSize: "1.6em", fontWeight: "700" },
  { tag: t.heading2, fontSize: "1.35em", fontWeight: "700" },
  { tag: t.heading3, fontSize: "1.2em", fontWeight: "600" },
  { tag: [t.heading4, t.heading5, t.heading6], fontSize: "1.05em", fontWeight: "600" },
  { tag: t.strong, fontWeight: "700" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strikethrough, textDecoration: "line-through", color: "var(--fg-muted)" },
  { tag: [t.link, t.url], color: "var(--accent)" },
  { tag: t.quote, color: "var(--fg-muted)" },
  { tag: t.monospace, fontFamily: MONO, fontSize: "0.9em" },
  { tag: [t.processingInstruction, t.contentSeparator], color: "var(--fg-muted)" },
  // Code inside fenced blocks
  {
    tag: [t.keyword, t.modifier, t.operatorKeyword, t.controlKeyword],
    color: "var(--syn-keyword)",
  },
  { tag: [t.string, t.regexp, t.special(t.string)], color: "var(--syn-string)" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "var(--syn-number)" },
  {
    tag: [t.comment, t.lineComment, t.blockComment],
    color: "var(--syn-comment)",
    fontStyle: "italic",
  },
  {
    tag: [
      t.function(t.variableName),
      t.definition(t.variableName),
      t.className,
      t.typeName,
      t.tagName,
    ],
    color: "var(--syn-fn)",
  },
  { tag: [t.propertyName, t.attributeName], color: "var(--syn-prop)" },
]);

export const editorHighlighting = syntaxHighlighting(highlightStyle);
