//! Read-side operations of the knowledge index (search, tags, links, graph).

use uuid::Uuid;

use super::Workspace;
use crate::models::{BrokenLink, Graph, NoteEntry, NoteLinks, NoteRef, SearchHit, TagCount};
use crate::Result;

/// Related notes shown next to the links of a note.
const RELATED_LIMIT: usize = 8;

impl Workspace {
    /// All notes with their tags.
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn list_entries(&self) -> Result<Vec<NoteEntry>> {
        let mut tags = self.db.note_tag_map().await?;
        Ok(self
            .db
            .list_notes()
            .await?
            .into_iter()
            .map(|record| {
                let (tags, inline_tags) = tags.remove(&record.id).unwrap_or_default();
                NoteEntry {
                    record,
                    tags,
                    inline_tags,
                }
            })
            .collect())
    }

    /// Full-text search; see [`crate::database::Database::search`].
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.db.search(query, limit).await
    }

    /// Every tag with its note count (nested tags roll up into their parents).
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn list_tags(&self) -> Result<Vec<TagCount>> {
        self.db.list_tags().await
    }

    /// Backlinks, outgoing links and related notes of a note.
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn note_links(&self, id: Uuid) -> Result<NoteLinks> {
        Ok(NoteLinks {
            backlinks: self.db.backlinks(id).await?,
            outgoing: self.db.outgoing(id).await?,
            related: self.db.related(id, RELATED_LIMIT).await?,
        })
    }

    /// The link graph: the whole vault, or notes within `depth` links of `center`.
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn graph(&self, center: Option<Uuid>, depth: usize) -> Result<Graph> {
        self.db.graph(center, depth).await
    }

    /// Notes that are not linked to or from any other note.
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn orphans(&self) -> Result<Vec<NoteRef>> {
        self.db.orphans().await
    }

    /// Links that point at no note.
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn broken_links(&self) -> Result<Vec<BrokenLink>> {
        self.db.broken_links().await
    }

    /// Which note a link target written in note `source` refers to.
    ///
    /// # Errors
    /// [`crate::Error::Database`] on failure.
    pub async fn resolve_link(&self, source: Uuid, target: &str) -> Result<Option<NoteRef>> {
        self.db.resolve_link(source, target).await
    }
}
