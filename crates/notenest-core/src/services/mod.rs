//! Application services: the logic between commands and storage.

pub mod reindex;
pub mod workspace;

pub use reindex::{reindex, ReindexReport};
pub use workspace::{NoteContent, OpenReport, SaveMode, Workspace};
