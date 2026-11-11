#[path = "src/brand_manifest.rs"]
mod brand_manifest;

use std::env;
use std::path::PathBuf;

fn main() {
    let brand_id =
        env::var("FLOWY_BRAND").unwrap_or_else(|_| "flowy".to_string());
    if brand_id.chars().any(|ch| {
        !ch.is_ascii_lowercase()
            && !ch.is_ascii_digit()
            && ch != '-'
            && ch != '_'
    }) {
        panic!("FLOWY_BRAND {brand_id} is not a safe directory name");
    }
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("CARGO_MANIFEST_DIR is not set");
        }));
    let root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .unwrap_or_else(|| panic!("flowy-app is not inside the workspace"));
    let dir = root.join("brands").join(&brand_id);
    println!("cargo:rerun-if-env-changed=FLOWY_BRAND");
    println!(
        "cargo:rerun-if-changed={}",
        dir.join("brand.toml").display()
    );
    let brand = brand_manifest::load(&dir)
        .unwrap_or_else(|error| panic!("brand {brand_id}: {error}"));
    println!("cargo:rerun-if-changed={}", brand.logo.display());
    println!("cargo:rerun-if-changed={}", brand.icon.display());
    rustc_env("FLOWY_PRODUCT", &brand.product);
    rustc_env("FLOWY_BUNDLE_ID", &brand.bundle_id);
    rustc_env("FLOWY_QUALIFIER_TLD", &brand.qualifier_tld);
    rustc_env("FLOWY_QUALIFIER_ORG", &brand.qualifier_org);
    rustc_env("FLOWY_QUALIFIER_APP", &brand.qualifier_app);
    rustc_env("FLOWY_URL_SCHEME", &brand.url_scheme);
    rustc_env("FLOWY_LOGO", &brand.logo.display().to_string());
    rustc_env("FLOWY_OTA_GITHUB_REPO", &brand.ota_github_repo);
    rustc_env("FLOWY_OTA_MS_CN_REPO", &brand.ota_modelscope_cn_repo);
    rustc_env("FLOWY_OTA_MS_AI_REPO", &brand.ota_modelscope_ai_repo);
    rustc_env("FLOWY_OTA_PREFIX", &brand.ota_prefix);
    rustc_env("FLOWY_OTA_PUBKEY", &brand.ota_pubkey);
}

fn rustc_env(key: &str, value: &str) {
    assert!(
        !value.chars().any(char::is_control),
        "{key} contains a control character"
    );
    println!("cargo:rustc-env={key}={value}");
}
