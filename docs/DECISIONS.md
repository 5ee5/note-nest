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
- Between the hash check and the rename in a save there is a tiny window in which an external write could
  be lost; closing it needs OS-level locking. (File watching, added in Milestone 5, does not close it.)
- Renaming, moving, deleting or restoring a folder reindexes the whole vault (since Milestone 5 a pass
  skips reading files it can prove unchanged).
- Symbolic links inside a vault are ignored and never followed.
- Files that are not valid UTF-8 are listed but cannot be opened, so they cannot be altered.
- Windows and macOS filesystem behaviour (case-insensitive names, rename-over-existing) is not covered by
  tests because CI only runs on Linux.

## Milestone 3: Markdown editor

- **CodeMirror 6 directly, no React wrapper.** The editor and toolbar are lazily loaded chunks; the language
  packs for fenced code load on demand. The editor store stays the source of truth: user edits flow in,
  external changes (reload from disk, draft restore) flow out as minimal edits that keep the cursor.
- **Frontmatter is shown folded as a "Properties" chip** and can be expanded and edited. An id guard (client
  side, mirroring the backend) restores the note's `id` if an edit would drop or damage it.
- **Line endings are preserved.** A note that uses CRLF throughout stays CRLF (CodeMirror would otherwise
  normalize it to LF on the first save).
- **Preview is rendered in Rust** (`pulldown-cmark` then an `ammonia` allowlist), as the spec places Markdown
  processing in the backend, and the sanitizer stays outside the webview. Rules: no scripts, handlers,
  iframes, forms or styles (the only inline CSS allowed is exact `text-align` on table cells); links keep only
  `http`, `https`, `mailto` and `#fragments`; relative note links are inert until Milestone 4; images are
  never fetched from the network. Relative image paths are resolved inside the vault (no `..` escapes, no
  symlinks), checked by content, limited to 8 MB each / 32 MB per note, and inlined as `data:` URIs, which
  avoids enabling Tauri's asset protocol. Task-list items render as read-only glyphs, not `<input>`s.
- **The browser mock has no Markdown engine**; its preview shows the escaped text. The real renderer is
  covered by Rust tests (including XSS vectors) and CI.
- **Image attachments** go to `<vault>/Attachments/<name>-001.<ext>` (the spec's layout), referenced by a path
  relative to the note. The type is decided by content (PNG, JPEG, GIF, WebP, SVG), never by name; limit
  25 MiB. The folder is hidden from the folder list and reserved. Bytes travel to Rust as a raw IPC body
  (metadata in headers), not a JSON array. The `image` crate is not needed yet.
- **Two platform changes:** `tauri-plugin-opener`, scoped to `http`, `https` and `mailto`, opens external
  links; `dragDropEnabled: false` lets dropped files reach the page, and a window-level guard cancels any drop
  outside the editor so the webview can never navigate to a dropped file.
- **Shortcuts:** Mod-B, Mod-I, Mod-Shift-X (strikethrough), Mod-E (code), Mod-Alt-1/2/3 (headings),
  Mod-Shift-7/8/9 (numbered, bulleted, checklist), Mod-F (find and replace), Mod-S (save now), Tab/Shift-Tab
  in lists. Mod-K stays with the command palette, so there is no link shortcut.
- **Large-note safeguards** (found by profiling a 5 MB note): the Markdown parser re-parses a whole paragraph
  on every edit, so a note over 8 MB or with an unbroken block over 100 KB opens as plain text (a large paste
  triggers the same check); the preview of a note over 2 million characters renders on request; counting
  trails typing for notes over 50 000 characters.

### Known limitations (Milestone 3)

- No scroll synchronization between the editor and the preview.
- Preview checkboxes are display-only; links to other notes do nothing until Milestone 4.
- The plain-text safeguard is checked when a note opens and on large single edits, not while typing a huge
  block character by character.
- Orphaned attachments (an image whose reference was deleted) are not cleaned up yet.
- Table editing is limited to inserting a table; there is no cell navigation.
- Clipboard paste and file drop, the opener plugin, and the raw-body attachment call have been verified in
  Chromium and by compiling the Tauri crate, but not in the real desktop webviews (WebKitGTK, WebView2,
  WKWebView).

## Milestone 4: graph view

- **Favorites and recently opened** were built with the storage engine (the pin star, `last_opened_at`, and
  the two sidebar views); Milestone 4 added nothing to them.
- **Graph rendering:** `d3-force` (one small dependency) lays the graph out, run to completion up front rather
  than animated, so the result is deterministic and unit-tested without a browser. A plain `<canvas>` draws
  it, with pan, zoom, hover highlighting, click to select and double-click to open. No graph UI library.
- **Placement:** the graph is a sidebar view (`{ kind: "graph" }`) that replaces the editor pane; the note
  explorer stays beside it listing every note, and selecting a note there recentres the local graph.
- **Two scopes:** "Around this note" (depth 1 to 3, backed by a neighbourhood query, so it stays fast in a
  large vault) and "Whole vault". A vault of more than 1 500 notes is not drawn until the user asks
  (`GRAPH_DRAW_LIMIT`), because 60 000 edges are unreadable and a 5 000-note layout blocks the page.
- **Mutual links** are drawn as one line; there are no arrowheads.

### Known limitations (graph view)

- The canvas is not keyboard-navigable; it has a text label with the counts, and the explorer remains the
  accessible way to reach every note. A fuller accessibility pass belongs to Milestone 5.
- The layout is recomputed on every change to the note list, so editing a note while a very large graph is
  open redraws it from scratch.
- Verified in Chromium against the sample vault (6 notes); not yet looked at on a large, dense real vault or
  in the desktop webviews.

## Milestone 5: watching the vault for outside changes

- **Events are hints; the reindex is the truth.** `notify` (the crate the spec names) watches the vault
  recursively. Events are never applied one by one, because the OS can drop, merge or reorder them and a
  rename can arrive as an unrelated delete and create. Each burst (quiet for 250 ms, at most 2 s) triggers
  the normal incremental reindex, which compares the whole vault with the index. A missed event is caught
  by the next pass, and the UI also asks for one (`reconcile`) whenever the window regains focus. This is the
  spec's "reconciliation mechanism" for missed or coalesced events.
- **Ignored:** anything under a hidden path (`.notenest/` with the index and drafts, `.git/`, `.nn-*.tmp`
  temp files, editor swap files) and access-only events. NoteNest's own saves do trigger a pass, which finds
  them already indexed and reports nothing.
- **Scan cache.** So a pass is cheap, the workspace remembers each note's size, modification time, id and
  hash from the last read. A file whose size and time are unchanged, and whose index row still matches, is
  not read again. A file read less than 2 s after it was modified is always read again (timestamps can be
  coarse, so two writes in one tick would otherwise look identical). On 5 000 notes a pass with nothing
  changed takes about 90 ms, against about 800 ms for the full read on open. The cache is in memory, so
  opening a vault still reads every file.
- **Duplicate ids.** When a copied note shares an id with another, the note the index already knows under
  that id keeps it (before, it was the first in sorted order, which could re-id the original note while it
  was open). A duplicate modified less than 2 s ago is left alone and retried, because rewriting a file that
  is still being copied would cut the copy short.
- **What the UI does** (`src/lib/vaultSync.ts`). It reloads the note lists, so tags, search, backlinks
  and the graph follow. The open note:
  - **Changed, nothing unsaved:** it reloads in place as a minimal edit, so the cursor stays put.
  - **Changed, unsaved text:** the existing conflict state (autosave paused, "Keep my version" or "Load
    the version on disk") plus a draft.
  - **Gone, nothing unsaved:** it closes with a notice. If exactly one note disappeared and one appeared,
    that is taken as a rename without an id in the frontmatter, and the editor follows it.
  - **Gone, unsaved text:** the new `missing` state. Autosave stops (saving would recreate a file the user
    deleted), the text stays in the editor and in recovery drafts, and the banner offers "Save as a new
    note" or "Close and keep a recovery copy". `stash_draft` keeps updating an existing draft after its note
    disappeared.
  - **Other files changed** (attachments): the preview re-renders.
- **Without a watcher.** If the OS watcher cannot start (e.g. the Linux inotify watch limit), opening still
  succeeds. A notice says outside changes are picked up when the window is focused.

### Known limitations (file watching)

- A note renamed outside NoteNest without an `id` in its frontmatter gets a new id, so it loses its
  `last_opened_at` (as before). The editor follows such a rename only when it is unambiguous.
- Network and some FUSE filesystems may deliver no events; focus reconciliation is the fallback.
- The tiny window between a save's hash check and its rename (see Milestone 2) remains.
- Verified with real inotify events in tests on Linux, and by compiling the Tauri crate. The desktop app
  (event delivery to the webview, FSEvents on macOS, ReadDirectoryChangesW on Windows) has not been run.

## Milestone 5: import and export

- **Import copies, never moves or overwrites.**
  - Files and folders are copied into the folder being viewed (or the vault root). A taken name gets a
    " 2", " 3", ... suffix, and a folder import always creates a new folder named after the source.
  - An imported note whose frontmatter `id` is already used in the vault gets a fresh id, so the original
    keeps its identity (and its backlinks).
  - Hidden files and folders (`.obsidian/`, `.git/`) and symbolic links are not copied. Files over 200 MB
    are skipped.
  - Importing the vault itself, a folder inside it, or a folder that contains it is refused.
- **Imported files bring their attachments.** Local files referenced with standard Markdown links or
  images (`![](img/a.png)`, `[pdf](../docs/x.pdf)`) are copied along. They keep the same relative path when
  that lands inside the vault and is free. Otherwise they go to `Attachments/` with the link rewritten.
  References that go through a hidden path component (`../.ssh/...`) are never followed. A folder import
  needs none of this, because the whole tree keeps its structure.
- **Export writes outside the vault only.**
  - The vault is exported into a new folder, `<vault name> export <date>`, inside the chosen folder, and
    existing files are never overwritten. Hidden entries (including `.notenest/`) are left out.
  - A single note goes to the file picked in a save dialog, which asks before replacing. Its attachments go
    next to it at the same relative path, or under `Attachments/` (renamed if taken, with the link
    rewritten) when the path would climb out of the export folder.
- **Standard Markdown** (on by default; a checkbox in Settings turns it off for an exact copy):
  - **Frontmatter:** NoteNest's own keys (`id`, `pinned`, `archived`) are removed and other frontmatter
    (tags, aliases) is kept. A block that held only our keys is dropped.
  - **Wiki links, vault export:** `[[Note#Heading|shown]]` becomes `[shown](relative/Note.md#heading)` with
    a percent-encoded path and a GitHub-style heading anchor.
  - **Wiki links, single-note export:** links become plain text, because the linked notes are not exported.
  - **Unresolved links** become their text.
- **Where:** Settings has an "Import and export" section, and the command palette has the same four
  commands. Both appear in the desktop app only, because they need native file dialogs
  (`dialog:allow-save` was added to the capabilities).
- **No ZIP, HTML or PDF.** A plain folder is the most standard format. The spec leaves HTML and PDF for
  later.

### Known limitations (import and export)

- Embeds (`![[image.png]]`) and same-note links (`[[#Heading]]`) are left as written on export.
  Reference-style links (`[x][ref]`) are not followed for attachments.
- After a standard export and an import back, links are regular Markdown links, which NoteNest does not
  count as wiki links (no backlinks or graph edges for them).
- Only `.md` files count as notes, so `.markdown` and `.txt` are imported as plain files by a folder import
  and refused by a file import.
- Import and export paths come from native dialogs in the webview. The backend refuses paths inside the vault,
  but otherwise trusts them. This is part of the Milestone 5 security review.

## Milestone 5: backup and recovery

- **A backup is an exact copy of the vault** in a new `<vault> backup <date time>` folder inside a folder the
  user chooses (Settings > Backup, or the command palette). It reuses the export code with standard Markdown
  off, so ids, frontmatter and attachments are preserved, and it never overwrites an earlier backup.
- **Verified:** after copying, every file is read back and compared with its original. A mismatch is an
  error, and the "last backup" time is only recorded after a clean check.
- **Restoring is opening the copy as a vault** (Settings > Change vault), which keeps every note id. To merge
  a backup into an existing vault, import its folder (duplicate ids get new ones).
- **Manual only.** There is no schedule and no background job, so nothing runs without the user asking. Settings
  shows when the last backup was made.
- **Already in place from earlier milestones:** the index rebuilds from the files, a damaged index is moved
  aside, unsaved text is kept as drafts, deleted notes go to the trash, and overwritten or link-edited
  files are copied to `.notenest/recovery/` first.

### Known limitations (backup)

- The trash and `.notenest/recovery/` are not part of a backup (hidden folders are skipped). Recent
  deletions are not in it.
- A backup inside a cloud-synced or same-drive folder protects against mistakes, not drive failure.
- No pruning of old backups; the user deletes them.
