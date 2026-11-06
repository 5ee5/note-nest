# NoteNest

Local-first, privacy-focused Markdown note-taking and personal knowledge management app.
Markdown files are the source of truth; no accounts or cloud services required.

**Status:** Milestone 2 (storage engine): notes are real Markdown files in a vault folder, with autosave, folders, trash and crash recovery. The editor is a plain textarea until Milestone 3.

## Stack

Rust, Tauri 2, React, TypeScript, Tailwind CSS, Zustand, SQLite (sqlx), pnpm. (CodeMirror 6 arrives in Milestone 3.)

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
