//! Image attachments: pasted or dropped images are stored in the vault's
//! `Attachments/` folder as `<name>-001.<ext>` and referenced by a relative path.

use serde::Serialize;
use uuid::Uuid;

use super::Workspace;
use crate::filesystem::image::ImageKind;
use crate::filesystem::{RelPath, ATTACHMENTS_DIR};
use crate::{Error, Result};

/// Largest attachment accepted.
pub const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
const MAX_STEM_CHARS: usize = 40;
const MAX_ATTEMPTS: u32 = 10_000;

/// A stored attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attachment {
    /// Path relative to the vault root, e.g. `Attachments/image-001.png`.
    pub vault_path: String,
    /// Path relative to the note that references it, ready for `![](...)`.
    pub markdown_path: String,
}

/// Reduces a suggested file name to a safe stem: ASCII letters, digits, `-` and `_`.
fn clean_stem(suggested: Option<&str>) -> String {
    let name = suggested.unwrap_or("");
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    let mut out = String::new();
    for c in stem.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '_' {
            c
        } else {
            '-'
        };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(c);
    }
    let out: String = out
        .trim_end_matches('-')
        .chars()
        .take(MAX_STEM_CHARS)
        .collect();
    if out.is_empty() {
        "image".to_owned()
    } else {
        out
    }
}

impl Workspace {
    /// Stores an image next to the vault's other attachments and returns how to
    /// reference it from note `note_id`. The file type is decided by the content,
    /// never by the suggested name.
    ///
    /// # Errors
    /// [`Error::UnsupportedFile`] unless the bytes are a PNG, JPEG, GIF, WebP or SVG;
    /// [`Error::TooLarge`] above 25 MiB; [`Error::NotFound`] for an unknown note.
    pub async fn add_attachment(
        &self,
        note_id: Uuid,
        bytes: &[u8],
        suggested_name: Option<&str>,
    ) -> Result<Attachment> {
        if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err(Error::TooLarge {
                what: "attachment".into(),
                limit: MAX_ATTACHMENT_BYTES,
            });
        }
        let kind = ImageKind::detect(bytes).ok_or_else(|| {
            Error::UnsupportedFile(
                "only PNG, JPEG, GIF, WebP and SVG images can be attached".into(),
            )
        })?;
        let _g = self.write_lock.lock().await;
        let record = self
            .db
            .get_note(note_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("note {note_id}")))?;
        let note_depth = RelPath::new(&record.path)?.components().count() - 1;

        let dir = RelPath::new(ATTACHMENTS_DIR)?;
        if !self.vault.exists(&dir) {
            self.vault.create_dir(&dir)?;
        }
        let stem = clean_stem(suggested_name);
        for n in 1..=MAX_ATTEMPTS {
            let name = format!("{stem}-{n:03}.{}", kind.extension());
            let path = dir.join(&name)?;
            match self.vault.create_new(&path, bytes) {
                Ok(()) => {
                    return Ok(Attachment {
                        vault_path: path.to_string(),
                        markdown_path: format!(
                            "{}{ATTACHMENTS_DIR}/{name}",
                            "../".repeat(note_depth)
                        ),
                    });
                }
                Err(Error::AlreadyExists(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Err(Error::AlreadyExists(format!(
            "no free attachment name for {stem:?}"
        )))
    }

    /// Number of files in the attachments folder (used by tests and diagnostics).
    #[cfg(test)]
    fn attachment_files(&self) -> Vec<String> {
        let dir = self.vault.root().join(ATTACHMENTS_DIR);
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|r| {
                r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nDATA";

    async fn open() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let (ws, _) = Workspace::open(dir.path(), false).await.unwrap();
        (dir, ws)
    }

    #[test]
    fn stems_are_cleaned() {
        assert_eq!(clean_stem(None), "image");
        assert_eq!(clean_stem(Some("diagram.svg")), "diagram");
        assert_eq!(clean_stem(Some("../../evil name!.png")), "evil-name");
        assert_eq!(clean_stem(Some("C:\\dir\\Photo 1.JPG")), "Photo-1");
        assert_eq!(clean_stem(Some("日本語.png")), "image");
        assert_eq!(clean_stem(Some("...")), "image");
        assert_eq!(clean_stem(Some(&"a".repeat(200))).len(), MAX_STEM_CHARS);
    }

    #[tokio::test]
    async fn stores_images_with_numbered_names_and_relative_paths() {
        let (dir, ws) = open().await;
        let n = ws.create_note(None, Some("N")).await.unwrap();
        let a = ws.add_attachment(n.id, PNG, None).await.unwrap();
        let b = ws
            .add_attachment(n.id, PNG, Some("image.png"))
            .await
            .unwrap();
        assert_eq!(a.vault_path, "Attachments/image-001.png");
        assert_eq!(a.markdown_path, "Attachments/image-001.png");
        assert_eq!(b.vault_path, "Attachments/image-002.png");
        let svg = ws
            .add_attachment(n.id, b"<svg></svg>", Some("diagram.svg"))
            .await
            .unwrap();
        assert_eq!(svg.vault_path, "Attachments/diagram-001.svg");
        assert_eq!(std::fs::read(dir.path().join(&a.vault_path)).unwrap(), PNG);
        assert_eq!(ws.attachment_files().len(), 3);
    }

    #[tokio::test]
    async fn markdown_path_is_relative_to_the_notes_folder() {
        let (_d, ws) = open().await;
        ws.create_folder("A/B").await.unwrap();
        let n = ws.create_note(Some("A/B"), Some("Deep")).await.unwrap();
        let a = ws.add_attachment(n.id, PNG, None).await.unwrap();
        assert_eq!(a.markdown_path, "../../Attachments/image-001.png");

        // The inserted link really resolves in the preview.
        let md = format!("![x]({})", a.markdown_path);
        let r = ws.render_markdown(Some(n.id), md).await.unwrap();
        assert!(r.html.contains("data:image/png;base64,"), "{}", r.html);
    }

    #[tokio::test]
    async fn the_type_comes_from_the_content_not_the_name() {
        let (_d, ws) = open().await;
        let n = ws.create_note(None, Some("N")).await.unwrap();
        let a = ws
            .add_attachment(n.id, PNG, Some("totally-a-script.exe"))
            .await
            .unwrap();
        assert_eq!(a.vault_path, "Attachments/totally-a-script-001.png");
        for bad in [
            &b"MZ\x90\x00 an executable"[..],
            b"<html></html>",
            b"",
            b"plain text",
        ] {
            assert!(
                matches!(
                    ws.add_attachment(n.id, bad, Some("x.png")).await,
                    Err(Error::UnsupportedFile(_))
                ),
                "{bad:?}"
            );
        }
        assert_eq!(
            ws.attachment_files().len(),
            1,
            "rejected data must not be stored"
        );
    }

    #[tokio::test]
    async fn oversized_and_orphaned_attachments_are_rejected() {
        let (_d, ws) = open().await;
        let n = ws.create_note(None, Some("N")).await.unwrap();
        let mut big = PNG.to_vec();
        big.resize(MAX_ATTACHMENT_BYTES as usize + 1, 0);
        assert!(matches!(
            ws.add_attachment(n.id, &big, None).await,
            Err(Error::TooLarge { .. })
        ));
        assert!(matches!(
            ws.add_attachment(Uuid::new_v4(), PNG, None).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn the_attachments_folder_is_hidden_and_reserved() {
        let (_d, ws) = open().await;
        let n = ws.create_note(None, Some("N")).await.unwrap();
        ws.add_attachment(n.id, PNG, None).await.unwrap();
        ws.reindex().await.unwrap();
        assert!(ws.list_folders().await.unwrap().is_empty());

        assert!(matches!(
            ws.create_folder("Attachments").await,
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            ws.create_folder("Attachments/Sub").await,
            Err(Error::InvalidPath(_))
        ));
        ws.create_folder("Mine").await.unwrap();
        assert!(matches!(
            ws.rename_folder("Mine", "Attachments").await,
            Err(Error::InvalidPath(_))
        ));
    }
}
