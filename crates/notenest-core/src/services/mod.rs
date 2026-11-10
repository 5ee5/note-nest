//! Application services: the logic between commands and storage.

pub mod attachments;
#[cfg(test)]
mod invariant;
pub mod knowledge;
pub mod link_update;
pub mod recovery;
pub mod reindex;
pub mod watch;
pub mod workspace;

pub use attachments::Attachment;
pub use link_update::{AffectedNote, LinkUpdateReport, RenameOutcome, SkippedNote};
pub use recovery::Draft;
pub use reindex::{reindex, reindex_cached, ReindexReport, ScanCache};
pub use watch::{watch, VaultChange, VaultWatcher};
pub use workspace::{NoteContent, OpenReport, SaveMode, Workspace};
