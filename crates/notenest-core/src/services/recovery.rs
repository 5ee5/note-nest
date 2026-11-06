//! Crash recovery for unsaved edits.
//!
//! While a note is being edited the UI stashes its text here more often than it
//! autosaves. Drafts live in `<vault>/.notenest/recovery/drafts/<note id>.json`.
//! After an unexpected exit they are offered back to the user; they are removed
//! only once the same text has been saved, or when the user discards them.

use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::workspace::preserve_id;
use super::Workspace;
use crate::filesystem::atomic::write_atomic;
use crate::filesystem::RelPath;
use crate::hash::content_hash;
use crate::{Error, Result};

/// Unsaved text for a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub note_id: Uuid,
    /// Path the note had when the draft was stashed.
    pub path: String,
    /// Content hash of the file when editing started (what the draft is based on).
    pub base_hash: String,
    pub content: String,
    pub stashed_at: DateTime<Utc>,
    /// The note no longer exists at `path` (deleted or moved since).
    #[serde(default)]
    pub note_missing: bool,
    /// The file changed on disk since the draft's base: restoring needs a decision.
    #[serde(default)]
    pub disk_changed: bool,
}

impl Workspace {
    fn drafts_dir(&self) -> PathBuf {
        self.vault.meta_dir().join("recovery").join("drafts")
    }

    fn draft_file(&self, id: Uuid) -> PathBuf {
        self.drafts_dir().join(format!("{id}.json"))
    }

    /// Stores the current unsaved text of a note. Cheap enough to call on every
    /// short debounce; replaces any previous draft for the note.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown note; [`Error::Io`] if the draft cannot be written.
    pub async fn stash_draft(&self, id: Uuid, content: &str, base_hash: &str) -> Result<()> {
        let record = self
            .db
            .get_note(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("note {id}")))?;
        let draft = Draft {
            note_id: id,
            path: record.path,
            base_hash: base_hash.to_owned(),
            content: content.to_owned(),
            stashed_at: Utc::now(),
            note_missing: false,
            disk_changed: false,
        };
        let json = serde_json::to_vec(&draft).map_err(|e| Error::Corrupt {
            context: "draft".into(),
            message: e.to_string(),
        })?;
        let dir = self.drafts_dir();
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        write_atomic(&self.draft_file(id), &json)
    }

    /// Lists drafts that hold text not present in the note on disk. Drafts that
    /// match the file are removed as already saved. Unreadable drafts are skipped
    /// and left in place.
    ///
    /// # Errors
    /// [`Error::Io`] if the drafts directory cannot be read.
    pub async fn list_drafts(&self) -> Result<Vec<Draft>> {
        let dir = self.drafts_dir();
        let read = match fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(&dir, e)),
        };
        let mut drafts = Vec::new();
        for entry in read {
            let path = entry.map_err(|e| Error::io(&dir, e))?.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let mut draft: Draft = match fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
            {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(file = %path.display(), error = %e, "unreadable draft left in place");
                    continue;
                }
            };
            // Prefer the note's current location; fall back to the recorded path.
            let current = self.db.get_note(draft.note_id).await?;
            let rel = RelPath::new(current.as_ref().map_or(&draft.path, |r| &r.path));
            let on_disk = rel.ok().and_then(|r| self.vault.read(&r).ok());
            match on_disk {
                Some(bytes) if bytes == preserve_id(&bytes, &draft.content).as_bytes() => {
                    let _ = fs::remove_file(&path); // already saved
                    continue;
                }
                Some(bytes) => draft.disk_changed = content_hash(&bytes) != draft.base_hash,
                None => draft.note_missing = true,
            }
            if let Some(r) = current {
                draft.path = r.path;
            }
            drafts.push(draft);
        }
        drafts.sort_by_key(|d| std::cmp::Reverse(d.stashed_at));
        Ok(drafts)
    }

    /// Removes a draft the user chose not to restore.
    ///
    /// # Errors
    /// [`Error::Io`] if the file cannot be removed.
    pub fn discard_draft(&self, id: Uuid) -> Result<()> {
        match fs::remove_file(self.draft_file(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(self.draft_file(id), e)),
        }
    }

    /// After a successful save: drop the draft if it holds exactly what was saved.
    /// A newer draft (more typing since) is kept.
    pub(super) fn clear_draft_if_saved(&self, id: Uuid, saved: &str) {
        let file = self.draft_file(id);
        let Ok(bytes) = fs::read(&file) else { return };
        let Ok(draft) = serde_json::from_slice::<Draft>(&bytes) else {
            return;
        };
        if draft.content == saved {
            if let Err(e) = fs::remove_file(&file) {
                tracing::warn!(file = %file.display(), error = %e, "could not remove saved draft");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::SaveMode;

    async fn open(dir: &tempfile::TempDir) -> Workspace {
        Workspace::open(dir.path(), false).await.unwrap().0
    }

    #[tokio::test]
    async fn unsaved_text_is_recoverable_after_an_unexpected_exit() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let ws = open(&dir).await;
            let n = ws.create_note(None, Some("A")).await.unwrap();
            let loaded = ws.read_note(n.id).await.unwrap();
            ws.stash_draft(n.id, "typed but never saved", &loaded.record.content_hash)
                .await
                .unwrap();
            n.id
            // Dropped without saving: simulates a crash.
        };
        let ws = open(&dir).await;
        let drafts = ws.list_drafts().await.unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].note_id, id);
        assert_eq!(drafts[0].content, "typed but never saved");
        assert!(!drafts[0].disk_changed && !drafts[0].note_missing);

        // Restoring = saving the draft text; the draft then disappears.
        let hash = ws.read_note(id).await.unwrap().record.content_hash;
        ws.save_note(id, &drafts[0].content, SaveMode::Expect(&hash))
            .await
            .unwrap();
        assert!(ws.list_drafts().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn saving_the_same_text_clears_the_draft_but_newer_drafts_survive() {
        let dir = tempfile::tempdir().unwrap();
        let ws = open(&dir).await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        let base = ws.read_note(n.id).await.unwrap().record.content_hash;

        ws.stash_draft(n.id, "v1", &base).await.unwrap();
        ws.stash_draft(n.id, "v2", &base).await.unwrap(); // typed more while v1 saves
        ws.save_note(n.id, "v1", SaveMode::Overwrite).await.unwrap();
        let left = ws.list_drafts().await.unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].content, "v2");
    }

    #[tokio::test]
    async fn flags_drafts_whose_file_changed_or_vanished() {
        let dir = tempfile::tempdir().unwrap();
        let ws = open(&dir).await;
        let a = ws.create_note(None, Some("A")).await.unwrap();
        let b = ws.create_note(None, Some("B")).await.unwrap();
        let base_a = ws.read_note(a.id).await.unwrap().record.content_hash;
        let base_b = ws.read_note(b.id).await.unwrap().record.content_hash;
        ws.stash_draft(a.id, "draft a", &base_a).await.unwrap();
        ws.stash_draft(b.id, "draft b", &base_b).await.unwrap();

        std::fs::write(dir.path().join("A.md"), "edited elsewhere").unwrap();
        ws.delete_note(b.id).await.unwrap();

        let drafts = ws.list_drafts().await.unwrap();
        let da = drafts.iter().find(|d| d.note_id == a.id).unwrap();
        let db = drafts.iter().find(|d| d.note_id == b.id).unwrap();
        assert!(da.disk_changed && !da.note_missing);
        assert!(db.note_missing);
        // The user's text is still there to be recovered.
        assert_eq!(db.content, "draft b");
    }

    #[tokio::test]
    async fn drafts_follow_a_renamed_note_and_can_be_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let ws = open(&dir).await;
        let n = ws.create_note(None, Some("Old")).await.unwrap();
        let base = ws.read_note(n.id).await.unwrap().record.content_hash;
        ws.stash_draft(n.id, "text", &base).await.unwrap();
        ws.rename_note(n.id, "New").await.unwrap();

        let drafts = ws.list_drafts().await.unwrap();
        assert_eq!(drafts[0].path, "New.md");
        assert!(!drafts[0].note_missing);

        ws.discard_draft(n.id).unwrap();
        ws.discard_draft(n.id).unwrap(); // idempotent
        assert!(ws.list_drafts().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn unreadable_drafts_are_skipped_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let ws = open(&dir).await;
        let n = ws.create_note(None, Some("A")).await.unwrap();
        let base = ws.read_note(n.id).await.unwrap().record.content_hash;
        ws.stash_draft(n.id, "good", &base).await.unwrap();
        let bad = ws.drafts_dir().join(format!("{}.json", Uuid::new_v4()));
        std::fs::write(&bad, b"{ not json").unwrap();

        assert_eq!(ws.list_drafts().await.unwrap().len(), 1);
        assert!(bad.exists());
    }

    #[tokio::test]
    async fn stashing_for_an_unknown_note_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ws = open(&dir).await;
        assert!(matches!(
            ws.stash_draft(Uuid::new_v4(), "x", "h").await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn stale_temp_files_from_a_crashed_write_are_cleaned_up_on_open() {
        let dir = tempfile::tempdir().unwrap();
        drop(open(&dir).await);
        let stale = dir.path().join(".nn-deadbeef.tmp");
        let fresh = dir.path().join(".nn-inprogress.tmp");
        std::fs::write(&stale, b"half written").unwrap();
        std::fs::write(&fresh, b"being written").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let (_ws, report) = Workspace::open(dir.path(), false).await.unwrap();
        assert_eq!(report.reindex.stale_temp_files_removed, 1);
        assert!(!stale.exists());
        assert!(
            fresh.exists(),
            "a recent temp file may belong to a write in progress"
        );
    }
}
