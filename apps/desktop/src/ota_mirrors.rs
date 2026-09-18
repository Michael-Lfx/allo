//! Multi-host OTA mirrors: regional check order + artifact URL construction.

use std::time::Duration;

pub const GITHUB_REPO: &str = "Michael-Lfx/allo";
pub const MS_CN_REPO: &str = "flowy2025/flowyaipc";
pub const MS_AI_REPO: &str = "flowy2025/flowy";
pub const MS_PREFIX: &str = "allo";

pub const PROBE_RANGE_END: u64 = 262_143;
pub const PROBE_MIN_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtaHost {
    ModelscopeCn,
    ModelscopeAi,
    Github,
}

impl OtaHost {
    pub fn cdn_host(self) -> &'static str {
        match self {
            Self::ModelscopeCn => "modelscope.cn",
            Self::ModelscopeAi => "modelscope.ai",
            Self::Github => "github.com",
        }
    }

    pub fn from_url(url: &str) -> Option<Self> {
        if url.contains("modelscope.cn") {
            Some(Self::ModelscopeCn)
        } else if url.contains("modelscope.ai") {
            Some(Self::ModelscopeAi)
        } else if url.contains("github.com") {
            Some(Self::Github)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionBucket {
    MainlandChina,
    AsiaRussia,
    Rest,
}

const MAINLAND_CHINA_TZ: &[&str] = &[
    "Asia/Shanghai",
    "Asia/Urumqi",
    "Asia/Chongqing",
    "Asia/Harbin",
    "Asia/Kashgar",
    "PRC",
];

const RUSSIA_TZ: &[&str] = &[
    "Europe/Moscow",
    "Europe/Kaliningrad",
    "Europe/Samara",
    "Europe/Volgograd",
    "Europe/Saratov",
    "Europe/Ulyanovsk",
    "Europe/Astrakhan",
    "Europe/Kirov",
    "Europe/Simferopol",
    "Asia/Yekaterinburg",
    "Asia/Omsk",
    "Asia/Novosibirsk",
    "Asia/Barnaul",
    "Asia/Tomsk",
    "Asia/Novokuznetsk",
    "Asia/Krasnoyarsk",
    "Asia/Irkutsk",
    "Asia/Chita",
    "Asia/Yakutsk",
    "Asia/Vladivostok",
    "Asia/Magadan",
    "Asia/Sakhalin",
    "Asia/Kamchatka",
    "Asia/Anadyr",
    "Asia/Ust-Nera",
];

/// CN → AI → GitHub for mainland China; AI → GitHub → CN for Asia+Russia; GitHub → AI → CN otherwise.
pub fn region_bucket(tz: Option<&str>, locale: Option<&str>) -> RegionBucket {
    if let Some(tz) = tz {
        if MAINLAND_CHINA_TZ.iter().any(|item| eq_ignore_ascii(tz, item)) {
            return RegionBucket::MainlandChina;
        }
        if RUSSIA_TZ.iter().any(|item| eq_ignore_ascii(tz, item)) {
            return RegionBucket::AsiaRussia;
        }
        if tz.starts_with("Asia/") || tz.starts_with("asia/") {
            return RegionBucket::AsiaRussia;
        }
    }
    let locale = locale.unwrap_or("").replace('_', "-");
    let locale_lc = locale.to_ascii_lowercase();
    if tz.is_none() && locale_lc == "zh-cn" {
        return RegionBucket::MainlandChina;
    }
    if tz.is_none() && (locale_lc.starts_with("ru") || is_asia_locale(&locale_lc)) {
        return RegionBucket::AsiaRussia;
    }
    RegionBucket::Rest
}

fn is_asia_locale(locale: &str) -> bool {
    matches!(
        locale.get(..2).unwrap_or(""),
        "ja" | "ko" | "th" | "vi" | "id" | "ms" | "hi" | "bn" | "ta" | "te" | "ur" | "fa" | "tr"
            | "kk" | "uz" | "ky" | "mn" | "ne" | "si" | "my" | "km" | "lo" | "fil" | "tl"
    ) || locale.starts_with("zh-")
}

fn eq_ignore_ascii(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

pub fn host_channel() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub fn modelscope_file_url(api_host: &str, repo: &str, path_in_repo: &str) -> String {
    format!(
        "https://{api_host}/api/v1/models/{repo}/repo?Revision=master&FilePath={path_in_repo}"
    )
}

pub fn modelscope_manifest_url(api_host: &str, repo: &str, channel: &str) -> String {
    modelscope_file_url(
        api_host,
        repo,
        &format!("{MS_PREFIX}/channels/{channel}/latest.json"),
    )
}

pub fn github_manifest_url(channel: &str) -> String {
    format!("https://github.com/{GITHUB_REPO}/releases/latest/download/latest-{channel}.json")
}

pub fn github_artifact_url(version: &str, filename: &str) -> String {
    let tag = version_tag(version);
    format!("https://github.com/{GITHUB_REPO}/releases/download/{tag}/{filename}")
}

pub fn version_tag(version: &str) -> String {
    let trimmed = version.trim().trim_start_matches('v');
    format!("v{trimmed}")
}

pub fn check_endpoint_order(bucket: RegionBucket) -> [OtaHost; 3] {
    match bucket {
        RegionBucket::MainlandChina => [OtaHost::ModelscopeCn, OtaHost::ModelscopeAi, OtaHost::Github],
        RegionBucket::AsiaRussia => [OtaHost::ModelscopeAi, OtaHost::Github, OtaHost::ModelscopeCn],
        RegionBucket::Rest => [OtaHost::Github, OtaHost::ModelscopeAi, OtaHost::ModelscopeCn],
    }
}

pub fn check_endpoints(bucket: RegionBucket, channel: &str) -> Vec<String> {
    check_endpoint_order(bucket)
        .into_iter()
        .map(|host| match host {
            OtaHost::ModelscopeCn => modelscope_manifest_url("modelscope.cn", MS_CN_REPO, channel),
            OtaHost::ModelscopeAi => modelscope_manifest_url("modelscope.ai", MS_AI_REPO, channel),
            OtaHost::Github => github_manifest_url(channel),
        })
        .collect()
}

pub fn artifact_filename(download_url: &str) -> Option<String> {
    let url = download_url.trim();
    if let Some(idx) = url.find("FilePath=") {
        let path = &url[idx + "FilePath=".len()..];
        let path = path.split('&').next().unwrap_or(path);
        let decoded = percent_decode(path);
        return decoded.rsplit('/').next().filter(|s| !s.is_empty()).map(str::to_owned);
    }
    url.split('?')
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn channel_folder_from_filename(filename: &str) -> Option<&'static str> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with("-setup.exe") || lower.ends_with(".exe") || lower.ends_with(".msi") {
        return Some("windows");
    }
    if lower.ends_with(".app.tar.gz") {
        return Some("macos");
    }
    if lower.ends_with(".appimage") || lower.ends_with(".deb") || lower.ends_with(".rpm") {
        return Some("linux");
    }
    None
}

pub fn channel_folder_from_url(download_url: &str) -> Option<&'static str> {
    if download_url.contains("/windows/") {
        return Some("windows");
    }
    if download_url.contains("/macos/") {
        return Some("macos");
    }
    if download_url.contains("/linux/") {
        return Some("linux");
    }
    artifact_filename(download_url).and_then(|name| channel_folder_from_filename(&name))
}

pub fn artifact_url(host: OtaHost, version: &str, folder: &str, filename: &str) -> String {
    let tag = version_tag(version);
    match host {
        OtaHost::ModelscopeCn => modelscope_file_url(
            "modelscope.cn",
            MS_CN_REPO,
            &format!("{MS_PREFIX}/{folder}/{tag}/{filename}"),
        ),
        OtaHost::ModelscopeAi => modelscope_file_url(
            "modelscope.ai",
            MS_AI_REPO,
            &format!("{MS_PREFIX}/{folder}/{tag}/{filename}"),
        ),
        OtaHost::Github => github_artifact_url(version, filename),
    }
}

pub fn candidate_artifact_urls(
    version: &str,
    folder: &str,
    filename: &str,
) -> Vec<(OtaHost, String)> {
    [
        OtaHost::ModelscopeCn,
        OtaHost::ModelscopeAi,
        OtaHost::Github,
    ]
    .into_iter()
    .map(|host| (host, artifact_url(host, version, folder, filename)))
    .collect()
}

pub fn probe_bps(bytes: u64, elapsed: Duration) -> Option<u64> {
    if bytes < PROBE_MIN_BYTES {
        return None;
    }
    let ms = elapsed.as_millis();
    if ms == 0 {
        return Some(bytes.saturating_mul(1_000));
    }
    Some((bytes.saturating_mul(1_000)) / (ms as u64))
}

pub fn pick_fastest(samples: &[(OtaHost, u64)]) -> Option<OtaHost> {
    samples
        .iter()
        .max_by_key(|(_, bps)| *bps)
        .map(|(host, _)| *host)
}

fn percent_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(value as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainland_china_prefers_cn() {
        assert_eq!(
            region_bucket(Some("Asia/Shanghai"), Some("en-US")),
            RegionBucket::MainlandChina
        );
        assert_eq!(
            check_endpoint_order(RegionBucket::MainlandChina)[0],
            OtaHost::ModelscopeCn
        );
        assert_eq!(
            region_bucket(None, Some("zh-CN")),
            RegionBucket::MainlandChina
        );
    }

    #[test]
    fn hong_kong_uses_asia_order() {
        assert_eq!(
            region_bucket(Some("Asia/Hong_Kong"), Some("zh-HK")),
            RegionBucket::AsiaRussia
        );
        assert_eq!(
            check_endpoint_order(RegionBucket::AsiaRussia),
            [OtaHost::ModelscopeAi, OtaHost::Github, OtaHost::ModelscopeCn]
        );
    }

    #[test]
    fn russia_and_rest() {
        assert_eq!(
            region_bucket(Some("Europe/Moscow"), None),
            RegionBucket::AsiaRussia
        );
        assert_eq!(
            region_bucket(Some("America/New_York"), Some("zh-CN")),
            RegionBucket::Rest
        );
        assert_eq!(
            check_endpoint_order(RegionBucket::Rest)[0],
            OtaHost::Github
        );
    }

    #[test]
    fn artifact_urls_share_filename() {
        let urls = candidate_artifact_urls("1.4.1", "windows", "Flowy_1.4.1_x64-setup.exe");
        assert!(urls[0].1.contains("modelscope.cn"));
        assert!(urls[0].1.contains("flowyaipc"));
        assert!(urls[1].1.contains("modelscope.ai"));
        assert!(urls[1].1.contains("flowy2025/flowy"));
        assert_eq!(
            urls[2].1,
            "https://github.com/Michael-Lfx/allo/releases/download/v1.4.1/Flowy_1.4.1_x64-setup.exe"
        );
        assert_eq!(
            artifact_filename(&urls[0].1).as_deref(),
            Some("Flowy_1.4.1_x64-setup.exe")
        );
    }

    #[test]
    fn pick_fastest_uses_bps() {
        assert_eq!(
            pick_fastest(&[
                (OtaHost::ModelscopeCn, 100),
                (OtaHost::Github, 900),
                (OtaHost::ModelscopeAi, 400),
            ]),
            Some(OtaHost::Github)
        );
        assert!(probe_bps(8_000, Duration::from_millis(10)).is_none());
        assert_eq!(probe_bps(32_768, Duration::from_millis(10)), Some(3_276_800));
    }
}
