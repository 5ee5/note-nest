//! Link resolution and the queries built on it: backlinks, outgoing links, the graph,
//! orphaned and broken links, related notes.
//!
//! Links are stored as target *keys* and matched against `note_keys` here, so the
//! answer is always current. A key can match several notes; [`Resolver`] picks one.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use sqlx::Row;
use uuid::Uuid;

use super::Database;
use crate::models::{Backlink, BrokenLink, Graph, GraphNode, NoteRef, OutgoingLink, RelatedNote};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Path,
    Title,
    Alias,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: Uuid,
    pub title: String,
    pub path: String,
    kind: Kind,
}

impl Candidate {
    #[must_use]
    pub fn note_ref(&self) -> NoteRef {
        NoteRef {
            id: self.id,
            title: self.title.clone(),
            path: self.path.clone(),
        }
    }
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(f, _)| f)
}

/// Matches link keys to notes. Rule: a path-qualified match beats a title match, which
/// beats an alias match; among equals prefer a note in the source's own folder, then the
/// shortest path, then alphabetical order.
#[derive(Debug, Default)]
pub struct Resolver {
    by_key: HashMap<String, Vec<Candidate>>,
}

impl Resolver {
    #[must_use]
    pub fn resolve(&self, source_path: &str, key: &str) -> Option<&Candidate> {
        let source_folder = folder_of(source_path);
        self.by_key.get(key)?.iter().min_by(|a, b| {
            let rank = |c: &Candidate| {
                (
                    c.kind as u8,
                    u8::from(folder_of(&c.path) != source_folder),
                    c.path.len(),
                )
            };
            rank(a).cmp(&rank(b)).then_with(|| a.path.cmp(&b.path))
        })
    }
}

impl Resolver {
    /// A copy of this resolver as it would be after the given notes got new paths and
    /// titles (`id -> (new path, new title)`): their path and title keys move, their alias
    /// keys stay. Used to preview what a rename or move would do to existing links.
    #[must_use]
    pub fn simulate(&self, moves: &HashMap<Uuid, (String, String)>) -> Resolver {
        let mut by_key: HashMap<String, Vec<Candidate>> = HashMap::new();
        for (key, list) in &self.by_key {
            for c in list {
                let mut c = c.clone();
                if let Some((path, title)) = moves.get(&c.id) {
                    if c.kind != Kind::Alias {
                        continue; // path and title keys are re-added below under the new names
                    }
                    // Alias keys stay, but now point at the note's new location.
                    c.path.clone_from(path);
                    c.title.clone_from(title);
                }
                by_key.entry(key.clone()).or_default().push(c);
            }
        }
        for (id, (path, title)) in moves {
            let pk = super::knowledge::path_key(path);
            let kind = if pk.contains('/') {
                Kind::Path
            } else {
                Kind::Title
            };
            for (key, kind) in [(pk, kind), (title.to_lowercase(), Kind::Title)] {
                let list = by_key.entry(key).or_default();
                if !list.iter().any(|c| c.id == *id) {
                    list.push(Candidate {
                        id: *id,
                        title: title.clone(),
                        path: path.clone(),
                        kind,
                    });
                }
            }
        }
        Resolver { by_key }
    }
}

/// A link (anywhere in the vault) that resolves to one of a set of notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedLink {
    pub source_path: String,
    pub target_id: Uuid,
    pub target_path: String,
    /// The target exactly as written, e.g. `Folder/Note#Heading`.
    pub target_text: String,
    pub line: usize,
}

fn parse_id(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| Error::Corrupt {
        context: "note id".into(),
        message: e.to_string(),
    })
}

/// Most values bound to one `IN (...)` list; SQLite caps the number of variables.
const IN_CHUNK: usize = 500;

fn in_list(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// `(link id, source id, source path, line, target key)` for the links matching `filter`,
/// which is an `IN` condition on `l.source_note_id` or `l.target_key` bound to `values`.
async fn fetch_link_keys(
    db: &Database,
    filter: &str,
    values: &[String],
) -> Result<Vec<(i64, Uuid, String, i64, String)>> {
    let sql = format!(
        "SELECT l.id, l.source_note_id, s.path, l.line, l.target_key \
         FROM note_links l JOIN notes s ON s.id = l.source_note_id WHERE {filter}"
    );
    let q = sqlx::query(&sql);
    values
        .iter()
        .fold(q, |q, v| q.bind(v))
        .fetch_all(db.pool())
        .await?
        .iter()
        .map(|r| {
            Ok((
                r.try_get("id")?,
                parse_id(&r.try_get::<String, _>("source_note_id")?)?,
                r.try_get("path")?,
                r.try_get("line")?,
                r.try_get("target_key")?,
            ))
        })
        .collect()
}

struct NoteRow {
    id: Uuid,
    title: String,
    path: String,
    archived: bool,
}

struct LinkRow {
    source: Uuid,
    source_path: String,
    key: String,
    target_text: String,
    line: usize,
    context: String,
}

impl Database {
    /// Every note key in memory, for resolving many links quickly. Built once and reused
    /// until the index next changes.
    pub async fn resolver(&self) -> Result<Arc<Resolver>> {
        // Read the generation before querying: a write that lands meanwhile makes the
        // stored copy stale, and the next call then rebuilds it.
        let generation = self.generation();
        if let Some(cached) = self.cached_resolver(generation) {
            return Ok(cached);
        }
        let resolver = Arc::new(self.load_resolver().await?);
        self.store_resolver(generation, Arc::clone(&resolver));
        Ok(resolver)
    }

    async fn load_resolver(&self) -> Result<Resolver> {
        let rows = sqlx::query(
            "SELECT k.key, k.kind, n.id, n.title, n.path FROM note_keys k JOIN notes n ON n.id = k.note_id",
        )
        .fetch_all(self.pool())
        .await?;
        let mut by_key: HashMap<String, Vec<Candidate>> = HashMap::new();
        for r in rows {
            let kind = match r.try_get::<String, _>("kind")?.as_str() {
                "path" => Kind::Path,
                "alias" => Kind::Alias,
                _ => Kind::Title,
            };
            by_key
                .entry(r.try_get("key")?)
                .or_default()
                .push(Candidate {
                    id: parse_id(&r.try_get::<String, _>("id")?)?,
                    title: r.try_get("title")?,
                    path: r.try_get("path")?,
                    kind,
                });
        }
        Ok(Resolver { by_key })
    }

    /// Which note a link written as `target` in `source_id` points at, if any.
    pub async fn resolve_link(&self, source_id: Uuid, target: &str) -> Result<Option<NoteRef>> {
        let path: Option<String> = sqlx::query_scalar("SELECT path FROM notes WHERE id = ?1")
            .bind(source_id.to_string())
            .fetch_optional(self.pool())
            .await?;
        let Some(path) = path else { return Ok(None) };
        let key = crate::extract::link_key(target);
        Ok(self
            .resolver()
            .await?
            .resolve(&path, &key)
            .map(Candidate::note_ref))
    }

    async fn all_notes(&self) -> Result<Vec<NoteRow>> {
        sqlx::query("SELECT id, title, path, archived FROM notes ORDER BY path")
            .fetch_all(self.pool())
            .await?
            .iter()
            .map(|r| {
                Ok(NoteRow {
                    id: parse_id(&r.try_get::<String, _>("id")?)?,
                    title: r.try_get("title")?,
                    path: r.try_get("path")?,
                    archived: r.try_get::<i64, _>("archived")? != 0,
                })
            })
            .collect()
    }

    /// Just what resolving a link needs (source, its path, the target key), in the same
    /// order as [`Database::all_links`]. Skips the text columns, which dominate the cost of
    /// loading a large vault's links when only the graph shape matters.
    async fn link_keys(&self) -> Result<Vec<(Uuid, String, String)>> {
        sqlx::query(
            "SELECT l.source_note_id, s.path, l.target_key \
             FROM note_links l JOIN notes s ON s.id = l.source_note_id ORDER BY s.path, l.line, l.id",
        )
        .fetch_all(self.pool())
        .await?
        .iter()
        .map(|r| {
            Ok((
                parse_id(&r.try_get::<String, _>("source_note_id")?)?,
                r.try_get("path")?,
                r.try_get("target_key")?,
            ))
        })
        .collect()
    }

    async fn all_links(&self) -> Result<Vec<LinkRow>> {
        sqlx::query(
            "SELECT l.source_note_id, s.path, l.target_key, l.target_text, l.display, l.line, l.context \
             FROM note_links l JOIN notes s ON s.id = l.source_note_id ORDER BY s.path, l.line, l.id",
        )
        .fetch_all(self.pool())
        .await?
        .iter()
        .map(|r| {
            Ok(LinkRow {
                source: parse_id(&r.try_get::<String, _>("source_note_id")?)?,
                source_path: r.try_get("path")?,
                key: r.try_get("target_key")?,
                target_text: r.try_get("target_text")?,
                line: usize::try_from(r.try_get::<i64, _>("line")?).unwrap_or(0),
                context: r.try_get("context")?,
            })
        })
        .collect()
    }

    /// Every link whose target is one of `targets` (including links from a target to itself
    /// or to another target), as currently resolved.
    pub async fn links_to(
        &self,
        targets: &std::collections::HashSet<Uuid>,
    ) -> Result<Vec<CapturedLink>> {
        let resolver = self.resolver().await?;
        Ok(self
            .all_links()
            .await?
            .into_iter()
            .filter_map(|l| {
                let c = resolver.resolve(&l.source_path, &l.key)?;
                targets.contains(&c.id).then(|| CapturedLink {
                    source_path: l.source_path,
                    target_id: c.id,
                    target_path: c.path.clone(),
                    target_text: l.target_text,
                    line: l.line,
                })
            })
            .collect())
    }

    /// Links in other notes that resolve to `id`, in path and line order.
    pub async fn backlinks(&self, id: Uuid) -> Result<Vec<Backlink>> {
        let resolver = self.resolver().await?;
        self.backlinks_with(id, &resolver).await
    }

    async fn backlinks_with(&self, id: Uuid, resolver: &Resolver) -> Result<Vec<Backlink>> {
        let rows = sqlx::query(
            "SELECT l.source_note_id, s.title, s.path, l.target_key, l.target_text, l.display, l.line, l.context \
             FROM note_links l JOIN notes s ON s.id = l.source_note_id \
             WHERE l.target_key IN (SELECT key FROM note_keys WHERE note_id = ?1) \
               AND l.source_note_id != ?1 \
             ORDER BY s.path, l.line, l.id",
        )
        .bind(id.to_string())
        .fetch_all(self.pool())
        .await?;
        let mut out = Vec::new();
        for r in rows {
            let path: String = r.try_get("path")?;
            let key: String = r.try_get("target_key")?;
            if resolver.resolve(&path, &key).map(|c| c.id) != Some(id) {
                continue; // another note is the better match for this link
            }
            out.push(Backlink {
                source: NoteRef {
                    id: parse_id(&r.try_get::<String, _>("source_note_id")?)?,
                    title: r.try_get("title")?,
                    path,
                },
                line: usize::try_from(r.try_get::<i64, _>("line")?).unwrap_or(0),
                context: r.try_get("context")?,
                target_text: r.try_get("target_text")?,
                display: r.try_get("display")?,
            });
        }
        Ok(out)
    }

    /// Links written in note `id`, each resolved (or marked broken).
    pub async fn outgoing(&self, id: Uuid) -> Result<Vec<OutgoingLink>> {
        let resolver = self.resolver().await?;
        self.outgoing_with(id, &resolver).await
    }

    async fn outgoing_with(&self, id: Uuid, resolver: &Resolver) -> Result<Vec<OutgoingLink>> {
        let rows = sqlx::query(
            "SELECT s.path, l.target_key, l.target_text, l.display, l.line, l.context \
             FROM note_links l JOIN notes s ON s.id = l.source_note_id \
             WHERE l.source_note_id = ?1 ORDER BY l.line, l.id",
        )
        .bind(id.to_string())
        .fetch_all(self.pool())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let path: String = r.try_get("path")?;
            let key: String = r.try_get("target_key")?;
            out.push(OutgoingLink {
                target: resolver.resolve(&path, &key).map(Candidate::note_ref),
                target_text: r.try_get("target_text")?,
                display: r.try_get("display")?,
                line: usize::try_from(r.try_get::<i64, _>("line")?).unwrap_or(0),
                context: r.try_get("context")?,
            });
        }
        Ok(out)
    }

    /// Backlinks, outgoing links and related notes of a note, computed together from one
    /// resolver and without loading every link in the vault.
    pub async fn note_links(
        &self,
        id: Uuid,
        related_limit: usize,
    ) -> Result<crate::models::NoteLinks> {
        let resolver = self.resolver().await?;
        let backlinks = self.backlinks_with(id, &resolver).await?;
        let outgoing = self.outgoing_with(id, &resolver).await?;
        let linked: HashSet<Uuid> = backlinks
            .iter()
            .map(|b| b.source.id)
            .chain(
                outgoing
                    .iter()
                    .filter_map(|o| o.target.as_ref().map(|t| t.id)),
            )
            .filter(|n| *n != id)
            .collect();
        let related = self.related_with(id, related_limit, &linked).await?;
        Ok(crate::models::NoteLinks {
            backlinks,
            outgoing,
            related,
        })
    }

    /// Links that point at no note.
    pub async fn broken_links(&self) -> Result<Vec<BrokenLink>> {
        let resolver = self.resolver().await?;
        let notes: HashMap<Uuid, NoteRow> = self
            .all_notes()
            .await?
            .into_iter()
            .map(|n| (n.id, n))
            .collect();
        Ok(self
            .all_links()
            .await?
            .into_iter()
            .filter(|l| resolver.resolve(&l.source_path, &l.key).is_none())
            .filter_map(|l| {
                let n = notes.get(&l.source)?;
                Some(BrokenLink {
                    source: NoteRef {
                        id: n.id,
                        title: n.title.clone(),
                        path: n.path.clone(),
                    },
                    target_text: l.target_text,
                    line: l.line,
                    context: l.context,
                })
            })
            .collect())
    }

    /// Resolved links as distinct directed `(from, to)` pairs without self loops.
    async fn edges(&self) -> Result<Vec<(Uuid, Uuid)>> {
        let resolver = self.resolver().await?;
        let mut seen = HashSet::new();
        let mut edges = Vec::new();
        for (source, source_path, key) in self.link_keys().await? {
            if let Some(c) = resolver.resolve(&source_path, &key) {
                if c.id != source && seen.insert((source, c.id)) {
                    edges.push((source, c.id));
                }
            }
        }
        Ok(edges)
    }

    /// The link graph. With `center`, only notes within `depth` links of it (in either
    /// direction); otherwise every note.
    pub async fn graph(&self, center: Option<Uuid>, depth: usize) -> Result<Graph> {
        match center {
            Some(c) => self.local_graph(c, depth).await,
            None => self.global_graph().await,
        }
    }

    async fn global_graph(&self) -> Result<Graph> {
        let notes = self.all_notes().await?;
        let total_notes = notes.len();
        let edges = self.edges().await?;
        let mut neighbours: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
        for &(a, b) in &edges {
            neighbours.entry(a).or_default().insert(b);
            neighbours.entry(b).or_default().insert(a);
        }
        let nodes = notes
            .into_iter()
            .map(|n| GraphNode {
                degree: neighbours.get(&n.id).map_or(0, HashSet::len),
                id: n.id,
                title: n.title,
                path: n.path,
                archived: n.archived,
            })
            .collect();
        Ok(Graph {
            nodes,
            edges,
            total_notes,
        })
    }

    /// The graph around `center`, built level by level from queries scoped to the notes
    /// reached so far, so its cost follows the size of the neighbourhood, not the vault.
    /// The result equals the global graph restricted to the notes within `depth` links
    /// (a node's `degree` still counts all of its neighbours, kept or not).
    async fn local_graph(&self, center: Uuid, depth: usize) -> Result<Graph> {
        let resolver = self.resolver().await?;
        let total_notes = usize::try_from(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notes")
                .fetch_one(self.pool())
                .await?,
        )
        .unwrap_or(0);

        let mut neighbours: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
        // Link rows seen so far, keyed by link id so one found twice counts once.
        let mut links: HashMap<i64, (String, i64, Uuid, Uuid)> = HashMap::new(); // path, line, source, target
        let mut seen = HashSet::from([center]);
        let mut frontier = vec![center];
        for level in 0..=depth {
            // Every link in or out of the frontier, resolved.
            let ids: Vec<String> = frontier.iter().map(Uuid::to_string).collect();
            let in_frontier: HashSet<Uuid> = frontier.iter().copied().collect();
            let mut rows = Vec::new();
            for chunk in ids.chunks(IN_CHUNK) {
                rows.extend(
                    fetch_link_keys(
                        self,
                        &format!("l.source_note_id IN ({})", in_list(chunk.len())),
                        chunk,
                    )
                    .await?,
                );
                let keys_sql = format!(
                    "SELECT DISTINCT key FROM note_keys WHERE note_id IN ({})",
                    in_list(chunk.len())
                );
                let keys = sqlx::query_scalar::<_, String>(&keys_sql);
                let keys = chunk
                    .iter()
                    .fold(keys, |q, id| q.bind(id))
                    .fetch_all(self.pool())
                    .await?;
                for key_chunk in keys.chunks(IN_CHUNK) {
                    rows.extend(
                        fetch_link_keys(
                            self,
                            &format!("l.target_key IN ({})", in_list(key_chunk.len())),
                            key_chunk,
                        )
                        .await?,
                    );
                }
            }
            for (link_id, source, source_path, line, key) in rows {
                let Some(target) = resolver.resolve(&source_path, &key).map(|c| c.id) else {
                    continue;
                };
                // Links that touch the frontier at neither end were fetched only because
                // they share a key; they belong to some other part of the graph.
                if target == source
                    || !(in_frontier.contains(&source) || in_frontier.contains(&target))
                {
                    continue;
                }
                neighbours.entry(source).or_default().insert(target);
                neighbours.entry(target).or_default().insert(source);
                links.insert(link_id, (source_path, line, source, target));
            }
            if level == depth {
                break;
            }
            let next: Vec<Uuid> = frontier
                .iter()
                .flat_map(|n| neighbours.get(n).into_iter().flatten().copied())
                .filter(|m| seen.insert(*m))
                .collect();
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        // Edges among the kept notes, in the same order as the global graph.
        let mut ordered: Vec<_> = links.iter().collect();
        ordered.sort_by(|a, b| (&a.1 .0, a.1 .1, a.0).cmp(&(&b.1 .0, b.1 .1, b.0)));
        let mut edge_seen = HashSet::new();
        let edges: Vec<(Uuid, Uuid)> = ordered
            .into_iter()
            .map(|(_, (_, _, s, t))| (*s, *t))
            .filter(|(s, t)| seen.contains(s) && seen.contains(t) && edge_seen.insert((*s, *t)))
            .collect();

        let kept: Vec<String> = seen.iter().map(Uuid::to_string).collect();
        let mut nodes = Vec::new();
        for chunk in kept.chunks(IN_CHUNK) {
            let sql = format!(
                "SELECT id, title, path, archived FROM notes WHERE id IN ({})",
                in_list(chunk.len())
            );
            let q = sqlx::query(&sql);
            for r in chunk
                .iter()
                .fold(q, |q, id| q.bind(id))
                .fetch_all(self.pool())
                .await?
            {
                let id = parse_id(&r.try_get::<String, _>("id")?)?;
                nodes.push(GraphNode {
                    degree: neighbours.get(&id).map_or(0, HashSet::len),
                    id,
                    title: r.try_get("title")?,
                    path: r.try_get("path")?,
                    archived: r.try_get::<i64, _>("archived")? != 0,
                });
            }
        }
        nodes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Graph {
            nodes,
            edges,
            total_notes,
        })
    }

    /// Notes with no resolved link to or from any other note (archived notes excluded).
    pub async fn orphans(&self) -> Result<Vec<NoteRef>> {
        let notes = self.all_notes().await?;
        let mut connected = HashSet::new();
        for (a, b) in self.edges().await? {
            connected.insert(a);
            connected.insert(b);
        }
        Ok(notes
            .into_iter()
            .filter(|n| !n.archived && !connected.contains(&n.id))
            .map(|n| NoteRef {
                id: n.id,
                title: n.title,
                path: n.path,
            })
            .collect())
    }

    /// Notes related to `id`: linked either way (strongest), sharing tags, or in the same
    /// folder. Best first; archived notes excluded.
    pub async fn related(&self, id: Uuid, limit: usize) -> Result<Vec<RelatedNote>> {
        let resolver = self.resolver().await?;
        let mut linked: HashSet<Uuid> = self
            .backlinks_with(id, &resolver)
            .await?
            .iter()
            .map(|b| b.source.id)
            .collect();
        linked.extend(
            self.outgoing_with(id, &resolver)
                .await?
                .iter()
                .filter_map(|o| o.target.as_ref().map(|t| t.id)),
        );
        linked.remove(&id);
        self.related_with(id, limit, &linked).await
    }

    async fn related_with(
        &self,
        id: Uuid,
        limit: usize,
        linked: &HashSet<Uuid>,
    ) -> Result<Vec<RelatedNote>> {
        let notes = self.all_notes().await?;
        let Some(me) = notes.iter().find(|n| n.id == id) else {
            return Ok(Vec::new());
        };
        let my_folder = folder_of(&me.path).to_owned();

        let mut shared: HashMap<Uuid, usize> = HashMap::new();
        let rows = sqlx::query(
            "SELECT nt2.note_id, COUNT(DISTINCT nt2.tag_id) AS shared \
             FROM note_tags nt1 JOIN note_tags nt2 ON nt2.tag_id = nt1.tag_id \
             WHERE nt1.note_id = ?1 AND nt2.note_id != ?1 GROUP BY nt2.note_id",
        )
        .bind(id.to_string())
        .fetch_all(self.pool())
        .await?;
        for r in rows {
            shared.insert(
                parse_id(&r.try_get::<String, _>("note_id")?)?,
                usize::try_from(r.try_get::<i64, _>("shared")?).unwrap_or(0),
            );
        }

        let mut scored: Vec<(usize, RelatedNote)> = notes
            .iter()
            .filter(|n| n.id != id && !n.archived)
            .filter_map(|n| {
                let tags = shared.get(&n.id).copied().unwrap_or(0);
                let is_linked = linked.contains(&n.id);
                let same_folder = folder_of(&n.path) == my_folder;
                let score = 3 * usize::from(is_linked) * 10
                    + tags * 3
                    + usize::from(same_folder && (is_linked || tags > 0));
                (is_linked || tags > 0).then(|| {
                    (
                        score,
                        RelatedNote {
                            note: NoteRef {
                                id: n.id,
                                title: n.title.clone(),
                                path: n.path.clone(),
                            },
                            shared_tags: tags,
                            linked: is_linked,
                        },
                    )
                })
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.note.path.cmp(&b.1.note.path))
        });
        Ok(scored.into_iter().take(limit).map(|(_, r)| r).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::{add, db};
    use crate::models::note_id_for_path;

    fn id(path: &str) -> Uuid {
        note_id_for_path(path)
    }

    #[tokio::test]
    async fn resolves_path_then_title_then_alias_preferring_the_sources_folder() {
        let (_d, db) = db().await;
        add(&db, "Alpha.md", "root").await;
        add(&db, "Projects/Alpha.md", "nested").await;
        add(&db, "Other.md", "---\naliases: [AA, Big Name]\n---\n").await;
        add(&db, "X/Dup.md", "").await;
        add(&db, "Yy/Dup.md", "").await;
        let r = db.resolver().await.unwrap();
        let path = |src: &str, key: &str| r.resolve(src, key).map(|c| c.path.clone());

        assert_eq!(
            path("Projects/Note.md", "alpha").as_deref(),
            Some("Projects/Alpha.md")
        );
        assert_eq!(path("Note.md", "alpha").as_deref(), Some("Alpha.md"));
        assert_eq!(
            path("Z/Note.md", "alpha").as_deref(),
            Some("Alpha.md"),
            "shortest path wins otherwise"
        );
        assert_eq!(
            path("Note.md", "projects/alpha").as_deref(),
            Some("Projects/Alpha.md"),
            "a path match beats a title match"
        );
        assert_eq!(path("Note.md", "aa").as_deref(), Some("Other.md"));
        assert_eq!(path("Note.md", "big name").as_deref(), Some("Other.md"));
        assert_eq!(
            path("Note.md", "dup").as_deref(),
            Some("X/Dup.md"),
            "equal length: alphabetical"
        );
        assert_eq!(path("Yy/Note.md", "dup").as_deref(), Some("Yy/Dup.md"));
        assert_eq!(path("Note.md", "missing"), None);
    }

    #[tokio::test]
    async fn backlinks_list_resolved_links_with_context() {
        let (_d, db) = db().await;
        let target = add(&db, "Rust.md", "About Rust.").await;
        add(
            &db,
            "A.md",
            "Intro\n\nI like [[Rust]] a lot.\nAnd [[rust|the crab]] too.",
        )
        .await;
        add(&db, "Sub/B.md", "See [[Rust.md#Safety]].").await;
        add(&db, "Rust2.md", "Self mention [[Rust2]] only.").await;
        let links = db.backlinks(target.id).await.unwrap();
        let summary: Vec<(String, usize, Option<String>)> = links
            .iter()
            .map(|l| (l.source.path.clone(), l.line, l.display.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                ("A.md".to_owned(), 3, None),
                ("A.md".to_owned(), 4, Some("the crab".to_owned())),
                ("Sub/B.md".to_owned(), 1, None),
            ]
        );
        assert_eq!(links[0].context, "I like [[Rust]] a lot.");
        assert!(
            db.backlinks(id("Rust2.md")).await.unwrap().is_empty(),
            "self links are not backlinks"
        );
    }

    #[tokio::test]
    async fn a_link_counts_only_for_the_note_it_resolves_to() {
        let (_d, db) = db().await;
        add(&db, "Projects/Alpha.md", "").await;
        add(&db, "Alpha.md", "").await;
        add(&db, "Projects/Note.md", "[[Alpha]]").await;
        // From Projects/ the same-folder note wins; the root note gets no backlink.
        assert_eq!(
            db.backlinks(id("Projects/Alpha.md")).await.unwrap().len(),
            1
        );
        assert!(db.backlinks(id("Alpha.md")).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn links_follow_renames_moves_and_deletes_without_reindexing_them() {
        let (_d, db) = db().await;
        let old = add(&db, "Old Name.md", "").await;
        add(&db, "Linker.md", "[[Old Name]] and [[New Name]]").await;
        assert_eq!(db.backlinks(old.id).await.unwrap().len(), 1);
        assert_eq!(db.broken_links().await.unwrap().len(), 1);

        // Rename the target: the old link breaks, the other one starts to resolve.
        let mut renamed = old.clone();
        renamed.path = "New Name.md".into();
        renamed.title = "New Name".into();
        db.upsert_indexed(&renamed, &crate::extract::extract(""))
            .await
            .unwrap();
        assert_eq!(db.backlinks(old.id).await.unwrap().len(), 1);
        let broken = db.broken_links().await.unwrap();
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].target_text, "Old Name");

        db.delete_note(old.id).await.unwrap();
        assert_eq!(db.broken_links().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn outgoing_links_show_targets_and_broken_ones() {
        let (_d, db) = db().await;
        add(&db, "B.md", "").await;
        add(&db, "A.md", "[[B]] [[Nowhere|x]]").await;
        let out = db.outgoing(id("A.md")).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].target.as_ref().map(|t| t.path.as_str()),
            Some("B.md")
        );
        assert!(out[1].target.is_none());
        assert_eq!(out[1].display.as_deref(), Some("x"));
    }

    #[tokio::test]
    async fn resolve_link_answers_for_the_editor() {
        let (_d, db) = db().await;
        add(&db, "Folder/Target.md", "").await;
        add(&db, "Folder/Source.md", "").await;
        let hit = db
            .resolve_link(id("Folder/Source.md"), "Target#Heading")
            .await
            .unwrap();
        assert_eq!(hit.map(|n| n.path), Some("Folder/Target.md".to_owned()));
        assert!(db
            .resolve_link(id("Folder/Source.md"), "Nope")
            .await
            .unwrap()
            .is_none());
        assert!(db
            .resolve_link(Uuid::new_v4(), "Target")
            .await
            .unwrap()
            .is_none());
    }

    async fn sample_graph(db: &Database) {
        add(db, "A.md", "[[B]] [[B]] [[A]]").await; // duplicate and self links collapse
        add(db, "B.md", "[[C]] [[A]]").await; // mutual with A
        add(db, "C.md", "[[D]]").await;
        add(db, "D.md", "").await;
        add(db, "Lonely.md", "no links").await;
        add(db, "Broken.md", "[[Nope]]").await;
    }

    #[tokio::test]
    async fn builds_the_global_graph() {
        let (_d, db) = db().await;
        sample_graph(&db).await;
        let g = db.graph(None, 0).await.unwrap();
        assert_eq!(g.total_notes, 6);
        assert_eq!(g.nodes.len(), 6);
        let mut edges: Vec<(String, String)> = g
            .edges
            .iter()
            .map(|(a, b)| {
                let name = |id: &Uuid| g.nodes.iter().find(|n| n.id == *id).unwrap().title.clone();
                (name(a), name(b))
            })
            .collect();
        edges.sort();
        assert_eq!(
            edges,
            [
                ("A".into(), "B".into()),
                ("B".into(), "A".into()),
                ("B".into(), "C".into()),
                ("C".into(), "D".into())
            ]
        );
        let degree = |t: &str| g.nodes.iter().find(|n| n.title == t).unwrap().degree;
        assert_eq!(
            (
                degree("A"),
                degree("B"),
                degree("C"),
                degree("D"),
                degree("Lonely")
            ),
            (1, 2, 2, 1, 0)
        );
    }

    #[tokio::test]
    async fn builds_local_graphs_by_depth() {
        let (_d, db) = db().await;
        sample_graph(&db).await;
        let titles = |g: Graph| {
            let mut t: Vec<String> = g.nodes.into_iter().map(|n| n.title).collect();
            t.sort();
            t
        };
        assert_eq!(titles(db.graph(Some(id("C.md")), 0).await.unwrap()), ["C"]);
        assert_eq!(
            titles(db.graph(Some(id("C.md")), 1).await.unwrap()),
            ["B", "C", "D"]
        );
        assert_eq!(
            titles(db.graph(Some(id("C.md")), 2).await.unwrap()),
            ["A", "B", "C", "D"]
        );
        let g = db.graph(Some(id("C.md")), 1).await.unwrap();
        assert_eq!(g.total_notes, 6);
        assert!(
            g.edges
                .iter()
                .all(|(a, b)| g.nodes.iter().any(|n| n.id == *a)
                    && g.nodes.iter().any(|n| n.id == *b))
        );
    }

    /// The scoped local graph must equal the global graph cut down to the same notes,
    /// including ambiguous keys (same title in two folders), aliases, self links,
    /// mutual links and links that stay unresolved.
    #[tokio::test]
    async fn local_graph_equals_the_global_graph_restricted_to_the_neighbourhood() {
        let (_d, db) = db().await;
        add(
            &db,
            "Hub.md",
            "[[Leaf]] [[X/Dup]] [[Dup]] [[Hub]] [[nowhere]] [[Al]]",
        )
        .await;
        add(&db, "Leaf.md", "back to [[Hub]] and [[Far]]").await;
        add(&db, "Far.md", "[[Farther]]").await;
        add(&db, "Farther.md", "[[Far]] [[Leaf]]").await;
        add(&db, "X/Dup.md", "[[Hub]]").await;
        add(&db, "Y/Dup.md", "[[Leaf]]").await;
        add(&db, "Y/Src.md", "[[Dup]]").await;
        add(&db, "Aliased.md", "---\naliases: [Al]\n---\n[[Y/Dup]]").await;
        add(&db, "Island.md", "nothing").await;
        let global = db.graph(None, 0).await.unwrap();
        let degree = |id: Uuid| global.nodes.iter().find(|n| n.id == id).unwrap().degree;

        let mut checked = 0;
        for node in &global.nodes {
            for depth in 0..=4 {
                // Notes within `depth` links of the centre, by plain BFS over the global edges.
                let mut keep = HashSet::from([node.id]);
                let mut frontier = vec![node.id];
                for _ in 0..depth {
                    let mut next = Vec::new();
                    for &(a, b) in &global.edges {
                        for (x, y) in [(a, b), (b, a)] {
                            if frontier.contains(&x) && keep.insert(y) {
                                next.push(y);
                            }
                        }
                    }
                    frontier = next;
                }
                let local = db.graph(Some(node.id), depth).await.unwrap();
                let mut got: Vec<_> = local.nodes.iter().map(|n| (n.id, n.degree)).collect();
                let mut want: Vec<_> = keep.iter().map(|&i| (i, degree(i))).collect();
                got.sort();
                want.sort();
                assert_eq!(got, want, "nodes from {} at depth {depth}", node.path);
                let want_edges: Vec<_> = global
                    .edges
                    .iter()
                    .filter(|(a, b)| keep.contains(a) && keep.contains(b))
                    .copied()
                    .collect();
                assert_eq!(
                    local.edges, want_edges,
                    "edges from {} at depth {depth}",
                    node.path
                );
                assert_eq!(local.total_notes, global.total_notes);
                checked += 1;
            }
        }
        assert_eq!(checked, 9 * 5);
        let missing = db.graph(Some(Uuid::new_v4()), 2).await.unwrap();
        assert!(missing.nodes.is_empty() && missing.edges.is_empty());
    }

    #[tokio::test]
    async fn finds_orphans_and_broken_links() {
        let (_d, db) = db().await;
        sample_graph(&db).await;
        let orphans: Vec<String> = db
            .orphans()
            .await
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(
            orphans,
            ["Broken", "Lonely"],
            "a note whose only link is broken is still an orphan"
        );
        let broken = db.broken_links().await.unwrap();
        assert_eq!(broken.len(), 1);
        assert_eq!(
            (
                broken[0].source.title.as_str(),
                broken[0].target_text.as_str()
            ),
            ("Broken", "Nope")
        );
    }

    #[tokio::test]
    async fn orphan_detection_ignores_archived_notes() {
        let (_d, db) = db().await;
        let mut n = add(&db, "Old.md", "").await;
        assert_eq!(db.orphans().await.unwrap().len(), 1);
        n.archived = true;
        db.upsert_indexed(&n, &crate::extract::extract(""))
            .await
            .unwrap();
        assert!(db.orphans().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn suggests_related_notes_by_links_and_shared_tags() {
        let (_d, db) = db().await;
        add(&db, "Me.md", "#rust #tools [[Linked]]").await;
        add(&db, "Linked.md", "").await;
        add(&db, "Sibling.md", "#rust #tools").await;
        add(&db, "Cousin.md", "#rust").await;
        add(&db, "Stranger.md", "#cooking").await;
        add(&db, "BackLinker.md", "[[Me]]").await;
        let related = db.related(id("Me.md"), 10).await.unwrap();
        let names: Vec<&str> = related.iter().map(|r| r.note.title.as_str()).collect();
        assert_eq!(names, ["BackLinker", "Linked", "Sibling", "Cousin"]);
        assert!(related[0].linked && related[0].shared_tags == 0);
        assert_eq!(related[2].shared_tags, 2);
        assert_eq!(db.related(id("Me.md"), 2).await.unwrap().len(), 2);
        assert!(db.related(Uuid::new_v4(), 5).await.unwrap().is_empty());
    }
}
