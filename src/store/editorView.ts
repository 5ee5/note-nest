import type { EditorView } from "@codemirror/view";
import { create } from "zustand";
import type { ActiveFormats } from "../components/editor/format";
import type { TextStats } from "../lib/stats";

const NO_FORMATS: ActiveFormats = {
  bold: false,
  italic: false,
  strike: false,
  code: false,
  heading: 0,
  list: null,
};

interface EditorViewState {
  /** The live CodeMirror view, or null when no note is open in the editor. */
  view: EditorView | null;
  formats: ActiveFormats;
  canUndo: boolean;
  canRedo: boolean;
  /** Counts for the current selection, or null when nothing is selected. */
  selection: TextStats | null;
  setView: (view: EditorView | null) => void;
  /** Publishes selection-dependent state for the toolbar (only when it changed). */
  publish: (next: {
    formats: ActiveFormats;
    canUndo: boolean;
    canRedo: boolean;
    selection: TextStats | null;
  }) => void;
}

const sameFormats = (a: ActiveFormats, b: ActiveFormats) =>
  a.bold === b.bold &&
  a.italic === b.italic &&
  a.strike === b.strike &&
  a.code === b.code &&
  a.heading === b.heading &&
  a.list === b.list;

/** Bridge between the CodeMirror view (in a lazily loaded chunk) and the toolbar. */
export const useEditorView = create<EditorViewState>((set, get) => ({
  view: null,
  formats: NO_FORMATS,
  canUndo: false,
  canRedo: false,
  selection: null,
  setView: (view) =>
    set(
      view
        ? { view }
        : { view: null, formats: NO_FORMATS, canUndo: false, canRedo: false, selection: null },
    ),
  publish: ({ formats, canUndo, canRedo, selection }) => {
    const s = get();
    const sameSelection =
      s.selection?.words === selection?.words && s.selection?.characters === selection?.characters;
    if (
      sameFormats(s.formats, formats) &&
      s.canUndo === canUndo &&
      s.canRedo === canRedo &&
      sameSelection
    ) {
      return;
    }
    set({ formats, canUndo, canRedo, selection });
  },
}));
