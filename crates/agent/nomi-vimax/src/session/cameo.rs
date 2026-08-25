//! Session-scoped user reference images (formerly "Cameo").
//!
//! Layout (under session working dir):
//! ```text
//! references/
//!   manifest.json
//!   photos/{id}.png
//!   by_category/{character|environment|prop|style}/{label}_{id8}.png
//!   reference_classification.json
//! ```
//!
//! Legacy sessions may still have `cameo/`; [`ensure_references_layout`] migrates on load.

use std::path::{Path, PathBuf};

use image::GenericImageView;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{
    copy_image_file_atomic, image_magic_kind, is_usable_image_file, scrub_unusable_image,
    write_image_bytes_atomic,
};

pub const CAMEO_MANIFEST_VERSION: u32 = 1;
/// On-disk folder name shown in technical artifacts (user-facing: 参考图).
pub const CAMEO_DIR: &str = "references";
pub const CAMEO_PHOTOS_DIR: &str = "references/photos";
pub const CAMEO_MANIFEST_REL: &str = "references/manifest.json";
pub const CAMEO_BY_CATEGORY_DIR: &str = "references/by_category";
const LEGACY_CAMEO_DIR: &str = "cameo";
const LEGACY_CAMEO_MANIFEST_REL: &str = "cameo/manifest.json";
pub const CAMEO_MAX_PHOTOS: usize = 8;
/// Film stills / high-res boards before PNG normalize can exceed 10MB.
pub const CAMEO_MAX_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameoManifest {
    #[serde(default = "default_manifest_version")]
    pub version: u32,
    #[serde(default)]
    pub photos: Vec<CameoPhotoEntry>,
}

fn default_manifest_version() -> u32 {
    CAMEO_MANIFEST_VERSION
}

impl Default for CameoManifest {
    fn default() -> Self {
        Self {
            version: CAMEO_MANIFEST_VERSION,
            photos: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameoPhotoEntry {
    pub id: String,
    /// Relative to session working dir, e.g. `references/photos/{id}.png`.
    pub rel_path: String,
    #[serde(default)]
    pub character_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// Bound `CharacterInScene.identifier_in_scene` after plan (optional cache).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_identifier: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CameoUpdate {
    pub character_name: Option<String>,
    pub description: Option<String>,
    pub bound_identifier: Option<Option<String>>,
}

/// Ensure `references/` layout exists; migrate legacy `cameo/` when needed.
pub fn ensure_references_layout(working_dir: &Path) -> VimaxResult<()> {
    let modern = working_dir.join(CAMEO_DIR);
    let legacy = working_dir.join(LEGACY_CAMEO_DIR);
    if modern.exists() {
        return Ok(());
    }
    if !legacy.exists() {
        return Ok(());
    }
    // Prefer rename (same volume); fall back to copy+keep legacy readable.
    match std::fs::rename(&legacy, &modern) {
        Ok(()) => {
            tracing::info!(
                from = %legacy.display(),
                to = %modern.display(),
                "migrated legacy cameo/ → references/"
            );
            rewrite_legacy_rel_paths(working_dir)?;
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "could not rename cameo/ → references/; reading legacy paths"
            );
        }
    }
    Ok(())
}

fn rewrite_legacy_rel_paths(working_dir: &Path) -> VimaxResult<()> {
    let path = working_dir.join(CAMEO_MANIFEST_REL);
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut manifest: CameoManifest = serde_json::from_str(&raw)?;
    let mut dirty = false;
    for photo in &mut manifest.photos {
        let cleaned = photo.rel_path.replace('\\', "/");
        if let Some(rest) = cleaned.strip_prefix("cameo/") {
            photo.rel_path = format!("references/{rest}");
            dirty = true;
        }
    }
    if dirty {
        save_manifest(working_dir, &manifest)?;
    }
    Ok(())
}

/// Load manifest if present; otherwise return empty.
pub fn load_manifest(working_dir: &Path) -> VimaxResult<CameoManifest> {
    let _ = ensure_references_layout(working_dir);
    let path = working_dir.join(CAMEO_MANIFEST_REL);
    let legacy = working_dir.join(LEGACY_CAMEO_MANIFEST_REL);
    let path = if path.exists() {
        path
    } else if legacy.exists() {
        legacy
    } else {
        return Ok(CameoManifest::default());
    };
    let raw = std::fs::read_to_string(&path)?;
    let mut manifest: CameoManifest = serde_json::from_str(&raw)?;
    if manifest.version == 0 {
        manifest.version = CAMEO_MANIFEST_VERSION;
    }
    Ok(manifest)
}

pub fn save_manifest(working_dir: &Path, manifest: &CameoManifest) -> VimaxResult<()> {
    let path = working_dir.join(CAMEO_MANIFEST_REL);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&tmp, raw)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Drop entries whose files are missing / unusable; rewrite manifest when dirty.
pub fn scrub_manifest(working_dir: &Path) -> VimaxResult<CameoManifest> {
    let mut manifest = load_manifest(working_dir)?;
    let before = manifest.photos.len();
    manifest.photos.retain(|entry| {
        if !is_safe_cameo_rel(&entry.rel_path) {
            return false;
        }
        let abs = working_dir.join(&entry.rel_path);
        let _ = scrub_unusable_image(&abs);
        is_usable_image_file(&abs)
    });
    if manifest.photos.len() != before || !working_dir.join(CAMEO_MANIFEST_REL).exists() {
        // Only persist when something changed or directory already uses cameo/.
        if working_dir.join(CAMEO_DIR).exists() || !manifest.photos.is_empty() {
            save_manifest(working_dir, &manifest)?;
        }
    }
    Ok(manifest)
}

pub fn list_photos(working_dir: &Path) -> VimaxResult<Vec<CameoPhotoEntry>> {
    Ok(scrub_manifest(working_dir)?.photos)
}

pub fn get_photo(working_dir: &Path, photo_id: &str) -> VimaxResult<CameoPhotoEntry> {
    let manifest = load_manifest(working_dir)?;
    manifest
        .photos
        .into_iter()
        .find(|p| p.id == photo_id)
        .ok_or_else(|| VimaxError::InvalidParams(format!("cameo photo not found: {photo_id}")))
}

pub fn photo_abs_path(working_dir: &Path, photo_id: &str) -> VimaxResult<PathBuf> {
    let entry = get_photo(working_dir, photo_id)?;
    if !is_safe_cameo_rel(&entry.rel_path) {
        return Err(VimaxError::InvalidParams("cameo path traversal".into()));
    }
    let abs = working_dir.join(&entry.rel_path);
    if !abs.starts_with(working_dir) {
        return Err(VimaxError::InvalidParams("cameo path escapes working dir".into()));
    }
    Ok(abs)
}

/// Normalize upload bytes to PNG, append manifest entry.
pub fn upload_photo(
    working_dir: &Path,
    bytes: &[u8],
    character_name: &str,
    description: &str,
) -> VimaxResult<CameoPhotoEntry> {
    if bytes.len() > CAMEO_MAX_BYTES {
        return Err(VimaxError::InvalidParams(format!(
            "reference image exceeds {CAMEO_MAX_BYTES} bytes"
        )));
    }
    if image_magic_kind(bytes).is_none() {
        return Err(VimaxError::InvalidParams(
            "reference image must be PNG, JPEG, or WEBP".into(),
        ));
    }
    let img = image::load_from_memory(bytes)
        .map_err(|e| VimaxError::InvalidParams(format!("invalid reference image: {e}")))?;
    let (width, height) = img.dimensions();

    let mut name = normalize_character_name(character_name);
    if name.is_empty() {
        // Labels are optional for env/prop/style refs; planning classifies visually.
        let n = load_manifest(working_dir).map(|m| m.photos.len() + 1).unwrap_or(1);
        name = format!("参考图{n}");
    }

    let mut manifest = load_manifest(working_dir)?;
    if manifest.photos.len() >= CAMEO_MAX_PHOTOS {
        return Err(VimaxError::InvalidParams(format!(
            "at most {CAMEO_MAX_PHOTOS} reference images per session"
        )));
    }

    let id = Uuid::new_v4().to_string();
    let rel_path = format!("{CAMEO_PHOTOS_DIR}/{id}.png");
    let abs = working_dir.join(&rel_path);
    write_image_bytes_atomic(bytes, &abs)?;

    let sha = format!("{:x}", Sha256::digest(bytes));
    let now = chrono::Local::now().to_rfc3339();
    let entry = CameoPhotoEntry {
        id,
        rel_path,
        character_name: name,
        description: description.trim().to_string(),
        sha256: sha,
        width,
        height,
        created_at: now.clone(),
        updated_at: now,
        bound_identifier: None,
    };
    manifest.photos.push(entry.clone());
    save_manifest(working_dir, &manifest)?;
    Ok(entry)
}

pub fn update_photo(
    working_dir: &Path,
    photo_id: &str,
    update: CameoUpdate,
) -> VimaxResult<CameoPhotoEntry> {
    let mut manifest = load_manifest(working_dir)?;
    let entry = manifest
        .photos
        .iter_mut()
        .find(|p| p.id == photo_id)
        .ok_or_else(|| VimaxError::InvalidParams(format!("cameo photo not found: {photo_id}")))?;

    if let Some(name) = update.character_name {
        let name = normalize_character_name(&name);
        if name.is_empty() {
            return Err(VimaxError::InvalidParams(
                "reference image label cannot be empty".into(),
            ));
        }
        entry.character_name = name;
    }
    if let Some(desc) = update.description {
        entry.description = desc.trim().to_string();
    }
    if let Some(bound) = update.bound_identifier {
        entry.bound_identifier = bound;
    }
    entry.updated_at = chrono::Local::now().to_rfc3339();
    let out = entry.clone();
    save_manifest(working_dir, &manifest)?;
    Ok(out)
}

pub fn delete_photo(working_dir: &Path, photo_id: &str) -> VimaxResult<()> {
    let mut manifest = load_manifest(working_dir)?;
    let Some(idx) = manifest.photos.iter().position(|p| p.id == photo_id) else {
        return Err(VimaxError::InvalidParams(format!(
            "cameo photo not found: {photo_id}"
        )));
    };
    let entry = manifest.photos.remove(idx);
    save_manifest(working_dir, &manifest)?;
    if is_safe_cameo_rel(&entry.rel_path) {
        let abs = working_dir.join(&entry.rel_path);
        let _ = scrub_unusable_image(&abs);
        if abs.exists() {
            let _ = std::fs::remove_file(&abs);
        }
        let part = crate::media_local::image_part_path(&abs);
        if part.exists() {
            let _ = std::fs::remove_file(&part);
        }
    }
    Ok(())
}

/// Persist bound character identifiers after planning.
///
/// Also renames each photo's `character_name` to the matched script
/// `identifier_in_scene` so UI / re-plan no longer show camera file stems
/// (e.g. `05382109` → `小`).
pub fn set_bindings(
    working_dir: &Path,
    bindings: &[(String, String)],
) -> VimaxResult<CameoManifest> {
    let mut manifest = load_manifest(working_dir)?;
    for (photo_id, identifier) in bindings {
        let id_name = normalize_character_name(identifier);
        if id_name.is_empty() {
            continue;
        }
        if let Some(entry) = manifest.photos.iter_mut().find(|p| p.id == *photo_id) {
            entry.bound_identifier = Some(id_name.clone());
            if entry.character_name.trim() != id_name {
                tracing::info!(
                    photo_id = %photo_id,
                    from = %entry.character_name,
                    to = %id_name,
                    "renamed cameo character_name to script identifier"
                );
                entry.character_name = id_name;
            }
            entry.updated_at = chrono::Local::now().to_rfc3339();
        }
    }
    save_manifest(working_dir, &manifest)?;
    Ok(manifest)
}

pub fn normalize_character_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn names_match(a: &str, b: &str) -> bool {
    let ka = normalize_match_key(a);
    let kb = normalize_match_key(b);
    !ka.is_empty() && ka == kb
}

/// Lowercased key with spaces / `_` / `-` stripped — shared by exact + fuzzy cameo match.
pub fn normalize_match_key(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn is_safe_cameo_rel(rel: &str) -> bool {
    let cleaned = rel.replace('\\', "/");
    !cleaned.contains("..")
        && !cleaned.starts_with('/')
        && (cleaned.starts_with(&format!("{CAMEO_PHOTOS_DIR}/"))
            || cleaned.starts_with("cameo/photos/"))
}

/// Copy classified uploads into `references/by_category/...` for technical artifacts.
pub fn materialize_by_category(
    working_dir: &Path,
    categories: &[(String, String, String)],
) -> VimaxResult<()> {
    // categories: (photo_id, category, suggested_label)
    let _ = ensure_references_layout(working_dir);
    let photos = list_photos(working_dir)?;
    let root = working_dir.join(CAMEO_BY_CATEGORY_DIR);
    if root.exists() {
        let _ = std::fs::remove_dir_all(&root);
    }
    std::fs::create_dir_all(&root)?;
    for (photo_id, category, label) in categories {
        let Some(photo) = photos.iter().find(|p| p.id == *photo_id) else {
            continue;
        };
        let src = working_dir.join(&photo.rel_path);
        if !is_usable_image_file(&src) {
            continue;
        }
        let cat = sanitize_component(category);
        let label = sanitize_component(if label.trim().is_empty() {
            photo.character_name.as_str()
        } else {
            label.as_str()
        });
        let id8: String = photo_id.chars().take(8).collect();
        let dest_dir = root.join(&cat);
        std::fs::create_dir_all(&dest_dir)?;
        let dest = dest_dir.join(format!("{label}_{id8}.png"));
        copy_image_file_atomic(&src, &dest)?;
    }
    Ok(())
}

fn sanitize_component(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || is_cjk(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').chars().take(48).collect::<String>();
    if out.is_empty() {
        "ref".into()
    } else {
        out
    }
}

fn is_cjk(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};

    fn tiny_jpeg() -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbImage::from_pixel(16, 12, Rgb([40, 80, 120]))
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    #[test]
    fn upload_normalizes_jpeg_and_lists() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path();
        let entry = upload_photo(working, &tiny_jpeg(), "  Alice  ", "hero").unwrap();
        assert_eq!(entry.character_name, "Alice");
        assert!(entry.rel_path.ends_with(".png"));
        let abs = working.join(&entry.rel_path);
        assert!(is_usable_image_file(&abs));
        assert_eq!(
            image_magic_kind(&std::fs::read(&abs).unwrap()),
            Some("png")
        );
        let list = list_photos(working).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, entry.id);
    }

    #[test]
    fn rejects_html_and_allows_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path();
        assert!(upload_photo(working, b"<html>x</html>", "Bob", "").is_err());
        let entry = upload_photo(working, &tiny_jpeg(), "   ", "").unwrap();
        assert!(entry.character_name.starts_with("参考图"));
        assert!(entry.rel_path.starts_with("references/photos/"));
    }

    #[test]
    fn rejects_oversized() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = tiny_jpeg();
        bytes.resize(CAMEO_MAX_BYTES + 1, 0);
        assert!(upload_photo(dir.path(), &bytes, "Bob", "").is_err());
    }

    #[test]
    fn delete_and_scrub_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path();
        let entry = upload_photo(working, &tiny_jpeg(), "Carol", "").unwrap();
        delete_photo(working, &entry.id).unwrap();
        assert!(list_photos(working).unwrap().is_empty());

        let orphan = upload_photo(working, &tiny_jpeg(), "Dave", "").unwrap();
        std::fs::remove_file(working.join(&orphan.rel_path)).unwrap();
        let scrubbed = scrub_manifest(working).unwrap();
        assert!(scrubbed.photos.is_empty());
    }

    #[test]
    fn update_and_bind() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path();
        let entry = upload_photo(working, &tiny_jpeg(), "Eve", "old").unwrap();
        let updated = update_photo(
            working,
            &entry.id,
            CameoUpdate {
                character_name: Some("Eve Prime".into()),
                description: Some("new".into()),
                bound_identifier: None,
            },
        )
        .unwrap();
        assert_eq!(updated.character_name, "Eve Prime");
        assert_eq!(updated.description, "new");
        set_bindings(working, &[(entry.id.clone(), "Eve_Prime".into())]).unwrap();
        let got = get_photo(working, &entry.id).unwrap();
        assert_eq!(got.bound_identifier.as_deref(), Some("Eve_Prime"));
        assert_eq!(got.character_name, "Eve_Prime");
        assert!(names_match("Eve Prime", "eve_prime"));
    }

    #[test]
    fn set_bindings_renames_anonymous_cameo_to_script_id() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path();
        let entry = upload_photo(working, &tiny_jpeg(), "05382109", "").unwrap();
        set_bindings(working, &[(entry.id.clone(), "小".into())]).unwrap();
        let got = get_photo(working, &entry.id).unwrap();
        assert_eq!(got.character_name, "小");
        assert_eq!(got.bound_identifier.as_deref(), Some("小"));
    }

    #[test]
    fn caps_photo_count() {
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path();
        let jpeg = tiny_jpeg();
        for i in 0..CAMEO_MAX_PHOTOS {
            upload_photo(working, &jpeg, &format!("C{i}"), "").unwrap();
        }
        assert!(upload_photo(working, &jpeg, "Overflow", "").is_err());
    }
}
