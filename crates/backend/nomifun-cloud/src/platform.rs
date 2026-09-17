//! Client platform string for Flowy API payloads.

pub fn client_platform() -> String {
    if cfg!(target_os = "windows") {
        "windows".to_string()
    } else if cfg!(target_os = "macos") {
        "mac".to_string()
    } else if cfg!(target_os = "linux") {
        "linux".to_string()
    } else {
        "unknown".to_string()
    }
}

pub fn os_version_string() -> String {
    if cfg!(target_os = "windows") {
        format!("Windows_NT {}", windows_release_hint())
    } else if cfg!(target_os = "macos") {
        macos_release_hint()
    } else if cfg!(target_os = "linux") {
        format!("Linux {}", linux_release_hint())
    } else {
        std::env::consts::OS.to_string()
    }
}

#[cfg(target_os = "macos")]
fn macos_release_hint() -> String {
    use std::process::Command;

    Command::new("/usr/bin/sw_vers")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| parse_sw_vers(&output))
        .unwrap_or_else(|| format!("macOS unknown (Darwin {})", std::env::consts::ARCH))
}

#[cfg(not(target_os = "macos"))]
fn macos_release_hint() -> String {
    String::new()
}

#[cfg(any(test, target_os = "macos"))]
fn parse_sw_vers(output: &str) -> Option<String> {
    let field = |name: &str| {
        output.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name)
                .then(|| value.trim())
                .filter(|value| !value.is_empty())
        })
    };
    let version = field("ProductVersion")?;
    let build = field("BuildVersion");
    Some(match build {
        Some(build) => format!("macOS {version} ({build})"),
        None => format!("macOS {version}"),
    })
}

#[cfg(target_os = "windows")]
fn windows_release_hint() -> String {
    read_windows_nt_version_from_registry()
        .or_else(read_windows_nt_version_from_ver)
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(not(target_os = "windows"))]
fn windows_release_hint() -> String {
    String::new()
}

/// Reads `major.minor.build` from the NT CurrentVersion registry key.
/// Avoids `cmd /C ver`, which often emits OEM/GBK bytes that fail strict UTF-8 decode.
#[cfg(target_os = "windows")]
fn read_windows_nt_version_from_registry() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()?;
    let build: String = key
        .get_value("CurrentBuildNumber")
        .or_else(|_| key.get_value("CurrentBuild"))
        .ok()?;
    let build = build.trim();
    if build.is_empty() {
        return None;
    }

    let major: Result<u32, _> = key.get_value("CurrentMajorVersionNumber");
    let minor: Result<u32, _> = key.get_value("CurrentMinorVersionNumber");
    if let (Ok(major), Ok(minor)) = (major, minor) {
        return Some(format!("{major}.{minor}.{build}"));
    }

    let current: String = key.get_value("CurrentVersion").ok()?;
    let current = current.trim();
    if current.is_empty() {
        None
    } else {
        Some(format!("{current}.{build}"))
    }
}

#[cfg(target_os = "windows")]
fn read_windows_nt_version_from_ver() -> Option<String> {
    let output = nomi_process_runtime::hidden_std_command("cmd")
        .args(["/C", "ver"])
        .output()
        .ok()?;
    // OEM code pages (e.g. GBK on zh-CN) are not UTF-8; lossy decode keeps ASCII version digits.
    parse_windows_ver_output(&String::from_utf8_lossy(&output.stdout))
}

/// Extracts `major.minor.build` from `ver` text such as
/// `Microsoft Windows [Version 10.0.26200.9445]` or localized
/// `Microsoft Windows [版本 10.0.26200.9445]`.
#[cfg(any(test, target_os = "windows"))]
fn parse_windows_ver_output(output: &str) -> Option<String> {
    for token in output.split(|c: char| !c.is_ascii_digit() && c != '.') {
        if token.is_empty() {
            continue;
        }
        let segments: Vec<&str> = token.split('.').filter(|s| !s.is_empty()).collect();
        if segments.len() >= 3 && segments.iter().all(|s| s.chars().all(|c| c.is_ascii_digit())) {
            return Some(format!("{}.{}.{}", segments[0], segments[1], segments[2]));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_release_hint() -> String {
    read_file_trim("/etc/os-release")
        .and_then(|content| parse_os_release(&content))
        .unwrap_or_else(|| std::env::consts::ARCH.to_string())
}

#[cfg(not(target_os = "linux"))]
fn linux_release_hint() -> String {
    String::new()
}

#[cfg(target_os = "linux")]
fn read_file_trim(path: &str) -> Option<String> {
    let value = std::fs::read_to_string(path).ok()?.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Parses `/etc/os-release` into `PRETTY_NAME (arch)` or `NAME VERSION_ID (arch)`.
#[cfg(any(test, target_os = "linux"))]
fn parse_os_release(content: &str) -> Option<String> {
    let mut pretty = None;
    let mut name = None;
    let mut version_id = None;
    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "PRETTY_NAME" => pretty = Some(value.to_string()),
            "NAME" => name = Some(value.to_string()),
            "VERSION_ID" => version_id = Some(value.to_string()),
            _ => {}
        }
    }

    let arch = std::env::consts::ARCH;
    if let Some(pretty) = pretty {
        return Some(format!("{pretty} ({arch})"));
    }
    match (name, version_id) {
        (Some(name), Some(version_id)) => Some(format!("{name} {version_id} ({arch})")),
        (Some(name), None) => Some(format!("{name} ({arch})")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_os_release, parse_sw_vers, parse_windows_ver_output};

    #[test]
    fn parses_macos_product_and_build_versions() {
        let output = "ProductName:\tmacOS\nProductVersion:\t12.4\nBuildVersion:\t21F79\n";
        assert_eq!(
            parse_sw_vers(output).as_deref(),
            Some("macOS 12.4 (21F79)")
        );
    }

    #[test]
    fn requires_product_version_but_not_build_version() {
        assert_eq!(
            parse_sw_vers("ProductVersion: 14.6.1\n").as_deref(),
            Some("macOS 14.6.1")
        );
        assert_eq!(parse_sw_vers("BuildVersion: 23G93\n"), None);
    }

    #[test]
    fn parses_windows_ver_english_and_localized() {
        assert_eq!(
            parse_windows_ver_output("\r\nMicrosoft Windows [Version 10.0.26200.9445]\r\n")
                .as_deref(),
            Some("10.0.26200")
        );
        assert_eq!(
            parse_windows_ver_output("Microsoft Windows [版本 10.0.19045.3803]").as_deref(),
            Some("10.0.19045")
        );
        assert_eq!(parse_windows_ver_output("no version here"), None);
    }

    #[test]
    fn parses_linux_os_release_pretty_name() {
        let content = r#"NAME="Ubuntu"
VERSION_ID="24.04"
PRETTY_NAME="Ubuntu 24.04.1 LTS"
"#;
        let parsed = parse_os_release(content).expect("os-release");
        assert!(parsed.starts_with("Ubuntu 24.04.1 LTS ("));
        assert!(parsed.ends_with(')'));
    }

    #[test]
    fn parses_linux_os_release_name_and_version_fallback() {
        let content = "NAME=\"Debian GNU/Linux\"\nVERSION_ID=\"12\"\n";
        let parsed = parse_os_release(content).expect("os-release");
        assert!(parsed.starts_with("Debian GNU/Linux 12 ("));
    }
}
