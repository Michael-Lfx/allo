//! Bind session Cameo photos into the film-root portrait registry.
//!
//! After copy, Cameo plates are privacy-anonymized via the image model: keep
//! photoreal wardrobe / pose / lighting, replace faces with a generic
//! unrecognizable virtual face so Seedance does not reject real-person refs.
//!
//! Separately, people-free **atmosphere** plates are generated for world-asset
//! (env/prop) img2img style locking — never feed portrait Cameo into vacant
//! plates, or Seedream may bake faces into “props” like group photos.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backends::VimaxImage;
use crate::domain::CharacterInScene;
use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{copy_image_file_atomic, is_usable_image_file};
use crate::session::cameo::{self, CameoPhotoEntry};

use super::{resolve_film_root, safe_component};

/// Img2img edit: keep photoreal plate, swap face to a generic unrecognizable one.
pub(crate) const CAMEO_FACE_PRIVACY_PROMPT: &str = "\
Keep the original photorealistic style, human pose, clothing, scene, lighting and composition unchanged. \
Only replace the face with an unrecognizable generic virtual face whose features look natural but do not \
correspond to any real person. Weaken real portrait identity cues; do not preserve identifiable facial identity. \
Keep skin texture, light direction, perspective, and overall photorealism consistent. \
Do not change the image style, body shape, hair silhouette, wardrobe, props, or background. \
Single subject. Photorealistic photography. No text, watermark, logo, or extra people.";

/// Img2img: strip every person from a Cameo photo; keep only vacant scene/atmosphere.
pub(crate) const CAMEO_ATMOSPHERE_PROMPT: &str = "\
Using this photo only as a scene and style reference, generate a completely people-free atmosphere plate. \
Erase every person, face, body, hand, silhouette, crowd, mannequin, and human figure — leave no human trace. \
Keep only the empty environment: architecture, furniture, set dressing, materials, color palette, lighting mood, \
weather, and era. If the source is a close-up portrait, invent a matching vacant interior/exterior of the same era \
and palette instead of keeping a face crop. Photorealistic vacant location plate. \
No group photos, no portraits, no framed photos of people, no reflections of people, no posters of faces. \
No text, watermark, or logo.";

/// Session working root that owns `cameo/` (parent of idea2video/script2video/novel2video).
pub(crate) fn resolve_session_root(working_dir: &Path) -> PathBuf {
    let film = resolve_film_root(working_dir);
    let name = film.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if matches!(name, "idea2video" | "script2video" | "novel2video" | "action2video") {
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

/// Load film registry, bind session Cameos, privacy-anonymize faces, persist registry.
pub(crate) async fn apply_session_cameos(
    working_dir: &Path,
    characters: &[CharacterInScene],
    image: Arc<dyn VimaxImage>,
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

    anonymize_bound_cameo_faces(&film_root, &mut registry, Arc::clone(&image)).await?;
    // Vacant atmosphere plates for env/prop style lock (never pass portrait Cameo).
    ensure_cameo_atmosphere_plates(&film_root, &registry, image).await?;

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

/// Run img2img face anonymization for every bound Cameo plate.
///
/// Keeps `{id}_cameo_raw.png` as the user upload; writes privacy-safe
/// `{id}_cameo.png` used by Seedance cast identity refs (not world-asset style).
async fn anonymize_bound_cameo_faces(
    film_root: &Path,
    registry: &mut HashMap<String, HashMap<String, HashMap<String, String>>>,
    image: Arc<dyn VimaxImage>,
) -> VimaxResult<()> {
    let mut updates: Vec<(String, String)> = Vec::new();
    for (identifier, views) in registry.iter() {
        let Some(item) = views.get("cameo") else {
            continue;
        };
        let Some(path_raw) = item.get("path") else {
            continue;
        };
        let plate = crate::session::resolve_stored_asset_path(path_raw, film_root);
        if !is_usable_image_file(&plate) {
            continue;
        }
        let raw_path = cameo_raw_path(&plate);
        let marker = cameo_privacy_marker(&plate);
        ensure_cameo_raw_plate(&plate, &raw_path)?;

        let raw_fp = file_fingerprint(&raw_path).unwrap_or_default();
        if is_usable_image_file(&plate)
            && marker.exists()
            && std::fs::read_to_string(&marker)
                .map(|s| s.trim() == raw_fp)
                .unwrap_or(false)
        {
            continue;
        }

        tracing::info!(
            character = %identifier,
            raw = %raw_path.display(),
            out = %plate.display(),
            "anonymizing cameo face for Seedance privacy"
        );
        let tmp = plate.with_extension("privacy_tmp.png");
        if let Err(err) = image
            .generate(CAMEO_FACE_PRIVACY_PROMPT, &[raw_path.as_path()], &tmp)
            .await
        {
            // Keep the raw plate usable so planning can continue; Seedance may still
            // reject it, but we surface a clear privacy-anonymize failure.
            let _ = std::fs::remove_file(&tmp);
            return Err(VimaxError::msg(format!(
                "cameo face privacy anonymize failed for <{identifier}>: {err}. \
Resume after checking the image model, or use a more illustrated style."
            )));
        }
        if !is_usable_image_file(&tmp) {
            let _ = std::fs::remove_file(&tmp);
            return Err(VimaxError::msg(format!(
                "cameo face privacy anonymize produced no image for <{identifier}>"
            )));
        }
        copy_image_file_atomic(&tmp, &plate)?;
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::write(&marker, raw_fp.as_bytes());
        updates.push((identifier.clone(), privacy_safe_registry_description(identifier, item)));
    }

    for (identifier, desc) in updates {
        if let Some(item) = registry
            .get_mut(&identifier)
            .and_then(|views| views.get_mut("cameo"))
        {
            item.insert("description".into(), desc);
        }
    }
    Ok(())
}

/// Build people-free atmosphere plates used as style refs for env/prop generation.
///
/// Never fails the Cameo bind pipeline: on image-model errors we log and continue so
/// planning can still proceed with text-only `[CAMEO SCENE LOCK]` hints.
async fn ensure_cameo_atmosphere_plates(
    film_root: &Path,
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    image: Arc<dyn VimaxImage>,
) -> VimaxResult<()> {
    for (identifier, views) in registry.iter() {
        let Some(item) = views.get("cameo") else {
            continue;
        };
        let Some(path_raw) = item.get("path") else {
            continue;
        };
        let plate = crate::session::resolve_stored_asset_path(path_raw, film_root);
        if !is_usable_image_file(&plate) {
            continue;
        }
        let raw_path = cameo_raw_path(&plate);
        let _ = ensure_cameo_raw_plate(&plate, &raw_path);
        let source = if is_usable_image_file(&raw_path) {
            raw_path.clone()
        } else {
            plate.clone()
        };
        let atmos = cameo_atmosphere_path(&plate);
        let marker = cameo_atmosphere_marker(&atmos);
        let src_fp = file_fingerprint(&source).unwrap_or_default();
        if is_usable_image_file(&atmos)
            && marker.exists()
            && std::fs::read_to_string(&marker)
                .map(|s| s.trim() == src_fp)
                .unwrap_or(false)
        {
            continue;
        }

        tracing::info!(
            character = %identifier,
            source = %source.display(),
            out = %atmos.display(),
            "generating people-free Cameo atmosphere plate for world-asset style lock"
        );
        let tmp = atmos.with_extension("atmosphere_tmp.png");
        if let Err(err) = image
            .generate(CAMEO_ATMOSPHERE_PROMPT, &[source.as_path()], &tmp)
            .await
        {
            let _ = std::fs::remove_file(&tmp);
            tracing::warn!(
                character = %identifier,
                error = %err,
                "cameo atmosphere plate failed; world assets will use text scene lock only"
            );
            continue;
        }
        if !is_usable_image_file(&tmp) {
            let _ = std::fs::remove_file(&tmp);
            tracing::warn!(
                character = %identifier,
                "cameo atmosphere plate produced no image; skipping style ref"
            );
            continue;
        }
        if let Err(err) = copy_image_file_atomic(&tmp, &atmos) {
            let _ = std::fs::remove_file(&tmp);
            tracing::warn!(
                character = %identifier,
                error = %err,
                "failed to persist cameo atmosphere plate"
            );
            continue;
        }
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::write(&marker, src_fp.as_bytes());
    }
    Ok(())
}

fn cameo_raw_path(plate: &Path) -> PathBuf {
    let name = plate
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("cameo.png");
    let raw_name = if let Some(stem) = name.strip_suffix("_cameo.png") {
        format!("{stem}_cameo_raw.png")
    } else if let Some(stem) = name.strip_suffix(".png") {
        format!("{stem}_raw.png")
    } else {
        format!("{name}_raw.png")
    };
    plate.with_file_name(raw_name)
}

fn cameo_privacy_marker(plate: &Path) -> PathBuf {
    PathBuf::from(format!("{}.privacy_safe", plate.display()))
}

fn cameo_atmosphere_path(plate: &Path) -> PathBuf {
    let name = plate
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("cameo.png");
    let atmos_name = if let Some(stem) = name.strip_suffix("_cameo.png") {
        format!("{stem}_cameo_atmosphere.png")
    } else if let Some(stem) = name.strip_suffix(".png") {
        format!("{stem}_atmosphere.png")
    } else {
        format!("{name}_atmosphere.png")
    };
    plate.with_file_name(atmos_name)
}

fn cameo_atmosphere_marker(atmosphere: &Path) -> PathBuf {
    PathBuf::from(format!("{}.atmosphere_safe", atmosphere.display()))
}

fn ensure_cameo_raw_plate(plate: &Path, raw_path: &Path) -> VimaxResult<()> {
    if is_usable_image_file(raw_path) {
        return Ok(());
    }
    if !is_usable_image_file(plate) {
        return Err(VimaxError::InvalidParams(format!(
            "cameo plate missing for privacy anonymize: {}",
            plate.display()
        )));
    }
    copy_image_file_atomic(plate, raw_path)
}

fn file_fingerprint(path: &Path) -> VimaxResult<String> {
    let bytes = std::fs::read(path)?;
    let len = bytes.len();
    // Cheap stable token (sha256 of file) so re-uploads force re-anonymize.
    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    Ok(format!("{len}:{digest}"))
}

fn privacy_safe_registry_description(
    identifier: &str,
    prior: &HashMap<String, String>,
) -> String {
    let prior_desc = prior.get("description").map(|s| s.as_str()).unwrap_or("");
    let feats = prior_desc
        .split("Features:")
        .nth(1)
        .map(str::trim)
        .unwrap_or("")
        .trim_end_matches('.')
        .to_string();
    let extra = if feats.is_empty() {
        String::new()
    } else {
        format!(" Features: {feats}.")
    };
    format!(
        "File = USER CAMEO identity plate for <{identifier}> (privacy-safe face). \
Match body, hair silhouette, wardrobe, age, and overall look. The face is a generic \
unrecognizable virtual face — do NOT restore a real-person likeness.{extra}"
    )
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
    let descriptive = is_descriptive_role_label(cameo_name);
    // When the filename/prompt leaked into character_name, users often put the real
    // cast id in description — try that before idx-order fallback.
    let desc_name = photo.description.trim();
    let desc_as_name = !desc_name.is_empty()
        && !is_anonymous_cameo_name(desc_name)
        && !is_descriptive_role_label(desc_name)
        && (anonymous || descriptive || !cameo::names_match(cameo_name, desc_name));

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

    // 2) Exact / fuzzy match on character_name (skip when it's a prompt/camera stem
    // or a demographic shorthand like "中年男人").
    if !anonymous && !descriptive {
        if let Some(hit) = match_by_label(cameo_name, photo, visible, used)? {
            return Ok(hit);
        }
    }

    // 2b) Exact / fuzzy match on description when it looks like a real cast label.
    if desc_as_name {
        if let Some(hit) = match_by_label(desc_name, photo, visible, used)? {
            return Ok(hit);
        }
    }

    // 2c) Demographic shorthand ↔ cast static_features (e.g. "中年男人" ↔ "中年男性…").
    if descriptive || anonymous {
        if let Some(hit) = unique_feature_hit(cameo_name, visible, used) {
            return Ok(hit);
        }
        if !desc_name.is_empty() {
            if let Some(hit) = unique_feature_hit(desc_name, visible, used) {
                return Ok(hit);
            }
        }
    }

    let remaining: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| !used.contains(&c.identifier_in_scene))
        .collect();

    // 3) Single remaining cast member — always safe (covers camera-filename cameos).
    if remaining.len() == 1 {
        return Ok(remaining[0]);
    }

    // 4) Only one visible cast in the whole scene (extra photos overwrite the same cameo).
    if visible.len() == 1 {
        return Ok(visible[0]);
    }

    // 5) Anonymous / descriptive / prompt-stem multi-photo / multi-cast: bind by ascending idx.
    if (anonymous || descriptive) && !remaining.is_empty() {
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

/// Exact name → used overwrite → unique fuzzy. `None` means no unique hit (caller falls back).
fn match_by_label<'a>(
    label: &str,
    photo: &CameoPhotoEntry,
    visible: &[&'a CharacterInScene],
    used: &HashSet<String>,
) -> VimaxResult<Option<&'a CharacterInScene>> {
    let name_hits: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| {
            !used.contains(&c.identifier_in_scene)
                && cameo::names_match(label, &c.identifier_in_scene)
        })
        .collect();
    if name_hits.len() == 1 {
        return Ok(Some(name_hits[0]));
    }
    if name_hits.len() > 1 {
        return Err(VimaxError::InvalidParams(format!(
            "cameo label {:?} matches multiple cast members — rename the photo or characters",
            photo.character_name
        )));
    }

    // Same identity already bound (second photo for the same role) → overwrite.
    let used_name_hits: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| {
            used.contains(&c.identifier_in_scene)
                && cameo::names_match(label, &c.identifier_in_scene)
        })
        .collect();
    if used_name_hits.len() == 1 {
        return Ok(Some(used_name_hits[0]));
    }

    // Unique fuzzy containment (e.g. cameo "小林" ↔ cast "林").
    Ok(unique_fuzzy_name_hit(label, visible, used))
}

/// True when the "name" is almost certainly a camera / export / prompt stem, not a cast label.
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
    // AI image / scene-prompt stems used as filenames, e.g.
    // "cramped old style chinese workers village rental" — not a cast id.
    if looks_like_scene_prompt_stem(t) {
        return true;
    }
    // Demographic shorthand ("中年男人", "young woman") — not a script identifier.
    if is_descriptive_role_label(t) {
        return true;
    }
    false
}

/// Casting shorthand / demographic label rather than a script `identifier_in_scene`.
///
/// Users often name Cameo uploads "中年男人" / "年轻女人" while the story uses 陈树生 / 林秀兰.
pub(crate) fn is_descriptive_role_label(name: &str) -> bool {
    let t = name.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    const EN_NEEDLES: &[&str] = &[
        "middle-aged",
        "middle aged",
        "young man",
        "young woman",
        "young boy",
        "young girl",
        "old man",
        "old woman",
        "old lady",
        "elderly",
        "gentleman",
        "lady",
        "boy",
        "girl",
        "man",
        "woman",
        "guy",
        "dude",
    ];
    // Short English demographic phrases (avoid matching real names like "Norman").
    if !t.chars().any(is_cjk_char) {
        let words: Vec<&str> = lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
            .filter(|s| !s.is_empty())
            .collect();
        if words.len() <= 4
            && EN_NEEDLES
                .iter()
                .any(|n| lower == *n || lower.contains(n))
            && words.iter().all(|w| {
                matches!(
                    *w,
                    "a" | "an"
                        | "the"
                        | "middle"
                        | "aged"
                        | "young"
                        | "old"
                        | "elderly"
                        | "man"
                        | "woman"
                        | "boy"
                        | "girl"
                        | "lady"
                        | "gentleman"
                        | "guy"
                        | "dude"
                        | "male"
                        | "female"
                        | "person"
                )
            })
        {
            return true;
        }
    }

    const CJK_ROLE: &[&str] = &[
        "男人", "女人", "男子", "女子", "男性", "女性", "大叔", "阿姨", "大爷", "大妈", "老头",
        "老太太", "帅哥", "美女", "小伙", "姑娘", "男孩", "女孩", "小孩", "儿童", "老人", "青年",
        "中年", "少年", "少女", "孕妇", "汉子", "妇人", "少妇", "大叔",
    ];
    let has_role = CJK_ROLE.iter().any(|w| t.contains(w));
    if !has_role {
        return false;
    }
    // Typical proper names are 2–4 CJK chars without role nouns; descriptors are short too
    // but always include a role/age cue ("中年男人", "年轻女子").
    let cjk_count = t.chars().filter(|c| is_cjk_char(*c)).count();
    cjk_count >= 2 && cjk_count <= 10
}

/// Heuristic: multi-token or long caption-like stems are scene/prompt labels, not 姓名.
fn looks_like_scene_prompt_stem(name: &str) -> bool {
    let tokens: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .collect();
    // Four+ tokens almost never form a cast identifier ("Mary Jane Watson" is 3).
    if tokens.len() >= 4 {
        return true;
    }
    let has_cjk = name.chars().any(is_cjk_char);
    if !has_cjk {
        let alnum_len = name.chars().filter(|c| c.is_ascii_alphanumeric()).count();
        // Long latin phrases with several words (common Midjourney/SD stems).
        if alnum_len >= 28 && tokens.len() >= 3 {
            return true;
        }
    } else {
        // Long CJK-only captions (person names are typically 2–4 chars).
        let cjk_count = name.chars().filter(|c| is_cjk_char(*c)).count();
        let significant = name
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
            .count();
        if cjk_count >= 10 && cjk_count * 2 >= significant {
            return true;
        }
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

/// Match demographic Cameo labels against cast `static_features` (unique hit only).
fn unique_feature_hit<'a>(
    label: &str,
    visible: &[&'a CharacterInScene],
    used: &HashSet<String>,
) -> Option<&'a CharacterInScene> {
    let cues = demographic_cues(label);
    if cues.is_empty() {
        return None;
    }
    let hits: Vec<&&CharacterInScene> = visible
        .iter()
        .filter(|c| {
            if used.contains(&c.identifier_in_scene) {
                return false;
            }
            let feats = c.static_features.to_ascii_lowercase();
            let feats_norm = cameo::normalize_match_key(&c.static_features);
            cues.iter().any(|cue| feats.contains(cue) || feats_norm.contains(cue))
        })
        .collect();
    if hits.len() == 1 {
        Some(hits[0])
    } else {
        None
    }
}

fn demographic_cues(label: &str) -> Vec<String> {
    let t = label.trim();
    if t.is_empty() {
        return Vec::new();
    }
    let lower = t.to_ascii_lowercase();
    let mut cues = Vec::new();
    const PAIRS: &[(&str, &[&str])] = &[
        ("中年", &["中年"]),
        ("年轻", &["年轻", "青年"]),
        ("老年", &["老年", "年迈", "老人"]),
        ("男人", &["男人", "男性", "男子", "汉子"]),
        ("女人", &["女人", "女性", "女子", "妇人"]),
        ("男子", &["男人", "男性", "男子"]),
        ("女子", &["女人", "女性", "女子"]),
        ("男性", &["男人", "男性", "男子"]),
        ("女性", &["女人", "女性", "女子"]),
        ("大叔", &["大叔", "中年", "男性"]),
        ("阿姨", &["阿姨", "中年", "女性"]),
        ("男孩", &["男孩", "少年", "儿童"]),
        ("女孩", &["女孩", "少女", "儿童"]),
        ("小孩", &["小孩", "儿童"]),
        ("儿童", &["儿童", "小孩"]),
        ("少年", &["少年", "男孩"]),
        ("少女", &["少女", "女孩"]),
        ("middle-aged", &["中年", "middle"]),
        ("middle aged", &["中年", "middle"]),
        ("young man", &["年轻", "青年", "男性", "男人"]),
        ("young woman", &["年轻", "青年", "女性", "女人"]),
        ("old man", &["老年", "老人", "男性"]),
        ("old woman", &["老年", "老人", "女性"]),
    ];
    for (needle, outs) in PAIRS {
        if t.contains(needle) || lower.contains(needle) {
            for o in *outs {
                let s = (*o).to_string();
                if !cues.iter().any(|c| c == &s) {
                    cues.push(s);
                }
            }
        }
    }
    cues
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
    let raw = cameo_raw_path(&dest);
    let marker = cameo_privacy_marker(&dest);
    // Cameo uploads are already normalized PNG — copy instead of decode/re-encode
    // (phone photos are often 20–50MP; re-encoding OOM / Windows rename flakes).
    copy_image_file_atomic(&src, &raw)?;
    let raw_fp = file_fingerprint(&raw).unwrap_or_default();
    let keep_anonymized = is_usable_image_file(&dest)
        && marker.exists()
        && std::fs::read_to_string(&marker)
            .map(|s| s.trim() == raw_fp)
            .unwrap_or(false);
    if !keep_anonymized {
        copy_image_file_atomic(&src, &dest)?;
        let _ = std::fs::remove_file(&marker);
    }
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

/// Usable **people-free** Cameo atmosphere paths from film-root portrait dirs (max `max`).
///
/// Used as style/scene context refs for vacant environment/prop plates.
/// Portrait Cameo (`*_cameo.png`) is intentionally never returned — feeding faces into
/// prop img2img can bake people into “group photo” style props and trip Seedance privacy.
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
        let plate = crate::session::resolve_stored_asset_path(path, film_root);
        let atmos = cameo_atmosphere_path(&plate);
        if is_usable_image_file(&atmos) {
            out.push(atmos);
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
Do NOT invent a conflicting setting. Keep plates empty-set (no people). \
Do NOT invent props that are portraits, group photos, selfies, or any image-of-people objects:\n{}\n",
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
        "File [{file_name}] = USER CAMEO identity plate for <{}> (privacy-safe face pending/applied). \
Match body, hair silhouette, wardrobe, and overall look. Face is a generic unrecognizable virtual \
face — do NOT restore a real-person likeness. Features: {feats}.{extra}",
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
        char_with_features(idx, id, "tall")
    }

    fn char_with_features(idx: i32, id: &str, features: &str) -> CharacterInScene {
        CharacterInScene {
            idx,
            identifier_in_scene: id.into(),
            is_visible: true,
            static_features: features.into(),
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
        assert!(is_anonymous_cameo_name(
            "cramnped old style chinese workers vilage rental"
        ));
        assert!(is_anonymous_cameo_name(
            "cramped_old_style_chinese_workers_village_rental"
        ));
        assert!(is_anonymous_cameo_name("拥挤的老式中国工人村出租屋街景"));
        assert!(!is_anonymous_cameo_name("小"));
        assert!(!is_anonymous_cameo_name("Alice"));
        assert!(!is_anonymous_cameo_name("陈树生"));
        assert!(!is_anonymous_cameo_name("Mary Jane Watson"));
        assert!(is_descriptive_role_label("中年男人"));
        assert!(is_descriptive_role_label("年轻女人"));
        assert!(is_anonymous_cameo_name("中年男人"));
        assert!(!is_descriptive_role_label("陈树生"));
        assert!(!is_descriptive_role_label("林秀兰"));
    }

    #[test]
    fn descriptive_role_label_binds_by_features_or_idx() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "中年男人", "").unwrap();
        let characters = vec![
            char_with_features(0, "陈树生", "中年男性，短发，工人打扮"),
            char_with_features(1, "林秀兰", "年轻女性，长发"),
        ];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].1, "陈树生");
        assert!(has_usable_cameo(&registry, "陈树生"));
    }

    #[test]
    fn descriptive_role_falls_back_to_idx_without_features() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "中年男人", "").unwrap();
        let characters = vec![char(0, "陈树生"), char(1, "林秀兰")];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings[0].1, "陈树生");
    }

    #[test]
    fn scene_prompt_cameo_binds_multi_cast_by_idx() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(
            session,
            &jpeg_bytes(),
            "cramnped old style chinese workers vilage rental",
            "",
        )
        .unwrap();
        let characters = vec![char(0, "陈树生"), char(1, "林秀兰")];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].1, "陈树生");
        assert!(has_usable_cameo(&registry, "陈树生"));
    }

    #[test]
    fn description_label_binds_when_name_is_prompt_stem() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(
            session,
            &jpeg_bytes(),
            "cramnped old style chinese workers vilage rental",
            "林秀兰",
        )
        .unwrap();
        let characters = vec![char(0, "陈树生"), char(1, "林秀兰")];
        let mut registry = HashMap::new();
        let bindings = bind_cameos_to_registry(session, &film, &characters, &mut registry).unwrap();
        assert_eq!(bindings[0].1, "林秀兰");
        assert!(has_usable_cameo(&registry, "林秀兰"));
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
        // Portrait Cameo must not be used as world style refs (faces → prop leakage).
        assert!(refs.is_empty());
        assert!(hint.contains("CAMEO SCENE LOCK"));
        assert!(hint.contains("rainy street"));
        assert!(!token.is_empty());
        assert_eq!(cameo_style_lock_token(session), token);

        // Once an atmosphere plate exists, style refs prefer it over the portrait.
        let plate = PathBuf::from(
            registry
                .get("Alice")
                .and_then(|v| v.get("cameo"))
                .and_then(|i| i.get("path"))
                .unwrap(),
        );
        let atmos = cameo_atmosphere_path(&plate);
        std::fs::copy(&plate, &atmos).unwrap();
        let refs2 = cameo_style_ref_paths(&film, 2);
        assert_eq!(refs2.len(), 1);
        assert!(refs2[0].ends_with("Alice_cameo_atmosphere.png"));
        assert!(!refs2[0].to_string_lossy().ends_with("Alice_cameo.png"));
    }

    #[test]
    fn write_cameo_keeps_raw_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path();
        let film = session.join("idea2video");
        std::fs::create_dir_all(&film).unwrap();
        cameo::upload_photo(session, &jpeg_bytes(), "Alice", "").unwrap();
        let photos = cameo::list_photos(session).unwrap();
        let dest = write_cameo_portrait(&film, &char(0, "Alice"), &photos[0]).unwrap();
        assert!(is_usable_image_file(&dest));
        assert!(is_usable_image_file(&cameo_raw_path(&dest)));
        assert!(!cameo_privacy_marker(&dest).exists());
    }

    #[test]
    fn cameo_raw_and_marker_paths() {
        let plate = PathBuf::from("character_portraits/0_Alice/Alice_cameo.png");
        assert_eq!(
            cameo_raw_path(&plate),
            PathBuf::from("character_portraits/0_Alice/Alice_cameo_raw.png")
        );
        assert_eq!(
            cameo_atmosphere_path(&plate),
            PathBuf::from("character_portraits/0_Alice/Alice_cameo_atmosphere.png")
        );
        assert!(cameo_privacy_marker(&plate)
            .to_string_lossy()
            .ends_with("Alice_cameo.png.privacy_safe"));
        assert!(cameo_atmosphere_marker(&cameo_atmosphere_path(&plate))
            .to_string_lossy()
            .ends_with("Alice_cameo_atmosphere.png.atmosphere_safe"));
    }

    #[test]
    fn privacy_prompt_keeps_photoreal_and_swaps_face() {
        assert!(CAMEO_FACE_PRIVACY_PROMPT.contains("unrecognizable generic virtual face"));
        assert!(CAMEO_FACE_PRIVACY_PROMPT.contains("clothing"));
        assert!(CAMEO_FACE_PRIVACY_PROMPT.contains("photorealistic"));
        assert!(CAMEO_ATMOSPHERE_PROMPT.contains("people-free"));
        assert!(CAMEO_ATMOSPHERE_PROMPT.contains("Erase every person"));
    }
}
