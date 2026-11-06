//! Markdown to sanitized HTML for the preview.
//!
//! Rendering is two steps: `pulldown-cmark` turns Markdown into HTML, then
//! `ammonia` removes everything not on a strict allowlist (no scripts, event
//! handlers, iframes, forms, styles). Link and image attributes get extra rules:
//!
//! - links keep only `http`, `https`, `mailto` and `#fragment` targets; relative
//!   links are inert until note linking exists;
//! - images are never fetched from the network. A relative `src` is resolved inside
//!   the vault (no `..` escapes, no symlinks), checked by content to be a real
//!   image, size-limited, and inlined as a `data:` URI. Anything else loses its `src`.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pulldown_cmark::{html, Event, Options, Parser};
use serde::Serialize;

use crate::filesystem::image::ImageKind;
use crate::filesystem::{RelPath, Vault};
use crate::frontmatter;
use crate::{Error, Result};

/// Safety limits for rendering.
#[derive(Debug, Clone, Copy)]
pub struct RenderLimits {
    /// Largest Markdown input that will be rendered.
    pub max_input_bytes: usize,
    /// Largest single image that will be inlined.
    pub max_image_bytes: u64,
    /// Total size of all images inlined into one document.
    pub max_total_image_bytes: usize,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 10 * 1024 * 1024,
            max_image_bytes: 8 * 1024 * 1024,
            max_total_image_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Where relative image paths are resolved from.
#[derive(Debug, Clone)]
pub struct ImageContext {
    pub vault: Vault,
    /// Folder of the note being rendered (`None` for the vault root).
    pub note_dir: Option<RelPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rendered {
    /// Sanitized HTML, safe to insert into the page.
    pub html: String,
    /// Images that could not be shown (missing, too large, not an image, ...).
    pub warnings: Vec<String>,
}

/// Renders a note's Markdown (frontmatter excluded) to sanitized HTML.
///
/// # Errors
/// [`Error::TooLarge`] if the input exceeds `limits.max_input_bytes`.
pub fn render_markdown(
    content: &str,
    images: Option<ImageContext>,
    limits: &RenderLimits,
) -> Result<Rendered> {
    if content.len() > limits.max_input_bytes {
        return Err(Error::TooLarge {
            what: "note".into(),
            limit: limits.max_input_bytes as u64,
        });
    }
    let source = frontmatter::body(content);

    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    // Task-list markers become plain glyphs: no <input> elements are ever emitted.
    let events = Parser::new_ext(source, options).map(|e| match e {
        Event::TaskListMarker(done) => Event::Text(if done { "☑ " } else { "☐ " }.into()),
        other => other,
    });
    let mut raw = String::with_capacity(source.len() * 2);
    html::push_html(&mut raw, events);

    let warnings = Arc::new(Mutex::new(Vec::<String>::new()));
    let total = Arc::new(AtomicUsize::new(0));
    let limits = *limits;
    let warn = Arc::clone(&warnings);

    let mut builder = ammonia::Builder::default();
    builder
        // Table column alignment is the only inline style allowed (see `filter_attribute`).
        .add_tag_attributes("th", ["style"])
        .add_tag_attributes("td", ["style"])
        .url_schemes(HashSet::from(["http", "https", "mailto", "data"]))
        .url_relative(ammonia::UrlRelative::PassThrough)
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |element, attribute, value| {
            filter_attribute(
                element,
                attribute,
                value,
                images.as_ref(),
                &limits,
                &total,
                &warn,
            )
        });
    let html = builder.clean(&raw).to_string();

    let warnings = warnings.lock().map(|w| w.clone()).unwrap_or_default();
    Ok(Rendered { html, warnings })
}

fn filter_attribute<'a>(
    element: &str,
    attribute: &str,
    value: &'a str,
    images: Option<&ImageContext>,
    limits: &RenderLimits,
    total: &AtomicUsize,
    warnings: &Mutex<Vec<String>>,
) -> Option<Cow<'a, str>> {
    let warn = |msg: String| {
        if let Ok(mut w) = warnings.lock() {
            if !w.contains(&msg) {
                w.push(msg);
            }
        }
    };
    match (element, attribute) {
        // Only the exact alignment declarations pulldown-cmark writes; any other CSS is dropped.
        ("th" | "td", "style") => {
            let v = value.trim().trim_end_matches(';').trim();
            matches!(
                v,
                "text-align: left" | "text-align: right" | "text-align: center"
            )
            .then(|| Cow::Borrowed(value))
        }
        ("img", "src") => {
            let ctx = images?;
            match inline_image(ctx, value, limits, total) {
                Ok(uri) => Some(Cow::Owned(uri)),
                Err(why) => {
                    warn(format!("{value}: {why}"));
                    None
                }
            }
        }
        // `data:` is allowed in the scheme list only for the images produced above, so
        // every other attribute that carries a URL must not use it.
        (_, "href" | "cite" | "src" | "poster" | "action") => {
            let scheme = url_scheme(value);
            match scheme.as_deref() {
                Some("http" | "https" | "mailto") => Some(Cow::Borrowed(value)),
                None if attribute == "href" && value.starts_with('#') => Some(Cow::Borrowed(value)),
                _ => None,
            }
        }
        _ => Some(Cow::Borrowed(value)),
    }
}

/// Lowercased scheme of a URL, ignoring leading whitespace/control characters
/// that browsers strip, or `None` for relative URLs.
fn url_scheme(url: &str) -> Option<String> {
    let cleaned: String = url
        .trim_start_matches(|c: char| c.is_whitespace() || c.is_control())
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let end = cleaned.find([':', '/', '?', '#'])?;
    (cleaned.as_bytes()[end] == b':').then(|| cleaned[..end].to_ascii_lowercase())
}

fn inline_image(
    ctx: &ImageContext,
    src: &str,
    limits: &RenderLimits,
    total: &AtomicUsize,
) -> std::result::Result<String, &'static str> {
    let rel = resolve_relative(ctx.note_dir.as_ref(), src).ok_or("not a path inside the vault")?;
    let size = ctx.vault.file_size(&rel).map_err(|_| "file not found")?;
    if size > limits.max_image_bytes {
        return Err("image is too large");
    }
    let size = usize::try_from(size).map_err(|_| "image is too large")?;
    if total.load(Ordering::Relaxed).saturating_add(size) > limits.max_total_image_bytes {
        return Err("too many images in this note");
    }
    let bytes = ctx.vault.read(&rel).map_err(|_| "file not found")?;
    let kind = ImageKind::detect(&bytes).ok_or("not a supported image")?;
    total.fetch_add(bytes.len(), Ordering::Relaxed);
    Ok(format!("data:{};base64,{}", kind.mime(), base64(&bytes)))
}

/// Resolves a markdown-relative URL against the note's folder, staying inside the vault.
fn resolve_relative(note_dir: Option<&RelPath>, src: &str) -> Option<RelPath> {
    let decoded = percent_decode(src)?;
    let path = decoded.split(['?', '#']).next()?;
    if path.contains(['\\', '\0']) || path.starts_with('/') || url_scheme(path).is_some() {
        return None;
    }
    let mut stack: Vec<&str> = note_dir
        .map(|d| d.components().collect())
        .unwrap_or_default();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            c => stack.push(c),
        }
    }
    if stack.is_empty() {
        return None;
    }
    RelPath::new(&stack.join("/")).ok()
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Standard base64 (RFC 4648) with padding.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(char::from(TABLE[(n >> 18) as usize & 63]));
        out.push(char::from(TABLE[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(TABLE[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(TABLE[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

    fn render(md: &str) -> Rendered {
        render_markdown(md, None, &RenderLimits::default()).unwrap()
    }

    fn vault_with(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        for (path, bytes) in files {
            let rel = RelPath::new(path).unwrap();
            if let Some(parent) = rel.parent() {
                let _ = vault.create_dir(&parent);
            }
            vault.create_new(&rel, bytes).unwrap();
        }
        (dir, vault)
    }

    fn ctx(vault: &Vault, dir: Option<&str>) -> Option<ImageContext> {
        Some(ImageContext {
            vault: vault.clone(),
            note_dir: dir.map(|d| RelPath::new(d).unwrap()),
        })
    }

    #[test]
    fn renders_common_markdown() {
        let r = render("# Title\n\nSome **bold**, *italic* and ~~gone~~ text.\n\n- a\n- b\n\n> quote\n\n`code`\n\n---\n");
        for needle in [
            "<h1>Title</h1>",
            "<strong>bold</strong>",
            "<em>italic</em>",
            "<del>gone</del>",
            "<li>a</li>",
            "<blockquote>",
            "<code>code</code>",
            "<hr",
        ] {
            assert!(r.html.contains(needle), "missing {needle} in {}", r.html);
        }
    }

    #[test]
    fn renders_tables_code_blocks_and_task_lists() {
        let r = render("| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n- [x] done\n- [ ] todo\n");
        assert!(r.html.contains("<table>") && r.html.contains("<td>1</td>"));
        assert!(r.html.contains("<pre><code") && r.html.contains("fn main() {}"));
        assert!(r.html.contains("☑ done") && r.html.contains("☐ todo"));
        assert!(!r.html.contains("<input"));
    }

    #[test]
    fn table_alignment_survives_but_no_other_css_does() {
        let r = render("| L | C | R |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n");
        for align in ["left", "center", "right"] {
            assert!(
                r.html.contains(&format!("text-align: {align}")),
                "{}",
                r.html
            );
        }
        for hostile in [
            "<table><tr><td style=\"text-align: right; background: url(javascript:alert(1))\">x</td></tr></table>",
            "<table><tr><td style=\"position: fixed; top: 0\">x</td></tr></table>",
            "<table><tr><td style=\"text-align: right\" onclick=\"alert(1)\">x</td></tr></table>",
        ] {
            let html = render(hostile).html;
            assert!(!html.contains("javascript:") && !html.contains("position") && !html.contains("onclick"), "{html}");
        }
        // A harmless alignment inside raw HTML is fine too.
        assert!(
            render("<table><tr><td style=\"text-align: right\">x</td></tr></table>")
                .html
                .contains("text-align: right")
        );
    }

    #[test]
    fn frontmatter_is_not_rendered() {
        let r = render(
            "---\nid: 123e4567-e89b-12d3-a456-426614174000\ntitle: Secret\n---\n# Visible\n",
        );
        assert!(
            r.html.contains("Visible")
                && !r.html.contains("Secret")
                && !r.html.contains("123e4567")
        );
    }

    #[test]
    fn strips_every_script_vector() {
        let vectors = [
            "<script>alert(1)</script>",
            "<img src=x onerror=alert(1)>",
            "<img src=\"x\" onerror=\"alert(1)\">",
            "[x](javascript:alert(1))",
            "[x](JaVaScRiPt:alert(1))",
            "[x](java\tscript:alert(1))",
            "[x](   javascript:alert(1))",
            "<a href=\"javascript:alert(1)\">x</a>",
            "<a href=\"vbscript:msgbox(1)\">x</a>",
            "<a href=\"data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==\">x</a>",
            "[x](data:text/html,<script>alert(1)</script>)",
            "![x](data:image/svg+xml,<svg onload=alert(1)>)",
            "<iframe src=\"https://evil.example\"></iframe>",
            "<object data=\"x\"></object><embed src=\"x\">",
            "<svg onload=alert(1)><circle/></svg>",
            "<style>body{display:none}</style>",
            "<p style=\"background:url(javascript:alert(1))\">x</p>",
            "<p onclick=\"alert(1)\">x</p>",
            "<form action=\"https://evil.example\"><input name=a></form>",
            "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">",
            "<base href=\"https://evil.example/\">",
            "<link rel=\"stylesheet\" href=\"https://evil.example/x.css\">",
            "<math><mi xlink:href=\"javascript:alert(1)\">x</mi></math>",
            "<details open ontoggle=alert(1)>x</details>",
        ];
        let banned = [
            "<script",
            "onerror",
            "onload",
            "onclick",
            "ontoggle",
            "javascript:",
            "vbscript:",
            "<iframe",
            "<object",
            "<embed",
            "<style",
            "<form",
            "<input",
            "<meta",
            "<base",
            "<link",
            "data:text/html",
            "evil.example",
            "display:none",
            "<svg",
        ];
        for v in vectors {
            let html = render(v).html.to_ascii_lowercase();
            for b in banned {
                assert!(!html.contains(b), "{v:?} leaked {b:?}: {html}");
            }
        }
    }

    #[test]
    fn external_links_are_kept_safely_and_relative_ones_are_inert() {
        let html = render("[ok](https://example.com) [mail](mailto:a@b.c) [frag](#top) [rel](Other.md) [abs](/etc/passwd)").html;
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("rel=\"noopener noreferrer\""));
        assert!(html.contains("href=\"mailto:a@b.c\"") && html.contains("href=\"#top\""));
        assert!(!html.contains("Other.md\"") && !html.contains("/etc/passwd"));
        assert!(html.contains(">rel</a>") || html.contains("rel</a>"));
    }

    #[test]
    fn remote_images_are_never_loaded() {
        let (_d, vault) = vault_with(&[]);
        let r = render_markdown(
            "![x](https://evil.example/pixel.png)",
            ctx(&vault, None),
            &RenderLimits::default(),
        )
        .unwrap();
        assert!(!r.html.contains("evil.example"));
        assert!(r
            .warnings
            .iter()
            .any(|w| w.contains("not a path inside the vault")));
    }

    #[test]
    fn inlines_vault_images_relative_to_the_note() {
        let (_d, vault) = vault_with(&[("Attachments/my pic.png", PNG), ("Notes/Sub/x.md", b"x")]);
        let r = render_markdown(
            "![a](../../Attachments/my%20pic.png) ![b](../../Attachments/./my pic.png)",
            ctx(&vault, Some("Notes/Sub")),
            &RenderLimits::default(),
        )
        .unwrap();
        let expected = format!("data:image/png;base64,{}", base64(PNG));
        assert!(r.html.contains(&expected), "{}", r.html);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn refuses_to_read_outside_the_vault_or_hidden_data() {
        let (d, vault) = vault_with(&[("note.md", b"x")]);
        std::fs::write(d.path().join(".notenest/secret.png"), PNG).unwrap();
        std::fs::write(d.path().parent().unwrap().join("outside.png"), PNG).unwrap();
        for src in [
            "../outside.png",
            "../../../etc/passwd",
            "/etc/passwd",
            "%2e%2e/outside.png",
            ".notenest/secret.png",
            "a\\..\\b.png",
            "file:///etc/passwd",
            "",
        ] {
            let r = render_markdown(
                &format!("![x](<{src}>)"),
                ctx(&vault, None),
                &RenderLimits::default(),
            )
            .unwrap();
            assert!(
                !r.html.contains("data:image"),
                "{src:?} was inlined: {}",
                r.html
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_images_are_refused() {
        let (d, vault) = vault_with(&[]);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("p.png"), PNG).unwrap();
        std::os::unix::fs::symlink(outside.path().join("p.png"), d.path().join("link.png"))
            .unwrap();
        let r = render_markdown(
            "![x](link.png)",
            ctx(&vault, None),
            &RenderLimits::default(),
        )
        .unwrap();
        assert!(!r.html.contains("data:image"));
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn non_images_and_oversized_images_are_dropped_with_a_warning() {
        let (_d, vault) = vault_with(&[("fake.png", b"this is text, not a png"), ("big.png", PNG)]);
        let limits = RenderLimits {
            max_image_bytes: 4,
            ..RenderLimits::default()
        };
        let r = render_markdown(
            "![a](fake.png) ![b](big.png) ![c](missing.png)",
            ctx(&vault, None),
            &limits,
        )
        .unwrap();
        assert!(!r.html.contains("data:image"));
        assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
    }

    #[test]
    fn total_image_budget_is_enforced() {
        let (_d, vault) = vault_with(&[("a.png", PNG), ("b.png", PNG)]);
        let limits = RenderLimits {
            max_total_image_bytes: PNG.len() + 1,
            ..RenderLimits::default()
        };
        let r = render_markdown("![a](a.png) ![b](b.png)", ctx(&vault, None), &limits).unwrap();
        assert_eq!(r.html.matches("data:image/png").count(), 1);
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn oversized_input_is_rejected() {
        let limits = RenderLimits {
            max_input_bytes: 10,
            ..RenderLimits::default()
        };
        assert!(matches!(
            render_markdown("0123456789ab", None, &limits),
            Err(Error::TooLarge { .. })
        ));
    }

    #[test]
    fn base64_matches_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn handles_hostile_and_degenerate_input() {
        for md in [
            "",
            "\0\0\0",
            "[",
            "![",
            "<",
            "<<<<>>>>",
            &"*".repeat(10_000),
            &"> ".repeat(500),
            &"[a](".repeat(1000),
        ] {
            let _ = render(md);
        }
    }
}
