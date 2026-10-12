//! Voice-bank manifest + remote version pointer.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Default ModelScope pointer. Override with [`VOICE_BANK_VERSION_ENV`].
///
/// Pack lives next to ffmpeg extras on NanoModel/ffmpeg `voices/`.
/// A 404 is fail-soft until the pack is hosted.
pub const DEFAULT_VOICE_BANK_VERSION_URL: &str = "https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/voices/version.json?view=false";

/// Env: full URL to remote `version.json`.
pub const VOICE_BANK_VERSION_ENV: &str = "FLOWY_VOICE_BANK_VERSION_URL";
/// Env: direct zip URL (used when `version.json` omits `zip_url`, or alone).
pub const VOICE_BANK_ZIP_ENV: &str = "FLOWY_VOICE_BANK_ZIP_URL";
/// Env: skip download and read this extracted directory (must contain `manifest.json`).
pub const VOICE_BANK_DIR_ENV: &str = "FLOWY_VOICE_BANK_DIR";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoiceGender {
    Female,
    Male,
    #[default]
    Unknown,
}

impl VoiceGender {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "female" | "f" | "woman" | "girl" | "女" | "女性" => Self::Female,
            "male" | "m" | "man" | "boy" | "男" | "男性" => Self::Male,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoiceAge {
    Child,
    Young,
    Adult,
    Mature,
    Elder,
}

impl VoiceAge {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "child" | "kid" | "儿童" | "小孩" | "童" => Self::Child,
            "young" | "teen" | "youth" | "年轻" | "青年" | "少女" | "少年" => Self::Young,
            "mature" | "middle" | "中年" | "成熟" => Self::Mature,
            "elder" | "old" | "senior" | "老年" | "中老年" => Self::Elder,
            _ => Self::Adult,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceEntry {
    pub id: String,
    pub file: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub gender: VoiceGender,
    #[serde(default = "default_age")]
    pub age: VoiceAge,
    #[serde(default)]
    pub pitch: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub transcript: Option<String>,
}

fn default_age() -> VoiceAge {
    VoiceAge::Adult
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceBankManifest {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub voices: Vec<VoiceEntry>,
}

fn default_schema() -> u32 {
    1
}

impl VoiceBankManifest {
    pub fn load_from_dir(root: &Path) -> Result<Self, String> {
        let path = root.join("manifest.json");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        let manifest: Self = serde_json::from_str(&raw)
            .map_err(|e| format!("parse {}: {e}", path.display()))?;
        if manifest.version.trim().is_empty() {
            return Err("manifest.json missing version".into());
        }
        if manifest.voices.is_empty() {
            return Err("manifest.json has no voices".into());
        }
        Ok(manifest)
    }

    pub fn usable_voices(&self, root: &Path) -> Vec<&VoiceEntry> {
        self.voices
            .iter()
            .filter(|v| {
                let rel = v.file.replace('\\', "/");
                !rel.is_empty()
                    && !Path::new(&rel).is_absolute()
                    && !rel.contains("..")
                    && crate::media_local::is_usable_audio_file(&root.join(rel))
            })
            .collect()
    }
}

/// Remote pointer hosted next to the zip.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteVersionPointer {
    pub version: String,
    #[serde(default)]
    pub zip_url: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

impl RemoteVersionPointer {
    pub fn resolve_zip_url(&self, version_url: &str) -> Option<String> {
        if let Ok(direct) = std::env::var(VOICE_BANK_ZIP_ENV) {
            let t = direct.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
        let filename = self
            .filename
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("voice-bank-{}.zip", self.version));
        // Prefer the zip beside the version.json we actually fetched. Hosted
        // `zip_url` can lag when the pack is copied to a new ModelScope path.
        if !version_url.trim().is_empty() {
            return Some(join_url_sibling(version_url, &filename));
        }
        self.zip_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }
}

pub fn join_url_sibling(version_url: &str, filename: &str) -> String {
    let (base, query) = match version_url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (version_url, None),
    };
    let dir = match base.rsplit_once('/') {
        Some((d, _)) => d,
        None => base,
    };
    let mut out = format!("{dir}/{filename}");
    if let Some(q) = query {
        if !out.contains('?') {
            out.push('?');
            out.push_str(q);
        }
    }
    out
}

pub fn parse_version(raw: &str) -> Vec<u64> {
    raw.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<u64>().ok())
        .collect()
}

/// True when `remote` should replace `local` (strictly newer, or local empty).
pub fn version_newer(remote: &str, local: &str) -> bool {
    if local.trim().is_empty() {
        return true;
    }
    let a = parse_version(remote);
    let b = parse_version(local);
    let n = a.len().max(b.len());
    for i in 0..n {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        match av.cmp(&bv) {
            Ordering::Greater => return true,
            Ordering::Less => return false,
            Ordering::Equal => {}
        }
    }
    false
}

pub fn resolved_version_url() -> Option<String> {
    if let Ok(url) = std::env::var(VOICE_BANK_VERSION_ENV) {
        let t = url.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let default = DEFAULT_VOICE_BANK_VERSION_URL.trim();
    (!default.is_empty()).then(|| default.to_string())
}

pub fn override_bank_dir() -> Option<PathBuf> {
    std::env::var(VOICE_BANK_DIR_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_detect_semver() {
        assert!(version_newer("1.0.1", "1.0.0"));
        assert!(version_newer("2.0.0", "1.9.9"));
        assert!(!version_newer("1.0.0", "1.0.0"));
        assert!(!version_newer("1.0.0", "1.0.1"));
        assert!(version_newer("1.0.0", ""));
    }

    #[test]
    fn sibling_zip_keeps_modelscope_query() {
        let url = join_url_sibling(
            "https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/voices/version.json?view=false",
            "voice-bank-1.1.0.zip",
        );
        assert_eq!(
            url,
            "https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/voices/voice-bank-1.1.0.zip?view=false"
        );
    }

    #[test]
    fn pointer_uses_zip_beside_version_json() {
        let p = RemoteVersionPointer {
            version: "1.1.0".into(),
            zip_url: Some("https://example.com/stale.zip".into()),
            filename: Some("voice-bank-1.1.0.zip".into()),
            sha256: None,
            size_bytes: None,
        };
        assert_eq!(
            p.resolve_zip_url("https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/voices/version.json?view=false").as_deref(),
            Some("https://www.modelscope.cn/datasets/NanoModel/ffmpeg/resolve/master/voices/voice-bank-1.1.0.zip?view=false")
        );
        assert_eq!(
            p.resolve_zip_url("").as_deref(),
            Some("https://example.com/stale.zip")
        );
    }
}
