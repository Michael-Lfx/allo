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
        format!("Linux {}", std::env::consts::ARCH)
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
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("cmd")
        .args(["/C", "ver"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(not(target_os = "windows"))]
fn windows_release_hint() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::parse_sw_vers;

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
}
