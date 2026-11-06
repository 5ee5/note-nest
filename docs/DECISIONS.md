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
