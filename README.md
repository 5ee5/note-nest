# NoteNest

Local-first, privacy-focused Markdown note-taking and personal knowledge management app.
Markdown files are the source of truth; no accounts or cloud services required.

**Status:** Milestone 1 (UI foundation). The UI runs on sample data; the storage engine is not implemented yet.

## Stack

Rust, Tauri 2, React, TypeScript, Tailwind CSS, Zustand, pnpm. (SQLite and CodeMirror 6 arrive in later milestones.)

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
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

## License

MIT
