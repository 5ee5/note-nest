//! All vault filesystem access lives here.

pub mod atomic;
pub mod path;
pub mod trash;
pub mod vault;

pub use path::RelPath;
pub use vault::Vault;

/// Name of the hidden directory inside a vault that holds NoteNest's own data
/// (index database, trash, recovery drafts). It is never treated as notes.
pub const META_DIR: &str = ".notenest";
