# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

NoteNest: local-first, offline Markdown note-taking app (Rust + Tauri 2 + React/TypeScript + Tailwind; SQLite and CodeMirror 6 planned). Markdown files on disk are the source of truth. Currently at Milestone 0 (shell only); no notes engine, sync, plugins or AI in the initial release. `docs/PROJECT_SPEC.md` is referenced by the project brief but does not exist yet.

## Commands

- Frontend: `pnpm install`, `pnpm dev`, `pnpm lint` (eslint + prettier check), `pnpm format`, `pnpm test` (vitest), `pnpm build` (tsc + vite). Single test: `pnpm vitest run src/App.test.tsx -t "name"`.
- Rust: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Single test: `cargo test -p notenest-core <name>`.
- Desktop app: `pnpm tauri dev` / `pnpm tauri build` (needs WebKitGTK 4.1 on Linux; not installed on the dev machine, so locally use `-p notenest-core` for cargo and rely on CI for the Tauri crate).
- pnpm is not provided by corepack here; it is installed at `~/.local/bin` (add to PATH).

## Architecture

- `crates/notenest-core`: all application logic; must stay free of Tauri dependencies.
- `src-tauri` (crate `notenest`, lib `notenest_lib`): thin adapter; Tauri commands only delegate to core.
- `src/`: React UI; talks to the backend only via `invoke`. Guard with `__TAURI_INTERNALS__` so it also runs in a plain browser.
- Capabilities are minimal (`core:default`) with a restrictive CSP in `src-tauri/tauri.conf.json`.

## Constraints

- Pinned for compatibility with Node 20: TypeScript ~5.9 (typescript-eslint doesn't support TS 7), vitest 4, happy-dom (jsdom 31 needs Node 22).
- `unsafe_code` is forbidden workspace-wide.
