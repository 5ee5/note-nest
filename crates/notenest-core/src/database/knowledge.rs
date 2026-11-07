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

/// Replaces everything derived for note `n`. The note row must already exist.
/// With `fresh` set, the note is known to have no derived rows yet (bulk reindex).
pub(super) async fn write_derived(
    conn: &mut SqliteConnection,
    n: &NoteRecord,
    x: &Extracted,
    fresh: bool,
) -> Result<()> {
    let id = n.id.to_string();
    let rowid: i64 = sqlx::query_scalar("SELECT rowid FROM notes WHERE id = ?1")
        .bind(&id)
        .fetch_one(&mut *conn)
        .await?;

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

    let tagged = x
        .frontmatter_tags
        .iter()
        .map(|t| (t, "frontmatter"))
        .chain(x.inline_tags.iter().map(|t| (t, "inline")));
    for (name, source) in tagged {
        sqlx::query("INSERT OR IGNORE INTO tags (name) VALUES (?1)")
            .bind(name)
            .execute(&mut *conn)
            .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id, source) \
             SELECT ?1, id, ?3 FROM tags WHERE name = ?2",
        )
        .bind(&id)
        .bind(name)
        .bind(source)
        .execute(&mut *conn)
        .await?;
    }

    for alias in &x.aliases {
        sqlx::query("INSERT OR IGNORE INTO note_aliases (note_id, alias) VALUES (?1, ?2)")
            .bind(&id)
            .bind(alias)
            .execute(&mut *conn)
            .await?;
    }

    // A path key only counts as a path match when it is path-qualified; a root-level
    // note's path key is just its title (so it does not outrank a same-folder note).
    // Insertion order is the precedence when keys coincide: path, then title, then alias.
    let pk = path_key(&n.path);
    let pk_kind = if pk.contains('/') { "path" } else { "title" };
    let keys = std::iter::once((pk, pk_kind))
        .chain(std::iter::once((n.title.to_lowercase(), "title")))
        .chain(x.aliases.iter().map(|a| (a.trim().to_lowercase(), "alias")));
    for (key, kind) in keys.filter(|(k, _)| !k.is_empty()) {
        sqlx::query("INSERT OR IGNORE INTO note_keys (key, note_id, kind) VALUES (?1, ?2, ?3)")
            .bind(key)
            .bind(&id)
            .bind(kind)
            .execute(&mut *conn)
            .await?;
    }

    for link in &x.links {
        sqlx::query(
            "INSERT INTO note_links (source_note_id, target_key, target_text, display, line, context) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(&id)
        .bind(&link.key)
        .bind(&link.target_text)
        .bind(&link.display)
        .bind(i64::try_from(link.line).unwrap_or(i64::MAX))
        .bind(&link.context)
        .execute(&mut *conn)
        .await?;
    }

    let tags_text = x
        .frontmatter_tags
        .iter()
        .chain(&x.inline_tags)
        .map(String::as_str)
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
