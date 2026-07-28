//! Bind session Cameo photos into the film-root portrait registry.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::domain::CharacterInScene;
use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{is_usable_image_file, write_image_bytes_atomic};
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
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return String::new();
    }
    format!(
        "\n\n[CAMEO CAST LOCK]\nThe user uploaded reference photos for these character names. \
You MUST include each as a visible character and keep identifier_in_scene equal (or an obvious \
variant of) these names: {}.\n",
        names.join(", ")
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
    let name_hits: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| {
            !used.contains(&c.identifier_in_scene)
                && cameo::names_match(&photo.character_name, &c.identifier_in_scene)
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

    // Deterministic fallback: single remaining visible character + this photo.
    let remaining: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| !used.contains(&c.identifier_in_scene))
        .collect();
    if remaining.len() == 1 {
        return Ok(remaining[0]);
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
    let bytes = std::fs::read(&src)?;
    write_image_bytes_atomic(&bytes, &dest)?;
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
        let p = PathBuf::from(path);
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
        if name.is_empty() {
            continue;
        }
        if desc.is_empty() {
            lines.push(format!("- Character photo for <{name}> (match era/palette/setting type from the photo)."));
        } else {
            lines.push(format!(
                "- Character photo for <{name}>: {desc} (match era/palette/materials/setting type from the photo)."
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
