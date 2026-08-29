//! Seedance input-image privacy repair.
//!
//! Provides two strategies for handling human face images that fail video model content moderation:
//!
//! 1. **Traditional img2img**: Run mild cinematic img2img to fictionalize faces (Soft/Strong tiers)
//! 2. **AI Sanitization (recommended)**: Use vision description + text-to-image to generate completely
//!    new AI images that maintain character essence without photographic fingerprints.
//!
//! AI sanitization is preferred because:
//! - Vision model provides detailed character descriptions
//! - T2I generates images with no real-person fingerprints
//! - More reliable for passing video model content moderation
//!
//! Content layout matches [`nomifun_cloud::VideoCreateParams::build_content_array`]:
//! `content[0]` = text prompt; `content[1..]` = images in submission order; then optional
//! reference video / audio.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backends::{ImageGenerateOpts, VimaxChat, VimaxImage};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local::{copy_image_file_atomic, is_usable_image_file};

use super::ai_face_sanitizer;

/// Forced positive look: cinematic fictional character, not a phone selfie / real-person still.
#[allow(dead_code)]
pub(crate) const FACE_PRIVACY_LOOK_CLAUSE: &str =
    "cinematic film look, fictional character, movie character, film grain, soft stylization";

/// Negative constraints (API field + baked into prompt — Seedream may ignore `negative_prompt`).
pub(crate) const FACE_PRIVACY_NEGATIVE: &str = "\
photorealistic photograph, real person, selfie, realistic skin texture, \
phone camera look, documentary snapshot, anime, cartoon, manga, comic, \
cel shading, 2d illustration, chibi, flat illustration";

/// Soft pass: denoise ~0.42 — keep rough look (age/gender/hair/wardrobe), fictionalize the face.
///
/// Do **not** say "preserve facial features" — that fights identity softening and leaves
/// Seedance-detectable real-person fingerprints.
pub(crate) const FACE_PRIVACY_SOFT_PROMPT: &str = "\
Mild img2img edit of this reference into a fictional movie character. \
Keep age band, gender presentation, hair silhouette, wardrobe, pose, framing, set, and lighting. \
Rewrite the face as a cinematic AI-generated film character: soften and replace photographic \
identity cues (pores, selfie microtexture, exact likeness) so it is clearly not a real-person still. \
cinematic film look, fictional character, movie character, film grain, soft stylization. \
Stay live-action movie-still — do NOT convert to anime, cartoon, manga, comic, or flat illustration. \
Avoid: photorealistic photograph, real person, selfie, realistic skin texture. \
No text, watermark, or logo.";

/// Stronger pass (~0.48): more face fictionalization, still cinematic live-action (not cartoon).
pub(crate) const FACE_PRIVACY_STRONG_PROMPT: &str = "\
Stronger mild img2img edit of this reference into a fictional movie character. \
Keep hair silhouette, wardrobe, pose, framing, set, lighting, and rough character look. \
Aggressively fictionalize the face: erase real-person photographic fingerprints and exact \
likeness while outputting a cinematic live-action film face (AI / movie character, not a selfie). \
cinematic film look, fictional character, movie character, film grain, soft stylization. \
Do NOT turn the face into anime, cartoon, manga, comic, or flat illustration. \
Avoid: photorealistic photograph, real person, selfie, realistic skin texture. \
No text, watermark, or logo.";

/// Bump when denoise/prompt recipe changes so stale weak edits are re-run.
const MARKER_SOFT: &str = "soft_cinematic_v2";
const MARKER_STRONG: &str = "strong_cinematic_v2";

/// Soft ≈ 0.42, Strong ≈ 0.48 (recommended 0.42–0.48 band for Seedance privacy).
const DENOISE_SOFT: f32 = 0.42;
const DENOISE_STRONG: f32 = 0.48;

/// How aggressively to rewrite faces on a flagged reference image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivacyFaceTier {
    /// Denoise ~0.42: keep wardrobe/hair/pose; fictionalize face identity.
    Soft,
    /// Denoise ~0.48: stronger face fictionalization; still cinematic live-action.
    Strong,
}

impl PrivacyFaceTier {
    fn marker_tag(self) -> &'static str {
        match self {
            Self::Soft => MARKER_SOFT,
            Self::Strong => MARKER_STRONG,
        }
    }

    fn prompt(self) -> &'static str {
        match self {
            Self::Soft => FACE_PRIVACY_SOFT_PROMPT,
            Self::Strong => FACE_PRIVACY_STRONG_PROMPT,
        }
    }

    fn denoise(self) -> f32 {
        match self {
            Self::Soft => DENOISE_SOFT,
            Self::Strong => DENOISE_STRONG,
        }
    }

    fn image_opts(self) -> ImageGenerateOpts {
        ImageGenerateOpts {
            negative_prompt: Some(FACE_PRIVACY_NEGATIVE.to_string()),
            denoising_strength: Some(self.denoise()),
        }
    }

    /// Next escalation after this tier already ran for the same raw fingerprint.
    pub(crate) fn escalate(self) -> Option<Self> {
        match self {
            Self::Soft => Some(Self::Strong),
            Self::Strong => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivacyFaceOutcome {
    /// File already privacy-safe at the requested tier (or stronger).
    Unchanged,
    /// Faces were rewritten and the on-disk image was replaced.
    Rewritten,
}

/// Parse `content[N]` from a Seedance / Flowy privacy rejection message.
///
/// Returns the raw content-array index (0 = text). Tolerates typos/spacing in
/// gateway wraps (`content[2]`, `content [2]`, `'content[2]'`).
pub(crate) fn parse_seedance_flagged_content_index(err_text: &str) -> Option<usize> {
    let lower = err_text.to_ascii_lowercase();
    // Prefer the most specific pattern first.
    const NEEDLES: &[&str] = &["content[", "content [", "'content[", "\"content["];
    for needle in NEEDLES {
        let mut search = lower.as_str();
        while let Some(pos) = search.find(needle) {
            let after = &search[pos + needle.len()..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                // Require a closing `]` soon after digits (avoid accidental matches).
                let rest = &after[digits.len()..];
                if rest.starts_with(']') || rest.trim_start().starts_with(']') {
                    return digits.parse().ok();
                }
            }
            search = &search[pos + 1..];
        }
    }
    None
}

/// Map Seedance `content[idx]` → local `images[]` slot for multi-ref R2V.
///
/// `content[0]` is always the text prompt. Image slots are `1..=image_count`.
/// Indices that land on reference video/audio (after images) return `None`.
pub(crate) fn content_index_to_image_slot(
    content_idx: usize,
    image_count: usize,
) -> Option<usize> {
    if content_idx == 0 || image_count == 0 {
        return None;
    }
    let image_idx = content_idx - 1;
    if image_idx < image_count {
        Some(image_idx)
    } else {
        None
    }
}

/// True when upstream rejected an **input image** for real-person / privacy likeness.
pub(crate) fn is_seedance_privacy_image_err_text(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.contains("privacyinformation")
        || s.contains("inputimagesensitivecontent")
        || s.contains("may contain real person")
        || (s.contains("real person") && s.contains("sensitive"))
        || s.contains("含真人")
}

fn privacy_raw_path(path: &Path) -> PathBuf {
    path.with_extension("privacy_raw.png")
}

fn privacy_marker_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.seedance_privacy", path.display()))
}

fn file_fingerprint(path: &Path) -> VimaxResult<String> {
    let bytes = std::fs::read(path)?;
    let len = bytes.len();
    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    Ok(format!("{len}:{digest}"))
}

fn read_marker_tier(marker: &Path, raw_fp: &str) -> Option<PrivacyFaceTier> {
    let Ok(raw) = std::fs::read_to_string(marker) else {
        return None;
    };
    let s = raw.trim();
    let (fp, tag) = match s.split_once('|') {
        Some(parts) => parts,
        None => return None,
    };
    if fp != raw_fp {
        return None;
    }
    match tag {
        MARKER_SOFT => Some(PrivacyFaceTier::Soft),
        MARKER_STRONG => Some(PrivacyFaceTier::Strong),
        // Legacy cartoon/erase markers — force re-run with cinematic recipe.
        _ => None,
    }
}

fn tier_satisfies(applied: PrivacyFaceTier, wanted: PrivacyFaceTier) -> bool {
    match (applied, wanted) {
        (PrivacyFaceTier::Strong, _) => true,
        (PrivacyFaceTier::Soft, PrivacyFaceTier::Soft) => true,
        (PrivacyFaceTier::Soft, PrivacyFaceTier::Strong) => false,
    }
}

/// Ensure `path` has faces rewritten for Seedance privacy at least to `tier`.
///
/// Keeps a one-time `*.privacy_raw.png` backup of the pre-anonymize bytes and
/// overwrites `path` in place so later shots reuse the safe plate.
///
/// When `force` is true (Seedance just rejected this image), ignore soft markers and
/// re-run from `*.privacy_raw.png` — prior "privacy safe" state is not trusted.
pub(crate) async fn ensure_seedance_privacy_face(
    image: &dyn VimaxImage,
    path: &Path,
    tier: PrivacyFaceTier,
    force: bool,
) -> VimaxResult<PrivacyFaceOutcome> {
    if !is_usable_image_file(path) {
        return Err(VimaxError::msg(format!(
            "privacy face repair: image missing or unreadable: {}",
            path.display()
        )));
    }

    let raw = privacy_raw_path(path);
    if !is_usable_image_file(&raw) {
        // If path was already rewritten, we may have lost the original — still proceed
        // from current bytes as the new raw baseline.
        copy_image_file_atomic(path, &raw)?;
    }
    let raw_fp = file_fingerprint(&raw).unwrap_or_default();
    let marker = privacy_marker_path(path);
    if !force {
        if let Some(applied) = read_marker_tier(&marker, &raw_fp) {
            if tier_satisfies(applied, tier) && is_usable_image_file(path) {
                return Ok(PrivacyFaceOutcome::Unchanged);
            }
        }
    } else if let Some(applied) = read_marker_tier(&marker, &raw_fp) {
        // Forced by an active Seedance reject: Soft marker is not enough to skip Soft
        // again — only skip when Strong was already applied for this raw fingerprint.
        if tier == PrivacyFaceTier::Strong
            && tier_satisfies(applied, PrivacyFaceTier::Strong)
            && is_usable_image_file(path)
        {
            return Ok(PrivacyFaceOutcome::Unchanged);
        }
        if tier == PrivacyFaceTier::Soft
            && matches!(applied, PrivacyFaceTier::Strong)
            && is_usable_image_file(path)
        {
            return Ok(PrivacyFaceOutcome::Unchanged);
        }
    }

    let tmp = path.with_extension("privacy_tmp.png");
    let _ = std::fs::remove_file(&tmp);
    let opts = tier.image_opts();
    tracing::info!(
        path = %path.display(),
        tier = ?tier,
        force,
        denoise = opts.denoising_strength,
        "Seedance privacy: mild cinematic face fictionalization on flagged ref"
    );
    // Always edit from the raw backup so regenerate can re-detect subject identity.
    if let Err(err) = image
        .generate_with_opts(tier.prompt(), &[raw.as_path()], &tmp, opts)
        .await
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(VimaxError::msg(format!(
            "privacy face repair failed for {}: {err}",
            path.display()
        )));
    }
    if !is_usable_image_file(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(VimaxError::msg(format!(
            "privacy face repair produced no image for {}",
            path.display()
        )));
    }
    copy_image_file_atomic(&tmp, path)?;
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::write(
        &marker,
        format!("{}|{}", raw_fp, tier.marker_tag()).as_bytes(),
    );
    Ok(PrivacyFaceOutcome::Rewritten)
}

/// Paths that typically carry human faces in multi-ref R2V (cast / continuity).
/// Env/prop plates are lower priority and skipped in blind sweeps to save cost.
pub(crate) fn is_likely_face_bearing_ref(path: &Path) -> bool {
    let s = path.to_string_lossy().to_ascii_lowercase();
    s.contains("character_portrait")
        || s.contains("three_view")
        || s.contains("cameo")
        || s.contains("video_last_frame")
        || s.contains("first_frame")
        || s.contains("last_frame")
        || s.contains("portrait")
}

/// Cameo identity plates are privacy-handled at bind time; skip render preflight.
fn is_cameo_identity_plate(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with("_cameo.png") && !name.contains("atmosphere")
}

fn cameo_privacy_marker_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.privacy_safe", path.display()))
}

fn has_cameo_privacy_marker(path: &Path) -> bool {
    cameo_privacy_marker_path(path).is_file()
}

fn is_continuity_ref(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n == "video_last_frame.png")
}

fn has_seedance_privacy_at_least_soft(path: &Path) -> bool {
    let marker = privacy_marker_path(path);
    let raw = privacy_raw_path(path);
    let raw_fp = file_fingerprint(&raw)
        .or_else(|_| file_fingerprint(path))
        .unwrap_or_default();
    read_marker_tier(&marker, &raw_fp).is_some()
}

pub(crate) fn should_preflight_video_ref(path: &Path) -> bool {
    if is_cameo_identity_plate(path) {
        return false;
    }
    if has_cameo_privacy_marker(path) {
        return false;
    }
    if has_seedance_privacy_at_least_soft(path) {
        return false;
    }
    if is_continuity_ref(path) {
        return true;
    }
    is_likely_face_bearing_ref(path)
}

/// Soft-tier privacy pass on refs before the first video create (and per-shot refs).
pub(crate) async fn preflight_video_ref_privacy(
    image: &dyn VimaxImage,
    ref_paths: &[&Path],
) -> VimaxResult<()> {
    for path in ref_paths {
        if should_preflight_video_ref(path) && is_usable_image_file(path) {
            let _ = ensure_seedance_privacy_face(image, path, PrivacyFaceTier::Soft, false).await?;
        }
    }
    Ok(())
}

/// Resolve which ref slots to privacy-repair for this reject.
///
/// Prefer the Seedance `content[N]` hit; if missing/unmappable, sweep all face-bearing
/// refs (and fall back to every image if none match heuristics).
pub(crate) fn privacy_repair_targets(
    err_text: &str,
    ref_paths: &[(PathBuf, String)],
) -> Vec<usize> {
    if let Some(content_idx) = parse_seedance_flagged_content_index(err_text) {
        if let Some(slot) = content_index_to_image_slot(content_idx, ref_paths.len()) {
            return vec![slot];
        }
    }
    let face_slots: Vec<usize> = ref_paths
        .iter()
        .enumerate()
        .filter(|(_, (p, _))| is_likely_face_bearing_ref(p))
        .map(|(i, _)| i)
        .collect();
    if !face_slots.is_empty() {
        return face_slots;
    }
    (0..ref_paths.len()).collect()
}

/// Resolve the next privacy tier for a path already attempted in this shot.
pub(crate) fn next_privacy_tier_for_path(
    path: &Path,
    attempted: &[(PathBuf, PrivacyFaceTier)],
) -> Option<PrivacyFaceTier> {
    let last = attempted
        .iter()
        .rev()
        .find(|(p, _)| p == path)
        .map(|(_, t)| *t);
    match last {
        None => Some(PrivacyFaceTier::Soft),
        Some(t) => t.escalate(),
    }
}

// =============================================================================
// AI Sanitization Strategy (Recommended)
// =============================================================================

/// Marker for AI sanitized images (more aggressive than traditional img2img)
const MARKER_AI_SANITIZED: &str = "ai_sanitized_v1";

/// Ensure image passes video model content moderation using AI sanitization.
///
/// This is the recommended strategy for handling human face images in video generation:
/// 1. Uses vision model to detect faces and generate detailed descriptions
/// 2. Generates completely new AI image via T2I
/// 3. Result has no photographic fingerprints and maintains character essence
///
/// This approach is more reliable than img2img because:
/// - T2I generates from scratch, not modifying the original
/// - No residual photographic artifacts from the original
/// - Character identity preserved through detailed description
pub async fn ensure_ai_sanitized_privacy_face(
    image: Arc<dyn VimaxImage>,
    chat: Arc<dyn VimaxChat>,
    path: &Path,
    style: &str,
) -> VimaxResult<PrivacyFaceOutcome> {
    if !is_usable_image_file(path) {
        return Err(VimaxError::msg(format!(
            "AI privacy sanitizer: image missing or unreadable: {}",
            path.display()
        )));
    }

    // Check if already sanitized (cache by file fingerprint)
    let raw = ai_sanitized_raw_path(path);
    if !is_usable_image_file(&raw) {
        copy_image_file_atomic(path, &raw)?;
    }
    let raw_fp = file_fingerprint(&raw).unwrap_or_default();
    let marker = ai_sanitized_marker_path(path);

    if let Some(cached) = read_ai_sanitized_marker(&marker, &raw_fp) {
        if cached && is_usable_image_file(path) {
            tracing::info!(
                path = %path.display(),
                "AI privacy sanitizer: using cached result"
            );
            return Ok(PrivacyFaceOutcome::Unchanged);
        }
    }

    // Step 1: Check if image has human face
    let has_face = match ai_face_sanitizer::detect_human_face(Arc::clone(&chat), path).await {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "AI privacy sanitizer: face detection failed, keeping original"
            );
            return Ok(PrivacyFaceOutcome::Unchanged);
        }
    };

    if !has_face {
        tracing::info!(
            path = %path.display(),
            "AI privacy sanitizer: no human face detected, keeping original"
        );
        let _ = std::fs::write(&marker, format!("{}|no_face", raw_fp));
        return Ok(PrivacyFaceOutcome::Unchanged);
    }

    // Step 2: Generate AI sanitized version
    tracing::info!(
        path = %path.display(),
        "AI privacy sanitizer: generating sanitized version"
    );

    let (_result_path, outcome) = ai_face_sanitizer::ai_sanitize_face_image(
        Arc::clone(&image),
        Arc::clone(&chat),
        path,
        path, // Overwrite in place
        style,
    )
    .await?;

    match outcome {
        ai_face_sanitizer::AiSanitizerOutcome::Generated
        | ai_face_sanitizer::AiSanitizerOutcome::Cached => {
            let _ = std::fs::write(&marker, format!("{}|{}", raw_fp, MARKER_AI_SANITIZED));
            Ok(PrivacyFaceOutcome::Rewritten)
        }
        ai_face_sanitizer::AiSanitizerOutcome::NoFace => {
            // Inconsistent detection, keep original
            let _ = std::fs::write(&marker, format!("{}|no_face", raw_fp));
            Ok(PrivacyFaceOutcome::Unchanged)
        }
    }
}

/// Sanitize multiple images in batch (for privacy repair targets)
#[allow(dead_code)]
pub async fn batch_ai_sanitize_privacy_faces(
    image: Arc<dyn VimaxImage>,
    chat: Arc<dyn VimaxChat>,
    paths: &[PathBuf],
    style: &str,
    progress_callback: impl Fn(usize, usize) + Send + 'static,
) -> Vec<VimaxResult<PrivacyFaceOutcome>> {
    let total = paths.len();
    let mut results = Vec::with_capacity(total);

    for (idx, path) in paths.iter().enumerate() {
        let result = ensure_ai_sanitized_privacy_face(
            Arc::clone(&image),
            Arc::clone(&chat),
            path,
            style,
        )
        .await;
        progress_callback(idx + 1, total);
        results.push(result);
    }

    results
}

// =============================================================================
// Utility Functions
// =============================================================================

fn ai_sanitized_raw_path(path: &Path) -> PathBuf {
    path.with_extension("ai_sanitized_raw.png")
}

fn ai_sanitized_marker_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.ai_privacy_safe", path.display()))
}

fn read_ai_sanitized_marker(marker: &Path, raw_fp: &str) -> Option<bool> {
    let Ok(raw) = std::fs::read_to_string(marker) else {
        return None;
    };
    let s = raw.trim();
    if !s.starts_with(raw_fp) {
        return None;
    }
    if s.contains(MARKER_AI_SANITIZED) {
        Some(true)
    } else if s.contains("no_face") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_index_from_real_gateway_message() {
        let msg = "API error 48: video generation request was rejected: seedance upstream status 400: \
InputImageSensitiveContentDetected.PrivacyInformation (The request failed because the input image \
'content[2]' may contain real person, Request id:0217877262591536845cd2f7c61da91e37e7e70760c22da5ef2f1)";
        assert_eq!(parse_seedance_flagged_content_index(msg), Some(2));
    }

    #[test]
    fn parses_content_index_with_spacing_and_typos() {
        assert_eq!(
            parse_seedance_flagged_content_index("input image content [3] may contain real person"),
            Some(3)
        );
        assert_eq!(
            parse_seedance_flagged_content_index("flagged content[0] text"),
            Some(0)
        );
        assert_eq!(parse_seedance_flagged_content_index("no index here"), None);
    }

    #[test]
    fn content_index_maps_to_image_slot() {
        // content[0]=text, content[1]=img0, content[2]=img1
        assert_eq!(content_index_to_image_slot(0, 3), None);
        assert_eq!(content_index_to_image_slot(1, 3), Some(0));
        assert_eq!(content_index_to_image_slot(2, 3), Some(1));
        assert_eq!(content_index_to_image_slot(3, 3), Some(2));
        // past images (would be audio/video)
        assert_eq!(content_index_to_image_slot(4, 3), None);
        assert_eq!(content_index_to_image_slot(1, 0), None);
    }

    #[test]
    fn privacy_image_err_detection() {
        assert!(is_seedance_privacy_image_err_text(
            "InputImageSensitiveContentDetected.PrivacyInformation content[2] real person"
        ));
        assert!(!is_seedance_privacy_image_err_text(
            "InputTextSensitiveContentDetected: prompt violates policy"
        ));
    }

    #[test]
    fn tier_escalation() {
        assert_eq!(
            PrivacyFaceTier::Soft.escalate(),
            Some(PrivacyFaceTier::Strong)
        );
        assert_eq!(PrivacyFaceTier::Strong.escalate(), None);
        let path = PathBuf::from("shots/0/video_last_frame.png");
        let mut attempted = Vec::new();
        assert_eq!(
            next_privacy_tier_for_path(&path, &attempted),
            Some(PrivacyFaceTier::Soft)
        );
        attempted.push((path.clone(), PrivacyFaceTier::Soft));
        assert_eq!(
            next_privacy_tier_for_path(&path, &attempted),
            Some(PrivacyFaceTier::Strong)
        );
        attempted.push((path.clone(), PrivacyFaceTier::Strong));
        assert_eq!(next_privacy_tier_for_path(&path, &attempted), None);
    }

    #[test]
    fn privacy_raw_path_keeps_stem() {
        let p = PathBuf::from("character_portraits/0_hero/front.png");
        assert_eq!(
            privacy_raw_path(&p),
            PathBuf::from("character_portraits/0_hero/front.privacy_raw.png")
        );
    }

    #[test]
    fn repair_targets_prefer_content_index_then_face_sweep() {
        let refs = vec![
            (PathBuf::from("shots/0/video_last_frame.png"), "c".into()),
            (PathBuf::from("envs/dock.png"), "e".into()),
            (PathBuf::from("character_portraits/0_hero/front.png"), "p".into()),
        ];
        assert_eq!(
            privacy_repair_targets("content[1] may contain real person", &refs),
            vec![0]
        );
        assert_eq!(
            privacy_repair_targets("upstream status 400 without index", &refs),
            vec![0, 2]
        );
        assert!(is_likely_face_bearing_ref(Path::new(
            "character_portraits/x/cameo.png"
        )));
        assert!(!is_likely_face_bearing_ref(Path::new(
            "environments/0_码头/plate.png"
        )));
    }

    #[test]
    fn cinematic_prompts_keep_live_action_and_avoid_cartoon_needles() {
        for p in [FACE_PRIVACY_SOFT_PROMPT, FACE_PRIVACY_STRONG_PROMPT] {
            let lower = p.to_ascii_lowercase();
            assert!(lower.contains("cinematic film look"));
            assert!(lower.contains("fictional character"));
            assert!(lower.contains("film grain"));
            // Must not positively ask for illustration (trips style safety → cartoon).
            assert!(!lower.contains("illustrated"));
            // "Preserve facial features" fights fictionalization — must stay out.
            assert!(!lower.contains("preserve facial"));
            assert!(!lower.contains("preserve overall facial"));
            // "anime" may appear only as a negation ("do NOT … anime").
            assert!(
                !crate::planning::wants_stylized_non_photoreal(p),
                "privacy prompt must stay cinematic live-action: {p}"
            );
        }
        assert!((0.42..=0.48).contains(&PrivacyFaceTier::Soft.denoise()));
        assert!((0.42..=0.48).contains(&PrivacyFaceTier::Strong.denoise()));
        assert!(PrivacyFaceTier::Soft.denoise() < PrivacyFaceTier::Strong.denoise());
        assert!(FACE_PRIVACY_LOOK_CLAUSE.contains("movie character"));
        let neg = FACE_PRIVACY_NEGATIVE.to_ascii_lowercase();
        assert!(neg.contains("photorealistic photograph"));
        assert!(neg.contains("real person"));
        assert!(neg.contains("selfie"));
        assert!(neg.contains("cartoon"));
    }

    #[test]
    fn legacy_marker_tags_do_not_satisfy_new_recipe() {
        // Simulate old soft/strong tags → re-run required.
        assert!(matches!(
            {
                let tag = "soft";
                match tag {
                    MARKER_SOFT => Some(PrivacyFaceTier::Soft),
                    MARKER_STRONG => Some(PrivacyFaceTier::Strong),
                    _ => None,
                }
            },
            None
        ));
    }

    #[test]
    fn preflight_skips_cameo_identity_plate() {
        let dir = tempfile::tempdir().unwrap();
        let plate = dir.path().join("Alice_cameo.png");
        std::fs::write(&plate, b"png").unwrap();
        assert!(!should_preflight_video_ref(&plate));
    }

    #[test]
    fn preflight_includes_continuity_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("video_last_frame.png");
        std::fs::write(&frame, b"png").unwrap();
        assert!(should_preflight_video_ref(&frame));
    }

    fn write_test_png(path: &Path) {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_raw(2, 2, vec![255; 12]).expect("buffer");
        img.save(path).expect("save png");
    }

    #[tokio::test]
    async fn preflight_calls_image_for_bare_continuity() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        use async_trait::async_trait;

        use crate::backends::{ImageGenerateOpts, VimaxImage};
        use crate::error::VimaxResult;

        struct CountingImage(Arc<AtomicUsize>);

        #[async_trait]
        impl VimaxImage for CountingImage {
            async fn generate(
                &self,
                _prompt: &str,
                ref_image_paths: &[&Path],
                out_path: &Path,
            ) -> VimaxResult<()> {
                self.0.fetch_add(1, Ordering::SeqCst);
                if let Some(src) = ref_image_paths.first() {
                    std::fs::copy(src, out_path)?;
                } else {
                    write_test_png(out_path);
                }
                Ok(())
            }

            async fn generate_with_opts(
                &self,
                _prompt: &str,
                ref_image_paths: &[&Path],
                out_path: &Path,
                _opts: ImageGenerateOpts,
            ) -> VimaxResult<()> {
                self.generate("", ref_image_paths, out_path).await
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("video_last_frame.png");
        write_test_png(&frame);
        let calls = Arc::new(AtomicUsize::new(0));
        let image = CountingImage(Arc::clone(&calls));
        preflight_video_ref_privacy(&image, &[frame.as_path()])
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn preflight_skips_when_cameo_marker_present() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        use async_trait::async_trait;

        use crate::backends::{ImageGenerateOpts, VimaxImage};
        use crate::error::VimaxResult;

        struct CountingImage(Arc<AtomicUsize>);

        #[async_trait]
        impl VimaxImage for CountingImage {
            async fn generate(
                &self,
                _prompt: &str,
                ref_image_paths: &[&Path],
                out_path: &Path,
            ) -> VimaxResult<()> {
                self.0.fetch_add(1, Ordering::SeqCst);
                if let Some(src) = ref_image_paths.first() {
                    std::fs::copy(src, out_path)?;
                } else {
                    write_test_png(out_path);
                }
                Ok(())
            }

            async fn generate_with_opts(
                &self,
                _prompt: &str,
                ref_image_paths: &[&Path],
                out_path: &Path,
                _opts: ImageGenerateOpts,
            ) -> VimaxResult<()> {
                self.generate("", ref_image_paths, out_path).await
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let plate = dir.path().join("hero_cameo.png");
        write_test_png(&plate);
        std::fs::write(super::cameo_privacy_marker_path(&plate), b"fp").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let image = CountingImage(Arc::clone(&calls));
        preflight_video_ref_privacy(&image, &[plate.as_path()])
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
