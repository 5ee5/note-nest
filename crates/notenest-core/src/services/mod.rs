//! Application services: the logic between commands and storage.

pub mod recovery;
pub mod reindex;
pub mod workspace;

pub use recovery::Draft;
pub use reindex::{reindex, ReindexReport};
pub use workspace::{NoteContent, OpenReport, SaveMode, Workspace};
