//! Crash-safe file replacement.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use uuid::Uuid;

use crate::{Error, Result};

/// Replaces the contents of `path` atomically: the new bytes are written to a
/// temporary file in the same directory, flushed to disk, then renamed over the
/// target. If anything fails the original file is left untouched and the
/// temporary file is removed.
///
/// # Errors
/// [`Error::Io`] if the file cannot be written; the target is unchanged then.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_inner(path, bytes, None)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Inject {
    /// Fail after the temp file is fully written, before the rename.
    BeforeRename,
}

fn write_atomic_inner(path: &Path, bytes: &[u8], inject: Option<Inject>) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::InvalidPath(format!("{} has no parent directory", path.display())))?;
    let tmp = dir.join(format!(".nn-{}.tmp", Uuid::new_v4().simple()));

    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Ok(meta) = fs::metadata(path) {
            // Keep the existing file's permissions across the replacement.
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        if inject == Some(Inject::BeforeRename) {
            return Err(io::Error::other("injected failure before rename"));
        }
        fs::rename(&tmp, path)
    })();

    match result {
        Ok(()) => {
            sync_dir(dir);
            Ok(())
        }
        Err(e) => {
            if let Err(cleanup) = fs::remove_file(&tmp) {
                tracing::warn!(tmp = %tmp.display(), error = %cleanup, "could not remove temp file");
            }
            Err(Error::io(path, e))
        }
    }
}

/// Flushes directory metadata so the rename survives a crash. Best effort: the
/// data itself is already durable, so a failure here is logged, not fatal.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) {
    if let Err(e) = fs::File::open(dir).and_then(|d| d.sync_all()) {
        tracing::warn!(dir = %dir.display(), error = %e, "could not sync directory");
    }
}

#[cfg(not(unix))]
pub(crate) fn sync_dir(_dir: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn creates_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        write_atomic(&p, b"one").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert!(tmp_files(dir.path()).is_empty());
    }

    #[test]
    fn failure_before_rename_keeps_original_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        fs::write(&p, b"original").unwrap();
        let err = write_atomic_inner(&p, b"new", Some(Inject::BeforeRename)).unwrap_err();
        assert!(matches!(err, Error::Io { .. }));
        assert_eq!(fs::read(&p).unwrap(), b"original");
        assert!(tmp_files(dir.path()).is_empty());
    }

    #[test]
    fn missing_parent_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope/a.md");
        assert!(matches!(write_atomic(&p, b"x"), Err(Error::Io { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        fs::write(&p, b"x").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&p, b"y").unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
