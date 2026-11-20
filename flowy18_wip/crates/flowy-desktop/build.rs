//! Embeds the brand icon into the Windows executable.
//!
//! `brands/<brand>/logo.png` is a plain PNG; Windows shell wants an ICO. The
//! ICO container here only wraps that PNG (Vista+ accepts PNG entries), so no
//! image re-encoding or extra build dependency is needed.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=FLOWY_BRAND");
    println!("cargo:rerun-if-env-changed=DX_RUSTC");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon();
    }
}

fn embed_windows_icon() {
    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"),
    );
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("flowy-desktop lives under crates/");
    let brand = std::env::var("FLOWY_BRAND").unwrap_or_else(|_| "flowy".into());
    let logo = root.join("brands").join(&brand).join("logo.png");
    println!("cargo:rerun-if-changed={}", logo.display());
    if !logo.is_file() {
        println!(
            "cargo:warning=brand logo {} is missing; the executable keeps the default icon",
            logo.display()
        );
        return;
    }
    let png = std::fs::read(&logo).expect("read brand logo");
    if png.len() < 24 || png.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        println!("cargo:warning=brand logo is not a PNG; skipping the icon");
        return;
    }
    let width = u32::from_be_bytes(png[16..20].try_into().expect("IHDR width"));
    let height =
        u32::from_be_bytes(png[20..24].try_into().expect("IHDR height"));
    if width == 0 || height == 0 || width > 256 || height > 256 {
        println!(
            "cargo:warning=brand logo is {width}x{height}; ICO entries need 1..=256 px per side"
        );
        return;
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let ico = out.join("brand.ico");
    let mut bytes = Vec::with_capacity(png.len() + 22);
    // ICONDIR: reserved, type=icon, image count.
    bytes.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // ICONDIRENTRY; 256 px is encoded as 0.
    bytes.push(u8::try_from(width).unwrap_or(0));
    bytes.push(u8::try_from(height).unwrap_or(0));
    bytes.extend_from_slice(&[0, 0, 1, 0, 32, 0]);
    bytes.extend_from_slice(
        &u32::try_from(png.len())
            .expect("logo fits u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&22u32.to_le_bytes());
    bytes.extend_from_slice(&png);
    std::fs::write(&ico, &bytes).expect("write brand icon");

    // `dx serve` already links `.winres/resource.lib`. That library has its
    // own icon, and `rc.exe` numbers every icon image from 1 inside each
    // compiled file. Linking `brand.res` as well makes `cvtres` fail with
    // `CVT1100: duplicate resource. type:ICON, name:1`. Fold the brand icon
    // into dx's resource library instead of adding a second one.
    if std::env::var_os("DX_RUSTC").is_some() {
        match install_brand_icon_into_dx(&ico) {
            DxIcon::Installed => return,
            DxIcon::Failed => {
                println!(
                    "cargo:warning=could not replace dx's icon with the brand logo; the executable keeps dx's icon"
                );
                if dx_links_its_own_icon() {
                    return;
                }
            }
            DxIcon::Absent => {}
        }
    }

    let Some(rc) = find_resource_compiler() else {
        println!(
            "cargo:warning=rc.exe not found; the executable keeps the default icon"
        );
        return;
    };
    let script = out.join("brand.rc");
    std::fs::write(
        &script,
        format!(
            "1 ICON \"{}\"\n",
            ico.display().to_string().replace('\\', "\\\\")
        ),
    )
    .expect("write resource script");
    let resource = out.join("brand.res");
    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg(&script)
        .status();
    match status {
        Ok(status) if status.success() => {}
        other => {
            println!(
                "cargo:warning=rc.exe failed ({other:?}); the executable keeps the default icon"
            );
            return;
        }
    }
    println!("cargo:rustc-link-arg-bins={}", resource.display());
}

#[derive(Debug, PartialEq, Eq)]
enum DxIcon {
    /// This build is not linking dx's Windows resource library.
    Absent,
    /// `resource.lib` now contains the brand icon and must be the only icon.
    Installed,
    /// dx's icon library is on the link line, but it could not be rebuilt.
    Failed,
}

/// dx compiles `.winres/resource.lib` before Cargo runs. Rebuild that one
/// library with the brand icon so the executable still shows the logo.
fn install_brand_icon_into_dx(ico: &Path) -> DxIcon {
    let scripts = dx_winres_scripts();
    if scripts.is_empty() {
        return DxIcon::Absent;
    }
    let Some(rc) = find_resource_compiler() else {
        return DxIcon::Failed;
    };
    let escaped = ico.display().to_string().replace('\\', "\\\\");
    let mut installed = false;
    for script in &scripts {
        println!("cargo:rerun-if-changed={}", script.display());
        let Ok(source) = std::fs::read_to_string(script) else {
            continue;
        };
        let Some(rewritten) = rewrite_icon_paths(&source, &escaped) else {
            continue;
        };
        let Some(dir) = script.parent() else {
            continue;
        };
        let temp_rc = dir.join("brand-icon.rc");
        if std::fs::write(&temp_rc, rewritten).is_err() {
            continue;
        }
        let lib = dir.join("resource.lib");
        let status = Command::new(&rc)
            .arg("/nologo")
            .arg(format!("/fo{}", lib.display()))
            .arg(&temp_rc)
            .status();
        if status.is_ok_and(|status| status.success()) {
            installed = true;
        }
    }
    if installed {
        DxIcon::Installed
    } else {
        DxIcon::Failed
    }
}

fn dx_links_its_own_icon() -> bool {
    dx_winres_scripts().iter().any(|script| {
        script
            .parent()
            .is_some_and(|dir| dir.join("resource.lib").is_file())
    })
}

fn dx_winres_scripts() -> Vec<PathBuf> {
    let Ok(args_dir) = std::env::var("DX_RUSTC") else {
        return Vec::new();
    };
    // `DX_RUSTC` is `target/dx/.captured-args/<session>`.
    let Some(dx_dir) = Path::new(&args_dir).parent().and_then(Path::parent)
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    collect_winres(dx_dir, 0, &mut found);
    found
}

fn collect_winres(dir: &Path, depth: u8, found: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|name| name == ".captured-args")
        {
            continue;
        }
        if path.file_name().is_some_and(|name| name == ".winres") {
            let script = path.join("resource.rc");
            if script.is_file() {
                found.push(script);
            }
            continue;
        }
        collect_winres(&path, depth + 1, found);
    }
}

/// Replace each `ID ICON "..."` path. Image ids are assigned per resource
/// file, so the id in the script stays whatever dx chose.
fn rewrite_icon_paths(source: &str, escaped_ico: &str) -> Option<String> {
    let mut changed = false;
    let mut rewritten = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let ending = &line[body.len()..];
        if let Some(icon) = rewrite_icon_line(body, escaped_ico) {
            changed = true;
            rewritten.push_str(&icon);
            rewritten.push_str(ending);
        } else {
            rewritten.push_str(line);
        }
    }
    changed.then_some(rewritten)
}

fn rewrite_icon_line(line: &str, escaped_ico: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') {
        return None;
    }
    let icon_at = trimmed.find(" ICON ")?;
    let id = trimmed[..icon_at].trim();
    if id.is_empty() || id.contains(' ') {
        return None;
    }
    let indent = &line[..line.len() - trimmed.len()];
    Some(format!("{indent}{id} ICON \"{escaped_ico}\""))
}

/// Newest `rc.exe` from the installed Windows SDK.
fn find_resource_compiler() -> Option<PathBuf> {
    let sdk_root = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)?
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&sdk_root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort();
    versions
        .into_iter()
        .rev()
        .map(|version| version.join("x64").join("rc.exe"))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::rewrite_icon_paths;

    #[test]
    fn brand_icon_replaces_dx_icon_path_and_keeps_its_id() {
        let source = "\
#pragma code_page(65001)\r\n\
32512 ICON \"C:\\\\dx\\\\icon.ico\"\r\n\
";
        let rewritten = rewrite_icon_paths(source, "C:\\\\brand\\\\logo.ico")
            .expect("icon line");
        assert!(rewritten.contains("32512 ICON \"C:\\\\brand\\\\logo.ico\""));
        assert!(!rewritten.contains("dx\\\\icon.ico"));
        assert!(rewritten.contains("#pragma code_page(65001)"));
    }

    #[test]
    fn version_info_is_not_treated_as_an_icon() {
        let source = "VALUE \"FileDescription\", \"Flowy\"\n";
        assert!(
            rewrite_icon_paths(source, "C:\\\\brand\\\\logo.ico").is_none()
        );
    }
}
