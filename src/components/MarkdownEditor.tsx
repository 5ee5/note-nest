import { useEffect, useRef, useState } from "react";
import { EditorView } from "@codemirror/view";
import { undoDepth, redoDepth } from "@codemirror/commands";
import type { EditorState } from "@codemirror/state";
import { textStats } from "../lib/stats";
import { followLink } from "../lib/actions";
import { useEditor } from "../store/editor";
import { useEditorView } from "../store/editorView";
import { useLinks } from "../store/links";
import { useNotes } from "../store/notes";
import { useUi } from "../store/ui";
import { activeFormats } from "./editor/format";
import { foldFrontmatter, findFrontmatter } from "./editor/frontmatter";
import { createEditorState } from "./editor/state";
import { docText, External, syncDocument } from "./editor/text";
import { setBrokenTargets } from "./editor/wikilinks";

/** Selections beyond this size are not counted (it would cost more than it helps). */
const MAX_COUNTED_SELECTION = 500_000;

function selectionStats(state: EditorState) {
  const ranges = state.selection.ranges.filter((r) => !r.empty);
  if (ranges.length === 0) return null;
  const size = ranges.reduce((n, r) => n + (r.to - r.from), 0);
  if (size > MAX_COUNTED_SELECTION) return { words: 0, characters: size };
  return textStats(ranges.map((r) => state.sliceDoc(r.from, r.to)).join("\n"));
}

function publishState(state: EditorState): void {
  useEditorView.getState().publish({
    selection: selectionStats(state),
    formats: activeFormats(state),
    canUndo: undoDepth(state) > 0,
    canRedo: redoDepth(state) > 0,
  });
}

interface Props {
  /** The note's full file text (the editor is the source while typing). */
  value: string;
  /** Called with the full text after every user edit. */
  onChange: (text: string) => void;
  /** Mod-S. */
  onSave: () => void;
  /** Gives the parent the view (for the toolbar) and `null` on unmount. */
  onView?: (view: EditorView | null) => void;
}

/**
 * CodeMirror 6 editing surface. Mount one per document (use `key`): each gets its own
 * undo history. Changes made elsewhere (e.g. reloading the disk version) arrive through
 * `value` and are applied as a minimal edit that keeps the cursor.
 */
export default function MarkdownEditor({ value, onChange, onSave, onView }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const lastEmitted = useRef<string>(value);
  const callbacks = useRef({ onChange, onSave, onView });
  const [initial] = useState(value);

  useEffect(() => {
    callbacks.current = { onChange, onSave, onView };
  });

  useEffect(() => {
    const state = createEditorState(initial, {
      onSave: () => callbacks.current.onSave(),
      wikiLinks: {
        onFollow: (target) => void followLink(target),
        getNotes: () => useNotes.getState().notes,
        getTags: () => useNotes.getState().tagCounts.map((t) => t.name),
      },
      onPlainMode: () =>
        useUi
          .getState()
          .notify(
            "info",
            "This note is very large or has a huge unbroken block, so Markdown highlighting is off to keep typing fast.",
          ),
      extensions: [
        EditorView.updateListener.of((u) => {
          if (u.docChanged || u.selectionSet) publishState(u.state);
          if (!u.docChanged || u.transactions.some((tr) => tr.annotation(External))) return;
          const text = docText(u.state);
          lastEmitted.current = text;
          callbacks.current.onChange(text);
        }),
      ],
    });
    const view = new EditorView({ state, parent: host.current! });
    viewRef.current = view;
    foldFrontmatter(view);
    // A note with nothing but its properties is new: let the user start typing.
    const fm = findFrontmatter(view.state.doc);
    if (initial.trim() === "" || (fm && fm.after >= view.state.doc.length)) view.focus();
    // Style links that point at nothing, using what the links panel already knows.
    const syncBroken = () => {
      const links = useLinks.getState();
      const current = links.noteId !== null && links.noteId === useEditor.getState().noteId;
      const targets = current
        ? (links.data?.outgoing ?? []).filter((o) => !o.target).map((o) => o.targetText)
        : [];
      view.dispatch({ effects: setBrokenTargets.of(new Set(targets)) });
    };
    syncBroken();
    const unsubscribeLinks = useLinks.subscribe(syncBroken);
    callbacks.current.onView?.(view);
    useEditorView.getState().setView(view);
    publishState(view.state);
    return () => {
      unsubscribeLinks();
      useEditorView.getState().setView(null);
      callbacks.current.onView?.(null);
      view.destroy();
      viewRef.current = null;
    };
  }, [initial]);

  // Text changed from outside (not by typing): apply it with a minimal edit.
  useEffect(() => {
    const view = viewRef.current;
    if (!view || value === lastEmitted.current) return;
    lastEmitted.current = value;
    syncDocument(view, value);
  }, [value]);

  return <div ref={host} className="h-full min-h-0" />;
}
