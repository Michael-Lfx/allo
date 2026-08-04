//! Session project archive (`.nomivimax`) — stream zip of metadata + working tree.
//!
//! Layout:
//! - `manifest.json` — `{ version, app, exported_at, session_id, workflow, title }`
//! - `session.json`  — full [`SessionRecord`] snapshot
//! - `working/**`    — entire session working directory (all generated assets)

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use zip::CompressionMethod;
use zip::read::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::domain::WorkflowKind;
use crate::error::{VimaxError, VimaxResult};
use crate::progress::RunStatus;
use crate::session::SessionRecord;

pub const ARCHIVE_APP: &str = "nomifun-vimax";
pub const ARCHIVE_VERSION: u32 = 1;
pub const ARCHIVE_EXTENSION: &str = "nomivimax";

pub const MANIFEST_ENTRY: &str = "manifest.json";
pub const SESSION_ENTRY: &str = "session.json";
pub const WORKING_PREFIX: &str = "working/";

/// Cap on archive member count (directories are skipped; only files count).
const MAX_ENTRIES: usize = 50_000;
/// Per-entry decompressed-size ceiling.
const MAX_ENTRY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Cumulative decompressed-size ceiling (zip-bomb guard).
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveManifest {
    pub version: u32,
    pub app: String,
    pub exported_at: String,
    pub session_id: String,
    pub workflow: String,
    #[serde(default)]
    pub title: String,
}

/// Stream-export a session working tree + metadata into `dest_path` (`.nomivimax`).
pub fn export_session_to_path(
    session: &SessionRecord,
    working_dir: &Path,
    dest_path: &Path,
) -> VimaxResult<PathBuf> {
    if !working_dir.is_dir() {
        return Err(VimaxError::InvalidParams(format!(
            "working directory missing: {}",
            working_dir.display()
        )));
    }
    let dest = normalize_dest_path(dest_path)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("nomivimax.tmp");
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }

    let file = File::create(&tmp)?;
    let mut zip = ZipWriter::new(file);

    let manifest = ArchiveManifest {
        version: ARCHIVE_VERSION,
        app: ARCHIVE_APP.into(),
        exported_at: chrono::Local::now().to_rfc3339(),
        session_id: session.session_id.clone(),
        workflow: session.workflow.as_str().to_string(),
        title: session.title.clone(),
    };
    write_json_entry(&mut zip, MANIFEST_ENTRY, &manifest)?;
    write_json_entry(&mut zip, SESSION_ENTRY, session)?;

    let mut file_count = 0usize;
    let mut total_bytes: u64 = 0;
    for entry in WalkDir::new(working_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            return Err(VimaxError::InvalidParams(format!(
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
            return Err(VimaxError::InvalidParams(format!(
                "project has too many files to export (>{MAX_ENTRIES})"
            )));
        }
        let size = meta.len();
        if size > MAX_ENTRY_BYTES {
            let _ = fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "file too large to export ({} > {MAX_ENTRY_BYTES} bytes): {}",
                size,
                path.display()
            )));
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_TOTAL_BYTES {
            let _ = fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "project exceeds export size budget ({MAX_TOTAL_BYTES} bytes)"
            )));
        }
        let rel = path
            .strip_prefix(working_dir)
            .map_err(|_| VimaxError::msg("path escapes working dir during export"))?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.is_empty() || rel.contains("..") {
            let _ = fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "invalid relative path during export: {rel}"
            )));
        }
        let entry_name = format!("{WORKING_PREFIX}{rel}");
        let opts = SimpleFileOptions::default().compression_method(compression_for(&rel));
        zip.start_file(&entry_name, opts)
            .map_err(|e| VimaxError::msg(format!("zip start_file {entry_name}: {e}")))?;
        let mut src = File::open(path)?;
        io::copy(&mut src, &mut zip)?;
    }

    zip.finish()
        .map_err(|e| VimaxError::msg(format!("zip finish: {e}")))?;
    if dest.exists() {
        fs::remove_file(&dest)?;
    }
    fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// Extract `working/**` into `staging_dir` and return the archived session record
/// (caller must rewrite id / working_dir before inserting into the index).
pub fn import_session_from_path(
    archive_path: &Path,
    staging_dir: &Path,
) -> VimaxResult<SessionRecord> {
    if !archive_path.is_file() {
        return Err(VimaxError::InvalidParams(format!(
            "archive not found: {}",
            archive_path.display()
        )));
    }
    if staging_dir.exists() {
        fs::remove_dir_all(staging_dir)?;
    }
    fs::create_dir_all(staging_dir)?;

    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(file)
        .map_err(|e| VimaxError::InvalidParams(format!("invalid project archive: {e}")))?;

    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut session_bytes: Option<Vec<u8>> = None;
    let mut total: u64 = 0;
    let mut file_count = 0usize;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| VimaxError::InvalidParams(format!("zip entry {i}: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            return Err(VimaxError::InvalidParams(
                "archive entry escapes archive root".into(),
            ));
        };
        let name = enclosed.to_string_lossy().replace('\\', "/");
        if name.contains('\0') || name.starts_with('/') || name.starts_with("../") {
            return Err(VimaxError::InvalidParams(format!(
                "unsafe archive entry: {name}"
            )));
        }

        file_count += 1;
        if file_count > MAX_ENTRIES {
            return Err(VimaxError::InvalidParams(format!(
                "archive has too many entries (>{MAX_ENTRIES})"
            )));
        }

        let cap = MAX_ENTRY_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(total));
        if name == MANIFEST_ENTRY || name == SESSION_ENTRY {
            // Metadata is small — bound-read into memory for JSON parse.
            let mut buf = Vec::new();
            (&mut entry)
                .take(cap.saturating_add(1))
                .read_to_end(&mut buf)?;
            if buf.len() as u64 > cap {
                return Err(VimaxError::InvalidParams(format!(
                    "archive entry '{name}' exceeds decompression budget"
                )));
            }
            total = total.saturating_add(buf.len() as u64);
            if name == MANIFEST_ENTRY {
                manifest_bytes = Some(buf);
            } else {
                session_bytes = Some(buf);
            }
            continue;
        }
        if let Some(rel) = name.strip_prefix(WORKING_PREFIX) {
            if rel.is_empty() || rel.contains("..") {
                return Err(VimaxError::InvalidParams(format!(
                    "unsafe working path in archive: {name}"
                )));
            }
            let dest = staging_dir.join(rel);
            if !dest.starts_with(staging_dir) {
                return Err(VimaxError::InvalidParams(format!(
                    "working path escapes staging dir: {name}"
                )));
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let written = stream_copy_bounded(&mut entry, &dest, cap)?;
            total = total.saturating_add(written);
            continue;
        }
        // Ignore unknown top-level entries for forward compatibility, but still
        // consume/count bytes so a bomb cannot hide in ignored paths.
        let skipped = discard_bounded(&mut entry, cap)?;
        total = total.saturating_add(skipped);
    }

    let manifest_raw = manifest_bytes.ok_or_else(|| {
        VimaxError::InvalidParams("archive missing manifest.json".into())
    })?;
    let session_raw = session_bytes.ok_or_else(|| {
        VimaxError::InvalidParams("archive missing session.json".into())
    })?;

    let manifest: ArchiveManifest = serde_json::from_slice(&manifest_raw)
        .map_err(|e| VimaxError::InvalidParams(format!("invalid manifest.json: {e}")))?;
    if manifest.app != ARCHIVE_APP {
        return Err(VimaxError::InvalidParams(format!(
            "unsupported archive app '{}'; expected '{ARCHIVE_APP}'",
            manifest.app
        )));
    }
    if manifest.version == 0 || manifest.version > ARCHIVE_VERSION {
        return Err(VimaxError::InvalidParams(format!(
            "unsupported archive version {}",
            manifest.version
        )));
    }
    if WorkflowKind::parse(&manifest.workflow).is_none() {
        return Err(VimaxError::InvalidParams(format!(
            "unknown workflow in manifest: {}",
            manifest.workflow
        )));
    }

    let mut session: SessionRecord = serde_json::from_slice(&session_raw)
        .map_err(|e| VimaxError::InvalidParams(format!("invalid session.json: {e}")))?;
    if session.workflow.as_str() != manifest.workflow {
        return Err(VimaxError::InvalidParams(
            "manifest workflow does not match session.json".into(),
        ));
    }
    if let Some(rel) = session.final_video.as_deref() {
        let cleaned = rel.replace('\\', "/");
        if cleaned.is_empty() || cleaned.contains("..") || cleaned.starts_with('/') {
            return Err(VimaxError::InvalidParams(format!(
                "invalid final_video path in archive: {cleaned}"
            )));
        }
        // Keep the pointer only when the file is present; otherwise clear it so
        // the UI does not show a broken film player after import.
        if staging_dir.join(&cleaned).is_file() {
            session.final_video = Some(cleaned);
        } else {
            session.final_video = None;
        }
    }
    if let Some(rel) = session.cover.as_deref() {
        let cleaned = rel.replace('\\', "/");
        if cleaned.is_empty() || cleaned.contains("..") || cleaned.starts_with('/') {
            return Err(VimaxError::InvalidParams(format!(
                "invalid cover path in archive: {cleaned}"
            )));
        }
        if staging_dir.join(&cleaned).is_file() {
            session.cover = Some(cleaned);
        } else {
            session.cover = None;
        }
    }

    // Normalize runtime fields; caller rewrites id / working_dir.
    session.status = RunStatus::Idle;
    session.summary = if session.final_video.is_some() {
        "imported project".into()
    } else {
        session.summary
    };

    Ok(session)
}

/// Copy `reader` into `dest`, rejecting if more than `cap` bytes are produced.
fn stream_copy_bounded<R: Read>(reader: &mut R, dest: &Path, cap: u64) -> VimaxResult<u64> {
    let mut out = File::create(dest)?;
    let mut limited = reader.take(cap.saturating_add(1));
    let written = io::copy(&mut limited, &mut out)?;
    if written > cap {
        let _ = fs::remove_file(dest);
        return Err(VimaxError::InvalidParams(format!(
            "archive entry exceeds decompression budget ({} bytes)",
            written
        )));
    }
    Ok(written)
}

fn discard_bounded<R: Read>(reader: &mut R, cap: u64) -> VimaxResult<u64> {
    let mut limited = reader.take(cap.saturating_add(1));
    let mut sink = io::sink();
    let written = io::copy(&mut limited, &mut sink)?;
    if written > cap {
        return Err(VimaxError::InvalidParams(
            "archive entry exceeds decompression budget".into(),
        ));
    }
    Ok(written)
}

fn write_json_entry<T: Serialize>(
    zip: &mut ZipWriter<File>,
    name: &str,
    value: &T,
) -> VimaxResult<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(name, opts)
        .map_err(|e| VimaxError::msg(format!("zip start_file {name}: {e}")))?;
    zip.write_all(&bytes)?;
    Ok(())
}

fn compression_for(rel: &str) -> CompressionMethod {
    match Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "webm" | "mov" | "mkv" | "avi" | "png" | "jpg" | "jpeg" | "webp" | "gif"
        | "mp3" | "wav" | "aac" | "m4a" | "zip" | "gz" | "bz2" | "7z" => CompressionMethod::Stored,
        _ => CompressionMethod::Deflated,
    }
}

fn normalize_dest_path(dest: &Path) -> VimaxResult<PathBuf> {
    let mut out = dest.to_path_buf();
    let has_ext = out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
        .unwrap_or(false);
    if !has_ext {
        out.set_extension(ARCHIVE_EXTENSION);
    }
    if out
        .file_name()
        .map(|n| n.to_string_lossy().contains(".."))
        .unwrap_or(true)
    {
        return Err(VimaxError::InvalidParams(
            "invalid destination archive path".into(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WorkflowKind;
    use crate::progress::RunStatus;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn sample_session(id: &str) -> SessionRecord {
        SessionRecord {
            session_id: id.into(),
            working_dir: format!(".working_dir/{id}"),
            title: "Demo Film".into(),
            workflow: WorkflowKind::Script2Video,
            idea: String::new(),
            script: "INT. ROOM".into(),
            novel_text: String::new(),
            user_requirement: String::new(),
            style: "cinematic".into(),
            llm_model: String::new(),
            image_model: String::new(),
            video_model: String::new(),
            target_duration_secs: 15,
            aspect_ratio: "16:9".into(),
            stage: "succeeded".into(),
            summary: "done".into(),
            status: RunStatus::Succeeded,
            stale: BTreeMap::new(),
            final_video: Some("script2video/final_video.mp4".into()),
            cover: None,
            created_at: "2026-01-01T00:00:00+08:00".into(),
            updated_at: "2026-01-01T00:00:00+08:00".into(),
        }
    }

    fn seed_working(root: &Path) {
        let shot = root.join("script2video/shots/0");
        fs::create_dir_all(&shot).unwrap();
        fs::write(root.join("script2video/script.txt"), b"hello script").unwrap();
        fs::write(root.join("script2video/final_video.mp4"), b"FAKEMP4DATA").unwrap();
        fs::write(shot.join("first_frame.png"), b"\x89PNG").unwrap();
        fs::write(shot.join("video.mp4"), vec![7u8; 64]).unwrap();
    }

    #[test]
    fn roundtrip_preserves_binary_assets_and_metadata() {
        let dir = tempdir().unwrap();
        let working = dir.path().join("working_src");
        seed_working(&working);
        let session = sample_session("src-id");
        let archive = dir.path().join("share.nomivimax");
        export_session_to_path(&session, &working, &archive).unwrap();
        assert!(archive.is_file());

        let staging = dir.path().join("staging");
        let imported = import_session_from_path(&archive, &staging).unwrap();
        assert_eq!(imported.workflow, WorkflowKind::Script2Video);
        assert_eq!(imported.title, "Demo Film");
        assert_eq!(imported.status, RunStatus::Idle);
        assert_eq!(
            imported.final_video.as_deref(),
            Some("script2video/final_video.mp4")
        );
        assert_eq!(
            fs::read(staging.join("script2video/final_video.mp4")).unwrap(),
            b"FAKEMP4DATA"
        );
        assert_eq!(
            fs::read(staging.join("script2video/shots/0/video.mp4")).unwrap(),
            vec![7u8; 64]
        );
        assert_eq!(
            fs::read(staging.join("script2video/script.txt")).unwrap(),
            b"hello script"
        );
    }

    #[test]
    fn import_rejects_bad_app_manifest() {
        let dir = tempdir().unwrap();
        let working = dir.path().join("w");
        seed_working(&working);
        let session = sample_session("x");
        let archive = dir.path().join("bad.nomivimax");
        export_session_to_path(&session, &working, &archive).unwrap();

        // Corrupt manifest app field by rebuilding a tiny bad zip.
        let bad = dir.path().join("evil.nomivimax");
        {
            let file = File::create(&bad).unwrap();
            let mut zip = ZipWriter::new(file);
            let manifest = br#"{"version":1,"app":"not-vimax","exported_at":"t","session_id":"x","workflow":"script2video","title":"t"}"#;
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file(MANIFEST_ENTRY, opts).unwrap();
            zip.write_all(manifest).unwrap();
            zip.start_file(SESSION_ENTRY, opts).unwrap();
            zip.write_all(&serde_json::to_vec(&session).unwrap())
                .unwrap();
            zip.finish().unwrap();
        }
        let err = import_session_from_path(&bad, &dir.path().join("s")).unwrap_err();
        assert!(err.to_string().contains("unsupported archive app"));
    }

    #[test]
    fn import_rejects_zip_slip() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join("slip.nomivimax");
        {
            let file = File::create(&bad).unwrap();
            let mut zip = ZipWriter::new(file);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            // Manually craft a suspicious name; enclosed_name should reject `../`.
            zip.start_file("../evil.txt", opts).unwrap();
            zip.write_all(b"nope").unwrap();
            zip.finish().unwrap();
        }
        let err = import_session_from_path(&bad, &dir.path().join("s")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escapes") || msg.contains("missing manifest"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn normalize_adds_extension() {
        let p = normalize_dest_path(Path::new("/tmp/my-film")).unwrap();
        assert!(p
            .extension()
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case(ARCHIVE_EXTENSION));
    }
}
