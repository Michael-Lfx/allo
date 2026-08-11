//! Local document-to-Markdown conversion for knowledge-base uploads.

use anydoc::{ConvertError, Format};
use nomifun_api_types::KnowledgeDocumentImportStatus;

/// Maximum generated Markdown size. The existing JSON knowledge upload is
/// bounded by the app's 10 MiB body limit, so conversion keeps the same
/// downstream operating envelope.
pub const MAX_IMPORTED_MARKDOWN_BYTES: usize = 10 * 1024 * 1024;

/// Extensions accepted by the knowledge import picker. `md` is handled as
/// UTF-8 text; every other extension is parsed by AnyDoc.
pub const SUPPORTED_IMPORT_EXTENSIONS: &[&str] = &[
    "md", "doc", "docx", "docm", "ppt", "pps", "pot", "pptx", "pptm", "ppsx", "ppsm",
    "xls", "xlsx", "xlsm", "xlsb", "odt", "ods", "odp", "rtf", "epub", "csv", "pdf",
];

#[derive(Debug)]
pub struct ConversionOutcome {
    pub format: Option<String>,
    pub status: KnowledgeDocumentImportStatus,
    pub markdown: Option<String>,
    pub detail: Option<String>,
}

impl ConversionOutcome {
    fn written(format: Option<String>, markdown: String) -> Self {
        Self { format, status: KnowledgeDocumentImportStatus::Written, markdown: Some(markdown), detail: None }
    }

    fn failed(
        format: Option<String>,
        status: KnowledgeDocumentImportStatus,
        detail: impl Into<String>,
    ) -> Self {
        Self { format, status, markdown: None, detail: Some(detail.into()) }
    }
}

/// Convert uploaded bytes into Markdown. Format markers in the bytes take
/// precedence over the source extension; the extension is the CSV fallback.
pub fn convert_to_markdown(bytes: Vec<u8>, source_path: String) -> ConversionOutcome {
    let extension = source_extension(&source_path);
    let detected = Format::from_bytes(&bytes);
    let hinted = extension.as_deref().and_then(Format::from_extension);

    if detected.is_none() && extension.as_deref().is_some_and(|ext| ext.eq_ignore_ascii_case("md")) {
        return match String::from_utf8(bytes) {
            Ok(markdown) => validate_markdown(Some("markdown".into()), markdown),
            Err(error) => ConversionOutcome::failed(
                Some("markdown".into()),
                KnowledgeDocumentImportStatus::InvalidUtf8,
                error.to_string(),
            ),
        };
    }

    let Some(format) = detected.or(hinted) else {
        return ConversionOutcome::failed(
            None,
            KnowledgeDocumentImportStatus::Unsupported,
            "unrecognized file content and extension",
        );
    };
    let format_name = format_name(format).to_owned();
    match anydoc::to_markdown_bytes(&bytes, format) {
        Ok(markdown) => validate_markdown(Some(format_name), markdown),
        Err(error) => ConversionOutcome::failed(
            Some(format_name),
            status_for_error(&error),
            error.to_string(),
        ),
    }
}

/// Replace the final source extension with `.md`, retaining a folder upload's
/// relative path underneath the selected knowledge-base destination.
pub fn target_markdown_path(source_path: &str, target_folder: &str) -> String {
    let source = source_path.replace('\\', "/").trim_matches('/').to_owned();
    let converted = match source.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => format!("{stem}.md"),
        _ => format!("{source}.md"),
    };
    let folder = target_folder.replace('\\', "/").trim_matches('/').to_owned();
    if folder.is_empty() { converted } else { format!("{folder}/{converted}") }
}

pub fn supports_source_path(source_path: &str) -> bool {
    source_extension(source_path)
        .as_deref()
        .is_some_and(|extension| SUPPORTED_IMPORT_EXTENSIONS.iter().any(|supported| extension.eq_ignore_ascii_case(supported)))
}

fn source_extension(source_path: &str) -> Option<String> {
    source_path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.').map(|(_, ext)| ext.to_owned()))
        .filter(|ext| !ext.is_empty())
}

fn validate_markdown(format: Option<String>, markdown: String) -> ConversionOutcome {
    if markdown.trim().is_empty() {
        return ConversionOutcome::failed(
            format,
            KnowledgeDocumentImportStatus::Malformed,
            "document produced no meaningful Markdown",
        );
    }
    if markdown.len() > MAX_IMPORTED_MARKDOWN_BYTES {
        return ConversionOutcome::failed(
            format,
            KnowledgeDocumentImportStatus::ResourceLimit,
            format!("generated Markdown exceeds {MAX_IMPORTED_MARKDOWN_BYTES} bytes"),
        );
    }
    ConversionOutcome::written(format, markdown)
}

fn status_for_error(error: &ConvertError) -> KnowledgeDocumentImportStatus {
    match error {
        ConvertError::Unsupported(_) => KnowledgeDocumentImportStatus::Unsupported,
        ConvertError::Malformed { .. } | ConvertError::Io(_) => KnowledgeDocumentImportStatus::Malformed,
        ConvertError::Encrypted => KnowledgeDocumentImportStatus::Encrypted,
        ConvertError::ResourceLimit { .. } => KnowledgeDocumentImportStatus::ResourceLimit,
        ConvertError::MissingPart { .. } => KnowledgeDocumentImportStatus::MissingPart,
        _ => KnowledgeDocumentImportStatus::Malformed,
    }
}

fn format_name(format: Format) -> &'static str {
    match format {
        Format::Doc => "doc",
        Format::Docx => "docx",
        Format::Odt => "odt",
        Format::Pdf => "pdf",
        Format::Ppt => "ppt",
        Format::Pptx => "pptx",
        Format::Rtf => "rtf",
        Format::Epub => "epub",
        Format::Excel => "excel",
        Format::Ods => "ods",
        Format::Odp => "odp",
        Format::Csv => "csv",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_document_extension() {
        for extension in SUPPORTED_IMPORT_EXTENSIONS {
            if *extension != "md" {
                assert!(Format::from_extension(extension).is_some(), "{extension}");
            }
        }
    }

    #[test]
    fn markdown_is_preserved_as_utf8() {
        let outcome = convert_to_markdown("# Note\n\n中文".as_bytes().to_vec(), "note.md".into());
        assert_eq!(outcome.status, KnowledgeDocumentImportStatus::Written);
        assert_eq!(outcome.markdown.as_deref(), Some("# Note\n\n中文"));
    }

    #[test]
    fn csv_uses_extension_fallback() {
        let outcome = convert_to_markdown(b"name,score\nAlice,10\n".to_vec(), "scores.csv".into());
        assert_eq!(outcome.status, KnowledgeDocumentImportStatus::Written);
        assert_eq!(outcome.format.as_deref(), Some("csv"));
    }

    #[test]
    fn binary_signature_beats_misleading_extension() {
        let outcome = convert_to_markdown(b"{\\rtf1\\ansi Hello}".to_vec(), "note.md".into());
        assert_eq!(outcome.status, KnowledgeDocumentImportStatus::Written);
        assert_eq!(outcome.format.as_deref(), Some("rtf"));
    }

    #[test]
    fn builds_markdown_target_under_destination() {
        assert_eq!(target_markdown_path("source\\reports\\Q1.PDF", "projects"), "projects/source/reports/Q1.md");
    }

    #[test]
    fn only_allows_declared_source_extensions() {
        assert!(supports_source_path("report.PDF"));
        assert!(supports_source_path("notes.md"));
        assert!(!supports_source_path("payload.exe"));
        assert!(!supports_source_path("extensionless"));
    }
}
