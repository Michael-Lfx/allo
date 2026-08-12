//! Pack / unpack ViMax skill directories as `.vimaxskill` (zip) for cloud Skill Hub.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use walkdir::WalkDir;
use zip::read::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::CompressionMethod;

use crate::error::{VimaxError, VimaxResult};

use super::parse::SKILL_MANIFEST;

/// Soft caps aligned with cloud Skill Hub package limits.
const MAX_PACKAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ENTRIES: usize = 100;
const MAX_ENTRY_BYTES: u64 = 5 * 1024 * 1024;

/// Zip a skill directory (must contain `SKILL.md`) into `dest_path`.
pub fn pack_skill_dir(skill_dir: &Path, dest_path: &Path) -> VimaxResult<u64> {
    if !skill_dir.join(SKILL_MANIFEST).is_file() {
        return Err(VimaxError::InvalidParams(format!(
            "no SKILL.md in {}",
            skill_dir.display()
        )));
    }
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest_path.with_extension("vimaxskill.tmp");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }

    let file = File::create(&tmp)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut file_count = 0usize;
    let mut total_bytes: u64 = 0;

    for entry in WalkDir::new(skill_dir)
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
            let _ = std::fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "refusing to pack symlink: {}",
                path.display()
            )));
        }
        if !meta.is_file() {
            continue;
        }
        file_count += 1;
        if file_count > MAX_ENTRIES {
            let _ = std::fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "skill has too many files to pack (>{MAX_ENTRIES})"
            )));
        }
        let size = meta.len();
        if size > MAX_ENTRY_BYTES {
            let _ = std::fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "file too large to pack ({} bytes): {}",
                size,
                path.display()
            )));
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_PACKAGE_BYTES {
            let _ = std::fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "skill exceeds pack size budget ({MAX_PACKAGE_BYTES} bytes)"
            )));
        }
        let rel = path
            .strip_prefix(skill_dir)
            .map_err(|_| VimaxError::msg("path escapes skill dir during pack"))?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.is_empty() || rel.contains("..") {
            let _ = std::fs::remove_file(&tmp);
            return Err(VimaxError::InvalidParams(format!(
                "invalid relative path during pack: {rel}"
            )));
        }
        let mut bytes = Vec::new();
        File::open(path)?.read_to_end(&mut bytes)?;
        zip.start_file(rel, options)
            .map_err(|e| VimaxError::msg(format!("zip start_file: {e}")))?;
        zip.write_all(&bytes)
            .map_err(|e| VimaxError::msg(format!("zip write: {e}")))?;
    }

    zip.finish()
        .map_err(|e| VimaxError::msg(format!("zip finish: {e}")))?;

    if dest_path.exists() {
        let _ = std::fs::remove_file(dest_path);
    }
    std::fs::rename(&tmp, dest_path)?;
    let meta = std::fs::metadata(dest_path)?;
    if meta.len() > MAX_PACKAGE_BYTES {
        let _ = std::fs::remove_file(dest_path);
        return Err(VimaxError::InvalidParams(format!(
            "packed skill too large ({} bytes); max is {MAX_PACKAGE_BYTES}",
            meta.len()
        )));
    }
    Ok(meta.len())
}

/// Extract a `.vimaxskill` / `.zip` into `dest_dir`, returning the directory that contains `SKILL.md`.
pub fn unpack_skill_package(package_path: &Path, dest_dir: &Path) -> VimaxResult<PathBuf> {
    if dest_dir.exists() {
        std::fs::remove_dir_all(dest_dir)?;
    }
    std::fs::create_dir_all(dest_dir)?;

    let file = File::open(package_path)?;
    let mut zip = ZipArchive::new(file)
        .map_err(|e| VimaxError::InvalidParams(format!("invalid skill package: {e}")))?;

    if zip.len() > MAX_ENTRIES {
        return Err(VimaxError::InvalidParams(format!(
            "skill package has too many entries (>{MAX_ENTRIES})"
        )));
    }

    let mut total_bytes: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| VimaxError::InvalidParams(format!("zip entry: {e}")))?;
        let name = entry.name().replace('\\', "/");
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            return Err(VimaxError::InvalidParams(format!(
                "unsafe zip entry path: {name}"
            )));
        }
        let out_path = dest_dir.join(&name);
        if entry.is_dir() || name.ends_with('/') {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let size = entry.size();
        if size > MAX_ENTRY_BYTES {
            return Err(VimaxError::InvalidParams(format!(
                "zip entry too large ({size} bytes): {name}"
            )));
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_PACKAGE_BYTES {
            return Err(VimaxError::InvalidParams(format!(
                "extracted skill exceeds size budget ({MAX_PACKAGE_BYTES} bytes)"
            )));
        }
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    find_skill_md_dir(dest_dir).ok_or_else(|| {
        VimaxError::InvalidParams("skill package missing SKILL.md".into())
    })
}

fn find_skill_md_dir(root: &Path) -> Option<PathBuf> {
    if root.join(SKILL_MANIFEST).is_file() {
        return Some(root.to_path_buf());
    }
    let mut found: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(SKILL_MANIFEST).is_file() {
                if found.is_some() {
                    // Ambiguous multi-root packages: prefer first, stop on second.
                    return found;
                }
                found = Some(path);
            }
        }
    }
    found
}

/// Patch / insert cloud provenance keys into SKILL.md frontmatter.
pub fn patch_cloud_provenance(md: &str, cloud_id: i64, cloud_version: &str) -> String {
    let mut lines: Vec<String> = md.lines().map(|l| l.to_string()).collect();
    let mut has_id = false;
    let mut has_ver = false;
    let mut has_source = false;
    for line in &mut lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("cloud-id:") || lower.starts_with("cloud_id:") {
            *line = format!("cloud-id: {cloud_id}");
            has_id = true;
        } else if lower.starts_with("cloud-version:") || lower.starts_with("cloud_version:") {
            *line = format!("cloud-version: \"{cloud_version}\"");
            has_ver = true;
        } else if lower.starts_with("source:") {
            *line = "source: cloud".into();
            has_source = true;
        }
    }
    if lines.first().map(|l| l.trim() == "---").unwrap_or(false) {
        let mut insert_at = 1usize;
        if !has_id {
            lines.insert(insert_at, format!("cloud-id: {cloud_id}"));
            insert_at += 1;
        }
        if !has_ver {
            lines.insert(insert_at, format!("cloud-version: \"{cloud_version}\""));
            insert_at += 1;
        }
        if !has_source {
            lines.insert(insert_at, "source: cloud".into());
        }
    }
    let mut out = lines.join("\n");
    if md.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}
