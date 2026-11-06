-- Index of the vault. Markdown files are the source of truth: everything here
-- can be rebuilt from them (except last_opened_at, which is UI convenience state).

CREATE TABLE notes (
    id             TEXT PRIMARY KEY NOT NULL,
    path           TEXT NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    content_hash   TEXT NOT NULL,
    pinned         INTEGER NOT NULL DEFAULT 0,
    archived       INTEGER NOT NULL DEFAULT 0,
    last_opened_at TEXT
) STRICT;

CREATE INDEX idx_notes_updated_at ON notes (updated_at);

CREATE TABLE folders (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL,
    parent_id INTEGER REFERENCES folders (id) ON DELETE CASCADE,
    path      TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;
