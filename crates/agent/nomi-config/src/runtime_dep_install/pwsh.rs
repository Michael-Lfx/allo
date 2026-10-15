//! Download a private PowerShell 7 zip into `{data_dir}/runtime/pwsh/<ver>/`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use crate::gateway::data_dir;

use super::probe::pick_fastest_url;

/// Pinned official portable build. Bump intentionally with hashes.
pub const PWSH_VERSION: &str = "7.6.7";

const IDENTITY_FILE: &str = ".pwsh-identity";
const EXE_NAME: &str = "pwsh.exe";

const MODELSCOPE_X64: &str = "https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/ps7/PowerShell-7.6.7-win-x64.zip?view=false";
const MODELSCOPE_ARM64: &str = "https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/ps7/PowerShell-7.6.7-win-arm64.zip?view=false";
const GITHUB_X64: &str =
    "https://github.com/PowerShell/PowerShell/releases/download/v7.6.7/PowerShell-7.6.7-win-x64.zip";
const GITHUB_ARM64: &str =
    "https://github.com/PowerShell/PowerShell/releases/download/v7.6.7/PowerShell-7.6.7-win-arm64.zip";

/// Official GitHub Release SHA256 for the two Windows zips (also in SHA256SUMS.txt).
/// Do not fetch the SUMS file at runtime — pin the Microsoft hashes here.
const SHA256_X64: &str = "d2f952ebdbf24386cb8923553aa58e41a846db35a6155ea04c58142a4b16dbb7";
const SHA256_ARM64: &str = "119bd9221254ef3637523cb1990797618f5fd68848b57de40f12b561d743149e";

#[derive(Debug, Error)]
pub enum PwshInstallError {
    #[error("no PowerShell 7 download mirrors configured for this OS/CPU")]
    NoMirrors,
    #[error("download failed: {0}")]
    Download(String),
    #[error("archive hash mismatch")]
    HashMismatch { expected: String, actual: String },
    #[error("extract failed: {0}")]
    Extract(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Copy)]
struct PwshMirror {
    url: &'static str,
    sha256: &'static str,
    filename: &'static str,
}

fn platform_mirrors() -> Vec<PwshMirror> {
    mirrors_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn mirrors_for(os: &str, arch: &str) -> Vec<PwshMirror> {
    match (os, arch) {
        ("windows", "x86_64") => vec![
            PwshMirror {
                url: MODELSCOPE_X64,
                sha256: SHA256_X64,
                filename: "PowerShell-7.6.7-win-x64.zip",
            },
            PwshMirror {
                url: GITHUB_X64,
                sha256: SHA256_X64,
                filename: "PowerShell-7.6.7-win-x64.zip",
            },
        ],
        ("windows", "aarch64") => vec![
            PwshMirror {
                url: MODELSCOPE_ARM64,
                sha256: SHA256_ARM64,
                filename: "PowerShell-7.6.7-win-arm64.zip",
            },
            PwshMirror {
                url: GITHUB_ARM64,
                sha256: SHA256_ARM64,
                filename: "PowerShell-7.6.7-win-arm64.zip",
            },
        ],
        _ => Vec::new(),
    }
}

pub fn managed_pwsh_dir(home: &Path) -> PathBuf {
    home.join("runtime").join("pwsh").join(PWSH_VERSION)
}

pub fn managed_pwsh_exe(home: &Path) -> PathBuf {
    let dir = managed_pwsh_dir(home);
    find_pwsh_exe(&dir).unwrap_or_else(|| dir.join(EXE_NAME))
}

fn identity_matches(dir: &Path, sha256: &str) -> bool {
    let Ok(raw) = std::fs::read_to_string(dir.join(IDENTITY_FILE)) else {
        return false;
    };
    let mut version_ok = false;
    let mut hash_ok = false;
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            version_ok = value.trim() == PWSH_VERSION;
        }
        if let Some(value) = line.strip_prefix("sha256=") {
            hash_ok = value.trim().eq_ignore_ascii_case(sha256);
        }
    }
    version_ok && hash_ok
}

fn expected_sha256() -> Option<&'static str> {
    mirrors_for(std::env::consts::OS, std::env::consts::ARCH)
        .first()
        .map(|mirror| mirror.sha256)
}

/// Point the process runtime at an already-installed sidecar without downloading.
pub fn advertise_managed_pwsh() {
    #[cfg(windows)]
    if managed_pwsh_ready() {
        register_override(&managed_pwsh_exe(&data_dir()));
    }
}

/// True when the pinned sidecar is already on disk and looks like a real pwsh.
pub fn managed_pwsh_ready() -> bool {
    #[cfg(not(windows))]
    {
        return false;
    }
    #[cfg(windows)]
    {
        let Some(sha) = expected_sha256() else {
            return false;
        };
        let dir = managed_pwsh_dir(&data_dir());
        identity_matches(&dir, sha)
            && find_pwsh_exe(&dir).is_some_and(|exe| nomi_process_runtime::is_usable_pwsh(&exe))
    }
}

fn register_override(exe: &Path) {
    #[cfg(windows)]
    {
        nomi_process_runtime::set_windows_powershell_override(Some(exe.to_path_buf()));
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
    }
}

/// Download PowerShell 7 into `{data_dir}/runtime/pwsh/<ver>/pwsh.exe`.
pub async fn ensure_pwsh(quiet: bool) -> Result<PathBuf, PwshInstallError> {
    #[cfg(not(windows))]
    {
        let _ = quiet;
        return Err(PwshInstallError::NoMirrors);
    }

    #[cfg(windows)]
    {
        ensure_pwsh_windows(quiet).await
    }
}

#[cfg(windows)]
async fn ensure_pwsh_windows(quiet: bool) -> Result<PathBuf, PwshInstallError> {
    let home = data_dir();
    let dest_dir = managed_pwsh_dir(&home);
    let Some(sha) = expected_sha256() else {
        return Err(PwshInstallError::NoMirrors);
    };
    if identity_matches(&dest_dir, sha)
        && let Some(dest) = find_pwsh_exe(&dest_dir).filter(|exe| nomi_process_runtime::is_usable_pwsh(exe))
    {
        register_override(&dest);
        return Ok(dest);
    }

    let mirrors = platform_mirrors();
    if mirrors.is_empty() {
        return Err(PwshInstallError::NoMirrors);
    }

    if !quiet {
        info!(
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            version = PWSH_VERSION,
            mirrors = mirrors.len(),
            "probing PowerShell 7 mirrors"
        );
    }

    let client = nomifun_net::proxy::apply_detected_proxy(
        Client::builder()
            .timeout(Duration::from_secs(300))
            .user_agent("allo/dep-install"),
    )
    .build()
    .map_err(|e| PwshInstallError::Download(e.to_string()))?;

    let urls: Vec<&str> = mirrors.iter().map(|m| m.url).collect();
    let start_idx = match pick_fastest_url(&client, &urls).await {
        Some(idx) => idx,
        None => {
            debug!("PowerShell 7 mirror probe failed; will try mirrors in order");
            0
        }
    };

    let mut ordered = Vec::with_capacity(mirrors.len());
    ordered.push(mirrors[start_idx]);
    for (i, mirror) in mirrors.iter().enumerate() {
        if i != start_idx {
            ordered.push(*mirror);
        }
    }

    let temp_dir = std::env::temp_dir().join(format!("allo-pwsh-{}", std::process::id()));
    tokio::fs::create_dir_all(&temp_dir).await?;

    let mut last_err = PwshInstallError::Download("no mirror attempted".into());
    for mirror in ordered {
        let archive_path = temp_dir.join(mirror.filename);
        debug!(url = mirror.url, "downloading PowerShell 7");
        if !quiet {
            info!(url = mirror.url, "downloading PowerShell 7");
        }
        match download_file(&client, mirror.url, &archive_path).await {
            Ok(()) => {}
            Err(e) => {
                warn!(url = mirror.url, error = %e, "PowerShell 7 download failed; trying next mirror");
                last_err = e;
                continue;
            }
        }

        match sha256_file(&archive_path) {
            Ok(actual) if actual.eq_ignore_ascii_case(mirror.sha256) => {}
            Ok(actual) => {
                warn!(
                    url = mirror.url,
                    expected = mirror.sha256,
                    actual = %actual,
                    "PowerShell 7 archive hash mismatch; trying next mirror"
                );
                last_err = PwshInstallError::HashMismatch {
                    expected: mirror.sha256.to_owned(),
                    actual,
                };
                let _ = tokio::fs::remove_file(&archive_path).await;
                continue;
            }
            Err(e) => {
                last_err = e;
                let _ = tokio::fs::remove_file(&archive_path).await;
                continue;
            }
        }

        let staging = temp_dir.join("extract");
        let _ = tokio::fs::remove_dir_all(&staging).await;
        tokio::fs::create_dir_all(&staging).await?;
        match extract_zip(&archive_path, &staging) {
            Ok(()) => {}
            Err(e) => {
                warn!(url = mirror.url, error = %e, "PowerShell 7 extract failed; trying next mirror");
                last_err = e;
                let _ = tokio::fs::remove_file(&archive_path).await;
                continue;
            }
        }

        let Some(extracted) = find_pwsh_exe(&staging) else {
            last_err = PwshInstallError::Extract("pwsh.exe missing from archive".into());
            continue;
        };
        if !nomi_process_runtime::is_usable_pwsh(&extracted) {
            last_err = PwshInstallError::Extract("extracted pwsh.exe failed PE checks".into());
            continue;
        }

        if dest_dir.exists() {
            tokio::fs::remove_dir_all(&dest_dir).await?;
        }
        tokio::fs::create_dir_all(dest_dir.parent().unwrap_or(&dest_dir)).await?;
        install_tree(&staging, &dest_dir).await?;
        let dest = find_pwsh_exe(&dest_dir).ok_or_else(|| {
            PwshInstallError::Extract("pwsh.exe missing after install".into())
        })?;
        unblock_tree(&dest_dir);
        write_identity(&dest_dir, mirror)?;
        register_override(&dest);
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        if !quiet {
            info!(path = %dest.display(), "PowerShell 7 installed");
        }
        return Ok(dest);
    }

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    Err(last_err)
}

#[cfg(windows)]
async fn install_tree(src: &Path, dst: &Path) -> Result<(), PwshInstallError> {
    match tokio::fs::rename(src, dst).await {
        Ok(()) => Ok(()),
        Err(error) => {
            warn!(
                error = %error,
                src = %src.display(),
                dst = %dst.display(),
                "PowerShell 7 rename across volumes failed; copying tree"
            );
            copy_dir_all(src, dst)?;
            let _ = tokio::fs::remove_dir_all(src).await;
            Ok(())
        }
    }
}

#[cfg(windows)]
fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), PwshInstallError> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

fn write_identity(dir: &Path, mirror: PwshMirror) -> Result<(), PwshInstallError> {
    let body = format!(
        "version={}\nsha256={}\nfilename={}\n",
        PWSH_VERSION, mirror.sha256, mirror.filename
    );
    std::fs::write(dir.join(IDENTITY_FILE), body)?;
    Ok(())
}

fn find_pwsh_exe(root: &Path) -> Option<PathBuf> {
    let direct = root.join(EXE_NAME);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(EXE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn sha256_file(path: &Path) -> Result<String, PwshInstallError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn unblock_tree(root: &Path) {
    fn unblock(path: &Path) {
        let mut ads = path.as_os_str().to_os_string();
        ads.push(":Zone.Identifier");
        let _ = std::fs::remove_file(&ads);
    }
    unblock(root);
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            unblock_tree(&path);
        } else {
            unblock(&path);
        }
    }
}

async fn download_file(client: &Client, url: &str, dest: &Path) -> Result<(), PwshInstallError> {
    let mut request = client.get(url);
    if (url.contains("github.com") || url.contains("githubusercontent.com"))
        && let Ok(token) = std::env::var("GITHUB_TOKEN")
    {
        request = request
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/octet-stream");
    }

    let response = request
        .send()
        .await
        .map_err(|e| PwshInstallError::Download(e.to_string()))?;
    if !response.status().is_success() {
        return Err(PwshInstallError::Download(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| PwshInstallError::Download(e.to_string()))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| PwshInstallError::Download(e.to_string()))?;
    }
    file.flush().await?;
    Ok(())
}

fn extract_zip(archive_path: &Path, dest: &Path) -> Result<(), PwshInstallError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| PwshInstallError::Extract(e.to_string()))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| PwshInstallError::Extract(e.to_string()))?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_mirrors_prefer_modelscope_and_pin_official_hashes() {
        let x64 = mirrors_for("windows", "x86_64");
        assert_eq!(x64[0].url, MODELSCOPE_X64);
        assert_eq!(x64[0].sha256, SHA256_X64);
        assert_eq!(x64[1].url, GITHUB_X64);
        let arm = mirrors_for("windows", "aarch64");
        assert_eq!(arm[0].url, MODELSCOPE_ARM64);
        assert_eq!(arm[0].sha256, SHA256_ARM64);
        assert!(mirrors_for("linux", "x86_64").is_empty());
    }

    #[test]
    fn identity_requires_version_and_hash() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(IDENTITY_FILE),
            format!("version={PWSH_VERSION}\nsha256={SHA256_X64}\n"),
        )
        .unwrap();
        assert!(identity_matches(root.path(), SHA256_X64));
        assert!(!identity_matches(root.path(), SHA256_ARM64));
    }
}
