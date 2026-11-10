//! Keeping `[[wiki links]]` working when a note is renamed or moved.
//!
//! Links are matched by name at query time, so a rename never corrupts the index; it
//! just makes some links in *other* notes stop resolving. This module rewrites exactly
//! those links, and nothing else:
//!
//! 1. before the operation, capture every link that resolves to a moved note;
//! 2. perform the operation;
//! 3. re-resolve each captured link against the new state: links that still resolve
//!    (title-only links after a move, alias links, ...) are left alone;
//! 4. for the rest, edit only the target text inside `[[...]]` (the `|display`, `#heading`
//!    and `.md` parts keep their form), at exact positions found by parsing the file.
//!
//! Safety: a note is skipped (and reported) if it changed on disk since it was indexed
//! or the link cannot be found; every original is copied to
//! `.notenest/recovery/link-updates/<time>/` before it is touched; each file is written
//! atomically. A failure on one note never stops the others.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::workspace::{child, Workspace};
use crate::database::{CapturedLink, Resolver};
use crate::extract::{extract, link_key};
use crate::filesystem::atomic::write_atomic;
use crate::filesystem::RelPath;
use crate::hash::content_hash;
use crate::models::{title_from_path, NoteRecord, NoteRef};
use crate::{Error, Result};

/// A note whose links were (or would be) rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AffectedNote {
    pub note: NoteRef,
    pub links: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedNote {
    pub note: NoteRef,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LinkUpdateReport {
    pub updated: Vec<AffectedNote>,
    pub skipped: Vec<SkippedNote>,
    /// Where the originals of the updated notes were copied (relative to the vault).
    pub backup_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameOutcome {
    pub record: NoteRecord,
    pub links: LinkUpdateReport,
}

/// How paths change in an operation.
#[derive(Debug, Clone)]
enum PathMap {
    File { from: String, to: String },
    Prefix { from: String, to: String },
}

impl PathMap {
    fn apply(&self, path: &str) -> String {
        match self {
            Self::File { from, to } => {
                if path == from {
                    to.clone()
                } else {
                    path.to_owned()
                }
            }
            Self::Prefix { from, to } => {
                if path == from {
                    to.clone()
                } else if let Some(rest) = path.strip_prefix(&format!("{from}/")) {
                    format!("{to}/{rest}")
                } else {
                    path.to_owned()
                }
            }
        }
    }
}

struct Planned {
    /// Where the source note lives before the operation (to find its record in a preview).
    old_source_path: String,
    /// Where it lives after.
    source_path: String,
    target_text: String,
    line: usize,
    /// The new target text, or `None` if no form of it would resolve correctly.
    new_target: Option<String>,
}

fn split_suffix(target: &str) -> (&str, &str) {
    let at = target.find(['#', '^']).unwrap_or(target.len());
    target.split_at(at)
}

/// The text to write in place of `old_text` so the link points at `new_target_path` again,
/// keeping its style (path-qualified or title-only, with or without `.md`, same heading).
fn rewrite_target(
    old_text: &str,
    new_target_path: &str,
    new_source_path: &str,
    resolver: &Resolver,
) -> Option<String> {
    let (base, suffix) = split_suffix(old_text);
    let had_md = base.to_ascii_lowercase().ends_with(".md");
    let path_qualified = base.contains('/');
    let no_ext = new_target_path
        .strip_suffix(".md")
        .unwrap_or(new_target_path);
    let title = no_ext.rsplit('/').next().unwrap_or(no_ext);
    let ext = if had_md { ".md" } else { "" };
    let forms: &[&str] = if path_qualified {
        &[no_ext]
    } else {
        &[title, no_ext]
    };
    forms.iter().find_map(|form| {
        let candidate = format!("{form}{ext}{suffix}");
        let hit = resolver.resolve(new_source_path, &link_key(&candidate))?;
        (hit.path == new_target_path).then_some(candidate)
    })
}

fn plan(captured: &[CapturedLink], map: &PathMap, resolver: &Resolver) -> Vec<Planned> {
    captured
        .iter()
        .filter_map(|c| {
            let source_path = map.apply(&c.source_path);
            let target_path = map.apply(&c.target_path);
            let still_ok = resolver
                .resolve(&source_path, &link_key(&c.target_text))
                .is_some_and(|hit| hit.path == target_path);
            if still_ok {
                return None;
            }
            Some(Planned {
                new_target: rewrite_target(&c.target_text, &target_path, &source_path, resolver),
                old_source_path: c.source_path.clone(),
                source_path,
                target_text: c.target_text.clone(),
                line: c.line,
            })
        })
        .collect()
}

/// Replaces link targets in `content`. Each edit is `(line, old target text, new target
/// text)` and is applied to the first not-yet-edited link with that line and target.
/// Returns the new text and how many links were rewritten.
fn rewrite_content(content: &str, edits: &mut Vec<(usize, String, String)>) -> (String, usize) {
    let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
    for link in extract(content).links {
        let Some(i) = edits
            .iter()
            .position(|e| e.0 == link.line && e.1 == link.target_text)
        else {
            continue;
        };
        let (_, old, new) = edits.remove(i);
        let span_text = &content[link.span.clone()];
        if let Some(rel) = span_text.find(&old) {
            let start = link.span.start + rel;
            replacements.push((start..start + old.len(), new));
        }
    }
    let applied = replacements.len();
    replacements.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut out = content.to_owned();
    for (range, new) in replacements {
        out.replace_range(range, &new);
    }
    (out, applied)
}

impl Workspace {
    // ------------------------------------------------------------- previews

    /// Notes whose links would be rewritten if note `id` were renamed to `new_title`.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] for a bad name; [`Error::NotFound`] for an unknown note.
    pub async fn preview_rename_note(
        &self,
        id: Uuid,
        new_title: &str,
    ) -> Result<Vec<AffectedNote>> {
        let record = self.record(id).await?;
        let from = RelPath::new(&record.path)?;
        let to = child(from.parent().as_ref(), &format!("{new_title}.md"))?;
        let map = PathMap::File {
            from: from.to_string(),
            to: to.to_string(),
        };
        self.preview(&HashSet::from([id]), &map).await
    }

    /// Notes whose links would be rewritten if note `id` were moved into `folder`.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown note or folder.
    pub async fn preview_move_note(
        &self,
        id: Uuid,
        folder: Option<&str>,
    ) -> Result<Vec<AffectedNote>> {
        let record = self.record(id).await?;
        let from = RelPath::new(&record.path)?;
        let to = child(self.folder_path(folder)?.as_ref(), from.file_name())?;
        let map = PathMap::File {
            from: from.to_string(),
            to: to.to_string(),
        };
        self.preview(&HashSet::from([id]), &map).await
    }

    /// Notes whose links would be rewritten if folder `path` were renamed to `new_name`.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] for a bad name.
    pub async fn preview_rename_folder(
        &self,
        path: &str,
        new_name: &str,
    ) -> Result<Vec<AffectedNote>> {
        let from = RelPath::new(path)?;
        let to = child(from.parent().as_ref(), new_name)?;
        let map = PathMap::Prefix {
            from: from.to_string(),
            to: to.to_string(),
        };
        self.preview_folder(&from, &map).await
    }

    /// Notes whose links would be rewritten if folder `path` were moved under `new_parent`.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown destination.
    pub async fn preview_move_folder(
        &self,
        path: &str,
        new_parent: Option<&str>,
    ) -> Result<Vec<AffectedNote>> {
        let from = RelPath::new(path)?;
        let to = child(self.folder_path(new_parent)?.as_ref(), from.file_name())?;
        let map = PathMap::Prefix {
            from: from.to_string(),
            to: to.to_string(),
        };
        self.preview_folder(&from, &map).await
    }

    async fn preview_folder(&self, folder: &RelPath, map: &PathMap) -> Result<Vec<AffectedNote>> {
        let moved = self.notes_under(folder).await?;
        self.preview(&moved, map).await
    }

    async fn notes_under(&self, folder: &RelPath) -> Result<HashSet<Uuid>> {
        Ok(self
            .db
            .list_notes()
            .await?
            .into_iter()
            .filter(|n| {
                folder.contains(&RelPath::new(&n.path).unwrap_or_else(|_| folder.clone()))
                    && n.path != folder.as_str()
            })
            .map(|n| n.id)
            .collect())
    }

    async fn preview(&self, moved: &HashSet<Uuid>, map: &PathMap) -> Result<Vec<AffectedNote>> {
        let captured = self.db.links_to(moved).await?;
        let notes = self.db.list_notes().await?;
        let moves: HashMap<Uuid, (String, String)> = notes
            .iter()
            .filter(|n| moved.contains(&n.id))
            .map(|n| {
                let new_path = map.apply(&n.path);
                let title = title_from_path(&new_path);
                (n.id, (new_path, title))
            })
            .collect();
        let resolver = self.db.resolver().await?.simulate(&moves);
        let planned = plan(&captured, map, &resolver);
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for p in &planned {
            *counts.entry(p.old_source_path.clone()).or_default() += 1;
        }
        Ok(notes
            .into_iter()
            .filter_map(|n| {
                counts.get(&n.path).map(|&links| AffectedNote {
                    note: NoteRef {
                        id: n.id,
                        title: n.title,
                        path: n.path,
                    },
                    links,
                })
            })
            .collect())
    }

    // ----------------------------------------------------------- operations

    /// Renames a note and rewrites the links to it in other notes (see the module docs).
    ///
    /// # Errors
    /// As [`Workspace::rename_note`]; problems with individual notes are reported in the
    /// returned [`LinkUpdateReport`], not as errors.
    pub async fn rename_note_with_links(&self, id: Uuid, new_title: &str) -> Result<RenameOutcome> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let from = RelPath::new(&record.path)?;
        let to = child(from.parent().as_ref(), &format!("{new_title}.md"))?;
        self.move_with_links(&record, &from, &to).await
    }

    /// Moves a note into `folder` and rewrites links that no longer resolve.
    ///
    /// # Errors
    /// As [`Workspace::move_note`].
    pub async fn move_note_with_links(
        &self,
        id: Uuid,
        folder: Option<&str>,
    ) -> Result<RenameOutcome> {
        let _g = self.write_lock.lock().await;
        let record = self.record(id).await?;
        let from = RelPath::new(&record.path)?;
        let to = child(self.folder_path(folder)?.as_ref(), from.file_name())?;
        self.move_with_links(&record, &from, &to).await
    }

    async fn move_with_links(
        &self,
        record: &NoteRecord,
        from: &RelPath,
        to: &RelPath,
    ) -> Result<RenameOutcome> {
        let captured = self.db.links_to(&HashSet::from([record.id])).await?;
        let fresh = self.relocate(record, from, to).await?;
        let map = PathMap::File {
            from: from.to_string(),
            to: to.to_string(),
        };
        let links = self.apply_link_updates(&captured, &map).await?;
        // The note itself may have been edited (links to itself), so read it back.
        let record = self
            .db
            .get_note_by_path(to.as_str())
            .await?
            .unwrap_or(fresh);
        Ok(RenameOutcome { record, links })
    }

    /// Renames a folder and rewrites the links that stopped resolving.
    ///
    /// # Errors
    /// As [`Workspace::rename_folder`].
    pub async fn rename_folder_with_links(
        &self,
        path: &str,
        new_name: &str,
    ) -> Result<LinkUpdateReport> {
        let from = RelPath::new(path)?;
        let to = child(from.parent().as_ref(), new_name)?;
        self.folder_with_links(&from, &to).await
    }

    /// Moves a folder under `new_parent` and rewrites the links that stopped resolving.
    ///
    /// # Errors
    /// As [`Workspace::move_folder`].
    pub async fn move_folder_with_links(
        &self,
        path: &str,
        new_parent: Option<&str>,
    ) -> Result<LinkUpdateReport> {
        let from = RelPath::new(path)?;
        let to = child(self.folder_path(new_parent)?.as_ref(), from.file_name())?;
        self.folder_with_links(&from, &to).await
    }

    async fn folder_with_links(&self, from: &RelPath, to: &RelPath) -> Result<LinkUpdateReport> {
        let _g = self.write_lock.lock().await;
        let moved = self.notes_under(from).await?;
        let captured = self.db.links_to(&moved).await?;
        self.vault.rename(from, to)?;
        self.rescan().await?;
        let map = PathMap::Prefix {
            from: from.to_string(),
            to: to.to_string(),
        };
        self.apply_link_updates(&captured, &map).await
    }

    // ------------------------------------------------------------- applying

    async fn apply_link_updates(
        &self,
        captured: &[CapturedLink],
        map: &PathMap,
    ) -> Result<LinkUpdateReport> {
        let resolver = self.db.resolver().await?;
        let mut by_source: BTreeMap<String, Vec<Planned>> = BTreeMap::new();
        for p in plan(captured, map, &resolver) {
            by_source.entry(p.source_path.clone()).or_default().push(p);
        }
        let mut report = LinkUpdateReport::default();
        let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3f").to_string();

        for (path, items) in by_source {
            let Some(record) = self.db.get_note_by_path(&path).await? else {
                continue;
            };
            let note = NoteRef {
                id: record.id,
                title: record.title.clone(),
                path: path.clone(),
            };
            let skip = |report: &mut LinkUpdateReport, reason: &str| {
                report.skipped.push(SkippedNote {
                    note: note.clone(),
                    reason: reason.to_owned(),
                });
            };
            let rel = RelPath::new(&path)?;
            let bytes = match self.vault.read(&rel) {
                Ok(b) => b,
                Err(e) => {
                    skip(&mut report, &e.to_string());
                    continue;
                }
            };
            if content_hash(&bytes) != record.content_hash {
                skip(
                    &mut report,
                    "it changed on disk since it was indexed; update its links by hand",
                );
                continue;
            }
            let Ok(text) = String::from_utf8(bytes.clone()) else {
                skip(&mut report, "it is not valid UTF-8");
                continue;
            };
            let unresolved = items.iter().filter(|i| i.new_target.is_none()).count();
            let mut edits: Vec<(usize, String, String)> = items
                .iter()
                .filter_map(|i| {
                    i.new_target
                        .clone()
                        .map(|n| (i.line, i.target_text.clone(), n))
                })
                .collect();
            if edits.is_empty() {
                skip(&mut report, "its links are ambiguous; update them by hand");
                continue;
            }
            let (new_text, applied) = rewrite_content(&text, &mut edits);
            if applied == 0 {
                skip(&mut report, "the links could not be located in the file");
                continue;
            }

            // Keep the original, then replace it atomically.
            let backup = self
                .vault
                .meta_dir()
                .join("recovery")
                .join("link-updates")
                .join(&stamp)
                .join(&path);
            let backed_up = backup
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(|e| Error::io(&backup, e))
                .and_then(|()| write_atomic(&backup, &bytes));
            if let Err(e) = backed_up {
                skip(
                    &mut report,
                    &format!("its backup failed ({e}); it was not changed"),
                );
                continue;
            }
            report.backup_dir = Some(format!(".notenest/recovery/link-updates/{stamp}"));
            if let Err(e) = self.vault.write(&rel, new_text.as_bytes()) {
                skip(&mut report, &format!("it could not be written ({e})"));
                continue;
            }
            let fresh = self.build_record(&rel, new_text.as_bytes(), Some(&record));
            self.store(Some(&record), &fresh, &new_text).await?;
            report.updated.push(AffectedNote {
                note: note.clone(),
                links: applied,
            });
            if unresolved > 0 || !edits.is_empty() {
                skip(
                    &mut report,
                    &format!("{} link(s) were left unchanged", unresolved + edits.len()),
                );
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::SaveMode;
    use std::fs;

    async fn open() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
        (dir, ws)
    }

    async fn note(ws: &Workspace, folder: Option<&str>, title: &str, content: &str) -> NoteRecord {
        let n = ws.create_note(folder, Some(title)).await.unwrap();
        ws.save_note(n.id, content, SaveMode::Overwrite)
            .await
            .unwrap()
            .record
    }

    fn read(dir: &tempfile::TempDir, rel: &str) -> String {
        fs::read_to_string(dir.path().join(rel)).unwrap()
    }

    /// After any link update the incrementally maintained index must equal a rebuild.
    async fn assert_index_matches_rebuild(ws: &Workspace) {
        let incremental = ws.database().derived_snapshot().await.unwrap();
        ws.reindex().await.unwrap();
        assert_eq!(incremental, ws.database().derived_snapshot().await.unwrap());
    }

    #[test]
    fn target_rewriting_keeps_the_links_style() {
        // Resolver content is exercised through the workspace tests below; this checks
        // the suffix handling on its own.
        assert_eq!(split_suffix("Note#Heading"), ("Note", "#Heading"));
        assert_eq!(split_suffix("Note^abc"), ("Note", "^abc"));
        assert_eq!(split_suffix("Note"), ("Note", ""));
    }

    #[test]
    fn content_rewriting_edits_only_the_targets() {
        let text = "a [[Old]] b [[Old|shown]] c `[[Old]]` [[Old]]\n";
        let mut edits = vec![(1, "Old".to_owned(), "New".to_owned()); 3];
        let (out, n) = rewrite_content(text, &mut edits);
        assert_eq!(n, 3);
        assert_eq!(out, "a [[New]] b [[New|shown]] c `[[Old]]` [[New]]\n");
    }

    #[tokio::test]
    async fn renaming_updates_every_form_of_link_and_nothing_else() {
        let (dir, ws) = open().await;
        let target = note(&ws, None, "Old Name", "---\naliases: [Legacy]\n---\nBody").await;
        let linker = note(
            &ws,
            None,
            "Linker",
            "Plain [[Old Name]]. Shown [[Old Name|the thing]]. Head [[Old Name#Intro]].\nLower [[old name]]. Ext [[Old Name.md]]. Alias [[Legacy]]. Other [[Elsewhere]].\n```\n[[Old Name]]\n```\n",
        )
        .await;

        let preview = ws.preview_rename_note(target.id, "New Name").await.unwrap();
        assert_eq!(preview.len(), 1);
        assert_eq!((preview[0].note.id, preview[0].links), (linker.id, 5));

        let out = ws
            .rename_note_with_links(target.id, "New Name")
            .await
            .unwrap();
        assert_eq!(out.record.path, "New Name.md");
        assert_eq!(out.links.updated.len(), 1);
        assert_eq!(out.links.updated[0].links, 5);
        assert!(out.links.skipped.is_empty());

        let text = read(&dir, "Linker.md");
        assert!(
            text.contains(
                "Plain [[New Name]]. Shown [[New Name|the thing]]. Head [[New Name#Intro]]."
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "Lower [[New Name]]. Ext [[New Name.md]]. Alias [[Legacy]]. Other [[Elsewhere]]."
            ),
            "{text}"
        );
        assert!(
            text.contains("```\n[[Old Name]]\n```"),
            "code must stay untouched: {text}"
        );
        assert_eq!(
            ws.database().broken_links().await.unwrap().len(),
            1,
            "only [[Elsewhere]] stays broken"
        );
        assert_eq!(
            ws.database().backlinks(out.record.id).await.unwrap().len(),
            6
        );
        assert_index_matches_rebuild(&ws).await;
    }

    #[tokio::test]
    async fn path_qualified_links_follow_a_rename() {
        let (dir, ws) = open().await;
        let target = note(&ws, None, "Docs", "").await;
        ws.delete_note(target.id).await.unwrap();
        ws.create_folder("Docs").await.unwrap();
        let target = note(&ws, Some("Docs"), "Spec", "").await;
        note(
            &ws,
            None,
            "Linker",
            "[[Docs/Spec]] and [[Docs/Spec.md#H|x]] and [[Spec]]",
        )
        .await;
        ws.rename_note_with_links(target.id, "Design")
            .await
            .unwrap();
        assert!(read(&dir, "Linker.md")
            .contains("[[Docs/Design]] and [[Docs/Design.md#H|x]] and [[Design]]"));
    }

    #[tokio::test]
    async fn moving_only_touches_links_that_stopped_resolving() {
        let (dir, ws) = open().await;
        ws.create_folder("Archive").await.unwrap();
        let target = note(&ws, None, "Topic", "").await;
        note(&ws, None, "ByTitle", "See [[Topic]].").await;
        note(&ws, None, "ByPath", "See [[Topic.md]] and [[topic]].").await;
        // Title-only links still resolve after the move, so there is nothing to rewrite.
        assert!(ws
            .preview_move_note(target.id, Some("Archive"))
            .await
            .unwrap()
            .is_empty());
        let out = ws
            .move_note_with_links(target.id, Some("Archive"))
            .await
            .unwrap();
        assert!(out.links.updated.is_empty() && out.links.backup_dir.is_none());
        assert_eq!(read(&dir, "ByTitle.md").matches("[[Topic]]").count(), 1);
        assert_eq!(ws.database().broken_links().await.unwrap().len(), 0);
        assert_index_matches_rebuild(&ws).await;
    }

    #[tokio::test]
    async fn a_move_that_makes_a_title_ambiguous_switches_that_link_to_a_path() {
        let (dir, ws) = open().await;
        ws.create_folder("Z").await.unwrap();
        ws.create_folder("Much/Deeper").await.unwrap();
        let moving = note(&ws, None, "Dup", "mine").await;
        note(&ws, Some("Z"), "Dup", "other").await;
        note(&ws, None, "Linker", "See [[Dup]].").await;
        // At the root `Dup` is the linker's own-folder note; deep down, Z/Dup is nearer.
        let preview = ws
            .preview_move_note(moving.id, Some("Much/Deeper"))
            .await
            .unwrap();
        assert_eq!(preview.len(), 1);
        let out = ws
            .move_note_with_links(moving.id, Some("Much/Deeper"))
            .await
            .unwrap();
        assert_eq!(out.links.updated[0].links, 1);
        assert!(read(&dir, "Linker.md").contains("[[Much/Deeper/Dup]]"));
        let links = ws.database().backlinks(out.record.id).await.unwrap();
        assert_eq!(links.len(), 1);
    }

    #[tokio::test]
    async fn renaming_a_folder_rewrites_path_links_into_it() {
        let (dir, ws) = open().await;
        ws.create_folder("Projects").await.unwrap();
        note(
            &ws,
            Some("Projects"),
            "Alpha",
            "Sibling [[Projects/Beta]] and [[Beta]]",
        )
        .await;
        note(&ws, Some("Projects"), "Beta", "").await;
        note(
            &ws,
            None,
            "Outside",
            "[[Projects/Alpha]] [[Alpha]] [[Projects/Beta#Top]]",
        )
        .await;

        let preview = ws.preview_rename_folder("Projects", "Work").await.unwrap();
        let mut got: Vec<(String, usize)> = preview
            .iter()
            .map(|a| (a.note.path.clone(), a.links))
            .collect();
        got.sort();
        assert_eq!(
            got,
            [
                ("Outside.md".to_owned(), 2),
                ("Projects/Alpha.md".to_owned(), 1)
            ]
        );

        let report = ws
            .rename_folder_with_links("Projects", "Work")
            .await
            .unwrap();
        assert_eq!(report.updated.len(), 2, "{report:?}");
        assert!(read(&dir, "Work/Alpha.md").contains("[[Work/Beta]] and [[Beta]]"));
        assert!(read(&dir, "Outside.md").contains("[[Work/Alpha]] [[Alpha]] [[Work/Beta#Top]]"));
        assert_eq!(ws.database().broken_links().await.unwrap().len(), 0);
        assert_index_matches_rebuild(&ws).await;
    }

    #[tokio::test]
    async fn moving_a_folder_rewrites_links_too() {
        let (dir, ws) = open().await;
        ws.create_folder("Inner").await.unwrap();
        ws.create_folder("Outer").await.unwrap();
        note(&ws, Some("Inner"), "Page", "").await;
        note(&ws, None, "Linker", "[[Inner/Page]]").await;
        let report = ws
            .move_folder_with_links("Inner", Some("Outer"))
            .await
            .unwrap();
        assert_eq!(report.updated.len(), 1);
        assert!(read(&dir, "Linker.md").contains("[[Outer/Inner/Page]]"));
    }

    #[tokio::test]
    async fn links_inside_the_renamed_note_itself_are_updated() {
        let (dir, ws) = open().await;
        let n = note(&ws, None, "Self", "I mention [[Self]] here.").await;
        let out = ws.rename_note_with_links(n.id, "Me").await.unwrap();
        assert!(read(&dir, "Me.md").contains("I mention [[Me]] here."));
        assert_eq!(out.links.updated.len(), 1);
        let reread = ws.read_note(out.record.id).await.unwrap();
        assert!(reread.content.contains("[[Me]]"));
    }

    #[tokio::test]
    async fn notes_changed_on_disk_since_indexing_are_skipped_untouched() {
        let (dir, ws) = open().await;
        let target = note(&ws, None, "Target", "").await;
        note(&ws, None, "Safe", "[[Target]]").await;
        note(&ws, None, "Edited", "[[Target]]").await;
        // Edited in another editor after the index last saw it.
        fs::write(
            dir.path().join("Edited.md"),
            "[[Target]] plus a new line typed elsewhere",
        )
        .unwrap();

        let out = ws
            .rename_note_with_links(target.id, "Renamed")
            .await
            .unwrap();
        assert_eq!(out.links.updated.len(), 1);
        assert_eq!(out.links.updated[0].note.title, "Safe");
        assert_eq!(out.links.skipped.len(), 1);
        assert_eq!(out.links.skipped[0].note.title, "Edited");
        assert!(out.links.skipped[0].reason.contains("changed on disk"));
        assert_eq!(
            read(&dir, "Edited.md"),
            "[[Target]] plus a new line typed elsewhere"
        );
    }

    #[tokio::test]
    async fn originals_are_backed_up_before_they_change() {
        let (dir, ws) = open().await;
        let target = note(&ws, None, "Target", "").await;
        note(&ws, None, "Linker", "[[Target]]").await;
        let before = read(&dir, "Linker.md");
        let out = ws
            .rename_note_with_links(target.id, "Renamed")
            .await
            .unwrap();
        let backup = out
            .links
            .backup_dir
            .expect("a backup directory is reported");
        assert_eq!(read(&dir, &format!("{backup}/Linker.md")), before);
        assert_ne!(read(&dir, "Linker.md"), before);
    }

    #[tokio::test]
    async fn crlf_notes_stay_crlf() {
        let (dir, ws) = open().await;
        let target = note(&ws, None, "Target", "").await;
        let linker = note(&ws, None, "Linker", "x").await;
        fs::write(
            dir.path().join("Linker.md"),
            "line one\r\nsee [[Target]]\r\nline three\r\n",
        )
        .unwrap();
        ws.read_note(linker.id).await.unwrap(); // absorb the external edit into the index
        ws.rename_note_with_links(target.id, "Renamed")
            .await
            .unwrap();
        assert_eq!(
            read(&dir, "Linker.md"),
            "line one\r\nsee [[Renamed]]\r\nline three\r\n"
        );
    }

    #[tokio::test]
    async fn failures_change_nothing() {
        let (dir, ws) = open().await;
        let target = note(&ws, None, "Target", "").await;
        note(&ws, None, "Taken", "").await;
        note(&ws, None, "Linker", "[[Target]]").await;
        let before = read(&dir, "Linker.md");
        assert!(matches!(
            ws.rename_note_with_links(target.id, "Taken").await,
            Err(Error::AlreadyExists(_))
        ));
        assert!(matches!(
            ws.rename_note_with_links(target.id, "a/b").await,
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            ws.preview_rename_note(target.id, "a/b").await,
            Err(Error::InvalidPath(_))
        ));
        assert_eq!(read(&dir, "Linker.md"), before);
        assert!(!dir.path().join(".notenest/recovery/link-updates").exists());
    }

    #[tokio::test]
    async fn renaming_something_nobody_links_to_reports_nothing() {
        let (_d, ws) = open().await;
        let n = note(&ws, None, "Lonely", "").await;
        let out = ws
            .rename_note_with_links(n.id, "Still Lonely")
            .await
            .unwrap();
        assert_eq!(out.links, LinkUpdateReport::default());
        assert!(ws
            .preview_rename_note(out.record.id, "Again")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn the_preview_matches_what_is_applied() {
        let (_d, ws) = open().await;
        ws.create_folder("D").await.unwrap();
        let target = note(&ws, Some("D"), "Page", "").await;
        for i in 0..4 {
            note(
                &ws,
                None,
                &format!("L{i}"),
                &"[[Page]] [[D/Page]] ".repeat(i + 1),
            )
            .await;
        }
        let mut preview: Vec<(String, usize)> = ws
            .preview_rename_note(target.id, "Renamed")
            .await
            .unwrap()
            .into_iter()
            .map(|a| (a.note.path, a.links))
            .collect();
        let out = ws
            .rename_note_with_links(target.id, "Renamed")
            .await
            .unwrap();
        let mut applied: Vec<(String, usize)> = out
            .links
            .updated
            .into_iter()
            .map(|a| (a.note.path, a.links))
            .collect();
        preview.sort();
        applied.sort();
        assert_eq!(preview, applied);
        assert_eq!(applied.len(), 4);
    }

    #[tokio::test]
    async fn tags_are_edited_in_the_frontmatter_only() {
        let (dir, ws) = open().await;
        let n = note(&ws, None, "Tagged", "Text with #inline tag.").await;
        let rec = ws
            .set_tags(
                n.id,
                &["Rust".into(), "Programming/Rust".into(), "123".into()],
            )
            .await
            .unwrap();
        let text = read(&dir, "Tagged.md");
        assert!(text.contains("tags: [rust, programming/rust]"), "{text}");
        assert!(text.contains("Text with #inline tag."));
        let names: Vec<String> = ws
            .database()
            .list_tags()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["inline", "programming", "programming/rust", "rust"]);

        ws.set_tags(rec.id, &[]).await.unwrap();
        assert!(!read(&dir, "Tagged.md").contains("tags:"));
        assert!(
            ws.set_tags(rec.id, &[]).await.is_ok(),
            "a no-op edit is fine"
        );
        assert_index_matches_rebuild(&ws).await;
    }
}
