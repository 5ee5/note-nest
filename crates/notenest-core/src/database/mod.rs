//! SQLite persistence: the rebuildable index of the vault plus app settings.
//!
//! All SQL lives in this module. Queries use the runtime `sqlx::query` API (not
//! the compile-time macros) so building never needs a live database.

mod knowledge;
mod links;
mod queries;
mod search;

pub use knowledge::MAX_INDEXED_BODY_BYTES;
pub use links::{Candidate, CapturedLink, Resolver};

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::{Error, Result};

/// Handle to the SQLite index. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

/// Result of [`Database::open_recovering`].
#[derive(Debug)]
pub struct Opened {
    pub database: Database,
    /// True when the existing file was unusable and a fresh, empty index was
    /// created: the caller must reindex from the vault.
    pub rebuilt: bool,
    /// Where the unusable file was moved (never deleted), if `rebuilt`.
    pub backup: Option<PathBuf>,
}

impl Database {
    /// Opens (creating if needed) the database at `path` and applies migrations.
    ///
    /// # Errors
    /// [`Error::Database`] / [`Error::Migration`] if the file is unusable.
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        let db = Self { pool };
        db.check_integrity().await?;
        Ok(db)
    }

    /// Like [`Database::open`], but if the file is corrupt or cannot be migrated it
    /// is moved aside (kept as a backup) and a fresh index is created. The index is
    /// derived data, so this never loses notes.
    ///
    /// # Errors
    /// Only if even a fresh database cannot be created, or the bad file cannot be moved.
    pub async fn open_recovering(path: &Path) -> Result<Opened> {
        // SQLite may silently re-initialize a very small non-database file, so check
        // the header ourselves first: anything that is not SQLite is kept as a backup.
        let attempt = if has_sqlite_header(path) {
            Self::open(path).await
        } else {
            Err(Error::Corrupt {
                context: "index database".into(),
                message: "file is not a SQLite database".into(),
            })
        };
        match attempt {
            Ok(database) => Ok(Opened {
                database,
                rebuilt: false,
                backup: None,
            }),
            Err(first) => {
                tracing::error!(error = %first, path = %path.display(), "index unusable, rebuilding");
                let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3f");
                let backup = path.with_extension(format!("db.corrupt-{stamp}"));
                std::fs::rename(path, &backup).map_err(|e| Error::io(path, e))?;
                for ext in ["db-wal", "db-shm"] {
                    let side = path.with_extension(ext);
                    if side.exists() {
                        let moved = backup.with_extension(format!("{stamp}.{ext}"));
                        std::fs::rename(&side, moved).map_err(|e| Error::io(&side, e))?;
                    }
                }
                let database = Self::open(path).await?;
                Ok(Opened {
                    database,
                    rebuilt: true,
                    backup: Some(backup),
                })
            }
        }
    }

    async fn check_integrity(&self) -> Result<()> {
        let row: (String,) = sqlx::query_as("PRAGMA quick_check")
            .fetch_one(&self.pool)
            .await?;
        if row.0 == "ok" {
            Ok(())
        } else {
            Err(Error::Corrupt {
                context: "index database".into(),
                message: row.0,
            })
        }
    }

    /// Closes all connections (flushing the WAL).
    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

/// True if the file is missing, empty, or starts with the SQLite magic string.
fn has_sqlite_header(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return true; // nothing there yet: a fresh database will be created
    };
    let mut head = [0u8; 16];
    match file.read(&mut head) {
        Ok(0) => true,
        Ok(n) => n == 16 && &head == b"SQLite format 3\0",
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::models::NoteRecord;

    fn note(path: &str) -> NoteRecord {
        let now = Utc::now();
        NoteRecord {
            id: Uuid::new_v4(),
            path: path.into(),
            title: "t".into(),
            created_at: now,
            updated_at: now,
            content_hash: "h".into(),
            pinned: false,
            archived: false,
            last_opened_at: None,
        }
    }

    #[tokio::test]
    async fn migrations_run_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        let db = Database::open(&path).await.unwrap();
        db.set_setting("theme", "dark").await.unwrap();
        db.close().await;
        let db = Database::open(&path).await.unwrap();
        assert_eq!(
            db.get_setting("theme").await.unwrap().as_deref(),
            Some("dark")
        );
        assert_eq!(db.get_setting("missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn upsert_keeps_created_at_and_last_opened() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("index.db")).await.unwrap();
        let mut n = note("a.md");
        db.upsert_note(&n).await.unwrap();
        let opened = Utc::now();
        db.set_last_opened(n.id, opened).await.unwrap();

        let created = n.created_at;
        n.created_at = created + chrono::Duration::days(1);
        n.title = "renamed".into();
        db.upsert_note(&n).await.unwrap();
        let got = db.get_note(n.id).await.unwrap().unwrap();
        assert_eq!(got.title, "renamed");
        assert_eq!(got.created_at.timestamp(), created.timestamp());
        assert!(got.last_opened_at.is_some());
    }

    #[tokio::test]
    async fn path_is_unique() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("index.db")).await.unwrap();
        db.upsert_note(&note("a.md")).await.unwrap();
        assert!(db.upsert_note(&note("a.md")).await.is_err());
    }

    #[tokio::test]
    async fn corrupt_index_is_moved_aside_and_rebuilt_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        std::fs::write(
            &path,
            b"this is definitely not a sqlite database, not even close",
        )
        .unwrap();

        let opened = Database::open_recovering(&path).await.unwrap();
        assert!(opened.rebuilt);
        assert!(opened.database.list_notes().await.unwrap().is_empty());
        let backup = opened.backup.unwrap();
        assert!(std::fs::read(backup)
            .unwrap()
            .starts_with(b"this is definitely"));
    }

    #[tokio::test]
    async fn tiny_garbage_files_are_backed_up_not_silently_reinitialized() {
        // SQLite itself sometimes accepts such a file as an empty database, so repeat to
        // make sure our own header check, not luck, is what rejects it. Each round uses
        // its own directory so a connection still closing cannot touch the next file.
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("index.db");
            std::fs::write(&path, b"short").unwrap();
            let opened = Database::open_recovering(&path).await.unwrap();
            assert!(opened.rebuilt);
            assert_eq!(std::fs::read(opened.backup.unwrap()).unwrap(), b"short");
        }
    }

    #[tokio::test]
    async fn healthy_index_is_not_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let opened = Database::open_recovering(&dir.path().join("index.db"))
            .await
            .unwrap();
        assert!(!opened.rebuilt && opened.backup.is_none());
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use chrono::Utc;

    use super::Database;
    use crate::extract::extract;
    use crate::hash::content_hash;
    use crate::models::{note_id_for_path, title_from_path, NoteRecord};

    pub async fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("index.db")).await.unwrap();
        (dir, db)
    }

    /// Indexes a note as the app would (id derived from the path).
    pub async fn add(db: &Database, path: &str, content: &str) -> NoteRecord {
        let now = Utc::now();
        let record = NoteRecord {
            id: note_id_for_path(path),
            path: path.to_owned(),
            title: title_from_path(path),
            created_at: now,
            updated_at: now,
            content_hash: content_hash(content.as_bytes()),
            pinned: false,
            archived: false,
            last_opened_at: None,
        };
        db.upsert_indexed(&record, &extract(content)).await.unwrap();
        record
    }
}
