# Architecture

## Backend

- `crates/notenest-core` holds all application logic and must not depend on Tauri. Modules:
  `filesystem` (path validation, atomic writes, trash; the only code that touches the vault),
  `database` (SQLite pool, versioned migrations, all SQL), `frontmatter` (the three keys NoteNest owns),
  `services` (`Workspace`: every note and folder operation; `reindex`; draft `recovery`),
  `config` (last-opened vault), `models`, `errors` (one serializable error enum with stable codes).
- `src-tauri` is a thin adapter: `commands.rs` unwraps arguments and calls `Workspace`; `state.rs` holds the
  open workspace. Capabilities are minimal (`core:default`, `dialog:allow-open`); the CSP is restrictive.
- Markdown files in the vault are the source of truth. `<vault>/.notenest/` holds the derived index
  (`index.db`), the trash and recovery data, and is never treated as notes.
- All mutating operations take one write lock, so internal operations never interleave.

## Frontend (`src/`)

- `lib/api`: the `NotesApi` interface with two implementations: `TauriNotesApi` (the real vault) and
  `MockNotesApi` (in memory; used in the browser and in tests). The only seam to the backend.
- `store/notes.ts`: notes, folders and trash lists (tests inject a backend with `useNotes.setState({ api })`).
- `store/editor.ts`: the open note's buffer, debounced autosave, draft stashing, and conflict/error states.
- `store/vault.ts`, `lib/startup.ts`: choosing and opening a vault; `store/recovery.ts`: drafts to offer back.
- `store/ui.ts`: UI state (theme, view, sort, search query, panel collapse, dialogs).
- `lib/notes.ts`: pure view filtering/sorting; `lib/theme.ts`: theme resolution and persistence;
  `lib/shortcuts.ts`: platform-aware shortcut matching; `lib/actions.ts`: actions shared by shortcuts and
  the command palette.
- `components/`: `AppLayout` (resizable/collapsible panels via react-resizable-panels), `Sidebar`,
  `NoteExplorer`, `EditorPane` (textarea until CodeMirror), `CommandPalette` (cmdk), `SettingsDialog` (Radix Dialog).
- Theming uses semantic CSS variables in `index.css` selected by `data-theme` on `<html>`; components use
  only semantic Tailwind colors, so new themes only redefine variables.

Decisions that deviate from the spec are recorded in `docs/DECISIONS.md`.
