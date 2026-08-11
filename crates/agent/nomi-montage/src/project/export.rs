//! `.nomimontage` project archive — zip of `project.json` + the whole project
//! directory tree (artifacts / assets / renders / pipeline / history / events).

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::error::{MontageError, MontageResult};
use crate::paths::ProjectPaths;

pub const ARCHIVE_APP: &str = "nomi-montage";
pub const ARCHIVE_VERSION: u32 = 1;
pub const ARCHIVE_EXTENSION: &str = "nomimontage";
pub const MANIFEST_ENTRY: &str = "manifest.json";
pub const PROJECT_PREFIX: &str = "project/";

const MAX_ENTRIES: usize = 50_000;
const MAX_ENTRY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveManifest {
    pub version: u32,
    pub app: String,
    pub exported_at: String,
    pub project_id: String,
}

/// Stream-export a project directory tree into `dest_path` (`.nomimontage`).
pub fn export_project_zip(paths: &ProjectPaths, project_id: &str, dest_path: &Path) -> MontageResult<PathBuf> {
    if !paths.root.is_dir() {
        return Err(MontageError::ProjectNotFound(project_id.to_string()));
    }
    let dest = normalize_dest_path(dest_path)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension(format!("{ARCHIVE_EXTENSION}.tmp"));
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }

    let file = File::create(&tmp)?;
    let mut zip = ZipWriter::new(file);

    let manifest = ArchiveManifest {
        version: ARCHIVE_VERSION,
        app: ARCHIVE_APP.into(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        project_id: project_id.to_string(),
    };
    write_json_entry(&mut zip, MANIFEST_ENTRY, &manifest)?;

    let mut file_count = 0usize;
    let mut total_bytes: u64 = 0;
    for entry in WalkDir::new(&paths.root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.file_type().is_symlink() {
            return Err(MontageError::InvalidParams(format!(
                "refusing to export symlink: {}",
                path.display()
            )));
        }
        if !meta.is_file() {
            continue;
        }
        file_count += 1;
        if file_count > MAX_ENTRIES {
            let _ = fs::remove_file(&tmp);
            return Err(MontageError::InvalidParams(format!("project has too many files to export (>{MAX_ENTRIES})")));
        }
        let size = meta.len();
        if size > MAX_ENTRY_BYTES {
            let _ = fs::remove_file(&tmp);
            return Err(MontageError::InvalidParams(format!("file too large to export: {}", path.display())));
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_TOTAL_BYTES {
            let _ = fs::remove_file(&tmp);
            return Err(MontageError::InvalidParams(format!(
                "project exceeds export size budget ({MAX_TOTAL_BYTES} bytes)"
            )));
        }
        let rel = path
            .strip_prefix(&paths.root)
            .map_err(|_| MontageError::msg("path escapes project root during export"))?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.is_empty() || rel.contains("..") {
            let _ = fs::remove_file(&tmp);
            return Err(MontageError::InvalidParams(format!("invalid relative path during export: {rel}")));
        }
        let entry_name = format!("{PROJECT_PREFIX}{rel}");
        let opts = SimpleFileOptions::default().compression_method(compression_for(&rel));
        zip.start_file(&entry_name, opts)
            .map_err(|e| MontageError::Zip(format!("start_file {entry_name}: {e}")))?;
        let mut src = File::open(path)?;
        io::copy(&mut src, &mut zip)?;
    }

    zip.finish().map_err(|e| MontageError::Zip(format!("finish: {e}")))?;
    if dest.exists() {
        fs::remove_file(&dest)?;
    }
    fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// Extract `project/**` into `dest_root` (caller decides the new project id / paths).
pub fn import_project_zip(archive_path: &Path, dest_root: &Path) -> MontageResult<ArchiveManifest> {
    if !archive_path.is_file() {
        return Err(MontageError::InvalidParams(format!("archive not found: {}", archive_path.display())));
    }
    if dest_root.exists() {
        fs::remove_dir_all(dest_root)?;
    }
    fs::create_dir_all(dest_root)?;

    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(file).map_err(|e| MontageError::InvalidParams(format!("invalid project archive: {e}")))?;

    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut total: u64 = 0;
    let mut file_count = 0usize;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| MontageError::InvalidParams(format!("zip entry {i}: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            return Err(MontageError::InvalidParams("archive entry escapes archive root".into()));
        };
        let name = enclosed.to_string_lossy().replace('\\', "/");
        if name.contains('\0') || name.starts_with('/') || name.starts_with("../") {
            return Err(MontageError::InvalidParams(format!("unsafe archive entry: {name}")));
        }

        file_count += 1;
        if file_count > MAX_ENTRIES {
            return Err(MontageError::InvalidParams(format!("archive has too many entries (>{MAX_ENTRIES})")));
        }
        let cap = MAX_ENTRY_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(total));

        if name == MANIFEST_ENTRY {
            let mut buf = Vec::new();
            (&mut entry).take(cap.saturating_add(1)).read_to_end(&mut buf)?;
            if buf.len() as u64 > cap {
                return Err(MontageError::InvalidParams("manifest.json exceeds decompression budget".into()));
            }
            total = total.saturating_add(buf.len() as u64);
            manifest_bytes = Some(buf);
            continue;
        }
        if let Some(rel) = name.strip_prefix(PROJECT_PREFIX) {
            if rel.is_empty() || rel.contains("..") {
                return Err(MontageError::InvalidParams(format!("unsafe project path in archive: {name}")));
            }
            let dest = dest_root.join(rel);
            if !dest.starts_with(dest_root) {
                return Err(MontageError::InvalidParams(format!("project path escapes destination: {name}")));
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            total = total.saturating_add(stream_copy_bounded(&mut entry, &dest, cap)?);
            continue;
        }
        total = total.saturating_add(discard_bounded(&mut entry, cap)?);
    }

    let manifest_raw = manifest_bytes.ok_or_else(|| MontageError::InvalidParams("archive missing manifest.json".into()))?;
    let manifest: ArchiveManifest = serde_json::from_slice(&manifest_raw)
        .map_err(|e| MontageError::InvalidParams(format!("invalid manifest.json: {e}")))?;
    if manifest.app != ARCHIVE_APP {
        return Err(MontageError::InvalidParams(format!(
            "unsupported archive app '{}'; expected '{ARCHIVE_APP}'",
            manifest.app
        )));
    }
    if manifest.version == 0 || manifest.version > ARCHIVE_VERSION {
        return Err(MontageError::InvalidParams(format!("unsupported archive version {}", manifest.version)));
    }
    Ok(manifest)
}

fn stream_copy_bounded<R: Read>(reader: &mut R, dest: &Path, cap: u64) -> MontageResult<u64> {
    let mut out = File::create(dest)?;
    let mut limited = reader.take(cap.saturating_add(1));
    let written = io::copy(&mut limited, &mut out)?;
    if written > cap {
        let _ = fs::remove_file(dest);
        return Err(MontageError::InvalidParams("archive entry exceeds decompression budget".into()));
    }
    Ok(written)
}

fn discard_bounded<R: Read>(reader: &mut R, cap: u64) -> MontageResult<u64> {
    let mut limited = reader.take(cap.saturating_add(1));
    let written = io::copy(&mut limited, &mut io::sink())?;
    if written > cap {
        return Err(MontageError::InvalidParams("archive entry exceeds decompression budget".into()));
    }
    Ok(written)
}

fn write_json_entry<T: Serialize>(zip: &mut ZipWriter<File>, name: &str, value: &T) -> MontageResult<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(name, opts).map_err(|e| MontageError::Zip(format!("start_file {name}: {e}")))?;
    zip.write_all(&bytes)?;
    Ok(())
}

fn compression_for(rel: &str) -> CompressionMethod {
    match Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "webm" | "mov" | "mkv" | "avi" | "png" | "jpg" | "jpeg" | "webp" | "gif" | "mp3" | "wav" | "aac" | "m4a" => {
            CompressionMethod::Stored
        }
        _ => CompressionMethod::Deflated,
    }
}

fn normalize_dest_path(dest: &Path) -> MontageResult<PathBuf> {
    let mut out = dest.to_path_buf();
    let has_ext = out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
        .unwrap_or(false);
    if !has_ext {
        out.set_extension(ARCHIVE_EXTENSION);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_import_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), "proj-1");
        paths.ensure_dirs().unwrap();
        std::fs::write(paths.artifact_path("script"), b"{\"title\":\"x\"}").unwrap();

        let archive = dir.path().join("out.nomimontage");
        export_project_zip(&paths, "proj-1", &archive).unwrap();
        assert!(archive.is_file());

        let staging = dir.path().join("staging");
        let manifest = import_project_zip(&archive, &staging).unwrap();
        assert_eq!(manifest.project_id, "proj-1");
        assert!(staging.join("artifacts").join("script.json").is_file());
    }

    #[test]
    fn rejects_wrong_app() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("evil.nomimontage");
        {
            let file = File::create(&bad).unwrap();
            let mut zip = ZipWriter::new(file);
            let opts = SimpleFileOptions::default();
            zip.start_file(MANIFEST_ENTRY, opts).unwrap();
            zip.write_all(br#"{"version":1,"app":"not-montage","exported_at":"t","project_id":"x"}"#).unwrap();
            zip.finish().unwrap();
        }
        let err = import_project_zip(&bad, &dir.path().join("s")).unwrap_err();
        assert!(err.to_string().contains("unsupported archive app"));
    }
}
