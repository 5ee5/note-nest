# Architecture

## Backend

- `crates/notenest-core` holds all application logic and must not depend on Tauri.
- `src-tauri` is a thin adapter: Tauri commands delegate to `notenest-core`.
- Capabilities are minimal (`core:default`); the CSP is restrictive.
- Markdown files on disk are the authoritative note storage (not yet implemented; Milestone 2).

## Frontend (`src/`)

- `lib/api`: the `NotesApi` interface and the in-memory mock. The only seam to the backend.
- `store/notes.ts`: notes data (created with `createNotesStore(api)` so tests can inject an API).
- `store/ui.ts`: UI state (theme, view, sort, search query, panel collapse, dialogs).
- `lib/notes.ts`: pure view filtering/sorting; `lib/theme.ts`: theme resolution and persistence;
  `lib/shortcuts.ts`: platform-aware shortcut matching; `lib/actions.ts`: actions shared by shortcuts and
  the command palette.
- `components/`: `AppLayout` (resizable/collapsible panels via react-resizable-panels), `Sidebar`,
  `NoteExplorer`, `EditorPane` (placeholder), `CommandPalette` (cmdk), `SettingsDialog` (Radix Dialog).
- Theming uses semantic CSS variables in `index.css` selected by `data-theme` on `<html>`; components use
  only semantic Tailwind colors, so new themes only redefine variables.

Decisions that deviate from the spec are recorded in `docs/DECISIONS.md`.
