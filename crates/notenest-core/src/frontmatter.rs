//! Minimal frontmatter handling.
//!
//! NoteNest owns exactly three top-level keys in a note's optional `---` block:
//! `id`, `pinned` and `archived`. Everything else in the file, including any other
//! frontmatter keys, their order and the line-ending style, is preserved verbatim.
//! A tiny hand-written parser is used on purpose: a full YAML round-trip could
//! rewrite user data.

use uuid::Uuid;

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
    let (bom, text) = match content.strip_prefix(BOM) {
        Some(rest) => (BOM, rest),
        None => ("", content),
    };
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };

    // Desired line per key: Some(line) = write it, None = remove the key.
    let wanted = |key: &str| -> Option<Option<String>> {
        match key {
            "id" => update.id.map(|u| Some(format!("id: {u}"))),
            "pinned" => update.pinned.map(|b| b.then(|| "pinned: true".to_owned())),
            "archived" => update
                .archived
                .map(|b| b.then(|| "archived: true".to_owned())),
            _ => None,
        }
    };

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
