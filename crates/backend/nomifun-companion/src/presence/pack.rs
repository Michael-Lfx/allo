//! CompanionPack load/validate. Explicit `runtime.json` only — no path-guessed clips.

use std::fs;
use std::path::Path;

use nomifun_api_types::{builtin_css_pack, CompanionPackManifest, CompanionPackRenderer, CompanionPackRuntime};

const ALLOWED_RUNTIME_FILES: &[&str] = &["runtime.json", "pack.manifest.json"];

pub fn builtin_pack_for_character(character: &str) -> CompanionPackRuntime {
    match character {
        "ink" => builtin_css_pack("ink", "Ink"),
        "bolt" => builtin_css_pack("bolt", "Bolt"),
        "custom" => builtin_css_pack("custom", "Custom"),
        _ => builtin_css_pack("mochi", "Mochi"),
    }
}

pub fn load_pack_dir(dir: &Path) -> Result<CompanionPackRuntime, String> {
    let runtime_path = dir.join("runtime.json");
    let raw = fs::read_to_string(&runtime_path)
        .map_err(|error| format!("read {}: {error}", runtime_path.display()))?;
    let pack: CompanionPackRuntime = serde_json::from_str(&raw)
        .map_err(|error| format!("parse runtime.json: {error}"))?;
    pack.validate()?;
    if let Some(manifest_raw) = fs::read_to_string(dir.join("pack.manifest.json")).ok() {
        let manifest: CompanionPackManifest = serde_json::from_str(&manifest_raw)
            .map_err(|error| format!("parse pack.manifest.json: {error}"))?;
        if manifest.pack_id != pack.id {
            return Err(format!(
                "pack.manifest.json pack_id '{}' != runtime.json id '{}'",
                manifest.pack_id, pack.id
            ));
        }
    }
    if pack.renderer == CompanionPackRenderer::Atlas {
        let Some(atlas) = &pack.atlas else {
            return Err("atlas pack missing atlas".into());
        };
        let atlas_path = dir.join(&atlas.path);
        if !atlas_path.is_file() {
            return Err(format!("missing atlas file {}", atlas.path));
        }
    }
    Ok(pack)
}

/// Strict directory contract used by `validate:companion-pack`.
pub fn validate_pack_dir(dir: &Path) -> Result<CompanionPackRuntime, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let pack = load_pack_dir(dir)?;
    let mut extra = Vec::new();
    let entries = fs::read_dir(dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        if ALLOWED_RUNTIME_FILES.contains(&name.as_ref()) {
            continue;
        }
        if pack.renderer == CompanionPackRenderer::Atlas
            && pack.atlas.as_ref().is_some_and(|atlas| atlas.path == name.as_ref())
        {
            continue;
        }
        extra.push(name.into_owned());
    }
    extra.sort();
    if !extra.is_empty() {
        return Err(format!(
            "pack directory has files outside the runtime contract: {}",
            extra.join(", ")
        ));
    }
    Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_pack_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("nomifun-pack-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn builtin_character_packs_validate() {
        for character in ["mochi", "ink", "bolt", "custom"] {
            builtin_pack_for_character(character).validate().unwrap();
        }
    }

    #[test]
    fn validate_pack_dir_rejects_extra_files() {
        let dir = temp_pack_dir();
        let pack = builtin_pack_for_character("mochi");
        fs::write(dir.join("runtime.json"), serde_json::to_vec(&pack).unwrap()).unwrap();
        fs::write(dir.join("notes.txt"), "nope").unwrap();
        let err = validate_pack_dir(&dir).unwrap_err();
        assert!(err.contains("notes.txt"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_pack_dir_accepts_css_runtime_only() {
        let dir = temp_pack_dir();
        let pack = builtin_pack_for_character("ink");
        fs::write(dir.join("runtime.json"), serde_json::to_vec(&pack).unwrap()).unwrap();
        validate_pack_dir(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
