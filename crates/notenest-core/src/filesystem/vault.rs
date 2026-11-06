//! The vault: a user-chosen directory of Markdown notes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::atomic::{sync_dir, write_atomic};
use super::{RelPath, META_DIR};
use crate::{Error, Result};

/// A vault root plus safe operations inside it. Symlinks inside the vault are
/// never followed, so no operation can reach outside the root.
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// Opens an existing directory as a vault, creating its `.notenest` data directory.
    ///
    /// # Errors
    /// [`Error::NotFound`] if `root` is not a directory; [`Error::Io`] otherwise.
    pub fn open(root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => Error::NotFound(format!("vault {}", root.display())),
            _ => Error::io(root, e),
        })?;
        if !root.is_dir() {
            return Err(Error::InvalidPath(format!(
                "{} is not a directory",
                root.display()
            )));
        }
        let vault = Self { root };
        let meta = vault.meta_dir();
        fs::create_dir_all(&meta).map_err(|e| Error::io(&meta, e))?;
        Ok(vault)
    }

    /// Creates the directory (and parents) if missing, then opens it.
    ///
    /// # Errors
    /// [`Error::Io`] if the directory cannot be created.
    pub fn create(root: &Path) -> Result<Self> {
        fs::create_dir_all(root).map_err(|e| Error::io(root, e))?;
        Self::open(root)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/.notenest`: index database, trash and recovery drafts.
    #[must_use]
    pub fn meta_dir(&self) -> PathBuf {
        self.root.join(META_DIR)
    }

    /// Maps a validated relative path to an absolute one, refusing any path that
    /// passes through a symlink.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] on symlinks; [`Error::Io`] if a component cannot be inspected.
    pub fn resolve(&self, rel: &RelPath) -> Result<PathBuf> {
        let mut cur = self.root.clone();
        let mut exists = true;
        for comp in rel.components() {
            cur.push(comp);
            if !exists {
                continue;
            }
            match fs::symlink_metadata(&cur) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(Error::InvalidPath(format!(
                        "{rel}: symbolic links are not followed"
                    )));
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => exists = false,
                Err(e) => return Err(Error::io(&cur, e)),
            }
        }
        Ok(cur)
    }

    #[must_use]
    pub fn exists(&self, rel: &RelPath) -> bool {
        self.resolve(rel)
            .is_ok_and(|p| fs::symlink_metadata(p).is_ok())
    }

    /// Reads a file.
    ///
    /// # Errors
    /// [`Error::NotFound`] if missing; other failures as [`Error::Io`].
    pub fn read(&self, rel: &RelPath) -> Result<Vec<u8>> {
        let path = self.resolve(rel)?;
        fs::read(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => Error::NotFound(rel.to_string()),
            _ => Error::io(path, e),
        })
    }

    /// Size in bytes of a regular file.
    ///
    /// # Errors
    /// [`Error::NotFound`] if missing or not a regular file; [`Error::InvalidPath`] on symlinks.
    pub fn file_size(&self, rel: &RelPath) -> Result<u64> {
        let path = self.resolve(rel)?;
        match fs::metadata(&path) {
            Ok(m) if m.is_file() => Ok(m.len()),
            Ok(_) => Err(Error::NotFound(rel.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::NotFound(rel.to_string())),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Atomically replaces (or creates) a file whose parent folder exists.
    ///
    /// # Errors
    /// See [`write_atomic`].
    pub fn write(&self, rel: &RelPath, bytes: &[u8]) -> Result<()> {
        write_atomic(&self.resolve(rel)?, bytes)
    }

    /// Atomically creates a new file, failing if it already exists.
    ///
    /// # Errors
    /// [`Error::AlreadyExists`] if the path is taken.
    pub fn create_new(&self, rel: &RelPath, bytes: &[u8]) -> Result<()> {
        let path = self.resolve(rel)?;
        if fs::symlink_metadata(&path).is_ok() {
            return Err(Error::AlreadyExists(rel.to_string()));
        }
        write_atomic(&path, bytes)
    }

    /// Creates a folder (and missing parents).
    ///
    /// # Errors
    /// [`Error::AlreadyExists`] if the folder itself exists.
    pub fn create_dir(&self, rel: &RelPath) -> Result<()> {
        let path = self.resolve(rel)?;
        if fs::symlink_metadata(&path).is_ok() {
            return Err(Error::AlreadyExists(rel.to_string()));
        }
        fs::create_dir_all(&path).map_err(|e| Error::io(path, e))
    }

    /// Renames or moves a file or folder without ever overwriting.
    ///
    /// # Errors
    /// [`Error::NotFound`] if `from` is missing; [`Error::AlreadyExists`] if `to`
    /// is taken by a different entry; [`Error::InvalidPath`] when moving a folder
    /// into itself.
    pub fn rename(&self, from: &RelPath, to: &RelPath) -> Result<()> {
        if from.contains(to) && from != to {
            return Err(Error::InvalidPath(format!(
                "cannot move {from} into itself"
            )));
        }
        let src = self.resolve(from)?;
        let dst = self.resolve(to)?;
        if fs::symlink_metadata(&src).is_err() {
            return Err(Error::NotFound(from.to_string()));
        }
        if fs::symlink_metadata(&dst).is_ok() {
            // Allow case-only renames on case-insensitive filesystems, where the
            // target "exists" because it is the source itself.
            let same = matches!((fs::canonicalize(&src), fs::canonicalize(&dst)), (Ok(a), Ok(b)) if a == b);
            if !same {
                return Err(Error::AlreadyExists(to.to_string()));
            }
        }
        fs::rename(&src, &dst).map_err(|e| Error::io(&src, e))?;
        if let Some(dir) = dst.parent() {
            sync_dir(dir);
        }
        Ok(())
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
    fn open_creates_meta_dir_and_rejects_missing_root() {
        let (_d, v) = vault();
        assert!(v.meta_dir().is_dir());
        assert!(matches!(
            Vault::open(Path::new("/definitely/not/here")),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn create_new_never_overwrites() {
        let (_d, v) = vault();
        v.create_new(&rel("a.md"), b"one").unwrap();
        assert!(matches!(
            v.create_new(&rel("a.md"), b"two"),
            Err(Error::AlreadyExists(_))
        ));
        assert_eq!(v.read(&rel("a.md")).unwrap(), b"one");
    }

    #[test]
    fn read_missing_is_not_found() {
        let (_d, v) = vault();
        assert!(matches!(v.read(&rel("nope.md")), Err(Error::NotFound(_))));
    }

    #[test]
    fn rename_moves_files_and_folders_without_overwriting() {
        let (_d, v) = vault();
        v.create_dir(&rel("A")).unwrap();
        v.create_new(&rel("A/x.md"), b"x").unwrap();
        v.create_new(&rel("y.md"), b"y").unwrap();

        v.rename(&rel("A/x.md"), &rel("z.md")).unwrap();
        assert_eq!(v.read(&rel("z.md")).unwrap(), b"x");
        assert!(matches!(
            v.rename(&rel("z.md"), &rel("y.md")),
            Err(Error::AlreadyExists(_))
        ));
        assert_eq!(v.read(&rel("y.md")).unwrap(), b"y");

        v.rename(&rel("A"), &rel("B")).unwrap();
        assert!(v.exists(&rel("B")) && !v.exists(&rel("A")));
        assert!(matches!(
            v.rename(&rel("B"), &rel("B/C")),
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            v.rename(&rel("ghost"), &rel("g2")),
            Err(Error::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed() {
        let (d, v) = vault();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), d.path().join("link")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.md"), d.path().join("s.md"))
            .unwrap();

        assert!(matches!(
            v.read(&rel("link/secret.md")),
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(v.read(&rel("s.md")), Err(Error::InvalidPath(_))));
        assert!(matches!(
            v.write(&rel("link/new.md"), b"x"),
            Err(Error::InvalidPath(_))
        ));
        assert!(!outside.path().join("new.md").exists());
    }
}
