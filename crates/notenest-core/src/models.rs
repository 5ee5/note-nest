//! Domain models. These must not depend on Tauri or the UI.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Fixed namespace for deriving ids of notes that carry no `id` in frontmatter.
const NOTE_ID_NAMESPACE: Uuid = Uuid::from_u128(0x6e6f_7465_6e65_7374_8000_0000_0000_0001);

/// Stable id for a note without an `id` in its frontmatter: derived from its path,
/// so rebuilding the index yields the same id without modifying the user's file.
#[must_use]
pub fn note_id_for_path(path: &str) -> Uuid {
    Uuid::new_v5(&NOTE_ID_NAMESPACE, path.as_bytes())
}

/// A note as stored in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRecord {
    pub id: Uuid,
    /// Vault-relative path, e.g. `Programming/Rust.md`.
    pub path: String,
    /// File name without the `.md` extension.
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub content_hash: String,
    pub pinned: bool,
    pub archived: bool,
    pub last_opened_at: Option<DateTime<Utc>>,
}

/// Title of a note: its file name without the `.md` extension.
#[must_use]
pub fn title_from_path(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md")
        .or_else(|| name.strip_suffix(".MD"))
        .unwrap_or(name)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_ids_are_stable_and_path_specific() {
        assert_eq!(note_id_for_path("a.md"), note_id_for_path("a.md"));
        assert_ne!(note_id_for_path("a.md"), note_id_for_path("b.md"));
    }

    #[test]
    fn titles_strip_the_extension_only() {
        assert_eq!(title_from_path("Programming/Rust.md"), "Rust");
        assert_eq!(title_from_path("v1.2 notes.md"), "v1.2 notes");
        assert_eq!(title_from_path("plain"), "plain");
    }
}
