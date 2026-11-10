//! Making notes portable: finding the local files a note references, and turning
//! NoteNest-flavoured Markdown into standard Markdown for export.
//!
//! Everything here works on text and vault-relative paths only; the services decide
//! where files come from and go to.

use std::fmt::Write as _;
use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};

use crate::extract::extract;
use crate::frontmatter::{body, strip_meta};

/// A standard Markdown link or image pointing at a local file, e.g. `![](Attachments/a.png)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRef {
    /// The destination as written, percent-decoded, without `#fragment` or `?query`.
    pub path: String,
    /// Byte range of the destination text in the note, if it could be located exactly
    /// (needed to rewrite it).
    pub span: Option<Range<usize>>,
    pub is_image: bool,
}

fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_WIKILINKS
}

/// True for destinations that are not local relative paths: URLs (`https:`, `mailto:`,
/// `data:`, `file:` ...), absolute paths and same-page anchors.
fn is_external(dest: &str) -> bool {
    if dest.is_empty() || dest.starts_with(['#', '/', '\\', '?']) {
        return true;
    }
    // A scheme: letters, digits, `+`, `-`, `.` before the first `:` (Windows drive letters too).
    dest.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// Removes backslash escapes (`\(` -> `(`), as Markdown does in link destinations.
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next.is_ascii_punctuation() {
                    out.push(next);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// Length of a destination written without `<>`: up to whitespace or the `)` that
/// closes the link, skipping escaped characters and balanced parentheses.
fn bare_destination_len(rest: &str) -> usize {
    let mut depth = 0usize;
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '(' => depth += 1,
            ')' if depth == 0 => return i,
            ')' => depth -= 1,
            c if c.is_whitespace() => return i,
            _ => {}
        }
    }
    rest.len()
}

/// Finds the destination `dest` (as parsed) inside the source text of a link, `[..](dest ..)`.
fn locate_destination(source: &str, start: usize, dest: &str) -> Option<Range<usize>> {
    let text = &source[start..];
    let mut search_end = text.len();
    while let Some(at) = text[..search_end].rfind("](") {
        let after = at + 2;
        let rest = &text[after..];
        let skipped = rest.len() - rest.trim_start().len();
        let rest = rest.trim_start();
        let (raw_start, raw) = if let Some(inner) = rest.strip_prefix('<') {
            let end = inner.find('>')?;
            (after + skipped + 1, &inner[..end])
        } else {
            (after + skipped, &rest[..bare_destination_len(rest)])
        };
        if raw == dest || unescape(raw) == dest {
            return Some(start + raw_start..start + raw_start + raw.len());
        }
        search_end = at;
    }
    None
}

/// Percent-decodes `s` (invalid sequences are kept as written).
#[must_use]
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes a relative path for a Markdown link destination: everything but
/// unreserved characters and `/`.
#[must_use]
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Every local file a note references through standard Markdown links and images
/// (not `[[wiki links]]`, not URLs). Links inside code are not references.
#[must_use]
pub fn local_refs(content: &str) -> Vec<LocalRef> {
    let text = body(content);
    let offset = (text.as_ptr() as usize).saturating_sub(content.as_ptr() as usize);
    let mut refs = Vec::new();
    for (event, range) in Parser::new_ext(text, parser_options()).into_offset_iter() {
        let (dest, is_image) = match event {
            Event::Start(Tag::Link {
                link_type: LinkType::Inline,
                dest_url,
                ..
            }) => (dest_url, false),
            Event::Start(Tag::Image {
                link_type: LinkType::Inline,
                dest_url,
                ..
            }) => (dest_url, true),
            _ => continue,
        };
        if is_external(&dest) {
            continue;
        }
        let path_part = dest.split(['#', '?']).next().unwrap_or("");
        if path_part.is_empty() {
            continue;
        }
        // Only rewritable if the destination has no fragment or query to keep.
        let span = locate_destination(content, range.start + offset, &dest)
            .filter(|_| !dest.contains(['#', '?']));
        refs.push(LocalRef {
            path: percent_decode(path_part).replace('\\', "/"),
            span,
            is_image,
        });
    }
    refs
}

/// Joins a relative `path` onto the vault folder `dir` ("" for the root) and resolves
/// `.` and `..`. `None` if it climbs out of the vault.
#[must_use]
pub fn normalize(dir: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            c => parts.push(c),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The vault folder of a vault path ("" for the root).
#[must_use]
pub fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The relative path from vault folder `from_dir` to vault path `to`.
#[must_use]
pub fn relative(from_dir: &str, to: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|p| !p.is_empty()).collect();
    let target: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count()
        .min(target.len().saturating_sub(1));
    let mut out: Vec<&str> = vec![".."; from.len() - common];
    out.extend(&target[common..]);
    out.join("/")
}

/// GitHub-style anchor for a heading: lower case, spaces to `-`, punctuation dropped.
#[must_use]
pub fn heading_slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

fn escape_link_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// What `[[wiki links]]` become in exported Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WikiLinks {
    /// `[shown](relative/path.md#heading)` when the target exists, else plain text.
    Relative,
    /// Always plain text (the linked notes are not part of the export).
    Text,
}

/// Turns a note into standard Markdown: NoteNest's own frontmatter keys are removed
/// (other frontmatter is kept) and `[[wiki links]]` become regular links or text.
/// `resolve(key)` is where a link with that key points (a vault path), if anywhere.
#[must_use]
pub fn standardize(
    content: &str,
    source_path: &str,
    resolve: impl Fn(&str) -> Option<String>,
    style: WikiLinks,
) -> String {
    let mut out = strip_meta(content);
    let mut links = extract(&out).links;
    links.sort_by_key(|l| std::cmp::Reverse(l.span.start));
    for link in links {
        let (base, suffix) = link.target_text.split_at(
            link.target_text
                .find(['#', '^'])
                .unwrap_or(link.target_text.len()),
        );
        let base = base.trim();
        let shown_base = base
            .strip_suffix(".md")
            .or_else(|| base.strip_suffix(".MD"))
            .unwrap_or(base);
        let shown = link
            .display
            .clone()
            .unwrap_or_else(|| match suffix.strip_prefix('#') {
                Some(h) if !h.is_empty() => format!("{shown_base} > {h}"),
                _ => shown_base.to_owned(),
            });
        let replacement = match (style, resolve(&link.key)) {
            (WikiLinks::Relative, Some(target)) => {
                let anchor = suffix
                    .strip_prefix('#')
                    .filter(|h| !h.is_empty())
                    .map(|h| format!("#{}", heading_slug(h)))
                    .unwrap_or_default();
                let dest = encode_path(&relative(folder_of(source_path), &target));
                format!("[{}]({dest}{anchor})", escape_link_text(&shown))
            }
            _ => shown,
        };
        out.replace_range(link.span, &replacement);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_local_references_with_their_positions() {
        let note = "---\ntags: [a]\n---\n![shot](Attachments/a%20b.png) [pdf](../Docs/x.pdf) \
                    [web](https://e.com) [mail](mailto:a@b.c) [top](#h) ![abs](/etc/x.png) \
                    [[Wiki]] `![code](no.png)` [n](Other.md) [esc](a\\(1\\).png) [t](<sp ace.png>)";
        let refs = local_refs(note);
        let paths: Vec<_> = refs.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "Attachments/a b.png",
                "../Docs/x.pdf",
                "Other.md",
                "a(1).png",
                "sp ace.png"
            ]
        );
        assert!(refs[0].is_image && !refs[1].is_image);
        for r in &refs {
            let span = r.span.clone().expect("located");
            let raw = &note[span];
            assert!(
                raw == r.path || percent_decode(raw) == r.path || unescape(raw) == r.path,
                "{raw} vs {}",
                r.path
            );
        }
    }

    #[test]
    fn a_reference_with_a_fragment_is_found_but_not_rewritable() {
        let refs = local_refs("[doc](Docs/manual.pdf#page=3)");
        assert_eq!(refs[0].path, "Docs/manual.pdf");
        assert!(refs[0].span.is_none());
    }

    #[test]
    fn path_helpers() {
        assert_eq!(normalize("A/B", "../C/x.png").as_deref(), Some("A/C/x.png"));
        assert_eq!(normalize("", "./x.png").as_deref(), Some("x.png"));
        assert_eq!(normalize("A", "../../x.png"), None);
        assert_eq!(relative("A/B", "A/C/x.png"), "../C/x.png");
        assert_eq!(relative("", "Attachments/x.png"), "Attachments/x.png");
        assert_eq!(relative("A", "A/x.md"), "x.md");
        assert_eq!(relative("A/B", "x.md"), "../../x.md");
        assert_eq!(encode_path("A B/ä(1).md"), "A%20B/%C3%A4%281%29.md");
        assert_eq!(percent_decode("A%20B/%C3%A4"), "A B/ä");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(heading_slug("Hello, World 2!"), "hello-world-2");
        assert_eq!(folder_of("A/B/c.md"), "A/B");
        assert_eq!(folder_of("c.md"), "");
    }

    #[test]
    fn standardizes_wiki_links_and_our_frontmatter() {
        let note = "---\nid: 123e4567-e89b-12d3-a456-426614174000\ntags: [x]\n---\n\
                    See [[Rust]], [[Projects/Plan#Next Steps|the plan]], [[Gone]] and `[[code]]`.";
        let resolve = |key: &str| match key {
            "rust" => Some("Programming/Rust.md".to_owned()),
            "projects/plan" => Some("Projects/Plan.md".to_owned()),
            _ => None,
        };
        let out = standardize(note, "Projects/Notes.md", resolve, WikiLinks::Relative);
        assert_eq!(
            out,
            "---\ntags: [x]\n---\nSee [Rust](../Programming/Rust.md), \
             [the plan](Plan.md#next-steps), Gone and `[[code]]`."
        );
        let text = standardize(note, "Projects/Notes.md", resolve, WikiLinks::Text);
        assert!(
            text.ends_with("See Rust, the plan, Gone and `[[code]]`."),
            "{text}"
        );
        let heading = standardize("[[A#Intro]]", "n.md", |_| None, WikiLinks::Text);
        assert_eq!(heading, "A > Intro");
        let spaced = standardize(
            "[[My Note]]",
            "n.md",
            |_| Some("Some Folder/My Note.md".into()),
            WikiLinks::Relative,
        );
        assert_eq!(spaced, "[My Note](Some%20Folder/My%20Note.md)");
    }
}
