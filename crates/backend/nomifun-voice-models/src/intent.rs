//! Persisted explicit KWS rebuild confirmation.
//!
//! The document records the confirmed default group identity (group id plus the
//! four exact path hashes). It is only a recovery target: verification always
//! re-derives hashes from the embedded manifest and the manager baseline, never
//! from this file.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::verifier::{default_kws_group, embedded_manifest, KwsGroup};

pub const REBUILD_INTENT_REL: &str = "kws/.voice-suite-rebuild-intent";
pub const REBUILD_INTENT_TMP_REL: &str = "kws/.voice-suite-rebuild-intent.tmp";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct Document {
    schema_version: u32,
    group_id: String,
    files: HashMap<String, String>,
}

fn document_for(group: &KwsGroup) -> Document {
    Document {
        schema_version: SCHEMA_VERSION,
        group_id: group.group_id.clone(),
        files: group.files.clone(),
    }
}

/// JSON document used by both Rust and the offline Python preparation tool.
pub fn default_document() -> String {
    let document = document_for(default_kws_group());
    let mut text =
        serde_json::to_string(&document).expect("the embedded KWS rebuild intent is serializable");
    text.push('\n');
    text
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "configuration: KWS rebuild intent path must not be a symlink: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "configuration: cannot inspect KWS rebuild intent path {}: {error}",
            path.display()
        )),
    }
}

fn parse_document(bytes: &[u8]) -> Result<Document, String> {
    let document: Document = serde_json::from_slice(bytes)
        .map_err(|error| format!("configuration: invalid KWS rebuild intent: {error}"))?;
    if document.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "configuration: unsupported KWS rebuild intent schema {}",
            document.schema_version
        ));
    }
    let default = default_kws_group();
    if document.group_id != default.group_id || document.files != default.files {
        return Err(
            "configuration: conflicting KWS rebuild intent: only the current supported default group may be rebuilt"
                .into(),
        );
    }
    Ok(document)
}

fn read_document(models_dir: &Path) -> Result<Option<Document>, String> {
    let path = models_dir.join(REBUILD_INTENT_REL);
    reject_symlink(&path)?;
    match std::fs::read(&path) {
        Ok(bytes) => parse_document(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "configuration: cannot read KWS rebuild intent {}: {error}",
            path.display()
        )),
    }
}

/// Returns whether a previously confirmed default-group rebuild still applies.
pub fn confirmed_default_rebuild(models_dir: &Path) -> Result<bool, String> {
    Ok(read_document(models_dir)?.is_some())
}

/// Publishes the confirmed target before the first model write.
///
/// The caller must already hold the shared directory write lease. The document
/// is written to a same-directory temporary file, fsynced, and atomically
/// renamed; symlinks and conflicting intents are rejected.
pub fn persist_default_rebuild(models_dir: &Path) -> Result<(), String> {
    let default = default_kws_group();
    let group_directory = models_dir.join("kws");
    reject_symlink(&group_directory)?;
    std::fs::create_dir_all(&group_directory).map_err(|error| {
        format!(
            "configuration: cannot create KWS directory {}: {error}",
            group_directory.display()
        )
    })?;
    reject_symlink(&group_directory)?;

    let path = models_dir.join(REBUILD_INTENT_REL);
    let temporary = models_dir.join(REBUILD_INTENT_TMP_REL);
    reject_symlink(&path)?;
    reject_symlink(&temporary)?;

    let expected = document_for(default);
    if path.exists() {
        let existing = std::fs::read(&path).map_err(|error| {
            format!(
                "configuration: cannot read KWS rebuild intent {}: {error}",
                path.display()
            )
        })?;
        match parse_document(&existing) {
            Ok(document) if document == expected => {
                remove_if_exists(&temporary)?;
                return Ok(());
            }
            _ => {
                return Err(
                    "configuration: conflicting KWS rebuild intent: only the current supported default group may be rebuilt"
                        .into(),
                );
            }
        }
    }

    remove_if_exists(&temporary)?;
    let text = default_document();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| {
            format!(
                "configuration: cannot create KWS rebuild intent {}: {error}",
                temporary.display()
            )
        })?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            format!(
                "configuration: cannot persist KWS rebuild intent {}: {error}",
                temporary.display()
            )
        })?;
    drop(file);
    std::fs::rename(&temporary, &path).map_err(|error| {
        format!(
            "configuration: cannot publish KWS rebuild intent {}: {error}",
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        if let Ok(directory) = std::fs::File::open(&group_directory) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

/// Removes the intent and any staging copy so an uninstall cannot revive it.
pub fn clear(models_dir: &Path) -> Result<(), String> {
    for relative in [REBUILD_INTENT_REL, REBUILD_INTENT_TMP_REL] {
        remove_if_exists(&models_dir.join(relative))?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "configuration: cannot remove KWS rebuild intent {}: {error}",
            path.display()
        )),
    }
}

/// Verify the document matches the embedded default group without touching disk.
pub fn document_matches_default(document: &str) -> bool {
    serde_json::from_str::<Document>(document)
        .ok()
        .is_some_and(|parsed| parsed == document_for(default_kws_group()))
}

/// Expose the embedded identity for offline-tool parity tests.
pub fn default_identity() -> (String, HashMap<String, String>) {
    let group = embedded_manifest()
        .kws_groups
        .iter()
        .find(|group| group.group_id == embedded_manifest().default_kws_group)
        .expect("default group");
    (group.group_id.clone(), group.files.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_contains_the_default_group_and_all_four_hashes() {
        let document = default_document();
        assert!(document_matches_default(&document));
        let parsed: Document = serde_json::from_str(&document).unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.group_id, default_kws_group().group_id);
        assert_eq!(parsed.files, default_kws_group().files);
        assert_eq!(parsed.files.len(), 4);
    }

    #[test]
    fn intent_cannot_supply_its_own_hashes() {
        let mut document: Document = serde_json::from_str(&default_document()).unwrap();
        document.files.insert("kws/encoder.int8.onnx".into(), "0".repeat(64));
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("kws")).unwrap();
        std::fs::write(directory.path().join(REBUILD_INTENT_REL), serde_json::to_vec(&document).unwrap()).unwrap();
        let error = confirmed_default_rebuild(directory.path()).unwrap_err();
        assert!(error.contains("conflicting"), "{error}");
    }

    #[test]
    fn clear_removes_intent_and_staging_copy() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("kws")).unwrap();
        std::fs::write(directory.path().join(REBUILD_INTENT_REL), default_document()).unwrap();
        std::fs::write(directory.path().join(REBUILD_INTENT_TMP_REL), b"stale").unwrap();
        clear(directory.path()).unwrap();
        assert!(!directory.path().join(REBUILD_INTENT_REL).exists());
        assert!(!directory.path().join(REBUILD_INTENT_TMP_REL).exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_intent_is_rejected() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("kws")).unwrap();
        let outside = directory.path().join("outside-intent");
        std::fs::write(&outside, default_document()).unwrap();
        symlink(&outside, directory.path().join(REBUILD_INTENT_REL)).unwrap();
        let error = confirmed_default_rebuild(directory.path()).unwrap_err();
        assert!(error.contains("symlink"), "{error}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), default_document());
    }
}
