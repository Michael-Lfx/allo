//! Arm64 macOS release builds need a definition of `__isPlatformVersionAtLeast`.
//!
//! `ort-sys` links a prebuilt ONNX Runtime whose CoreML objects
//! (`host_utils.mm.o`, `model.mm.o`) call that Clang `@available` helper.
//! Rust 1.91+ exports it from `std`, but this workspace's release profile sets
//! `lto = "thin"`, and thin LTO discards the symbol because no Rust crate
//! references it (rust-lang/rust#164915). The arm64 link then fails. Intel Mac
//! builds never link `ort`, so they do not hit this.
//!
//! The object below is a normal static archive member. Dev and test links keep
//! `std`'s weak copy and do not pull it in. Release links, where LTO has
//! dropped that copy, pull this one definition. Other targets do nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=macos_platform_version.c");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let links_ort = std::env::var_os("CARGO_FEATURE_SILERO_VAD").is_some();
    if target_os != "macos" || target_arch != "aarch64" || !links_ort {
        return;
    }

    // Match rustc's aarch64-apple-darwin default. Leaving this unset makes
    // clang stamp the object with the runner's host OS and the final link
    // warns that it was built for a newer macOS than 11.0.
    cc::Build::new()
        .file("macos_platform_version.c")
        .flag("-mmacosx-version-min=11.0")
        .compile("macos_platform_version");
}
