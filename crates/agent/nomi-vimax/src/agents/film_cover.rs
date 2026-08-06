//! Film poster / cover art — display-only key art (never muxed into the final video).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use walkdir::WalkDir;

use crate::backends::{VimaxChat, VimaxImage};
use crate::error::VimaxResult;
use crate::json_util::complete_and_parse_llm_json;
use crate::media_local::{self, is_usable_image_file};
use crate::progress::ProgressCallback;

const MAX_COVER_REFS: usize = 6;
pub const COVER_FILENAME: &str = "cover.png";

#[derive(Debug, Deserialize)]
struct CoverBrief {
    #[serde(default)]
    include_cast: bool,
    /// Short theme-bearing title / wordmark to paint onto the poster (may be empty).
    #[serde(default)]
    title_text: String,
    #[serde(default)]
    prompt: String,
}

/// Ensure `{film_dir}/cover.png` exists via asset-guided img2img.
/// Failures are logged and ignored so planning/render can continue.
pub async fn ensure_film_cover(
    film_dir: &Path,
    chat: Arc<dyn VimaxChat>,
    image: Arc<dyn VimaxImage>,
    style: &str,
    synopsis: &str,
    progress: &Option<ProgressCallback>,
) -> Option<PathBuf> {
    let out = film_dir.join(COVER_FILENAME);
    if is_usable_image_file(&out) {
        return Some(out);
    }

    if let Some(cb) = progress {
        cb("film_cover_start", "正在生成影片封面", None);
    }

    match generate_cover(film_dir, &out, chat, image, style, synopsis).await {
        Ok(()) if is_usable_image_file(&out) => {
            if let Some(cb) = progress {
                cb("film_cover_done", "影片封面已就绪", None);
            }
            Some(out)
        }
        Ok(()) => {
            tracing::warn!(path = %out.display(), "film cover write reported ok but file unusable");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, path = %out.display(), "film cover generation failed");
            let _ = std::fs::remove_file(&out);
            None
        }
    }
}

/// When AI cover is missing, extract a still from the finished film (~15% in).
pub async fn ensure_cover_from_final_video(
    film_dir: &Path,
    final_video: &Path,
) -> Option<PathBuf> {
    let out = film_dir.join(COVER_FILENAME);
    if is_usable_image_file(&out) {
        return Some(out);
    }
    if !final_video.is_file() {
        return None;
    }
    match media_local::extract_frame_at_ratio(final_video, &out, 0.15).await {
        Ok(()) if is_usable_image_file(&out) => Some(out),
        Ok(()) => {
            tracing::warn!(path = %out.display(), "cover frame extract wrote unusable file");
            None
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                video = %final_video.display(),
                "cover frame extract failed"
            );
            None
        }
    }
}

async fn generate_cover(
    film_dir: &Path,
    out: &Path,
    chat: Arc<dyn VimaxChat>,
    image: Arc<dyn VimaxImage>,
    style: &str,
    synopsis: &str,
) -> VimaxResult<()> {
    let aspect = load_aspect_ratio(film_dir).await;
    let candidates = collect_cover_candidates(film_dir);
    let brief = ask_cover_brief(chat.as_ref(), style, synopsis, &candidates, &aspect).await?;
    let refs = filter_refs_for_brief(&candidates, brief.include_cast);
    let mut prompt = brief.prompt.trim().to_string();
    if prompt.is_empty() {
        prompt = default_cover_prompt(style, synopsis, brief.include_cast, &brief.title_text, &aspect);
    }
    if !brief.include_cast {
        prompt.push_str(
            " No people, no faces, no cast likenesses — environment, mood, and objects only.",
        );
    }
    prompt.push_str(&title_lettering_clause(&brief.title_text));
    prompt.push_str(&format!(
        " {} film poster / key art, single strong composition, no watermark, no UI chrome, no buttons, no player controls.",
        crate::aspect::aspect_prompt_clause(&aspect)
    ));

    let ref_paths: Vec<&Path> = refs.iter().map(|p| p.as_path()).collect();
    image.generate(&prompt, &ref_paths, out).await?;
    let _ = crate::session::write_text_artifact(
        &film_dir.join("cover_generation_prompt.txt"),
        &prompt,
    )
    .await;
    let _ = crate::session::write_json_artifact(
        &film_dir.join("cover_brief.json"),
        &serde_json::json!({
            "include_cast": brief.include_cast,
            "title_text": brief.title_text,
            "prompt": prompt,
        }),
    )
    .await;
    Ok(())
}

async fn load_aspect_ratio(film_dir: &Path) -> String {
    crate::aspect::load_aspect_from_dir(film_dir).await
}

async fn ask_cover_brief(
    chat: &dyn VimaxChat,
    style: &str,
    synopsis: &str,
    candidates: &[(PathBuf, String)],
    aspect: &str,
) -> VimaxResult<CoverBrief> {
    let mut asset_lines = String::new();
    for (i, (path, kind)) in candidates.iter().enumerate() {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("asset.png");
        asset_lines.push_str(&format!("- [{i}] ({kind}) {name}\n"));
    }
    if asset_lines.is_empty() {
        asset_lines.push_str("(no reference assets yet)\n");
    }

    let syn = truncate(synopsis.trim(), 1200);
    let frame = crate::aspect::aspect_prompt_clause(aspect);
    let system = r#"You design a single film poster (key art) for a short AI-generated film.
Return ONLY one JSON object:
{"include_cast":true|false,"title_text":"string","prompt":"string"}
Rules:
- include_cast=true only when a person/character on the poster clearly helps sell the story.
- include_cast=false for mood pieces, landscapes, object-driven stories, or when faces would distract.
- title_text: a short theme-bearing title / wordmark (1–8 words or a few Chinese characters) that captures the film's meaning. Match the user's story language (Chinese story → 简体中文 title). Prefer evocative poster titles over literal long sentences. Empty only when lettering would hurt the image.
- prompt: detailed image-model instructions for one poster still. Prefer English for visual directions, but quote title_text exactly as given so the model can render those glyphs.
- The poster MUST match the requested aspect / frame orientation.
- The poster SHOULD include integrated title lettering (painted / typeset into the key art) when title_text is non-empty — like a real movie poster, not a UI overlay.
- Never ask for logos, subtitles blocks, watermarks, or player/UI chrome."#;

    let user = format!(
        "Visual style: {style}\nPoster frame: {frame} ({aspect})\n\nStory / synopsis:\n{syn}\n\nAvailable reference assets:\n{asset_lines}\nChoose include_cast, invent a short title_text, and write the poster prompt."
    );

    match complete_and_parse_llm_json::<CoverBrief>(chat, system, &user).await {
        Ok(mut b) => {
            b.title_text = sanitize_title_text(&b.title_text);
            Ok(b)
        }
        Err(e) => {
            tracing::warn!(error = %e, "cover brief JSON parse failed after retries; using defaults");
            let title = infer_fallback_title(synopsis);
            Ok(CoverBrief {
                include_cast: candidates.iter().any(|(_, k)| k == "cast"),
                title_text: title.clone(),
                prompt: default_cover_prompt(style, synopsis, true, &title, aspect),
            })
        }
    }
}

fn filter_refs_for_brief(candidates: &[(PathBuf, String)], include_cast: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // Prefer env → prop → cast (cast last / skipped).
    for want in ["environment", "prop", "cast"] {
        if want == "cast" && !include_cast {
            continue;
        }
        for (path, kind) in candidates {
            if kind == want && out.len() < MAX_COVER_REFS {
                out.push(path.clone());
            }
        }
    }
    out.truncate(MAX_COVER_REFS);
    out
}

fn collect_cover_candidates(film_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut cast = Vec::new();
    let mut env = Vec::new();
    let mut prop = Vec::new();
    if !film_dir.is_dir() {
        return Vec::new();
    }
    for entry in WalkDir::new(film_dir)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !name.ends_with(".png") && !name.ends_with(".jpg") && !name.ends_with(".jpeg") {
            continue;
        }
        if name == COVER_FILENAME {
            continue;
        }
        if !is_usable_image_file(path) {
            continue;
        }
        if name.contains("three_view") || name.contains("character_portrait") || name.contains("cameo")
        {
            cast.push((path.to_path_buf(), "cast".into()));
        } else if name.contains("environment_plate") || name.contains("environment") {
            env.push((path.to_path_buf(), "environment".into()));
        } else if name.contains("prop") {
            prop.push((path.to_path_buf(), "prop".into()));
        }
    }
    // Cap each bucket so WalkDir order doesn't flood refs.
    cast.truncate(3);
    env.truncate(3);
    prop.truncate(2);
    let mut all = Vec::new();
    all.extend(env);
    all.extend(prop);
    all.extend(cast);
    all
}

fn default_cover_prompt(
    style: &str,
    synopsis: &str,
    include_cast: bool,
    title_text: &str,
    aspect: &str,
) -> String {
    let syn = truncate(synopsis.trim(), 400);
    let frame = crate::aspect::aspect_prompt_clause(aspect);
    let base = if include_cast {
        format!(
            "{frame} cinematic key art poster, style: {style}. Story mood: {syn}. Hero characters in a dramatic still, film-poster lighting."
        )
    } else {
        format!(
            "{frame} cinematic key art poster, style: {style}. Story mood: {syn}. Evocative location/atmosphere only, no people."
        )
    };
    if title_text.trim().is_empty() {
        base
    } else {
        format!(
            "{base} Integrate elegant poster title lettering reading exactly \"{}\".",
            title_text.trim()
        )
    }
}

/// Image-model clause that paints theme-bearing glyphs into the key art.
fn title_lettering_clause(title_text: &str) -> String {
    let t = title_text.trim();
    if t.is_empty() {
        return String::new();
    }
    format!(
        " Include prominent, readable film-poster title lettering integrated into the composition that reads exactly \"{t}\" (theme-bearing wordmark / characters / letters — not a UI caption, not a watermark)."
    )
}

fn sanitize_title_text(raw: &str) -> String {
    let t = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '「' || c == '」' || c == '《' || c == '》')
        .trim();
    // Keep poster titles short so glyphs stay legible.
    truncate(t, 24)
}

/// Best-effort short title when the LLM brief fails to parse.
fn infer_fallback_title(synopsis: &str) -> String {
    let line = synopsis
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .trim_start_matches(['#', '*', '-', ' ', '　']);
    if line.is_empty() {
        return String::new();
    }
    sanitize_title_text(line)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    #[test]
    fn collect_skips_cover_and_classifies_assets() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("character_portraits/0_Alice")).unwrap();
        std::fs::create_dir_all(root.join("environments/0_ROOM")).unwrap();
        std::fs::create_dir_all(root.join("props/0_KEY")).unwrap();
        write_png(&root.join("character_portraits/0_Alice/alice_three_view.png"));
        write_png(&root.join("environments/0_ROOM/ROOM_environment_plate.png"));
        write_png(&root.join("props/0_KEY/KEY_prop.png"));
        write_png(&root.join(COVER_FILENAME));

        let c = collect_cover_candidates(root);
        assert!(c.iter().any(|(_, k)| k == "cast"));
        assert!(c.iter().any(|(_, k)| k == "environment"));
        assert!(c.iter().any(|(_, k)| k == "prop"));
        assert!(!c.iter().any(|(p, _)| {
            p.file_name().and_then(|s| s.to_str()) == Some(COVER_FILENAME)
        }));
    }

    #[test]
    fn filter_drops_cast_when_requested() {
        let candidates = vec![
            (PathBuf::from("a_three_view.png"), "cast".into()),
            (PathBuf::from("room_environment_plate.png"), "environment".into()),
            (PathBuf::from("key_prop.png"), "prop".into()),
        ];
        let refs = filter_refs_for_brief(&candidates, false);
        assert!(refs.iter().all(|p| {
            let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            !n.contains("three_view")
        }));
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn title_lettering_clause_quotes_theme_text() {
        let clause = title_lettering_clause("夜航");
        assert!(clause.contains("夜航"));
        assert!(clause.contains("title lettering"));
        assert!(title_lettering_clause("   ").is_empty());
    }

    #[test]
    fn sanitize_title_keeps_short_wordmark() {
        assert_eq!(sanitize_title_text("  「归途」 "), "归途");
        assert!(sanitize_title_text(&"中".repeat(40)).chars().count() <= 25);
    }

    fn write_png(path: &Path) {
        RgbImage::from_pixel(8, 8, Rgb([10, 20, 30]))
            .save(path)
            .unwrap();
    }
}
