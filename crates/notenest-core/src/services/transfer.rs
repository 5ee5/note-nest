//! Importing Markdown from elsewhere on disk, and exporting notes or the whole vault.
//!
//! Safety rules:
//! - Importing never overwrites anything in the vault: names that are taken get a
//!   " 2", " 3", ... suffix. An imported note whose frontmatter `id` is already used
//!   gets a fresh one, so the original keeps its identity.
//! - Exporting a vault always creates a new folder and never overwrites files.
//!   Exporting one note writes the file the user chose in a save dialog (which asks
//!   before replacing); its attachments are never written over existing files.
//! - Neither may happen inside the vault (that would import or export into itself).
//! - Hidden files and folders and symbolic links are never copied, and references
//!   through hidden path components (`../.ssh/key`) are not followed.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;
use walkdir::WalkDir;

use super::Workspace;
use crate::filesystem::atomic::write_atomic;
use crate::filesystem::{RelPath, ATTACHMENTS_DIR};
use crate::frontmatter::{parse_meta, set_meta, MetaUpdate};
use crate::portable::{
    encode_path, folder_of, local_refs, normalize, relative, standardize, WikiLinks,
};
use crate::{Error, Result};

const MAX_NAME_ATTEMPTS: usize = 10_000;
/// Largest single file copied by an import (notes and attachments).
pub const MAX_IMPORT_FILE_BYTES: u64 = 200 * 1024 * 1024;

/// Something that was not imported or exported, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// Notes created, as vault paths.
    pub notes: Vec<String>,
    /// Other files copied into the vault (attachments).
    pub files: usize,
    /// Imported notes that were given a new id because theirs was already in use.
    pub new_ids: usize,
    /// For a folder import: the folder created in the vault.
    pub folder: Option<String>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExportReport {
    /// The file or folder written.
    pub destination: String,
    pub notes: usize,
    /// Other files copied (attachments).
    pub files: usize,
    pub skipped: Vec<Skipped>,
}

fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

fn is_markdown(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// A relative reference that goes through a hidden file or folder (`.ssh/`, `.env`).
fn through_hidden(path: &str) -> bool {
    path.split('/')
        .any(|c| c != "." && c != ".." && is_hidden_name(c))
}

fn skip(path: impl Into<String>, reason: impl Into<String>) -> Skipped {
    Skipped {
        path: path.into(),
        reason: reason.into(),
    }
}

fn join(dir: Option<&RelPath>, name: &str) -> Result<RelPath> {
    match dir {
        Some(d) => d.join(name),
        None => RelPath::new(name),
    }
}

/// `name.ext` -> `name-2.ext`, `name-3.ext`, ...
fn numbered(name: &str, n: usize) -> String {
    if n == 1 {
        return name.to_owned();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{n}.{ext}"),
        _ => format!("{name}-{n}"),
    }
}

fn read_limited(path: &Path) -> std::result::Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > MAX_IMPORT_FILE_BYTES {
        return Err(format!(
            "larger than {} MB",
            MAX_IMPORT_FILE_BYTES / (1024 * 1024)
        ));
    }
    fs::read(path).map_err(|e| e.to_string())
}

/// Gives `text` a fresh frontmatter id if its id is already taken. Returns whether it did.
fn claim_id(text: &mut String, used: &mut HashSet<Uuid>) -> bool {
    let Some(id) = parse_meta(text).id else {
        return false; // the id derives from the (new, unique) path
    };
    if used.insert(id) {
        return false;
    }
    let fresh = Uuid::new_v4();
    *text = set_meta(
        text,
        &MetaUpdate {
            id: Some(fresh),
            ..Default::default()
        },
    );
    used.insert(fresh);
    true
}

/// A file outside the vault: a new name in `dir` for `name` that is not taken.
fn free_external(dir: &Path, name: &str) -> Option<PathBuf> {
    (1..=MAX_NAME_ATTEMPTS)
        .map(|n| dir.join(numbered(name, n)))
        .find(|p| fs::symlink_metadata(p).is_err())
}

impl Workspace {
    /// Fails if `path` is the vault or inside it (once symlinks are resolved).
    fn outside_vault(&self, path: &Path, what: &str) -> Result<PathBuf> {
        let canonical = fs::canonicalize(path).map_err(|e| Error::io(path, e))?;
        if canonical.starts_with(self.vault.root()) {
            return Err(Error::InvalidPath(format!(
                "{what} cannot be inside the vault: {}",
                path.display()
            )));
        }
        Ok(canonical)
    }

    fn ensure_dir(&self, rel: &RelPath) -> Result<()> {
        if self.vault.resolve(rel)?.is_dir() {
            return Ok(());
        }
        self.vault.create_dir(rel)
    }

    /// A free vault path for a note named `stem` in `dir`: `stem.md`, `stem 2.md`, ...
    fn free_note_path(&self, dir: Option<&RelPath>, stem: &str) -> Result<RelPath> {
        for n in 1..=MAX_NAME_ATTEMPTS {
            let name = if n == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem} {n}.md")
            };
            let path = join(dir, &name)?;
            if !self.vault.exists(&path) {
                return Ok(path);
            }
        }
        Err(Error::AlreadyExists(format!("no free name for {stem:?}")))
    }

    fn free_vault_file(&self, dir: &RelPath, name: &str) -> Result<RelPath> {
        for n in 1..=MAX_NAME_ATTEMPTS {
            let path = dir.join(&numbered(name, n))?;
            if !self.vault.exists(&path) {
                return Ok(path);
            }
        }
        Err(Error::AlreadyExists(format!("no free name for {name:?}")))
    }

    /// Copies the local files `text` references (relative to `src_dir` on disk) into
    /// the vault, next to the note at the same relative path when that is free, else
    /// into `Attachments/` with the link rewritten.
    fn import_attachments(
        &self,
        text: &mut String,
        src_dir: &Path,
        note_dir: &str,
        report: &mut ImportReport,
    ) -> Result<()> {
        let mut refs = local_refs(text);
        refs.sort_by_key(|r| std::cmp::Reverse(r.span.as_ref().map_or(0, |s| s.start)));
        for r in refs {
            if is_markdown(&r.path) || through_hidden(&r.path) {
                continue;
            }
            let src = src_dir.join(&r.path);
            if fs::symlink_metadata(&src).map_or(true, |m| !m.is_file()) {
                continue; // a broken reference stays as it was
            }
            let bytes = match read_limited(&src) {
                Ok(b) => b,
                Err(reason) => {
                    report.skipped.push(skip(src.to_string_lossy(), reason));
                    continue;
                }
            };
            // Same relative place, if that is inside the vault and free (or identical).
            let same_place = normalize(note_dir, &r.path).and_then(|p| RelPath::new(&p).ok());
            if let Some(dest) = &same_place {
                if !self.vault.exists(dest) {
                    if let Some(parent) = dest.parent() {
                        self.ensure_dir(&parent)?;
                    }
                    self.vault.create_new(dest, &bytes)?;
                    report.files += 1;
                    continue;
                }
                if self.vault.read(dest).is_ok_and(|b| b == bytes) {
                    continue;
                }
            }
            let Some(span) = r.span else {
                report.skipped.push(skip(
                    src.to_string_lossy(),
                    "its link could not be updated to a new location",
                ));
                continue;
            };
            let attachments = RelPath::new(ATTACHMENTS_DIR)?;
            self.ensure_dir(&attachments)?;
            let file_name = src
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| RelPath::new(n).is_ok())
                .unwrap_or("attachment");
            let dest = self.free_vault_file(&attachments, file_name)?;
            self.vault.create_new(&dest, &bytes)?;
            report.files += 1;
            text.replace_range(span, &encode_path(&relative(note_dir, dest.as_str())));
        }
        Ok(())
    }

    /// Copies Markdown files into the vault folder `folder` (the root if `None`),
    /// together with the local files they reference.
    ///
    /// # Errors
    /// [`Error::NotFound`] if the folder does not exist; I/O errors writing the vault.
    /// Problems with individual source files are reported in [`ImportReport::skipped`].
    pub async fn import_files(
        &self,
        sources: &[PathBuf],
        folder: Option<&str>,
    ) -> Result<ImportReport> {
        let _g = self.write_lock.lock().await;
        let dir = self.folder_path(folder)?;
        let mut used: HashSet<Uuid> = self.db.list_notes().await?.iter().map(|n| n.id).collect();
        let mut report = ImportReport::default();
        for src in sources {
            let shown = src.to_string_lossy().into_owned();
            let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !is_markdown(name) || is_hidden_name(name) {
                report.skipped.push(skip(shown, "not a Markdown file"));
                continue;
            }
            if let Err(e) = self.outside_vault(src, "a file to import") {
                report.skipped.push(skip(shown, e.to_string()));
                continue;
            }
            let bytes = match read_limited(src) {
                Ok(b) => b,
                Err(reason) => {
                    report.skipped.push(skip(shown, reason));
                    continue;
                }
            };
            let stem = &name[..name.len() - 3];
            let path = match self.free_note_path(dir.as_ref(), stem) {
                Ok(p) => p,
                Err(e) => {
                    report.skipped.push(skip(shown, e.to_string()));
                    continue;
                }
            };
            let bytes = match String::from_utf8(bytes) {
                Ok(mut text) => {
                    if claim_id(&mut text, &mut used) {
                        report.new_ids += 1;
                    }
                    let src_dir = src.parent().unwrap_or(Path::new("."));
                    self.import_attachments(
                        &mut text,
                        src_dir,
                        folder_of(path.as_str()),
                        &mut report,
                    )?;
                    text.into_bytes()
                }
                // Not UTF-8: copied byte for byte (NoteNest will not open it for editing).
                Err(e) => e.into_bytes(),
            };
            self.vault.create_new(&path, &bytes)?;
            report.notes.push(path.to_string());
        }
        self.rescan().await?;
        Ok(report)
    }

    /// Copies a folder (notes, attachments and subfolders) into the vault folder
    /// `folder` (the root if `None`), as a new folder named like the source.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] if the source is the vault, inside it, or contains it;
    /// [`Error::NotFound`] if the target folder does not exist; I/O errors.
    pub async fn import_folder(&self, source: &Path, folder: Option<&str>) -> Result<ImportReport> {
        let _g = self.write_lock.lock().await;
        let dir = self.folder_path(folder)?;
        let source = self.outside_vault(source, "a folder to import")?;
        if self.vault.root().starts_with(&source) {
            return Err(Error::InvalidPath(format!(
                "cannot import a folder that contains the vault: {}",
                source.display()
            )));
        }
        if !source.is_dir() {
            return Err(Error::InvalidPath(format!(
                "not a folder: {}",
                source.display()
            )));
        }
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !is_hidden_name(n))
            .unwrap_or("Imported");
        let top = (1..=MAX_NAME_ATTEMPTS)
            .map(|n| {
                if n == 1 {
                    join(dir.as_ref(), name)
                } else {
                    join(dir.as_ref(), &format!("{name} {n}"))
                }
            })
            .find(|p| p.as_ref().map_or(true, |p| !self.vault.exists(p)))
            .unwrap_or_else(|| Err(Error::AlreadyExists(name.to_owned())))?;
        self.vault.create_dir(&top)?;

        let mut used: HashSet<Uuid> = self.db.list_notes().await?.iter().map(|n| n.id).collect();
        let mut report = ImportReport {
            folder: Some(top.to_string()),
            ..Default::default()
        };
        let walker = WalkDir::new(&source)
            .follow_links(false)
            .sort_by_file_name()
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| e.file_name().to_str().is_none_or(|n| !is_hidden_name(n)));
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let at = e.path().map(|p| p.to_string_lossy().into_owned());
                    report
                        .skipped
                        .push(skip(at.unwrap_or_default(), e.to_string()));
                    continue;
                }
            };
            let shown = entry.path().to_string_lossy().into_owned();
            if entry.file_type().is_symlink() {
                report
                    .skipped
                    .push(skip(shown, "symbolic links are not copied"));
                continue;
            }
            let Some(rel) = entry
                .path()
                .strip_prefix(&source)
                .ok()
                .and_then(|r| r.to_str())
                .map(|r| r.replace('\\', "/"))
            else {
                report
                    .skipped
                    .push(skip(shown, "the name is not valid Unicode"));
                continue;
            };
            let Ok(dest) = RelPath::new(&format!("{top}/{rel}")) else {
                report
                    .skipped
                    .push(skip(shown, "the name is not allowed in a vault"));
                continue;
            };
            if entry.file_type().is_dir() {
                self.ensure_dir(&dest)?;
                continue;
            }
            let bytes = match read_limited(entry.path()) {
                Ok(b) => b,
                Err(reason) => {
                    report.skipped.push(skip(shown, reason));
                    continue;
                }
            };
            if is_markdown(&rel) {
                let bytes = match String::from_utf8(bytes) {
                    Ok(mut text) => {
                        if claim_id(&mut text, &mut used) {
                            report.new_ids += 1;
                        }
                        text.into_bytes()
                    }
                    Err(e) => e.into_bytes(),
                };
                self.vault.create_new(&dest, &bytes)?;
                report.notes.push(dest.to_string());
            } else {
                self.vault.create_new(&dest, &bytes)?;
                report.files += 1;
            }
        }
        self.rescan().await?;
        Ok(report)
    }

    /// Writes one note to `dest` (a file outside the vault; `.md` is added if missing),
    /// with the files it references next to it. With `standard`, NoteNest's own
    /// frontmatter keys are removed and `[[wiki links]]` become plain text, since the
    /// notes they point at are not exported.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown note; [`Error::InvalidPath`] for a
    /// destination inside the vault; I/O errors.
    pub async fn export_note(&self, id: Uuid, dest: &Path, standard: bool) -> Result<ExportReport> {
        let record = self.record(id).await?;
        let mut dest = dest.to_owned();
        if !is_markdown(&dest.to_string_lossy()) {
            let mut name = dest.file_name().unwrap_or_default().to_owned();
            name.push(".md");
            dest.set_file_name(name);
        }
        let out_dir = dest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| Error::InvalidPath(format!("no folder in {}", dest.display())))?
            .to_owned();
        let out_dir = self.outside_vault(&out_dir, "the export destination")?;
        let dest = out_dir.join(dest.file_name().unwrap_or_default());

        let bytes = self.vault.read(&RelPath::new(&record.path)?)?;
        let mut report = ExportReport {
            destination: dest.to_string_lossy().into_owned(),
            notes: 1,
            ..Default::default()
        };
        let Ok(mut text) = String::from_utf8(bytes.clone()) else {
            write_atomic(&dest, &bytes)?;
            return Ok(report);
        };
        if standard {
            text = standardize(&text, &record.path, |_| None, WikiLinks::Text);
        }

        let note_dir = folder_of(&record.path).to_owned();
        let mut refs = local_refs(&text);
        refs.sort_by_key(|r| std::cmp::Reverse(r.span.as_ref().map_or(0, |s| s.start)));
        for r in refs {
            if is_markdown(&r.path) || through_hidden(&r.path) {
                continue;
            }
            let Some(src) = normalize(&note_dir, &r.path).and_then(|p| RelPath::new(&p).ok())
            else {
                continue;
            };
            let Ok(file) = self.vault.read(&src) else {
                continue; // broken reference
            };
            // Keep the relative path if it stays inside the export folder.
            let kept = normalize("", &r.path).map(|p| out_dir.join(p));
            let target = match kept {
                Some(p) if fs::read(&p).map_or(true, |b| b == file) => Some(p),
                _ => None,
            };
            let target = match target {
                Some(p) => p,
                None => {
                    let Some(span) = r.span.clone() else {
                        report.skipped.push(skip(
                            src.as_str(),
                            "its link could not be updated to a new location",
                        ));
                        continue;
                    };
                    let folder = out_dir.join(ATTACHMENTS_DIR);
                    let Some(p) = free_external(&folder, src.file_name()) else {
                        report.skipped.push(skip(src.as_str(), "no free file name"));
                        continue;
                    };
                    let rel = p
                        .strip_prefix(&out_dir)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    text.replace_range(span, &encode_path(&rel));
                    p
                }
            };
            if fs::symlink_metadata(&target).is_err() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
                }
                write_atomic(&target, &file)?;
            }
            report.files += 1;
        }
        write_atomic(&dest, text.as_bytes())?;
        Ok(report)
    }

    /// Copies every note, attachment and folder into a new folder inside `parent`
    /// (named after the vault). Hidden files and folders (including NoteNest's own
    /// `.notenest/`) are left out. With `standard`, notes are converted to standard
    /// Markdown: NoteNest's frontmatter keys are removed and `[[wiki links]]` become
    /// relative links.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] if `parent` is inside the vault; I/O errors.
    pub async fn export_vault(&self, parent: &Path, standard: bool) -> Result<ExportReport> {
        self.copy_vault(parent, standard, "export").await
    }

    /// Copies the vault exactly (ids and all) into a new `<vault> backup <date time>`
    /// folder inside `parent`, then re-reads every copied file and compares it with the
    /// original. The copy is itself a vault: open it, or import it, to restore.
    /// The time is recorded in the `last_backup` setting.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] if `parent` is inside the vault; [`Error::Corrupt`] if a
    /// copied file does not match its original; I/O errors.
    pub async fn backup_vault(&self, parent: &Path) -> Result<ExportReport> {
        let report = self.copy_vault(parent, false, "backup").await?;
        let out = PathBuf::from(&report.destination);
        let root = self.vault.root().to_owned();
        let mut bad = Vec::new();
        let walker = WalkDir::new(&root)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| e.file_name().to_str().is_none_or(|n| !is_hidden_name(n)));
        for entry in walker.filter_map(std::result::Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&root) else {
                continue;
            };
            let same = match (fs::read(entry.path()), fs::read(out.join(rel))) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            };
            if !same {
                bad.push(rel.to_string_lossy().into_owned());
            }
        }
        if !bad.is_empty() {
            return Err(Error::Corrupt {
                context: report.destination,
                message: format!("copied files differ from the originals: {}", bad.join(", ")),
            });
        }
        self.db
            .set_setting("last_backup", &chrono::Utc::now().to_rfc3339())
            .await?;
        Ok(report)
    }

    async fn copy_vault(&self, parent: &Path, standard: bool, label: &str) -> Result<ExportReport> {
        let parent = self.outside_vault(parent, "the destination")?;
        let name = self
            .vault
            .root()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("NoteNest");
        let when = if label == "backup" {
            chrono::Local::now().format("%Y-%m-%d %H-%M")
        } else {
            chrono::Local::now().format("%Y-%m-%d")
        };
        let base = format!("{name} {label} {when}");
        let out = (1..=MAX_NAME_ATTEMPTS)
            .map(|n| {
                parent.join(if n == 1 {
                    base.clone()
                } else {
                    format!("{base} {n}")
                })
            })
            .find(|p| fs::create_dir(p).is_ok())
            .ok_or_else(|| Error::AlreadyExists(base.clone()))?;
        let resolver = if standard {
            Some(self.db.resolver().await?)
        } else {
            None
        };
        let mut report = ExportReport {
            destination: out.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let root = self.vault.root().to_owned();
        let walker = WalkDir::new(&root)
            .follow_links(false)
            .sort_by_file_name()
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| e.file_name().to_str().is_none_or(|n| !is_hidden_name(n)));
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let at = e.path().map(|p| p.to_string_lossy().into_owned());
                    report
                        .skipped
                        .push(skip(at.unwrap_or_default(), e.to_string()));
                    continue;
                }
            };
            if entry.file_type().is_symlink() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&root) else {
                continue;
            };
            let target = out.join(rel);
            let rel = rel.to_string_lossy().replace('\\', "/");
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target).map_err(|e| Error::io(&target, e))?;
                continue;
            }
            let bytes = match fs::read(entry.path()) {
                Ok(b) => b,
                Err(e) => {
                    report.skipped.push(skip(rel, e.to_string()));
                    continue;
                }
            };
            let markdown = is_markdown(&rel);
            let bytes = match (&resolver, markdown, String::from_utf8(bytes)) {
                (Some(resolver), true, Ok(text)) => standardize(
                    &text,
                    &rel,
                    |key| resolver.resolve(&rel, key).map(|c| c.path.clone()),
                    WikiLinks::Relative,
                )
                .into_bytes(),
                (_, _, Ok(text)) => text.into_bytes(),
                (_, _, Err(e)) => e.into_bytes(),
            };
            write_atomic(&target, &bytes)?;
            if markdown {
                report.notes += 1;
            } else {
                report.files += 1;
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::note_id_for_path;

    async fn vault() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
        (dir, ws)
    }

    fn write(path: &Path, content: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[tokio::test]
    async fn imports_files_with_their_attachments_without_overwriting() {
        let (vdir, ws) = vault().await;
        let existing = ws.create_note(None, Some("Plan")).await.unwrap();
        let existing_text = read(&vdir.path().join("Plan.md"));
        let id = parse_meta(&existing_text).id.unwrap();

        let src = tempfile::tempdir().unwrap();
        let notes = src.path().join("notes");
        write(&notes.join("img/a.png"), b"A");
        write(&src.path().join("shared/b.png"), b"B");
        write(&src.path().join(".secret/k.png"), b"K");
        write(
            &notes.join("Plan.md"),
            format!(
                "---\nid: {id}\n---\n![a](img/a.png) ![b](../shared/b.png) ![k](../.secret/k.png) \
                 ![gone](missing.png)"
            )
            .as_bytes(),
        );
        write(&notes.join("Other.md"), b"![a](img/a.png)");
        write(&notes.join("photo.jpg"), b"not markdown");

        let report = ws
            .import_files(
                &[
                    notes.join("Plan.md"),
                    notes.join("Other.md"),
                    notes.join("photo.jpg"),
                ],
                None,
            )
            .await
            .unwrap();
        assert_eq!(report.notes, ["Plan 2.md", "Other.md"]);
        assert_eq!(report.files, 2); // img/a.png once, b.png
        assert_eq!(report.new_ids, 1);
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].path.ends_with("photo.jpg"));

        // The existing note is untouched and keeps its id; the copy got a fresh one.
        assert_eq!(read(&vdir.path().join("Plan.md")), existing_text);
        let imported = read(&vdir.path().join("Plan 2.md"));
        let new_id = parse_meta(&imported).id.unwrap();
        assert_ne!(new_id, existing.id);
        // Same relative place when possible; otherwise Attachments/ with the link rewritten.
        assert_eq!(read(&vdir.path().join("img/a.png")), "A");
        assert_eq!(read(&vdir.path().join("Attachments/b.png")), "B");
        assert!(imported.contains("![b](Attachments/b.png)"), "{imported}");
        // Hidden paths are never followed; broken references stay as written.
        assert!(!vdir.path().join("Attachments/k.png").exists());
        assert!(imported.contains("![k](../.secret/k.png)"));
        assert!(imported.contains("![gone](missing.png)"));
        // Indexed right away.
        let titles: Vec<_> = ws
            .list_notes()
            .await
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert!(titles.contains(&"Plan 2".to_owned()) && titles.contains(&"Other".to_owned()));
    }

    #[tokio::test]
    async fn imports_into_a_folder_and_rejects_files_from_the_vault() {
        let (vdir, ws) = vault().await;
        ws.create_folder("Inbox").await.unwrap();
        let src = tempfile::tempdir().unwrap();
        write(&src.path().join("n.md"), b"hello");
        let report = ws
            .import_files(&[src.path().join("n.md")], Some("Inbox"))
            .await
            .unwrap();
        assert_eq!(report.notes, ["Inbox/n.md"]);

        let report = ws
            .import_files(&[vdir.path().join("Inbox/n.md")], None)
            .await
            .unwrap();
        assert!(report.notes.is_empty());
        assert!(report.skipped[0].reason.contains("inside the vault"));
        assert!(ws.import_files(&[], Some("Nope")).await.is_err());
    }

    #[tokio::test]
    async fn imports_a_folder_tree_as_a_new_folder() {
        let (vdir, ws) = vault().await;
        let src = tempfile::tempdir().unwrap();
        let root = src.path().join("Research");
        let id = Uuid::new_v4();
        write(
            &root.join("index.md"),
            format!("---\nid: {id}\n---\n[[Topic]]").as_bytes(),
        );
        write(&root.join("Sub/Topic.md"), b"topic ![](../img/x.png)");
        write(&root.join("img/x.png"), b"X");
        write(&root.join(".obsidian/config.json"), b"{}");
        write(&root.join(".hidden.md"), b"h");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", root.join("link")).unwrap();

        let first = ws.import_folder(&root, None).await.unwrap();
        assert_eq!(first.folder.as_deref(), Some("Research"));
        assert_eq!(first.notes, ["Research/Sub/Topic.md", "Research/index.md"]);
        assert_eq!(first.files, 1);
        assert_eq!(first.new_ids, 0);
        assert_eq!(read(&vdir.path().join("Research/img/x.png")), "X");
        assert!(!vdir.path().join("Research/.obsidian").exists());
        assert!(!vdir.path().join("Research/.hidden.md").exists());
        assert!(!vdir.path().join("Research/link").exists());

        // Again: a new folder, and the copied ids are made unique.
        let second = ws.import_folder(&root, None).await.unwrap();
        assert_eq!(second.folder.as_deref(), Some("Research 2"));
        assert_eq!(second.new_ids, 1);
        let ids: HashSet<Uuid> = ws
            .list_notes()
            .await
            .unwrap()
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids.len(), 4);
        assert!(ids.contains(&id));
        assert!(ws.database().get_note(id).await.unwrap().unwrap().path == "Research/index.md");
    }

    #[tokio::test]
    async fn refuses_to_import_the_vault_or_a_folder_containing_it() {
        let (vdir, ws) = vault().await;
        fs::create_dir(vdir.path().join("Sub")).unwrap();
        for p in [
            vdir.path().to_owned(),
            vdir.path().join("Sub"),
            vdir.path().parent().unwrap().to_owned(),
        ] {
            let err = ws.import_folder(&p, None).await.unwrap_err();
            assert!(matches!(err, Error::InvalidPath(_)), "{p:?}: {err}");
        }
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    async fn sample(ws: &Workspace, vdir: &Path) -> Uuid {
        ws.create_folder("Projects").await.unwrap();
        write(&vdir.join("Attachments/pic.png"), PNG);
        let a = ws
            .create_note(Some("Projects"), Some("Plan"))
            .await
            .unwrap();
        let text = read(&vdir.join("Projects/Plan.md"));
        fs::write(
            vdir.join("Projects/Plan.md"),
            format!(
                "{text}See [[Ideas#Big One]] and [[Nowhere]].\n![pic](../Attachments/pic.png)\n"
            ),
        )
        .unwrap();
        write(&vdir.join("Ideas.md"), b"# Ideas\n## Big One\n");
        ws.reindex().await.unwrap();
        a.id
    }

    #[tokio::test]
    async fn exports_the_vault_as_standard_markdown() {
        let (vdir, ws) = vault().await;
        sample(&ws, vdir.path()).await;
        ws.stash_draft(note_id_for_path("Ideas.md"), "draft", "h")
            .await
            .unwrap();
        let out = tempfile::tempdir().unwrap();

        let report = ws.export_vault(out.path(), true).await.unwrap();
        let dest = PathBuf::from(&report.destination);
        assert!(dest.starts_with(out.path()));
        assert_eq!((report.notes, report.files), (2, 1));
        let plan = read(&dest.join("Projects/Plan.md"));
        assert!(!plan.contains("id:"), "{plan}");
        assert!(
            plan.contains("[Ideas > Big One](../Ideas.md#big-one)"),
            "{plan}"
        );
        assert!(plan.contains("Nowhere."), "{plan}");
        assert!(plan.contains("![pic](../Attachments/pic.png)"));
        assert_eq!(fs::read(dest.join("Attachments/pic.png")).unwrap(), PNG);
        assert!(!dest.join(".notenest").exists());

        // A second export never touches the first.
        let again = ws.export_vault(out.path(), false).await.unwrap();
        assert_ne!(again.destination, report.destination);
        let exact = read(&PathBuf::from(&again.destination).join("Projects/Plan.md"));
        assert_eq!(exact, read(&vdir.path().join("Projects/Plan.md")));
    }

    #[tokio::test]
    async fn exports_one_note_with_its_attachments() {
        let (vdir, ws) = vault().await;
        let id = sample(&ws, vdir.path()).await;
        let out = tempfile::tempdir().unwrap();
        // Something unrelated already sits where the image would go.
        write(&out.path().join("Attachments/pic.png"), b"OTHER");

        let report = ws
            .export_note(id, &out.path().join("Shared plan"), true)
            .await
            .unwrap();
        let dest = out.path().join("Shared plan.md");
        assert_eq!(PathBuf::from(&report.destination), dest);
        assert_eq!(report.files, 1);
        let text = read(&dest);
        assert!(text.contains("See Ideas > Big One and Nowhere."), "{text}");
        // The image escaped the export folder, so it moved under Attachments/, renamed
        // because that name was taken by a different file, which is left alone.
        assert!(text.contains("![pic](Attachments/pic-2.png)"), "{text}");
        assert_eq!(
            fs::read(out.path().join("Attachments/pic-2.png")).unwrap(),
            PNG
        );
        assert_eq!(read(&out.path().join("Attachments/pic.png")), "OTHER");

        let err = ws
            .export_note(id, &vdir.path().join("copy.md"), true)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidPath(_)));
        assert!(ws.export_vault(vdir.path(), true).await.is_err());
    }

    #[tokio::test]
    async fn an_exported_vault_imports_back_with_working_links() {
        let (vdir, ws) = vault().await;
        sample(&ws, vdir.path()).await;
        let out = tempfile::tempdir().unwrap();
        let exported = ws.export_vault(out.path(), true).await.unwrap();

        let (_v2, other) = vault().await;
        let report = other
            .import_folder(Path::new(&exported.destination), None)
            .await
            .unwrap();
        assert_eq!(report.notes.len(), 2);
        assert_eq!(report.files, 1);
        let top = report.folder.unwrap();
        let plan = other
            .database()
            .get_note_by_path(&format!("{top}/Projects/Plan.md"))
            .await
            .unwrap()
            .unwrap();
        let content = other.read_note(plan.id).await.unwrap().content;
        let html = other.render_markdown(Some(plan.id), content).await.unwrap();
        assert!(html.warnings.is_empty(), "{:?}", html.warnings);
        assert!(html.html.contains("<img"), "{}", html.html);
    }

    #[tokio::test]
    async fn backs_up_an_exact_verified_copy_that_opens_as_a_vault() {
        let (vdir, ws) = vault().await;
        let id = sample(&ws, vdir.path()).await;
        let out = tempfile::tempdir().unwrap();

        let report = ws.backup_vault(out.path()).await.unwrap();
        let dest = PathBuf::from(&report.destination);
        assert!(dest
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(" backup "));
        assert_eq!(
            read(&dest.join("Projects/Plan.md")),
            read(&vdir.path().join("Projects/Plan.md"))
        );
        assert!(ws.get_setting("last_backup").await.unwrap().is_some());

        // Restoring: the backup opens as a vault with the same note ids.
        let (restored, _) = Workspace::open(&dest, false).await.unwrap();
        assert!(restored.database().get_note(id).await.unwrap().is_some());
        // A second backup never touches the first.
        let second = ws.backup_vault(out.path()).await.unwrap();
        assert_ne!(second.destination, report.destination);
        assert!(ws.backup_vault(vdir.path()).await.is_err());
    }
}
