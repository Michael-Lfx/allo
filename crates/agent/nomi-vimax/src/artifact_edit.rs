//! Direct artifact edits (text save / binary replace / image-prompt update).
//!
//! Complements LLM [`crate::revise`] with creator-facing "edit in place" flows
//! used by the Technical artifacts panel.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{VimaxError, VimaxResult};
use crate::revise::{invalidate_stale, resolve_artifact_path, stale_keys_for_revision, ReviseResult};

/// Soft limit for text / JSON artifact writes (2 MiB).
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
/// Soft limit for binary artifact replacement (10 MiB, matches Cameo).
pub const MAX_BINARY_BYTES: usize = 10 * 1024 * 1024;

/// Write (or overwrite) a text/JSON artifact, then invalidate downstream files.
pub async fn write_text_artifact(
    working_dir: &Path,
    relative_path: &str,
    content: &str,
) -> VimaxResult<ReviseResult> {
    if content.len() > MAX_TEXT_BYTES {
        return Err(VimaxError::InvalidParams(format!(
            "artifact content exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }
    let target_path = resolve_artifact_path(working_dir, relative_path)?;
    ensure_parent(&target_path).await?;

    let rel = normalize_rel(working_dir, &target_path);
    let mut body = content.to_string();
    if rel.ends_with(".json") || looks_like_json_document(content) {
        let value: Value = serde_json::from_str(content.trim()).map_err(|e| {
            VimaxError::InvalidParams(format!("invalid JSON for {rel}: {e}"))
        })?;
        body = serde_json::to_string_pretty(&value)?;
    }

    tokio::fs::write(&target_path, &body).await?;
    let stale = stale_keys_for_revision(&rel);
    let invalidated = invalidate_stale(working_dir, &rel, &stale).await?;
    Ok(ReviseResult {
        revised_path: rel,
        stale_keys: stale.iter().map(|s| (*s).to_string()).collect(),
        invalidated,
    })
}

/// Replace a binary artifact (typically an image) and invalidate dependent media.
pub async fn replace_binary_artifact(
    working_dir: &Path,
    relative_path: &str,
    bytes: &[u8],
) -> VimaxResult<ReviseResult> {
    if bytes.is_empty() {
        return Err(VimaxError::InvalidParams("empty file upload".into()));
    }
    if bytes.len() > MAX_BINARY_BYTES {
        return Err(VimaxError::InvalidParams(format!(
            "artifact file exceeds {MAX_BINARY_BYTES} bytes"
        )));
    }
    let target_path = resolve_artifact_path(working_dir, relative_path)?;
    if !is_replaceable_media(&relative_path.replace('\\', "/")) {
        return Err(VimaxError::InvalidParams(format!(
            "only image artifacts can be replaced via upload: {relative_path}"
        )));
    }
    ensure_parent(&target_path).await?;
    tokio::fs::write(&target_path, bytes).await?;

    let rel = normalize_rel(working_dir, &target_path);
    let mut invalidated = Vec::new();
    let lower = rel.to_ascii_lowercase();
    if lower.contains("character_portrait")
        || lower.contains("three_view")
        || lower.contains("/environments/")
        || lower.contains("/props/")
    {
        // Identity / world plates affect all downstream frames.
        let stale = ["frames", "clips", "final_video"];
        invalidated = invalidate_stale(working_dir, &rel, &stale).await?;
        Ok(ReviseResult {
            revised_path: rel,
            stale_keys: stale.iter().map(|s| (*s).to_string()).collect(),
            invalidated,
        })
    } else {
        // Shot frames: keep this file; clear sibling clip + final.
        invalidate_local_shot_media(&target_path, &mut invalidated).await?;
        remove_if_exists_file(&working_dir.join("final_video.mp4"), &mut invalidated).await?;
        if let Some(scope) = target_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            remove_if_exists_file(&scope.join("final_video.mp4"), &mut invalidated).await?;
        }
        Ok(ReviseResult {
            revised_path: rel,
            stale_keys: vec!["clips".into(), "final_video".into()],
            invalidated,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagePromptInfo {
    pub image_path: String,
    pub prompt_path: String,
    pub prompt: String,
    /// True when a companion selector / prompt file already exists on disk.
    pub exists: bool,
}

/// Resolve the editable image-generation prompt for a rendered frame image.
pub async fn get_image_prompt(
    working_dir: &Path,
    image_rel: &str,
) -> VimaxResult<ImagePromptInfo> {
    let image_path = resolve_artifact_path(working_dir, image_rel)?;
    let rel = normalize_rel(working_dir, &image_path);
    if !is_replaceable_media(&rel) {
        return Err(VimaxError::InvalidParams(format!(
            "not an image artifact: {image_rel}"
        )));
    }

    let selector_path = companion_selector_path(&image_path)?;
    let txt_path = companion_txt_prompt_path(&image_path)?;
    let prompt_path = if selector_path.is_file() {
        selector_path.clone()
    } else {
        txt_path.clone()
    };
    let prompt_rel = normalize_rel(working_dir, &prompt_path);

    // 1) Selector JSON (prefer the actual full generation prompt).
    if let Some(prompt) = read_prompt_from_selector(&selector_path).await? {
        return Ok(ImagePromptInfo {
            image_path: rel,
            prompt_path: prompt_rel,
            prompt,
            exists: true,
        });
    }

    // 2) Sidecar plain-text prompt written at generation time.
    if let Some(prompt) = read_nonempty_text_file(&txt_path).await? {
        return Ok(ImagePromptInfo {
            image_path: rel,
            prompt_path: normalize_rel(working_dir, &txt_path),
            prompt,
            exists: true,
        });
    }

    // 3) Shot frame fallbacks: shot_description / storyboard visual copy.
    if let Some(prompt) = fallback_shot_frame_prompt(working_dir, &image_path).await? {
        let _ = persist_prompt_sidecar_best_effort(&txt_path, &prompt).await;
        return Ok(ImagePromptInfo {
            image_path: rel,
            prompt_path: prompt_rel,
            prompt,
            exists: true,
        });
    }

    // 4) World asset plates / props — rebuild from world_assets.json or registry.
    if let Some(prompt) = fallback_world_asset_prompt(working_dir, &image_path).await? {
        let _ = persist_prompt_sidecar_best_effort(&txt_path, &prompt).await;
        return Ok(ImagePromptInfo {
            image_path: rel,
            prompt_path: prompt_rel,
            prompt,
            exists: true,
        });
    }

    // 5) Character three-view — rebuild from characters.json when sidecar missing.
    if let Some(prompt) = fallback_portrait_prompt(working_dir, &image_path).await? {
        let _ = persist_prompt_sidecar_best_effort(&txt_path, &prompt).await;
        return Ok(ImagePromptInfo {
            image_path: rel,
            prompt_path: prompt_rel,
            prompt,
            exists: true,
        });
    }

    // 6) Film cover brief sidecar / story synopsis fallback.
    if let Some(prompt) = fallback_cover_prompt(working_dir, &image_path).await? {
        let _ = persist_prompt_sidecar_best_effort(
            &image_path
                .parent()
                .unwrap_or(working_dir)
                .join("cover_generation_prompt.txt"),
            &prompt,
        )
        .await;
        return Ok(ImagePromptInfo {
            image_path: rel,
            prompt_path: prompt_rel,
            prompt,
            exists: true,
        });
    }

    // 7) Last resort: any sibling *_generation_prompt.txt / selector sharing a prefix.
    if let Some((path, prompt)) = fallback_sibling_prompt_files(&image_path).await? {
        return Ok(ImagePromptInfo {
            image_path: rel,
            prompt_path: normalize_rel(working_dir, &path),
            prompt,
            exists: true,
        });
    }

    Ok(ImagePromptInfo {
        image_path: rel,
        prompt_path: prompt_rel,
        prompt: String::new(),
        exists: false,
    })
}

async fn persist_prompt_sidecar_best_effort(path: &Path, prompt: &str) -> VimaxResult<()> {
    if prompt.trim().is_empty() || path.is_file() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, prompt).await?;
    Ok(())
}

/// Update the image prompt and drop the image so the next render regenerates it.
pub async fn update_image_prompt(
    working_dir: &Path,
    image_rel: &str,
    prompt: &str,
) -> VimaxResult<ReviseResult> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Err(VimaxError::InvalidParams(
            "image prompt must not be empty".into(),
        ));
    }
    if trimmed.len() > MAX_TEXT_BYTES {
        return Err(VimaxError::InvalidParams(format!(
            "image prompt exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }

    let image_path = resolve_artifact_path(working_dir, image_rel)?;
    let rel = normalize_rel(working_dir, &image_path);
    if !is_replaceable_media(&rel) {
        return Err(VimaxError::InvalidParams(format!(
            "not an image artifact: {image_rel}"
        )));
    }

    let selector_path = companion_selector_path(&image_path)?;
    let txt_path = companion_txt_prompt_path(&image_path)?;
    ensure_parent(&selector_path).await?;
    write_prompt_override(&selector_path, &txt_path, trimmed).await?;

    // Force pipeline resume to rebuild this frame from the updated selector.
    if image_path.is_file() {
        tokio::fs::remove_file(&image_path).await?;
    }

    let mut invalidated = vec![rel.clone()];
    invalidate_local_shot_media(&image_path, &mut invalidated).await?;
    remove_if_exists_file(&working_dir.join("final_video.mp4"), &mut invalidated).await?;
    if let Some(scope) = image_path.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
        remove_if_exists_file(&scope.join("final_video.mp4"), &mut invalidated).await?;
    }

    Ok(ReviseResult {
        revised_path: normalize_rel(working_dir, &selector_path),
        stale_keys: vec!["clips".into(), "final_video".into()],
        invalidated,
    })
}

fn companion_selector_path(image_path: &Path) -> VimaxResult<PathBuf> {
    let stem = prompt_lookup_stem(image_path)?;
    let parent = image_path
        .parent()
        .ok_or_else(|| VimaxError::InvalidParams("image has no parent directory".into()))?;
    Ok(parent.join(format!("{stem}_selector_output.json")))
}

fn companion_txt_prompt_path(image_path: &Path) -> VimaxResult<PathBuf> {
    let stem = prompt_lookup_stem(image_path)?;
    let parent = image_path
        .parent()
        .ok_or_else(|| VimaxError::InvalidParams("image has no parent directory".into()))?;
    Ok(parent.join(format!("{stem}_generation_prompt.txt")))
}

/// Normalize stems like `first_frame.privacy_bak` / `first_frame.i2v_stylized`
/// back to the canonical artifact name used for companion prompt files.
fn prompt_lookup_stem(image_path: &Path) -> VimaxResult<String> {
    let raw = image_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| VimaxError::InvalidParams("invalid image filename".into()))?;
    let mut stem = raw.to_string();
    for suffix in [".privacy_bak", ".i2v_stylized", ".stylized"] {
        if let Some(base) = stem.strip_suffix(suffix) {
            stem = base.to_string();
            break;
        }
    }
    Ok(stem)
}

async fn read_prompt_from_selector(path: &Path) -> VimaxResult<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        let trimmed = raw.trim();
        return Ok((!trimmed.is_empty()).then(|| trimmed.to_string()));
    };
    for key in [
        "full_prompt",
        "final_prompt",
        "generation_prompt",
        "text_prompt",
        "prompt",
    ] {
        if let Some(p) = value.get(key).and_then(|v| v.as_str()) {
            let t = p.trim();
            if !t.is_empty() {
                return Ok(Some(t.to_string()));
            }
        }
    }
    Ok(None)
}

async fn read_nonempty_text_file(path: &Path) -> VimaxResult<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    let trimmed = raw.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

async fn fallback_shot_frame_prompt(
    working_dir: &Path,
    image_path: &Path,
) -> VimaxResult<Option<String>> {
    let Some(shot_dir) = image_path.parent() else {
        return Ok(None);
    };
    let is_shot_dir = shot_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("shots");
    let desc_path = shot_dir.join("shot_description.json");
    // Accept either classic .../shots/N/ layout or any folder that already has a
    // shot_description.json next to the frame (imported / alternate layouts).
    if !is_shot_dir && !desc_path.is_file() {
        return Ok(None);
    }

    let stem = prompt_lookup_stem(image_path)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if desc_path.is_file() {
        if let Ok(raw) = tokio::fs::read_to_string(&desc_path).await {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                let preferred = if stem.contains("last_frame") {
                    [
                        "lf_desc",
                        "visual_desc",
                        "ff_desc",
                        "motion_desc",
                        "description",
                        "prompt",
                    ]
                } else {
                    [
                        "ff_desc",
                        "visual_desc",
                        "lf_desc",
                        "motion_desc",
                        "description",
                        "prompt",
                    ]
                };
                for key in preferred {
                    if let Some(p) = value.get(key).and_then(|v| v.as_str()) {
                        let t = p.trim();
                        if !t.is_empty() {
                            return Ok(Some(t.to_string()));
                        }
                    }
                }
            }
        }
    }

    // storyboard.json one level above shots/
    if let Some(scene_root) = shot_dir.parent().and_then(|p| p.parent()) {
        let board = scene_root.join("storyboard.json");
        if board.is_file() {
            let shot_idx = shot_dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|s| s.parse::<i64>().ok());
            if let Ok(raw) = tokio::fs::read_to_string(&board).await {
                if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                    if let Some(prompt) = visual_desc_from_storyboard(&parsed, shot_idx) {
                        return Ok(Some(prompt));
                    }
                }
            }
        }
    }

    let _ = working_dir;
    Ok(None)
}

async fn fallback_world_asset_prompt(
    working_dir: &Path,
    image_path: &Path,
) -> VimaxResult<Option<String>> {
    let path_norm = normalize_path_str(&image_path.to_string_lossy()).to_lowercase();
    let name = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let under_env = path_norm.contains("/environments/");
    let under_prop = path_norm.contains("/props/");
    let is_env = under_env || name.contains("environment_plate") || name == "plate.png";
    let is_prop = under_prop || name.contains("_prop.") || name == "prop.png";
    if !is_env && !is_prop {
        return Ok(None);
    }

    let film_root = find_dir_with_file(image_path, "world_assets.json")
        .or_else(|| find_dir_with_file(image_path, "world_assets_registry.json"))
        .unwrap_or_else(|| working_dir.to_path_buf());

    let style = read_nonempty_text_file(&film_root.join("style.txt"))
        .await?
        .or(read_nonempty_text_file(&working_dir.join("style.txt")).await?)
        .unwrap_or_default();
    let style_clause = crate::planning::style_prompt_clause(&style);
    let theme = read_nonempty_text_file(&film_root.join("story.txt"))
        .await?
        .or(read_nonempty_text_file(&working_dir.join("story.txt")).await?)
        .unwrap_or_else(|| style.clone());
    let theme_short: String = theme_excerpt_for_prompt(&theme);

    let parent_name = image_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let dir_idx = parent_name
        .split_once('_')
        .and_then(|(idx, _)| idx.parse::<usize>().ok());

    let spec_path = film_root.join("world_assets.json");
    if spec_path.is_file() {
        if let Ok(raw) = tokio::fs::read_to_string(&spec_path).await {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                if is_env {
                    if let Some(envs) = value.get("environments").and_then(|v| v.as_array()) {
                        let mut matched: Option<&Value> = None;
                        for env in envs {
                            let slug = env
                                .get("slugline")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let idx = env.get("idx").and_then(|v| v.as_u64()).map(|v| v as usize);
                            if slug_matches_image(slug, image_path)
                                || path_contains_slug(image_path, slug)
                                || (dir_idx.is_some() && dir_idx == idx)
                                || (!parent_name.is_empty()
                                    && !slug.is_empty()
                                    && path_loose_match(Path::new(&parent_name), slug))
                            {
                                matched = Some(env);
                                break;
                            }
                        }
                        if matched.is_none() && envs.len() == 1 {
                            matched = envs.first();
                        }
                        if let Some(env) = matched {
                            let slug = env
                                .get("slugline")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let desc = env
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let prompt = include_str!(
                                "../prompts/world_assets__prompt_template_environment_plate.txt"
                            )
                            .replace("{theme}", &theme_short)
                            .replace("{slugline}", slug)
                            .replace("{description}", &strip_people_light(desc))
                            .replace("{style}", &style_clause);
                            return Ok(Some(prompt));
                        }
                    }
                }
                if is_prop {
                    if let Some(props) = value.get("props").and_then(|v| v.as_array()) {
                        let mut matched: Option<&Value> = None;
                        for prop in props {
                            let prop_name = prop.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let idx = prop.get("idx").and_then(|v| v.as_u64()).map(|v| v as usize);
                            if slug_matches_image(prop_name, image_path)
                                || path_contains_slug(image_path, prop_name)
                                || (dir_idx.is_some() && dir_idx == idx)
                                || (!parent_name.is_empty()
                                    && !prop_name.is_empty()
                                    && path_loose_match(Path::new(&parent_name), prop_name))
                            {
                                matched = Some(prop);
                                break;
                            }
                        }
                        if matched.is_none() && props.len() == 1 {
                            matched = props.first();
                        }
                        if let Some(prop) = matched {
                            let prop_name = prop.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let desc = prop
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let prompt = include_str!(
                                "../prompts/world_assets__prompt_template_prop.txt"
                            )
                            .replace("{theme}", &theme_short)
                            .replace("{name}", prop_name)
                            .replace("{description}", &strip_people_light(desc))
                            .replace("{style}", &style_clause);
                            return Ok(Some(prompt));
                        }
                    }
                }
            }
        }
    }

    // Registry description is weaker but better than empty.
    if let Some(prompt) = registry_description_for_image(&film_root, image_path).await? {
        return Ok(Some(prompt));
    }
    Ok(None)
}

fn theme_excerpt_for_prompt(script_or_story: &str) -> String {
    let compact: String = script_or_story
        .split_whitespace()
        .take(40)
        .collect::<Vec<_>>()
        .join(" ");
    let excerpt: String = if compact.is_empty() {
        script_or_story.chars().take(140).collect()
    } else {
        compact.chars().take(140).collect()
    };
    excerpt
}

async fn fallback_portrait_prompt(
    working_dir: &Path,
    image_path: &Path,
) -> VimaxResult<Option<String>> {
    let path_norm = normalize_path_str(&image_path.to_string_lossy()).to_lowercase();
    let name = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_portrait = path_norm.contains("character_portrait")
        || name.contains("three_view")
        || name == "three_view.png";
    if !is_portrait {
        return Ok(None);
    }

    let film_root = find_dir_with_file(image_path, "characters.json")
        .unwrap_or_else(|| working_dir.to_path_buf());
    let chars_path = film_root.join("characters.json");
    if !chars_path.is_file() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(&chars_path).await?;
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Ok(None);
    };
    let rows: Vec<&Value> = if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else if let Some(arr) = value.get("characters").and_then(|v| v.as_array()) {
        arr.iter().collect()
    } else {
        return Ok(None);
    };

    let parent_name = image_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let stem = prompt_lookup_stem(image_path).unwrap_or_default();

    let mut matched: Option<&Value> = None;
    for ch in &rows {
        let id = ch
            .get("identifier_in_scene")
            .or_else(|| ch.get("identifier"))
            .or_else(|| ch.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if id.is_empty() {
            continue;
        }
        if path_loose_match(image_path, id)
            || path_loose_match(Path::new(parent_name), id)
            || stem.to_ascii_lowercase().contains(&slug_to_safe(id).to_ascii_lowercase())
            || parent_name.to_ascii_lowercase().contains(&slug_to_safe(id).to_ascii_lowercase())
        {
            matched = Some(ch);
            break;
        }
    }
    if matched.is_none() && rows.len() == 1 {
        matched = rows.first().copied();
    }
    let Some(ch) = matched else {
        return Ok(None);
    };

    let id = ch
        .get("identifier_in_scene")
        .or_else(|| ch.get("identifier"))
        .or_else(|| ch.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("character");
    let static_f = ch
        .get("static_features")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let dynamic_f = ch
        .get("dynamic_features")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let features = format!("(static) {static_f}; (dynamic) {dynamic_f}");
    let features: String = features.chars().take(520).collect();
    let style = read_nonempty_text_file(&film_root.join("style.txt"))
        .await?
        .or(read_nonempty_text_file(&working_dir.join("style.txt")).await?)
        .unwrap_or_default();
    let style_line = crate::planning::portrait_style_line_for_image(&style);
    let medium_lock = crate::planning::portrait_medium_lock_line(&style);
    let face_guidance =
        crate::planning::portrait_face_clause_for_character(id, &features, &style);
    let age_lock =
        crate::planning::child_style_lock_if_needed_for_style(id, &features, &style);
    let prompt = include_str!(
        "../prompts/character_portraits_generator__prompt_template_three_view.txt"
    )
    .replace("{identifier}", id)
    .replace("{features}", &features)
    .replace("{style}", &style_line)
    .replace("{medium_lock}", &medium_lock)
    .replace("{face_guidance}", &face_guidance)
    .replace("{age_lock}", &age_lock);
    Ok(Some(prompt))
}

async fn registry_description_for_image(
    film_root: &Path,
    image_path: &Path,
) -> VimaxResult<Option<String>> {
    let registry_path = film_root.join("world_assets_registry.json");
    if !registry_path.is_file() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(&registry_path).await?;
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Ok(None);
    };
    let target = normalize_path_str(&image_path.to_string_lossy());
    let Some(obj) = value.as_object() else {
        return Ok(None);
    };
    for (_group, group_val) in obj {
        let Some(items) = group_val.as_object() else {
            continue;
        };
        for (_key, item) in items {
            let path = item
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let desc = item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if desc.trim().is_empty() {
                continue;
            }
            let norm = normalize_path_str(path);
            if target.ends_with(&norm)
                || norm.ends_with(
                    image_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(""),
                )
                || paths_share_filename(image_path, path)
            {
                return Ok(Some(desc.trim().to_string()));
            }
        }
    }
    Ok(None)
}

async fn fallback_cover_prompt(
    working_dir: &Path,
    image_path: &Path,
) -> VimaxResult<Option<String>> {
    let name = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name != "cover.png" && name != "cover.jpg" && name != "cover.jpeg" && name != "cover.webp"
    {
        return Ok(None);
    }
    let parent = image_path.parent().unwrap_or(working_dir);
    if let Some(prompt) = read_nonempty_text_file(&parent.join("cover_generation_prompt.txt")).await?
    {
        return Ok(Some(prompt));
    }
    let brief = parent.join("cover_brief.json");
    if brief.is_file() {
        if let Ok(raw) = tokio::fs::read_to_string(&brief).await {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                if let Some(p) = value.get("prompt").and_then(|v| v.as_str()) {
                    let t = p.trim();
                    if !t.is_empty() {
                        return Ok(Some(t.to_string()));
                    }
                }
            }
        }
    }
    // Last resort: synthesize a usable poster prompt from story + style.
    let style = read_nonempty_text_file(&parent.join("style.txt"))
        .await?
        .or(read_nonempty_text_file(&working_dir.join("style.txt")).await?)
        .unwrap_or_default();
    let story = read_nonempty_text_file(&parent.join("story.txt"))
        .await?
        .or(read_nonempty_text_file(&parent.join("script.txt")).await?)
        .or(read_nonempty_text_file(&working_dir.join("story.txt")).await?)
        .or(read_nonempty_text_file(&working_dir.join("script.txt")).await?)
        .unwrap_or_default();
    if story.trim().is_empty() && style.trim().is_empty() {
        return Ok(None);
    }
    let syn: String = story.chars().take(600).collect();
    let style_clause = crate::planning::style_prompt_clause(&style);
    Ok(Some(format!(
        "{style_clause} Film poster / key art. Story mood: {syn}. Single strong composition, no watermark, no UI chrome."
    )))
}

async fn fallback_sibling_prompt_files(image_path: &Path) -> VimaxResult<Option<(PathBuf, String)>> {
    let Some(parent) = image_path.parent() else {
        return Ok(None);
    };
    let stem = prompt_lookup_stem(image_path).unwrap_or_default();
    if !parent.is_dir() {
        return Ok(None);
    }
    let mut rd = tokio::fs::read_dir(parent).await?;
    let mut exact: Vec<PathBuf> = Vec::new();
    let mut loose: Vec<PathBuf> = Vec::new();
    while let Some(entry) = rd.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with("_generation_prompt.txt")
            || lower.ends_with("_selector_output.json"))
        {
            continue;
        }
        let path = entry.path();
        let stem_l = stem.to_ascii_lowercase();
        if !stem_l.is_empty() && lower.starts_with(&stem_l) {
            exact.push(path);
        } else if lower.contains("generation_prompt")
            || lower.contains("selector_output")
            || (!stem_l.is_empty()
                && stem_l
                    .split('_')
                    .next()
                    .is_some_and(|p| p.len() >= 2 && lower.contains(p)))
        {
            loose.push(path);
        }
    }
    exact.sort();
    loose.sort();
    for path in exact.into_iter().chain(loose) {
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(prompt) = read_prompt_from_selector(&path).await? {
                return Ok(Some((path, prompt)));
            }
        } else if let Some(prompt) = read_nonempty_text_file(&path).await? {
            return Ok(Some((path, prompt)));
        }
    }
    Ok(None)
}

fn find_dir_with_file(start: &Path, filename: &str) -> Option<PathBuf> {
    let mut cur = start.parent()?;
    for _ in 0..8 {
        if cur.join(filename).is_file() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
    None
}

fn slug_matches_image(slug: &str, image_path: &Path) -> bool {
    path_loose_match(image_path, slug)
}

fn path_contains_slug(image_path: &Path, slug: &str) -> bool {
    path_loose_match(image_path, slug)
}

fn path_loose_match(image_path: &Path, label: &str) -> bool {
    let path = normalize_path_str(&image_path.to_string_lossy()).to_lowercase();
    let compact: String = label
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '.' && *c != ',' && *c != '，' && *c != '。')
        .collect();
    let compact_l = compact.to_lowercase();
    if compact_l.chars().count() >= 4 && path.contains(&compact_l) {
        return true;
    }
    for token in label.split(|c: char| !(c.is_alphanumeric() || is_cjk(c))) {
        if token.chars().count() >= 2 && path.contains(&token.to_lowercase()) {
            return true;
        }
    }
    let safe = slug_to_safe(label);
    !safe.is_empty() && path.contains(&safe.to_lowercase())
}

fn is_cjk(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
}

fn slug_to_safe(s: &str) -> String {
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
    out.trim_matches('_').chars().take(80).collect()
}

fn normalize_path_str(s: &str) -> String {
    s.replace('\\', "/")
}

fn paths_share_filename(image_path: &Path, other: &str) -> bool {
    let left = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let right = Path::new(other)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(other);
    !left.is_empty() && left.eq_ignore_ascii_case(right)
}

fn strip_people_light(text: &str) -> String {
    // Lightweight mirror of world_assets::strip_people_mentions for prompt rebuild.
    let mut s = text.to_string();
    for p in ["人影", "人群", "人们", "行人", "顾客", "people", "crowd", "person"] {
        s = s.replace(p, "");
    }
    s
}

fn visual_desc_from_storyboard(parsed: &Value, shot_idx: Option<i64>) -> Option<String> {
    let rows: &[Value] = if let Some(arr) = parsed.as_array() {
        arr.as_slice()
    } else if let Some(obj) = parsed.as_object() {
        obj.get("storyboard")
            .or_else(|| obj.get("shots"))
            .and_then(|v| v.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[])
    } else {
        &[]
    };
    if rows.is_empty() {
        return None;
    }
    let row = if let Some(idx) = shot_idx {
        rows.iter().find(|row| {
            row.get("idx")
                .or_else(|| row.get("index"))
                .or_else(|| row.get("shot_index"))
                .and_then(|v| v.as_i64())
                == Some(idx)
        })
        .or_else(|| rows.get(idx as usize))
    } else {
        rows.first()
    }?;
    for key in ["visual_desc", "visualDescription", "description", "prompt"] {
        if let Some(p) = row.get(key).and_then(|v| v.as_str()) {
            let t = p.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

async fn write_prompt_override(
    selector_path: &Path,
    txt_path: &Path,
    prompt: &str,
) -> VimaxResult<()> {
    let mut value = if selector_path.is_file() {
        let raw = tokio::fs::read_to_string(selector_path)
            .await
            .unwrap_or_default();
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| {
            serde_json::json!({
                "reference_image_path_and_text_pairs": [],
                "text_prompt": "",
            })
        })
    } else {
        serde_json::json!({
            "reference_image_path_and_text_pairs": [],
            "text_prompt": "",
        })
    };

    if let Some(obj) = value.as_object_mut() {
        // Keep text_prompt for older readers; mark full_prompt as the override used on regen.
        obj.insert("text_prompt".into(), Value::String(prompt.to_string()));
        obj.insert("full_prompt".into(), Value::String(prompt.to_string()));
        obj.insert("prompt_override".into(), Value::Bool(true));
    }
    let pretty = serde_json::to_string_pretty(&value)?;
    tokio::fs::write(selector_path, pretty).await?;
    // Sidecar mirrors the editable prompt for non-JSON consumers / portraits.
    if let Some(parent) = txt_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(txt_path, prompt).await?;
    Ok(())
}

fn is_replaceable_media(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".bmp")
}

async fn invalidate_local_shot_media(
    image_path: &Path,
    removed: &mut Vec<String>,
) -> VimaxResult<()> {
    let Some(shot_dir) = image_path.parent() else {
        return Ok(());
    };
    // Only clear sibling clip when this looks like a shot folder (.../shots/N).
    let is_shot_dir = shot_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("shots");
    if is_shot_dir {
        remove_if_exists_file(&shot_dir.join("video.mp4"), removed).await?;
        remove_if_exists_file(&shot_dir.join("video.webm"), removed).await?;
        remove_if_exists_file(&shot_dir.join("video.mov"), removed).await?;
    }
    Ok(())
}

async fn remove_if_exists_file(path: &Path, removed: &mut Vec<String>) -> VimaxResult<()> {
    if path.is_file() {
        tokio::fs::remove_file(path).await?;
        removed.push(path.display().to_string());
    }
    Ok(())
}

fn looks_like_json_document(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with('{') || t.starts_with('[')
}

fn normalize_rel(working_dir: &Path, abs: &Path) -> String {
    abs.strip_prefix(working_dir)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

async fn ensure_parent(path: &Path) -> VimaxResult<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn write_text_pretty_prints_json_and_invalidates() {
        let dir = tempdir().unwrap();
        let storyboard = dir.path().join("script2video");
        tokio::fs::create_dir_all(storyboard.join("shots/0")).await.unwrap();
        tokio::fs::write(
            storyboard.join("storyboard.json"),
            r#"[{"idx":0,"visual_desc":"old"}]"#,
        )
        .await
        .unwrap();
        tokio::fs::write(storyboard.join("shots/0/first_frame.png"), b"png")
            .await
            .unwrap();
        tokio::fs::write(storyboard.join("shots/0/video.mp4"), b"mp4")
            .await
            .unwrap();

        let result = write_text_artifact(
            dir.path(),
            "script2video/storyboard.json",
            r#"[{"idx":0,"visual_desc":"new rain"}]"#,
        )
        .await
        .unwrap();

        assert_eq!(result.revised_path, "script2video/storyboard.json");
        assert!(!storyboard.join("shots/0/first_frame.png").exists());
        assert!(!storyboard.join("shots/0/video.mp4").exists());
        let saved = tokio::fs::read_to_string(storyboard.join("storyboard.json"))
            .await
            .unwrap();
        assert!(saved.contains("new rain"));
        assert!(saved.contains('\n'));
    }

    #[tokio::test]
    async fn replace_image_keeps_file_clears_clip() {
        let dir = tempdir().unwrap();
        let shot = dir.path().join("script2video/shots/0");
        tokio::fs::create_dir_all(&shot).await.unwrap();
        tokio::fs::write(shot.join("first_frame.png"), b"old").await.unwrap();
        tokio::fs::write(shot.join("video.mp4"), b"mp4").await.unwrap();

        replace_binary_artifact(
            dir.path(),
            "script2video/shots/0/first_frame.png",
            b"new-png-bytes",
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read(shot.join("first_frame.png")).await.unwrap(),
            b"new-png-bytes"
        );
        assert!(!shot.join("video.mp4").exists());
    }

    #[tokio::test]
    async fn update_image_prompt_rewrites_selector_and_drops_frame() {
        let dir = tempdir().unwrap();
        let shot = dir.path().join("script2video/shots/0");
        tokio::fs::create_dir_all(&shot).await.unwrap();
        tokio::fs::write(shot.join("first_frame.png"), b"png").await.unwrap();
        tokio::fs::write(
            shot.join("first_frame_selector_output.json"),
            r#"{"reference_image_path_and_text_pairs":[],"text_prompt":"old prompt","full_prompt":"full old prompt"}"#,
        )
        .await
        .unwrap();

        let info = get_image_prompt(dir.path(), "script2video/shots/0/first_frame.png")
            .await
            .unwrap();
        assert!(info.exists);
        assert_eq!(info.prompt, "full old prompt");

        update_image_prompt(
            dir.path(),
            "script2video/shots/0/first_frame.png",
            "cinematic rain push-in",
        )
        .await
        .unwrap();

        assert!(!shot.join("first_frame.png").exists());
        let saved: Value = serde_json::from_str(
            &tokio::fs::read_to_string(shot.join("first_frame_selector_output.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved.get("full_prompt").and_then(|v| v.as_str()),
            Some("cinematic rain push-in")
        );
        assert_eq!(saved.get("prompt_override").and_then(|v| v.as_bool()), Some(true));
    }

    #[tokio::test]
    async fn get_image_prompt_falls_back_to_shot_description() {
        let dir = tempdir().unwrap();
        let shot = dir.path().join("script2video/shots/0");
        tokio::fs::create_dir_all(&shot).await.unwrap();
        tokio::fs::write(shot.join("first_frame.png"), b"png").await.unwrap();
        tokio::fs::write(
            shot.join("shot_description.json"),
            r#"{"idx":0,"ff_desc":"close-up in rain","visual_desc":"wide establishing"}"#,
        )
        .await
        .unwrap();

        let info = get_image_prompt(dir.path(), "script2video/shots/0/first_frame.png")
            .await
            .unwrap();
        assert!(info.exists);
        assert_eq!(info.prompt, "close-up in rain");
        // Fallback should be persisted for next open.
        assert!(shot.join("first_frame_generation_prompt.txt").is_file());
    }

    #[tokio::test]
    async fn get_image_prompt_rebuilds_chinese_environment_plate() {
        let dir = tempdir().unwrap();
        let film = dir.path().join("idea2video");
        let env_dir = film.join("environments/0_老夜巷口");
        tokio::fs::create_dir_all(&env_dir).await.unwrap();
        tokio::fs::write(env_dir.join("雨夜巷口_environment_plate.png"), b"png")
            .await
            .unwrap();
        tokio::fs::write(film.join("style.txt"), "cinematic noir").await.unwrap();
        tokio::fs::write(film.join("story.txt"), "雨夜里的追逐").await.unwrap();
        tokio::fs::write(
            film.join("world_assets.json"),
            r#"{"environments":[{"idx":0,"slugline":"雨夜巷口","description":"狭窄石板路，霓虹倒影，无行人"}],"props":[]}"#,
        )
        .await
        .unwrap();

        let info = get_image_prompt(
            dir.path(),
            "idea2video/environments/0_雨夜巷口/雨夜巷口_environment_plate.png",
        )
        .await
        .unwrap();
        assert!(info.exists);
        assert!(info.prompt.contains("雨夜巷口"));
        assert!(info.prompt.contains("cinematic") || info.prompt.contains("noir") || !info.prompt.is_empty());
    }

    #[tokio::test]
    async fn get_image_prompt_rebuilds_prop_by_dir_index() {
        let dir = tempdir().unwrap();
        let film = dir.path().join("idea2video");
        let prop_dir = film.join("props/1_红伞");
        tokio::fs::create_dir_all(&prop_dir).await.unwrap();
        tokio::fs::write(prop_dir.join("红伞_prop.png"), b"png").await.unwrap();
        tokio::fs::write(film.join("style.txt"), "anime").await.unwrap();
        tokio::fs::write(
            film.join("world_assets.json"),
            r#"{"environments":[],"props":[{"idx":0,"name":"旧怀表","description":"铜制"},{"idx":1,"name":"红伞","description":"油纸伞"}]}"#,
        )
        .await
        .unwrap();

        let info = get_image_prompt(dir.path(), "idea2video/props/1_红伞/红伞_prop.png")
            .await
            .unwrap();
        assert!(info.exists, "prompt empty: {:?}", info.prompt);
        assert!(
            info.prompt.contains("红伞") || info.prompt.contains("油纸"),
            "unexpected prompt: {}",
            info.prompt
        );
        assert!(
            !info.prompt.contains("旧怀表"),
            "matched wrong prop: {}",
            info.prompt
        );
    }
}
