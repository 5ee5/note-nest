//! Full-text search and tag listing.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::Row;
use uuid::Uuid;

use super::Database;
use crate::models::{SearchHit, SnippetPart, TagCount};
use crate::tag;
use crate::{Error, Result};

const MAX_TERMS: usize = 16;
const MAX_TERM_CHARS: usize = 100;
const MARK_START: char = '\u{1}';
const MARK_END: char = '\u{2}';

/// One search word or quoted phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub text: String,
    pub phrase: bool,
}

/// A search box entry split into text terms and `#tag` / `tag:name` filters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedQuery {
    pub terms: Vec<Term>,
    pub tags: Vec<String>,
}

/// Splits user input. Anything unusual is simply treated as text: the result is only
/// ever turned into FTS5 syntax by quoting each term, so input can never inject
/// operators, column filters or syntax errors.
#[must_use]
pub fn parse_query(input: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let (token, phrase) = if c == '"' {
            chars.next();
            let mut t = String::new();
            for ch in chars.by_ref() {
                if ch == '"' {
                    break;
                }
                t.push(ch);
            }
            (t, true)
        } else {
            let mut t = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                t.push(ch);
                chars.next();
            }
            (t, false)
        };
        if !phrase {
            let tag_name = token
                .strip_prefix('#')
                .or_else(|| token.strip_prefix("tag:"));
            if let Some(name) = tag_name {
                if let Some(t) = tag::normalize(name) {
                    if !parsed.tags.contains(&t) {
                        parsed.tags.push(t);
                    }
                }
                continue;
            }
        }
        // Control characters (NUL would end the string early inside SQLite; the snippet
        // markers are control characters too) never belong in a search term.
        let token: String = token.chars().filter(|c| !c.is_control()).collect();
        let text: String = token.split_whitespace().collect::<Vec<_>>().join(" ");
        let text: String = text.chars().take(MAX_TERM_CHARS).collect();
        if text.chars().any(char::is_alphanumeric) && parsed.terms.len() < MAX_TERMS {
            parsed.terms.push(Term { text, phrase });
        }
    }
    parsed
}

impl ParsedQuery {
    /// The FTS5 MATCH expression: every term quoted, words as prefixes, all ANDed.
    fn match_expression(&self) -> String {
        self.terms
            .iter()
            .map(|t| {
                let quoted = format!("\"{}\"", t.text.replace('"', "\"\""));
                if t.phrase {
                    quoted
                } else {
                    format!("{quoted}*")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Splits FTS5 `snippet()` output (matches wrapped in control characters) into parts,
/// collapsing whitespace so snippets read as one line.
fn parse_marked(s: &str) -> Vec<SnippetPart> {
    let mut parts: Vec<SnippetPart> = Vec::new();
    let mut current = String::new();
    let mut matched = false;
    let mut last_space = true;
    let flush = |text: &mut String, matched: bool, parts: &mut Vec<SnippetPart>| {
        if !text.is_empty() {
            parts.push(SnippetPart {
                text: std::mem::take(text),
                matched,
            });
        }
    };
    for c in s.chars() {
        match c {
            MARK_START => {
                flush(&mut current, matched, &mut parts);
                matched = true;
            }
            MARK_END => {
                flush(&mut current, matched, &mut parts);
                matched = false;
            }
            c if c.is_whitespace() => {
                if !last_space {
                    current.push(' ');
                }
                last_space = true;
            }
            c => {
                current.push(c);
                last_space = false;
            }
        }
    }
    flush(&mut current, matched, &mut parts);
    parts
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Database {
    /// Full-text search over titles, text and tags, best match first. Words match by
    /// prefix and ignore accents and case; `"quoted phrases"` match exactly; `#tag` or
    /// `tag:name` limits results to notes with that tag (or a nested tag of it). A query
    /// with only tags lists the matching notes, newest first.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let parsed = parse_query(query);
        if parsed.terms.is_empty() && parsed.tags.is_empty() {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(50);
        let tag_filter = |first_param: usize, i: usize| {
            format!(
                " AND n.id IN (SELECT nt.note_id FROM note_tags nt JOIN tags t ON t.id = nt.tag_id \
                 WHERE t.name = ?{a} OR t.name LIKE ?{b} ESCAPE '\\')",
                a = first_param + 2 * i,
                b = first_param + 2 * i + 1
            )
        };

        let has_terms = !parsed.terms.is_empty();
        let mut sql = if has_terms {
            String::from(
                "SELECT n.id, n.path, n.title, n.archived, \
                 snippet(note_fts, 0, char(1), char(2), '…', 12) AS tsnip, \
                 snippet(note_fts, 1, char(1), char(2), '…', 16) AS bsnip \
                 FROM note_fts JOIN notes n ON n.rowid = note_fts.rowid WHERE note_fts MATCH ?1",
            )
        } else {
            String::from("SELECT n.id, n.path, n.title, n.archived, '' AS tsnip, '' AS bsnip FROM notes n WHERE 1 = 1")
        };
        let first_tag_param = if has_terms { 2 } else { 1 };
        for i in 0..parsed.tags.len() {
            sql.push_str(&tag_filter(first_tag_param, i));
        }
        let limit_param = first_tag_param + 2 * parsed.tags.len();
        if has_terms {
            sql.push_str(&format!(
                " ORDER BY bm25(note_fts, 10.0, 1.0, 5.0) LIMIT ?{limit_param}"
            ));
        } else {
            sql.push_str(&format!(" ORDER BY n.updated_at DESC LIMIT ?{limit_param}"));
        }

        let mut q = sqlx::query(&sql);
        if has_terms {
            q = q.bind(parsed.match_expression());
        }
        for t in &parsed.tags {
            q = q.bind(t.clone()).bind(format!("{}/%", like_escape(t)));
        }
        let rows = q.bind(limit).fetch_all(self.pool()).await?;

        rows.iter()
            .map(|r| {
                let title: String = r.try_get("title")?;
                let title_parts = parse_marked(&r.try_get::<String, _>("tsnip")?);
                Ok(SearchHit {
                    id: Uuid::parse_str(&r.try_get::<String, _>("id")?).map_err(|e| {
                        Error::Corrupt {
                            context: "note id".into(),
                            message: e.to_string(),
                        }
                    })?,
                    title_parts: if title_parts.is_empty() {
                        vec![SnippetPart {
                            text: title.clone(),
                            matched: false,
                        }]
                    } else {
                        title_parts
                    },
                    title,
                    path: r.try_get("path")?,
                    archived: r.try_get::<i64, _>("archived")? != 0,
                    snippet: parse_marked(&r.try_get::<String, _>("bsnip")?),
                })
            })
            .collect()
    }

    /// Tags of every note: `(frontmatter tags, inline-only tags)` by note id.
    pub async fn note_tag_map(
        &self,
    ) -> Result<std::collections::HashMap<Uuid, (Vec<String>, Vec<String>)>> {
        let rows = sqlx::query(
            "SELECT nt.note_id, t.name, nt.source FROM note_tags nt JOIN tags t ON t.id = nt.tag_id ORDER BY t.name",
        )
        .fetch_all(self.pool())
        .await?;
        let mut map: std::collections::HashMap<Uuid, (Vec<String>, Vec<String>)> =
            std::collections::HashMap::new();
        for r in rows {
            let id = Uuid::parse_str(&r.try_get::<String, _>("note_id")?).map_err(|e| {
                Error::Corrupt {
                    context: "note id".into(),
                    message: e.to_string(),
                }
            })?;
            let name: String = r.try_get("name")?;
            let entry = map.entry(id).or_default();
            if r.try_get::<String, _>("source")? == "frontmatter" {
                entry.0.push(name);
            } else {
                entry.1.push(name);
            }
        }
        Ok(map)
    }

    /// Every tag with the number of notes using it. A nested tag also counts toward its
    /// parents (`a/b/c` is listed as `a`, `a/b` and `a/b/c`), so a parent's count is the
    /// number of notes a click on it would show.
    pub async fn list_tags(&self) -> Result<Vec<TagCount>> {
        let rows = sqlx::query(
            "SELECT t.name, nt.note_id FROM note_tags nt JOIN tags t ON t.id = nt.tag_id",
        )
        .fetch_all(self.pool())
        .await?;
        let mut notes_by_tag: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for r in rows {
            let name: String = r.try_get("name")?;
            let note: String = r.try_get("note_id")?;
            let mut prefix = String::new();
            for part in name.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                notes_by_tag
                    .entry(prefix.clone())
                    .or_default()
                    .insert(note.clone());
            }
        }
        Ok(notes_by_tag
            .into_iter()
            .map(|(name, notes)| TagCount {
                name,
                count: notes.len(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::{add, db};

    fn q(s: &str) -> ParsedQuery {
        parse_query(s)
    }

    fn texts(p: &ParsedQuery) -> Vec<(&str, bool)> {
        p.terms
            .iter()
            .map(|t| (t.text.as_str(), t.phrase))
            .collect()
    }

    #[test]
    fn parses_words_phrases_and_tag_filters() {
        let p = q("rust \"borrow checker\" #Lang tag:programming/rust  ");
        assert_eq!(texts(&p), [("rust", false), ("borrow checker", true)]);
        assert_eq!(p.tags, ["lang", "programming/rust"]);
    }

    #[test]
    fn junk_input_becomes_harmless_text_or_nothing() {
        assert!(q("").terms.is_empty() && q("   ").terms.is_empty());
        assert!(q("* - ( ) ^ + ~").terms.is_empty());
        assert!(q("#").tags.is_empty() && q("tag:").tags.is_empty() && q("#123").tags.is_empty());
        assert_eq!(
            texts(&q("\"unbalanced phrase")),
            [("unbalanced phrase", true)]
        );
        assert_eq!(
            texts(&q("AND OR NOT NEAR")),
            [
                ("AND", false),
                ("OR", false),
                ("NOT", false),
                ("NEAR", false)
            ]
        );
        assert_eq!(q(&"word ".repeat(100)).terms.len(), MAX_TERMS);
        assert_eq!(
            texts(&q("a\u{0}b \u{1}x\u{2}")),
            [("ab", false), ("x", false)]
        );
    }

    #[test]
    fn every_term_is_quoted_in_the_match_expression() {
        let e = q("title:secret \"a \"\"b\" foo*").match_expression();
        assert!(e.starts_with("\"title:secret\"*"), "{e}");
        assert_eq!(q("a \"b c\"").match_expression(), "\"a\"* \"b c\"");
    }

    #[tokio::test]
    async fn finds_notes_by_prefix_ignoring_case_and_accents() {
        let (_d, db) = db().await;
        add(&db, "Programming.md", "Systems programming in Rust.").await;
        add(&db, "Épsilon.md", "A Greek letter.").await;
        assert_eq!(db.search("prog", 10).await.unwrap()[0].title, "Programming");
        assert_eq!(db.search("PROGRAMM", 10).await.unwrap().len(), 1);
        assert_eq!(db.search("epsilon", 10).await.unwrap()[0].title, "Épsilon");
        assert_eq!(db.search("épsi", 10).await.unwrap().len(), 1);
        assert!(db.search("zzz", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn titles_outrank_body_text() {
        let (_d, db) = db().await;
        add(&db, "Notes on cooking.md", "Many recipes.").await;
        add(
            &db,
            "Journal.md",
            "Today I read about cooking, and cooking again; cooking is fun.",
        )
        .await;
        let hits = db.search("cooking", 10).await.unwrap();
        assert_eq!(
            hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(),
            ["Notes on cooking", "Journal"]
        );
    }

    #[tokio::test]
    async fn phrases_match_only_adjacent_words() {
        let (_d, db) = db().await;
        add(&db, "A.md", "the quick brown fox").await;
        add(&db, "B.md", "brown is quick").await;
        assert_eq!(db.search("\"quick brown\"", 10).await.unwrap().len(), 1);
        assert_eq!(db.search("quick brown", 10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn snippets_mark_the_matches() {
        let (_d, db) = db().await;
        add(
            &db,
            "A.md",
            "---\nid: x\n---\nA long text about gardening and more gardening tips.",
        )
        .await;
        let hit = &db.search("garden", 10).await.unwrap()[0];
        let matched: Vec<&str> = hit
            .snippet
            .iter()
            .filter(|p| p.matched)
            .map(|p| p.text.as_str())
            .collect();
        assert!(
            !matched.is_empty() && matched.iter().all(|m| m.starts_with("garden")),
            "{:?}",
            hit.snippet
        );
        assert!(
            !hit.snippet.iter().any(|p| p.text.contains("id: x")),
            "frontmatter must not be searchable"
        );
        assert!(hit
            .snippet
            .iter()
            .all(|p| !p.text.contains('\n') && !p.text.contains(MARK_START)));
    }

    #[tokio::test]
    async fn tag_filters_include_nested_tags_and_combine_with_text() {
        let (_d, db) = db().await;
        add(&db, "A.md", "---\ntags: [programming/rust]\n---\nOwnership").await;
        add(&db, "B.md", "Ownership of #programming things").await;
        add(&db, "C.md", "Ownership and #cooking").await;
        let titles = |hits: Vec<SearchHit>| {
            let mut t: Vec<String> = hits.into_iter().map(|h| h.title).collect();
            t.sort();
            t
        };
        assert_eq!(
            titles(db.search("ownership #programming", 10).await.unwrap()),
            ["A", "B"]
        );
        assert_eq!(
            titles(
                db.search("ownership tag:programming/rust", 10)
                    .await
                    .unwrap()
            ),
            ["A"]
        );
        assert_eq!(
            titles(db.search("#programming #rust", 10).await.unwrap()),
            Vec::<String>::new()
        );
        assert_eq!(titles(db.search("#cooking", 10).await.unwrap()), ["C"]);
    }

    #[tokio::test]
    async fn tags_are_searchable_as_words_and_underscores_are_not_wildcards() {
        let (_d, db) = db().await;
        add(&db, "A.md", "---\ntags: [my_tag]\n---\nText").await;
        add(&db, "B.md", "---\ntags: [myXtag]\n---\nText").await;
        let hits = db.search("#my_tag", 10).await.unwrap();
        assert_eq!(
            hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(),
            ["A"]
        );
    }

    #[tokio::test]
    async fn hostile_queries_never_fail() {
        let (_d, db) = db().await;
        add(&db, "A.md", "hello world").await;
        for query in [
            "\"",
            "\"\"",
            "\"hello",
            "hello\"",
            "AND",
            "OR hello",
            "NOT hello",
            "NEAR(hello world)",
            "title:hello",
            "body: hello",
            "tags:hello",
            "{title body}: hello",
            "hello*",
            "*",
            "**",
            "()",
            ")(",
            "hello AND (world",
            "-hello",
            "^hello",
            "hello OR OR",
            "\u{0}",
            "\u{1}hello\u{2}",
            "😀",
            "a\u{0}b",
            "' OR 1=1 --",
            "\\",
            "%",
            "_",
            &"x".repeat(10_000),
            &"\"".repeat(1000),
            "#a/",
            "tag:/",
            "#\"",
        ] {
            let result = db.search(query, 10).await;
            assert!(result.is_ok(), "{query:?} failed: {result:?}");
        }
        // Quoting keeps operators literal: this searches for the words, not "NOT".
        assert_eq!(db.search("hello NOT world", 10).await.unwrap().len(), 0);
        assert_eq!(db.search("hello world", 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn removed_and_changed_notes_leave_the_index() {
        let (_d, db) = db().await;
        let a = add(&db, "A.md", "needle in a haystack").await;
        assert_eq!(db.search("needle", 10).await.unwrap().len(), 1);
        add(&db, "A.md", "now something else").await;
        assert!(db.search("needle", 10).await.unwrap().is_empty());
        assert_eq!(db.search("something", 10).await.unwrap().len(), 1);
        db.delete_note(a.id).await.unwrap();
        assert!(db.search("something", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn respects_the_limit() {
        let (_d, db) = db().await;
        for i in 0..8 {
            add(&db, &format!("N{i}.md"), "shared word").await;
        }
        assert_eq!(db.search("shared", 3).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn lists_tags_with_nested_roll_up_counts() {
        let (_d, db) = db().await;
        add(&db, "A.md", "---\ntags: [programming/rust, home]\n---\n").await;
        add(&db, "B.md", "#programming/python and #programming/rust").await;
        add(&db, "C.md", "---\ntags: [programming]\n---\n").await;
        let tags: Vec<(String, usize)> = db
            .list_tags()
            .await
            .unwrap()
            .into_iter()
            .map(|t| (t.name, t.count))
            .collect();
        assert_eq!(
            tags,
            [
                ("home".to_owned(), 1),
                ("programming".to_owned(), 3),
                ("programming/python".to_owned(), 1),
                ("programming/rust".to_owned(), 2),
            ]
        );
    }

    #[tokio::test]
    async fn tags_disappear_when_no_note_uses_them() {
        let (_d, db) = db().await;
        add(&db, "A.md", "#temp").await;
        assert_eq!(db.list_tags().await.unwrap().len(), 1);
        add(&db, "A.md", "no tags now").await;
        assert!(db.list_tags().await.unwrap().is_empty());
    }
}
