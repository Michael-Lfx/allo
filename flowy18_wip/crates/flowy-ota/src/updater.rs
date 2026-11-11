use crate::channel::ChannelConfig;
use crate::error::UpdateError;
use crate::hosts::host_allowed;
use crate::install_gate;
use crate::manifest::{self, Manifest};
use crate::mirrors;
use crate::package::PackageKind;
use crate::rollout;
use crate::verify;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, RANGE};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub release_date: Option<String>,
    pub download_url: String,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ReadyPackage {
    pub version: String,
    pub(crate) path: PathBuf,
    pub(crate) kind: PackageKind,
}

impl ReadyPackage {
    /// Size of the retained package file; used by the download cache measure.
    #[must_use]
    pub fn retained_len(&self) -> u64 {
        fs::metadata(&self.path).map_or(0, |meta| meta.len())
    }
}

pub struct Updater {
    config: ChannelConfig,
    data_dir: PathBuf,
    http: Client,
    locale: Option<String>,
}

impl std::fmt::Debug for Updater {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Updater")
            .field("current_version", &self.config.current_version)
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

impl Updater {
    pub fn new(
        config: ChannelConfig,
        data_dir: PathBuf,
        locale: Option<String>,
    ) -> Result<Self, UpdateError> {
        let http = flowy_net::configure(
            Client::builder()
                .timeout(FETCH_TIMEOUT)
                .user_agent(format!("flowy-ota/{}", config.current_version)),
        )
        .build()
        .map_err(|error| UpdateError::Network(error.to_string()))?;
        fs::create_dir_all(&data_dir)
            .map_err(|error| UpdateError::Install(error.to_string()))?;
        Ok(Self {
            config,
            data_dir,
            http,
            locale,
        })
    }

    pub fn check(&self) -> Result<Option<AvailableUpdate>, UpdateError> {
        let channel = mirrors::host_channel();
        let bucket = mirrors::region_bucket(None, self.locale.as_deref());
        let endpoints =
            mirrors::check_endpoints(&self.config, bucket, channel);
        let mut last_error = None;
        for endpoint in endpoints {
            match self.fetch_manifest(&endpoint) {
                Ok(manifest) => {
                    return self.available_from_manifest(manifest);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            UpdateError::Network("no OTA endpoint responded".into())
        }))
    }

    pub fn download(
        &self,
        version: &str,
        on_progress: &mut dyn FnMut(DownloadProgress),
    ) -> Result<ReadyPackage, UpdateError> {
        let Some(update) = self.check()? else {
            return Err(UpdateError::NotReady);
        };
        if update.version != version {
            return Err(UpdateError::VersionMismatch {
                wanted: version.to_owned(),
                got: update.version,
            });
        }
        let url =
            self.select_artifact_url(&update.version, &update.download_url);
        ensure_allowed_url(&url)?;
        let kind = PackageKind::from_url(&url).ok_or_else(|| {
            UpdateError::Manifest("unknown package kind".into())
        })?;
        if !kind.matches_host() {
            return Err(UpdateError::PlatformUnsupported);
        }
        let bytes = self.download_bytes(&url, on_progress)?;
        verify::verify_bytes(self.config.pubkey, &update.signature, &bytes)?;
        let path = self.package_path(&update.version, kind);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| UpdateError::Install(error.to_string()))?;
        }
        crate_disk_commit(&path, &bytes)?;
        Ok(ReadyPackage {
            version: update.version,
            path,
            kind,
        })
    }

    pub fn install(&self, package: &ReadyPackage) -> Result<(), UpdateError> {
        if !package.path.is_file() {
            return Err(UpdateError::NotReady);
        }
        if !package.kind.matches_host() {
            return Err(UpdateError::PlatformUnsupported);
        }
        match package.kind {
            PackageKind::MacAppArchive => {
                #[cfg(target_os = "macos")]
                {
                    let context = install_gate::install_context();
                    let app = crate::macos_swap::replace_app(
                        &package.path,
                        &context,
                    )?;
                    crate::macos_swap::relaunch_and_exit(&app)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    Err(UpdateError::PlatformUnsupported)
                }
            }
            PackageKind::WindowsSetup | PackageKind::WindowsMsi => {
                #[cfg(target_os = "windows")]
                {
                    crate::windows_install::install_and_exit(
                        &package.path,
                        package.kind,
                    )
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Err(UpdateError::PlatformUnsupported)
                }
            }
            PackageKind::LinuxAppImage => {
                #[cfg(target_os = "linux")]
                {
                    crate::linux_install::install_appimage_and_exit(
                        &package.path,
                    )
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Err(UpdateError::PlatformUnsupported)
                }
            }
        }
    }

    fn available_from_manifest(
        &self,
        manifest: Manifest,
    ) -> Result<Option<AvailableUpdate>, UpdateError> {
        if !manifest::is_newer(self.config.current_version, &manifest.version) {
            return Ok(None);
        }
        let rollout = rollout_percent(&manifest);
        if rollout < 100 {
            let install_id = rollout::load_or_create_install_id(&self.data_dir);
            if !rollout::device_in_rollout(
                &install_id,
                &manifest.version,
                rollout,
            ) {
                return Ok(None);
            }
        }
        let Some(artifact) = manifest::artifact_for_platform(&manifest) else {
            return Err(UpdateError::Manifest(format!(
                "no artifact for {}",
                manifest::platform_key()
            )));
        };
        Ok(Some(AvailableUpdate {
            version: manifest.version,
            current_version: self.config.current_version.to_owned(),
            notes: manifest.notes,
            release_date: manifest.pub_date,
            download_url: artifact.url,
            signature: artifact.signature,
        }))
    }

    fn fetch_manifest(&self, endpoint: &str) -> Result<Manifest, UpdateError> {
        ensure_allowed_url(endpoint)?;
        let response = self
            .http
            .get(endpoint)
            .timeout(FETCH_TIMEOUT)
            .send()
            .map_err(|error| UpdateError::Network(error.to_string()))?;
        if !response.status().is_success() {
            return Err(UpdateError::Network(format!(
                "manifest status {}",
                response.status()
            )));
        }
        let final_url = response.url().clone();
        ensure_allowed_host(final_url.host_str())?;
        response
            .json()
            .map_err(|error| UpdateError::Manifest(error.to_string()))
    }

    fn select_artifact_url(&self, version: &str, download_url: &str) -> String {
        let Some(filename) = mirrors::artifact_filename(download_url) else {
            return download_url.to_owned();
        };
        let folder = mirrors::channel_folder_from_url(download_url)
            .unwrap_or_else(mirrors::host_channel);
        let candidates = mirrors::candidate_artifact_urls(
            &self.config,
            version,
            folder,
            &filename,
        );
        let client = flowy_net::configure(
            Client::builder()
                .timeout(PROBE_TIMEOUT)
                .user_agent("flowy-ota-probe"),
        )
        .build()
        .ok();
        let Some(client) = client else {
            return download_url.to_owned();
        };
        let mut samples = Vec::new();
        for (host, url) in &candidates {
            if let Some(bps) = probe_one(&client, url) {
                samples.push((*host, bps));
            }
        }
        if let Some(host) = mirrors::pick_fastest(&samples)
            && let Some((_, winner)) =
                candidates.iter().find(|(item, _)| *item == host)
        {
            return winner.clone();
        }
        download_url.to_owned()
    }

    fn download_bytes(
        &self,
        url: &str,
        on_progress: &mut dyn FnMut(DownloadProgress),
    ) -> Result<Vec<u8>, UpdateError> {
        let response = self
            .http
            .get(url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .map_err(|error| UpdateError::Network(error.to_string()))?;
        if !response.status().is_success() {
            return Err(UpdateError::Network(format!(
                "download status {}",
                response.status()
            )));
        }
        ensure_allowed_host(response.url().host_str())?;
        let total = response.content_length();
        let mut reader = response;
        let mut bytes = Vec::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| UpdateError::Network(error.to_string()))?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            on_progress(DownloadProgress {
                received: bytes.len() as u64,
                total,
            });
        }
        Ok(bytes)
    }

    fn package_path(&self, version: &str, kind: PackageKind) -> PathBuf {
        self.data_dir
            .join("updates")
            .join(format!("package-{version}.{}", kind.extension()))
    }
}

fn rollout_percent(manifest: &Manifest) -> u8 {
    let value = match &manifest.rollout {
        Some(value) => value.clone(),
        None => return 100,
    };
    let wrapped = Value::Object({
        let mut map = serde_json::Map::new();
        map.insert("rollout".into(), value);
        map
    });
    rollout::parse_rollout_percent(&wrapped)
}

fn probe_one(client: &Client, url: &str) -> Option<u64> {
    ensure_allowed_url(url).ok()?;
    let start = Instant::now();
    let mut headers = HeaderMap::new();
    headers.insert(
        RANGE,
        HeaderValue::from_str(&format!("bytes=0-{}", mirrors::PROBE_RANGE_END))
            .ok()?,
    );
    let response = client.get(url).headers(headers).send().ok()?;
    if !(response.status().is_success() || response.status().as_u16() == 206) {
        return None;
    }
    ensure_allowed_host(response.url().host_str()).ok()?;
    let bytes = response.bytes().ok()?;
    mirrors::probe_bps(bytes.len() as u64, start.elapsed())
}

fn ensure_allowed_url(url: &str) -> Result<(), UpdateError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| UpdateError::Network(error.to_string()))?;
    ensure_allowed_host(parsed.host_str())
}

fn ensure_allowed_host(host: Option<&str>) -> Result<(), UpdateError> {
    let Some(host) = host else {
        return Err(UpdateError::Network("missing host".into()));
    };
    if host_allowed(host) {
        Ok(())
    } else {
        Err(UpdateError::Network(format!("host not allowed: {host}")))
    }
}

fn crate_disk_commit(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| UpdateError::Install(error.to_string()))?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)
        .map_err(|error| UpdateError::Install(error.to_string()))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        UpdateError::Install(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_path_nests_under_updates() {
        let updater = Updater {
            config: ChannelConfig::allo("1.8.0"),
            data_dir: PathBuf::from("/tmp/flowy-ota-test"),
            http: Client::new(),
            locale: None,
        };
        assert!(
            updater
                .package_path("1.9.0", PackageKind::WindowsSetup)
                .ends_with("updates/package-1.9.0.exe")
        );
    }
}
