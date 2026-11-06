//! Error type shared by all core operations.
//!
//! Every fallible operation returns [`Result`]; nothing fails silently.

use std::path::PathBuf;

use serde::{ser::SerializeStruct, Serialize, Serializer};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O operation failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A user-supplied path was rejected (traversal, reserved name, symlink escape, ...).
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// The target already exists and would be overwritten.
    #[error("already exists: {0}")]
    AlreadyExists(String),

    /// The requested item does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The file changed on disk since the caller last read it.
    #[error("{path} changed on disk since it was loaded")]
    Conflict {
        path: String,
        /// Hash the caller expected.
        expected: String,
        /// Hash currently on disk.
        actual: String,
    },

    /// Stored data could not be parsed.
    #[error("corrupt data in {context}: {message}")]
    Corrupt { context: String, message: String },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Stable machine-readable code, used by the UI to branch on error kinds.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::InvalidPath(_) => "invalid_path",
            Self::AlreadyExists(_) => "already_exists",
            Self::NotFound(_) => "not_found",
            Self::Conflict { .. } => "conflict",
            Self::Corrupt { .. } => "corrupt",
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("Error", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_and_message() {
        let json = serde_json::to_value(Error::NotFound("a.md".into())).unwrap();
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "not found: a.md");
    }
}
