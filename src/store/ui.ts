import { create } from "zustand";
import { applyTheme, loadTheme, saveTheme, type Theme } from "../lib/theme";
import type { AffectedNote } from "../lib/api";
import type { SortKey, View } from "../lib/notes";

/** Edit only, edit and preview side by side, or preview only. */
export type EditorMode = "edit" | "split" | "preview";

export function isEditorMode(v: unknown): v is EditorMode {
  return v === "edit" || v === "split" || v === "preview";
}

export interface Notice {
  id: number;
  kind: "error" | "warning" | "info";
  message: string;
}

export interface PromptRequest {
  title: string;
  label: string;
  initial?: string;
  confirmLabel?: string;
  /** Resolve by returning; throw to keep the dialog open and show the message. */
  onSubmit: (value: string) => Promise<void> | void;
}

export type LinkChoice = "update" | "keep" | "cancel";

/** Asks whether links in other notes should follow a rename (see `askLinkUpdate`). */
export interface LinkUpdateRequest {
  heading: string;
  affected: AffectedNote[];
  resolve: (choice: LinkChoice) => void;
}

interface UiState {
  theme: Theme;
  editorMode: EditorMode;
  view: View;
  sort: SortKey;
  query: string;
  selectedNoteId: string | null;
  sidebarCollapsed: boolean;
  explorerCollapsed: boolean;
  paletteOpen: boolean;
  settingsOpen: boolean;
  notices: Notice[];
  prompt: PromptRequest | null;
  linkUpdate: LinkUpdateRequest | null;
  /** Incremented to ask the explorer to focus its search box. */
  searchFocusTick: number;

  setTheme: (theme: Theme) => void;
  setEditorMode: (mode: EditorMode) => void;
  setView: (view: View) => void;
  setSort: (sort: SortKey) => void;
  setQuery: (query: string) => void;
  selectNote: (id: string | null) => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  setExplorerCollapsed: (collapsed: boolean) => void;
  setPaletteOpen: (open: boolean) => void;
  setSettingsOpen: (open: boolean) => void;
  focusSearch: () => void;
  notify: (kind: Notice["kind"], message: string) => void;
  dismissNotice: (id: number) => void;
  openPrompt: (prompt: PromptRequest) => void;
  closePrompt: () => void;
  openLinkUpdate: (request: LinkUpdateRequest) => void;
  closeLinkUpdate: () => void;
}

let noticeCounter = 0;

export const useUi = create<UiState>((set) => ({
  theme: loadTheme(),
  editorMode: "edit",
  view: { kind: "all" },
  sort: "updated",
  query: "",
  selectedNoteId: null,
  sidebarCollapsed: false,
  explorerCollapsed: false,
  paletteOpen: false,
  settingsOpen: false,
  notices: [],
  prompt: null,
  linkUpdate: null,
  searchFocusTick: 0,

  setTheme: (theme) => {
    saveTheme(theme);
    applyTheme(theme);
    set({ theme });
  },
  setEditorMode: (editorMode) => set({ editorMode }),
  setView: (view) => set({ view, query: "" }),
  setSort: (sort) => set({ sort }),
  setQuery: (query) => set({ query }),
  selectNote: (selectedNoteId) => set({ selectedNoteId }),
  setSidebarCollapsed: (sidebarCollapsed) => set({ sidebarCollapsed }),
  setExplorerCollapsed: (explorerCollapsed) => set({ explorerCollapsed }),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  focusSearch: () =>
    set((s) => ({ explorerCollapsed: false, searchFocusTick: s.searchFocusTick + 1 })),
  notify: (kind, message) =>
    set((s) => ({ notices: [...s.notices, { id: ++noticeCounter, kind, message }] })),
  dismissNotice: (id) => set((s) => ({ notices: s.notices.filter((n) => n.id !== id) })),
  openPrompt: (prompt) => set({ prompt }),
  closePrompt: () => set({ prompt: null }),
  openLinkUpdate: (linkUpdate) => set({ linkUpdate }),
  closeLinkUpdate: () => set({ linkUpdate: null }),
}));
