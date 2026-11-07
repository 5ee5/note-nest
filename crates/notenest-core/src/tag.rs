//! Tag names: normalization and finding `#tags` in text.
//!
//! A tag is lower-case, made of letters, digits, `_`, `-` and `/` (nesting, e.g.
//! `programming/rust`), contains at least one letter, and is at most 100 characters.

const MAX_TAG_CHARS: usize = 100;

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// Normalizes a user-entered or frontmatter tag, or `None` if it is not a valid tag.
/// A leading `#` is accepted, spaces become `-`, case is folded.
#[must_use]
pub fn normalize(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches('#');
    let mut out = String::new();
    for c in trimmed.chars() {
        let c = if c.is_whitespace() { '-' } else { c };
        if !is_tag_char(c) {
            return None;
        }
        if c == '/' && out.ends_with('/') {
            continue;
        }
        out.extend(c.to_lowercase());
    }
    let out = out.trim_matches(['/', '-']).to_owned();
    let valid = !out.is_empty()
        && out.chars().count() <= MAX_TAG_CHARS
        && out.chars().any(char::is_alphabetic);
    valid.then_some(out)
}

/// `#tags` in plain text. A `#` starts a tag only at the start of the text or after
/// whitespace or an opening bracket/quote, so URL fragments (`site/#top`), `a#b` and
/// numbers (`#123`) are not tags.
#[must_use]
pub fn scan_inline(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut prev: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let boundary_ok =
            prev.is_none_or(|p| p.is_whitespace() || matches!(p, '(' | '[' | '{' | '"' | '\''));
        if c == '#' && boundary_ok {
            let rest = &text[i + 1..];
            let len = rest
                .char_indices()
                .find(|&(_, ch)| !is_tag_char(ch))
                .map_or(rest.len(), |(j, _)| j);
            if len > 0 {
                if let Some(tag) = normalize(&rest[..len]) {
                    if !found.contains(&tag) {
                        found.push(tag);
                    }
                }
                // Skip the characters of the tag we just consumed.
                for _ in 0..rest[..len].chars().count() {
                    chars.next();
                }
                prev = rest[..len].chars().last();
                continue;
            }
        }
        prev = Some(c);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize("Rust").as_deref(), Some("rust"));
        assert_eq!(
            normalize("#Programming/Rust").as_deref(),
            Some("programming/rust")
        );
        assert_eq!(normalize("  my tag  ").as_deref(), Some("my-tag"));
        assert_eq!(normalize("a//b/").as_deref(), Some("a/b"));
        assert_eq!(normalize("日本語").as_deref(), Some("日本語"));
        assert_eq!(normalize("v2").as_deref(), Some("v2"));
    }

    #[test]
    fn rejects_invalid_names() {
        for bad in [
            "",
            "#",
            "123",
            "2026/10",
            "c++",
            "a.b",
            "tag!",
            "-",
            "/",
            &"a".repeat(101),
        ] {
            assert_eq!(normalize(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn finds_inline_tags() {
        assert_eq!(
            scan_inline("a #one and #Two/Sub, #three."),
            vec!["one", "two/sub", "three"]
        );
        assert_eq!(scan_inline("#start of text"), vec!["start"]);
        assert_eq!(scan_inline("(#paren) \"#quoted\""), vec!["paren", "quoted"]);
        assert_eq!(scan_inline("#a #a #A"), vec!["a"]);
        assert_eq!(scan_inline("#日本 text"), vec!["日本"]);
    }

    #[test]
    fn ignores_things_that_are_not_tags() {
        for text in [
            "issue #123 fixed",
            "see https://example.com/#top",
            "a#b",
            "# heading-ish",
            "price #",
            "##double",
            "color #fff0 and #00ff00 maybe",
        ] {
            let tags = scan_inline(text);
            assert!(
                tags.iter().all(|t| t.chars().any(char::is_alphabetic)),
                "{text:?} -> {tags:?}"
            );
        }
        assert!(scan_inline("issue #123 fixed").is_empty());
        assert!(scan_inline("see https://example.com/#top").is_empty());
        assert!(scan_inline("a#b").is_empty());
        assert!(scan_inline("# heading-ish").is_empty());
        assert!(scan_inline("##double").is_empty());
    }
}
