//! Soft deletion. Deleted files and folders are moved into
//! `<vault>/.notenest/trash/<id>/` so they can be restored; nothing is ever
//! hard-deleted by these operations.

use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::atomic::write_atomic;
use super::{RelPath, Vault};
use crate::{Error, Result};

const ITEM: &str = "item";
const META: &str = "meta.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashEntry {
    pub id: Uuid,
    pub original_path: String,
    pub trashed_at: DateTime<Utc>,
    pub is_dir: bool,
}

impl Vault {
    fn trash_dir(&self) -> PathBuf {
        self.meta_dir().join("trash")
    }

    /// Moves a file or folder to the trash.
    ///
    /// # Errors
    /// [`Error::NotFound`] if the path does not exist.
    pub fn trash(&self, rel: &RelPath) -> Result<TrashEntry> {
        let src = self.resolve(rel)?;
        let meta = fs::symlink_metadata(&src).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => Error::NotFound(rel.to_string()),
            _ => Error::io(&src, e),
        })?;
        let entry = TrashEntry {
            id: Uuid::new_v4(),
            original_path: rel.to_string(),
            trashed_at: Utc::now(),
            is_dir: meta.is_dir(),
        };
        let dir = self.trash_dir().join(entry.id.to_string());
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;

        let json = serde_json::to_vec_pretty(&entry).map_err(|e| Error::Corrupt {
            context: "trash metadata".into(),
            message: e.to_string(),
        })?;
        let moved = write_atomic(&dir.join(META), &json)
            .and_then(|()| fs::rename(&src, dir.join(ITEM)).map_err(|e| Error::io(&src, e)));
        if let Err(e) = moved {
            // Nothing was moved yet (or the move failed): drop the empty bookkeeping.
            let _ = fs::remove_file(dir.join(META));
            let _ = fs::remove_dir(&dir);
            return Err(e);
        }
        Ok(entry)
    }

    /// Lists trashed items, newest first. Incomplete entries (no stored item) are skipped.
    ///
    /// # Errors
    /// [`Error::Io`] if the trash directory cannot be read.
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>> {
        let base = self.trash_dir();
        let read = match fs::read_dir(&base) {
            Ok(r) => r,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(&base, e)),
        };
        let mut entries = Vec::new();
        for item in read {
            let dir = item.map_err(|e| Error::io(&base, e))?.path();
            if fs::symlink_metadata(dir.join(ITEM)).is_err() {
                continue;
            }
            let bytes = match fs::read(dir.join(META)) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(dir = %dir.display(), error = %e, "unreadable trash metadata");
                    continue;
                }
            };
            match serde_json::from_slice::<TrashEntry>(&bytes) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    tracing::warn!(dir = %dir.display(), error = %e, "corrupt trash metadata");
                }
            }
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.trashed_at));
        Ok(entries)
    }

    /// Restores a trashed item to its original location, recreating missing parent folders.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown id; [`Error::AlreadyExists`] if the original
    /// location is occupied (the trashed item is left in place).
    pub fn restore(&self, id: Uuid) -> Result<RelPath> {
        let dir = self.trash_dir().join(id.to_string());
        let bytes = fs::read(dir.join(META)).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => Error::NotFound(format!("trash entry {id}")),
            _ => Error::io(dir.join(META), e),
        })?;
        let entry: TrashEntry = serde_json::from_slice(&bytes).map_err(|e| Error::Corrupt {
            context: format!("trash entry {id}"),
            message: e.to_string(),
        })?;
        let target = RelPath::new(&entry.original_path)?;
        let dst = self.resolve(&target)?;
        if fs::symlink_metadata(&dst).is_ok() {
            return Err(Error::AlreadyExists(target.to_string()));
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let item = dir.join(ITEM);
        fs::rename(&item, &dst).map_err(|e| Error::io(&item, e))?;
        if let Err(e) = fs::remove_file(dir.join(META)).and_then(|()| fs::remove_dir(&dir)) {
            tracing::warn!(dir = %dir.display(), error = %e, "could not clean up trash entry");
        }
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(s: &str) -> RelPath {
        RelPath::new(s).unwrap()
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        (dir, v)
    }

    #[test]
    fn trash_and_restore_a_file() {
        let (_d, v) = vault();
        v.create_dir(&rel("A")).unwrap();
        v.create_new(&rel("A/x.md"), b"hello").unwrap();

        let entry = v.trash(&rel("A/x.md")).unwrap();
        assert!(!v.exists(&rel("A/x.md")));
        assert_eq!(v.list_trash().unwrap(), vec![entry.clone()]);

        assert_eq!(v.restore(entry.id).unwrap(), rel("A/x.md"));
        assert_eq!(v.read(&rel("A/x.md")).unwrap(), b"hello");
        assert!(v.list_trash().unwrap().is_empty());
    }

    #[test]
    fn trash_and_restore_a_folder_recreating_parents() {
        let (_d, v) = vault();
        v.create_dir(&rel("A/B")).unwrap();
        v.create_new(&rel("A/B/x.md"), b"x").unwrap();
        let entry = v.trash(&rel("A")).unwrap();
        assert!(entry.is_dir);
        v.restore(entry.id).unwrap();
        assert_eq!(v.read(&rel("A/B/x.md")).unwrap(), b"x");
    }

    #[test]
    fn restore_refuses_to_overwrite_and_keeps_the_trashed_copy() {
        let (_d, v) = vault();
        v.create_new(&rel("x.md"), b"old").unwrap();
        let entry = v.trash(&rel("x.md")).unwrap();
        v.create_new(&rel("x.md"), b"new").unwrap();

        assert!(matches!(v.restore(entry.id), Err(Error::AlreadyExists(_))));
        assert_eq!(v.read(&rel("x.md")).unwrap(), b"new");
        assert_eq!(v.list_trash().unwrap().len(), 1);
    }

    #[test]
    fn same_name_can_be_trashed_twice() {
        let (_d, v) = vault();
        v.create_new(&rel("x.md"), b"1").unwrap();
        let first = v.trash(&rel("x.md")).unwrap();
        v.create_new(&rel("x.md"), b"2").unwrap();
        let second = v.trash(&rel("x.md")).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(v.list_trash().unwrap().len(), 2);
    }

    #[test]
    fn missing_items_and_unknown_ids_are_errors() {
        let (_d, v) = vault();
        assert!(matches!(v.trash(&rel("ghost.md")), Err(Error::NotFound(_))));
        assert!(matches!(v.restore(Uuid::new_v4()), Err(Error::NotFound(_))));
    }

    #[test]
    fn incomplete_trash_dirs_are_skipped() {
        let (_d, v) = vault();
        fs::create_dir_all(v.trash_dir().join("orphan")).unwrap();
        assert!(v.list_trash().unwrap().is_empty());
    }
}
