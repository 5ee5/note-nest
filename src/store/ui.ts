import { create } from "zustand";
import { applyTheme, loadTheme, saveTheme, type Theme } from "../lib/theme";
import type { SortKey, View } from "../lib/notes";

interface UiState {
  theme: Theme;
  view: View;
  sort: SortKey;
  query: string;
  selectedNoteId: string | null;
  sidebarCollapsed: boolean;
  explorerCollapsed: boolean;
  paletteOpen: boolean;
  settingsOpen: boolean;
  /** Incremented to ask the explorer to focus its search box. */
  searchFocusTick: number;

  setTheme: (theme: Theme) => void;
  setView: (view: View) => void;
  setSort: (sort: SortKey) => void;
  setQuery: (query: string) => void;
  selectNote: (id: string | null) => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  setExplorerCollapsed: (collapsed: boolean) => void;
  setPaletteOpen: (open: boolean) => void;
  setSettingsOpen: (open: boolean) => void;
  focusSearch: () => void;
}

export const useUi = create<UiState>((set) => ({
  theme: loadTheme(),
  view: { kind: "all" },
  sort: "updated",
  query: "",
  selectedNoteId: null,
  sidebarCollapsed: false,
  explorerCollapsed: false,
  paletteOpen: false,
  settingsOpen: false,
  searchFocusTick: 0,

  setTheme: (theme) => {
    saveTheme(theme);
    applyTheme(theme);
    set({ theme });
  },
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
}));
