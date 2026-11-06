# Architecture (Milestone 0)

- `notenest-core` holds all application logic and must not depend on Tauri.
- `src-tauri` is a thin adapter: Tauri commands delegate to `notenest-core`.
- The frontend talks to the backend only through Tauri commands (`invoke`).
- Capabilities are kept minimal (`core:default`); the CSP is restrictive.
- Markdown files on disk are the authoritative note storage (not yet implemented).

`docs/PROJECT_SPEC.md` has not been provided yet; decisions here are provisional.
