//! Download ffmpeg into `{data_dir}/bin` for long-video concat.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use reqwest::Client;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use crate::gateway::data_dir;

use super::probe::pick_fastest_url;

#[derive(Debug, Error)]
pub enum FfmpegInstallError {
    #[error("no ffmpeg download mirrors configured for this OS/CPU")]
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
    TarXz,
}

#[derive(Debug, Clone, Copy)]
struct FfmpegMirror {
    url: &'static str,
    format: ArchiveFormat,
    /// True for known lite/essentials/static builds that usually omit hardware
    /// encoders (gyan essentials, johnvansickle static). Only used to sharpen
    /// the missing-hw-encoder hint after a fresh install.
    lite_hw: bool,
}

fn mirror(url: &'static str, format: ArchiveFormat) -> FfmpegMirror {
    FfmpegMirror {
        url,
        format,
        lite_hw: false,
    }
}

fn lite_mirror(url: &'static str, format: ArchiveFormat) -> FfmpegMirror {
    FfmpegMirror {
        url,
        format,
        lite_hw: true,
    }
}

fn platform_mirrors() -> Vec<FfmpegMirror> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("windows", "x86_64") => vec![
            mirror(
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
                ArchiveFormat::Zip,
            ),
            // gyan "essentials" is the default-slim release — may lack NVENC/QSV.
            lite_mirror(
                "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
                ArchiveFormat::Zip,
            ),
        ],
        ("windows", "aarch64") => vec![
            mirror(
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-winarm64-gpl.zip",
                ArchiveFormat::Zip,
            ),
            mirror(
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-winarm64-gpl-7.1.zip",
                ArchiveFormat::Zip,
            ),
        ],
        ("linux", "x86_64") => vec![
            // johnvansickle static builds typically lack NVENC/VAAPI drivers.
            lite_mirror(
                "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz",
                ArchiveFormat::TarXz,
            ),
            mirror(
                "https://ffmpeg.martin-riedl.de/redirect/latest/linux/amd64/release/ffmpeg.zip",
                ArchiveFormat::Zip,
            ),
        ],
        ("linux", "aarch64") => vec![
            lite_mirror(
                "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-arm64-static.tar.xz",
                ArchiveFormat::TarXz,
            ),
            mirror(
                "https://ffmpeg.martin-riedl.de/redirect/latest/linux/arm64/release/ffmpeg.zip",
                ArchiveFormat::Zip,
            ),
        ],
        ("macos", "x86_64") => vec![
            mirror(
                "https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip",
                ArchiveFormat::Zip,
            ),
            mirror(
                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip",
                ArchiveFormat::Zip,
            ),
        ],
        ("macos", "aarch64") => vec![
            mirror(
                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip",
                ArchiveFormat::Zip,
            ),
            mirror(
                "https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip",
                ArchiveFormat::Zip,
            ),
        ],
        _ => Vec::new(),
    }
}

fn managed_ffmpeg_path(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        home.join("bin").join("ffmpeg.exe")
    }
    #[cfg(not(windows))]
    {
        home.join("bin").join("ffmpeg")
    }
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// Download ffmpeg release build into `{data_dir}/bin/ffmpeg[.exe]`.
pub async fn ensure_ffmpeg(quiet: bool) -> Result<PathBuf, FfmpegInstallError> {
    let home = data_dir();
    let dest = managed_ffmpeg_path(&home);
    if dest.is_file() {
        return Ok(dest);
    }

    let mirrors = platform_mirrors();
    if mirrors.is_empty() {
        return Err(FfmpegInstallError::NoMirrors);
    }

    if !quiet {
        info!(
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            mirrors = mirrors.len(),
            "probing ffmpeg mirrors"
        );
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent("allo/dep-install")
        .build()
        .map_err(|e| FfmpegInstallError::Download(e.to_string()))?;

    let urls: Vec<&str> = mirrors.iter().map(|m| m.url).collect();
    let start_idx = match pick_fastest_url(&client, &urls).await {
        Some(idx) => idx,
        None => {
            debug!("ffmpeg mirror probe failed; will try mirrors in order");
            0
        }
    };

    std::fs::create_dir_all(home.join("bin"))?;

    let mut ordered: Vec<FfmpegMirror> = Vec::with_capacity(mirrors.len());
    ordered.push(mirrors[start_idx]);
    for (i, mirror) in mirrors.iter().enumerate() {
        if i != start_idx {
            ordered.push(*mirror);
        }
    }

    let temp_dir = std::env::temp_dir().join(format!("allo-ffmpeg-{}", std::process::id()));
    tokio::fs::create_dir_all(&temp_dir).await?;

    let mut last_err = FfmpegInstallError::Download("no mirror attempted".into());
    for mirror in ordered {
        let archive_path = temp_dir.join(archive_filename(mirror));
        debug!(url = mirror.url, "downloading ffmpeg");
        if !quiet {
            info!(url = mirror.url, "downloading ffmpeg");
        }
        match download_file(&client, mirror.url, &archive_path).await {
            Ok(()) => {}
            Err(e) => {
                warn!(url = mirror.url, error = %e, "ffmpeg download failed; trying next mirror");
                last_err = e;
                continue;
            }
        }

        match extract_ffmpeg(&archive_path, mirror.format, &dest).await {
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
                    info!(path = %dest.display(), "ffmpeg installed");
                }
                // Fresh install: detect lite/essentials/static builds that lack
                // hardware encoders and hint once (concat would fall back to
                // slow software libx264). Only runs on the install path — the
                // already-installed fast path stays probe-free.
                hint_missing_hw_encoders(&dest, mirror.lite_hw).await;
                return Ok(dest);
            }
            Err(e) => {
                warn!(url = mirror.url, error = %e, "ffmpeg extract failed; trying next mirror");
                last_err = e;
                let _ = tokio::fs::remove_file(&archive_path).await;
            }
        }
    }

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    Err(last_err)
}

/// Probe the freshly installed ffmpeg and warn once per process when it has no
/// usable hardware H.264 encoder. `installed_lite` sharpens the message when we
/// know the managed download was a lite/essentials/static build.
async fn hint_missing_hw_encoders(ffmpeg: &Path, installed_lite: bool) {
    use std::sync::OnceLock;
    static HINTED: OnceLock<()> = OnceLock::new();
    if HINTED.get().is_some() {
        return;
    }
    let probe = crate::ffmpeg_hw::probe_ffmpeg_hw(ffmpeg).await;
    if probe.encode_plan.uses_hw {
        return;
    }
    let _ = HINTED.set(());
    let build_note = if installed_lite {
        " The managed download is a lite/essentials/static build. "
    } else {
        " "
    };
    tracing::warn!(
        path = %ffmpeg.display(),
        encoders = ?probe.caps.h264_encoders,
        "ffmpeg has no usable hardware H.264 encoder.{build_note}\
Long-video concat and clip re-encode fall back to slow software libx264. \
Install a full GPL build with NVENC/QSV/AMF/VideoToolbox (e.g. BtbN win64-gpl) \
to enable hardware acceleration."
    );
}

fn archive_filename(mirror: FfmpegMirror) -> String {
    mirror
        .url
        .rsplit('/')
        .next()
        .unwrap_or("ffmpeg-archive")
        .to_string()
}

async fn download_file(client: &Client, url: &str, dest: &Path) -> Result<(), FfmpegInstallError> {
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
        .map_err(|e| FfmpegInstallError::Download(e.to_string()))?;
    if !response.status().is_success() {
        return Err(FfmpegInstallError::Download(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| FfmpegInstallError::Download(e.to_string()))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| FfmpegInstallError::Download(e.to_string()))?;
    }
    file.flush().await?;
    Ok(())
}

async fn extract_ffmpeg(
    archive_path: &Path,
    format: ArchiveFormat,
    dest: &Path,
) -> Result<(), FfmpegInstallError> {
    match format {
        ArchiveFormat::Zip => extract_from_zip(archive_path, dest),
        ArchiveFormat::TarXz => extract_from_tar_xz(archive_path, dest),
    }
}

fn extract_from_zip(archive_path: &Path, dest: &Path) -> Result<(), FfmpegInstallError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| FfmpegInstallError::Extract(e.to_string()))?;
    let target = binary_name();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| FfmpegInstallError::Extract(e.to_string()))?;
        let name = entry.name().replace('\\', "/");
        if name.ends_with(target) || name == target {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(FfmpegInstallError::Extract(format!(
        "{target} not found in zip"
    )))
}

fn extract_from_tar_xz(archive_path: &Path, dest: &Path) -> Result<(), FfmpegInstallError> {
    let file = std::fs::File::open(archive_path)?;
    let decompressor = xz2::read::XzDecoder::new(file);
    let mut archive = tar::Archive::new(decompressor);
    let target = binary_name();

    for entry in archive
        .entries()
        .map_err(|e| FfmpegInstallError::Extract(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| FfmpegInstallError::Extract(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| FfmpegInstallError::Extract(e.to_string()))?;
        if path.file_name().and_then(|n| n.to_str()) == Some(target) {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(FfmpegInstallError::Extract(format!(
        "{target} not found in tar.xz"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_x64_has_zip_mirrors() {
        if std::env::consts::OS == "windows" && std::env::consts::ARCH == "x86_64" {
            let mirrors = platform_mirrors();
            assert!(mirrors.len() >= 2);
            assert!(mirrors.iter().all(|m| m.format == ArchiveFormat::Zip));
        }
    }
}
