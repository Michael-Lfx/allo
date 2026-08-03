//! Seedance 2.0 aspect ratios — shared by video create, cover art, and UI.

/// Ratios accepted by Seedance 2.0 / 2.0-fast (`ratio` API field).
pub const SEEDANCE_ASPECT_RATIOS: &[&str] =
    &["16:9", "9:16", "1:1", "4:3", "3:4", "21:9"];

pub const DEFAULT_ASPECT_RATIO: &str = "16:9";

/// Normalize user/config aspect to a Seedance-supported value (default 16:9).
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
        _ => DEFAULT_ASPECT_RATIO.into(),
    }
}

/// Prompt clause describing canvas orientation for image/video models.
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

/// DashScope-style `W*H` size string (~720p tier).
pub fn aspect_to_dashscope_size(ratio: &str) -> &'static str {
    match normalize_aspect_ratio(ratio).as_str() {
        "9:16" => "720*1280",
        "1:1" => "1024*1024",
        "4:3" => "1280*960",
        "3:4" => "960*1280",
        "21:9" => "1470*630",
        _ => "1280*720",
    }
}

/// Seedream / OpenAI-style size (prefer explicit pixels over generic "2K").
pub fn aspect_to_seedream_size(ratio: &str) -> &'static str {
    match normalize_aspect_ratio(ratio).as_str() {
        "9:16" => "720x1280",
        "1:1" => "1024x1024",
        "4:3" => "1280x960",
        "3:4" => "960x1280",
        "21:9" => "1470x630",
        _ => "1280x720",
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

/// JSON `extra` blob for [`nomifun_cloud::ImageGenerationRequest`] so both
/// Seedream (`size`) and DashScope (`parameters.size`) honor the ratio.
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
    }

    #[test]
    fn size_maps_are_consistent() {
        assert_eq!(aspect_to_dashscope_size("9:16"), "720*1280");
        assert_eq!(aspect_to_seedream_size("1:1"), "1024x1024");
        assert_eq!(aspect_to_upload_dims("16:9"), (1280, 720));
    }
}
