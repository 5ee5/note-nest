# NoteNest

Local-first, privacy-focused Markdown note-taking and personal knowledge management app.
Markdown files are the source of truth; no accounts or cloud services required.

**Status:** Milestone 3 (Markdown editor): notes are real Markdown files in a vault folder, edited in a CodeMirror editor with formatting tools, live preview, image attachments and statistics, plus autosave, folders, trash and crash recovery. Tags, links and search arrive in Milestone 4.

## Stack

Rust, Tauri 2, React, TypeScript, Tailwind CSS, Zustand, SQLite (sqlx), pnpm. CodeMirror 6, pulldown-cmark and ammonia.

## Layout

- `crates/notenest-core` — Tauri-independent core logic
- `src-tauri` — thin Tauri shell exposing core to the UI
- `src` — React frontend

## Development

Prerequisites: Rust (stable), Node >= 20.19, pnpm 10, and the
[Tauri 2 system dependencies](https://v2.tauri.app/start/prerequisites/)
(on Linux, WebKitGTK 4.1).

```sh
pnpm install
pnpm tauri dev      # run the desktop app
pnpm dev            # frontend only, in a browser
pnpm lint && pnpm test && pnpm build
scripts/docker-check.sh 'cargo clippy --workspace --all-targets -- -D warnings'   # no WebKitGTK needed
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

## License

MIT
