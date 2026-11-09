use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

use super::knowledge::{delete_note_tx, prune_tags, write_derived};
use super::Database;
use crate::extract::Extracted;
use crate::models::{IndexEntry, NoteRecord};
use crate::{Error, Result};

fn parse_time(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| Error::Corrupt {
            context: "timestamp".into(),
            message: format!("{s:?}: {e}"),
        })
}

fn note_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<NoteRecord> {
    let id: String = row.try_get("id")?;
    let last_opened: Option<String> = row.try_get("last_opened_at")?;
    Ok(NoteRecord {
        id: Uuid::parse_str(&id).map_err(|e| Error::Corrupt {
            context: "note id".into(),
            message: e.to_string(),
        })?,
        path: row.try_get("path")?,
        title: row.try_get("title")?,
        created_at: parse_time(&row.try_get::<String, _>("created_at")?)?,
        updated_at: parse_time(&row.try_get::<String, _>("updated_at")?)?,
        content_hash: row.try_get("content_hash")?,
        pinned: row.try_get::<i64, _>("pinned")? != 0,
        archived: row.try_get::<i64, _>("archived")? != 0,
        last_opened_at: last_opened.as_deref().map(parse_time).transpose()?,
    })
}

const NOTE_COLUMNS: &str =
    "id, path, title, created_at, updated_at, content_hash, pinned, archived, last_opened_at";

fn insert_note_sql() -> String {
    format!("INSERT INTO notes ({NOTE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")
}

impl Database {
    pub async fn list_notes(&self) -> Result<Vec<NoteRecord>> {
        let rows = sqlx::query(&format!("SELECT {NOTE_COLUMNS} FROM notes ORDER BY path"))
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(note_from_row).collect()
    }

    pub async fn get_note(&self, id: Uuid) -> Result<Option<NoteRecord>> {
        let row = sqlx::query(&format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1"))
            .bind(id.to_string())
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(note_from_row).transpose()
    }

    pub async fn get_note_by_path(&self, path: &str) -> Result<Option<NoteRecord>> {
        let row = sqlx::query(&format!("SELECT {NOTE_COLUMNS} FROM notes WHERE path = ?1"))
            .bind(path)
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(note_from_row).transpose()
    }

    /// Inserts or updates a note together with everything derived from its content,
    /// atomically. Keyed by id; `created_at` and `last_opened_at` of an existing row
    /// are kept.
    pub async fn upsert_indexed(&self, n: &NoteRecord, x: &Extracted) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            "INSERT INTO notes (id, path, title, created_at, updated_at, content_hash, pinned, archived) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(id) DO UPDATE SET path = excluded.path, title = excluded.title, \
               updated_at = excluded.updated_at, content_hash = excluded.content_hash, \
               pinned = excluded.pinned, archived = excluded.archived",
        )
        .bind(n.id.to_string())
        .bind(&n.path)
        .bind(&n.title)
        .bind(n.created_at.to_rfc3339())
        .bind(n.updated_at.to_rfc3339())
        .bind(&n.content_hash)
        .bind(i64::from(n.pinned))
        .bind(i64::from(n.archived))
        .execute(&mut *tx)
        .await?;
        write_derived(&mut tx, n, x, false, None).await?;
        prune_tags(&mut tx).await?;
        tx.commit().await?;
        self.index_changed();
        Ok(())
    }

    /// Like [`Database::upsert_indexed`] for a note with no content-derived data
    /// (no tags or links); the title and path keys are still indexed.
    pub async fn upsert_note(&self, n: &NoteRecord) -> Result<()> {
        self.upsert_indexed(n, &Extracted::default()).await
    }

    pub async fn delete_note(&self, id: Uuid) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        delete_note_tx(&mut tx, &id.to_string()).await?;
        tx.commit().await?;
        self.index_changed();
        Ok(())
    }

    pub async fn set_last_opened(&self, id: Uuid, at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE notes SET last_opened_at = ?2 WHERE id = ?1")
            .bind(id.to_string())
            .bind(at.to_rfc3339())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Replaces the whole index (notes, everything derived from them, and folders) in
    /// one transaction (used by reindexing), so readers never observe a half-built index.
    pub async fn replace_index(&self, entries: &[IndexEntry], folders: &[String]) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("DELETE FROM notes").execute(&mut *tx).await?; // cascades to derived tables
        sqlx::query("DELETE FROM note_fts")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM tags").execute(&mut *tx).await?;
        sqlx::query("DELETE FROM folders").execute(&mut *tx).await?;
        for entry in entries {
            insert_entry(&mut tx, entry).await?;
        }
        insert_folders(&mut tx, folders).await?;
        tx.commit().await?;
        self.index_changed();
        Ok(())
    }

    /// Brings the index in line with a scan of the vault without rebuilding it: deletes
    /// the notes in `remove`, then (re)inserts `entries`, which the caller has reduced to
    /// the notes that are new or differ from the index. Folders are rewritten only when
    /// they changed. One transaction, so readers never see a half-updated index.
    ///
    /// A note in `entries` that already exists is deleted first (by id and by path) and
    /// inserted again, so moves, swaps and content edits all go through one path.
    pub async fn sync_index(
        &self,
        entries: &[IndexEntry],
        remove: &[Uuid],
        folders: &[String],
    ) -> Result<()> {
        let mut current = self.list_folders().await?;
        let mut wanted: Vec<String> = folders.to_vec();
        current.sort();
        wanted.sort();
        let folders_changed = current != wanted;
        if entries.is_empty() && remove.is_empty() && !folders_changed {
            return Ok(());
        }

        let mut tx = self.pool().begin().await?;
        for id in remove {
            delete_note_tx(&mut tx, &id.to_string()).await?;
        }
        for entry in entries {
            delete_note_tx(&mut tx, &entry.record.id.to_string()).await?;
            let at_path: Option<String> =
                sqlx::query_scalar("SELECT id FROM notes WHERE path = ?1")
                    .bind(&entry.record.path)
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some(other) = at_path {
                delete_note_tx(&mut tx, &other).await?;
            }
        }
        for entry in entries {
            insert_entry(&mut tx, entry).await?;
        }
        if folders_changed {
            sqlx::query("DELETE FROM folders").execute(&mut *tx).await?;
            insert_folders(&mut tx, folders).await?;
        }
        prune_tags(&mut tx).await?;
        tx.commit().await?;
        self.index_changed();
        Ok(())
    }
}

async fn insert_entry(tx: &mut sqlx::SqliteConnection, entry: &IndexEntry) -> Result<()> {
    let IndexEntry {
        record: n,
        extracted,
    } = entry;
    let inserted = sqlx::query(&insert_note_sql())
        .bind(n.id.to_string())
        .bind(&n.path)
        .bind(&n.title)
        .bind(n.created_at.to_rfc3339())
        .bind(n.updated_at.to_rfc3339())
        .bind(&n.content_hash)
        .bind(i64::from(n.pinned))
        .bind(i64::from(n.archived))
        .bind(n.last_opened_at.map(|t| t.to_rfc3339()))
        .execute(&mut *tx)
        .await?;
    write_derived(tx, n, extracted, true, Some(inserted.last_insert_rowid())).await
}

async fn insert_folders(tx: &mut sqlx::SqliteConnection, folders: &[String]) -> Result<()> {
    let mut sorted: Vec<&String> = folders.iter().collect();
    sorted.sort();
    let mut ids = std::collections::HashMap::<&str, i64>::new();
    for path in sorted {
        let (parent, name) = match path.rsplit_once('/') {
            Some((p, n)) => (ids.get(p).copied(), n),
            None => (None, path.as_str()),
        };
        let row = sqlx::query(
            "INSERT INTO folders (name, parent_id, path) VALUES (?1, ?2, ?3) RETURNING id",
        )
        .bind(name)
        .bind(parent)
        .bind(path)
        .fetch_one(&mut *tx)
        .await?;
        ids.insert(path.as_str(), row.try_get("id")?);
    }
    Ok(())
}

impl Database {
    /// Records a folder (and any missing ancestors) in the index.
    pub async fn ensure_folder(&self, path: &str) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        let mut parent: Option<i64> = None;
        let mut current = String::new();
        for name in path.split('/') {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(name);
            let existing = sqlx::query("SELECT id FROM folders WHERE path = ?1")
                .bind(&current)
                .fetch_optional(&mut *tx)
                .await?;
            let id: i64 = if let Some(row) = existing {
                row.try_get("id")?
            } else {
                sqlx::query(
                    "INSERT INTO folders (name, parent_id, path) VALUES (?1, ?2, ?3) RETURNING id",
                )
                .bind(name)
                .bind(parent)
                .bind(&current)
                .fetch_one(&mut *tx)
                .await?
                .try_get("id")?
            };
            parent = Some(id);
        }
        tx.commit().await?;
        Ok(())
    }

    /// Folder paths, sorted.
    pub async fn list_folders(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT path FROM folders ORDER BY path")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(|r| Ok(r.try_get("path")?)).collect()
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(self.pool())
            .await?;
        row.map(|r| Ok(r.try_get("value")?)).transpose()
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
