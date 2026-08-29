//! Aspect ratios for **Seedance video** + **Seedream film cover** only.
//!
//! Character portraits / world plates intentionally ignore session aspect and
//! keep the model default canvas (`2K`). Cover generation maps the session
//! ratio onto Seedream-supported 2K pixel sizes (unsupported → 16:9).

use std::path::Path;

/// Ratios accepted by Seedance 2.0 / 2.0-fast (`ratio` API field).
/// These are also in Seedream 5.x's supported ratio set.
pub const SEEDANCE_ASPECT_RATIOS: &[&str] =
    &["16:9", "9:16", "1:1", "4:3", "3:4", "21:9"];

pub const DEFAULT_ASPECT_RATIO: &str = "16:9";

/// Integer parts of a normalized Seedance ratio (`16:9` → `(16, 9)`).
pub fn aspect_parts(ratio: &str) -> (u32, u32) {
    match normalize_aspect_ratio(ratio).as_str() {
        "9:16" => (9, 16),
        "1:1" => (1, 1),
        "4:3" => (4, 3),
        "3:4" => (3, 4),
        "21:9" => (21, 9),
        _ => (16, 9),
    }
}

/// Read `aspect_ratio.txt` from `dir` or its parent (film root vs scene workdir).
pub async fn load_aspect_from_dir(dir: &Path) -> String {
    for d in [dir, dir.parent().unwrap_or(dir)] {
        let p = d.join("aspect_ratio.txt");
        if let Ok(text) = tokio::fs::read_to_string(&p).await {
            let n = normalize_aspect_ratio(&text);
            if !n.is_empty() {
                return n;
            }
        }
    }
    DEFAULT_ASPECT_RATIO.to_string()
}

/// Normalize user/config aspect to a Seedance + Seedream-supported value (default 16:9).
pub fn normalize_aspect_ratio(raw: &str) -> String {
    let t = raw.trim().replace('：', ":");
    let lower = t.to_ascii_lowercase();
    for &r in SEEDANCE_ASPECT_RATIOS {
        if lower == r.to_ascii_lowercase() {
            return r.to_string();
        }
    }
    // Common aliases
    match lower.as_str() {
        "landscape" | "widescreen" | "wide" => "16:9".into(),
        "portrait" | "vertical" | "mobile" => "9:16".into(),
        "square" => "1:1".into(),
        "ultrawide" | "cinema" => "21:9".into(),
        // Seedream-only ratios we do not expose for video → fall back.
        "2:3" | "3:2" | "4:5" | "5:4" | "9:21" | "auto" => DEFAULT_ASPECT_RATIO.into(),
        _ => DEFAULT_ASPECT_RATIO.into(),
    }
}

/// Prompt clause describing canvas orientation for cover / video prompts.
pub fn aspect_prompt_clause(ratio: &str) -> String {
    let r = normalize_aspect_ratio(ratio);
    match r.as_str() {
        "9:16" => "Tall 9:16 vertical poster frame".into(),
        "1:1" => "Square 1:1 frame".into(),
        "4:3" => "Classic 4:3 landscape frame".into(),
        "3:4" => "Classic 3:4 portrait frame".into(),
        "21:9" => "Ultrawide 21:9 cinematic frame".into(),
        _ => "Wide 16:9 cinematic frame".into(),
    }
}

/// DashScope-style `W*H` size (~Seedream 2K-class posters).
pub fn aspect_to_dashscope_size(ratio: &str) -> &'static str {
    match normalize_aspect_ratio(ratio).as_str() {
        "9:16" => "1584*2816",
        "1:1" => "2048*2048",
        "4:3" => "2368*1776",
        "3:4" => "1776*2368",
        "21:9" => "3136*1344",
        _ => "2816*1584",
    }
}

/// Seedream 5.x OpenAI-style `size` for **film cover only**.
///
/// Uses official 2K preset pixel sizes (also satisfy the ≥3_686_400 floor seen
/// on some channels). Unsupported ratios already collapsed to 16:9 upstream.
pub fn aspect_to_seedream_size(ratio: &str) -> &'static str {
    match normalize_aspect_ratio(ratio).as_str() {
        "9:16" => "1584x2816", // official 2K
        "1:1" => "2048x2048",
        "4:3" => "2368x1776",
        "3:4" => "1776x2368",
        "21:9" => "3136x1344",
        _ => "2816x1584", // 16:9 official 2K
    }
}

/// Max upload/resize dimensions (longer side ≤ 1280) matching the aspect.
pub fn aspect_to_upload_dims(ratio: &str) -> (u32, u32) {
    match normalize_aspect_ratio(ratio).as_str() {
        "9:16" => (720, 1280),
        "1:1" => (1024, 1024),
        "4:3" => (1280, 960),
        "3:4" => (960, 1280),
        "21:9" => (1280, 549),
        _ => (1280, 720),
    }
}

/// JSON `extra` for cover generation — Seedream `size` + DashScope `parameters.size`.
/// Portraits / world plates must NOT call this (they keep default `2K`).
pub fn image_request_extra_for_aspect(ratio: &str) -> serde_json::Value {
    let r = normalize_aspect_ratio(ratio);
    serde_json::json!({
        "size": aspect_to_seedream_size(&r),
        "parameters": {
            "prompt_extend": false,
            "watermark": false,
            "size": aspect_to_dashscope_size(&r)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_seedance_ratios_and_aliases() {
        assert_eq!(normalize_aspect_ratio("9:16"), "9:16");
        assert_eq!(normalize_aspect_ratio(" 1:1 "), "1:1");
        assert_eq!(normalize_aspect_ratio("portrait"), "9:16");
        assert_eq!(normalize_aspect_ratio("nope"), "16:9");
        assert_eq!(normalize_aspect_ratio("16：9"), "16:9");
        assert_eq!(normalize_aspect_ratio("2:3"), "16:9");
        assert_eq!(aspect_parts("16:9"), (16, 9));
        assert_eq!(aspect_parts("portrait"), (9, 16));
    }

    #[test]
    fn size_maps_meet_seedream_min_pixels() {
        const MIN_PX: u32 = 3_686_400;
        for ratio in SEEDANCE_ASPECT_RATIOS {
            let size = aspect_to_seedream_size(ratio);
            let (w, h) = size
                .split_once('x')
                .map(|(a, b)| (a.parse::<u32>().unwrap(), b.parse::<u32>().unwrap()))
                .expect("WxH");
            assert!(
                w.saturating_mul(h) >= MIN_PX,
                "{ratio} → {size} has only {} px",
                w * h
            );
        }
        assert_eq!(aspect_to_seedream_size("16:9"), "2816x1584");
        assert_eq!(aspect_to_dashscope_size("9:16"), "1584*2816");
        assert_eq!(aspect_to_upload_dims("16:9"), (1280, 720));
    }
}
