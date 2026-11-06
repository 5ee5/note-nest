//! Application services: the logic between commands and storage.

pub mod attachments;
pub mod recovery;
pub mod reindex;
pub mod workspace;

pub use attachments::Attachment;
pub use recovery::Draft;
pub use reindex::{reindex, ReindexReport};
pub use workspace::{NoteContent, OpenReport, SaveMode, Workspace};
