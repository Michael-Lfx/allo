use crate::MAX_PACKAGE_BYTES;
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use tar::Archive as TarArchive;
use thiserror::Error;
use zip::ZipArchive;

const MAX_ZIP_FILES: usize = 200;
const MAX_PATH_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadBody {
    Zip(Vec<u8>),
    GitHub(GitHubHandoff),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubHandoff {
    pub repo: String,
    pub commit: String,
    pub path: String,
    pub content_hash: String,
    pub archive_url: String,
}

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("skill package exceeds 8 MiB")]
    TooLarge,
    #[error("skill archive path escapes the destination")]
    PathEscape,
    #[error("skill archive contains too many files")]
    TooManyFiles,
    #[error("skill archive path is too deep")]
    DepthTooDeep,
    #[error("skill archive contains a symbolic link")]
    Symlink,
    #[error("GitHub handoff was rejected")]
    RejectedHandoff,
    #[error("download content type is unsupported")]
    UnsupportedContentType,
    #[error("destination already exists")]
    DestinationExists,
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("archive I/O: {0}")]
    Io(#[from] io::Error),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubDownload {
    source_ref: String,
    repo: String,
    commit: String,
    path: String,
    content_hash: String,
    archive_url: String,
}

pub fn classify_download(
    content_type: &str,
    body: &[u8],
) -> Result<DownloadBody, ArchiveError> {
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if content_type == "application/json" || content_type.ends_with("+json") {
        let handoff: GitHubDownload = serde_json::from_slice(body)
            .map_err(|_| ArchiveError::RejectedHandoff)?;
        if handoff.source_ref != "public-github"
            || !github_archive_allowed(&handoff.archive_url)
            || handoff.repo.trim().is_empty()
            || handoff.commit.trim().is_empty()
            || handoff.path.trim().is_empty()
            || handoff.content_hash.trim().is_empty()
        {
            return Err(ArchiveError::RejectedHandoff);
        }
        return Ok(DownloadBody::GitHub(GitHubHandoff {
            repo: handoff.repo,
            commit: handoff.commit,
            path: handoff.path,
            content_hash: handoff.content_hash,
            archive_url: handoff.archive_url,
        }));
    }
    let zip_type = matches!(
        content_type.as_str(),
        "application/zip"
            | "application/x-zip-compressed"
            | "application/octet-stream"
    );
    if zip_type || body.starts_with(b"PK") {
        Ok(DownloadBody::Zip(body.to_vec()))
    } else {
        Err(ArchiveError::UnsupportedContentType)
    }
}

#[must_use]
pub fn github_archive_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let Some((authority, path)) = rest.split_once('/') else {
        return false;
    };
    authority == "github.com" && !path.is_empty()
}

pub fn extract_skill_zip(
    bytes: &[u8],
    dest: &Path,
) -> Result<(), ArchiveError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PACKAGE_BYTES {
        return Err(ArchiveError::TooLarge);
    }
    if dest.exists() {
        return Err(ArchiveError::DestinationExists);
    }
    fs::create_dir_all(dest)?;
    let result = extract_zip_inner(bytes, dest);
    if result.is_err() {
        let _ = fs::remove_dir_all(dest);
    }
    result
}

pub fn extract_github_skill(
    bytes: &[u8],
    skill_path: &str,
    dest: &Path,
) -> Result<(), ArchiveError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PACKAGE_BYTES {
        return Err(ArchiveError::TooLarge);
    }
    if dest.exists() {
        return Err(ArchiveError::DestinationExists);
    }
    let skill_path = handoff_skill_path(skill_path)?;
    fs::create_dir_all(dest)?;
    let result = if bytes.starts_with(&[0x1f, 0x8b]) {
        extract_github_tar(bytes, skill_path.as_deref(), dest)
    } else if bytes.starts_with(b"PK") {
        extract_github_zip(bytes, skill_path.as_deref(), dest)
    } else {
        Err(ArchiveError::UnsupportedContentType)
    };
    if result.is_err() {
        let _ = fs::remove_dir_all(dest);
    }
    result
}

/// `ClawHub` GitHub handoffs carry a SHA-256 content hash. Accept either the
/// archive digest or a deterministic digest over the extracted skill files.
pub fn content_hash_matches(
    expected: &str,
    archive: &[u8],
    root: &Path,
) -> Result<bool, ArchiveError> {
    let expected = expected
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or_else(|| expected.trim())
        .to_ascii_lowercase();
    if expected.len() != 64
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ArchiveError::RejectedHandoff);
    }
    if sha256_hex(archive) == expected {
        return Ok(true);
    }
    Ok(tree_manifest_hash(root)? == expected)
}

pub fn collect_tree_hashes(
    root: &Path,
) -> Result<BTreeMap<String, String>, ArchiveError> {
    let mut files = BTreeMap::new();
    collect_tree_files(root, root, &mut files)?;
    Ok(files)
}

fn collect_tree_files(
    root: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), ArchiveError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(ArchiveError::Symlink);
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_tree_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ArchiveError::PathEscape)?;
            let relative = normalize_relative(&relative.to_string_lossy());
            let bytes = fs::read(&path)?;
            files.insert(relative, sha256_hex(&bytes));
        }
    }
    Ok(())
}

fn tree_manifest_hash(root: &Path) -> Result<String, ArchiveError> {
    let files = collect_tree_hashes(root)?;
    let mut digest = Sha256::new();
    for (path, hash) in files {
        if path == "skill-card.md" || path == "_meta.json" {
            continue;
        }
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update(hash.as_bytes());
        digest.update(b"\n");
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn handoff_skill_path(path: &str) -> Result<Option<String>, ArchiveError> {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(ArchiveError::PathEscape);
    }
    let trimmed = normalized.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(None);
    }
    let relative = safe_relative_path(trimmed)?;
    Ok(Some(relative.to_string_lossy().replace('\\', "/")))
}

fn archive_relative_entry(
    name: &str,
    skill_path: Option<&str>,
) -> Result<Option<PathBuf>, ArchiveError> {
    let normalized = name.replace('\\', "/");
    let relative = if let Some(skill_path) = skill_path {
        let marker = format!("/{skill_path}/");
        let Some(index) = normalized.find(&marker) else {
            return Ok(None);
        };
        &normalized[index + marker.len()..]
    } else {
        let Some((_, relative)) = normalized.split_once('/') else {
            return Ok(None);
        };
        relative
    };
    if relative.is_empty() {
        return Ok(None);
    }
    Ok(Some(safe_relative_path(relative)?))
}

fn extract_github_zip(
    bytes: &[u8],
    skill_path: Option<&str>,
    dest: &Path,
) -> Result<(), ArchiveError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut files = 0_usize;
    let mut written_total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
        {
            return Err(ArchiveError::Symlink);
        }
        let Some(relative) = archive_relative_entry(entry.name(), skill_path)?
        else {
            continue;
        };
        let output = dest.join(&relative);
        if !output.starts_with(dest) {
            return Err(ArchiveError::PathEscape);
        }
        if entry.is_dir() || entry.name().ends_with(['/', '\\']) {
            fs::create_dir_all(&output)?;
            continue;
        }
        files += 1;
        if files > MAX_ZIP_FILES {
            return Err(ArchiveError::TooManyFiles);
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&output)?;
        let remaining = MAX_PACKAGE_BYTES.saturating_sub(written_total);
        let mut limited = Read::take(entry, remaining.saturating_add(1));
        let written = io::copy(&mut limited, &mut output)?;
        if written > remaining {
            return Err(ArchiveError::TooLarge);
        }
        written_total = written_total.saturating_add(written);
    }
    Ok(())
}

fn extract_github_tar(
    bytes: &[u8],
    skill_path: Option<&str>,
    dest: &Path,
) -> Result<(), ArchiveError> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = TarArchive::new(decoder);
    let mut files = 0_usize;
    let mut written_total = 0_u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(ArchiveError::Symlink);
        }
        if !entry_type.is_file() && !entry_type.is_dir() {
            continue;
        }
        let path = entry.path()?;
        let path = path.to_string_lossy();
        let Some(relative) = archive_relative_entry(&path, skill_path)? else {
            continue;
        };
        let output = dest.join(&relative);
        if !output.starts_with(dest) {
            return Err(ArchiveError::PathEscape);
        }
        if entry_type.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        files += 1;
        if files > MAX_ZIP_FILES {
            return Err(ArchiveError::TooManyFiles);
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&output)?;
        let remaining = MAX_PACKAGE_BYTES.saturating_sub(written_total);
        let mut limited = Read::take(&mut entry, remaining.saturating_add(1));
        let written = io::copy(&mut limited, &mut output)?;
        if written > remaining {
            return Err(ArchiveError::TooLarge);
        }
        written_total = written_total.saturating_add(written);
    }
    Ok(())
}

pub fn normalize_relative(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

fn extract_zip_inner(bytes: &[u8], dest: &Path) -> Result<(), ArchiveError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut files = 0_usize;
    let mut written_total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
        {
            return Err(ArchiveError::Symlink);
        }
        let relative = safe_relative_path(entry.name())?;
        if entry.enclosed_name().is_none() {
            return Err(ArchiveError::PathEscape);
        }
        let output = dest.join(&relative);
        if !output.starts_with(dest) {
            return Err(ArchiveError::PathEscape);
        }
        let is_dir = entry.is_dir() || entry.name().ends_with(['/', '\\']);
        if is_dir {
            fs::create_dir_all(&output)?;
            continue;
        }
        if files >= MAX_ZIP_FILES {
            return Err(ArchiveError::TooManyFiles);
        }
        files += 1;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&output)?;
        let remaining = MAX_PACKAGE_BYTES.saturating_sub(written_total);
        let mut limited = io::Read::take(entry, remaining.saturating_add(1));
        let written = io::copy(&mut limited, &mut output)?;
        if written > remaining {
            return Err(ArchiveError::TooLarge);
        }
        written_total = written_total.saturating_add(written);
    }
    Ok(())
}

fn safe_relative_path(name: &str) -> Result<PathBuf, ArchiveError> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.starts_with("//") {
        return Err(ArchiveError::PathEscape);
    }
    if normalized
        .as_bytes()
        .get(1)
        .is_some_and(|byte| *byte == b':')
    {
        return Err(ArchiveError::PathEscape);
    }
    let mut relative = PathBuf::new();
    let mut depth = 0_usize;
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => {
                relative.push(part);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(ArchiveError::PathEscape);
            }
        }
    }
    if depth == 0 {
        return Err(ArchiveError::PathEscape);
    }
    if depth > MAX_PATH_DEPTH {
        return Err(ArchiveError::DepthTooDeep);
    }
    Ok(relative)
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tar::{Builder, Header};
    use zip::write::SimpleFileOptions;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "flowy-skills-archive-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn zip_with_files(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in files {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn only_github_archive_hosts_are_allowed() {
        assert!(github_archive_allowed(
            "https://github.com/owner/repo/archive/abc.tar.gz"
        ));
        assert!(!github_archive_allowed(
            "https://codeload.github.com/owner/repo/archive/abc.tar.gz"
        ));
        assert!(!github_archive_allowed(
            "http://github.com/owner/repo/archive/abc.tar.gz"
        ));
        assert!(!github_archive_allowed("https://evil.example/github.com"));
    }

    #[test]
    fn classifies_zip_and_validates_github_handoff() {
        let zip = classify_download("application/zip", b"PK\x03\x04").unwrap();
        assert!(matches!(zip, DownloadBody::Zip(_)));
        let body = r#"
        {
          "sourceRef": "public-github",
          "repo": "owner/repo",
          "commit": "abc",
          "path": "skills/pdf",
          "contentHash": "sha256:abc",
          "archiveUrl": "https://github.com/owner/repo/archive/abc.tar.gz"
        }
        "#;
        let handoff =
            classify_download("application/json", body.as_bytes()).unwrap();
        assert!(matches!(handoff, DownloadBody::GitHub(_)));
        let bad_host = body.replace("github.com", "evil.example");
        assert!(matches!(
            classify_download("application/json", bad_host.as_bytes()),
            Err(ArchiveError::RejectedHandoff)
        ));
    }

    #[test]
    fn extracts_github_tar_skill_subtree_and_checks_content_hash() {
        let dir = TestDir::new("github-tar");
        let skill = b"# GitHub skill";
        let mut builder = Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.set_size(u64::try_from(skill.len()).unwrap());
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "repo-abc/skills/demo/SKILL.md",
                &skill[..],
            )
            .unwrap();
        let mut header = Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "repo-abc/outside.txt", &b"nope!"[..])
            .unwrap();
        let tar = builder.into_inner().unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar).unwrap();
        let archive = encoder.finish().unwrap();

        let bad_output = dir.path.join("bad");
        assert!(matches!(
            extract_github_skill(&archive, "/skills/demo", &bad_output),
            Err(ArchiveError::PathEscape)
        ));
        let output = dir.path.join("skill");
        extract_github_skill(&archive, "skills/demo", &output).unwrap();
        assert_eq!(fs::read(output.join("SKILL.md")).unwrap(), skill);
        assert!(!output.join("outside.txt").exists());
        assert!(
            content_hash_matches(
                &format!("sha256:{}", sha256_hex(&archive)),
                &archive,
                &output
            )
            .unwrap()
        );
        assert!(
            !content_hash_matches(&"0".repeat(64), &archive, &output).unwrap()
        );
    }

    #[test]
    fn extracts_regular_skill_archive() {
        let dir = TestDir::new("valid");
        let bytes = zip_with_files(&[
            ("SKILL.md", b"# PDF"),
            ("scripts/run.sh", b"echo ok"),
        ]);
        extract_skill_zip(&bytes, &dir.path.join("out")).unwrap();
        assert_eq!(
            fs::read(dir.path.join("out").join("SKILL.md")).unwrap(),
            b"# PDF"
        );
        assert!(dir.path.join("out").join("scripts/run.sh").is_file());
    }

    #[test]
    fn rejects_parent_path_and_removes_partial_output() {
        let dir = TestDir::new("escape");
        let bytes = zip_with_files(&[
            ("SKILL.md", b"# PDF"),
            ("../../outside.txt", b"bad"),
        ]);
        let output = dir.path.join("out");
        assert!(matches!(
            extract_skill_zip(&bytes, &output),
            Err(ArchiveError::PathEscape)
        ));
        assert!(!output.exists());
        assert!(!dir.path.join("outside.txt").exists());
    }

    #[test]
    fn rejects_oversized_input_before_writing() {
        let dir = TestDir::new("large");
        let bytes = vec![0_u8; usize::try_from(MAX_PACKAGE_BYTES).unwrap() + 1];
        let output = dir.path.join("out");
        assert!(matches!(
            extract_skill_zip(&bytes, &output),
            Err(ArchiveError::TooLarge)
        ));
        assert!(!output.exists());
    }

    #[test]
    fn hashes_known_sha256_value() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
