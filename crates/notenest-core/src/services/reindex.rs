//! Rebuilds the SQLite index from the Markdown files in the vault.
//!
//! The vault is the source of truth, so this can always recover the index. User
//! files are read, never modified, with one exception: when two notes carry the
//! same frontmatter `id` (e.g. a file copied outside NoteNest), one of them gets a
//! fresh id written to its frontmatter so ids stay unique. The note the index
//! already knows under that id keeps it; otherwise the first path in sorted order.
//!
//! A [`ScanCache`] lets repeated passes (the file watcher's) skip reading files
//! whose size and modification time are unchanged since they were last read and
//! whose index row still matches what was read then.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::database::Database;
use crate::extract::extract;
use crate::filesystem::{RelPath, Vault};
use crate::frontmatter::{parse_meta, set_meta, Meta, MetaUpdate};
use crate::hash::content_hash;
use crate::models::{note_id_for_path, title_from_path, IndexEntry, NoteRecord};
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
    /// Notes left out of this pass because they share an id with another note and
    /// were modified moments ago (possibly still being copied), so rewriting them
    /// now could cut the copy short. A later pass gives them a fresh id.
    pub deferred: Vec<String>,
    /// Leftover temp files from interrupted atomic writes that were removed.
    pub stale_temp_files_removed: usize,
    /// Notes added to or updated in the index by this pass.
    pub changed: Vec<Uuid>,
    /// Notes removed from the index by this pass.
    pub removed: Vec<Uuid>,
    /// The folder list changed.
    pub folders_changed: bool,
}

/// Temp files older than this are leftovers of a crashed write, not one in progress.
const STALE_TEMP_AGE: Duration = Duration::from_secs(300);

/// A file modified more recently than this before it was read may be modified again
/// within the same timestamp tick (coarse on some filesystems), so its size and
/// modification time are not trusted to prove it unchanged. Duplicate ids on files
/// this fresh are not rewritten yet either.
const SETTLE: Duration = Duration::from_secs(2);

fn is_temp_name(name: &str) -> bool {
    name.starts_with(".nn-") && name.ends_with(".tmp")
}

#[derive(Debug, Clone)]
struct CachedFile {
    len: u64,
    modified: SystemTime,
    read_at: SystemTime,
    id: Uuid,
    hash: String,
}

impl CachedFile {
    fn trusted_for(&self, len: u64, modified: SystemTime) -> bool {
        self.len == len
            && self.modified == modified
            && self
                .read_at
                .duration_since(self.modified)
                .is_ok_and(|age| age >= SETTLE)
    }
}

/// What the last scan read, by path. Shared by clones; empty means "read everything".
#[derive(Debug, Default, Clone)]
pub struct ScanCache(Arc<Mutex<HashMap<String, CachedFile>>>);

impl ScanCache {
    fn snapshot(&self) -> HashMap<String, CachedFile> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn replace(&self, files: HashMap<String, CachedFile>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = files;
    }

    /// Forgets everything, so the next scan reads every file.
    pub fn clear(&self) {
        self.replace(HashMap::new());
    }
}

struct Scan {
    /// Notes that were read in this pass.
    notes: Vec<IndexEntry>,
    /// Notes proven unchanged without reading them (their index rows are current).
    unchanged: Vec<Uuid>,
    folders: Vec<String>,
    report: ReindexReport,
    cache: HashMap<String, CachedFile>,
}

/// True when the indexed row already reflects the scanned file. Everything derived is a
/// function of the file content (hashed) and the path (title).
fn same_note(old: &NoteRecord, new: &NoteRecord) -> bool {
    old.path == new.path
        && old.content_hash == new.content_hash
        && old.pinned == new.pinned
        && old.archived == new.archived
}

fn to_utc(t: std::io::Result<SystemTime>, fallback: DateTime<Utc>) -> DateTime<Utc> {
    t.map_or(fallback, DateTime::<Utc>::from)
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|n| n.starts_with('.'))
}

/// Lists the vault's folders and Markdown files (relative path, is_dir), removing
/// stale temp files on the way.
fn walk(vault: &Vault, report: &mut ReindexReport) -> Result<Vec<(String, bool)>> {
    let root = vault.root().to_owned();
    let mut found: Vec<(String, bool)> = Vec::new();

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
    Ok(found)
}

/// A note file found by the scan.
enum Candidate {
    /// Its index row is current; it was not read.
    Unchanged {
        rel: String,
        cached: CachedFile,
    },
    Read(ReadFile),
}

struct ReadFile {
    rel: String,
    text: String,
    meta: Meta,
    id: Uuid,
    hash: String,
    len: u64,
    modified: Option<SystemTime>,
    read_at: SystemTime,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Candidate {
    fn id(&self) -> Uuid {
        match self {
            Self::Unchanged { cached, .. } => cached.id,
            Self::Read(f) => f.id,
        }
    }

    fn rel(&self) -> &str {
        match self {
            Self::Unchanged { rel, .. } => rel,
            Self::Read(f) => &f.rel,
        }
    }
}

fn read_candidate(
    vault: &Vault,
    rel: String,
    cache: &HashMap<String, CachedFile>,
    existing: &HashMap<Uuid, NoteRecord>,
    report: &mut ReindexReport,
) -> Result<Option<Candidate>> {
    let abs = vault.resolve(&RelPath::new(&rel)?)?;
    let before = fs::metadata(&abs).ok();
    if let (Some(m), Some(cached)) = (&before, cache.get(&rel)) {
        let current = existing
            .get(&cached.id)
            .is_some_and(|r| r.path == rel && r.content_hash == cached.hash);
        if current && m.modified().is_ok_and(|t| cached.trusted_for(m.len(), t)) {
            return Ok(Some(Candidate::Unchanged {
                rel,
                cached: cached.clone(),
            }));
        }
    }
    let read_at = SystemTime::now();
    let bytes = match fs::read(&abs) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(path = %rel, error = %e, "cannot read note during reindex");
            report.unreadable.push(rel);
            return Ok(None);
        }
    };
    let now = Utc::now();
    let fs_meta = fs::metadata(&abs).ok();
    let modified = fs_meta.as_ref().and_then(|m| m.modified().ok());
    let updated_at = fs_meta.as_ref().map_or(now, |m| to_utc(m.modified(), now));
    let created_at = fs_meta
        .as_ref()
        .map_or(updated_at, |m| to_utc(m.created(), updated_at));
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let meta = parse_meta(&text);
    let id = meta.id.unwrap_or_else(|| note_id_for_path(&rel));
    Ok(Some(Candidate::Read(ReadFile {
        hash: content_hash(&bytes),
        len: bytes.len() as u64,
        rel,
        text,
        meta,
        id,
        modified,
        read_at,
        created_at,
        updated_at,
    })))
}

/// Gives a note that shares its id with another one a fresh id, in its frontmatter.
/// Returns false (and leaves the file alone) if it was modified too recently.
fn fix_duplicate(vault: &Vault, file: &mut ReadFile, report: &mut ReindexReport) -> bool {
    let settled = file
        .modified
        .and_then(|t| file.read_at.duration_since(t).ok())
        .is_some_and(|age| age >= SETTLE);
    if !settled {
        tracing::info!(path = %file.rel, "duplicate note id on a fresh file; fixing it later");
        report.deferred.push(file.rel.clone());
        return false;
    }
    let fresh = Uuid::new_v4();
    let rewritten = set_meta(
        &file.text,
        &MetaUpdate {
            id: Some(fresh),
            ..Default::default()
        },
    );
    let Ok(rel_path) = RelPath::new(&file.rel) else {
        return false;
    };
    match vault.write(&rel_path, rewritten.as_bytes()) {
        Ok(()) => {
            tracing::warn!(path = %file.rel, "duplicate note id replaced");
            file.hash = content_hash(rewritten.as_bytes());
            file.text = rewritten;
            file.id = fresh;
            // The file changed under the cached stat; make sure it is read next time.
            file.modified = None;
            report.duplicate_ids_fixed.push(file.rel.clone());
            true
        }
        Err(e) => {
            tracing::warn!(path = %file.rel, error = %e, "cannot fix duplicate id; skipping note");
            report.unreadable.push(file.rel.clone());
            false
        }
    }
}

fn scan(
    vault: &Vault,
    cache: &HashMap<String, CachedFile>,
    existing: &HashMap<Uuid, NoteRecord>,
) -> Result<Scan> {
    let mut report = ReindexReport::default();
    let found = walk(vault, &mut report)?;

    let mut folders = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for (rel, is_dir) in found {
        if is_dir {
            folders.push(rel);
        } else if let Some(c) = read_candidate(vault, rel, cache, existing, &mut report)? {
            candidates.push(c);
        }
    }

    // Which candidate keeps each id: the path the index already has for it, else the first.
    let mut keeper: HashMap<Uuid, usize> = HashMap::new();
    for (i, c) in candidates.iter().enumerate() {
        let id = c.id();
        let indexed_here = existing.get(&id).is_some_and(|r| r.path == c.rel());
        match keeper.get(&id) {
            None => {
                keeper.insert(id, i);
            }
            Some(&k) => {
                let k_indexed = existing
                    .get(&id)
                    .is_some_and(|r| r.path == candidates[k].rel());
                if indexed_here && !k_indexed {
                    keeper.insert(id, i);
                }
            }
        }
    }
    let mut notes: Vec<IndexEntry> = Vec::new();
    let mut unchanged = Vec::new();
    let mut new_cache = HashMap::new();
    for (i, c) in candidates.into_iter().enumerate() {
        let mut file = match c {
            Candidate::Unchanged { rel, cached } => {
                unchanged.push(cached.id);
                new_cache.insert(rel, cached);
                continue;
            }
            Candidate::Read(f) => f,
        };
        if keeper.get(&file.id) != Some(&i) && !fix_duplicate(vault, &mut file, &mut report) {
            continue;
        }
        if let Some(modified) = file.modified {
            new_cache.insert(
                file.rel.clone(),
                CachedFile {
                    len: file.len,
                    modified,
                    read_at: file.read_at,
                    id: file.id,
                    hash: file.hash.clone(),
                },
            );
        }
        notes.push(IndexEntry {
            record: NoteRecord {
                id: file.id,
                title: title_from_path(&file.rel),
                created_at: file.created_at,
                updated_at: file.updated_at,
                content_hash: file.hash,
                pinned: file.meta.pinned,
                archived: file.meta.archived,
                last_opened_at: None,
                path: file.rel,
            },
            extracted: extract(&file.text),
        });
    }

    report.notes = notes.len() + unchanged.len();
    report.folders = folders.len();
    Ok(Scan {
        notes,
        unchanged,
        folders,
        report,
        cache: new_cache,
    })
}

/// Rebuilds the index from the vault, reading every file. Existing `created_at`
/// and `last_opened_at` values are preserved for notes whose id is unchanged.
///
/// # Errors
/// [`Error::Io`] if the vault cannot be traversed; [`Error::Database`] if the
/// index cannot be written (the previous index is then left as it was).
pub async fn reindex(vault: &Vault, db: &Database) -> Result<ReindexReport> {
    reindex_cached(vault, db, &ScanCache::default()).await
}

/// Like [`reindex`], but skips reading files that `cache` proves unchanged, and
/// updates the cache.
///
/// # Errors
/// See [`reindex`].
pub async fn reindex_cached(
    vault: &Vault,
    db: &Database,
    cache: &ScanCache,
) -> Result<ReindexReport> {
    let existing: HashMap<Uuid, NoteRecord> = db
        .list_notes()
        .await?
        .into_iter()
        .map(|n| (n.id, n))
        .collect();
    // The derived data is only trustworthy if it was produced by the current extractor.
    let incremental =
        db.get_setting(EXTRACT_VERSION_KEY).await?.as_deref() == Some(EXTRACT_VERSION);
    let known = if incremental {
        cache.snapshot()
    } else {
        HashMap::new()
    };

    let v = vault.clone();
    let ex = existing.clone();
    let mut scanned = tokio::task::spawn_blocking(move || scan(&v, &known, &ex))
        .await
        .map_err(|e| Error::Io {
            path: vault.root().to_owned(),
            source: std::io::Error::other(e),
        })??;

    for entry in &mut scanned.notes {
        if let Some(old) = existing.get(&entry.record.id) {
            entry.record.created_at = old.created_at;
            entry.record.last_opened_at = old.last_opened_at;
        }
    }

    let mut scanned_ids: HashSet<Uuid> = scanned.notes.iter().map(|e| e.record.id).collect();
    scanned_ids.extend(scanned.unchanged.iter().copied());
    let mut removed: Vec<Uuid> = existing
        .keys()
        .filter(|id| !scanned_ids.contains(id))
        .copied()
        .collect();
    removed.sort();
    let mut report = scanned.report;
    if incremental {
        // Only touch what differs from the index; an unchanged vault writes nothing.
        let changed: Vec<IndexEntry> = scanned
            .notes
            .into_iter()
            .filter(|e| {
                existing
                    .get(&e.record.id)
                    .is_none_or(|old| !same_note(old, &e.record))
            })
            .collect();
        report.folders_changed = db.sync_index(&changed, &removed, &scanned.folders).await?;
        report.changed = changed.iter().map(|e| e.record.id).collect();
    } else {
        db.replace_index(&scanned.notes, &scanned.folders).await?;
        db.set_setting(EXTRACT_VERSION_KEY, EXTRACT_VERSION).await?;
        report.folders_changed = true;
        report.changed = scanned.notes.iter().map(|e| e.record.id).collect();
    }
    report.removed = removed;
    cache.replace(scanned.cache);
    Ok(report)
}

/// Bump when `extract` (or the derived tables) change what is stored for a note, so
/// existing indexes are rebuilt in full once instead of trusting stale derived data.
const EXTRACT_VERSION: &str = "1";
const EXTRACT_VERSION_KEY: &str = "extract_version";

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

    /// Backdates a file's modification time, as if it was written a while ago.
    fn age(path: &std::path::Path) {
        let then = SystemTime::now() - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(then)
            .unwrap();
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
        age(&vault.root().join("b.md"));

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

    /// Index of the vault built from scratch in a separate database, as ground truth.
    async fn fresh_snapshot(vault: &Vault) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("fresh.db")).await.unwrap();
        reindex(vault, &db).await.unwrap();
        db.derived_snapshot().await.unwrap()
    }

    #[tokio::test]
    async fn incremental_reindex_matches_a_fresh_rebuild_after_external_changes() {
        let (dir, vault, db) = setup().await;
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let md = |id: Uuid, body: &str| format!("---\nid: {id}\n---\n{body}");
        vault.create_dir(&rel("Sub")).unwrap();
        vault
            .create_new(&rel("a.md"), md(a, "alpha #one [[b]]").as_bytes())
            .unwrap();
        vault
            .create_new(&rel("b.md"), md(b, "beta #two [[a]]").as_bytes())
            .unwrap();
        vault.create_new(&rel("keep.md"), b"keep #one").unwrap();
        vault.create_new(&rel("gone.md"), b"gone #only").unwrap();
        vault.create_new(&rel("Sub/old.md"), b"old").unwrap();

        reindex(&vault, &db).await.unwrap();
        // Nothing changed: the index must come out identical.
        let before = db.derived_snapshot().await.unwrap();
        reindex(&vault, &db).await.unwrap();
        assert_eq!(db.derived_snapshot().await.unwrap(), before);

        // External edits: change content, delete, add, swap two notes' paths, new folder.
        let root = dir.path();
        fs::write(root.join("keep.md"), "keep #three [[a]]").unwrap();
        fs::remove_file(root.join("gone.md")).unwrap();
        fs::write(root.join("new.md"), "new #one").unwrap();
        fs::rename(root.join("a.md"), root.join("tmp.md")).unwrap();
        fs::rename(root.join("b.md"), root.join("a.md")).unwrap();
        fs::rename(root.join("tmp.md"), root.join("b.md")).unwrap();
        fs::rename(root.join("Sub/old.md"), root.join("moved.md")).unwrap();
        fs::remove_dir(root.join("Sub")).unwrap();
        fs::create_dir(root.join("Fresh")).unwrap();

        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.notes, 5);
        assert_eq!(
            db.derived_snapshot().await.unwrap(),
            fresh_snapshot(&vault).await
        );
        assert_eq!(db.list_folders().await.unwrap(), vec!["Fresh"]);
        // The swapped notes kept their ids at their new paths.
        assert_eq!(db.get_note(a).await.unwrap().unwrap().path, "b.md");
        assert_eq!(db.get_note(b).await.unwrap().unwrap().path, "a.md");
    }

    #[tokio::test]
    async fn incremental_reindex_keeps_created_and_last_opened() {
        let (_d, vault, db) = setup().await;
        vault.create_new(&rel("n.md"), b"n").unwrap();
        reindex(&vault, &db).await.unwrap();
        let note = db.list_notes().await.unwrap().remove(0);
        let at = Utc::now();
        db.set_last_opened(note.id, at).await.unwrap();
        vault.write(&rel("n.md"), b"n changed").unwrap();
        reindex(&vault, &db).await.unwrap();
        let after = db.list_notes().await.unwrap().remove(0);
        assert_eq!(after.created_at, note.created_at);
        assert_eq!(
            after.last_opened_at.map(|t| t.timestamp()),
            Some(at.timestamp())
        );
        assert_ne!(after.content_hash, note.content_hash);
    }

    #[tokio::test]
    async fn an_index_from_an_older_extractor_is_rebuilt_in_full() {
        let (_d, vault, db) = setup().await;
        vault.create_new(&rel("n.md"), b"hello #tag").unwrap();
        reindex(&vault, &db).await.unwrap();
        // Simulate stale derived data from an older extractor, then a version bump.
        sqlx::query("DELETE FROM note_tags")
            .execute(db.pool())
            .await
            .unwrap();
        db.set_setting(EXTRACT_VERSION_KEY, "0").await.unwrap();
        reindex(&vault, &db).await.unwrap();
        let tags: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM note_tags")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(tags, 1);
    }

    #[tokio::test]
    async fn a_fresh_duplicate_is_left_alone_until_it_settles() {
        let (_d, vault, db) = setup().await;
        let id = Uuid::new_v4();
        let body = format!("---\nid: {id}\n---\nSame");
        vault.create_new(&rel("a.md"), body.as_bytes()).unwrap();
        age(&vault.root().join("a.md"));
        vault.create_new(&rel("b.md"), body.as_bytes()).unwrap();

        // b.md may still be being copied: it is neither rewritten nor indexed yet.
        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.deferred, vec!["b.md"]);
        assert!(report.duplicate_ids_fixed.is_empty());
        assert_eq!(vault.read(&rel("b.md")).unwrap(), body.as_bytes());
        assert!(db.get_note_by_path("b.md").await.unwrap().is_none());

        age(&vault.root().join("b.md"));
        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.duplicate_ids_fixed, vec!["b.md"]);
        assert!(report.deferred.is_empty());
    }

    #[tokio::test]
    async fn the_indexed_note_keeps_its_id_when_a_copy_sorts_before_it() {
        let (_d, vault, db) = setup().await;
        let id = Uuid::new_v4();
        let body = format!("---\nid: {id}\n---\nOriginal");
        vault.create_new(&rel("b.md"), body.as_bytes()).unwrap();
        reindex(&vault, &db).await.unwrap();
        // Copied outside NoteNest to a name that sorts first.
        vault.create_new(&rel("a.md"), body.as_bytes()).unwrap();
        age(&vault.root().join("a.md"));

        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.duplicate_ids_fixed, vec!["a.md"]);
        assert_eq!(db.get_note(id).await.unwrap().unwrap().path, "b.md");
        // The original file is untouched.
        assert_eq!(vault.read(&rel("b.md")).unwrap(), body.as_bytes());
    }

    #[tokio::test]
    async fn reports_what_changed() {
        let (dir, vault, db) = setup().await;
        vault.create_new(&rel("a.md"), b"A").unwrap();
        vault.create_new(&rel("b.md"), b"B").unwrap();
        let first = reindex(&vault, &db).await.unwrap();
        assert_eq!(first.changed.len(), 2);
        assert!(first.folders_changed);

        let nothing = reindex(&vault, &db).await.unwrap();
        assert!(nothing.changed.is_empty() && nothing.removed.is_empty());
        assert!(!nothing.folders_changed);

        let a = note_id_for_path("a.md");
        let b = note_id_for_path("b.md");
        fs::write(dir.path().join("a.md"), "A edited").unwrap();
        fs::remove_file(dir.path().join("b.md")).unwrap();
        fs::create_dir(dir.path().join("Folder")).unwrap();
        let report = reindex(&vault, &db).await.unwrap();
        assert_eq!(report.changed, vec![a]);
        assert_eq!(report.removed, vec![b]);
        assert!(report.folders_changed);
    }

    async fn path_hash(db: &Database, path: &str) -> String {
        db.get_note_by_path(path)
            .await
            .unwrap()
            .unwrap()
            .content_hash
    }

    #[tokio::test]
    async fn the_cache_skips_settled_files_with_the_same_size_and_time() {
        let (dir, vault, db) = setup().await;
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        age(&path);
        let cache = ScanCache::default();
        reindex_cached(&vault, &db, &cache).await.unwrap();
        let before = path_hash(&db, "a.md").await;

        // Same size and modification time: proven unchanged without reading it.
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, "two").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let report = reindex_cached(&vault, &db, &cache).await.unwrap();
        assert!(report.changed.is_empty());
        assert_eq!(path_hash(&db, "a.md").await, before);

        // Without the cache every file is read again.
        reindex(&vault, &db).await.unwrap();
        assert_ne!(path_hash(&db, "a.md").await, before);
    }

    #[tokio::test]
    async fn the_cache_rereads_changed_and_freshly_written_files() {
        let (dir, vault, db) = setup().await;
        let path = dir.path().join("a.md");
        let cache = ScanCache::default();

        // A different size is always read.
        fs::write(&path, "one").unwrap();
        age(&path);
        reindex_cached(&vault, &db, &cache).await.unwrap();
        fs::write(&path, "longer").unwrap();
        age(&path);
        let report = reindex_cached(&vault, &db, &cache).await.unwrap();
        assert_eq!(report.changed.len(), 1);

        // A file read moments after it was written is not trusted by its timestamp:
        // a second same-size write within the same tick would be invisible otherwise.
        fs::write(&path, "fresh1").unwrap();
        reindex_cached(&vault, &db, &cache).await.unwrap();
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, "fresh2").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let report = reindex_cached(&vault, &db, &cache).await.unwrap();
        assert_eq!(report.changed.len(), 1);
        let text = String::from_utf8(vault.read(&rel("a.md")).unwrap()).unwrap();
        assert_eq!(text, "fresh2");
        assert_eq!(path_hash(&db, "a.md").await, content_hash(text.as_bytes()));
    }

    #[tokio::test]
    async fn the_cache_does_not_hide_an_index_that_changed_behind_it() {
        let (dir, vault, db) = setup().await;
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        age(&path);
        let cache = ScanCache::default();
        reindex_cached(&vault, &db, &cache).await.unwrap();
        // Something else changed the index row; the cached stat must not be trusted.
        let id = note_id_for_path("a.md");
        db.delete_note(id).await.unwrap();
        let report = reindex_cached(&vault, &db, &cache).await.unwrap();
        assert_eq!(report.changed, vec![id]);
        assert!(db.get_note(id).await.unwrap().is_some());
    }
}
