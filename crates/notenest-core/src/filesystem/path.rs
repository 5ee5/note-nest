//! Validation of user-supplied vault-relative paths.

use std::fmt;

use crate::{Error, Result};

const MAX_COMPONENT_BYTES: usize = 255;
const FORBIDDEN_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\\', '/'];
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A validated, normalized path relative to the vault root, using `/` separators.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelPath(String);

impl RelPath {
    /// Validates `input` and normalizes it. `\` is rejected rather than converted,
    /// because it is a valid filename character on Unix but a separator on Windows.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] for empty, absolute, traversal (`..`, `.`), hidden
    /// (leading `.`), reserved, over-long, or otherwise unsafe components.
    pub fn new(input: &str) -> Result<Self> {
        if input.is_empty() {
            return Err(invalid(input, "path is empty"));
        }
        if input.starts_with('/') {
            return Err(invalid(input, "path must be relative"));
        }
        let mut parts = Vec::new();
        for comp in input.split('/') {
            validate_component(input, comp)?;
            parts.push(comp);
        }
        Ok(Self(parts.join("/")))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// The final component.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The containing folder, or `None` for a top-level entry.
    #[must_use]
    pub fn parent(&self) -> Option<RelPath> {
        self.0.rsplit_once('/').map(|(p, _)| Self(p.to_owned()))
    }

    /// Appends a validated child component.
    ///
    /// # Errors
    /// [`Error::InvalidPath`] if `name` is not a valid single component.
    pub fn join(&self, name: &str) -> Result<Self> {
        validate_component(name, name)?;
        Ok(Self(format!("{}/{name}", self.0)))
    }

    /// True if `other` is this path or lies beneath it.
    #[must_use]
    pub fn contains(&self, other: &RelPath) -> bool {
        other.0 == self.0 || other.0.starts_with(&format!("{}/", self.0))
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn invalid(input: &str, why: &str) -> Error {
    Error::InvalidPath(format!("{input:?}: {why}"))
}

fn validate_component(input: &str, comp: &str) -> Result<()> {
    if comp.is_empty() {
        return Err(invalid(input, "empty path component"));
    }
    if comp == "." || comp == ".." {
        return Err(invalid(input, "'.' and '..' are not allowed"));
    }
    if comp.starts_with('.') {
        return Err(invalid(input, "names starting with '.' are reserved"));
    }
    if comp.len() > MAX_COMPONENT_BYTES {
        return Err(invalid(input, "name is too long"));
    }
    if comp.ends_with(' ') || comp.ends_with('.') {
        return Err(invalid(input, "name must not end with a space or dot"));
    }
    if comp
        .chars()
        .any(|c| c.is_control() || FORBIDDEN_CHARS.contains(&c))
    {
        return Err(invalid(input, "name contains forbidden characters"));
    }
    let stem = comp.split('.').next().unwrap_or(comp).to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        return Err(invalid(input, "name is reserved on Windows"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_paths() {
        let p = RelPath::new("Programming/Rust notes.md").unwrap();
        assert_eq!(p.as_str(), "Programming/Rust notes.md");
        assert_eq!(p.file_name(), "Rust notes.md");
        assert_eq!(p.parent().unwrap().as_str(), "Programming");
        assert!(RelPath::new("a.md").unwrap().parent().is_none());
        assert!(RelPath::new("日本語/メモ.md").is_ok());
    }

    #[test]
    fn rejects_unsafe_paths() {
        for bad in [
            "",
            "/etc/passwd",
            "../x.md",
            "a/../b.md",
            "a/./b.md",
            "a//b.md",
            "a/",
            ".notenest/index.db",
            "a/.hidden.md",
            "a\\b.md",
            "C:/x.md",
            "bad|name.md",
            "trailing.",
            "trailing ",
            "con.md",
            "LPT1",
            "nul\0byte.md",
        ] {
            assert!(RelPath::new(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(RelPath::new(&"a".repeat(256)).is_err());
    }

    #[test]
    fn join_and_contains() {
        let dir = RelPath::new("Projects").unwrap();
        let file = dir.join("x.md").unwrap();
        assert_eq!(file.as_str(), "Projects/x.md");
        assert!(dir.contains(&file));
        assert!(dir.contains(&dir));
        assert!(!dir.contains(&RelPath::new("Projects2/x.md").unwrap()));
        assert!(dir.join("../x").is_err());
        assert!(dir.join("a/b").is_err());
    }
}
