//! Rebuilds the SQLite index from the Markdown files in the vault.
//!
//! The vault is the source of truth, so this can always recover the index. User
//! files are read, never modified, with one exception: when two notes carry the
//! same frontmatter `id` (e.g. a file copied outside NoteNest), the later path in
//! sorted order gets a fresh id written to its frontmatter so ids stay unique.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::database::Database;
use crate::filesystem::{RelPath, Vault};
use crate::frontmatter::{parse_meta, set_meta, MetaUpdate};
use crate::hash::content_hash;
use crate::models::{note_id_for_path, title_from_path, NoteRecord};
use crate::{Error, Result};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReindexReport {
    pub notes: usize,
    pub folders: usize,
    /// Markdown files that could not be read; they are left alone on disk.
    pub unreadable: Vec<String>,
    /// Paths skipped because their names are not valid vault paths.
    pub skipped_invalid: Vec<String>,
    /// Notes given a new id because another note already used theirs.
    pub duplicate_ids_fixed: Vec<String>,
    /// Leftover temp files from interrupted atomic writes that were removed.
    pub stale_temp_files_removed: usize,
}

/// Temp files older than this are leftovers of a crashed write, not one in progress.
const STALE_TEMP_AGE: std::time::Duration = std::time::Duration::from_secs(300);

fn is_temp_name(name: &str) -> bool {
    name.starts_with(".nn-") && name.ends_with(".tmp")
}

struct Scan {
    notes: Vec<NoteRecord>,
    folders: Vec<String>,
    report: ReindexReport,
}

fn to_utc(t: std::io::Result<SystemTime>, fallback: DateTime<Utc>) -> DateTime<Utc> {
    t.map_or(fallback, DateTime::<Utc>::from)
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|n| n.starts_with('.'))
}

fn scan(vault: &Vault) -> Result<Scan> {
    let root = vault.root().to_owned();
    let mut found: Vec<(String, bool)> = Vec::new(); // (relative path, is_dir)
    let mut report = ReindexReport::default();

    let walker = WalkDir::new(&root)
        .follow_links(false)
        .sort_by_file_name()
        .min_depth(1)
        .into_iter()
        // Hidden directories (.git, .notenest, ...) are skipped; hidden files are
        // visited only so stale temp files can be cleaned up.
        .filter_entry(|e| !(e.file_type().is_dir() && is_hidden(e.file_name())));
    for entry in walker {
        let entry = entry.map_err(|e| {
            let path = e.path().unwrap_or(&root).to_owned();
            let msg = e.to_string();
            Error::io(
                path,
                e.into_io_error()
                    .unwrap_or_else(|| std::io::Error::other(msg)),
            )
        })?;
        let ft = entry.file_type();
        if ft.is_symlink() {
            continue;
        }
        if ft.is_file() && is_hidden(entry.file_name()) {
            let name = entry.file_name().to_str().unwrap_or_default();
            let stale = is_temp_name(name)
                && entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > STALE_TEMP_AGE);
            if stale && fs::remove_file(entry.path()).is_ok() {
                report.stale_temp_files_removed += 1;
            }
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        let Some(rel) = rel.to_str().map(|s| s.replace('\\', "/")) else {
            report
                .skipped_invalid
                .push(entry.path().to_string_lossy().into_owned());
            continue;
        };
        if ft.is_dir() {
            match RelPath::new(&rel) {
                Ok(_) => found.push((rel, true)),
                Err(_) => report.skipped_invalid.push(rel),
            }
        } else if ft.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            match RelPath::new(&rel) {
                Ok(_) => found.push((rel, false)),
                Err(_) => report.skipped_invalid.push(rel),
            }
        }
    }

    let now = Utc::now();
    let mut notes: Vec<NoteRecord> = Vec::new();
    let mut folders = Vec::new();
    let mut used_ids: HashSet<Uuid> = HashSet::new();

    for (rel, is_dir) in found {
        if is_dir {
            folders.push(rel);
            continue;
        }
        let rel_path = RelPath::new(&rel)?;
        let abs = vault.resolve(&rel_path)?;
        let bytes = match fs::read(&abs) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(path = %rel, error = %e, "cannot read note during reindex");
                report.unreadable.push(rel);
                continue;
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        let meta = parse_meta(&text);
        let fs_meta = fs::metadata(&abs).ok();
        let updated_at = fs_meta.as_ref().map_or(now, |m| to_utc(m.modified(), now));
        let created_at = fs_meta
            .as_ref()
            .map_or(updated_at, |m| to_utc(m.created(), updated_at));

        let mut hash = content_hash(&bytes);
        let mut id = meta.id.unwrap_or_else(|| note_id_for_path(&rel));
        if !used_ids.insert(id) {
            // Duplicate id: give this copy a new one and persist it.
            let fresh = Uuid::new_v4();
            let rewritten = set_meta(
                &text,
                &MetaUpdate {
                    id: Some(fresh),
                    ..Default::default()
                },
            );
            match vault.write(&rel_path, rewritten.as_bytes()) {
                Ok(()) => {
                    tracing::warn!(path = %rel, "duplicate note id replaced");
                    hash = content_hash(rewritten.as_bytes());
                    id = fresh;
                    used_ids.insert(id);
                    report.duplicate_ids_fixed.push(rel.clone());
                }
                Err(e) => {
                    tracing::warn!(path = %rel, error = %e, "cannot fix duplicate id; skipping note");
                    report.unreadable.push(rel);
                    continue;
                }
            }
        }
        notes.push(NoteRecord {
            id,
            title: title_from_path(&rel),
            path: rel,
            created_at,
            updated_at,
            content_hash: hash,
            pinned: meta.pinned,
            archived: meta.archived,
            last_opened_at: None,
        });
    }

    report.notes = notes.len();
    report.folders = folders.len();
    Ok(Scan {
        notes,
        folders,
        report,
    })
}

/// Rebuilds the index from the vault. Existing `created_at` and `last_opened_at`
/// values are preserved for notes whose id is unchanged.
///
/// # Errors
/// [`Error::Io`] if the vault cannot be traversed; [`Error::Database`] if the
/// index cannot be written (the previous index is then left as it was).
pub async fn reindex(vault: &Vault, db: &Database) -> Result<ReindexReport> {
    let v = vault.clone();
    let mut scanned = tokio::task::spawn_blocking(move || scan(&v))
        .await
        .map_err(|e| Error::Io {
            path: vault.root().to_owned(),
            source: std::io::Error::other(e),
        })??;

    let existing: HashMap<Uuid, NoteRecord> = db
        .list_notes()
        .await?
        .into_iter()
        .map(|n| (n.id, n))
        .collect();
    for note in &mut scanned.notes {
        if let Some(old) = existing.get(&note.id) {
            note.created_at = old.created_at;
            note.last_opened_at = old.last_opened_at;
        }
    }
    db.replace_index(&scanned.notes, &scanned.folders).await?;
    Ok(scanned.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::RelPath;

    async fn setup() -> (tempfile::TempDir, Vault, Database) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let db = Database::open(&vault.meta_dir().join("index.db"))
            .await
            .unwrap();
        (dir, vault, db)
    }

    fn rel(s: &str) -> RelPath {
        RelPath::new(s).unwrap()
    }

    #[tokio::test]
    async fn indexes_notes_and_folders_ignoring_everything_else() {
        let (dir, vault, db) = setup().await;
        vault.create_dir(&rel("Programming/Deep")).unwrap();
        vault.create_dir(&rel("Empty")).unwrap();
        vault
            .create_new(&rel("Programming/Rust.md"), b"# Rust\n")
            .unwrap();
        vault.create_new(&rel("Inbox.MD"), b"x").unwrap();
        fs::write(dir.path().join("image.png"), b"png").unwrap();
        fs::write(dir.path().join(".hidden.md"), b"h").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/x.md"), b"g").unwrap();

        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.notes, 2);
        let titles: Vec<_> = db
            .list_notes()
            .await
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(titles, vec!["Inbox", "Rust"]);
        assert_eq!(
            db.list_folders().await.unwrap(),
            vec!["Empty", "Programming", "Programming/Deep"]
        );
    }

    #[tokio::test]
    async fn reads_frontmatter_state_and_derives_ids_for_plain_notes() {
        let (_d, vault, db) = setup().await;
        let id = Uuid::new_v4();
        vault
            .create_new(
                &rel("a.md"),
                format!("---\nid: {id}\npinned: true\n---\nA").as_bytes(),
            )
            .unwrap();
        vault.create_new(&rel("b.md"), b"B").unwrap();
        reindex(&vault, &db).await.unwrap();

        let a = db.get_note(id).await.unwrap().unwrap();
        assert!(a.pinned && !a.archived);
        let b = db.get_note_by_path("b.md").await.unwrap().unwrap();
        assert_eq!(b.id, note_id_for_path("b.md"));
        // Plain notes are never modified by indexing.
        assert_eq!(vault.read(&rel("b.md")).unwrap(), b"B");
    }

    #[tokio::test]
    async fn reindexing_is_stable_and_preserves_ui_state() {
        let (_d, vault, db) = setup().await;
        vault.create_new(&rel("a.md"), b"A").unwrap();
        reindex(&vault, &db).await.unwrap();
        let first = db.get_note_by_path("a.md").await.unwrap().unwrap();
        let opened = Utc::now();
        db.set_last_opened(first.id, opened).await.unwrap();

        reindex(&vault, &db).await.unwrap();
        let second = db.get_note_by_path("a.md").await.unwrap().unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.created_at, second.created_at);
        assert_eq!(
            second.last_opened_at.map(|t| t.timestamp()),
            Some(opened.timestamp())
        );
    }

    #[tokio::test]
    async fn removes_rows_for_files_deleted_outside_the_app() {
        let (dir, vault, db) = setup().await;
        vault.create_new(&rel("a.md"), b"A").unwrap();
        vault.create_new(&rel("b.md"), b"B").unwrap();
        reindex(&vault, &db).await.unwrap();
        fs::remove_file(dir.path().join("a.md")).unwrap();
        reindex(&vault, &db).await.unwrap();
        let paths: Vec<_> = db
            .list_notes()
            .await
            .unwrap()
            .into_iter()
            .map(|n| n.path)
            .collect();
        assert_eq!(paths, vec!["b.md"]);
    }

    #[tokio::test]
    async fn duplicate_ids_get_a_fresh_id_written_to_the_later_file() {
        let (_d, vault, db) = setup().await;
        let id = Uuid::new_v4();
        let body = format!("---\nid: {id}\n---\nSame");
        vault.create_new(&rel("a.md"), body.as_bytes()).unwrap();
        vault.create_new(&rel("b.md"), body.as_bytes()).unwrap();

        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.duplicate_ids_fixed, vec!["b.md"]);
        assert_eq!(db.get_note(id).await.unwrap().unwrap().path, "a.md");
        let b = db.get_note_by_path("b.md").await.unwrap().unwrap();
        assert_ne!(b.id, id);
        let on_disk = String::from_utf8(vault.read(&rel("b.md")).unwrap()).unwrap();
        assert_eq!(parse_meta(&on_disk).id, Some(b.id));
        assert!(on_disk.ends_with("Same"));
        // The index matches the files, so a second pass changes nothing.
        let again = reindex(&vault, &db).await.unwrap();
        assert!(again.duplicate_ids_fixed.is_empty());
    }

    #[tokio::test]
    async fn invalid_utf8_notes_are_still_indexed_and_untouched() {
        let (_d, vault, db) = setup().await;
        vault
            .create_new(&rel("latin.md"), &[0x66, 0xe9, 0x6e])
            .unwrap();
        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.notes, 1);
        assert_eq!(
            vault.read(&rel("latin.md")).unwrap(),
            vec![0x66, 0xe9, 0x6e]
        );
    }
}
