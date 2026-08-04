//! Shared classification for content that the local extractor deliberately
//! defers to the managed remote provider.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferredDocumentKind {
    Pdf,
    UnsupportedDocument,
}

/// Classify only binary/document responses. Text and known structured web
/// responses stay on the normal local HTML path.
pub(crate) fn classify_document(
    content_type: Option<&str>,
    body_prefix: &[u8],
) -> Option<DeferredDocumentKind> {
    if body_prefix.starts_with(b"%PDF-") {
        return Some(DeferredDocumentKind::Pdf);
    }

    let content_type = content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    match content_type.as_deref() {
        Some("application/pdf" | "application/x-pdf" | "application/acrobat") => {
            Some(DeferredDocumentKind::Pdf)
        }
        Some(value)
            if !value.starts_with("text/")
                && !matches!(
                    value,
                    "application/xhtml+xml"
                        | "application/json"
                        | "application/xml"
                        | "application/x-javascript"
                        | "application/javascript"
                )
                && !value.ends_with("+json")
                && !value.ends_with("+xml") =>
        {
            Some(DeferredDocumentKind::UnsupportedDocument)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_pdf_by_magic_or_mime() {
        assert_eq!(
            classify_document(None, b"%PDF-1.7"),
            Some(DeferredDocumentKind::Pdf)
        );
        assert_eq!(
            classify_document(Some("APPLICATION/PDF; charset=binary"), b"not html"),
            Some(DeferredDocumentKind::Pdf)
        );
    }

    #[test]
    fn leaves_web_and_structured_content_local() {
        for content_type in [
            None,
            Some("text/html"),
            Some("application/xhtml+xml"),
            Some("application/json"),
            Some("application/ld+json"),
            Some("application/xml"),
            Some("application/atom+xml"),
            Some("application/javascript"),
        ] {
            assert_eq!(classify_document(content_type, b"<html>"), None);
        }
    }

    #[test]
    fn classifies_other_application_content_as_unsupported() {
        assert_eq!(
            classify_document(Some("application/msword"), b"binary"),
            Some(DeferredDocumentKind::UnsupportedDocument)
        );
    }

    #[test]
    fn preserves_unsupported_non_text_binary_classification() {
        for content_type in ["image/png", "audio/mpeg", "video/mp4", "font/woff2"] {
            assert_eq!(
                classify_document(Some(content_type), b"binary"),
                Some(DeferredDocumentKind::UnsupportedDocument),
                "{content_type} must remain local-only"
            );
        }
    }
}
