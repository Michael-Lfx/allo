//! Bind session Cameo photos into the film-root portrait registry.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::domain::CharacterInScene;
use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{copy_image_file_atomic, is_usable_image_file};
use crate::session::cameo::{self, CameoPhotoEntry};

use super::{resolve_film_root, safe_component};

/// Session working root that owns `cameo/` (parent of idea2video/script2video/novel2video).
pub(crate) fn resolve_session_root(working_dir: &Path) -> PathBuf {
    let film = resolve_film_root(working_dir);
    let name = film.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if matches!(name, "idea2video" | "script2video" | "novel2video") {
        film.parent().unwrap_or(film.as_path()).to_path_buf()
    } else {
        film
    }
}

/// Hint appended to character-extraction input so LLM keeps user-provided names.
///
/// Skips anonymous names (camera file stems like `05382109`) so we do not force
/// the extractor to invent cast ids from WeChat/phone filenames.
pub(crate) fn cameo_extractor_hint(session_root: &Path) -> String {
    let Ok(photos) = cameo::list_photos(session_root) else {
        return String::new();
    };
    if photos.is_empty() {
        return String::new();
    }
    let names: Vec<String> = photos
        .iter()
        .map(|p| p.character_name.trim().to_string())
        .filter(|n| !n.is_empty() && !is_anonymous_cameo_name(n))
        .collect();
    let anon_count = photos
        .iter()
        .filter(|p| is_anonymous_cameo_name(p.character_name.trim()))
        .count();
    if names.is_empty() && anon_count == 0 {
        return String::new();
    }
    let mut parts = Vec::new();
    if !names.is_empty() {
        parts.push(format!(
            "You MUST include each as a visible character and keep identifier_in_scene equal (or an obvious \
variant of) these names: {}.",
            names.join(", ")
        ));
    }
    if anon_count > 0 {
        parts.push(format!(
            "The user also uploaded {anon_count} reference photo(s) without a real character name \
(camera/file id only). Keep the story's natural character names; Cameo photos will be bound by \
fallback to the matching cast member — do NOT invent identifiers from numeric filenames."
        ));
    }
    format!(
        "\n\n[CAMEO CAST LOCK]\nThe user uploaded reference photos for cast identity. {}\n",
        parts.join(" ")
    )
}

/// Load film registry, bind session Cameos, persist registry (+ scene mirror).
pub(crate) async fn apply_session_cameos(
    working_dir: &Path,
    characters: &[CharacterInScene],
) -> VimaxResult<()> {
    let film_root = resolve_film_root(working_dir);
    let session_root = resolve_session_root(working_dir);
    let photos = cameo::list_photos(&session_root)?;
    if photos.is_empty() {
        return Ok(());
    }

    let registry_path = film_root.join("character_portraits_registry.json");
    let mut registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
        if registry_path.exists() {
            crate::session::read_json_artifact(&registry_path).await?
        } else {
            HashMap::new()
        };

    let film = film_root.clone();
    let session = session_root.clone();
    let chars = characters.to_vec();
    registry = tokio::task::spawn_blocking(move || {
        let mut reg = registry;
        bind_cameos_to_registry(&session, &film, &chars, &mut reg)?;
        Ok::<_, VimaxError>(reg)
    })
    .await
    .map_err(|e| VimaxError::msg(format!("cameo bind join error: {e}")))??;

    crate::session::write_json_artifact(&registry_path, &registry).await?;
    if film_root != working_dir {
        crate::session::write_json_artifact(
            &working_dir.join("character_portraits_registry.json"),
            &registry,
        )
        .await?;
    }
    Ok(())
}

/// Copy Cameo photos into film-root portrait dirs and register a `cameo` view.
///
/// Returns bindings `(photo_id, identifier_in_scene)`. Errors when a photo cannot
/// be uniquely matched to an extracted character.
pub(crate) fn bind_cameos_to_registry(
    session_root: &Path,
    film_root: &Path,
    characters: &[CharacterInScene],
    registry: &mut HashMap<String, HashMap<String, HashMap<String, String>>>,
) -> VimaxResult<Vec<(String, String)>> {
    let photos = cameo::list_photos(session_root)?;
    if photos.is_empty() {
        return Ok(Vec::new());
    }

    let visible: Vec<&CharacterInScene> = characters.iter().filter(|c| c.is_visible).collect();
    let mut used_chars: HashSet<String> = HashSet::new();
    let mut bindings: Vec<(String, String)> = Vec::new();

    for photo in &photos {
        let matched = match_cameo_to_character(photo, &visible, &used_chars)?;
        used_chars.insert(matched.identifier_in_scene.clone());
        let dest = write_cameo_portrait(film_root, matched, photo)?;
        let desc = cameo_registry_description(matched, photo, &dest);
        let mut item = HashMap::new();
        item.insert("path".into(), dest.to_string_lossy().to_string());
        item.insert("description".into(), desc);
        registry
            .entry(matched.identifier_in_scene.clone())
            .or_default()
            .insert("cameo".into(), item);
        bindings.push((photo.id.clone(), matched.identifier_in_scene.clone()));
    }

    cameo::set_bindings(session_root, &bindings)?;
    Ok(bindings)
}

fn match_cameo_to_character<'a>(
    photo: &CameoPhotoEntry,
    visible: &[&'a CharacterInScene],
    used: &HashSet<String>,
) -> VimaxResult<&'a CharacterInScene> {
    let cameo_name = photo.character_name.trim();
    let anonymous = is_anonymous_cameo_name(cameo_name);

    // 1) Explicit prior binding (re-plan / resume).
    if let Some(bound) = photo
        .bound_identifier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(hit) = visible
            .iter()
            .find(|c| cameo::names_match(bound, &c.identifier_in_scene))
        {
            return Ok(*hit);
        }
    }

    // 2) Exact name match among unused cast.
    if !anonymous {
        let name_hits: Vec<&&CharacterInScene> = visible
            .iter()
            .filter(|c| {
                !used.contains(&c.identifier_in_scene)
                    && cameo::names_match(cameo_name, &c.identifier_in_scene)
            })
            .collect();
        if name_hits.len() == 1 {
            return Ok(name_hits[0]);
        }
        if name_hits.len() > 1 {
            return Err(VimaxError::InvalidParams(format!(
                "cameo character_name {:?} matches multiple cast members — rename the photo or characters",
                photo.character_name
            )));
        }

        // Same identity already bound (second photo for the same role) → overwrite.
        let used_name_hits: Vec<&&CharacterInScene> = visible
            .iter()
            .filter(|c| {
                used.contains(&c.identifier_in_scene)
                    && cameo::names_match(cameo_name, &c.identifier_in_scene)
            })
            .collect();
        if used_name_hits.len() == 1 {
            return Ok(used_name_hits[0]);
        }

        // 3) Unique fuzzy containment (e.g. cameo "小林" ↔ cast "林").
        if let Some(hit) = unique_fuzzy_name_hit(cameo_name, visible, used) {
            return Ok(hit);
        }
    }

    let remaining: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| !used.contains(&c.identifier_in_scene))
        .collect();

    // 4) Single remaining cast member — always safe (covers camera-filename cameos).
    if remaining.len() == 1 {
        return Ok(remaining[0]);
    }

    // 5) Only one visible cast in the whole scene (extra photos overwrite the same cameo).
    if visible.len() == 1 {
        return Ok(visible[0]);
    }

    // 6) Anonymous multi-photo / multi-cast: bind by ascending idx order.
    if anonymous && !remaining.is_empty() {
        let mut ordered = remaining;
        ordered.sort_by_key(|c| c.idx);
        return Ok(ordered[0]);
    }

    Err(VimaxError::InvalidParams(format!(
        "could not uniquely bind cameo {:?} to an extracted character — \
set character_name to match identifier_in_scene (candidates: {})",
        photo.character_name,
        visible
            .iter()
            .map(|c| c.identifier_in_scene.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// True when the "name" is almost certainly a camera / export stem, not a cast label.
pub(crate) fn is_anonymous_cameo_name(name: &str) -> bool {
    let t = name.trim();
    if t.is_empty() {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "character" | "characters" | "role" | "cast" | "角色" | "人物" | "未命名" | "untitled"
    ) {
        return true;
    }
    // UI placeholder `角色1` / `角色 2`
    if lower.starts_with("角色") {
        let rest = t.trim_start_matches("角色").trim();
        if rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    if lower.starts_with("character") {
        let rest = lower.trim_start_matches("character").trim_matches(|c: char| {
            c.is_ascii_whitespace() || c == '_' || c == '-'
        });
        if rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    // Pure digits (WeChat / screenshot stems like `05382109`).
    if t.chars().all(|c| c.is_ascii_digit()) && t.len() >= 4 {
        return true;
    }
    // Hex-ish ids without CJK.
    let alnum: String = t.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if alnum.len() >= 6
        && alnum.chars().all(|c| c.is_ascii_hexdigit())
        && !t.chars().any(is_cjk_char)
    {
        return true;
    }
    // `IMG_1234` / `DSC01234` / `mmexport1712345678901`
    if looks_like_camera_stem(&lower) {
        return true;
    }
    false
}

fn is_cjk_char(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
        || (0x3040..=0x30FF).contains(&u)
        || (0xAC00..=0xD7AF).contains(&u)
}

fn looks_like_camera_stem(lower: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "img_", "img-", "dsc", "dscn", "dsc_", "photo_", "pic_", "mmexport", "wx_camera_",
        "screenshot",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p))
}

fn unique_fuzzy_name_hit<'a>(
    cameo_name: &str,
    visible: &[&'a CharacterInScene],
    used: &HashSet<String>,
) -> Option<&'a CharacterInScene> {
    let key = cameo::normalize_match_key(cameo_name);
    if key.chars().count() < 2 {
        return None;
    }
    let hits: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| {
            if used.contains(&c.identifier_in_scene) {
                return false;
            }
            let other = cameo::normalize_match_key(&c.identifier_in_scene);
            if other.chars().count() < 2 {
                return false;
            }
            other.contains(&key) || key.contains(&other)
        })
        .collect();
    if hits.len() == 1 {
        Some(hits[0])
    } else {
        None
    }
}

fn write_cameo_portrait(
    film_root: &Path,
    character: &CharacterInScene,
    photo: &CameoPhotoEntry,
) -> VimaxResult<PathBuf> {
    let src = session_photo_abs(film_root, photo)?;
    if !is_usable_image_file(&src) {
        return Err(VimaxError::InvalidParams(format!(
            "cameo photo file missing or unusable: {}",
            photo.rel_path
        )));
    }
    let dir = film_root.join("character_portraits").join(format!(
        "{}_{}",
        character.idx,
        safe_component(&character.identifier_in_scene)
    ));
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(format!(
        "{}_cameo.png",
        safe_component(&character.identifier_in_scene)
    ));
    // Cameo uploads are already normalized PNG — copy instead of decode/re-encode
    // (phone photos are often 20–50MP; re-encoding OOM / Windows rename flakes).
    copy_image_file_atomic(&src, &dest)?;
    Ok(dest)
}

fn session_photo_abs(film_root: &Path, photo: &CameoPhotoEntry) -> VimaxResult<PathBuf> {
    let session_root = resolve_session_root(film_root);
    let cleaned = photo.rel_path.replace('\\', "/");
    if cleaned.contains("..") || !cleaned.starts_with("cameo/photos/") {
        return Err(VimaxError::InvalidParams("invalid cameo rel_path".into()));
    }
    Ok(session_root.join(cleaned))
}

/// Usable Cameo portrait paths from the film-root character registry (max `max`).
/// Used as style/scene context refs for vacant environment/prop plates.
pub(crate) fn cameo_style_ref_paths(film_root: &Path, max: usize) -> Vec<PathBuf> {
    if max == 0 {
        return Vec::new();
    }
    let registry_path = film_root.join("character_portraits_registry.json");
    let Ok(raw) = std::fs::read_to_string(&registry_path) else {
        return Vec::new();
    };
    let Ok(registry) =
        serde_json::from_str::<HashMap<String, HashMap<String, HashMap<String, String>>>>(&raw)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for views in registry.values() {
        if out.len() >= max {
            break;
        }
        let Some(item) = views.get("cameo") else {
            continue;
        };
        let Some(path) = item.get("path") else {
            continue;
        };
        let p = crate::session::resolve_stored_asset_path(path, film_root);
        if is_usable_image_file(&p) {
            out.push(p);
        }
    }
    out
}

/// Text hint so world-asset extraction prefers settings implied by user photos.
pub(crate) fn cameo_scene_context_hint(session_root: &Path) -> String {
    let Ok(photos) = cameo::list_photos(session_root) else {
        return String::new();
    };
    if photos.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    for p in &photos {
        let name = p.character_name.trim();
        let desc = p.description.trim();
        let label = if name.is_empty() || is_anonymous_cameo_name(name) {
            "user reference subject".to_string()
        } else {
            name.to_string()
        };
        if desc.is_empty() {
            lines.push(format!(
                "- Character photo for <{label}> (match era/palette/setting type from the photo)."
            ));
        } else {
            lines.push(format!(
                "- Character photo for <{label}>: {desc} (match era/palette/materials/setting type from the photo)."
            ));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "\n\n[CAMEO SCENE LOCK]\nThe user uploaded reference photos. Environments and props MUST match the \
visual world implied by these photos (era, location type, lighting mood, materials, color palette). \
Do NOT invent a conflicting setting. Keep plates empty-set (no people):\n{}\n",
        lines.join("\n")
    )
}

/// Stable fingerprint of session Cameo photos — used to invalidate stale world plates.
pub(crate) fn cameo_style_lock_token(session_root: &Path) -> String {
    let Ok(photos) = cameo::list_photos(session_root) else {
        return String::new();
    };
    if photos.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = photos
        .iter()
        .map(|p| format!("{}:{}", p.id, p.sha256))
        .collect();
    parts.sort();
    parts.join("|")
}

/// Collect Cameo style refs + scene hint + lock token for world-asset generation.
pub(crate) fn world_cameo_context(working_dir: &Path) -> (Vec<PathBuf>, String, String) {
    let film_root = resolve_film_root(working_dir);
    let session_root = resolve_session_root(working_dir);
    (
        cameo_style_ref_paths(&film_root, 2),
        cameo_scene_context_hint(&session_root),
        cameo_style_lock_token(&session_root),
    )
}

fn cameo_registry_description(
    character: &CharacterInScene,
    photo: &CameoPhotoEntry,
    dest: &Path,
) -> String {
    let file_name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("cameo.png");
    let feats = character.static_features.trim();
    let user_desc = photo.description.trim();
    let extra = if user_desc.is_empty() {
        String::new()
    } else {
        format!(" User note: {user_desc}.")
    };
    format!(
        "File [{file_name}] = USER CAMEO identity lock for <{}>. Match face/hair/body EXACTLY. \
Do not redesign identity. Features: {feats}.{extra}",
        character.identifier_in_scene
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::has_usable_cameo;
    use image::{ImageFormat, Rgb, RgbImage};

    fn jpeg_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbImage::from_pixel(12, 10, Rgb([200, 100, 50]))
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    fn char(idx: i32, id: &str) -> CharacterInScene {
        CharacterInScene {
            idx,
            identifier_in_scene: id.into(),
            is_visible: true,
            static_features: "tall".into(),
            dynamic_features: None,
            voice_profile: None,
        }
    }

    #[test]
    fn binds_by_name_and_writes_cameo_view() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "Alice", "me").unwrap();
        let characters = vec![char(0, "Alice"), char(1, "Bob")];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings.len(), 1);
        assert!(has_usable_cameo(&registry, "Alice"));
        assert!(!has_usable_cameo(&registry, "Bob"));
    }

    #[test]
    fn single_character_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("script2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "Me", "").unwrap();
        let characters = vec![char(0, "Hero")];
        let mut registry = HashMap::new();
        bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert!(has_usable_cameo(&registry, "Hero"));
    }

    #[test]
    fn digit_filename_cameo_binds_to_single_cast() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        // UI used to default character_name from WeChat/camera file stems.
        cameo::upload_photo(session, &jpeg_bytes(), "05382109", "").unwrap();
        let characters = vec![char(0, "小")];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].1, "小");
        assert!(has_usable_cameo(&registry, "小"));
        let photos = cameo::list_photos(session).unwrap();
        assert_eq!(photos[0].character_name, "小");
        assert_eq!(photos[0].bound_identifier.as_deref(), Some("小"));
    }

    #[test]
    fn multiple_digit_cameos_overwrite_single_cast() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "05382109", "").unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "05382110", "").unwrap();
        let characters = vec![char(0, "小")];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings.len(), 2);
        assert!(bindings.iter().all(|(_, id)| id == "小"));
        assert!(has_usable_cameo(&registry, "小"));
    }

    #[test]
    fn anonymous_cameos_bind_multi_cast_by_idx_order() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("script2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "IMG_0001", "").unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "IMG_0002", "").unwrap();
        let characters = vec![char(1, "Bob"), char(0, "Alice")];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings.len(), 2);
        // Ascending idx: Alice (0) then Bob (1)
        assert_eq!(bindings[0].1, "Alice");
        assert_eq!(bindings[1].1, "Bob");
    }

    #[test]
    fn detects_anonymous_cameo_names() {
        assert!(is_anonymous_cameo_name("05382109"));
        assert!(is_anonymous_cameo_name("IMG_1234"));
        assert!(is_anonymous_cameo_name("mmexport1712345678901"));
        assert!(is_anonymous_cameo_name("Character"));
        assert!(!is_anonymous_cameo_name("小"));
        assert!(!is_anonymous_cameo_name("Alice"));
    }

    #[test]
    fn ambiguous_name_errors() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("script2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "Alex", "").unwrap();
        let characters = vec![char(0, "Alex"), char(1, "alex")];
        // names_match treats Alex/alex as same — both unused → ambiguous
        let mut registry = HashMap::new();
        // Wait: both match "Alex", so name_hits.len() > 1
        let err = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap_err();
        assert!(err.to_string().contains("multiple") || err.to_string().contains("uniquely"));
    }

    #[test]
    fn unmatched_multi_cast_errors() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("script2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "Unknown", "").unwrap();
        let characters = vec![char(0, "Alice"), char(1, "Bob")];
        let mut registry = HashMap::new();
        assert!(bind_cameos_to_registry(session, &film, &characters, &mut registry).is_err());
    }

    #[test]
    fn world_cameo_context_exposes_refs_and_lock() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "Alice", "rainy street at night").unwrap();
        let characters = vec![char(0, "Alice")];
        let mut registry = HashMap::new();
        bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        let reg_path = film.join("character_portraits_registry.json");
        std::fs::write(reg_path, serde_json::to_string_pretty(&registry).unwrap()).unwrap();

        let (refs, hint, token) = world_cameo_context(&film);
        assert_eq!(refs.len(), 1);
        assert!(is_usable_image_file(&refs[0]));
        assert!(hint.contains("CAMEO SCENE LOCK"));
        assert!(hint.contains("rainy street"));
        assert!(!token.is_empty());
        assert_eq!(cameo_style_lock_token(session), token);
    }
}
