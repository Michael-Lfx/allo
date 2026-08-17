//! Download ripgrep into `{data_dir}/bin` so Grep works without a system `rg`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use reqwest::Client;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use crate::gateway::data_dir;

use super::probe::pick_fastest_url;

/// Pinned official release; bump intentionally when updating.
const RG_VERSION: &str = "15.2.0";

#[derive(Debug, Error)]
pub enum RipgrepInstallError {
    #[error("no ripgrep download mirrors configured for this OS/CPU")]
    NoMirrors,
    #[error("download failed: {0}")]
    Download(String),
    #[error("extract failed: {0}")]
    Extract(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFormat {
    Zip,
    TarGz,
}

#[derive(Debug, Clone, Copy)]
struct RgMirror {
    url: &'static str,
    format: ArchiveFormat,
}

fn mirror(url: &'static str, format: ArchiveFormat) -> RgMirror {
    RgMirror { url, format }
}

fn platform_mirrors() -> Vec<RgMirror> {
    // URLs must stay in sync with `RG_VERSION`.
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => vec![mirror(
            "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-pc-windows-msvc.zip",
            ArchiveFormat::Zip,
        )],
        ("windows", "aarch64") => vec![mirror(
            "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-aarch64-pc-windows-msvc.zip",
            ArchiveFormat::Zip,
        )],
        ("linux", "x86_64") => vec![mirror(
            "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz",
            ArchiveFormat::TarGz,
        )],
        ("linux", "aarch64") => vec![mirror(
            "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-aarch64-unknown-linux-gnu.tar.gz",
            ArchiveFormat::TarGz,
        )],
        ("macos", "x86_64") => vec![mirror(
            "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-apple-darwin.tar.gz",
            ArchiveFormat::TarGz,
        )],
        ("macos", "aarch64") => vec![mirror(
            "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-aarch64-apple-darwin.tar.gz",
            ArchiveFormat::TarGz,
        )],
        _ => Vec::new(),
    }
}

fn managed_rg_path(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        home.join("bin").join("rg.exe")
    }
    #[cfg(not(windows))]
    {
        home.join("bin").join("rg")
    }
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "rg.exe"
    } else {
        "rg"
    }
}

/// Download ripgrep into `{data_dir}/bin/rg[.exe]`.
pub async fn ensure_ripgrep(quiet: bool) -> Result<PathBuf, RipgrepInstallError> {
    let home = data_dir();
    let dest = managed_rg_path(&home);
    if dest.is_file() {
        return Ok(dest);
    }

    let mirrors = platform_mirrors();
    if mirrors.is_empty() {
        return Err(RipgrepInstallError::NoMirrors);
    }

    if !quiet {
        info!(
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            version = RG_VERSION,
            mirrors = mirrors.len(),
            "probing ripgrep mirrors"
        );
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(180))
        .user_agent("allo/dep-install")
        .build()
        .map_err(|e| RipgrepInstallError::Download(e.to_string()))?;

    let urls: Vec<&str> = mirrors.iter().map(|m| m.url).collect();
    let start_idx = match pick_fastest_url(&client, &urls).await {
        Some(idx) => idx,
        None => {
            debug!("ripgrep mirror probe failed; will try mirrors in order");
            0
        }
    };

    std::fs::create_dir_all(home.join("bin"))?;

    let mut ordered: Vec<RgMirror> = Vec::with_capacity(mirrors.len());
    ordered.push(mirrors[start_idx]);
    for (i, mirror) in mirrors.iter().enumerate() {
        if i != start_idx {
            ordered.push(*mirror);
        }
    }

    let temp_dir = std::env::temp_dir().join(format!("allo-ripgrep-{}", std::process::id()));
    tokio::fs::create_dir_all(&temp_dir).await?;

    let mut last_err = RipgrepInstallError::Download("no mirror attempted".into());
    for mirror in ordered {
        let archive_path = temp_dir.join(archive_filename(mirror));
        debug!(url = mirror.url, "downloading ripgrep");
        if !quiet {
            info!(url = mirror.url, "downloading ripgrep");
        }
        match download_file(&client, mirror.url, &archive_path).await {
            Ok(()) => {}
            Err(e) => {
                warn!(url = mirror.url, error = %e, "ripgrep download failed; trying next mirror");
                last_err = e;
                continue;
            }
        }

        match extract_rg(&archive_path, mirror.format, &dest).await {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&dest) {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = std::fs::set_permissions(&dest, perms);
                    }
                }
                let _ = tokio::fs::remove_dir_all(&temp_dir).await;
                if !quiet {
                    info!(path = %dest.display(), "ripgrep installed");
                }
                return Ok(dest);
            }
            Err(e) => {
                warn!(url = mirror.url, error = %e, "ripgrep extract failed; trying next mirror");
                last_err = e;
                let _ = tokio::fs::remove_file(&archive_path).await;
            }
        }
    }

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    Err(last_err)
}

fn archive_filename(mirror: RgMirror) -> String {
    mirror
        .url
        .rsplit('/')
        .next()
        .unwrap_or("ripgrep-archive")
        .to_string()
}

async fn download_file(client: &Client, url: &str, dest: &Path) -> Result<(), RipgrepInstallError> {
    let mut request = client.get(url);
    if url.contains("github.com") || url.contains("githubusercontent.com") {
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            request = request
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/octet-stream");
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| RipgrepInstallError::Download(e.to_string()))?;
    if !response.status().is_success() {
        return Err(RipgrepInstallError::Download(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| RipgrepInstallError::Download(e.to_string()))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| RipgrepInstallError::Download(e.to_string()))?;
    }
    file.flush().await?;
    Ok(())
}

async fn extract_rg(
    archive_path: &Path,
    format: ArchiveFormat,
    dest: &Path,
) -> Result<(), RipgrepInstallError> {
    match format {
        ArchiveFormat::Zip => extract_from_zip(archive_path, dest),
        ArchiveFormat::TarGz => extract_from_tar_gz(archive_path, dest),
    }
}

fn extract_from_zip(archive_path: &Path, dest: &Path) -> Result<(), RipgrepInstallError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| RipgrepInstallError::Extract(e.to_string()))?;
    let target = binary_name();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| RipgrepInstallError::Extract(e.to_string()))?;
        let name = entry.name().replace('\\', "/");
        if name.ends_with(target) || name == target {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(RipgrepInstallError::Extract(format!(
        "{target} not found in zip"
    )))
}

fn extract_from_tar_gz(archive_path: &Path, dest: &Path) -> Result<(), RipgrepInstallError> {
    let file = std::fs::File::open(archive_path)?;
    let decompressor = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decompressor);
    let target = binary_name();

    for entry in archive
        .entries()
        .map_err(|e| RipgrepInstallError::Extract(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| RipgrepInstallError::Extract(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| RipgrepInstallError::Extract(e.to_string()))?;
        if path.file_name().and_then(|n| n.to_str()) == Some(target) {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(RipgrepInstallError::Extract(format!(
        "{target} not found in tar.gz"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_has_mirrors() {
        let mirrors = platform_mirrors();
        assert!(
            !mirrors.is_empty(),
            "expected ripgrep mirrors for {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        for m in &mirrors {
            assert!(m.url.contains(RG_VERSION));
            assert!(m.url.contains("BurntSushi/ripgrep"));
        }
    }
}
