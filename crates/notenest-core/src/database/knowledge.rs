//! Writes the derived knowledge tables (tags, aliases, link keys, links, full-text
//! index) for a note. All functions run inside the caller's transaction.

use sqlx::SqliteConnection;

use crate::extract::Extracted;
use crate::models::NoteRecord;
use crate::Result;

/// Longest body text indexed for full-text search (very large notes are truncated).
pub const MAX_INDEXED_BODY_BYTES: usize = 2 * 1024 * 1024;

fn truncate_on_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The lower-cased vault path without its `.md` extension: the "path key" of a note.
pub(crate) fn path_key(path: &str) -> String {
    let lower = path.to_lowercase();
    lower.strip_suffix(".md").unwrap_or(&lower).to_owned()
}

/// Appends `(?, ?, ...)` groups for `rows` rows of `width` parameters, numbered from `first`.
fn placeholders(rows: usize, width: usize, first: usize) -> String {
    (0..rows)
        .map(|r| {
            let params = (0..width)
                .map(|c| format!("?{}", first + r * width + c))
                .collect::<Vec<_>>();
            format!("({})", params.join(", "))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Most rows sent in one multi-row INSERT (well below SQLite's parameter limit).
const BATCH_ROWS: usize = 100;

/// Replaces everything derived for note `n`. The note row must already exist.
///
/// `rowid` is the note's `rowid` when the caller already knows it (a row just inserted);
/// otherwise it is looked up. With `fresh` set, the note is known to have no derived rows
/// yet (bulk reindex), so nothing needs clearing. Rows are written in a handful of
/// multi-row statements: each statement costs a round trip to the database worker, so
/// fewer statements make reindexing a large vault several times faster.
pub(super) async fn write_derived(
    conn: &mut SqliteConnection,
    n: &NoteRecord,
    x: &Extracted,
    fresh: bool,
    rowid: Option<i64>,
) -> Result<()> {
    let id = n.id.to_string();
    let rowid = match rowid {
        Some(r) => r,
        None => {
            sqlx::query_scalar("SELECT rowid FROM notes WHERE id = ?1")
                .bind(&id)
                .fetch_one(&mut *conn)
                .await?
        }
    };

    if !fresh {
        for sql in [
            "DELETE FROM note_tags WHERE note_id = ?1",
            "DELETE FROM note_aliases WHERE note_id = ?1",
            "DELETE FROM note_keys WHERE note_id = ?1",
            "DELETE FROM note_links WHERE source_note_id = ?1",
        ] {
            sqlx::query(sql).bind(&id).execute(&mut *conn).await?;
        }
        sqlx::query("DELETE FROM note_fts WHERE rowid = ?1")
            .bind(rowid)
            .execute(&mut *conn)
            .await?;
    }

    // Tags: make sure the names exist, then link the note to them (one statement per source).
    let all_tags: Vec<&String> = x.frontmatter_tags.iter().chain(&x.inline_tags).collect();
    if !all_tags.is_empty() {
        let sql = format!(
            "INSERT OR IGNORE INTO tags (name) VALUES {}",
            placeholders(all_tags.len(), 1, 1)
        );
        let mut q = sqlx::query(&sql);
        for t in &all_tags {
            q = q.bind(t.as_str());
        }
        q.execute(&mut *conn).await?;
        for (names, source) in [
            (&x.frontmatter_tags, "frontmatter"),
            (&x.inline_tags, "inline"),
        ] {
            if names.is_empty() {
                continue;
            }
            let marks = (0..names.len())
                .map(|i| format!("?{}", i + 3))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "INSERT OR IGNORE INTO note_tags (note_id, tag_id, source) \
                 SELECT ?1, id, ?2 FROM tags WHERE name IN ({marks})"
            );
            let mut q = sqlx::query(&sql).bind(&id).bind(source);
            for t in names {
                q = q.bind(t.as_str());
            }
            q.execute(&mut *conn).await?;
        }
    }

    if !x.aliases.is_empty() {
        let sql = format!(
            "INSERT OR IGNORE INTO note_aliases (note_id, alias) VALUES {}",
            placeholders(x.aliases.len(), 2, 1)
        );
        let mut q = sqlx::query(&sql);
        for a in &x.aliases {
            q = q.bind(&id).bind(a.as_str());
        }
        q.execute(&mut *conn).await?;
    }

    // A path key only counts as a path match when it is path-qualified; a root-level
    // note's path key is just its title (so it does not outrank a same-folder note).
    // Insertion order is the precedence when keys coincide: path, then title, then alias.
    let pk = path_key(&n.path);
    let pk_kind = if pk.contains('/') { "path" } else { "title" };
    let keys: Vec<(String, &str)> = std::iter::once((pk, pk_kind))
        .chain(std::iter::once((n.title.to_lowercase(), "title")))
        .chain(x.aliases.iter().map(|a| (a.trim().to_lowercase(), "alias")))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    let sql = format!(
        "INSERT OR IGNORE INTO note_keys (key, note_id, kind) VALUES {}",
        placeholders(keys.len(), 3, 1)
    );
    let mut q = sqlx::query(&sql);
    for (key, kind) in &keys {
        q = q.bind(key.as_str()).bind(&id).bind(*kind);
    }
    q.execute(&mut *conn).await?;

    for chunk in x.links.chunks(BATCH_ROWS) {
        let sql = format!(
            "INSERT INTO note_links (source_note_id, target_key, target_text, display, line, context) VALUES {}",
            placeholders(chunk.len(), 6, 1)
        );
        let mut q = sqlx::query(&sql);
        for link in chunk {
            q = q
                .bind(&id)
                .bind(&link.key)
                .bind(&link.target_text)
                .bind(&link.display)
                .bind(i64::try_from(link.line).unwrap_or(i64::MAX))
                .bind(&link.context);
        }
        q.execute(&mut *conn).await?;
    }

    let tags_text = all_tags
        .iter()
        .map(|t| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    sqlx::query("INSERT INTO note_fts (rowid, title, body, tags) VALUES (?1, ?2, ?3, ?4)")
        .bind(rowid)
        .bind(&n.title)
        .bind(truncate_on_boundary(&x.body, MAX_INDEXED_BODY_BYTES))
        .bind(tags_text)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Removes a note and everything derived from it.
pub(super) async fn delete_note_tx(conn: &mut SqliteConnection, id: &str) -> Result<()> {
    let rowid: Option<i64> = sqlx::query_scalar("SELECT rowid FROM notes WHERE id = ?1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(rowid) = rowid {
        sqlx::query("DELETE FROM note_fts WHERE rowid = ?1")
            .bind(rowid)
            .execute(&mut *conn)
            .await?;
    }
    // The other derived tables are removed by ON DELETE CASCADE.
    sqlx::query("DELETE FROM notes WHERE id = ?1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    prune_tags(conn).await
}

/// Drops tags that no note uses any more.
pub(super) async fn prune_tags(conn: &mut SqliteConnection) -> Result<()> {
    sqlx::query("DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM note_tags)")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[cfg(test)]
impl super::Database {
    /// Every derived fact as sorted text lines, for comparing two indexes of the same vault.
    /// Independent of row ids, so an incrementally maintained index can be compared with
    /// a freshly rebuilt one.
    pub(crate) async fn derived_snapshot(&self) -> Result<Vec<String>> {
        use sqlx::Row;
        let mut lines = Vec::new();
        let queries: [(&str, &str); 7] = [
            ("note", "SELECT id || '|' || path || '|' || title || '|' || pinned || '|' || archived AS v FROM notes"),
            ("tagrow", "SELECT name AS v FROM tags"),
            (
                "tag",
                "SELECT nt.note_id || '|' || t.name || '|' || nt.source AS v FROM note_tags nt JOIN tags t ON t.id = nt.tag_id",
            ),
            ("alias", "SELECT note_id || '|' || alias AS v FROM note_aliases"),
            ("key", "SELECT note_id || '|' || key || '|' || kind AS v FROM note_keys"),
            (
                "link",
                "SELECT source_note_id || '|' || target_key || '|' || target_text || '|' || COALESCE(display, '~') || '|' || line || '|' || context AS v FROM note_links",
            ),
            (
                "fts",
                "SELECT n.id || '|' || f.title || '|' || f.body || '|' || f.tags AS v FROM note_fts f JOIN notes n ON n.rowid = f.rowid",
            ),
        ];
        for (label, sql) in queries {
            for row in sqlx::query(sql).fetch_all(self.pool()).await? {
                lines.push(format!("{label}|{}", row.try_get::<String, _>("v")?));
            }
        }
        // Rows in the FTS table that belong to no note would show up as a count mismatch.
        let orphans: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM note_fts WHERE rowid NOT IN (SELECT rowid FROM notes)",
        )
        .fetch_one(self.pool())
        .await?;
        lines.push(format!("orphan-fts|{orphans}"));
        lines.sort();
        Ok(lines)
    }
}
