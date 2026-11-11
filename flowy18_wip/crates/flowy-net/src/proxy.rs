//! System proxy detection for outbound HTTP clients.
//!
//! When the process already has `HTTP(S)_PROXY` / `ALL_PROXY`, reqwest reads
//! those itself and this module leaves the builder alone. Otherwise it probes
//! the OS once (cached for 250ms) and applies the result.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

#[cfg(any(test, target_os = "macos", target_os = "linux"))]
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
};

const SYSTEM_PROXY_CACHE_TTL: Duration = Duration::from_millis(250);
#[cfg(any(target_os = "macos", target_os = "linux"))]
const SYSTEM_PROXY_COMMAND_TIMEOUT: Duration = Duration::from_millis(750);
#[cfg(any(test, target_os = "macos", target_os = "linux"))]
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);

const PROXY_ENV_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
struct SystemProxyConfig {
    http_proxy: Option<String>,
    https_proxy: Option<String>,
    all_proxy: Option<String>,
    no_proxy: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedSystemProxyConfig {
    detected_at: Instant,
    config: Option<SystemProxyConfig>,
}

static SYSTEM_PROXY_CACHE: LazyLock<Mutex<Option<CachedSystemProxyConfig>>> =
    LazyLock::new(|| Mutex::new(None));

/// Apply the OS proxy when the process has not already set proxy env vars.
pub fn apply_proxy(
    mut builder: reqwest::blocking::ClientBuilder,
) -> reqwest::blocking::ClientBuilder {
    if process_has_proxy_env() {
        return builder;
    }
    let Some(config) = system_proxy_config() else {
        return builder;
    };
    let no_proxy = effective_no_proxy(&config);

    if let Some(proxy_url) = config.all_proxy.as_deref()
        && let Ok(proxy) = reqwest::Proxy::all(proxy_url)
    {
        return builder.proxy(proxy.no_proxy(no_proxy));
    }
    if let Some(proxy_url) = config.http_proxy.as_deref()
        && let Ok(proxy) = reqwest::Proxy::http(proxy_url)
    {
        builder = builder.proxy(proxy.no_proxy(no_proxy.clone()));
    }
    if let Some(proxy_url) = config.https_proxy.as_deref()
        && let Ok(proxy) = reqwest::Proxy::https(proxy_url)
    {
        builder = builder.proxy(proxy.no_proxy(no_proxy));
    }
    builder
}

fn process_has_proxy_env() -> bool {
    PROXY_ENV_KEYS.iter().any(|key| {
        std::env::var(key).is_ok_and(|value| !value.trim().is_empty())
    })
}

fn system_proxy_config() -> Option<SystemProxyConfig> {
    let now = Instant::now();
    let cached = SYSTEM_PROXY_CACHE
        .lock()
        .expect("system proxy cache lock")
        .as_ref()
        .filter(|entry| {
            now.duration_since(entry.detected_at) < SYSTEM_PROXY_CACHE_TTL
        })
        .cloned();
    if let Some(entry) = cached {
        return entry.config;
    }
    let config = detect_platform_proxy();
    *SYSTEM_PROXY_CACHE.lock().expect("system proxy cache lock") =
        Some(CachedSystemProxyConfig {
            detected_at: now,
            config: config.clone(),
        });
    config
}

fn effective_no_proxy(config: &SystemProxyConfig) -> Option<reqwest::NoProxy> {
    let mut items = Vec::new();
    if let Ok(value) =
        std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy"))
        && !value.trim().is_empty()
    {
        items.push(value);
    }
    if let Some(value) = &config.no_proxy
        && !value.trim().is_empty()
    {
        items.push(value.clone());
    }
    if items.is_empty() {
        None
    } else {
        reqwest::NoProxy::from_string(&items.join(","))
    }
}

#[cfg(target_os = "macos")]
fn detect_platform_proxy() -> Option<SystemProxyConfig> {
    let stdout = command_stdout_with_timeout(
        Command::new("/usr/sbin/scutil").arg("--proxy"),
        SYSTEM_PROXY_COMMAND_TIMEOUT,
    )
    .or_else(|| {
        command_stdout_with_timeout(
            Command::new("scutil").arg("--proxy"),
            SYSTEM_PROXY_COMMAND_TIMEOUT,
        )
    })?;
    parse_scutil_proxy(&stdout)
}

#[cfg(target_os = "windows")]
fn detect_platform_proxy() -> Option<SystemProxyConfig> {
    let key = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    if key.get_u32("ProxyEnable").unwrap_or(0) == 0 {
        return None;
    }
    let proxy_server = key.get_string("ProxyServer").unwrap_or_default();
    let proxy_override = key.get_string("ProxyOverride").ok();
    parse_windows_proxy_settings(true, &proxy_server, proxy_override.as_deref())
}

#[cfg(target_os = "linux")]
fn detect_platform_proxy() -> Option<SystemProxyConfig> {
    None
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
)))]
fn detect_platform_proxy() -> Option<SystemProxyConfig> {
    None
}

#[cfg(any(test, target_os = "macos", target_os = "linux"))]
fn command_stdout_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Option<String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started_at = Instant::now();
    loop {
        if let Some(status) = child.try_wait().ok()? {
            let mut stdout = Vec::new();
            if let Some(mut pipe) = child.stdout.take() {
                pipe.read_to_end(&mut stdout).ok()?;
            }
            if !status.success() {
                return None;
            }
            return Some(String::from_utf8_lossy(&stdout).into_owned());
        }
        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(COMMAND_POLL_INTERVAL);
    }
}

fn parse_scutil_proxy(text: &str) -> Option<SystemProxyConfig> {
    let mut http_enable = false;
    let mut https_enable = false;
    let mut socks_enable = false;
    let mut http_host = None;
    let mut https_host = None;
    let mut socks_host = None;
    let mut http_port = None;
    let mut https_port = None;
    let mut socks_port = None;
    let mut exceptions = Vec::new();
    let mut in_exceptions = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.starts_with("ExceptionsList") && line.contains("<array>") {
            in_exceptions = true;
            continue;
        }
        if in_exceptions {
            if line.starts_with('}') {
                in_exceptions = false;
                continue;
            }
            if let Some((_, value)) = line.split_once(':') {
                let item = value.trim().trim_matches('"');
                if !item.is_empty() {
                    exceptions.push(item.to_owned());
                }
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "HTTPEnable" => http_enable = value == "1",
            "HTTPSEnable" => https_enable = value == "1",
            "SOCKSEnable" => socks_enable = value == "1",
            "HTTPProxy" => http_host = non_empty(value),
            "HTTPSProxy" => https_host = non_empty(value),
            "SOCKSProxy" => socks_host = non_empty(value),
            "HTTPPort" => http_port = value.parse().ok(),
            "HTTPSPort" => https_port = value.parse().ok(),
            "SOCKSPort" => socks_port = value.parse().ok(),
            _ => {}
        }
    }

    let http_proxy =
        enabled_proxy_url(http_enable, "http", http_host.as_deref(), http_port);
    let https_proxy = enabled_proxy_url(
        https_enable,
        "http",
        https_host.as_deref(),
        https_port,
    );
    let all_proxy = if http_proxy.is_none()
        && https_proxy.is_none()
        && socks_enable
    {
        enabled_proxy_url(true, "socks5h", socks_host.as_deref(), socks_port)
    } else {
        None
    };
    if http_proxy.is_none() && https_proxy.is_none() && all_proxy.is_none() {
        return None;
    }
    Some(SystemProxyConfig {
        http_proxy,
        https_proxy,
        all_proxy,
        no_proxy: if exceptions.is_empty() {
            None
        } else {
            Some(exceptions.join(","))
        },
    })
}

fn enabled_proxy_url(
    enabled: bool,
    scheme: &str,
    host: Option<&str>,
    port: Option<u16>,
) -> Option<String> {
    if !enabled {
        return None;
    }
    let host = host?;
    let port = port?;
    Some(format!("{scheme}://{host}:{port}"))
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(any(test, target_os = "windows"))]
fn parse_windows_proxy_settings(
    enabled: bool,
    proxy_server: &str,
    proxy_override: Option<&str>,
) -> Option<SystemProxyConfig> {
    if !enabled {
        return None;
    }
    let server = proxy_server.trim();
    if server.is_empty() {
        return None;
    }
    let mut http_proxy = None;
    let mut https_proxy = None;
    let mut all_proxy = None;
    if server.contains('=') {
        for part in server.split(';') {
            let Some((kind, endpoint)) = part.split_once('=') else {
                continue;
            };
            let url = normalize_proxy_url("http", endpoint.trim())?;
            match kind.trim().to_ascii_lowercase().as_str() {
                "http" => http_proxy = Some(url),
                "https" => https_proxy = Some(url),
                "socks" => {
                    all_proxy =
                        Some(normalize_proxy_url("socks5h", endpoint.trim())?);
                }
                _ => {}
            }
        }
    } else {
        http_proxy = normalize_proxy_url("http", server);
        https_proxy.clone_from(&http_proxy);
    }
    if http_proxy.is_none() && https_proxy.is_none() && all_proxy.is_none() {
        return None;
    }
    let no_proxy = proxy_override.and_then(|value| {
        let items: Vec<_> = value
            .split(&[';', ' '][..])
            .map(str::trim)
            .filter(|item| {
                !item.is_empty() && !item.eq_ignore_ascii_case("<local>")
            })
            .map(str::to_owned)
            .collect();
        (!items.is_empty()).then(|| items.join(","))
    });
    Some(SystemProxyConfig {
        http_proxy,
        https_proxy,
        all_proxy,
        no_proxy,
    })
}

#[cfg(any(test, target_os = "windows"))]
fn normalize_proxy_url(default_scheme: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.contains("://") {
        return Some(value.to_owned());
    }
    if let Some((host, port)) = value.rsplit_once(':')
        && port.parse::<u16>().is_ok()
    {
        return Some(format!("{default_scheme}://{host}:{port}"));
    }
    let port = if default_scheme.starts_with("socks") {
        1080
    } else {
        80
    };
    Some(format!("{default_scheme}://{value}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scutil_http_proxy_is_parsed() {
        let text = r"
<dictionary> {
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7890
  HTTPSProxy : 127.0.0.1
  ExceptionsList : <array> {
    0 : localhost
  }
}
";
        let config = parse_scutil_proxy(text).unwrap();
        assert_eq!(config.http_proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(
            config.https_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(config.no_proxy.as_deref(), Some("localhost"));
    }

    #[test]
    fn windows_named_proxy_entries_are_parsed() {
        let config = parse_windows_proxy_settings(
            true,
            "http=127.0.0.1:7890;https=127.0.0.1:7890",
            Some("localhost;<local>"),
        )
        .unwrap();
        assert_eq!(config.http_proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(
            config.https_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(config.no_proxy.as_deref(), Some("localhost"));
    }
}
