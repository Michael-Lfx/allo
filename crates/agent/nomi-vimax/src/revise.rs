//! Artifact revision — faithful port of ViMax `_revise_narrative_artifact`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backends::VimaxChat;
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::{extract_json_str, strip_trailing_commas};

const REVISE_SYSTEM: &str = "Revise this ViMax structured artifact exactly as requested. \
Return only the complete replacement file content, with no Markdown fences or explanation. \
If the file is JSON, preserve valid JSON and the existing schema shape.";

/// Revise an artifact file in-place with the LLM, then invalidate stale downstream files.
pub async fn revise_artifact(
    chat: &Arc<dyn VimaxChat>,
    working_dir: &Path,
    revision_target: &str,
    revision_instruction: &str,
) -> VimaxResult<ReviseResult> {
    let instruction = revision_instruction.trim();
    if instruction.is_empty() {
        return Err(VimaxError::InvalidParams(
            "revision_instruction is required when revision_target is provided".into(),
        ));
    }
    let target_path = resolve_artifact_path(working_dir, revision_target)?;
    if !target_path.is_file() {
        return Err(VimaxError::InvalidParams(format!(
            "Revision target does not exist: {revision_target}"
        )));
    }

    let before = tokio::fs::read_to_string(&target_path).await?;
    let rel = target_path
        .strip_prefix(working_dir)
        .unwrap_or(&target_path)
        .to_string_lossy()
        .replace('\\', "/");

    let user = format!(
        "Target: {rel}\nRevision instruction: {instruction}\n\nCurrent file content:\n{before}"
    );
    let revised_raw = chat.complete_text(REVISE_SYSTEM, &user).await?;
    let mut revised = strip_markdown_fences(revised_raw.trim());

    if rel.ends_with(".json") || before.trim_start().starts_with('{') || before.trim_start().starts_with('[')
    {
        // Validate / pretty-print JSON when possible.
        match extract_json_str(&revised)
            .ok()
            .map(|s| strip_trailing_commas(&s))
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            Some(v) => {
                revised = serde_json::to_string_pretty(&v)?;
            }
            None => {
                return Err(VimaxError::Llm(format!(
                    "Revision output was not valid JSON for {rel}"
                )));
            }
        }
    }

    tokio::fs::write(&target_path, &revised).await?;
    let stale = stale_keys_for_revision(&rel);
    let invalidated = invalidate_stale(working_dir, &rel, &stale).await?;

    Ok(ReviseResult {
        revised_path: rel,
        stale_keys: stale.iter().map(|s| (*s).to_string()).collect(),
        invalidated,
    })
}

#[derive(Debug, Clone)]
pub struct ReviseResult {
    pub revised_path: String,
    pub stale_keys: Vec<String>,
    pub invalidated: Vec<String>,
}

fn resolve_artifact_path(working_dir: &Path, revision_target: &str) -> VimaxResult<PathBuf> {
    let cleaned = revision_target.replace('\\', "/");
    if cleaned.starts_with('/') || cleaned.contains("..") {
        return Err(VimaxError::InvalidParams(format!(
            "revision_target must be relative to session working_dir: {revision_target}"
        )));
    }
    let path = working_dir.join(&cleaned);
    let canon_root = working_dir
        .canonicalize()
        .unwrap_or_else(|_| working_dir.to_path_buf());
    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
    if canon != canon_root && !canon.starts_with(&canon_root) {
        return Err(VimaxError::InvalidParams(format!(
            "revision_target escapes session working_dir: {revision_target}"
        )));
    }
    Ok(path)
}

fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let mut lines: Vec<&str> = trimmed.lines().collect();
    if lines.first().is_some_and(|l| l.starts_with("```")) {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim() == "```") {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}

fn stale_keys_for_revision(target: &str) -> Vec<&'static str> {
    if target.contains("storyboard.json") {
        return vec![
            "shot_descriptions",
            "camera_tree",
            "frames",
            "clips",
            "final_video",
        ];
    }
    if target.contains("shot_description.json") {
        return vec!["frames", "clips", "final_video"];
    }
    if target.contains("camera_tree.json") {
        return vec!["frames", "clips", "final_video"];
    }
    if target.ends_with("script.json") || target.ends_with("story.txt") || target.ends_with("script.txt")
    {
        return vec![
            "storyboard",
            "shot_descriptions",
            "camera_tree",
            "frames",
            "clips",
            "final_video",
        ];
    }
    if target.ends_with("characters.json") {
        return vec![
            "storyboard",
            "shot_descriptions",
            "frames",
            "clips",
            "final_video",
        ];
    }
    vec!["frames", "clips", "final_video"]
}

async fn invalidate_stale(
    working_dir: &Path,
    revised_rel: &str,
    stale: &[&str],
) -> VimaxResult<Vec<String>> {
    let mut removed = Vec::new();
    let scope = revision_scope_dir(working_dir, revised_rel);

    for key in stale {
        match *key {
            "shot_descriptions" => {
                remove_if_exists(&scope.join("shot_descriptions.json"), &mut removed).await?;
                // Individual shot json kept; frames/clips cleared separately.
            }
            "camera_tree" => {
                remove_if_exists(&scope.join("camera_tree.json"), &mut removed).await?;
            }
            "storyboard" => {
                remove_if_exists(&scope.join("storyboard.json"), &mut removed).await?;
            }
            "frames" => {
                clear_shot_globs(&scope, &["first_frame.png", "last_frame.png", "new_camera_*.png", "*_selector_output.json", "transition_video_*.mp4"], &mut removed).await?;
            }
            "clips" => {
                clear_shot_globs(&scope, &["video.mp4"], &mut removed).await?;
            }
            "final_video" => {
                remove_if_exists(&scope.join("final_video.mp4"), &mut removed).await?;
                remove_if_exists(&working_dir.join("final_video.mp4"), &mut removed).await?;
            }
            _ => {}
        }
    }
    Ok(removed)
}

fn revision_scope_dir(working_dir: &Path, revised_rel: &str) -> PathBuf {
    // idea2video/scene_0/storyboard.json → idea2video/scene_0
    // script2video/storyboard.json → script2video
    let path = working_dir.join(revised_rel);
    if let Some(parent) = path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some("shots") {
            return parent
                .parent()
                .unwrap_or(working_dir)
                .to_path_buf();
        }
        return parent.to_path_buf();
    }
    working_dir.to_path_buf()
}

async fn remove_if_exists(path: &Path, removed: &mut Vec<String>) -> VimaxResult<()> {
    if path.is_file() {
        tokio::fs::remove_file(path).await?;
        removed.push(path.display().to_string());
    }
    Ok(())
}

async fn clear_shot_globs(
    scope: &Path,
    patterns: &[&str],
    removed: &mut Vec<String>,
) -> VimaxResult<()> {
    let shots = scope.join("shots");
    if !shots.is_dir() {
        return Ok(());
    }
    let mut rd = tokio::fs::read_dir(&shots).await?;
    while let Some(entry) = rd.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let dir = entry.path();
        for pat in patterns {
            if let Some(name) = pat.strip_suffix('*').and_then(|p| p.strip_prefix('*')) {
                // *foo* style — rare; treat as contains
                let mut inner = tokio::fs::read_dir(&dir).await?;
                while let Some(f) = inner.next_entry().await? {
                    let fname = f.file_name().to_string_lossy().to_string();
                    if fname.contains(name.trim_matches('*')) {
                        remove_if_exists(&f.path(), removed).await?;
                    }
                }
            } else if let Some(prefix) = pat.strip_suffix('*') {
                let mut inner = tokio::fs::read_dir(&dir).await?;
                while let Some(f) = inner.next_entry().await? {
                    let fname = f.file_name().to_string_lossy().to_string();
                    if fname.starts_with(prefix) {
                        remove_if_exists(&f.path(), removed).await?;
                    }
                }
            } else {
                remove_if_exists(&dir.join(pat), removed).await?;
            }
        }
    }
    Ok(())
}
