import type { EditorView } from "@codemirror/view";
import { create } from "zustand";
import type { ActiveFormats } from "../components/editor/format";

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
  setView: (view: EditorView | null) => void;
  /** Publishes selection-dependent state for the toolbar (only when it changed). */
  publish: (next: { formats: ActiveFormats; canUndo: boolean; canRedo: boolean }) => void;
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
  setView: (view) =>
    set(view ? { view } : { view: null, formats: NO_FORMATS, canUndo: false, canRedo: false }),
  publish: ({ formats, canUndo, canRedo }) => {
    const s = get();
    if (sameFormats(s.formats, formats) && s.canUndo === canUndo && s.canRedo === canRedo) return;
    set({ formats, canUndo, canRedo });
  },
}));
