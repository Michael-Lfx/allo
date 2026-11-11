//! Versioned, validated, atomic persistence for flow documents.

use crate::model::{Document, ValidationError};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// The marker that identifies a Flowy document.
pub const FORMAT: &str = "flowy.document";
/// The newest document format this build understands.
pub const VERSION: u32 = 1;
/// The conventional file extension for a Flowy document.
pub const EXTENSION: &str = "flowy.json";

#[derive(Debug, Serialize, Deserialize)]
struct FileEnvelope {
    format: String,
    version: u32,
    document: Document,
}

/// A failure while saving, loading, or validating a document.
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    /// The file could not be read or written.
    #[error("could not access {}: {source}", path.display())]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The operating system error.
        source: io::Error,
    },
    /// The document could not be serialized.
    #[error("could not encode the document: {0}")]
    Encode(#[from] serde_json::Error),
    /// The file is not valid JSON.
    #[error("this file is not valid Flowy JSON: {0}")]
    Decode(serde_json::Error),
    /// The file belongs to another application.
    #[error("this is not a Flowy document (found format {0:?})")]
    WrongFormat(String),
    /// The file was written by a newer version of Flowy.
    #[error(
        "this document uses format version {found}; this build supports up to {supported}"
    )]
    WrongVersion {
        /// The version in the file.
        found: u32,
        /// The newest supported version.
        supported: u32,
    },
    /// The document violates the model's invariants.
    #[error("the document is not valid: {0:?}")]
    Invalid(Vec<ValidationError>),
}

/// Encodes a document as pretty JSON with a trailing newline.
///
/// # Errors
///
/// Returns [`PersistError::Encode`] if serialization fails.
pub fn encode(document: &Document) -> Result<String, PersistError> {
    let envelope = FileEnvelope {
        format: FORMAT.to_string(),
        version: VERSION,
        document: document.clone(),
    };
    let mut json = serde_json::to_string_pretty(&envelope)?;
    json.push('\n');
    Ok(json)
}

/// Decodes and validates a document.
///
/// # Errors
///
/// Returns a [`PersistError`] when the JSON is malformed, belongs to another
/// application, uses an unsupported version, or violates the model invariants.
pub fn decode(json: &str) -> Result<Document, PersistError> {
    let envelope: FileEnvelope =
        serde_json::from_str(json).map_err(PersistError::Decode)?;
    if envelope.format != FORMAT {
        return Err(PersistError::WrongFormat(envelope.format));
    }
    if envelope.version == 0 || envelope.version > VERSION {
        return Err(PersistError::WrongVersion {
            found: envelope.version,
            supported: VERSION,
        });
    }

    let mut document = envelope.document;
    document.normalize_ids();
    document.validate().map_err(PersistError::Invalid)?;
    Ok(document)
}

/// Loads a document from disk.
///
/// # Errors
///
/// Returns [`PersistError::Io`] if the file cannot be read, or any error from
/// [`decode`] if its contents are unsuitable.
pub fn load(path: &Path) -> Result<Document, PersistError> {
    let json = fs::read_to_string(path).map_err(|source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    decode(&json)
}

/// Saves a document atomically through a temporary file in the same directory.
///
/// # Errors
///
/// Returns a [`PersistError`] when encoding or writing fails.
pub fn save(path: &Path, document: &Document) -> Result<(), PersistError> {
    let json = encode(document)?;
    write_atomic(path, json.as_bytes())
}

/// Writes bytes to `path` through a temporary file and rename, so a crash
/// during the write leaves the previous file intact.
///
/// # Errors
///
/// Returns [`PersistError::Io`] if the directory cannot be created or the file
/// cannot be replaced.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PersistError> {
    atomic_write(path, bytes).map_err(|source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes bytes to `path` through a temporary file and rename.
///
/// On Unix the temporary file is created with mode `0o600`. On Windows, an
/// existing destination is removed before rename when needed.
///
/// # Errors
///
/// Returns an [`io::Error`] if the directory cannot be created or the file
/// cannot be replaced.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("flowy");
    let temporary = path.with_file_name(format!(
        ".{file_name}.{}.{stamp}.tmp",
        std::process::id()
    ));

    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);

        match fs::rename(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // Windows does not replace an existing destination with rename.
                fs::remove_file(path)?;
                fs::rename(&temporary, path)
            }
            Err(error) => Err(error),
        }
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vec2;
    use crate::model::NodeKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/flowy-test-data")
            .join(format!("{name}-{}-{stamp}.flowy.json", std::process::id()))
    }

    fn document() -> Document {
        let mut doc = Document::new("Persisted");
        let trigger = doc.add_node(NodeKind::Trigger, Vec2::new(0.0, 0.0));
        let output = doc.add_node(NodeKind::Output, Vec2::new(300.0, 0.0));
        doc.add_edge(trigger, output).unwrap();
        doc
    }

    #[test]
    fn round_trips_through_json() {
        let doc = document();
        let json = encode(&doc).unwrap();
        let decoded = decode(&json).unwrap();
        assert_eq!(decoded, doc);
    }

    #[test]
    fn saves_and_loads_through_the_filesystem() {
        let path = scratch_path("roundtrip");
        let doc = document();
        save(&path, &doc).unwrap();
        assert_eq!(load(&path).unwrap(), doc);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn atomic_write_replaces_existing_content() {
        let path = scratch_path("atomic");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_foreign_formats_and_future_versions() {
        let foreign = r#"{"format":"other","version":1,"document":{"title":"x","nodes":[],"edges":[]}}"#;
        assert!(matches!(decode(foreign), Err(PersistError::WrongFormat(_))));

        let future = r#"{"format":"flowy.document","version":9,"document":{"title":"x","nodes":[],"edges":[]}}"#;
        assert!(matches!(
            decode(future),
            Err(PersistError::WrongVersion { found: 9, .. })
        ));
    }

    #[test]
    fn rejects_structurally_invalid_documents() {
        let invalid = r#"{
            "format": "flowy.document",
            "version": 1,
            "document": {
                "title": "broken",
                "nodes": [{
                    "id": 1,
                    "kind": "action",
                    "title": "A",
                    "description": "",
                    "position": {"x": 0.0, "y": 0.0}
                }],
                "edges": [{"id": 1, "from": 1, "to": 1}]
            }
        }"#;
        assert!(matches!(decode(invalid), Err(PersistError::Invalid(_))));
    }
}
