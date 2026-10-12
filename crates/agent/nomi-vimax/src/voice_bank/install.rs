//! Download, verify, extract, and atomically replace the local voice bank.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::StreamExt;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{info, warn};

use nomifun_common::zip_safe::{
    safe_zip_entry_path, zip_entry_is_symlink, ZipColonPolicy, ZipExtractionBudget,
};
use nomi_config::data_dir;

use super::manifest::{
    override_bank_dir, resolved_version_url, version_newer, RemoteVersionPointer, VoiceBankManifest,
    VOICE_BANK_ZIP_ENV,
};

static INSTALL_LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
const CONNECT_TIMEOUT_SECS: u64 = 20;
const TRANSFER_TIMEOUT_SECS: u64 = 300;
const MAX_ZIP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 512;

#[derive(Debug, Error)]
pub enum VoiceBankInstallError {
    #[error("voice bank download failed: {0}")]
    Download(String),
    #[error("voice bank extract failed: {0}")]
    Extract(String),
    #[error("voice bank io: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct LocalVoiceBank {
    pub root: PathBuf,
    pub manifest: VoiceBankManifest,
}

pub fn current_dir() -> PathBuf {
    data_dir().join("voice-bank").join("current")
}

fn staging_dir() -> PathBuf {
    data_dir()
        .join("voice-bank")
        .join(format!("staging-{}", std::process::id()))
}

fn install_lock() -> Arc<Mutex<()>> {
    INSTALL_LOCK
        .get_or_init(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Load the extracted pack. `FLOWY_VOICE_BANK_DIR` wins for local testing.
pub fn load_local_bank() -> Option<LocalVoiceBank> {
    if let Some(dir) = override_bank_dir() {
        return load_bank_at(&dir);
    }
    load_bank_at(&current_dir())
}

fn load_bank_at(root: &Path) -> Option<LocalVoiceBank> {
    let manifest = VoiceBankManifest::load_from_dir(root).ok()?;
    if manifest.usable_voices(root).is_empty() {
        return None;
    }
    Some(LocalVoiceBank {
        root: root.to_path_buf(),
        manifest,
    })
}

fn local_version() -> Option<String> {
    load_local_bank().map(|b| b.manifest.version)
}

/// Fetch remote `version.json` (when configured) and replace the local pack if newer.
pub async fn ensure_installed() -> Result<Option<LocalVoiceBank>, VoiceBankInstallError> {
    if override_bank_dir().is_some() {
        return Ok(load_local_bank());
    }
    let lock = install_lock();
    let _guard = lock.lock().await;

    if let Some(url) = resolved_version_url() {
        match sync_from_remote(&url).await {
            Ok(()) => {}
            Err(err) => {
                if load_local_bank().is_some() {
                    warn!(error = %err, "voice bank update failed; keeping installed pack");
                } else {
                    return Err(err);
                }
            }
        }
    } else if let Ok(zip) = std::env::var(VOICE_BANK_ZIP_ENV) {
        let zip = zip.trim().to_string();
        if !zip.is_empty() && load_local_bank().is_none() {
            download_and_install(&zip, None).await?;
        }
    }

    Ok(load_local_bank())
}

async fn sync_from_remote(version_url: &str) -> Result<(), VoiceBankInstallError> {
    let pointer = fetch_version_pointer(version_url).await?;
    let local = local_version().unwrap_or_default();
    if !version_newer(&pointer.version, &local) {
        info!(
            local = %local,
            remote = %pointer.version,
            "voice bank already up to date"
        );
        return Ok(());
    }
    let zip_url = pointer.resolve_zip_url(version_url).ok_or_else(|| {
        VoiceBankInstallError::Download("remote version.json has no zip url".into())
    })?;
    info!(
        local = %local,
        remote = %pointer.version,
        url = %zip_url,
        "updating voice bank"
    );
    download_and_install(&zip_url, pointer.sha256.as_deref()).await
}

async fn fetch_version_pointer(
    url: &str,
) -> Result<RemoteVersionPointer, VoiceBankInstallError> {
    let client = http_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| VoiceBankInstallError::Download(e.to_string()))?;
    let status = response.status();
    if status.as_u16() == 404 {
        return Err(VoiceBankInstallError::Download(format!(
            "voice bank not hosted yet ({url} HTTP 404)"
        )));
    }
    if !status.is_success() {
        return Err(VoiceBankInstallError::Download(format!(
            "HTTP {status} fetching {url}"
        )));
    }
    let pointer = response
        .json::<RemoteVersionPointer>()
        .await
        .map_err(|e| VoiceBankInstallError::Download(format!("parse version.json: {e}")))?;
    if pointer.version.trim().is_empty() {
        return Err(VoiceBankInstallError::Download(
            "version.json missing version".into(),
        ));
    }
    Ok(pointer)
}

fn http_client() -> Result<reqwest::Client, VoiceBankInstallError> {
    nomifun_net::proxy::apply_detected_proxy(
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(TRANSFER_TIMEOUT_SECS))
            .user_agent("allo/voice-bank"),
    )
    .build()
    .map_err(|e| VoiceBankInstallError::Download(e.to_string()))
}

async fn download_and_install(
    zip_url: &str,
    expected_sha256: Option<&str>,
) -> Result<(), VoiceBankInstallError> {
    let home = data_dir().join("voice-bank");
    tokio::fs::create_dir_all(&home).await?;
    let zip_path = home.join("download.zip");
    if zip_path.exists() {
        let _ = tokio::fs::remove_file(&zip_path).await;
    }
    let digest = download_zip(zip_url, &zip_path).await?;
    if let Some(expected) = expected_sha256.map(str::trim).filter(|s| !s.is_empty()) {
        if !digest.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&zip_path).await;
            return Err(VoiceBankInstallError::Download(format!(
                "sha256 mismatch: got {digest}, expected {expected}"
            )));
        }
    }
    let staging = staging_dir();
    if staging.exists() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    tokio::fs::create_dir_all(&staging).await?;
    let staging_clone = staging.clone();
    let zip_clone = zip_path.clone();
    tokio::task::spawn_blocking(move || extract_zip_safe(&zip_clone, &staging_clone))
        .await
        .map_err(|e| VoiceBankInstallError::Extract(e.to_string()))??;
    let unpacked = locate_unpacked_root(&staging)?;
    VoiceBankManifest::load_from_dir(&unpacked)
        .map_err(VoiceBankInstallError::Extract)?;
    if VoiceBankManifest::load_from_dir(&unpacked)
        .ok()
        .map(|m| m.usable_voices(&unpacked).len())
        .unwrap_or(0)
        == 0
    {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(VoiceBankInstallError::Extract(
            "extracted pack has no usable wav clips".into(),
        ));
    }
    atomic_replace_current(&unpacked).await?;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    let _ = tokio::fs::remove_file(&zip_path).await;
    info!(path = %current_dir().display(), sha256 = %digest, "voice bank installed");
    Ok(())
}

async fn download_zip(url: &str, dest: &Path) -> Result<String, VoiceBankInstallError> {
    let client = http_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| VoiceBankInstallError::Download(e.to_string()))?;
    if !response.status().is_success() {
        return Err(VoiceBankInstallError::Download(format!(
            "HTTP {} downloading {url}",
            response.status()
        )));
    }
    if let Some(len) = response.content_length() {
        if len > MAX_ZIP_BYTES {
            return Err(VoiceBankInstallError::Download(format!(
                "zip too large ({len} bytes)"
            )));
        }
    }
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| VoiceBankInstallError::Download(e.to_string()))?;
        written += chunk.len() as u64;
        if written > MAX_ZIP_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            return Err(VoiceBankInstallError::Download(
                "zip exceeded 64 MiB cap".into(),
            ));
        }
        hasher.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn extract_zip_safe(archive_path: &Path, destination: &Path) -> Result<(), VoiceBankInstallError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| VoiceBankInstallError::Extract(e.to_string()))?;
    let mut budget = ZipExtractionBudget::new(MAX_UNCOMPRESSED_BYTES, MAX_ZIP_ENTRIES);
    budget
        .check_entry_count(archive.len())
        .map_err(|e| VoiceBankInstallError::Extract(e.to_string()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| VoiceBankInstallError::Extract(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        if zip_entry_is_symlink(entry.unix_mode()) {
            return Err(VoiceBankInstallError::Extract(format!(
                "refusing symlink entry {}",
                entry.name()
            )));
        }
        let Some(relative) = safe_zip_entry_path(entry.name(), ZipColonPolicy::RejectDrivePrefix)
        else {
            return Err(VoiceBankInstallError::Extract(format!(
                "unsafe zip path {}",
                entry.name()
            )));
        };
        let target = destination.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        let bytes = io::copy(&mut entry, &mut out)?;
        out.flush()?;
        budget
            .record_written(bytes)
            .map_err(|e| VoiceBankInstallError::Extract(e.to_string()))?;
    }
    Ok(())
}

/// Zip may contain `manifest.json` at root or under one folder.
fn locate_unpacked_root(staging: &Path) -> Result<PathBuf, VoiceBankInstallError> {
    if staging.join("manifest.json").is_file() {
        return Ok(staging.to_path_buf());
    }
    let mut found = None;
    for entry in std::fs::read_dir(staging)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join("manifest.json").is_file() {
            if found.is_some() {
                return Err(VoiceBankInstallError::Extract(
                    "zip contains multiple manifest.json trees".into(),
                ));
            }
            found = Some(path);
        }
    }
    found.ok_or_else(|| VoiceBankInstallError::Extract("zip missing manifest.json".into()))
}

async fn atomic_replace_current(unpacked: &Path) -> Result<(), VoiceBankInstallError> {
    let dest = current_dir();
    let parent = dest
        .parent()
        .ok_or_else(|| VoiceBankInstallError::Other("voice-bank dest has no parent".into()))?;
    tokio::fs::create_dir_all(parent).await?;
    let backup = parent.join("current.bak");
    if backup.exists() {
        let _ = tokio::fs::remove_dir_all(&backup).await;
    }
    if dest.exists() {
        tokio::fs::rename(&dest, &backup).await?;
    }
    match tokio::fs::rename(unpacked, &dest).await {
        Ok(()) => {
            let _ = tokio::fs::remove_dir_all(&backup).await;
            Ok(())
        }
        Err(err) => {
            if backup.exists() && !dest.exists() {
                let _ = tokio::fs::rename(&backup, &dest).await;
            }
            Err(err.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_bank::manifest::VoiceEntry;
    use crate::voice_bank::{VoiceAge, VoiceGender};
    use zip::write::SimpleFileOptions;

    fn tiny_wav() -> Vec<u8> {
        let mut bytes = Vec::from(&b"RIFF"[..]);
        bytes.extend_from_slice(&316u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24000u32.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&280u32.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0u8, 280));
        bytes
    }

    #[test]
    fn extract_rejects_zip_slip() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("../escape.wav", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"nope").unwrap();
            zip.finish().unwrap();
        }
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        let err = extract_zip_safe(&zip_path, &dest).unwrap_err();
        assert!(err.to_string().contains("unsafe"), "{err}");
    }

    #[test]
    fn extract_and_locate_nested_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("pack.zip");
        let manifest = VoiceBankManifest {
            schema_version: 1,
            version: "1.0.0".into(),
            name: Some("test".into()),
            voices: vec![VoiceEntry {
                id: "zh_f_adult_01".into(),
                file: "wav/a.wav".into(),
                language: "zh".into(),
                gender: VoiceGender::Female,
                age: VoiceAge::Adult,
                pitch: None,
                tags: vec!["清亮".into()],
                source: None,
                transcript: None,
            }],
        };
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = SimpleFileOptions::default();
            zip.start_file("voice-bank/manifest.json", opts).unwrap();
            zip.write_all(serde_json::to_vec_pretty(&manifest).unwrap().as_slice())
                .unwrap();
            zip.start_file("voice-bank/wav/a.wav", opts).unwrap();
            zip.write_all(&tiny_wav()).unwrap();
            zip.finish().unwrap();
        }
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        extract_zip_safe(&zip_path, &dest).unwrap();
        let root = locate_unpacked_root(&dest).unwrap();
        let loaded = VoiceBankManifest::load_from_dir(&root).unwrap();
        assert_eq!(loaded.version, "1.0.0");
        assert_eq!(loaded.usable_voices(&root).len(), 1);
    }
}
