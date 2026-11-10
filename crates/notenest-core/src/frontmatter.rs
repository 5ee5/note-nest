//! Minimal frontmatter handling.
//!
//! NoteNest owns exactly three top-level keys in a note's optional `---` block:
//! `id`, `pinned` and `archived`. Everything else in the file, including any other
//! frontmatter keys, their order and the line-ending style, is preserved verbatim.
//! A tiny hand-written parser is used on purpose: a full YAML round-trip could
//! rewrite user data.

use uuid::Uuid;

use crate::tag;

const BOM: &str = "\u{feff}";

/// NoteNest-owned metadata read from a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Meta {
    pub id: Option<Uuid>,
    pub pinned: bool,
    pub archived: bool,
}

/// Requested changes. `None` leaves a key alone; `Some(false)` removes the
/// `pinned`/`archived` key (absent means false).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetaUpdate {
    pub id: Option<Uuid>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
}

struct Block {
    /// Byte offset of the first line inside the block.
    start: usize,
    /// Byte offset of the closing delimiter line.
    end: usize,
    /// Byte offset just past the closing delimiter line.
    after: usize,
}

fn trim_eol(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn find_block(text: &str) -> Option<Block> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    if trim_eol(first) != "---" {
        return None;
    }
    let start = first.len();
    let mut offset = start;
    for line in lines {
        let t = trim_eol(line);
        if t == "---" || t == "..." {
            return Some(Block {
                start,
                end: offset,
                after: offset + line.len(),
            });
        }
        offset += line.len();
    }
    None
}

fn key_value(line: &str) -> Option<(&str, &str)> {
    let t = trim_eol(line);
    if t.starts_with([' ', '\t', '#', '-']) {
        return None;
    }
    let (k, v) = t.split_once(':')?;
    let v = v.trim().trim_matches(['"', '\'']);
    Some((k.trim_end(), v))
}

/// Lists read from frontmatter for knowledge features.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lists {
    /// Normalized tags (see [`crate::tag::normalize`]), in order, without duplicates.
    pub tags: Vec<String>,
    /// Alternative names the note can be linked by.
    pub aliases: Vec<String>,
}

fn raw_key_value(line: &str) -> Option<(&str, &str)> {
    let t = trim_eol(line);
    if t.starts_with([' ', '\t', '#', '-']) {
        return None;
    }
    let (k, v) = t.split_once(':')?;
    Some((k.trim_end(), v.trim()))
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// Splits `a, "b, c", d` on commas outside quotes.
fn split_list(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, ',') => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

fn is_block_item(line: &str) -> Option<&str> {
    let t = trim_eol(line).trim_start();
    let rest = t.strip_prefix('-')?;
    (rest.is_empty() || rest.starts_with(' ')).then(|| rest.trim())
}

/// Reads the `tags` and `aliases` lists from the frontmatter block: flow style
/// (`[a, b]`), block style (`- a` lines) or a plain scalar (`a, b`). Invalid tags are
/// skipped. Content without a block yields empty lists.
#[must_use]
pub fn parse_lists(content: &str) -> Lists {
    let text = content.strip_prefix(BOM).unwrap_or(content);
    let mut lists = Lists::default();
    let Some(block) = find_block(text) else {
        return lists;
    };
    let lines: Vec<&str> = text[block.start..block.end].split_inclusive('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let Some((key, value)) = raw_key_value(lines[i]) else {
            i += 1;
            continue;
        };
        let is_tags = key == "tags";
        if !is_tags && key != "aliases" {
            i += 1;
            continue;
        }
        let mut items: Vec<String> = Vec::new();
        if value.is_empty() {
            while let Some(item) = lines.get(i + 1).and_then(|l| is_block_item(l)) {
                items.push(unquote(item).to_owned());
                i += 1;
            }
        } else if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            items.extend(split_list(inner).into_iter().map(|s| unquote(s).to_owned()));
        } else if is_tags {
            // Scalars may separate tags by commas or spaces.
            items.extend(
                unquote(value)
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .map(str::to_owned),
            );
        } else {
            items.extend(split_list(value).into_iter().map(|s| unquote(s).to_owned()));
        }
        let target = if is_tags {
            &mut lists.tags
        } else {
            &mut lists.aliases
        };
        for item in items {
            let cleaned = if is_tags {
                tag::normalize(&item)
            } else {
                Some(item.trim().to_owned())
            };
            if let Some(c) = cleaned.filter(|c| !c.is_empty()) {
                if !target.contains(&c) {
                    target.push(c);
                }
            }
        }
        i += 1;
    }
    lists
}

/// Replaces the frontmatter `tags` list with `tags` (normalized, de-duplicated),
/// keeping every other key, the key's list style (flow or block) and line endings.
/// An empty list removes the key (and the block if nothing else is left in it).
#[must_use]
pub fn set_tags(content: &str, tags: &[String]) -> String {
    let mut clean: Vec<String> = Vec::new();
    for t in tags {
        if let Some(n) = tag::normalize(t) {
            if !clean.contains(&n) {
                clean.push(n);
            }
        }
    }
    let (bom, text) = match content.strip_prefix(BOM) {
        Some(rest) => (BOM, rest),
        None => ("", content),
    };
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let flow = |tags: &[String]| format!("tags: [{}]", tags.join(", "));

    let Some(block) = find_block(text) else {
        if clean.is_empty() {
            return content.to_owned();
        }
        return format!("{bom}---{eol}{}{eol}---{eol}{text}", flow(&clean));
    };

    let lines: Vec<&str> = text[block.start..block.end].split_inclusive('\n').collect();
    let found = lines
        .iter()
        .position(|l| raw_key_value(l).is_some_and(|(k, _)| k == "tags"));
    let mut inner = String::new();
    let mut kept = false;
    match found {
        Some(i) => {
            let (_, value) = raw_key_value(lines[i]).unwrap_or(("", ""));
            let is_block = value.is_empty();
            let mut end = i + 1;
            let mut indent = "  ".to_owned();
            if is_block {
                while let Some(l) = lines.get(end).filter(|l| is_block_item(l).is_some()) {
                    if end == i + 1 {
                        let lead = l.len() - l.trim_start().len();
                        indent = l[..lead].to_owned();
                    }
                    end += 1;
                }
            }
            for l in &lines[..i] {
                inner.push_str(l);
                kept = true;
            }
            let ending = &lines[i][trim_eol(lines[i]).len()..];
            let ending = if ending.is_empty() { eol } else { ending };
            if !clean.is_empty() {
                kept = true;
                if is_block {
                    inner.push_str(&format!("tags:{ending}"));
                    for t in &clean {
                        inner.push_str(&format!("{indent}- {t}{ending}"));
                    }
                } else {
                    inner.push_str(&flow(&clean));
                    inner.push_str(ending);
                }
            }
            for l in &lines[end..] {
                inner.push_str(l);
                kept = true;
            }
        }
        None => {
            for l in &lines {
                inner.push_str(l);
                kept = true;
            }
            if !clean.is_empty() {
                if !inner.is_empty() && !inner.ends_with('\n') {
                    inner.push_str(eol);
                }
                inner.push_str(&flow(&clean));
                inner.push_str(eol);
                kept = true;
            }
        }
    }
    if !kept && found.is_some() {
        // The block only held the tags and they are gone: drop it entirely.
        return format!("{bom}{}", &text[block.after..]);
    }
    let open = &text[..block.start];
    let close = &text[block.end..block.after];
    format!("{bom}{open}{inner}{close}{}", &text[block.after..])
}

/// The note text without its leading frontmatter block (and BOM). Text without a
/// block is returned unchanged.
#[must_use]
pub fn body(content: &str) -> &str {
    let text = content.strip_prefix(BOM).unwrap_or(content);
    match find_block(text) {
        Some(block) => &text[block.after..],
        None => text,
    }
}

/// Reads NoteNest metadata from note content. Malformed values are ignored.
#[must_use]
pub fn parse_meta(content: &str) -> Meta {
    let text = content.strip_prefix(BOM).unwrap_or(content);
    let mut meta = Meta::default();
    let Some(block) = find_block(text) else {
        return meta;
    };
    let mut seen_id = false;
    let mut seen_pinned = false;
    let mut seen_archived = false;
    for line in text[block.start..block.end].split_inclusive('\n') {
        let Some((k, v)) = key_value(line) else {
            continue;
        };
        match k {
            "id" if !seen_id => {
                seen_id = true;
                meta.id = Uuid::parse_str(v).ok();
            }
            "pinned" if !seen_pinned => {
                seen_pinned = true;
                meta.pinned = v == "true";
            }
            "archived" if !seen_archived => {
                seen_archived = true;
                meta.archived = v == "true";
            }
            _ => {}
        }
    }
    meta
}

/// Returns `content` with the requested metadata changes applied. Content is
/// returned unchanged when nothing needs to change.
#[must_use]
pub fn set_meta(content: &str, update: &MetaUpdate) -> String {
    // Desired line per key: Some(line) = write it, None = remove the key.
    edit_meta(content, |key| match key {
        "id" => update.id.map(|u| Some(format!("id: {u}"))),
        "pinned" => update.pinned.map(|b| b.then(|| "pinned: true".to_owned())),
        "archived" => update
            .archived
            .map(|b| b.then(|| "archived: true".to_owned())),
        _ => None,
    })
}

/// Returns `content` without the keys NoteNest owns (`id`, `pinned`, `archived`);
/// the frontmatter block is dropped if nothing else was in it.
#[must_use]
pub fn strip_meta(content: &str) -> String {
    edit_meta(content, |_| Some(None))
}

/// `wanted(key)`: `None` = leave the key alone, `Some(None)` = remove it,
/// `Some(Some(line))` = write that line.
fn edit_meta(content: &str, wanted: impl Fn(&str) -> Option<Option<String>>) -> String {
    let (bom, text) = match content.strip_prefix(BOM) {
        Some(rest) => (BOM, rest),
        None => ("", content),
    };
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };

    let Some(block) = find_block(text) else {
        let lines: Vec<String> = ["id", "pinned", "archived"]
            .iter()
            .filter_map(|k| wanted(k).flatten())
            .collect();
        if lines.is_empty() {
            return content.to_owned();
        }
        let mut out = String::from(bom);
        out.push_str("---");
        out.push_str(eol);
        for l in &lines {
            out.push_str(l);
            out.push_str(eol);
        }
        out.push_str("---");
        out.push_str(eol);
        out.push_str(text);
        return out;
    };

    let mut body = String::new();
    let mut handled = [false; 3];
    let mut removed_any = false;
    let mut kept_any = false;
    for line in text[block.start..block.end].split_inclusive('\n') {
        let key = key_value(line).map(|(k, _)| k);
        let slot = key.and_then(|k| ["id", "pinned", "archived"].iter().position(|x| *x == k));
        match slot {
            Some(i) if !handled[i] => {
                handled[i] = true;
                match wanted(["id", "pinned", "archived"][i]) {
                    None => {
                        body.push_str(line);
                        kept_any = true;
                    }
                    Some(Some(new_line)) => {
                        let ending = &line[trim_eol(line).len()..];
                        body.push_str(&new_line);
                        body.push_str(if ending.is_empty() { eol } else { ending });
                        kept_any = true;
                    }
                    Some(None) => removed_any = true,
                }
            }
            _ => {
                body.push_str(line);
                kept_any = true;
            }
        }
    }
    if !body.is_empty() && !body.ends_with('\n') {
        body.push_str(eol);
    }
    for (i, key) in ["id", "pinned", "archived"].iter().enumerate() {
        if handled[i] {
            continue;
        }
        if let Some(Some(line)) = wanted(key) {
            body.push_str(&line);
            body.push_str(eol);
            kept_any = true;
        }
    }

    if !kept_any && removed_any {
        // The block only held keys we owned and they are all gone: drop it entirely.
        return format!("{bom}{}", &text[block.after..]);
    }
    let open = &text[..block.start];
    let close = &text[block.end..block.after];
    format!("{bom}{open}{body}{close}{}", &text[block.after..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn id() -> Uuid {
        Uuid::parse_str(ID).unwrap()
    }

    fn with_id() -> MetaUpdate {
        MetaUpdate {
            id: Some(id()),
            ..Default::default()
        }
    }

    #[test]
    fn strip_meta_removes_only_our_keys() {
        assert_eq!(
            strip_meta(&format!(
                "---\nid: {ID}\ntags: [a]\npinned: true\n---\nBody"
            )),
            "---\ntags: [a]\n---\nBody"
        );
        assert_eq!(
            strip_meta(&format!("---\nid: {ID}\narchived: true\n---\nBody")),
            "Body"
        );
        assert_eq!(strip_meta("No block"), "No block");
        assert_eq!(strip_meta("---\ntitle: x\n---\nB"), "---\ntitle: x\n---\nB");
    }

    #[test]
    fn body_skips_the_frontmatter_block() {
        assert_eq!(body("---\ntitle: x\n---\n# Hi\n"), "# Hi\n");
        assert_eq!(body("\u{feff}---\r\nid: a\r\n---\r\nText"), "Text");
        assert_eq!(body("# No block\n"), "# No block\n");
        assert_eq!(body("---\nunclosed\n"), "---\nunclosed\n");
    }

    #[test]
    fn parses_owned_keys() {
        let m = parse_meta(&format!(
            "---\nid: {ID}\npinned: true\narchived: false\n---\n# Hi\n"
        ));
        assert_eq!(
            m,
            Meta {
                id: Some(id()),
                pinned: true,
                archived: false
            }
        );
    }

    #[test]
    fn no_frontmatter_or_malformed_values_yield_defaults() {
        assert_eq!(parse_meta("# Title\n"), Meta::default());
        assert_eq!(
            parse_meta("---\nid: not-a-uuid\npinned: maybe\n---\n"),
            Meta::default()
        );
        assert_eq!(
            parse_meta("---\nid: abc\nno closing delimiter"),
            Meta::default()
        );
        assert_eq!(parse_meta("---\n  id: indented\n---\n"), Meta::default());
    }

    #[test]
    fn accepts_quoted_ids_bom_and_crlf() {
        let m = parse_meta(&format!("\u{feff}---\r\nid: \"{ID}\"\r\n---\r\nbody"));
        assert_eq!(m.id, Some(id()));
    }

    #[test]
    fn adds_a_block_to_plain_notes() {
        let out = set_meta("# Hello\n", &with_id());
        assert_eq!(out, format!("---\nid: {ID}\n---\n# Hello\n"));
        assert_eq!(parse_meta(&out).id, Some(id()));
    }

    #[test]
    fn preserves_foreign_keys_order_and_body() {
        let src = "---\ntitle: My note\ntags: [a, b]\nid: 123e4567-e89b-12d3-a456-000000000000\n---\nBody\n";
        let out = set_meta(src, &with_id());
        assert_eq!(
            out,
            format!("---\ntitle: My note\ntags: [a, b]\nid: {ID}\n---\nBody\n")
        );
    }

    #[test]
    fn appends_missing_keys_inside_the_existing_block() {
        let out = set_meta(
            "---\ntitle: x\n---\nBody",
            &MetaUpdate {
                pinned: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(out, "---\ntitle: x\npinned: true\n---\nBody");
    }

    #[test]
    fn keeps_crlf_and_bom() {
        let out = set_meta("\u{feff}line one\r\nline two\r\n", &with_id());
        assert_eq!(
            out,
            format!("\u{feff}---\r\nid: {ID}\r\n---\r\nline one\r\nline two\r\n")
        );
    }

    #[test]
    fn false_removes_keys_and_empty_blocks_disappear() {
        let src = format!("---\nid: {ID}\npinned: true\n---\nBody\n");
        let unpinned = set_meta(
            &src,
            &MetaUpdate {
                pinned: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(unpinned, format!("---\nid: {ID}\n---\nBody\n"));

        let only_pin = "---\npinned: true\n---\nBody\n";
        let out = set_meta(
            only_pin,
            &MetaUpdate {
                pinned: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(out, "Body\n");
    }

    #[test]
    fn no_op_updates_leave_content_untouched() {
        let plain = "---\n---\nBody\n";
        assert_eq!(set_meta(plain, &MetaUpdate::default()), plain);
        assert_eq!(set_meta("text", &MetaUpdate::default()), "text");
        assert_eq!(
            set_meta(
                "text",
                &MetaUpdate {
                    pinned: Some(false),
                    ..Default::default()
                }
            ),
            "text"
        );
    }

    #[test]
    fn is_idempotent() {
        let once = set_meta("# A\n", &with_id());
        assert_eq!(set_meta(&once, &with_id()), once);
    }

    #[test]
    fn unclosed_block_is_treated_as_body() {
        let out = set_meta("---\nnot closed\n", &with_id());
        assert_eq!(out, format!("---\nid: {ID}\n---\n---\nnot closed\n"));
    }
}

#[cfg(test)]
mod list_tests {
    use super::*;

    fn tags(content: &str) -> Vec<String> {
        parse_lists(content).tags
    }

    #[test]
    fn reads_flow_block_and_scalar_tags() {
        assert_eq!(
            tags("---\ntags: [Rust, \"my tag\", #a/b]\n---\n"),
            ["rust", "my-tag", "a/b"]
        );
        // Hand-edited files may mix indentation; every `- item` under the key counts.
        assert_eq!(
            tags("---\ntags:\n  - one\n  - Two\n- three\n---\n"),
            ["one", "two", "three"]
        );
        assert_eq!(tags("---\ntags:\n- x\n- y\nother: z\n---\n"), ["x", "y"]);
        assert_eq!(tags("---\ntags: a, b c\n---\n"), ["a", "b", "c"]);
        assert_eq!(tags("---\ntags: solo\n---\n"), ["solo"]);
    }

    #[test]
    fn skips_invalid_tags_and_duplicates() {
        assert_eq!(tags("---\ntags: [ok, 123, c++, ok, OK]\n---\n"), ["ok"]);
        assert!(tags("no block #tag").is_empty());
        assert!(tags("---\ntags: []\n---\n").is_empty());
        assert!(tags("---\ntitle: x\n---\n").is_empty());
    }

    #[test]
    fn reads_aliases_keeping_their_case_and_spaces() {
        let l = parse_lists("---\naliases: [\"Rust Lang\", RL]\n---\n");
        assert_eq!(l.aliases, ["Rust Lang", "RL"]);
        let l = parse_lists("---\naliases:\n  - First One\n  - Second\n---\n");
        assert_eq!(l.aliases, ["First One", "Second"]);
    }

    #[test]
    fn set_tags_keeps_other_keys_and_the_list_style() {
        let flow = "---\ntitle: x\ntags: [old]\nid: a\n---\nBody\n";
        assert_eq!(
            set_tags(flow, &["New".into(), "b/c".into()]),
            "---\ntitle: x\ntags: [new, b/c]\nid: a\n---\nBody\n"
        );

        let block = "---\ntags:\n  - old\n  - older\nid: a\n---\nBody\n";
        assert_eq!(
            set_tags(block, &["x".into(), "y".into()]),
            "---\ntags:\n  - x\n  - y\nid: a\n---\nBody\n"
        );
        let unindented = "---\ntags:\n- old\n---\n";
        assert_eq!(
            set_tags(unindented, &["new".into()]),
            "---\ntags:\n- new\n---\n"
        );
    }

    #[test]
    fn set_tags_adds_the_key_or_the_whole_block() {
        assert_eq!(
            set_tags("---\ntitle: x\n---\nB", &["a".into()]),
            "---\ntitle: x\ntags: [a]\n---\nB"
        );
        assert_eq!(
            set_tags("# Note\n", &["a".into()]),
            "---\ntags: [a]\n---\n# Note\n"
        );
        assert_eq!(
            set_tags("\u{feff}Text", &["a".into()]),
            "\u{feff}---\ntags: [a]\n---\nText"
        );
        assert_eq!(
            set_tags("---\r\nid: 1\r\n---\r\nB", &["a".into()]),
            "---\r\nid: 1\r\ntags: [a]\r\n---\r\nB"
        );
    }

    #[test]
    fn set_tags_can_remove_the_key_and_an_empty_block() {
        assert_eq!(
            set_tags("---\ntitle: x\ntags: [a]\n---\nB", &[]),
            "---\ntitle: x\n---\nB"
        );
        assert_eq!(set_tags("---\ntags: [a]\n---\nB", &[]), "B");
        assert_eq!(
            set_tags("---\ntags:\n  - a\n  - b\nid: 1\n---\nB", &[]),
            "---\nid: 1\n---\nB"
        );
        assert_eq!(set_tags("plain", &[]), "plain");
        assert_eq!(
            set_tags("---\ntitle: x\n---\nB", &[]),
            "---\ntitle: x\n---\nB"
        );
    }

    #[test]
    fn set_tags_drops_invalid_names_and_duplicates() {
        assert_eq!(
            set_tags(
                "x",
                &["ok".into(), "OK".into(), "123".into(), "no way!".into()]
            ),
            "---\ntags: [ok]\n---\nx"
        );
    }

    #[test]
    fn set_tags_is_idempotent_and_round_trips() {
        let once = set_tags("---\nid: 1\n---\nB", &["a".into(), "b".into()]);
        assert_eq!(set_tags(&once, &["a".into(), "b".into()]), once);
        assert_eq!(parse_lists(&once).tags, ["a", "b"]);
    }

    #[test]
    fn owned_meta_is_untouched_by_tag_edits() {
        let src = "---\nid: 123e4567-e89b-12d3-a456-426614174000\npinned: true\n---\nB";
        let out = set_tags(src, &["t".into()]);
        assert_eq!(parse_meta(&out), parse_meta(src));
    }
}
