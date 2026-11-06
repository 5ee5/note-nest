//! Recognizing image files by their content, never by their extension.

/// Image formats NoteNest accepts for attachments and previews.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
}

impl ImageKind {
    /// Detects the format from the file's leading bytes.
    #[must_use]
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else if looks_like_svg(bytes) {
            Some(Self::Svg)
        } else {
            None
        }
    }

    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Svg => "image/svg+xml",
        }
    }

    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(2048)];
    let Ok(text) =
        std::str::from_utf8(head).or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()]))
    else {
        return false;
    };
    let t = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase();
    t.starts_with("<svg")
        || ((t.starts_with("<?xml") || t.starts_with("<!doctype svg")) && t.contains("<svg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats_by_content() {
        assert_eq!(
            ImageKind::detect(b"\x89PNG\r\n\x1a\nrest"),
            Some(ImageKind::Png)
        );
        assert_eq!(
            ImageKind::detect(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageKind::Jpeg)
        );
        assert_eq!(ImageKind::detect(b"GIF89a..."), Some(ImageKind::Gif));
        assert_eq!(
            ImageKind::detect(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some(ImageKind::Webp)
        );
        assert_eq!(
            ImageKind::detect(b"<svg xmlns='x'></svg>"),
            Some(ImageKind::Svg)
        );
        assert_eq!(
            ImageKind::detect(b"\xef\xbb\xbf<?xml version='1.0'?>\n<SVG></SVG>"),
            Some(ImageKind::Svg)
        );
    }

    #[test]
    fn rejects_everything_else() {
        assert_eq!(ImageKind::detect(b""), None);
        assert_eq!(ImageKind::detect(b"just text"), None);
        assert_eq!(ImageKind::detect(b"<html><svg></svg></html>"), None);
        assert_eq!(ImageKind::detect(b"RIFF\x00\x00\x00\x00WAVEfmt "), None);
        assert_eq!(ImageKind::detect(&[0x00, 0x01, 0x02]), None);
    }

    #[test]
    fn mime_and_extension_agree() {
        assert_eq!(ImageKind::Jpeg.mime(), "image/jpeg");
        assert_eq!(ImageKind::Jpeg.extension(), "jpg");
        assert_eq!(ImageKind::Svg.mime(), "image/svg+xml");
    }
}
