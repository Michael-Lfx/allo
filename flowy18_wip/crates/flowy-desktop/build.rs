//! Embeds the brand icon into the Windows executable.
//!
//! `brands/<brand>/logo.png` is a plain PNG; Windows shell wants an ICO. The
//! ICO container here only wraps that PNG (Vista+ accepts PNG entries), so no
//! image re-encoding or extra build dependency is needed.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=FLOWY_BRAND");
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
