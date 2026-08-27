//! AI Face Sanitizer — replace real human faces with AI-generated faces via
//! vision description + pure text-to-image (NO reference image).
//!
//! This approach generates completely NEW images using ONLY text descriptions,
//! avoiding the problems of img2img:
//! 1. No photographic fingerprints from the original image
//! 2. No style contamination from reference images (cartoon/reflection won't affect output)
//! 3. Generated images are truly AI-created and pass video model content moderation
//!
//! The flow is:
//! 1. Vision model describes the real image content in detail
//! 2. Pure T2I generates a completely new image from text only (no reference image)
//! 3. The generated image has no connection to the original photograph

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::backends::{ImageGenerateOpts, VimaxChat, VimaxImage};
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::complete_vision_and_parse_llm_json;
use crate::media_local::{copy_image_file_atomic, is_usable_image_file};

/// Marker file suffix for AI sanitized images
const AI_SANITIZED_MARKER: &str = "ai_sanitized";

/// Marker file suffix for vision description cache
const DESCRIPTION_MARKER: &str = "vision_desc";

/// Vision timeout — if multimodal model is unavailable, fall back gracefully.
const VISION_TIMEOUT_SECS: u64 = 120;

/// System prompt for detailed image description (for face sanitization)
const DETAILED_DESCRIPTION_SYSTEM: &str = r#"You are an expert visual analyst specializing in human character description for AI image generation.

Your task is to provide extremely detailed descriptions of human characters in images, which will be used to generate new AI images that maintain character identity and visual style.

Focus on these aspects:
1. **Facial Features**: Precise description of face shape, eye shape/color, nose shape, lip shape, chin, jawline, eyebrows, any distinctive features (scars, moles, freckles, wrinkles)
2. **Age & Gender**: Apparent age range, gender presentation, ethnicity/skin tone
3. **Hair**: Style, color, length, texture, any accessories (glasses, hats, earrings)
4. **Expression & Emotion**: Current expression, emotional tone, gaze direction
5. **Pose & Body**: Body position, gestures, posture, movement tendency
6. **Wardrobe**: Clothing items, colors, patterns, accessories, shoes
7. **Lighting & Style**: Lighting mood, color grading, visual style
8. **Setting/Background**: Environment, props, context

Provide descriptions that are detailed enough for an AI to generate a visually similar character while clearly being a different person (different face, but same style/mood).

IMPORTANT: Describe the person as if you need to recreate a similar-looking character in an AI-generated image."#;

/// User prompt template for detailed description
fn detailed_description_user(has_face: bool) -> String {
    if has_face {
        r#"Analyze this image and provide a detailed description of the human character(s) for AI image generation.

If the image contains real human faces, describe them in extreme detail to enable recreating a similar-looking AI-generated character.

Reply JSON only with this exact schema:
{
    "description": "Comprehensive description in English covering all aspects (facial features, hair, body, pose, clothing, lighting, background, style). Be extremely detailed for facial features.",
    "key_features": ["specific distinguishing feature 1", "specific feature 2", "..."],
    "style_keywords": ["style", "mood", "color", "keywords"],
    "prompt_for_generation": "Ready-to-use text prompt for AI image generation that captures this character's essence"
}"#.to_string()
    } else {
        r#"This image does not appear to contain real human faces. 

Reply JSON only with this exact schema:
{
    "description": "Brief description of the image content",
    "key_features": [],
    "style_keywords": ["style", "mood", "color", "keywords"],
    "prompt_for_generation": "Text prompt for AI image generation"
}"#.to_string()
    }
}

/// Response structure for vision description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionDescription {
    pub description: String,
    #[serde(default)]
    pub key_features: Vec<String>,
    #[serde(default)]
    pub style_keywords: Vec<String>,
    #[serde(default)]
    pub prompt_for_generation: String,
}

/// Result of AI face sanitization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiSanitizerOutcome {
    /// Image already sanitized and cached
    Cached,
    /// New AI face image was generated
    Generated,
    /// No human face detected, original kept
    NoFace,
}

/// Check if an image contains real human face(s) using vision model
pub async fn detect_human_face(
    chat: Arc<dyn VimaxChat>,
    image_path: &Path,
) -> VimaxResult<bool> {
    let system = "You are a strict gate for human face detection. Answer accurately.";
    let user = r#"Does this image contain at least one real human face?

Answer JSON only:
{"has_real_human_face": true|false, "reason": "brief explanation"}

Return false for: animals, anime/cartoon faces without real human identity, toys, sculptures, empty scenes, landscapes, props-only shots."#;

    #[derive(Deserialize)]
    struct FaceDetectResp {
        #[serde(rename = "has_real_human_face")]
        has_real_human_face: bool,
    }

    let resp: FaceDetectResp = complete_vision_and_parse_llm_json(
        chat.as_ref(),
        system,
        user,
        &[image_path],
    )
    .await?;

    Ok(resp.has_real_human_face)
}

/// Get detailed vision description of an image
async fn get_vision_description(
    chat: Arc<dyn VimaxChat>,
    image_path: &Path,
    has_face: bool,
) -> VimaxResult<VisionDescription> {
    let user = detailed_description_user(has_face);

    tokio::time::timeout(
        std::time::Duration::from_secs(VISION_TIMEOUT_SECS),
        complete_vision_and_parse_llm_json::<VisionDescription>(
            chat.as_ref(),
            DETAILED_DESCRIPTION_SYSTEM,
            &user,
            &[image_path],
        ),
    )
    .await
    .map_err(|_| {
        VimaxError::msg(format!(
            "vision description timed out after {}s",
            VISION_TIMEOUT_SECS
        ))
    })?
}

/// Build prompt for PURE T2I (no reference image) that ensures realistic film style
///
/// Key improvements over img2img approach:
/// 1. Explicitly requests photorealistic/cinematic style (not cartoon)
/// 2. Describes exact visual features to maintain character identity
/// 3. No reference image means no style contamination
pub(crate) fn build_pure_t2i_prompt(description: &VisionDescription, style: &str) -> String {
    let mut parts = Vec::new();

    // Part 1: Core character description - be very detailed about appearance
    parts.push(description.description.clone());

    // Part 2: Explicit realism/cinematic constraints
    parts.push(
        "PHOTOREALISTIC cinematic film still, high-end movie production quality, \
        professional cinematography, film grain, shallow depth of field, \
        cinematic lighting, warm color grading, anamorphic lens look, \
        35mm film aesthetic, movie screenshot".to_string(),
    );

    // Part 3: Negative constraints - AVOID non-realistic styles
    parts.push(
        "AVOID: anime, cartoon, illustration, painting, drawing, sketch, manga, \
        comic, 2D, 3D render, CGI, digital art, watermark, text, logo, \
        blurry, low quality, amateur photo, selfie, phone camera look".to_string(),
    );

    // Part 4: Style keywords if provided
    if !description.style_keywords.is_empty() {
        parts.push(format!(
            "Style direction: {}",
            description.style_keywords.join(", ")
        ));
    }

    // Part 5: Explicit style guidance
    if !style.is_empty() {
        parts.push(format!("Requested style: {style}"));
    }

    // Part 6: Final quality requirements
    parts.push(
        "High detail, sharp focus, professional color grading, \
        cinematic atmosphere, film production quality".to_string(),
    );

    parts.join(". ")
}

/// Generate AI-sanitized version of an image using vision description + T2I
///
/// Returns the path to the generated AI image, or the original path if no face detected
pub async fn ai_sanitize_face_image(
    image: Arc<dyn VimaxImage>,
    chat: Arc<dyn VimaxChat>,
    source_path: &Path,
    output_path: &Path,
    style: &str,
) -> VimaxResult<(PathBuf, AiSanitizerOutcome)> {
    // Check if already sanitized
    let marker = ai_sanitized_marker_path(source_path);
    let raw_fp = file_fingerprint(source_path)?;

    if let Some(cached) = read_ai_sanitizer_marker(&marker, &raw_fp) {
        if is_usable_image_file(output_path) {
            tracing::info!(
                path = %source_path.display(),
                "AI face sanitizer: using cached result"
            );
            return Ok((output_path.to_path_buf(), cached));
        }
    }

    // Step 1: Detect if image has human face
    let has_face = match detect_human_face(Arc::clone(&chat), source_path).await {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(
                path = %source_path.display(),
                error = %err,
                "AI face sanitizer: face detection failed, keeping original"
            );
            return Ok((source_path.to_path_buf(), AiSanitizerOutcome::NoFace));
        }
    };

    if !has_face {
        tracing::info!(
            path = %source_path.display(),
            "AI face sanitizer: no human face detected, keeping original"
        );
        // Mark that we checked and found no face
        let _ = std::fs::write(&marker, format!("{}|no_face", raw_fp));
        return Ok((source_path.to_path_buf(), AiSanitizerOutcome::NoFace));
    }

    // Step 2: Get detailed vision description
    let vision_desc = match get_vision_description(Arc::clone(&chat), source_path, true).await {
        Ok(desc) => desc,
        Err(err) => {
            tracing::warn!(
                path = %source_path.display(),
                error = %err,
                "AI face sanitizer: vision description failed, falling back to original"
            );
            return Ok((source_path.to_path_buf(), AiSanitizerOutcome::NoFace));
        }
    };

    // Cache vision description for debugging/audit
    let desc_marker = description_marker_path(source_path);
    let _ = std::fs::write(
        &desc_marker,
        serde_json::to_string_pretty(&vision_desc).unwrap_or_default(),
    );

    tracing::info!(
        path = %source_path.display(),
        description_length = vision_desc.description.len(),
        key_features = vision_desc.key_features.len(),
        "AI face sanitizer: generated vision description"
    );

    // Step 3: Generate AI face image using PURE T2I (no reference image)
    // This ensures:
    // 1. No photographic fingerprints from the original
    // 2. No style contamination (cartoon/real won't affect output)
    // 3. Generated image is truly AI-created and passes video model moderation
    let prompt = build_pure_t2i_prompt(&vision_desc, style);

    let tmp = output_path.with_extension("ai_sanitize_tmp.png");
    let _ = std::fs::remove_file(&tmp);

    let opts = ImageGenerateOpts {
        negative_prompt: Some(
            "photorealistic photograph, real person, selfie, realistic skin texture, \
            phone camera look, documentary snapshot, anime, cartoon, manga, comic, \
            cel shading, 2d illustration, chibi, flat illustration, watermark, text, logo"
                .to_string(),
        ),
        denoising_strength: None, // Not used for pure T2I
        ..Default::default()
    };

    // PURE T2I: No reference images - generate from text only
    if let Err(err) = image
        .generate_with_opts(&prompt, &[], &tmp, opts)
        .await
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(VimaxError::msg(format!(
            "AI face sanitization failed for {}: {err}",
            source_path.display()
        )));
    }

    if !is_usable_image_file(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(VimaxError::msg(format!(
            "AI face sanitization produced no image for {}",
            source_path.display()
        )));
    }

    // Step 4: Save sanitized image
    copy_image_file_atomic(&tmp, output_path)?;
    let _ = std::fs::remove_file(&tmp);

    // Step 5: Write marker
    let _ = std::fs::write(&marker, format!("{}|ai_generated", raw_fp));

    tracing::info!(
        input = %source_path.display(),
        output = %output_path.display(),
        "AI face sanitizer: generated pure T2I sanitized image (no reference)"
    );

    Ok((output_path.to_path_buf(), AiSanitizerOutcome::Generated))
}

/// Ensure an image has AI-sanitized faces for video model compliance
///
/// This is the main entry point for the sanitization pipeline.
/// It checks cache, detects faces, generates vision description, and creates
/// a new AI image that will pass video model content moderation.
pub async fn ensure_ai_sanitized_face(
    image: Arc<dyn VimaxImage>,
    chat: Arc<dyn VimaxChat>,
    source_path: &Path,
    output_path: &Path,
    style: &str,
) -> VimaxResult<AiSanitizerOutcome> {
    if !is_usable_image_file(source_path) {
        return Err(VimaxError::msg(format!(
            "AI face sanitizer: source image missing or unreadable: {}",
            source_path.display()
        )));
    }

    let (result_path, outcome) =
        ai_sanitize_face_image(image, chat, source_path, output_path, style).await?;

    // If outcome is Generated, result_path should be output_path
    // If outcome is NoFace or Cached, result_path is the original or cached
    match outcome {
        AiSanitizerOutcome::Generated => {
            if result_path != output_path {
                return Err(VimaxError::msg(String::from(
                    "AI face sanitizer: generated path mismatch",
                )));
            }
        }
        AiSanitizerOutcome::Cached => {
            if result_path != output_path {
                // Copy cached result to expected output path
                copy_image_file_atomic(&result_path, output_path)?;
            }
        }
        AiSanitizerOutcome::NoFace => {
            // No face: use original
            if source_path != output_path {
                copy_image_file_atomic(source_path, output_path)?;
            }
        }
    }

    Ok(outcome)
}

// ============================================================================
// Utility functions
// ============================================================================

fn ai_sanitized_marker_path(source: &Path) -> PathBuf {
    PathBuf::from(format!("{}.{}", source.display(), AI_SANITIZED_MARKER))
}

fn description_marker_path(source: &Path) -> PathBuf {
    PathBuf::from(format!("{}.{}", source.display(), DESCRIPTION_MARKER))
}

fn read_ai_sanitizer_marker(marker: &Path, raw_fp: &str) -> Option<AiSanitizerOutcome> {
    let Ok(raw) = std::fs::read_to_string(marker) else {
        return None;
    };
    let s = raw.trim();
    if !s.starts_with(raw_fp) {
        return None;
    }
    if s.contains("ai_generated") {
        Some(AiSanitizerOutcome::Generated)
    } else if s.contains("no_face") {
        Some(AiSanitizerOutcome::NoFace)
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sanitization_prompt() {
        let desc = VisionDescription {
            description: "A middle-aged Asian man with short black hair, wearing a blue jacket".to_string(),
            key_features: vec!["Asian male".into(), "short black hair".into()],
            style_keywords: vec!["cinematic".into(), "warm lighting".into()],
            prompt_for_generation: "middle-aged man portrait".to_string(),
        };

        let prompt = build_pure_t2i_prompt(&desc, "film noir");
        assert!(prompt.contains("A middle-aged Asian man"));
        assert!(prompt.contains("cinematic"));
        assert!(prompt.contains("film noir"));
        assert!(prompt.contains("PHOTOREALISTIC"));
        assert!(prompt.contains("AVOID"));
    }

    #[test]
    fn marker_path_construction() {
        let source = PathBuf::from("character_portraits/0_hero/front.png");
        let marker = ai_sanitized_marker_path(&source);
        assert!(marker.to_string_lossy().ends_with(".ai_sanitized"));
        assert!(marker.to_string_lossy().contains("front.png"));

        let desc_marker = description_marker_path(&source);
        assert!(desc_marker.to_string_lossy().ends_with(".vision_desc"));
    }

    #[test]
    fn sanitization_prompt_includes_negative_constraints() {
        let desc = VisionDescription {
            description: "Test description".to_string(),
            key_features: vec![],
            style_keywords: vec![],
            prompt_for_generation: "test".to_string(),
        };

        let prompt = build_pure_t2i_prompt(&desc, "");
        assert!(prompt.contains("PHOTOREALISTIC"));
        assert!(prompt.contains("cinematic"));
        assert!(prompt.contains("selfie"));
        assert!(prompt.contains("AVOID"));
        assert!(prompt.contains("anime"));
        assert!(prompt.contains("cartoon"));
    }
}
