//! Read-only materialization of an external local-folder knowledge source.
//!
//! A registered local folder remains the user's property. This module scans it
//! without following links, turns supported documents into Markdown under the
//! backend data directory, and leaves all downstream knowledge consumers with
//! one ordinary Markdown root to read.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use nomifun_api_types::{
    KnowledgeDocumentImportStatus, KnowledgeLocalSyncError, KnowledgeLocalSyncState,
    KnowledgeLocalSyncSummary,
};
use nomifun_common::{AppError, TimestampMs, now_ms};
use serde::{Deserialize, Serialize};

use crate::document_import::{self, ConversionOutcome};
use crate::service::portable_writeback_path_identity;
use crate::KB_LOCAL_PROJECTION_REL_DIR;

const MANIFEST_FILE: &str = "manifest.json";
const VIEW_DIR: &str = "view";
const OVERRIDES_DIR: &str = "overrides";
const TOMBSTONES_FILE: &str = "tombstones.json";
const MAX_SYNC_ERRORS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProjectionManifest {
    #[serde(default)]
    version: u8,
    #[serde(default)]
    last_synced_at: Option<TimestampMs>,
    #[serde(default)]
    scanned: u64,
    #[serde(default)]
    entries: BTreeMap<String, ManifestEntry>,
    #[serde(default)]
    errors: Vec<ManifestError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestEntry {
    source_path: String,
    target_path: String,
    size: u64,
    modified_at: Option<TimestampMs>,
    status: ManifestStatus,
    #[serde(default)]
    detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ManifestStatus {
    Written,
    Conflict,
    Unsupported,
    Malformed,
    Encrypted,
    ResourceLimit,
    MissingPart,
    InvalidUtf8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestError {
    source_path: String,
    status: ManifestStatus,
    #[serde(default)]
    detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Tombstones {
    #[serde(default)]
    paths: BTreeSet<String>,
    #[serde(default)]
    restored: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct SourceDocument {
    source_path: String,
    target_path: String,
    absolute_path: PathBuf,
    size: u64,
    modified_at: Option<TimestampMs>,
}

#[derive(Debug, Default)]
struct OverrideEntries {
    directories: BTreeSet<String>,
    files: BTreeSet<String>,
}

/// Per-base paths inside the backend-owned projection directory.
#[derive(Debug, Clone)]
pub struct ProjectionPaths {
    pub root: PathBuf,
    pub view: PathBuf,
    pub overrides: PathBuf,
    manifest: PathBuf,
    tombstones: PathBuf,
}

pub fn paths(data_dir: &Path, kb_id: &str) -> ProjectionPaths {
    let root = data_dir.join(KB_LOCAL_PROJECTION_REL_DIR).join(kb_id);
    ProjectionPaths {
        view: root.join(VIEW_DIR),
        overrides: root.join(OVERRIDES_DIR),
        manifest: root.join(MANIFEST_FILE),
        tombstones: root.join(TOMBSTONES_FILE),
        root,
    }
}

pub fn summary(
    data_dir: &Path,
    kb_id: &str,
    source_available: bool,
) -> KnowledgeLocalSyncSummary {
    let projection = paths(data_dir, kb_id);
    let manifest = read_manifest(&projection.manifest).unwrap_or_default();
    let mut summary = summary_from_manifest(&manifest, source_available);
    if !source_available {
        summary.state = KnowledgeLocalSyncState::Unavailable;
    } else if manifest.last_synced_at.is_none() {
        summary.state = KnowledgeLocalSyncState::Idle;
    }
    summary
}

/// Materialize the external folder synchronously. The caller owns concurrency
/// control because AnyDoc parsing must share the service-wide blocking limit.
pub fn sync(
    data_dir: &Path,
    kb_id: &str,
    source_root: &Path,
) -> Result<KnowledgeLocalSyncSummary, AppError> {
    if !source_root.is_dir() {
        return Ok(KnowledgeLocalSyncSummary {
            state: KnowledgeLocalSyncState::Unavailable,
            last_synced_at: read_manifest(&paths(data_dir, kb_id).manifest)
                .ok()
                .and_then(|manifest| manifest.last_synced_at),
            scanned: 0,
            written: 0,
            conflicts: 0,
            failed: 0,
            errors: Vec::new(),
            source_available: false,
        });
    }

    let projection = paths(data_dir, kb_id);
    std::fs::create_dir_all(&projection.view)
        .map_err(|error| AppError::Internal(format!("failed to create local knowledge view: {error}")))?;
    std::fs::create_dir_all(&projection.overrides)
        .map_err(|error| AppError::Internal(format!("failed to create local knowledge overrides: {error}")))?;

    let tombstones = read_tombstones(&projection.tombstones).unwrap_or_default();
    let source_directories = scan_source_directories(source_root)?;
    let docs = scan_source_documents(source_root)?;
    let mut by_target: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, document) in docs.iter().enumerate() {
        by_target
            .entry(portable_key(&document.target_path))
            .or_default()
            .push(index);
    }

    let mut manifest = ProjectionManifest {
        version: 1,
        last_synced_at: Some(now_ms()),
        scanned: docs.len() as u64,
        entries: BTreeMap::new(),
        errors: Vec::new(),
    };
    let overrides = collect_override_entries(&projection.overrides)?;
    let mut expected_view_paths = overrides.files.clone();
    let mut expected_view_directories = source_directories
        .into_iter()
        .filter(|directory| !is_tombstoned(&tombstones, directory))
        .collect::<BTreeSet<_>>();
    expected_view_directories.extend(overrides.directories.iter().cloned());
    let mut written = 0u64;
    let mut conflicts = 0u64;
    let mut failed = 0u64;

    for document in &docs {
        let target = document.target_path.clone();
        let conflict = by_target
            .get(&portable_key(&target))
            .is_some_and(|same_target| same_target.len() > 1);
        let target = target_for_portable_path(&projection.overrides, &target)?;
        let override_path = projection.overrides.join(&target);
        let view_path = projection.view.join(&target);
        let tombstoned = is_tombstoned(&tombstones, &target);

        if conflict {
            let entry = manifest_entry(document, ManifestStatus::Conflict, Some("more than one source file maps to this Markdown path".into()));
            record_error(&mut manifest, &entry);
            manifest.entries.insert(document.source_path.clone(), entry);
            conflicts += 1;
            continue;
        }
        if tombstoned {
            if override_path.is_file() {
                expected_view_paths.insert(target.clone());
                copy_file(&override_path, &view_path)?;
            }
            continue;
        }
        expected_view_paths.insert(target.clone());

        if override_path.is_file() {
            copy_file(&override_path, &view_path)?;
            manifest.entries.insert(
                document.source_path.clone(),
                manifest_entry(document, ManifestStatus::Written, Some("using app-managed override".into())),
            );
            written += 1;
            continue;
        }

        let outcome = convert_document(document)?;
        let status = manifest_status(outcome.status);
        let entry = manifest_entry(document, status, outcome.detail.clone());
        if let Some(markdown) = outcome.markdown {
            write_text(&view_path, &markdown)?;
            written += 1;
        } else {
            if matches!(status, ManifestStatus::Conflict) {
                conflicts += 1;
            } else {
                failed += 1;
            }
            record_error(&mut manifest, &entry);
            let _ = std::fs::remove_file(&view_path);
        }
        manifest.entries.insert(document.source_path.clone(), entry);
    }

    // Overrides may be app-created documents with no corresponding source
    // file. They are part of the visible Markdown projection as well.
    for directory in &expected_view_directories {
        std::fs::create_dir_all(projection.view.join(directory)).map_err(|error| {
            AppError::Internal(format!("failed to create local knowledge view directory: {error}"))
        })?;
    }
    for target in overrides.files {
        let override_path = projection.overrides.join(&target);
        let view_path = projection.view.join(&target);
        if override_path.is_file() {
            copy_file(&override_path, &view_path)?;
        }
    }

    for path in &expected_view_paths {
        expected_view_directories.extend(parent_directories(path));
    }

    prune_view_files(&projection.view, &expected_view_paths)?;
    prune_view_directories(&projection.view, &expected_view_directories)?;
    write_manifest(&projection.manifest, &manifest)?;

    let mut result = summary_from_manifest(&manifest, true);
    result.written = written;
    result.conflicts = conflicts;
    result.failed = failed;
    result.state = if failed > 0 || conflicts > 0 {
        KnowledgeLocalSyncState::Partial
    } else {
        KnowledgeLocalSyncState::Ready
    };
    Ok(result)
}

fn target_for_portable_path(overrides: &Path, requested: &str) -> Result<String, AppError> {
    let requested_key = portable_writeback_path_identity(requested);
    let mut matches = Vec::new();
    for candidate in collect_override_paths(overrides)? {
        if portable_writeback_path_identity(&candidate) == requested_key {
            matches.push(candidate);
        }
    }
    match matches.len() {
        0 => Ok(requested.to_owned()),
        1 => Ok(matches.remove(0)),
        _ => Err(AppError::Conflict(format!(
            "more than one local knowledge override aliases the Markdown target: {requested}"
        ))),
    }
}

/// Record an app-managed edit/deletion such that a later source sync does not
/// silently resurrect or overwrite it.
pub fn set_tombstone(data_dir: &Path, kb_id: &str, rel_path: &str, deleted: bool) -> Result<(), AppError> {
    let projection = paths(data_dir, kb_id);
    std::fs::create_dir_all(&projection.root)
        .map_err(|error| AppError::Internal(format!("failed to create local projection root: {error}")))?;
    let mut tombstones = read_tombstones(&projection.tombstones).unwrap_or_default();
    if deleted {
        tombstones.paths.insert(rel_path.to_owned());
        tombstones.restored.retain(|restored| {
            restored != rel_path
                && !restored
                    .strip_prefix(rel_path)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        });
    } else {
        tombstones.paths.remove(rel_path);
        tombstones.restored.insert(rel_path.to_owned());
    }
    write_json(&projection.tombstones, &tombstones)
}

pub fn clear_projection(data_dir: &Path, kb_id: &str) {
    let projection = paths(data_dir, kb_id);
    let _ = std::fs::remove_dir_all(projection.root);
}

fn scan_source_documents(root: &Path) -> Result<Vec<SourceDocument>, AppError> {
    let mut documents = Vec::new();
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .filter_entry(|entry| !is_excluded_directory_or_link(entry));
    for entry in walker {
        let entry = entry.map_err(|error| AppError::Internal(format!("failed to scan local folder: {error}")))?;
        if !entry.file_type().is_file() || is_link_or_reparse(entry.path(), entry.metadata().ok().as_ref()) {
            continue;
        }
        let relative = match entry.path().strip_prefix(root) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let Some(source_path) = relative.to_str().map(|_| normalize_relative_path(relative)) else {
            continue;
        };
        if !document_import::supports_source_path(&source_path) {
            continue;
        }
        if !is_safe_relative_path(&source_path) {
            continue;
        }
        let target_path = document_import::target_markdown_path(&source_path, "");
        if !is_safe_relative_path(&target_path) {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| {
            AppError::Internal(format!("failed to inspect local knowledge document {}: {error}", entry.path().display()))
        })?;
        documents.push(SourceDocument {
            source_path,
            target_path,
            absolute_path: entry.path().to_path_buf(),
            size: metadata.len(),
            modified_at: modified_ms(&metadata),
        });
    }
    documents.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok(documents)
}

fn scan_source_directories(root: &Path) -> Result<BTreeSet<String>, AppError> {
    let mut directories = BTreeSet::new();
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .filter_entry(|entry| !is_excluded_directory_or_link(entry));
    for entry in walker {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!("failed to scan local folder directories: {error}"))
        })?;
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            continue;
        }
        let relative = match entry.path().strip_prefix(root) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let Some(relative) = relative.to_str().map(|_| normalize_relative_path(relative)) else {
            continue;
        };
        if is_safe_relative_path(&relative) {
            directories.insert(relative);
        }
    }
    Ok(directories)
}

fn convert_document(document: &SourceDocument) -> Result<ConversionOutcome, AppError> {
    if document.size > document_import::MAX_IMPORTED_SOURCE_BYTES {
        return Ok(ConversionOutcome {
            format: None,
            status: KnowledgeDocumentImportStatus::ResourceLimit,
            markdown: None,
            detail: Some(format!(
                "source document exceeds {} bytes",
                document_import::MAX_IMPORTED_SOURCE_BYTES
            )),
        });
    }
    let bytes = std::fs::read(&document.absolute_path).map_err(|error| {
        AppError::Internal(format!("failed to read local document {}: {error}", document.absolute_path.display()))
    })?;
    Ok(document_import::convert_to_markdown(bytes, document.source_path.clone()))
}

fn manifest_entry(document: &SourceDocument, status: ManifestStatus, detail: Option<String>) -> ManifestEntry {
    ManifestEntry {
        source_path: document.source_path.clone(),
        target_path: document.target_path.clone(),
        size: document.size,
        modified_at: document.modified_at,
        status,
        detail,
    }
}

fn record_error(manifest: &mut ProjectionManifest, entry: &ManifestEntry) {
    if manifest.errors.len() < MAX_SYNC_ERRORS {
        manifest.errors.push(ManifestError {
            source_path: entry.source_path.clone(),
            status: entry.status,
            detail: entry.detail.clone(),
        });
    }
}

fn summary_from_manifest(manifest: &ProjectionManifest, source_available: bool) -> KnowledgeLocalSyncSummary {
    let mut written = 0u64;
    let mut conflicts = 0u64;
    let mut failed = 0u64;
    for entry in manifest.entries.values() {
        match entry.status {
            ManifestStatus::Written => written += 1,
            ManifestStatus::Conflict => conflicts += 1,
            _ => failed += 1,
        }
    }
    let state = if !source_available {
        KnowledgeLocalSyncState::Unavailable
    } else if manifest.last_synced_at.is_none() {
        KnowledgeLocalSyncState::Idle
    } else if failed > 0 || conflicts > 0 {
        KnowledgeLocalSyncState::Partial
    } else {
        KnowledgeLocalSyncState::Ready
    };
    KnowledgeLocalSyncSummary {
        state,
        last_synced_at: manifest.last_synced_at,
        scanned: manifest.scanned,
        written,
        conflicts,
        failed,
        errors: manifest.errors.iter().cloned().map(|error| KnowledgeLocalSyncError {
            source_path: error.source_path,
            status: import_status(error.status),
            detail: error.detail,
        }).collect(),
        source_available,
    }
}

fn manifest_status(status: KnowledgeDocumentImportStatus) -> ManifestStatus {
    match status {
        KnowledgeDocumentImportStatus::Written => ManifestStatus::Written,
        KnowledgeDocumentImportStatus::Conflict => ManifestStatus::Conflict,
        KnowledgeDocumentImportStatus::Unsupported => ManifestStatus::Unsupported,
        KnowledgeDocumentImportStatus::Malformed => ManifestStatus::Malformed,
        KnowledgeDocumentImportStatus::Encrypted => ManifestStatus::Encrypted,
        KnowledgeDocumentImportStatus::ResourceLimit => ManifestStatus::ResourceLimit,
        KnowledgeDocumentImportStatus::MissingPart => ManifestStatus::MissingPart,
        KnowledgeDocumentImportStatus::InvalidUtf8 => ManifestStatus::InvalidUtf8,
    }
}

fn import_status(status: ManifestStatus) -> KnowledgeDocumentImportStatus {
    match status {
        ManifestStatus::Written => KnowledgeDocumentImportStatus::Written,
        ManifestStatus::Conflict => KnowledgeDocumentImportStatus::Conflict,
        ManifestStatus::Unsupported => KnowledgeDocumentImportStatus::Unsupported,
        ManifestStatus::Malformed => KnowledgeDocumentImportStatus::Malformed,
        ManifestStatus::Encrypted => KnowledgeDocumentImportStatus::Encrypted,
        ManifestStatus::ResourceLimit => KnowledgeDocumentImportStatus::ResourceLimit,
        ManifestStatus::MissingPart => KnowledgeDocumentImportStatus::MissingPart,
        ManifestStatus::InvalidUtf8 => KnowledgeDocumentImportStatus::InvalidUtf8,
    }
}

fn read_manifest(path: &Path) -> Result<ProjectionManifest, AppError> {
    read_json(path)
}

fn write_manifest(path: &Path, manifest: &ProjectionManifest) -> Result<(), AppError> {
    write_json(path, manifest)
}

fn read_tombstones(path: &Path) -> Result<Tombstones, AppError> {
    read_json(path)
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T, AppError> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).map_err(|error| {
            AppError::Internal(format!("local knowledge projection metadata is invalid: {error}"))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(AppError::Internal(format!("failed to read local knowledge projection metadata: {error}"))),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let serialized = serde_json::to_vec_pretty(value)
        .map_err(|error| AppError::Internal(format!("failed to encode local knowledge projection metadata: {error}")))?;
    write_bytes(path, &serialized)
}

fn write_text(path: &Path, content: &str) -> Result<(), AppError> {
    write_bytes(path, content.as_bytes())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| AppError::Internal("local projection path has no parent".into()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| AppError::Internal(format!("failed to create local projection directory: {error}")))?;
    let temp = path.with_extension(format!("tmp-{}", nomifun_common::generate_id()));
    std::fs::write(&temp, bytes)
        .map_err(|error| AppError::Internal(format!("failed to write local projection file: {error}")))?;
    publish_replace(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        AppError::Internal(format!("failed to publish local projection file: {error}"))
    })
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), AppError> {
    let bytes = std::fs::read(source)
        .map_err(|error| AppError::Internal(format!("failed to read local override: {error}")))?;
    write_bytes(destination, &bytes)
}

fn prune_view_files(view: &Path, expected: &BTreeSet<String>) -> Result<(), AppError> {
    if !view.is_dir() {
        return Ok(());
    }
    let files = walkdir::WalkDir::new(view)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect::<Vec<_>>();
    for file in files {
        let Ok(relative) = file.strip_prefix(view) else { continue };
        let Some(relative) = relative.to_str().map(|_| normalize_relative_path(relative)) else { continue };
        if !expected.contains(&relative) {
            let _ = std::fs::remove_file(file);
        }
    }
    Ok(())
}

fn prune_view_directories(root: &Path, expected: &BTreeSet<String>) -> Result<(), AppError> {
    let mut directories = walkdir::WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_dir())
        .map(|entry| entry.path().to_path_buf())
        .collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        let Ok(relative) = directory.strip_prefix(root) else { continue };
        let Some(relative) = relative.to_str().map(|_| normalize_relative_path(relative)) else {
            continue;
        };
        if !expected.contains(&relative) {
            std::fs::remove_dir_all(&directory).map_err(|error| {
                AppError::Internal(format!("failed to prune local knowledge view directory: {error}"))
            })?;
        }
    }
    Ok(())
}

/// `WalkDir::filter_entry` controls descent, so regular files must stay in
/// the iterator. Only directories that should not be traversed (and links)
/// are filtered here; callers still decide which files to consume.
fn is_excluded_directory_or_link(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    if is_link_or_reparse(entry.path(), std::fs::symlink_metadata(entry.path()).ok().as_ref()) {
        return true;
    }
    entry.file_type().is_dir()
        && entry.file_name().to_str().is_none_or(|name| {
            let key = portable_key(name);
            key.starts_with('.') || key == "node_modules" || key == "_trash"
        })
}

fn is_safe_relative_path(path: &str) -> bool {
    let path = path.trim_matches('/');
    !path.is_empty()
        && path.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && !component.ends_with([' ', '.'])
                && !is_windows_reserved_component(component)
                && !component.chars().any(|character| {
                    character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                })
        })
}

fn is_windows_reserved_component(component: &str) -> bool {
    let key = portable_key(component);
    let stem = key.split_once('.').map(|(stem, _)| stem).unwrap_or(&key);
    matches!(stem, "con" | "prn" | "aux" | "nul")
        || stem
            .strip_prefix("com")
            .or_else(|| stem.strip_prefix("lpt"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn is_tombstoned(tombstones: &Tombstones, path: &str) -> bool {
    let deleted = tombstones.paths.iter().any(|entry| {
        entry == path
            || path
                .strip_prefix(entry)
                .is_some_and(|rest| rest.starts_with('/'))
    });
    deleted
        && !tombstones.restored.contains(path)
}

fn collect_override_entries(overrides: &Path) -> Result<OverrideEntries, AppError> {
    if !overrides.is_dir() {
        return Ok(OverrideEntries::default());
    }
    let mut result = OverrideEntries::default();
    for entry in walkdir::WalkDir::new(overrides)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_excluded_directory_or_link(entry))
    {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!("failed to inspect local knowledge overrides: {error}"))
        })?;
        if is_link_or_reparse(entry.path(), entry.metadata().ok().as_ref()) {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(overrides) else {
            continue;
        };
        let relative = normalize_relative_path(relative);
        if !is_safe_relative_path(&relative) {
            continue;
        }
        if entry.file_type().is_dir() {
            result.directories.insert(relative);
        } else if entry.file_type().is_file() {
            result.files.insert(relative);
        }
    }
    Ok(result)
}

fn collect_override_paths(overrides: &Path) -> Result<BTreeSet<String>, AppError> {
    Ok(collect_override_entries(overrides)?.files)
}

#[cfg(not(windows))]
fn publish_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn publish_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    let source = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let destination = destination.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: both paths are nul-terminated UTF-16 buffers valid for this call.
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").trim_matches('/').to_owned()
}

fn parent_directories(path: &str) -> impl Iterator<Item = String> + '_ {
    path.split('/').scan(Vec::new(), |segments, segment| {
        segments.push(segment);
        Some(segments.join("/"))
    })
    .collect::<Vec<_>>()
    .into_iter()
    .take_while(move |candidate| candidate != path)
}

fn portable_key(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    value.nfc().flat_map(char::to_lowercase).collect()
}

fn modified_ms(metadata: &std::fs::Metadata) -> Option<TimestampMs> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as TimestampMs)
}

#[cfg(not(windows))]
fn is_link_or_reparse(_path: &Path, metadata: Option<&std::fs::Metadata>) -> bool {
    metadata.is_some_and(|metadata| metadata.file_type().is_symlink())
}

#[cfg(windows)]
fn is_link_or_reparse(_path: &Path, metadata: Option<&std::fs::Metadata>) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.is_some_and(|metadata| {
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & 0x0000_0400 != 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let source_root = temp.path().join("source");
        std::fs::create_dir_all(&source_root).unwrap();
        (temp, data_dir, source_root)
    }

    fn write_source(root: &Path, rel_path: &str, content: impl AsRef<[u8]>) {
        let path = root.join(rel_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn file_snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
        walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                let rel_path = normalize_relative_path(entry.path().strip_prefix(root).unwrap());
                (rel_path, std::fs::read(entry.path()).unwrap())
            })
            .collect()
    }

    #[test]
    fn projection_paths_are_owned_by_data_directory() {
        let paths = paths(Path::new("/data"), "kb");
        assert_eq!(paths.view, PathBuf::from("/data").join(KB_LOCAL_PROJECTION_REL_DIR).join("kb").join("view"));
    }

    #[test]
    fn rejects_unsafe_relative_paths() {
        assert!(is_safe_relative_path("docs/guide.docx"));
        assert!(!is_safe_relative_path("../guide.docx"));
        assert!(!is_safe_relative_path("docs/CON.docx"));
    }

    #[test]
    fn sync_projects_markdown_and_csv_without_modifying_the_external_folder() {
        let (_temp, data_dir, source_root) = fixture();
        write_source(&source_root, "notes/Guide.MD", "# Original guide\n\nKeep this text.");
        write_source(&source_root, "tables/people.csv", "name,score\nAlice,10\nBob,8\n");
        write_source(&source_root, "ignored.txt", "this is not a supported document");
        let before = file_snapshot(&source_root);

        let result = sync(&data_dir, "kb", &source_root).unwrap();
        let projection = paths(&data_dir, "kb");

        assert_eq!(result.state, KnowledgeLocalSyncState::Ready);
        assert_eq!(result.scanned, 2);
        assert_eq!(result.written, 2);
        assert_eq!(result.conflicts, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(
            std::fs::read_to_string(projection.view.join("notes/Guide.md")).unwrap(),
            "# Original guide\n\nKeep this text."
        );
        let csv_projection = std::fs::read_to_string(projection.view.join("tables/people.md")).unwrap();
        assert!(csv_projection.contains("Alice"));
        assert!(csv_projection.contains("Bob"));
        assert!(!projection.view.join("tables/people.csv").exists());
        assert!(!projection.view.join("ignored.txt").exists());
        assert_eq!(file_snapshot(&source_root), before, "sync must not alter the external source folder");
    }

    #[test]
    fn sync_reports_each_source_that_maps_to_the_same_markdown_target_as_a_conflict() {
        let (_temp, data_dir, source_root) = fixture();
        write_source(&source_root, "report.md", "# Markdown source");
        write_source(&source_root, "report.csv", "name,score\nAlice,10\n");
        let before = file_snapshot(&source_root);

        let result = sync(&data_dir, "kb", &source_root).unwrap();
        let projection = paths(&data_dir, "kb");

        assert_eq!(result.state, KnowledgeLocalSyncState::Partial);
        assert_eq!(result.scanned, 2);
        assert_eq!(result.written, 0);
        assert_eq!(result.conflicts, 2);
        assert_eq!(result.failed, 0);
        assert_eq!(result.errors.len(), 2);
        assert!(result
            .errors
            .iter()
            .all(|error| error.status == KnowledgeDocumentImportStatus::Conflict));
        assert!(!projection.view.join("report.md").exists());
        assert_eq!(file_snapshot(&source_root), before, "conflict detection must be read-only");
    }

    #[test]
    fn tombstones_hide_source_documents_and_can_be_cleared_on_a_later_sync() {
        let (_temp, data_dir, source_root) = fixture();
        write_source(&source_root, "docs/keep.md", "# Keep");
        write_source(&source_root, "docs/remove.md", "# Remove");
        let before = file_snapshot(&source_root);

        sync(&data_dir, "kb", &source_root).unwrap();
        set_tombstone(&data_dir, "kb", "docs/remove.md", true).unwrap();
        let removed = sync(&data_dir, "kb", &source_root).unwrap();
        let projection = paths(&data_dir, "kb");

        assert_eq!(removed.state, KnowledgeLocalSyncState::Ready);
        assert_eq!(removed.scanned, 2, "the source scan count must not hide tombstoned documents");
        assert_eq!(removed.written, 1);
        assert!(projection.view.join("docs/keep.md").is_file());
        assert!(!projection.view.join("docs/remove.md").exists());
        assert_eq!(file_snapshot(&source_root), before, "tombstones must not delete source files");

        set_tombstone(&data_dir, "kb", "docs/remove.md", false).unwrap();
        let restored = sync(&data_dir, "kb", &source_root).unwrap();
        assert_eq!(restored.scanned, 2);
        assert_eq!(restored.written, 2);
        assert_eq!(
            std::fs::read_to_string(projection.view.join("docs/remove.md")).unwrap(),
            "# Remove"
        );
    }

    #[test]
    fn directory_tombstones_hide_every_source_document_below_that_directory() {
        let (_temp, data_dir, source_root) = fixture();
        write_source(&source_root, "docs/a.md", "# A");
        write_source(&source_root, "docs/nested/b.md", "# B");
        write_source(&source_root, "elsewhere.md", "# Elsewhere");

        set_tombstone(&data_dir, "kb", "docs", true).unwrap();
        let result = sync(&data_dir, "kb", &source_root).unwrap();
        let projection = paths(&data_dir, "kb");

        assert_eq!(result.scanned, 3);
        assert_eq!(result.written, 1);
        assert!(!projection.view.join("docs/a.md").exists());
        assert!(!projection.view.join("docs/nested/b.md").exists());
        assert_eq!(std::fs::read_to_string(projection.view.join("elsewhere.md")).unwrap(), "# Elsewhere");
    }

    #[test]
    fn restoring_a_child_clears_only_its_ancestor_directory_tombstone() {
        let (_temp, data_dir, source_root) = fixture();
        write_source(&source_root, "docs/a.md", "# A");
        write_source(&source_root, "docs/b.md", "# B");
        set_tombstone(&data_dir, "kb", "docs", true).unwrap();
        set_tombstone(&data_dir, "kb", "docs/a.md", false).unwrap();

        sync(&data_dir, "kb", &source_root).unwrap();
        let projection = paths(&data_dir, "kb");
        assert!(projection.view.join("docs/a.md").is_file());
        assert!(!projection.view.join("docs/b.md").exists());
    }

    #[test]
    fn overrides_win_over_the_source_and_override_only_files_are_materialized() {
        let (_temp, data_dir, source_root) = fixture();
        write_source(&source_root, "guide.md", "# Source version");
        let before = file_snapshot(&source_root);
        sync(&data_dir, "kb", &source_root).unwrap();

        let projection = paths(&data_dir, "kb");
        write_source(&projection.overrides, "guide.md", "# Edited in the app");
        write_source(&projection.overrides, "drafts/local.md", "# Local-only draft");

        let result = sync(&data_dir, "kb", &source_root).unwrap();

        assert_eq!(result.state, KnowledgeLocalSyncState::Ready);
        assert_eq!(std::fs::read_to_string(projection.view.join("guide.md")).unwrap(), "# Edited in the app");
        assert_eq!(
            std::fs::read_to_string(projection.view.join("drafts/local.md")).unwrap(),
            "# Local-only draft"
        );
        assert_eq!(file_snapshot(&source_root), before, "overrides must never be written back to the source folder");
    }

    #[test]
    fn invalid_utf8_markdown_is_reported_as_a_non_fatal_import_error() {
        let (_temp, data_dir, source_root) = fixture();
        write_source(&source_root, "broken.md", [0xff, 0xfe, 0x00]);

        let result = sync(&data_dir, "kb", &source_root).unwrap();

        assert_eq!(result.state, KnowledgeLocalSyncState::Partial);
        assert_eq!(result.scanned, 1);
        assert_eq!(result.written, 0);
        assert_eq!(result.failed, 1);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].source_path, "broken.md");
        assert_eq!(result.errors[0].status, KnowledgeDocumentImportStatus::InvalidUtf8);
        assert!(!paths(&data_dir, "kb").view.join("broken.md").exists());
    }
}
