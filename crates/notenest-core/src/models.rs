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

/// A note together with everything derived from its content, ready to be indexed.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub record: NoteRecord,
    pub extracted: crate::extract::Extracted,
}

/// A highlighted fragment of text: `matched` parts are what the search found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnippetPart {
    pub text: String,
    pub matched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    pub id: Uuid,
    pub title: String,
    pub path: String,
    pub archived: bool,
    pub title_parts: Vec<SnippetPart>,
    pub snippet: Vec<SnippetPart>,
}

/// A tag with the number of notes using it (or one of its nested tags).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagCount {
    pub name: String,
    pub count: usize,
}

/// Enough to show and open a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteRef {
    pub id: Uuid,
    pub title: String,
    pub path: String,
}

/// A link in another note that points at this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Backlink {
    pub source: NoteRef,
    pub line: usize,
    pub context: String,
    pub target_text: String,
    pub display: Option<String>,
}

/// A link written in a note; `target` is `None` when it points at nothing (broken).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutgoingLink {
    pub target_text: String,
    pub display: Option<String>,
    pub line: usize,
    pub context: String,
    pub target: Option<NoteRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrokenLink {
    pub source: NoteRef,
    pub target_text: String,
    pub line: usize,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    pub id: Uuid,
    pub title: String,
    pub path: String,
    pub archived: bool,
    /// Number of distinct notes this one is linked to or from.
    pub degree: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    /// Directed `[from, to]` pairs between `nodes`, without duplicates or self loops.
    pub edges: Vec<(Uuid, Uuid)>,
    /// Notes in the whole vault (a local graph shows only some of them).
    pub total_notes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedNote {
    pub note: NoteRef,
    pub shared_tags: usize,
    /// Linked from or to the note.
    pub linked: bool,
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
