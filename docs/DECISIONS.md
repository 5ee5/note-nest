# Decisions

Short records of decisions that deviate from, or interpret, `docs/PROJECT_SPEC.md`.

## CI runs on Forgejo Actions, not GitHub Actions

The spec lists GitHub Actions. The repository is hosted on a Forgejo instance, so the workflow lives in
`.forgejo/workflows/ci.yml`. It runs a single Linux job in a `node:22-bookworm` container (macOS/Windows
are not covered until matching runners exist).

## Milestone 1 uses an in-memory notes backend

The spec puts the storage engine in Milestone 2. Milestone 1 therefore talks to a `NotesApi` interface
(`src/lib/api/types.ts`) implemented by an in-memory mock with clearly labelled sample data. Milestone 2
provides a Tauri-backed implementation of the same interface.

## Theme preference lives in localStorage until Milestone 2

The spec stores settings in SQLite, which does not exist yet. The theme is kept in `localStorage`
(accessed through `safeStorage`, which never throws). Milestone 2 should migrate it to the settings table.

## shadcn-style components are written by hand

The shadcn/ui CLI is network-dependent and changes often. The few components needed (dialog, command
palette) are built directly on Radix Dialog and cmdk with Tailwind classes, which keeps dependencies small.

## Frontend test tooling is pinned for Node 20

Vitest 4 and happy-dom are used (jsdom 31 and `@testing-library/jest-dom` need Node 22). TypeScript is
pinned to ~5.9 because typescript-eslint does not support TS 7.

## Milestone 2: storage engine

- **Identity and state live in the note's frontmatter.** A note's `id`, `pinned` and `archived` are stored
  as top-level keys of an optional `---` block, so they survive an index rebuild and external renames. A
  hand-written parser owns only those three keys and preserves everything else byte for byte (no YAML
  round-trip). Notes created by NoteNest carry an `id`; notes created elsewhere are left untouched until they
  are pinned or archived. Until then their id is derived from their path (UUIDv5), so it is stable across
  rebuilds but changes if the file is renamed or moved.
- **`save_note` keeps the frontmatter `id`.** If a note had an `id` on disk and the new text drops or changes
  it, the original is restored and the written content is returned to the editor.
- **The SQLite index is derived data** at `<vault>/.notenest/index.db` and can always be rebuilt from the
  files (`reindex`, which also runs on open). A corrupt or unmigratable index is moved aside (never deleted)
  and rebuilt. Only `last_opened_at` is not recoverable; it is convenience state.
- **Schema.** The first migration creates `notes`, `folders` and `settings`. The spec's `tags`, `note_tags`
  and `note_links` tables arrive in the Milestone 4 migration together with the features that use them.
- **sqlx 0.8** is used rather than 0.9 (0.9 requires a newer Rust and is brand new). Queries use the runtime
  API, so builds need no database.
- **Delete is move-to-trash** (`<vault>/.notenest/trash/<id>/`), restorable, never a hard delete.
- **No silent overwrites.** Saves carry the content hash the editor last saw; a mismatch returns a conflict
  and writes nothing. "Keep my version" first backs up the on-disk file to `.notenest/recovery/conflicts/`.
- **Crash recovery.** The editor stashes unsaved text to `.notenest/recovery/drafts/` on a short debounce
  (300 ms) and autosaves on a longer one (800 ms). Drafts are offered back after an unexpected exit and are
  deleted only once the same text is saved or the user discards them. Stale `.nn-*.tmp` files from
  interrupted writes are cleaned up on open.
- **Settings.** The theme is stored in the `settings` table; `localStorage` remains as a flash-free cache.
- **Temporary editor.** A plain `<textarea>` stands in until CodeMirror arrives in Milestone 3. It shows the
  whole file, including frontmatter.
- **Vault location.** The last vault path is kept in the OS app-config directory; the default suggestion is
  `~/NoteNest`, with a folder picker (`tauri-plugin-dialog`).

### Known limitations (Milestone 2)

- Opening a vault re-reads and hashes every note (O(vault size)); fine for ordinary vaults, to be optimized
  in Milestone 5.
- There is no file watching yet (Milestone 5). External edits are detected when a note is opened or saved,
  and by reindexing on open. Between the hash check and the rename in a save there is a tiny window in which
  an external write could be lost; closing it needs OS-level locking.
- Renaming, moving, deleting or restoring a folder reindexes the whole vault.
- Symbolic links inside a vault are ignored and never followed.
- Files that are not valid UTF-8 are listed but cannot be opened, so they cannot be altered.
- Windows and macOS filesystem behaviour (case-insensitive names, rename-over-existing) is not covered by
  tests because CI only runs on Linux.
