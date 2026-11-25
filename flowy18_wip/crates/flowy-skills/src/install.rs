use crate::{
    ArchiveError, DownloadBody, GitHubHandoff, HttpBytes, HttpText,
    InstallTransport, MAX_PACKAGE_BYTES, NetTransport, ScanError, SkillId,
    Store, StoreError, TransportError, classify_download, collect_tree_hashes,
    content_hash_matches, extract_github_skill, extract_skill_zip,
    normalize_relative, parse_skill_id, percent_encode, retry_delay, scan_tree,
    sha256_hex,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStep {
    Resolve,
    Verify,
    Fetch,
    Hash,
    Scan,
    Stage,
    Commit,
}

impl InstallStep {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::Verify => "verify",
            Self::Fetch => "fetch",
            Self::Hash => "hash",
            Self::Scan => "scan",
            Self::Stage => "stage",
            Self::Commit => "commit",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "resolve" => Some(Self::Resolve),
            "verify" => Some(Self::Verify),
            "fetch" => Some(Self::Fetch),
            "hash" => Some(Self::Hash),
            "scan" => Some(Self::Scan),
            "stage" => Some(Self::Stage),
            "commit" => Some(Self::Commit),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstallOrigin {
    Official,
    #[default]
    Community,
}

impl InstallOrigin {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Community => "community",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "official" => Some(Self::Official),
            "community" => Some(Self::Community),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallProgress {
    Verifying,
    Downloading,
    Validating,
    Scanning,
    Committing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallRecord {
    pub id: SkillId,
    pub step: InstallStep,
    pub auto_invoke: bool,
    pub scripts_enabled: bool,
    pub content_hash: String,
    pub origin: InstallOrigin,
    pub display_name: String,
    pub summary: String,
    pub installed_at: i64,
    pub latest_version: Option<String>,
    pub checked_at: Option<i64>,
}

impl InstallRecord {
    #[must_use]
    pub const fn community(id: SkillId) -> Self {
        Self {
            id,
            step: InstallStep::Resolve,
            auto_invoke: false,
            scripts_enabled: false,
            content_hash: String::new(),
            origin: InstallOrigin::Community,
            display_name: String::new(),
            summary: String::new(),
            installed_at: 0,
            latest_version: None,
            checked_at: None,
        }
    }

    #[must_use]
    pub fn official(
        id: SkillId,
        title: String,
        summary: String,
        auto_invoke: bool,
    ) -> Self {
        Self {
            id,
            step: InstallStep::Commit,
            auto_invoke,
            scripts_enabled: true,
            content_hash: String::new(),
            origin: InstallOrigin::Official,
            display_name: title,
            summary,
            installed_at: unix_now(),
            latest_version: None,
            checked_at: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallEvent {
    Verified { ok: bool },
    Fetched,
    Hashed { expected: String, actual: String },
    Scanned,
    Staged,
    Committed,
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("skill verification rejected installation: {0}")]
    Rejected(String),
    #[error("skill verification response is invalid: {0}")]
    InvalidVerify(String),
    #[error("skill transport: {0}")]
    Transport(#[from] TransportError),
    #[error("skill archive: {0}")]
    Archive(#[from] ArchiveError),
    #[error("skill scan: {0}")]
    Scan(#[from] ScanError),
    #[error("skill store: {0}")]
    Store(#[from] StoreError),
    #[error("skill filesystem: {0}")]
    Io(#[from] io::Error),
    #[error("content hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("event is not valid at install step {step:?}")]
    Unexpected { step: InstallStep },
    #[error("skill package content hash does not match: expected {expected}")]
    ContentHashMismatch { expected: String },
    #[error("skill is already installed at {0}")]
    AlreadyInstalled(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReceipt {
    pub id: SkillId,
    pub display_name: String,
    pub summary: String,
    pub files: Vec<VerifiedFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOutcome {
    pub record: InstallRecord,
    pub root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateCheck {
    pub current: SkillId,
    pub latest: SkillId,
    pub available: bool,
}

#[must_use]
pub fn begin_community_install(id: &SkillId) -> InstallRecord {
    InstallRecord::community(id.clone())
}

pub fn advance(
    record: &InstallRecord,
    event: InstallEvent,
) -> Result<InstallRecord, InstallError> {
    let mut next = record.clone();
    match (record.step, event) {
        (InstallStep::Resolve, InstallEvent::Verified { ok: true }) => {
            next.step = InstallStep::Verify;
        }
        (InstallStep::Resolve, InstallEvent::Verified { ok: false }) => {
            return Err(InstallError::Rejected("verification failed".into()));
        }
        (InstallStep::Verify, InstallEvent::Fetched) => {
            next.step = InstallStep::Fetch;
        }
        (InstallStep::Fetch, InstallEvent::Hashed { expected, actual }) => {
            if expected != actual {
                return Err(InstallError::HashMismatch { expected, actual });
            }
            next.content_hash = actual;
            next.step = InstallStep::Hash;
        }
        (InstallStep::Hash, InstallEvent::Scanned) => {
            next.step = InstallStep::Scan;
        }
        (InstallStep::Scan, InstallEvent::Staged) => {
            next.step = InstallStep::Stage;
        }
        (InstallStep::Stage, InstallEvent::Committed) => {
            next.step = InstallStep::Commit;
        }
        (step, _) => return Err(InstallError::Unexpected { step }),
    }
    Ok(next)
}

#[must_use]
pub fn verify_url(id: &SkillId) -> String {
    format!(
        "https://clawhub.ai/api/v1/skills/{}/verify?ownerHandle={}&tag=latest",
        percent_encode(&id.slug),
        percent_encode(&id.owner_handle)
    )
}

#[must_use]
pub fn download_url(id: &SkillId) -> String {
    format!(
        "https://clawhub.ai/api/v1/download?slug={}&ownerHandle={}&version={}",
        percent_encode(&id.slug),
        percent_encode(&id.owner_handle),
        percent_encode(&id.version)
    )
}

pub fn accept_verify_body(body: &str) -> Result<(), InstallError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| InstallError::InvalidVerify(error.to_string()))?;
    let passed = value.get("ok").and_then(Value::as_bool) == Some(true)
        && value.get("decision").and_then(Value::as_str) == Some("pass")
        && value
            .get("security")
            .and_then(|security| security.get("status"))
            .and_then(Value::as_str)
            == Some("clean");
    if passed {
        Ok(())
    } else {
        Err(InstallError::Rejected(
            "verify did not pass the security gate".into(),
        ))
    }
}

pub fn parse_verify_body(
    body: &str,
    requested: &SkillId,
) -> Result<VerifyReceipt, InstallError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| InstallError::InvalidVerify(error.to_string()))?;
    if value.get("ok").and_then(Value::as_bool) != Some(true)
        || value.get("decision").and_then(Value::as_str) != Some("pass")
    {
        return Err(InstallError::Rejected(
            "verify did not return ok=true and decision=pass".into(),
        ));
    }
    let security = value
        .get("security")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            InstallError::InvalidVerify("missing security".into())
        })?;
    if security.get("status").and_then(Value::as_str) != Some("clean") {
        return Err(InstallError::Rejected(
            "security status is not clean".into(),
        ));
    }
    let owner = value
        .get("publisherHandle")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            InstallError::InvalidVerify("missing publisherHandle".into())
        })?;
    let slug = value
        .get("slug")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| InstallError::InvalidVerify("missing slug".into()))?;
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            InstallError::InvalidVerify("missing resolved version".into())
        })?;
    let id = parse_skill_id(owner, slug, version)
        .map_err(|error| InstallError::InvalidVerify(error.to_string()))?;
    if id.owner_handle != requested.owner_handle || id.slug != requested.slug {
        return Err(InstallError::Rejected(
            "verify identity does not match the requested skill".into(),
        ));
    }
    let files = value
        .get("artifact")
        .and_then(|artifact| artifact.get("files"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            InstallError::InvalidVerify("missing artifact files".into())
        })?;
    if files.is_empty() || files.len() > 200 {
        return Err(InstallError::InvalidVerify(
            "artifact file count is invalid".into(),
        ));
    }
    let mut verified = Vec::with_capacity(files.len());
    let mut total = 0_u64;
    for file in files {
        let path = file
            .get("path")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                InstallError::InvalidVerify("file path missing".into())
            })?;
        let sha256 = file
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|value| value.len() == 64)
            .ok_or_else(|| {
                InstallError::InvalidVerify("file sha256 missing".into())
            })?;
        let size =
            file.get("size").and_then(Value::as_u64).ok_or_else(|| {
                InstallError::InvalidVerify("file size missing".into())
            })?;
        total = total.saturating_add(size);
        verified.push(VerifiedFile {
            path: normalize_relative(path),
            sha256: sha256.to_ascii_lowercase(),
            size,
        });
    }
    if total > MAX_PACKAGE_BYTES {
        return Err(InstallError::Rejected(
            "verified files exceed the package size cap".into(),
        ));
    }
    if !verified.iter().any(|file| file.path == "SKILL.md") {
        return Err(InstallError::Rejected("artifact has no SKILL.md".into()));
    }
    let display_name = value
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or(&id.slug)
        .to_owned();
    let summary = value
        .get("card")
        .and_then(|_| value.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(VerifyReceipt {
        id,
        display_name,
        summary,
        files: verified,
    })
}

pub struct SkillInstaller<T> {
    root: PathBuf,
    transport: T,
    sleeper: Arc<dyn Fn(Duration) + Send + Sync>,
    fixed_jitter_ms: Option<u64>,
}

enum InstallPackage {
    Zip(Vec<u8>),
    GitHub {
        handoff: GitHubHandoff,
        archive: Vec<u8>,
    },
}

impl<T> std::fmt::Debug for SkillInstaller<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkillInstaller")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl SkillInstaller<NetTransport> {
    pub fn network(root: impl Into<PathBuf>) -> Result<Self, InstallError> {
        let transport = NetTransport::new()?;
        Ok(Self::new(root, transport))
    }
}

impl<T: crate::CatalogTransport + InstallTransport> SkillInstaller<T> {
    pub fn new(root: impl Into<PathBuf>, transport: T) -> Self {
        Self {
            root: root.into(),
            transport,
            sleeper: Arc::new(thread::sleep),
            fixed_jitter_ms: None,
        }
    }

    #[cfg(test)]
    fn with_controls<S>(
        root: impl Into<PathBuf>,
        transport: T,
        sleeper: S,
        jitter_ms: u64,
    ) -> Self
    where
        S: Fn(Duration) + Send + Sync + 'static,
    {
        Self {
            root: root.into(),
            transport,
            sleeper: Arc::new(sleeper),
            fixed_jitter_ms: Some(jitter_ms),
        }
    }

    pub fn install(
        &self,
        requested: &SkillId,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<InstallOutcome, InstallError> {
        self.install_with_previous(requested, None, progress)
    }

    pub fn install_update(
        &self,
        requested: &SkillId,
        previous: &InstallRecord,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<InstallOutcome, InstallError> {
        self.install_with_previous(requested, Some(previous), progress)
    }

    fn install_with_previous(
        &self,
        requested: &SkillId,
        previous: Option<&InstallRecord>,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<InstallOutcome, InstallError> {
        progress(InstallProgress::Verifying);
        let mut record = begin_community_install(requested);
        let verify = self.fetch_verify(requested)?;
        record.id = verify.id.clone();
        record = advance(&record, InstallEvent::Verified { ok: true })?;
        progress(InstallProgress::Downloading);
        let downloaded = self.fetch_package(&verify.id)?;
        record = advance(&record, InstallEvent::Fetched)?;
        progress(InstallProgress::Validating);
        let staging = self.staging_path();
        let install_root = install_dir(&self.root, &verify.id);
        if install_root.exists() {
            let store = Store::open(&self.db_path())?;
            let recorded = store
                .installed()?
                .iter()
                .any(|record| record.id == verify.id);
            if recorded {
                return Err(InstallError::AlreadyInstalled(
                    install_root.display().to_string(),
                ));
            }
            fs::remove_dir_all(&install_root)?;
        }
        let content_hash = match &downloaded {
            InstallPackage::Zip(bytes) => {
                extract_skill_zip(bytes, &staging)?;
                sha256_hex(bytes)
            }
            InstallPackage::GitHub { handoff, archive } => {
                extract_github_skill(archive, &handoff.path, &staging)?;
                if !content_hash_matches(
                    &handoff.content_hash,
                    archive,
                    &staging,
                )? {
                    return Err(InstallError::ContentHashMismatch {
                        expected: handoff.content_hash.clone(),
                    });
                }
                handoff.content_hash.clone()
            }
        };
        record = advance(
            &record,
            InstallEvent::Hashed {
                expected: content_hash.clone(),
                actual: content_hash.clone(),
            },
        )?;
        let guard = StagingGuard(Some(staging.clone()));
        verify_tree(&staging, &verify.files)?;
        progress(InstallProgress::Scanning);
        scan_tree(&staging)?;
        record = advance(&record, InstallEvent::Scanned)?;
        record = advance(&record, InstallEvent::Staged)?;
        progress(InstallProgress::Committing);
        if let Some(parent) = install_root.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&staging, &install_root)?;
        guard.disarm();
        record = advance(&record, InstallEvent::Committed)?;
        record.auto_invoke = false;
        record.scripts_enabled = false;
        record.content_hash = content_hash;
        record.display_name = verify.display_name;
        record.summary = verify.summary;
        record.installed_at = unix_now();
        record.latest_version = Some(record.id.version.clone());
        record.checked_at = Some(unix_now());
        let store = Store::open(&self.db_path())?;
        let commit = if let Some(previous) =
            previous.filter(|previous| previous.id != record.id)
        {
            record.auto_invoke = previous.auto_invoke;
            store.replace_install(&previous.id, &record)
        } else {
            store.upsert_install(&record)
        };
        if let Err(error) = commit {
            let _ = fs::remove_dir_all(&install_root);
            return Err(error.into());
        }
        Ok(InstallOutcome {
            record,
            root: install_root,
        })
    }

    pub fn check_update(
        &self,
        record: &InstallRecord,
    ) -> Result<UpdateCheck, InstallError> {
        let verify = self.fetch_verify(&record.id)?;
        Ok(UpdateCheck {
            current: record.id.clone(),
            latest: verify.id.clone(),
            available: version_is_newer(&verify.id.version, &record.id.version),
        })
    }

    pub fn remove(&self, record: &InstallRecord) -> Result<(), InstallError> {
        if record.origin == InstallOrigin::Official {
            return Err(InstallError::Rejected(
                "official skills cannot be removed".into(),
            ));
        }
        let root = install_dir(&self.root, &record.id);
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        Store::open(&self.db_path())?.remove_install(&record.id)?;
        Ok(())
    }

    fn fetch_verify(
        &self,
        requested: &SkillId,
    ) -> Result<VerifyReceipt, InstallError> {
        let response = self.request_text_with_retry(&verify_url(requested))?;
        if response.status != 200 {
            return Err(InstallError::Rejected(format!(
                "verify HTTP status {}",
                response.status
            )));
        }
        parse_verify_body(&response.body, requested)
    }

    fn fetch_package(
        &self,
        id: &SkillId,
    ) -> Result<InstallPackage, InstallError> {
        let response = self.request_bytes_with_retry(&download_url(id))?;
        if response.status != 200 {
            return Err(InstallError::Rejected(format!(
                "download HTTP status {}",
                response.status
            )));
        }
        if u64::try_from(response.body.len()).unwrap_or(u64::MAX)
            > MAX_PACKAGE_BYTES
        {
            return Err(InstallError::Archive(ArchiveError::TooLarge));
        }
        match classify_download(&response.content_type, &response.body)? {
            DownloadBody::Zip(bytes) => Ok(InstallPackage::Zip(bytes)),
            DownloadBody::GitHub(handoff) => {
                let archive =
                    self.request_bytes_with_retry(&handoff.archive_url)?;
                if archive.status != 200 {
                    return Err(InstallError::Rejected(format!(
                        "GitHub archive HTTP status {}",
                        archive.status
                    )));
                }
                if u64::try_from(archive.body.len()).unwrap_or(u64::MAX)
                    > MAX_PACKAGE_BYTES
                {
                    return Err(InstallError::Archive(ArchiveError::TooLarge));
                }
                Ok(InstallPackage::GitHub {
                    handoff,
                    archive: archive.body,
                })
            }
        }
    }

    fn request_text_with_retry(
        &self,
        url: &str,
    ) -> Result<HttpText, InstallError> {
        let first = self.transport.get_text(url)?;
        match retry_delay(
            first.status,
            first.retry_after_secs,
            self.jitter_ms(),
        ) {
            Some(delay) => {
                (self.sleeper)(Duration::from_millis(delay));
                Ok(self.transport.get_text(url)?)
            }
            None => Ok(first),
        }
    }

    fn request_bytes_with_retry(
        &self,
        url: &str,
    ) -> Result<HttpBytes, InstallError> {
        let first = self.transport.get_bytes(url)?;
        match retry_delay(
            first.status,
            first.retry_after_secs,
            self.jitter_ms(),
        ) {
            Some(delay) => {
                (self.sleeper)(Duration::from_millis(delay));
                Ok(self.transport.get_bytes(url)?)
            }
            None => Ok(first),
        }
    }

    fn jitter_ms(&self) -> u64 {
        self.fixed_jitter_ms.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| u64::from(elapsed.subsec_nanos()) % 251)
        })
    }

    fn staging_path(&self) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        self.root
            .join("skills")
            .join("staging")
            .join(format!("install-{}-{stamp}-{sequence}", std::process::id()))
    }

    fn db_path(&self) -> PathBuf {
        self.root.join("skills").join("market.sqlite3")
    }
}

pub fn install_dir(root: &Path, id: &SkillId) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(id.owner_handle.as_bytes());
    digest.update([0]);
    digest.update(id.slug.as_bytes());
    digest.update([0]);
    digest.update(id.version.as_bytes());
    root.join("skills")
        .join("installed")
        .join(format!("{:x}", digest.finalize()))
}

pub fn cleanup_staging(root: &Path) {
    let staging = root.join("skills").join("staging");
    if let Ok(entries) = fs::read_dir(staging) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            }
        }
    }
}

struct StagingGuard(Option<PathBuf>);

impl StagingGuard {
    fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn verify_tree(
    root: &Path,
    files: &[VerifiedFile],
) -> Result<(), InstallError> {
    let expected = files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let actual = collect_tree_hashes(root)?;
    for path in expected.keys() {
        if !actual.contains_key(path) {
            return Err(InstallError::Rejected(format!(
                "artifact file is missing: {path}"
            )));
        }
    }
    for (path, hash) in &actual {
        let Some(file) = expected.get(path) else {
            if is_clawhub_metadata(path) {
                continue;
            }
            return Err(InstallError::Rejected(format!(
                "unexpected artifact file: {path}"
            )));
        };
        if hash != &file.sha256 {
            return Err(InstallError::HashMismatch {
                expected: file.sha256.clone(),
                actual: hash.clone(),
            });
        }
    }
    Ok(())
}

fn is_clawhub_metadata(path: &str) -> bool {
    matches!(path, "skill-card.md" | "_meta.json")
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |value: &str| {
        value
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()
    };
    match (parse(latest), parse(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => latest != current,
    }
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CatalogTransport, HttpBytes, HttpText, UNVERSIONED};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::io::{Cursor, Write};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zip::write::SimpleFileOptions;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "flowy-skills-install-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct FakeTransport {
        verify: String,
        package: Vec<u8>,
    }

    impl CatalogTransport for FakeTransport {
        fn get_text(&self, _url: &str) -> Result<HttpText, TransportError> {
            Ok(HttpText {
                status: 200,
                body: self.verify.clone(),
                retry_after_secs: None,
            })
        }

        fn post_json(
            &self,
            _url: &str,
            _body: &Value,
        ) -> Result<HttpText, TransportError> {
            unreachable!()
        }
    }

    impl InstallTransport for FakeTransport {
        fn get_bytes(&self, _url: &str) -> Result<HttpBytes, TransportError> {
            Ok(HttpBytes {
                status: 200,
                content_type: "application/zip".into(),
                body: self.package.clone(),
                retry_after_secs: None,
            })
        }
    }

    struct QueueTransport {
        text: Mutex<VecDeque<HttpText>>,
        bytes: Mutex<VecDeque<HttpBytes>>,
    }

    impl QueueTransport {
        fn new(text: Vec<HttpText>, bytes: Vec<HttpBytes>) -> Self {
            Self {
                text: Mutex::new(text.into()),
                bytes: Mutex::new(bytes.into()),
            }
        }
    }

    impl CatalogTransport for QueueTransport {
        fn get_text(&self, _url: &str) -> Result<HttpText, TransportError> {
            Ok(self.text.lock().unwrap().pop_front().unwrap())
        }

        fn post_json(
            &self,
            _url: &str,
            _body: &Value,
        ) -> Result<HttpText, TransportError> {
            unreachable!()
        }
    }

    impl InstallTransport for QueueTransport {
        fn get_bytes(&self, _url: &str) -> Result<HttpBytes, TransportError> {
            Ok(self.bytes.lock().unwrap().pop_front().unwrap())
        }
    }

    fn http_text(
        status: u16,
        body: impl Into<String>,
        retry_after_secs: Option<u64>,
    ) -> HttpText {
        HttpText {
            status,
            body: body.into(),
            retry_after_secs,
        }
    }

    fn http_bytes(
        status: u16,
        content_type: &str,
        body: Vec<u8>,
        retry_after_secs: Option<u64>,
    ) -> HttpBytes {
        HttpBytes {
            status,
            content_type: content_type.into(),
            body,
            retry_after_secs,
        }
    }

    fn github_archive() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "repo-abc/skills/demo/SKILL.md",
                SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(
                b"---\nname: demo\ndescription: Demo skill.\n---\n\nUse the demo.\n",
            )
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn package() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("SKILL.md", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(
                b"---\nname: demo\ndescription: Demo skill.\n---\n\nUse the demo.\n",
            )
            .unwrap();
        writer
            .start_file("skill-card.md", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"# Demo card\n").unwrap();
        writer
            .start_file("_meta.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"{\"slug\":\"demo\",\"version\":\"2.0.0\"}")
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn verify_body(version: &str, package: &[u8]) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(package)).unwrap();
        let mut skill = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("SKILL.md").unwrap(),
            &mut skill,
        )
        .unwrap();
        json!({
            "ok": true,
            "decision": "pass",
            "publisherHandle": "owner",
            "slug": "demo",
            "version": version,
            "displayName": "Demo",
            "security": { "status": "clean" },
            "artifact": {
                "files": [{
                    "path": "SKILL.md",
                    "sha256": sha256_hex(skill.as_bytes()),
                    "size": skill.len()
                }]
            }
        })
        .to_string()
    }

    #[test]
    fn unversioned_verify_resolves_and_installs() {
        let dir = TestDir::new("verified");
        let package = package();
        let transport = FakeTransport {
            verify: verify_body("2.0.0", &package),
            package,
        };
        let installer = SkillInstaller::new(dir.0.clone(), transport);
        let requested = parse_skill_id("owner", "demo", UNVERSIONED).unwrap();
        let mut stages = Vec::new();
        let outcome = installer
            .install(&requested, &mut |stage| stages.push(stage))
            .unwrap();
        assert_eq!(outcome.record.id.version, "2.0.0");
        assert_eq!(outcome.record.step, InstallStep::Commit);
        assert!(!outcome.record.auto_invoke);
        assert!(!outcome.record.scripts_enabled);
        assert!(stages.contains(&InstallProgress::Verifying));
        assert!(stages.contains(&InstallProgress::Committing));
        let catalog = crate::load_installed(&dir.0).unwrap();
        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].name, "demo");
        assert_eq!(catalog.skills[0].id.version, "2.0.0");
        assert!(!catalog.skills[0].auto_invoke);
        assert!(!catalog.skills[0].scripts_enabled);
    }

    #[test]
    fn install_retries_rate_limits_once_after_server_delay() {
        let dir = TestDir::new("retry");
        let package = package();
        let verify = verify_body("2.0.0", &package);
        let transport = QueueTransport::new(
            vec![http_text(429, "", Some(2)), http_text(200, verify, None)],
            vec![
                http_bytes(503, "application/zip", Vec::new(), Some(1)),
                http_bytes(200, "application/zip", package, None),
            ],
        );
        let sleeps = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&sleeps);
        let installer = SkillInstaller::with_controls(
            dir.0.clone(),
            transport,
            move |delay| recorder.lock().unwrap().push(delay),
            0,
        );
        let requested = parse_skill_id("owner", "demo", UNVERSIONED).unwrap();
        installer.install(&requested, &mut |_stage| {}).unwrap();
        assert_eq!(
            *sleeps.lock().unwrap(),
            vec![Duration::from_millis(2000), Duration::from_millis(1000)]
        );
    }

    #[test]
    fn community_install_state_machine_advances_in_order() {
        let id = parse_skill_id("owner", "demo", "1.0.0").unwrap();
        let mut record = begin_community_install(&id);
        record = advance(&record, InstallEvent::Verified { ok: true }).unwrap();
        record = advance(&record, InstallEvent::Fetched).unwrap();
        record = advance(
            &record,
            InstallEvent::Hashed {
                expected: "abc".into(),
                actual: "abc".into(),
            },
        )
        .unwrap();
        record = advance(&record, InstallEvent::Scanned).unwrap();
        record = advance(&record, InstallEvent::Staged).unwrap();
        record = advance(&record, InstallEvent::Committed).unwrap();
        assert_eq!(record.step, InstallStep::Commit);
        assert!(matches!(
            advance(&record, InstallEvent::Committed),
            Err(InstallError::Unexpected { .. })
        ));
    }

    #[test]
    fn install_update_swaps_versions_and_preserves_auto_invoke() {
        let dir = TestDir::new("update");
        let package = package();
        let transport = FakeTransport {
            verify: verify_body("2.0.0", &package),
            package,
        };
        let installer = SkillInstaller::new(dir.0.clone(), transport);
        let store =
            Store::open(&dir.0.join("skills").join("market.sqlite3")).unwrap();
        let mut previous = InstallRecord::community(
            parse_skill_id("owner", "demo", "1.0.0").unwrap(),
        );
        previous.step = InstallStep::Commit;
        previous.auto_invoke = true;
        previous.installed_at = 1000;
        store.upsert_install(&previous).unwrap();
        let requested = parse_skill_id("owner", "demo", UNVERSIONED).unwrap();
        let outcome = installer
            .install_update(&requested, &previous, &mut |_stage| {})
            .unwrap();
        assert_eq!(outcome.record.id.version, "2.0.0");
        assert!(outcome.record.auto_invoke);
        let installed = store.installed().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].id.version, "2.0.0");
        assert!(installed[0].auto_invoke);
    }

    #[test]
    fn github_handoff_extracts_the_skill_subtree_and_checks_content_hash() {
        let dir = TestDir::new("github-handoff");
        let package = package();
        let archive = github_archive();
        let handoff = json!({
            "sourceRef": "public-github",
            "repo": "owner/repo",
            "commit": "abc",
            "path": "skills/demo",
            "contentHash": format!("sha256:{}", sha256_hex(&archive)),
            "archiveUrl": "https://github.com/owner/repo/archive/abc.zip"
        })
        .to_string();
        let transport = QueueTransport::new(
            vec![http_text(200, verify_body("2.0.0", &package), None)],
            vec![
                http_bytes(200, "application/json", handoff.into_bytes(), None),
                http_bytes(200, "application/zip", archive, None),
            ],
        );
        let installer = SkillInstaller::new(dir.0.clone(), transport);
        let requested = parse_skill_id("owner", "demo", UNVERSIONED).unwrap();
        let outcome = installer.install(&requested, &mut |_stage| {}).unwrap();
        assert_eq!(outcome.record.id.version, "2.0.0");
        assert!(outcome.record.content_hash.starts_with("sha256:"));
        assert!(outcome.root.join("SKILL.md").is_file());
    }

    #[test]
    fn verify_requires_publisher_and_slug_from_the_response() {
        let package = package();
        let requested = parse_skill_id("owner", "demo", UNVERSIONED).unwrap();
        let body: Value =
            serde_json::from_str(&verify_body("2.0.0", &package)).unwrap();
        for field in ["publisherHandle", "slug"] {
            let mut missing = body.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(matches!(
                parse_verify_body(&missing.to_string(), &requested),
                Err(InstallError::InvalidVerify(_))
            ));
        }
    }

    #[test]
    fn community_records_start_manual_and_script_disabled() {
        let id = parse_skill_id("owner", "demo", UNVERSIONED).unwrap();
        let record = begin_community_install(&id);
        assert_eq!(record.step, InstallStep::Resolve);
        assert!(!record.auto_invoke);
        assert!(!record.scripts_enabled);
    }

    #[test]
    fn verify_rejects_none_clean_security() {
        let requested = parse_skill_id("owner", "demo", UNVERSIONED).unwrap();
        let body = json!({
            "ok": true,
            "decision": "pass",
            "publisherHandle": "owner",
            "slug": "demo",
            "version": "1.0.0",
            "security": { "status": "warn" },
            "artifact": { "files": [{
                "path": "SKILL.md", "sha256": "0".repeat(64), "size": 1
            }] }
        })
        .to_string();
        assert!(parse_verify_body(&body, &requested).is_err());
    }

    #[test]
    fn update_check_resolves_a_different_latest_version() {
        let package = package();
        let transport = FakeTransport {
            verify: verify_body("2.0.0", &package),
            package,
        };
        let dir = TestDir::new("update-check");
        let installer = SkillInstaller::new(dir.0.clone(), transport);
        let mut record = InstallRecord::community(
            parse_skill_id("owner", "demo", "1.0.0").unwrap(),
        );
        record.step = InstallStep::Commit;
        let check = installer.check_update(&record).unwrap();
        assert!(check.available);
        assert_eq!(check.latest.version, "2.0.0");
    }

    #[test]
    fn update_check_does_not_report_a_downgrade() {
        assert!(version_is_newer("2.0.0", "1.9.9"));
        assert!(!version_is_newer("1.9.9", "2.0.0"));
    }
}
