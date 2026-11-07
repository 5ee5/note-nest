# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

NoteNest: local-first, offline Markdown note-taking app (Rust + Tauri 2 + React/TypeScript + Tailwind, SQLite, CodeMirror 6). Markdown files on disk are the source of truth. The full spec is `docs/PROJECT_SPEC.md` (gitignored on purpose, kept locally); read it before implementing features, follow its milestone order, and do not build post-v1 features (sync, plugins, AI). Current state: Milestones 0-3 done (UI, storage engine, CodeMirror editor with preview, attachments and statistics); Milestone 4 (tags, wiki links, backlinks, full-text search, favorites, recents, graph) is next. Decisions deviating from the spec go in `docs/DECISIONS.md`.

## Commands

- Frontend: `pnpm install`, `pnpm dev`, `pnpm lint` (eslint + prettier check), `pnpm format`, `pnpm test` (vitest), `pnpm build` (tsc + vite). Single test: `pnpm vitest run src/App.test.tsx -t "name"`.
- Rust: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Single test: `cargo test -p notenest-core <name>`.
- Desktop app: `pnpm tauri dev` / `pnpm tauri build` need WebKitGTK 4.1, which is not installed locally. Locally use `cargo test -p notenest-core`; to compile/lint the Tauri crate run it in a container: `scripts/docker-check.sh 'cargo clippy --workspace --all-targets -- -D warnings'`. CI builds it too. The desktop app itself has never been run locally.
- pnpm is installed at `~/.local/bin` (corepack's is broken); add it to PATH.
- CI: Forgejo Actions (`.forgejo/workflows/ci.yml`), runner `e5-gentoo-docker` is a background process from `~/forgejo-runner/` (not a service; may need restarting: `forgejo-runner -c ~/forgejo-runner/config.yml daemon`). Check runs with `fj -H git.5ee5.dev actions tasks`.
- Visual/browser checks: host Chromium lacks libs; run Playwright inside a `node:22-bookworm` container with `apt install chromium`. CodeMirror cannot run in happy-dom, so App tests stub the editor with a textarea; editor logic is tested headlessly on `EditorState` (see `src/components/editor/*.test.ts`) and real typing/paste/drop in Chromium.

## Architecture

See `docs/ARCHITECTURE.md`. In short: `crates/notenest-core` (Tauri-free logic; `services::Workspace` is the entry point) <- `src-tauri` (thin command adapter) <- React UI, which reaches data only through the `NotesApi` interface (`src/lib/api`: `TauriNotesApi` on desktop, `MockNotesApi` in a browser and in tests). Notes keep `id`/`pinned`/`archived` in frontmatter; the SQLite index in `<vault>/.notenest/` is rebuildable. Deviations and known limitations are in `docs/DECISIONS.md`.

## Constraints

- Pinned for Node 20: TypeScript ~5.9, vitest 4, happy-dom; do not add `@testing-library/jest-dom` or jsdom 31 (need Node 22).
- `unsafe_code` is forbidden workspace-wide.
- Commits: Conventional Commits; no Claude attribution trailers. Commit straight to `main`; the user waived the per-commit approval step for the Milestone 1 session (still print the author identity).
