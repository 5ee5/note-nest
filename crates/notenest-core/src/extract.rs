//! Everything the knowledge index needs from a note, in one pass over its Markdown:
//! tags (frontmatter and inline), aliases, wiki links and the searchable body.
//!
//! The Markdown parser decides what is text, so `#tags` and `[[links]]` inside code
//! spans and code blocks are never picked up.

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::{frontmatter, tag};

/// Longest context line stored with a link.
const MAX_CONTEXT_CHARS: usize = 200;

/// A `[[wiki link]]` found in a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedLink {
    /// The target exactly as written, e.g. `Folder/Note#Heading`.
    pub target_text: String,
    /// Lower-cased target without `#heading`, `^block` and `.md`; what notes are matched by.
    pub key: String,
    /// Display text of `[[target|shown]]`, if any.
    pub display: Option<String>,
    /// 1-based line of the link in the whole note (frontmatter included).
    pub line: usize,
    /// The text of that line, trimmed and shortened.
    pub context: String,
    /// Byte range of the whole `[[...]]` in the note text, for rewriting.
    pub span: Range<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extracted {
    /// Tags from the frontmatter `tags:` list.
    pub frontmatter_tags: Vec<String>,
    /// Tags written as `#tag` in the text (excluding those already in the frontmatter).
    pub inline_tags: Vec<String>,
    pub aliases: Vec<String>,
    pub links: Vec<ExtractedLink>,
    /// The note without its frontmatter, as indexed for full-text search.
    pub body: String,
}

/// The matching key of a link target: lower-cased, `#heading`/`^block` suffix and a
/// trailing `.md` removed. Empty for targets that point nowhere (e.g. `[[#heading]]`).
#[must_use]
pub fn link_key(target: &str) -> String {
    let base = target.split(['#', '^']).next().unwrap_or("").trim();
    let base = base
        .strip_suffix(".md")
        .or_else(|| base.strip_suffix(".MD"))
        .unwrap_or(base);
    base.trim().to_lowercase()
}

fn shorten(line: &str) -> String {
    let t = line.trim();
    if t.chars().count() <= MAX_CONTEXT_CHARS {
        t.to_owned()
    } else {
        let cut: String = t.chars().take(MAX_CONTEXT_CHARS).collect();
        format!("{cut}…")
    }
}

/// Extracts tags, aliases, links and the searchable body from a note.
#[must_use]
pub fn extract(content: &str) -> Extracted {
    let lists = frontmatter::parse_lists(content);
    let body = frontmatter::body(content);
    // `body` is a suffix slice of `content`; this is its offset in `content`.
    let offset = (body.as_ptr() as usize).saturating_sub(content.as_ptr() as usize);

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_WIKILINKS;
    let mut inline_tags: Vec<String> = Vec::new();
    let mut links = Vec::new();
    let mut in_code_block = false;
    let mut in_other_link = 0usize;
    let mut wiki: Option<(String, bool, Range<usize>, String)> = None; // target, pothole, span, text
    let mut line_no = 1usize;
    let mut counted_to = 0usize;

    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                ..
            }) => {
                wiki = Some((dest_url.to_string(), has_pothole, range, String::new()));
            }
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => in_other_link += 1,
            Event::End(TagEnd::Link) if wiki.is_some() => {
                let Some((target, pothole, span, text)) = wiki.take() else {
                    continue;
                };
                let key = link_key(&target);
                if key.is_empty() {
                    continue;
                }
                let span = span.start + offset..span.end + offset;
                line_no += content[counted_to..span.start]
                    .bytes()
                    .filter(|&b| b == b'\n')
                    .count();
                counted_to = span.start;
                let line_start = content[..span.start].rfind('\n').map_or(0, |i| i + 1);
                let line_end = content[span.start..]
                    .find('\n')
                    .map_or(content.len(), |i| span.start + i);
                links.push(ExtractedLink {
                    target_text: target,
                    key,
                    display: pothole
                        .then(|| text.trim().to_owned())
                        .filter(|t| !t.is_empty()),
                    line: line_no,
                    context: shorten(&content[line_start..line_end]),
                    span,
                });
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                in_other_link = in_other_link.saturating_sub(1)
            }
            Event::Text(text) => {
                if let Some((.., shown)) = wiki.as_mut() {
                    shown.push_str(&text);
                } else if !in_code_block && in_other_link == 0 {
                    for t in tag::scan_inline(&text) {
                        if !lists.tags.contains(&t) && !inline_tags.contains(&t) {
                            inline_tags.push(t);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Extracted {
        frontmatter_tags: lists.tags,
        inline_tags,
        aliases: lists.aliases,
        links,
        body: body.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(md: &str) -> Vec<(String, Option<String>, usize)> {
        extract(md)
            .links
            .into_iter()
            .map(|l| (l.key, l.display, l.line))
            .collect()
    }

    #[test]
    fn finds_wiki_links_with_keys_display_and_lines() {
        let md = "Intro\n\nSee [[Rust]] and [[Programming/Linux|the OS]].\n\n[[Note#Heading]] [[Other.md]]\n";
        assert_eq!(
            links(md),
            vec![
                ("rust".to_owned(), None, 3),
                ("programming/linux".to_owned(), Some("the OS".to_owned()), 3),
                ("note".to_owned(), None, 5),
                ("other".to_owned(), None, 5),
            ]
        );
    }

    #[test]
    fn link_spans_point_at_the_exact_text_even_with_frontmatter() {
        let md = "\u{feff}---\nid: x\n---\nText [[Alpha|a]] more [[Beta]]\n";
        let e = extract(md);
        let spans: Vec<&str> = e.links.iter().map(|l| &md[l.span.clone()]).collect();
        assert_eq!(spans, ["[[Alpha|a]]", "[[Beta]]"]);
        assert_eq!(e.links[0].line, 4);
        assert_eq!(e.links[0].context, "Text [[Alpha|a]] more [[Beta]]");
    }

    #[test]
    fn ignores_links_and_tags_in_code() {
        let md = "`[[inline]] #code` and\n\n```\n[[block]]\n#blocktag\n```\n\n    [[indented]]\n\nreal [[kept]] #kept\n";
        let e = extract(md);
        assert_eq!(e.links.len(), 1);
        assert_eq!(e.links[0].key, "kept");
        assert_eq!(e.inline_tags, ["kept"]);
    }

    #[test]
    fn plain_markdown_links_and_images_are_not_wiki_links_or_tags() {
        let e = extract("[text #nope](https://a.b/#frag) ![alt #no](x.png) [[wiki]]\n");
        assert_eq!(e.links.len(), 1);
        assert!(e.inline_tags.is_empty());
    }

    #[test]
    fn same_note_heading_links_and_empty_links_are_skipped() {
        assert!(extract("[[#Heading]] [[]] [[ ]]").links.is_empty());
    }

    #[test]
    fn collects_tags_from_frontmatter_and_text() {
        let md = "---\ntags: [Rust, Programming/Rust]\naliases: [RL]\n---\n# Title #in-heading\n\nText #rust #New/Tag and #123\n";
        let e = extract(md);
        assert_eq!(e.frontmatter_tags, ["rust", "programming/rust"]);
        // #rust is already a frontmatter tag, so it is not listed twice.
        assert_eq!(e.inline_tags, ["in-heading", "new/tag"]);
        assert_eq!(e.aliases, ["RL"]);
    }

    #[test]
    fn headings_are_not_tags_but_tags_in_headings_are() {
        let e = extract("# Heading\n\n## Another #todo\n");
        assert_eq!(e.inline_tags, ["todo"]);
    }

    #[test]
    fn body_excludes_the_frontmatter() {
        let e = extract("---\ntitle: x\n---\nBody text\n");
        assert_eq!(e.body, "Body text\n");
        assert_eq!(extract("No block").body, "No block");
    }

    #[test]
    fn link_keys_normalize_targets() {
        assert_eq!(link_key("  My Note  "), "my note");
        assert_eq!(link_key("Folder/Note.md#H"), "folder/note");
        assert_eq!(link_key("Note^block"), "note");
        assert_eq!(link_key("#only"), "");
    }

    #[test]
    fn long_context_lines_are_shortened_on_a_char_boundary() {
        let line = format!("[[x]] {}", "é".repeat(500));
        let e = extract(&line);
        assert!(e.links[0].context.chars().count() <= MAX_CONTEXT_CHARS + 1);
        assert!(e.links[0].context.ends_with('…'));
    }

    #[test]
    fn survives_hostile_input() {
        for md in [
            "[[",
            "]]",
            "[[[[a]]]]",
            "[[a|]]",
            "[[a|b|c]]",
            "\\[[escaped]]",
            "[[\n]]",
            "#",
            "##",
            &"[[a]]".repeat(5000),
        ] {
            let _ = extract(md);
        }
        assert!(extract("\\[[escaped]]").links.is_empty());
    }
}
