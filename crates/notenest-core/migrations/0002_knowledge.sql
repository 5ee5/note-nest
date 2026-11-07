-- Knowledge index: tags, aliases, wiki links and full-text search. Like everything in
-- this database it is derived from the Markdown files and rebuilt by reindexing.

CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE note_tags (
    note_id TEXT    NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags (id)  ON DELETE CASCADE,
    source  TEXT    NOT NULL CHECK (source IN ('frontmatter', 'inline')),
    PRIMARY KEY (note_id, tag_id, source)
) STRICT;

CREATE INDEX idx_note_tags_tag ON note_tags (tag_id);

CREATE TABLE note_aliases (
    note_id TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    alias   TEXT NOT NULL,
    PRIMARY KEY (note_id, alias)
) STRICT;

-- Names a note can be linked by (lower-cased): its vault path without ".md", its
-- title, and its aliases. Links are matched against these at query time, so a rename,
-- move, delete or restore never leaves a stale link target behind.
CREATE TABLE note_keys (
    key     TEXT NOT NULL,
    note_id TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    kind    TEXT NOT NULL CHECK (kind IN ('path', 'title', 'alias')),
    PRIMARY KEY (key, note_id)
) STRICT;

CREATE INDEX idx_note_keys_note ON note_keys (note_id);

CREATE TABLE note_links (
    id             INTEGER PRIMARY KEY,
    source_note_id TEXT    NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    target_key     TEXT    NOT NULL,
    target_text    TEXT    NOT NULL,
    display        TEXT,
    line           INTEGER NOT NULL,
    context        TEXT    NOT NULL
) STRICT;

CREATE INDEX idx_note_links_source ON note_links (source_note_id);
CREATE INDEX idx_note_links_target ON note_links (target_key);

-- Rows share the rowid of their note in `notes`, so they can be replaced by rowid.
CREATE VIRTUAL TABLE note_fts USING fts5 (
    title,
    body,
    tags,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3 4'
);
