//! Watches the vault for changes made outside NoteNest.
//!
//! Filesystem events are only a hint: they can be dropped, merged or reordered by
//! the OS, and a rename may arrive as an unrelated delete and create. So events are
//! never applied one by one. Each burst of them (debounced) triggers a
//! [`Workspace::reindex`], which compares the whole vault with the index and is
//! cheap for files that did not change. A missed event is caught by the next pass,
//! and the app also reconciles when its window regains focus.
//!
//! NoteNest's own writes cause events too; the reindex finds them already indexed
//! and reports nothing. Events inside hidden entries (`.notenest/`, `.git/`, temp
//! and swap files) are ignored entirely.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::time::{timeout, Instant};
use uuid::Uuid;

use super::reindex::ReindexReport;
use super::workspace::Workspace;
use crate::{Error, Result};

/// How long the vault must be quiet before a burst of events is processed.
pub const QUIET: Duration = Duration::from_millis(250);
/// The longest a continuous stream of events delays processing.
pub const MAX_DELAY: Duration = Duration::from_secs(2);
/// When to look again after a pass that left work for later (or failed).
const RETRY: Duration = Duration::from_secs(2);
/// Ceiling for the retry delay after repeated failures.
const MAX_RETRY: Duration = Duration::from_secs(60);

/// What a reconciliation found changed on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VaultChange {
    /// Notes added or modified (including moved or renamed with their id).
    pub changed: Vec<Uuid>,
    /// Notes that no longer exist under their id.
    pub removed: Vec<Uuid>,
    /// Folders were added, renamed or removed.
    pub folders: bool,
    /// Files other than notes (such as attachments) were added, changed or removed.
    pub files: bool,
}

impl VaultChange {
    #[must_use]
    pub fn from_report(report: &ReindexReport, files: bool) -> Self {
        Self {
            changed: report.changed.clone(),
            removed: report.removed.clone(),
            folders: report.folders_changed,
            files,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty() && !self.folders && !self.files
    }
}

/// Watches a vault until dropped.
pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for VaultWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultWatcher").finish_non_exhaustive()
    }
}

impl Drop for VaultWatcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    /// A Markdown file may have changed.
    Notes,
    /// Something else may have changed: a folder or another file.
    Other,
    /// The OS lost track; assume anything changed.
    Rescan,
}

impl Signal {
    fn files(self) -> bool {
        !matches!(self, Self::Notes)
    }
}

fn watch_error(root: &Path, e: &notify::Error) -> Error {
    Error::io(root, std::io::Error::other(e.to_string()))
}

/// What an event means for the vault, if anything.
fn classify(root: &Path, event: &notify::Event) -> Option<Signal> {
    if event.need_rescan() {
        return Some(Signal::Rescan);
    }
    if matches!(event.kind, EventKind::Access(_)) {
        return None;
    }
    let mut signal = None;
    for path in &event.paths {
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            // The vault folder itself changed (moved, deleted, permissions).
            return Some(Signal::Rescan);
        }
        let hidden = rel
            .components()
            .any(|c| c.as_os_str().to_str().is_none_or(|s| s.starts_with('.')));
        if hidden {
            continue;
        }
        let markdown = rel
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"));
        signal = Some(if markdown && signal != Some(Signal::Other) {
            Signal::Notes
        } else {
            Signal::Other
        });
    }
    signal
}

/// Starts watching `ws`'s vault. After every burst of relevant changes the index
/// is reconciled, and `on_change` is called if anything changed. Must be called
/// from within a Tokio runtime.
///
/// # Errors
/// [`Error::Io`] if the OS watcher cannot be set up (e.g. the inotify watch limit
/// is reached). The app still works then; it just notices outside changes later.
pub fn watch<F>(ws: Workspace, on_change: F) -> Result<VaultWatcher>
where
    F: Fn(VaultChange) + Send + Sync + 'static,
{
    let root: PathBuf = ws.vault().root().to_owned();
    let (tx, rx) = unbounded_channel();
    let filter_root = root.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let signal = match res {
            Ok(event) => classify(&filter_root, &event),
            Err(e) => {
                tracing::warn!(error = %e, "file watcher error; rescanning");
                Some(Signal::Rescan)
            }
        };
        if let Some(s) = signal {
            // Fails only once the task is gone, i.e. while shutting down.
            let _ = tx.send(s);
        }
    })
    .map_err(|e| watch_error(&root, &e))?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| watch_error(&root, &e))?;
    let task = tokio::spawn(run(ws, rx, on_change));
    Ok(VaultWatcher {
        _watcher: watcher,
        task,
    })
}

async fn run<F>(ws: Workspace, mut rx: UnboundedReceiver<Signal>, on_change: F)
where
    F: Fn(VaultChange),
{
    let mut retry: Option<Duration> = None;
    let mut failures: u32 = 0;
    loop {
        // Wait for the next event, or for the retry timer.
        let first = match retry {
            None => match rx.recv().await {
                Some(s) => Some(s),
                None => return,
            },
            Some(delay) => match timeout(delay, rx.recv()).await {
                Ok(Some(s)) => Some(s),
                Ok(None) => return,
                Err(_) => None,
            },
        };
        let mut files = first.is_some_and(Signal::files);
        // Let the burst finish, but not forever.
        let deadline = Instant::now() + MAX_DELAY;
        loop {
            let wait = QUIET.min(deadline.saturating_duration_since(Instant::now()));
            if wait.is_zero() {
                break;
            }
            match timeout(wait, rx.recv()).await {
                Ok(Some(s)) => files |= s.files(),
                Ok(None) => return,
                Err(_) => break,
            }
        }
        match ws.reindex().await {
            Ok(report) => {
                failures = 0;
                retry = (!report.deferred.is_empty()).then_some(RETRY);
                let change = VaultChange::from_report(&report, files);
                if !change.is_empty() {
                    on_change(change);
                }
            }
            Err(e) => {
                failures = failures.saturating_add(1);
                let delay = RETRY
                    .saturating_mul(2u32.saturating_pow(failures.min(6) - 1))
                    .min(MAX_RETRY);
                tracing::warn!(error = %e, ?delay, "reconciling after a file change failed");
                retry = Some(delay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RenameMode};

    fn event(kind: EventKind, paths: &[&str]) -> notify::Event {
        let mut e = notify::Event::new(kind);
        for p in paths {
            e = e.add_path(PathBuf::from(p));
        }
        e
    }

    #[test]
    fn classifies_events() {
        let root = Path::new("/v");
        let modify = EventKind::Modify(ModifyKind::Any);
        assert_eq!(
            classify(root, &event(modify, &["/v/a.md"])),
            Some(Signal::Notes)
        );
        assert_eq!(
            classify(root, &event(modify, &["/v/Sub/b.MD"])),
            Some(Signal::Notes)
        );
        assert_eq!(
            classify(root, &event(modify, &["/v/attachments/x.png"])),
            Some(Signal::Other)
        );
        assert_eq!(
            classify(
                root,
                &event(
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                    &["/v/a.md", "/v/Folder"]
                )
            ),
            Some(Signal::Other)
        );
        assert_eq!(
            classify(
                root,
                &event(EventKind::Create(CreateKind::Folder), &["/v/New"])
            ),
            Some(Signal::Other)
        );
        // Ignored: the index and recovery area, hidden files and folders, outside paths.
        for p in [
            "/v/.notenest/index.db",
            "/v/.notenest/recovery/drafts/x.json",
            "/v/.git/HEAD",
            "/v/Sub/.nn-1.tmp",
            "/v/.a.md.swp",
            "/elsewhere/a.md",
        ] {
            assert_eq!(classify(root, &event(modify, &[p])), None, "{p}");
        }
        assert_eq!(
            classify(
                root,
                &event(
                    EventKind::Access(notify::event::AccessKind::Any),
                    &["/v/a.md"]
                )
            ),
            None
        );
        assert_eq!(
            classify(root, &event(modify, &["/v"])),
            Some(Signal::Rescan)
        );
        let rescan = notify::Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert_eq!(classify(root, &rescan), Some(Signal::Rescan));
    }

    #[test]
    fn an_empty_report_is_no_change() {
        let report = ReindexReport::default();
        assert!(VaultChange::from_report(&report, false).is_empty());
        assert!(!VaultChange::from_report(&report, true).is_empty());
    }

    // ------------------------------------------------------------ real watcher

    use crate::services::SaveMode;
    use std::fs;
    use tokio::sync::mpsc::UnboundedReceiver as Rx;

    async fn watched() -> (tempfile::TempDir, Workspace, VaultWatcher, Rx<VaultChange>) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "alpha").unwrap();
        let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
        let (tx, rx) = unbounded_channel();
        let watcher = watch(ws.clone(), move |c| {
            let _ = tx.send(c);
        })
        .unwrap();
        (dir, ws, watcher, rx)
    }

    async fn next(rx: &mut Rx<VaultChange>) -> VaultChange {
        timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("no change reported in time")
            .unwrap()
    }

    /// No change is reported within a generous window.
    async fn quiet(rx: &mut Rx<VaultChange>) {
        // A closed channel (the watcher is gone) counts as quiet too.
        let waited = timeout(QUIET * 4 + MAX_DELAY, rx.recv()).await;
        assert!(
            !matches!(waited, Ok(Some(_))),
            "unexpected change: {waited:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reports_notes_changed_added_and_removed_outside_the_app() {
        let (dir, ws, _w, mut rx) = watched().await;
        let a = ws.list_notes().await.unwrap()[0].id;

        fs::write(dir.path().join("a.md"), "alpha, edited elsewhere").unwrap();
        let change = next(&mut rx).await;
        assert_eq!(change.changed, vec![a]);
        assert!(change.removed.is_empty());
        let indexed = ws.database().get_note(a).await.unwrap().unwrap();
        assert_eq!(
            indexed.content_hash,
            crate::hash::content_hash(b"alpha, edited elsewhere")
        );

        fs::write(dir.path().join("b.md"), "beta").unwrap();
        let change = next(&mut rx).await;
        assert_eq!(change.changed.len(), 1);

        fs::remove_file(dir.path().join("a.md")).unwrap();
        let change = next(&mut rx).await;
        assert_eq!(change.removed, vec![a]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn picks_up_folder_moves_and_attachments() {
        let (dir, ws, _w, mut rx) = watched().await;
        fs::create_dir(dir.path().join("Projects")).unwrap();
        fs::write(dir.path().join("Projects/p.md"), "p").unwrap();
        let change = next(&mut rx).await;
        assert!(change.folders);

        fs::rename(dir.path().join("Projects"), dir.path().join("Archive")).unwrap();
        let change = next(&mut rx).await;
        assert!(change.folders);
        let paths: Vec<_> = ws
            .list_notes()
            .await
            .unwrap()
            .into_iter()
            .map(|n| n.path)
            .collect();
        assert!(paths.contains(&"Archive/p.md".to_owned()), "{paths:?}");

        fs::write(dir.path().join("Archive/pic.png"), b"png").unwrap();
        let change = next(&mut rx).await;
        assert!(change.files && change.changed.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_apps_own_writes_report_nothing() {
        let (_dir, ws, _w, mut rx) = watched().await;
        let note = ws
            .read_note(ws.list_notes().await.unwrap()[0].id)
            .await
            .unwrap();
        ws.save_note(
            note.record.id,
            "alpha, saved by NoteNest",
            SaveMode::Expect(&note.record.content_hash),
        )
        .await
        .unwrap();
        ws.stash_draft(note.record.id, "draft", "hash")
            .await
            .unwrap();
        quiet(&mut rx).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stops_when_dropped() {
        let (dir, _ws, w, mut rx) = watched().await;
        drop(w);
        fs::write(dir.path().join("a.md"), "changed").unwrap();
        quiet(&mut rx).await;
    }
}
