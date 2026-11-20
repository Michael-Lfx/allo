//! Extension-to-file-icon mapping shared by the resource library and the
//! chat file cards. Classification is purely lexical: nothing here touches
//! the filesystem.

use flowy_ui as theme;

/// The file-type artwork a name maps onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    Code,
    Document,
    Java,
    Image,
    Audio,
    Video,
    Pdf,
    Presentation,
    Spreadsheet,
    Archive,
}

/// Classifies `path` by its extension alone. Only the last path segment
/// counts, so `notes/report.pdf` resolves to [`FileKind::Pdf`]. A name with
/// no known extension returns `None`, which lets the caller fall back to its
/// own category.
pub(crate) fn from_path(path: &str) -> Option<FileKind> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())?;
    match extension.as_str() {
        "java" => Some(FileKind::Java),
        "c" | "cc" | "cpp" | "cs" | "css" | "go" | "h" | "hpp" | "htm"
        | "html" | "js" | "json" | "jsx" | "kt" | "kts" | "log" | "md"
        | "markdown" | "php" | "ps1" | "py" | "rb" | "rs" | "scss"
        | "sh" | "swift" | "toml" | "ts" | "tsx" | "txt" | "xml"
        | "yaml" | "yml" => Some(FileKind::Code),
        "doc" | "docx" | "odt" | "rtf" => Some(FileKind::Document),
        "bmp" | "gif" | "jpeg" | "jpg" | "png" | "svg" | "webp"
        | "avif" => Some(FileKind::Image),
        "aac" | "flac" | "m4a" | "mp3" | "ogg" | "wav" => Some(FileKind::Audio),
        "avi" | "mkv" | "mov" | "mp4" | "webm" => Some(FileKind::Video),
        "pdf" => Some(FileKind::Pdf),
        "odp" | "ppt" | "pptx" => Some(FileKind::Presentation),
        "csv" | "ods" | "xls" | "xlsx" => Some(FileKind::Spreadsheet),
        "7z" | "bz2" | "gz" | "rar" | "tar" | "tgz" | "xz" | "zip" => {
            Some(FileKind::Archive)
        }
        _ => None,
    }
}

impl FileKind {
    /// `IconPark` file-type artwork copied from the parent UI assets so
    /// library documents and chat deliverables use the same language.
    pub(crate) fn icon(self) -> gpui::Img {
        match self {
            Self::Code => theme::icons::code_file(),
            Self::Document => theme::icons::docx_file(),
            Self::Java => theme::icons::java_file(),
            Self::Image => theme::icons::jpg_file(),
            Self::Audio => theme::icons::mp3_file(),
            Self::Video => theme::icons::mp4_file(),
            Self::Pdf => theme::icons::pdf_file(),
            Self::Presentation => theme::icons::ppt_file(),
            Self::Spreadsheet => theme::icons::xls_file(),
            Self::Archive => theme::icons::zip_file(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FileKind, from_path};

    #[test]
    fn extensions_map_onto_the_shared_kinds() {
        let cases = [
            ("main.rs", FileKind::Code),
            ("index.html", FileKind::Code),
            ("app.js", FileKind::Code),
            ("notes.md", FileKind::Code),
            ("Main.java", FileKind::Java),
            ("report.docx", FileKind::Document),
            ("hero.png", FileKind::Image),
            ("voice.mp3", FileKind::Audio),
            ("teaser.mp4", FileKind::Video),
            ("guide.pdf", FileKind::Pdf),
            ("deck.pptx", FileKind::Presentation),
            ("budget.xlsx", FileKind::Spreadsheet),
            ("ledger.csv", FileKind::Spreadsheet),
            ("assets.zip", FileKind::Archive),
            ("assets.7z", FileKind::Archive),
            ("archive.tar.gz", FileKind::Archive),
        ];
        for (path, kind) in cases {
            assert_eq!(from_path(path), Some(kind), "{path}");
        }
    }

    #[test]
    fn extensions_are_case_insensitive() {
        assert_eq!(from_path("REPORTS.PDF"), Some(FileKind::Pdf));
        assert_eq!(from_path("Hero.JPG"), Some(FileKind::Image));
    }

    #[test]
    fn unknown_extensions_stay_unclassified() {
        for path in [
            "README",
            "1104-byd-presentation",
            "notes.unknown",
            ".gitignore",
            "",
        ] {
            assert_eq!(from_path(path), None, "{path:?}");
        }
    }

    #[test]
    fn directories_do_not_decide_the_kind() {
        assert_eq!(from_path("docs/guide.pdf"), Some(FileKind::Pdf));
        assert_eq!(
            from_path(r"C:\Users\me\paper.docx"),
            Some(FileKind::Document)
        );
    }
}
