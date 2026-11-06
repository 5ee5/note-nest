//! The workspace: a vault plus its index, and every operation the app performs
//! on notes and folders.
//!
//! Invariants:
//! - Markdown files are the source of truth; the index follows them.
//! - Writes are atomic and never overwrite changes made outside NoteNest unless
//!   the caller explicitly asks, in which case the on-disk version is backed up first.
//! - Deleting moves to the trash; nothing is destroyed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::reindex::{reindex, ReindexReport};
use crate::database::Database;
use crate::filesystem::atomic::write_atomic;
use crate::filesystem::trash::TrashEntry;
use crate::filesystem::{RelPath, Vault, ATTACHMENTS_DIR};
use crate::frontmatter::{parse_meta, set_meta, MetaUpdate};
use crate::hash::content_hash;
use crate::markdown::{render_markdown, ImageContext, RenderLimits, Rendered};
use crate::models::{note_id_for_path, title_from_path, NoteRecord};
use crate::{Error, Result};

const DEFAULT_TITLE: &str = "Untitled";
const MAX_NAME_ATTEMPTS: usize = 10_000;

/// A note's index record plus its full file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteContent {
    pub record: NoteRecord,
    /// Entire file, including any frontmatter.
    pub content: String,
}

/// How [`Workspace::save_note`] treats changes made outside NoteNest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMode<'a> {
    /// Save only if the file on disk still has this content hash; otherwise
    /// return [`Error::Conflict`] and write nothing.
    Expect(&'a str),
    /// Save regardless, after backing up the on-disk version if it differs.
    Overwrite,
}

#[derive(Debug)]
pub struct OpenReport {
    /// The index file was unusable and has been rebuilt from the vault.
    pub index_rebuilt: bool,
    /// Where the unusable index was kept, if it was rebuilt.
    pub index_backup: Option<PathBuf>,
    pub reindex: ReindexReport,
}

/// Cheap to clone; clones share one write lock.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub(super) vault: Vault,
    pub(super) db: Database,
    pub(super) write_lock: Arc<Mutex<()>>,
}

fn utc(t: std::io::Result<std::time::SystemTime>) -> Option<DateTime<Utc>> {
    t.ok().map(DateTime::<Utc>::from)
}

impl Workspace {
    /// Opens the vault at `root` (creating the directory when `create` is set),
    /// recovers the index if needed, and reconciles it with the files on disk.
    ///
    /// # Errors
    /// If the vault cannot be opened or the index cannot be created.
    pub async fn open(root: &Path, create: bool) -> Result<(Self, OpenReport)> {
        let vault = if create {
            Vault::create(root)?
        } else {
            Vault::open(root)?
        };
        let opened = Database::open_recovering(&vault.meta_dir().join("index.db")).await?;
        let ws = Self {
            vault,
            db: opened.database,
            write_lock: Arc::new(Mutex::new(())),
        };
        let report = reindex(&ws.vault, &ws.db).await?;
        Ok((
            ws,
            OpenReport {
                index_rebuilt: opened.rebuilt,
                index_backup: opened.backup,
                reindex: report,
            },
        ))
    }

    #[must_use]
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Rebuilds the index from the vault.
    ///
    /// # Errors
    /// See [`reindex`].
    pub async fn reindex(&self) -> Result<ReindexReport> {
        let _g = self.write_lock.lock().await;
        reindex(&self.vault, &self.db).await
    }

    // ---------------------------------------------------------------- queries

    /// # Errors
    /// [`Error::Database`] on failure.
    pub async fn list_notes(&self) -> Result<Vec<NoteRecord>> {
        self.db.list_notes().await
    }

    /// # Errors
    /// [`Error::Database`] on failure.
    pub async fn list_folders(&self) -> Result<Vec<String>> {
        let mut folders = self.db.list_folders().await?;
        folders.retain(|f| !is_attachments_path(f));
        Ok(folders)
    }

    /// # Errors
    /// [`Error::Io`] if the trash cannot be read.
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>> {
        self.vault.list_trash()
    }

    /// # Errors
    /// [`Error::Database`] on failure.
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.db.get_setting(key).await
    }

    /// # Errors
    /// [`Error::Database`] on failure.
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db.set_setting(key, value).await
    }

    /// # Errors
    /// [`Error::NotFound`] for an unknown id.
    pub async fn mark_opened(&self, id: Uuid) -> Result<()> {
        self.record(id).await?;
        self.db.set_last_opened(id, Utc::now()).await
    }

    // ------------------------------------------------------------------ notes

    /// Reads a note. If the file changed outside NoteNest the index is refreshed
    /// first, so the returned record always describes the returned content.
    ///
    /// # Errors
    /// [`Error::NotFound`]; [`Error::Corrupt`] if the file is not valid UTF-8
    /// (it is never opened, so it cannot be altered by saving).
    pub async fn read_note(&self, id: Uuid) -> Result<NoteContent> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let path = RelPath::new(&record.path)?;
        let bytes = self.vault.read(&path)?;
        let content = String::from_utf8(bytes.clone()).map_err(|_| Error::Corrupt {
            context: record.path.clone(),
            message: "not valid UTF-8; refusing to open it so it cannot be altered".into(),
        })?;
        let record = if content_hash(&bytes) == record.content_hash {
            record
        } else {
            let fresh = self.build_record(&path, &bytes, Some(&record));
            self.store(Some(&record), &fresh).await?;
            fresh
        };
        Ok(NoteContent { record, content })
    }

    /// Creates a note in `folder` (vault root if `None`). Without a `title`, a free
    /// "Untitled", "Untitled 2", ... name is chosen. A note created here carries
    /// its id in frontmatter.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] for a bad title/folder; [`Error::AlreadyExists`] if an
    /// explicit title is taken; [`Error::NotFound`] if the folder is missing.
    pub async fn create_note(
        &self,
        folder: Option<&str>,
        title: Option<&str>,
    ) -> Result<NoteRecord> {
        let _g = self.write_lock.lock().await;
        let dir = self.folder_path(folder)?;
        let path = match title {
            Some(t) => child(dir.as_ref(), &format!("{t}.md"))?,
            None => self.unique_note_path(dir.as_ref(), DEFAULT_TITLE)?,
        };
        let id = Uuid::new_v4();
        let content = set_meta(
            "",
            &MetaUpdate {
                id: Some(id),
                ..Default::default()
            },
        );
        self.vault.create_new(&path, content.as_bytes())?;
        let record = self.build_record(&path, content.as_bytes(), None);
        self.store(None, &record).await?;
        Ok(record)
    }

    /// Saves new content for a note. See [`SaveMode`].
    ///
    /// NoteNest owns a note's frontmatter `id`: if the note had one on disk and the
    /// new content dropped or changed it, the original is restored. The returned
    /// [`NoteContent`] holds the content exactly as written, so callers can
    /// resynchronize their buffer when it differs from what they sent.
    ///
    /// # Errors
    /// [`Error::Conflict`] when `mode` is `Expect` and the file changed;
    /// [`Error::NotFound`] if the file vanished; [`Error::Io`] if writing fails (the
    /// previous file content is then intact).
    pub async fn save_note(
        &self,
        id: Uuid,
        content: &str,
        mode: SaveMode<'_>,
    ) -> Result<NoteContent> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let path = RelPath::new(&record.path)?;
        let on_disk = self.vault.read(&path)?;
        let disk_hash = content_hash(&on_disk);
        match mode {
            SaveMode::Expect(expected) if expected != disk_hash => {
                return Err(Error::Conflict {
                    path: record.path,
                    expected: expected.to_owned(),
                    actual: disk_hash,
                });
            }
            SaveMode::Overwrite if on_disk != content.as_bytes() => {
                self.backup_conflicting(id, &on_disk)?;
            }
            _ => {}
        }
        let to_write = preserve_id(&on_disk, content);
        self.vault.write(&path, to_write.as_bytes())?;
        self.clear_draft_if_saved(id, content);
        let fresh = self.build_record(&path, to_write.as_bytes(), Some(&record));
        self.store(Some(&record), &fresh).await?;
        Ok(NoteContent {
            record: fresh,
            content: to_write,
        })
    }

    /// Renames a note (its title is its file name).
    ///
    /// # Errors
    /// [`Error::AlreadyExists`] if the name is taken; [`Error::InvalidPath`] for a bad name.
    pub async fn rename_note(&self, id: Uuid, new_title: &str) -> Result<NoteRecord> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let from = RelPath::new(&record.path)?;
        let to = child(from.parent().as_ref(), &format!("{new_title}.md"))?;
        self.relocate(&record, &from, &to).await
    }

    /// Moves a note to `folder` (vault root if `None`), keeping its name.
    ///
    /// # Errors
    /// [`Error::AlreadyExists`] if the destination has a note of that name.
    pub async fn move_note(&self, id: Uuid, folder: Option<&str>) -> Result<NoteRecord> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let from = RelPath::new(&record.path)?;
        let dir = self.folder_path(folder)?;
        let to = child(dir.as_ref(), from.file_name())?;
        self.relocate(&record, &from, &to).await
    }

    /// Copies a note to "<title> copy.md" (numbered if needed) with a fresh id.
    ///
    /// # Errors
    /// [`Error::NotFound`] if the source file is gone.
    pub async fn duplicate_note(&self, id: Uuid) -> Result<NoteRecord> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let from = RelPath::new(&record.path)?;
        let bytes = self.vault.read(&from)?;
        let text = String::from_utf8(bytes).map_err(|_| Error::Corrupt {
            context: record.path.clone(),
            message: "not valid UTF-8; refusing to duplicate it".into(),
        })?;
        let new_id = Uuid::new_v4();
        let copy = set_meta(
            &text,
            &MetaUpdate {
                id: Some(new_id),
                ..Default::default()
            },
        );
        let to =
            self.unique_note_path(from.parent().as_ref(), &format!("{} copy", record.title))?;
        self.vault.create_new(&to, copy.as_bytes())?;
        let new = self.build_record(&to, copy.as_bytes(), None);
        self.store(None, &new).await?;
        Ok(new)
    }

    /// Pins or unpins a note. The state lives in the note's frontmatter, so it
    /// survives an index rebuild. Edits only the frontmatter of the current file.
    ///
    /// # Errors
    /// [`Error::Corrupt`] if the file is not valid UTF-8.
    pub async fn set_pinned(&self, id: Uuid, pinned: bool) -> Result<NoteRecord> {
        self.update_flags(
            id,
            MetaUpdate {
                pinned: Some(pinned),
                ..Default::default()
            },
        )
        .await
    }

    /// Archives or unarchives a note (stored in frontmatter, like pinning).
    ///
    /// # Errors
    /// [`Error::Corrupt`] if the file is not valid UTF-8.
    pub async fn set_archived(&self, id: Uuid, archived: bool) -> Result<NoteRecord> {
        self.update_flags(
            id,
            MetaUpdate {
                archived: Some(archived),
                ..Default::default()
            },
        )
        .await
    }

    /// Moves a note to the trash.
    ///
    /// # Errors
    /// [`Error::NotFound`] if the note or file is missing.
    pub async fn delete_note(&self, id: Uuid) -> Result<TrashEntry> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let entry = self.vault.trash(&RelPath::new(&record.path)?)?;
        self.db.delete_note(id).await?;
        Ok(entry)
    }

    /// Renders note text to sanitized HTML for the preview. Relative image paths are
    /// resolved from the folder of note `id`; without an id no images are inlined.
    /// Runs off the async executor so a large note cannot stall the UI.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown id; [`Error::TooLarge`] for oversized input.
    pub async fn render_markdown(&self, id: Option<Uuid>, content: String) -> Result<Rendered> {
        let images = match id {
            Some(id) => {
                let record = self.record(id).await?;
                Some(ImageContext {
                    vault: self.vault.clone(),
                    note_dir: RelPath::new(&record.path)?.parent(),
                })
            }
            None => None,
        };
        tokio::task::spawn_blocking(move || {
            render_markdown(&content, images, &RenderLimits::default())
        })
        .await
        .map_err(|e| Error::Io {
            path: PathBuf::from("<markdown renderer>"),
            source: std::io::Error::other(e),
        })?
    }

    // ---------------------------------------------------------------- folders

    /// Creates a folder (and missing parents).
    ///
    /// # Errors
    /// [`Error::AlreadyExists`] if it exists.
    pub async fn create_folder(&self, path: &str) -> Result<()> {
        let _g = self.write_lock.lock().await;
        let rel = RelPath::new(path)?;
        reject_reserved(&rel)?;
        self.vault.create_dir(&rel)?;
        self.db.ensure_folder(rel.as_str()).await
    }

    /// Renames a folder (last path component).
    ///
    /// # Errors
    /// [`Error::AlreadyExists`] if the name is taken.
    pub async fn rename_folder(&self, path: &str, new_name: &str) -> Result<()> {
        let _g = self.write_lock.lock().await;
        let from = RelPath::new(path)?;
        let to = child(from.parent().as_ref(), new_name)?;
        reject_reserved(&from)?;
        reject_reserved(&to)?;
        self.vault.rename(&from, &to)?;
        reindex(&self.vault, &self.db).await.map(|_| ())
    }

    /// Moves a folder under `new_parent` (vault root if `None`).
    ///
    /// # Errors
    /// [`Error::InvalidPath`] when moving a folder into itself.
    pub async fn move_folder(&self, path: &str, new_parent: Option<&str>) -> Result<()> {
        let _g = self.write_lock.lock().await;
        let from = RelPath::new(path)?;
        let dir = self.folder_path(new_parent)?;
        let to = child(dir.as_ref(), from.file_name())?;
        self.vault.rename(&from, &to)?;
        reindex(&self.vault, &self.db).await.map(|_| ())
    }

    /// Moves a folder and everything in it to the trash.
    ///
    /// # Errors
    /// [`Error::NotFound`] if it does not exist.
    pub async fn delete_folder(&self, path: &str) -> Result<TrashEntry> {
        let _g = self.write_lock.lock().await;
        let entry = self.vault.trash(&RelPath::new(path)?)?;
        reindex(&self.vault, &self.db).await?;
        Ok(entry)
    }

    /// Restores a trashed note or folder to its original location.
    ///
    /// # Errors
    /// [`Error::AlreadyExists`] if the original location is occupied.
    pub async fn restore(&self, trash_id: Uuid) -> Result<RelPath> {
        let _g = self.write_lock.lock().await;
        let path = self.vault.restore(trash_id)?;
        reindex(&self.vault, &self.db).await?;
        Ok(path)
    }

    // ---------------------------------------------------------------- helpers

    async fn record(&self, id: Uuid) -> Result<NoteRecord> {
        self.db
            .get_note(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("note {id}")))
    }

    fn folder_path(&self, folder: Option<&str>) -> Result<Option<RelPath>> {
        let Some(f) = folder else { return Ok(None) };
        let rel = RelPath::new(f)?;
        if !self.vault.resolve(&rel)?.is_dir() {
            return Err(Error::NotFound(format!("folder {rel}")));
        }
        Ok(Some(rel))
    }

    fn unique_note_path(&self, dir: Option<&RelPath>, base: &str) -> Result<RelPath> {
        for n in 1..=MAX_NAME_ATTEMPTS {
            let name = if n == 1 {
                base.to_owned()
            } else {
                format!("{base} {n}")
            };
            let path = child(dir, &format!("{name}.md"))?;
            if !self.vault.exists(&path) {
                return Ok(path);
            }
        }
        Err(Error::AlreadyExists(format!("no free name for {base:?}")))
    }

    /// Builds an index record from file bytes. `prev` supplies `created_at` and
    /// `last_opened_at` of an existing record.
    fn build_record(&self, path: &RelPath, bytes: &[u8], prev: Option<&NoteRecord>) -> NoteRecord {
        let text = String::from_utf8_lossy(bytes);
        let meta = parse_meta(&text);
        let fs_meta = self
            .vault
            .resolve(path)
            .ok()
            .and_then(|p| std::fs::metadata(p).ok());
        let now = Utc::now();
        let updated_at = fs_meta
            .as_ref()
            .and_then(|m| utc(m.modified()))
            .unwrap_or(now);
        let created_at = prev
            .map(|p| p.created_at)
            .or_else(|| fs_meta.as_ref().and_then(|m| utc(m.created())))
            .unwrap_or(updated_at);
        NoteRecord {
            id: meta.id.unwrap_or_else(|| note_id_for_path(path.as_str())),
            title: title_from_path(path.as_str()),
            path: path.to_string(),
            created_at,
            updated_at,
            content_hash: content_hash(bytes),
            pinned: meta.pinned,
            archived: meta.archived,
            last_opened_at: prev.and_then(|p| p.last_opened_at),
        }
    }

    /// Writes `new` to the index, replacing `old` when the id changed.
    async fn store(&self, old: Option<&NoteRecord>, new: &NoteRecord) -> Result<()> {
        if let Some(old) = old {
            if old.id != new.id {
                self.db.delete_note(old.id).await?;
            }
        }
        self.db.upsert_note(new).await?;
        if let Some(at) = new.last_opened_at {
            self.db.set_last_opened(new.id, at).await?;
        }
        Ok(())
    }

    async fn relocate(
        &self,
        record: &NoteRecord,
        from: &RelPath,
        to: &RelPath,
    ) -> Result<NoteRecord> {
        if from == to {
            return Ok(record.clone());
        }
        self.vault.rename(from, to)?;
        let bytes = self.vault.read(to)?;
        let fresh = self.build_record(to, &bytes, Some(record));
        self.store(Some(record), &fresh).await?;
        Ok(fresh)
    }

    async fn update_flags(&self, id: Uuid, mut update: MetaUpdate) -> Result<NoteRecord> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let path = RelPath::new(&record.path)?;
        let bytes = self.vault.read(&path)?;
        let text = String::from_utf8(bytes).map_err(|_| Error::Corrupt {
            context: record.path.clone(),
            message: "not valid UTF-8; refusing to modify it".into(),
        })?;
        // Pin the note's current identity into the file so it survives a rebuild.
        update.id = Some(parse_meta(&text).id.unwrap_or(record.id));
        let new_text = set_meta(&text, &update);
        self.vault.write(&path, new_text.as_bytes())?;
        let fresh = self.build_record(&path, new_text.as_bytes(), Some(&record));
        self.store(Some(&record), &fresh).await?;
        Ok(fresh)
    }

    /// Keeps a copy of changes made outside NoteNest before they are overwritten.
    fn backup_conflicting(&self, id: Uuid, bytes: &[u8]) -> Result<()> {
        let dir = self.vault.meta_dir().join("recovery").join("conflicts");
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3f");
        write_atomic(&dir.join(format!("{id}-{stamp}.md")), bytes)
    }
}

/// What `save_note` writes for `content`: the same text, except that a note's
/// frontmatter `id` (if the file on disk has one) is kept.
pub(super) fn preserve_id(on_disk: &[u8], content: &str) -> String {
    match parse_meta(&String::from_utf8_lossy(on_disk)).id {
        Some(k) if parse_meta(content).id != Some(k) => set_meta(
            content,
            &MetaUpdate {
                id: Some(k),
                ..Default::default()
            },
        ),
        _ => content.to_owned(),
    }
}

/// True for the attachments folder and anything beneath it.
pub(super) fn is_attachments_path(path: &str) -> bool {
    path == ATTACHMENTS_DIR || path.starts_with(&format!("{ATTACHMENTS_DIR}/"))
}

fn reject_reserved(path: &RelPath) -> Result<()> {
    if is_attachments_path(path.as_str()) {
        return Err(Error::InvalidPath(format!(
            "{path}: the {ATTACHMENTS_DIR} folder is reserved for image attachments"
        )));
    }
    Ok(())
}

/// Joins a single name onto `dir`; names containing `/` are rejected so a
/// title can never create nested paths.
fn child(dir: Option<&RelPath>, name: &str) -> Result<RelPath> {
    if name.contains('/') {
        return Err(Error::InvalidPath(format!(
            "{name:?}: name must not contain '/'"
        )));
    }
    match dir {
        Some(d) => d.join(name),
        None => RelPath::new(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `Database::close` can return before SQLite has finished closing the files; the
    /// WAL files disappear once the last connection is really closed. Tests that edit
    /// the index file afterwards must wait for that, or the late close can rewrite it.
    fn wait_for_close(dir: &tempfile::TempDir) {
        let meta = dir.path().join(".notenest");
        for _ in 0..300 {
            if !meta.join("index.db-wal").exists() && !meta.join("index.db-shm").exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    async fn open() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
        (dir, ws)
    }

    fn disk(dir: &tempfile::TempDir, rel: &str) -> String {
        fs::read_to_string(dir.path().join(rel)).unwrap()
    }

    #[tokio::test]
    async fn create_note_picks_free_names_and_stores_the_id_in_the_file() {
        let (dir, ws) = open().await;
        let a = ws.create_note(None, None).await.unwrap();
        let b = ws.create_note(None, None).await.unwrap();
        assert_eq!(
            (a.path.as_str(), b.path.as_str()),
            ("Untitled.md", "Untitled 2.md")
        );
        assert_eq!(parse_meta(&disk(&dir, "Untitled.md")).id, Some(a.id));
        assert!(matches!(
            ws.create_note(None, Some("Untitled")).await,
            Err(Error::AlreadyExists(_))
        ));
    }

    #[tokio::test]
    async fn create_note_in_a_folder_and_rejects_bad_input() {
        let (_d, ws) = open().await;
        ws.create_folder("Projects").await.unwrap();
        let n = ws
            .create_note(Some("Projects"), Some("Plan"))
            .await
            .unwrap();
        assert_eq!(n.path, "Projects/Plan.md");
        assert!(matches!(
            ws.create_note(Some("Nope"), None).await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            ws.create_note(None, Some("../x")).await,
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            ws.create_note(Some("../.."), None).await,
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            ws.create_note(None, Some("a/b")).await,
            Err(Error::InvalidPath(_))
        ));
    }

    #[tokio::test]
    async fn notes_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
            let n = ws.create_note(None, Some("Persist")).await.unwrap();
            ws.save_note(n.id, "hello", SaveMode::Overwrite)
                .await
                .unwrap();
            ws.database().close().await;
            n.id
        };
        let (ws, report) = Workspace::open(dir.path(), false).await.unwrap();
        assert!(!report.index_rebuilt);
        let got = ws.read_note(id).await.unwrap();
        assert!(got.content.ends_with("hello"));
        assert_eq!(got.record.id, id);
    }

    #[tokio::test]
    async fn save_detects_external_modification_and_writes_nothing() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        let loaded = ws.read_note(n.id).await.unwrap();

        fs::write(dir.path().join("A.md"), "edited in VS Code").unwrap();
        let err = ws
            .save_note(
                n.id,
                "my edit",
                SaveMode::Expect(&loaded.record.content_hash),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Conflict { .. }));
        assert_eq!(disk(&dir, "A.md"), "edited in VS Code");
    }

    #[tokio::test]
    async fn overwrite_backs_up_the_external_version_first() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        fs::write(dir.path().join("A.md"), "external").unwrap();

        ws.save_note(n.id, "mine", SaveMode::Overwrite)
            .await
            .unwrap();
        assert_eq!(disk(&dir, "A.md"), "mine");
        let backups: Vec<_> = fs::read_dir(dir.path().join(".notenest/recovery/conflicts"))
            .unwrap()
            .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        assert_eq!(backups, vec!["external"]);
    }

    #[tokio::test]
    async fn expected_hash_saves_succeed_and_chain() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        let mut hash = ws.read_note(n.id).await.unwrap().record.content_hash;
        for text in ["one", "two", "three"] {
            let rec = ws
                .save_note(n.id, text, SaveMode::Expect(&hash))
                .await
                .unwrap();
            assert_eq!(rec.record.id, n.id);
            hash = rec.record.content_hash;
        }
        assert!(disk(&dir, "A.md").ends_with("three"));
    }

    #[tokio::test]
    async fn read_absorbs_external_edits_into_the_index() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        fs::write(dir.path().join("A.md"), "changed outside").unwrap();
        let got = ws.read_note(n.id).await.unwrap();
        assert_eq!(got.content, "changed outside");
        assert_eq!(got.record.content_hash, content_hash(b"changed outside"));
        let listed = ws.list_notes().await.unwrap();
        assert_eq!(listed[0].content_hash, got.record.content_hash);
    }

    #[tokio::test]
    async fn failed_write_surfaces_an_error_and_keeps_the_original() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        ws.save_note(n.id, "original", SaveMode::Overwrite)
            .await
            .unwrap();
        // Make the note's directory entry unwritable by replacing the file with a directory
        // that cannot be renamed over: simulate failure by removing the parent-less path.
        fs::remove_file(dir.path().join("A.md")).unwrap();
        let err = ws
            .save_note(n.id, "new", SaveMode::Overwrite)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{err:?}");
        assert!(!dir.path().join("A.md").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_only_folder_makes_save_fail_loudly_without_data_loss() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, ws) = open().await;
        ws.create_folder("Locked").await.unwrap();
        let n = ws.create_note(Some("Locked"), Some("A")).await.unwrap();
        let original = ws
            .save_note(n.id, "original", SaveMode::Overwrite)
            .await
            .unwrap()
            .content;

        let locked = dir.path().join("Locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let denied = fs::File::create(locked.join("probe")).is_err(); // false when running as root
        let result = ws.save_note(n.id, "new", SaveMode::Overwrite).await;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        if denied {
            assert!(matches!(result, Err(Error::Io { .. })));
            assert_eq!(disk(&dir, "Locked/A.md"), original);
            let stray: Vec<_> = fs::read_dir(&locked)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(stray, vec![std::ffi::OsString::from("A.md")]);
        }
    }

    #[tokio::test]
    async fn rename_and_move_keep_marked_ids_and_never_overwrite() {
        let (dir, ws) = open().await;
        ws.create_folder("A").await.unwrap();
        let n = ws.create_note(None, Some("One")).await.unwrap();
        let other = ws.create_note(None, Some("Two")).await.unwrap();

        let renamed = ws.rename_note(n.id, "Uno").await.unwrap();
        assert_eq!(
            (renamed.id, renamed.path.as_str(), renamed.title.as_str()),
            (n.id, "Uno.md", "Uno")
        );
        assert!(matches!(
            ws.rename_note(n.id, "Two").await,
            Err(Error::AlreadyExists(_))
        ));

        let moved = ws.move_note(n.id, Some("A")).await.unwrap();
        assert_eq!((moved.id, moved.path.as_str()), (n.id, "A/Uno.md"));
        assert!(dir.path().join("A/Uno.md").exists() && !dir.path().join("Uno.md").exists());
        assert_eq!(ws.move_note(n.id, None).await.unwrap().path, "Uno.md");
        assert!(dir.path().join("Two.md").exists());
        assert_eq!(other.path, "Two.md");
    }

    #[tokio::test]
    async fn plain_notes_get_a_new_derived_id_when_renamed_and_stay_unmodified() {
        let (dir, ws) = open().await;
        fs::write(dir.path().join("plain.md"), "no frontmatter").unwrap();
        ws.reindex().await.unwrap();
        let before = ws.list_notes().await.unwrap().remove(0);
        let after = ws.rename_note(before.id, "renamed").await.unwrap();
        assert_ne!(before.id, after.id);
        assert_eq!(after.id, note_id_for_path("renamed.md"));
        assert_eq!(disk(&dir, "renamed.md"), "no frontmatter");
        assert_eq!(ws.list_notes().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn duplicate_gets_a_new_id_and_a_free_name() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("Idea")).await.unwrap();
        ws.save_note(
            n.id,
            &format!("---\nid: {}\n---\nbody", n.id),
            SaveMode::Overwrite,
        )
        .await
        .unwrap();
        let d1 = ws.duplicate_note(n.id).await.unwrap();
        let d2 = ws.duplicate_note(n.id).await.unwrap();
        assert_eq!(
            (d1.path.as_str(), d2.path.as_str()),
            ("Idea copy.md", "Idea copy 2.md")
        );
        assert!(d1.id != n.id && d2.id != d1.id);
        assert!(disk(&dir, "Idea copy.md").ends_with("body"));
        assert_eq!(parse_meta(&disk(&dir, "Idea copy.md")).id, Some(d1.id));
    }

    #[tokio::test]
    async fn pin_and_archive_live_in_frontmatter_and_survive_an_index_rebuild() {
        let (dir, ws) = open().await;
        fs::write(dir.path().join("p.md"), "---\ntitle: keep me\n---\nBody\n").unwrap();
        ws.reindex().await.unwrap();
        let n = ws.list_notes().await.unwrap().remove(0);

        let pinned = ws.set_pinned(n.id, true).await.unwrap();
        assert!(pinned.pinned);
        ws.set_archived(n.id, true).await.unwrap();
        let text = disk(&dir, "p.md");
        assert!(
            text.contains("title: keep me")
                && text.contains("pinned: true")
                && text.ends_with("Body\n")
        );

        // Delete the index entirely, then recover everything from the files.
        ws.database().close().await;
        wait_for_close(&dir);
        fs::remove_file(dir.path().join(".notenest/index.db")).unwrap();
        let (ws2, _) = Workspace::open(dir.path(), false).await.unwrap();
        let again = ws2.list_notes().await.unwrap().remove(0);
        assert!(again.pinned && again.archived);
        assert_eq!(again.id, n.id);

        let unpinned = ws2.set_pinned(n.id, false).await.unwrap();
        assert!(!unpinned.pinned && unpinned.archived);
    }

    #[tokio::test]
    async fn delete_and_restore_a_note() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("Gone")).await.unwrap();
        let saved = ws
            .save_note(n.id, "precious", SaveMode::Overwrite)
            .await
            .unwrap();

        let entry = ws.delete_note(n.id).await.unwrap();
        assert!(ws.list_notes().await.unwrap().is_empty() && !dir.path().join("Gone.md").exists());
        assert_eq!(ws.list_trash().unwrap().len(), 1);

        assert_eq!(ws.restore(entry.id).await.unwrap().as_str(), "Gone.md");
        assert_eq!(disk(&dir, "Gone.md"), saved.content);
        assert!(saved.content.ends_with("precious"));
        assert_eq!(ws.list_notes().await.unwrap().len(), 1);
        assert!(ws.list_trash().unwrap().is_empty());
    }

    #[tokio::test]
    async fn folder_rename_move_delete_and_restore_keep_the_index_in_sync() {
        let (dir, ws) = open().await;
        ws.create_folder("Work/Deep").await.unwrap();
        ws.create_note(Some("Work/Deep"), Some("N")).await.unwrap();
        assert_eq!(ws.list_folders().await.unwrap(), vec!["Work", "Work/Deep"]);

        ws.rename_folder("Work", "Job").await.unwrap();
        assert_eq!(ws.list_folders().await.unwrap(), vec!["Job", "Job/Deep"]);
        assert_eq!(ws.list_notes().await.unwrap()[0].path, "Job/Deep/N.md");

        ws.create_folder("Archive").await.unwrap();
        ws.move_folder("Job", Some("Archive")).await.unwrap();
        assert!(dir.path().join("Archive/Job/Deep/N.md").exists());
        assert!(matches!(
            ws.move_folder("Archive", Some("Archive/Job")).await,
            Err(Error::InvalidPath(_))
        ));

        let entry = ws.delete_folder("Archive/Job").await.unwrap();
        assert!(ws.list_notes().await.unwrap().is_empty());
        ws.restore(entry.id).await.unwrap();
        assert_eq!(
            ws.list_notes().await.unwrap()[0].path,
            "Archive/Job/Deep/N.md"
        );
        assert!(matches!(
            ws.create_folder("Archive").await,
            Err(Error::AlreadyExists(_))
        ));
    }

    #[tokio::test]
    async fn preview_inlines_images_relative_to_the_note() {
        let (dir, ws) = open().await;
        fs::create_dir(dir.path().join("Attachments")).unwrap();
        ws.create_folder("Notes").await.unwrap();
        fs::write(
            dir.path().join("Attachments/p.png"),
            b"\x89PNG\r\n\x1a\nxxxx",
        )
        .unwrap();
        let n = ws.create_note(Some("Notes"), Some("N")).await.unwrap();

        let r = ws
            .render_markdown(Some(n.id), "# T\n\n![p](../Attachments/p.png)".into())
            .await
            .unwrap();
        assert!(r.html.contains("data:image/png;base64,") && r.warnings.is_empty());
        // Without a note there is nothing to resolve paths against.
        let r = ws
            .render_markdown(None, "![p](Attachments/p.png)".into())
            .await
            .unwrap();
        assert!(!r.html.contains("data:image"));
        assert!(matches!(
            ws.render_markdown(Some(Uuid::new_v4()), "x".into()).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn unknown_ids_are_not_found() {
        let (_d, ws) = open().await;
        let ghost = Uuid::new_v4();
        assert!(matches!(ws.read_note(ghost).await, Err(Error::NotFound(_))));
        assert!(matches!(
            ws.save_note(ghost, "x", SaveMode::Overwrite).await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            ws.delete_note(ghost).await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            ws.mark_opened(ghost).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn invalid_utf8_notes_cannot_be_opened_or_modified() {
        let (dir, ws) = open().await;
        fs::write(dir.path().join("bin.md"), [0xff, 0xfe, 0x00]).unwrap();
        ws.reindex().await.unwrap();
        let n = ws.list_notes().await.unwrap().remove(0);
        assert!(matches!(
            ws.read_note(n.id).await,
            Err(Error::Corrupt { .. })
        ));
        assert!(matches!(
            ws.set_pinned(n.id, true).await,
            Err(Error::Corrupt { .. })
        ));
        assert_eq!(
            fs::read(dir.path().join("bin.md")).unwrap(),
            vec![0xff, 0xfe, 0x00]
        );
    }

    #[tokio::test]
    async fn corrupt_index_is_rebuilt_from_the_vault_on_open() {
        let dir = tempfile::tempdir().unwrap();
        {
            let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
            ws.create_note(None, Some("Safe")).await.unwrap();
            ws.database().close().await;
        }
        wait_for_close(&dir);
        // Model a damaged main file with no WAL to recover from (a WAL left over from
        // a clean shutdown can legitimately let SQLite reconstruct the pages).
        for ext in ["db-wal", "db-shm"] {
            let _ = fs::remove_file(dir.path().join(".notenest").join(format!("index.{ext}")));
        }
        fs::write(
            dir.path().join(".notenest/index.db"),
            b"garbage garbage garbage garbage",
        )
        .unwrap();
        let (ws, report) = Workspace::open(dir.path(), false).await.unwrap();
        assert!(report.index_rebuilt && report.index_backup.is_some());
        assert_eq!(ws.list_notes().await.unwrap()[0].title, "Safe");
    }

    #[tokio::test]
    async fn concurrent_saves_are_serialized_and_never_interleave() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        let mut handles = Vec::new();
        for i in 0..20 {
            let ws = ws.clone();
            handles.push(tokio::spawn(async move {
                let text = format!("version {i}");
                ws.save_note(n.id, &text, SaveMode::Overwrite)
                    .await
                    .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let text = disk(&dir, "A.md");
        assert!(text.contains("version "));
        assert_eq!(parse_meta(&text).id, Some(n.id));
        let rec = ws.list_notes().await.unwrap().remove(0);
        assert_eq!(rec.content_hash, content_hash(text.as_bytes()));
    }
}
