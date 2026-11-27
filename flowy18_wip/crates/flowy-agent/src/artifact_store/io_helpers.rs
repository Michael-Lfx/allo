//! Stateless file I/O, hashing, atomic persistence, and path/name validation helpers.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use flowy_agent_core::SessionOrigin;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{ArtifactManifest, MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES};

const COPY_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(super) struct Snapshot {
    pub(super) digest: String,
    pub(super) size: u64,
    pub(super) prefix: Vec<u8>,
}

pub(super) fn snapshot_file(file: &mut File) -> io::Result<Snapshot> {
    file.rewind()?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut prefix = Vec::with_capacity(16);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        if size > MAX_ARTIFACT_BYTES {
            return Err(invalid("artifact exceeds the 128 MiB per-file limit"));
        }
        if prefix.len() < 16 {
            let remaining = 16 - prefix.len();
            prefix.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Snapshot {
        digest: hex(&hasher.finalize()),
        size,
        prefix,
    })
}

pub(super) fn copy_and_hash(
    source: &mut File,
    destination: &mut File,
) -> io::Result<Snapshot> {
    source.rewind()?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut prefix = Vec::with_capacity(16);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        if size > MAX_ARTIFACT_BYTES {
            return Err(invalid("artifact exceeds the 128 MiB per-file limit"));
        }
        if prefix.len() < 16 {
            let remaining = 16 - prefix.len();
            prefix.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        hasher.update(&buffer[..read]);
        destination.write_all(&buffer[..read])?;
    }
    Ok(Snapshot {
        digest: hex(&hasher.finalize()),
        size,
        prefix,
    })
}

pub(super) const fn origin_may_own_artifacts(origin: &SessionOrigin) -> bool {
    matches!(origin, SessionOrigin::Home | SessionOrigin::Project { .. })
}

pub(super) fn validate_relative_path(raw: &str) -> io::Result<PathBuf> {
    if raw.trim().is_empty() || raw.chars().any(char::is_control) {
        return Err(invalid("path must be a non-empty relative file path"));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(invalid("absolute paths are not accepted"));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(invalid("path traversal components are not accepted"));
        }
    }
    Ok(path.to_path_buf())
}

pub(super) fn reject_link(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata)
    {
        return Err(invalid(
            "symbolic links and reparse points are not accepted",
        ));
    }
    Ok(())
}

pub(super) fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    flowy_agent_core::tools::is_windows_reparse_point(metadata)
}

pub(super) fn validate_identifier(value: &str) -> io::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|char| {
            char.is_ascii_alphanumeric() || matches!(char, '-' | '_')
        })
    {
        return Err(invalid("invalid host identifier"));
    }
    Ok(())
}

pub(super) fn validate_delivery_key(value: &str) -> io::Result<()> {
    let length = value.chars().count();
    if !(1..=128).contains(&length) || value.chars().any(char::is_control) {
        return Err(invalid(
            "delivery_key must contain 1 to 128 printable characters",
        ));
    }
    Ok(())
}

pub(super) fn safe_display_name(
    provided: Option<&str>,
    source_name: &str,
) -> io::Result<String> {
    let source_extension = Path::new(source_name)
        .extension()
        .and_then(|value| value.to_str());
    let candidate = provided.unwrap_or(source_name);
    if candidate.chars().count() > 255 {
        return Err(invalid("display_name exceeds 255 Unicode scalar values"));
    }
    let mut safe = candidate
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_owned();
    if safe.is_empty() || safe == "." || safe == ".." {
        return Err(invalid("display_name is empty or invalid"));
    }
    let provided_extension = Path::new(&safe)
        .extension()
        .and_then(|value| value.to_str());
    if let Some(extension) = source_extension
        && provided_extension
            .is_none_or(|given| !given.eq_ignore_ascii_case(extension))
    {
        safe.push('.');
        safe.push_str(extension);
    }
    if safe.chars().count() > 255 {
        return Err(invalid("display_name exceeds 255 Unicode scalar values"));
    }
    Ok(safe)
}

pub(super) fn media_type(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let media_type = match extension.as_str() {
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "pptx" => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        "docx" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        "xlsx" => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        }
        "zip" => "application/zip",
        _ => return None,
    };
    Some(media_type.to_owned())
}

pub(super) fn validate_known_header(
    path: &str,
    prefix: &[u8],
) -> io::Result<()> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let valid = match extension.as_str() {
        "pdf" => prefix.starts_with(b"%PDF-"),
        "png" => prefix.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => prefix.starts_with(&[0xff, 0xd8, 0xff]),
        "webp" => {
            prefix.len() >= 12
                && &prefix[0..4] == b"RIFF"
                && &prefix[8..12] == b"WEBP"
        }
        "pptx" | "docx" | "xlsx" | "zip" => prefix.starts_with(b"PK\x03\x04"),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("file content does not match its extension"))
    }
}

pub(super) fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn read_manifest(path: &Path) -> io::Result<ArtifactManifest> {
    let manifest: ArtifactManifest =
        read_limited_json(path, MAX_MANIFEST_BYTES)?;
    if manifest.schema_version != 1 {
        return Err(invalid("unsupported artifact manifest version"));
    }
    Ok(manifest)
}

pub(super) fn read_limited_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    limit: u64,
) -> io::Result<T> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || is_windows_reparse_point(&metadata)
        || metadata.len() > limit
    {
        return Err(invalid("managed metadata file is invalid or too large"));
    }
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("managed metadata file exceeds its read limit"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(super) fn write_new_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file =
        OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| invalid("metadata path has no parent"))?;
    fs::create_dir_all(parent)?;
    let (temporary, mut output) = loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary =
            parent.join(format!(".artifact-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(output) => break (temporary, output),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        output.write_all(bytes)?;
        output.sync_all()?;
        drop(output);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

pub(super) fn random_id() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(hex(&bytes))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn now_ms() -> u64 {
    crate::handle::now_ms()
}

pub(super) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[must_use]
pub fn can_open_with_default_app(file_name: &str) -> bool {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "pdf"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "pptx"
            | "docx"
            | "xlsx"
            | "csv"
            | "tsv"
            | "txt"
            | "md"
            | "json"
            | "xml"
            | "rtf"
            | "odt"
            | "ods"
            | "odp"
    )
}
