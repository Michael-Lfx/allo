//! Multimodal classification of user-uploaded session reference photos.
//!
//! Agent-mode uploads all land in `cameo/`; this agent decides which are cast
//! identity plates vs environment / prop / style refs before planning assets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::backends::VimaxChat;
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::complete_vision_and_parse_llm_json;
use crate::media_local::is_usable_image_file;
use crate::session::cameo::{self, CameoPhotoEntry};

use super::formats::REF_IMAGE_CLASSIFY;

pub const CLASSIFICATION_CACHE_REL: &str = "references/reference_classification.json";
const LEGACY_CLASSIFICATION_CACHE_REL: &str = "cameo/reference_classification.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceImageCategory {
    Character,
    Environment,
    Prop,
    Style,
}

impl ReferenceImageCategory {
    pub fn is_character(self) -> bool {
        matches!(self, Self::Character)
    }

    pub fn is_world_ref(self) -> bool {
        !self.is_character()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Environment => "environment",
            Self::Prop => "prop",
            Self::Style => "style",
        }
    }
}

impl Default for ReferenceImageCategory {
    fn default() -> Self {
        Self::Character
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceImageClassification {
    pub photo_id: String,
    #[serde(default)]
    pub sha256: String,
    pub category: ReferenceImageCategory,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub suggested_label: String,
    /// True when category came from name heuristics after vision failure / miss.
    #[serde(default)]
    pub heuristic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReferenceClassificationReport {
    #[serde(default)]
    pub classifications: Vec<ReferenceImageClassification>,
}

impl ReferenceClassificationReport {
    pub fn by_id(&self) -> HashMap<String, &ReferenceImageClassification> {
        self.classifications
            .iter()
            .map(|c| (c.photo_id.clone(), c))
            .collect()
    }

    pub fn category_for(&self, photo_id: &str) -> ReferenceImageCategory {
        self.classifications
            .iter()
            .find(|c| c.photo_id == photo_id)
            .map(|c| c.category)
            .unwrap_or(ReferenceImageCategory::Character)
    }

    pub fn character_photos<'a>(
        &'a self,
        photos: &'a [CameoPhotoEntry],
    ) -> Vec<&'a CameoPhotoEntry> {
        photos
            .iter()
            .filter(|p| self.category_for(&p.id).is_character())
            .collect()
    }

    pub fn world_photos<'a>(
        &'a self,
        photos: &'a [CameoPhotoEntry],
    ) -> Vec<(&'a CameoPhotoEntry, &'a ReferenceImageClassification)> {
        let map = self.by_id();
        photos
            .iter()
            .filter_map(|p| {
                let c = map.get(&p.id)?;
                if c.category.is_world_ref() {
                    Some((p, *c))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ClassificationCacheFile {
    #[serde(default)]
    pub classifications: Vec<ReferenceImageClassification>,
}

pub struct ReferenceImageClassifier {
    chat: Arc<dyn VimaxChat>,
}

impl ReferenceImageClassifier {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self { chat }
    }

    /// Classify session uploads; reuse cache entries whose sha256 still matches.
    pub async fn classify_session(
        &self,
        session_root: &Path,
    ) -> VimaxResult<ReferenceClassificationReport> {
        let photos = cameo::list_photos(session_root)?;
        if photos.is_empty() {
            let empty = ReferenceClassificationReport::default();
            let _ = save_cache(session_root, &empty);
            return Ok(empty);
        }

        let mut cached = load_cache(session_root);
        let mut by_id: HashMap<String, ReferenceImageClassification> = cached
            .classifications
            .drain(..)
            .map(|c| (c.photo_id.clone(), c))
            .collect();

        let mut need_vision: Vec<&CameoPhotoEntry> = Vec::new();
        let mut resolved: Vec<ReferenceImageClassification> = Vec::new();

        for photo in &photos {
            if let Some(hit) = by_id.remove(&photo.id) {
                if !hit.sha256.is_empty() && hit.sha256 == photo.sha256 {
                    resolved.push(hit);
                    continue;
                }
            }
            need_vision.push(photo);
        }

        if !need_vision.is_empty() {
            // Large film stills + multi-model retries can look "stuck" on classify_references.
            // Hard-cap wall time then fall back to name heuristics so planning continues.
            const CLASSIFY_VISION_TIMEOUT_SECS: u64 = 90;
            let vision = tokio::time::timeout(
                std::time::Duration::from_secs(CLASSIFY_VISION_TIMEOUT_SECS),
                self.classify_photos(session_root, &need_vision),
            )
            .await;
            match vision {
                Ok(Ok(mut fresh)) => resolved.append(&mut fresh),
                Ok(Err(err)) => {
                    tracing::warn!(
                        error = %err,
                        count = need_vision.len(),
                        "reference image vision classify failed; using name heuristics"
                    );
                    for photo in &need_vision {
                        resolved.push(heuristic_classification(photo));
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        count = need_vision.len(),
                        timeout_secs = CLASSIFY_VISION_TIMEOUT_SECS,
                        "reference image vision classify timed out; using name heuristics"
                    );
                    for photo in &need_vision {
                        resolved.push(heuristic_classification(photo));
                    }
                }
            }
        }

        // Stable order matching manifest photo order.
        let mut ordered = Vec::with_capacity(photos.len());
        for photo in &photos {
            if let Some(c) = resolved.iter().find(|c| c.photo_id == photo.id) {
                ordered.push(c.clone());
            } else {
                ordered.push(heuristic_classification(photo));
            }
        }

        let report = ReferenceClassificationReport {
            classifications: ordered,
        };
        save_cache(session_root, &report)?;
        // Expose human-readable copies under references/by_category for artifacts UI.
        let cats: Vec<(String, String, String)> = report
            .classifications
            .iter()
            .map(|c| {
                (
                    c.photo_id.clone(),
                    c.category.as_str().to_string(),
                    if c.suggested_label.trim().is_empty() {
                        c.summary.clone()
                    } else {
                        c.suggested_label.clone()
                    },
                )
            })
            .collect();
        if let Err(err) = cameo::materialize_by_category(session_root, &cats) {
            tracing::warn!(error = %err, "failed to materialize reference by_category copies");
        }
        Ok(report)
    }

    async fn classify_photos(
        &self,
        session_root: &Path,
        photos: &[&CameoPhotoEntry],
    ) -> VimaxResult<Vec<ReferenceImageClassification>> {
        let mut meta_lines = String::new();
        let mut image_paths: Vec<PathBuf> = Vec::new();
        let mut abs_refs: Vec<&Path> = Vec::new();

        for (idx, photo) in photos.iter().enumerate() {
            let abs = session_root.join(&photo.rel_path);
            if !is_usable_image_file(&abs) {
                return Err(VimaxError::InvalidParams(format!(
                    "reference photo unusable: {}",
                    photo.rel_path
                )));
            }
            meta_lines.push_str(&format!(
                "Image {idx}: photo_id={} label={:?} description={:?}\n",
                photo.id,
                photo.character_name.trim(),
                photo.description.trim()
            ));
            image_paths.push(abs);
        }
        for p in &image_paths {
            abs_refs.push(p.as_path());
        }

        let system = include_str!(
            "../../prompts/reference_image_classifier__system_prompt_template.txt"
        )
        .replace("{format_instructions}", REF_IMAGE_CLASSIFY);
        let user = include_str!(
            "../../prompts/reference_image_classifier__human_prompt_template.txt"
        )
        .replace("{photo_meta}", &meta_lines);

        #[derive(Deserialize)]
        struct LlmItem {
            photo_id: String,
            category: String,
            #[serde(default)]
            summary: String,
            #[serde(default)]
            suggested_label: String,
        }
        #[derive(Deserialize)]
        struct LlmResp {
            classifications: Vec<LlmItem>,
        }

        let resp: LlmResp = complete_vision_and_parse_llm_json(
            self.chat.as_ref(),
            &system,
            &user,
            &abs_refs,
        )
        .await?;

        let mut by_id: HashMap<String, LlmItem> = HashMap::new();
        for item in resp.classifications {
            by_id.insert(item.photo_id.clone(), item);
        }

        let mut out = Vec::with_capacity(photos.len());
        for photo in photos {
            if let Some(item) = by_id.remove(&photo.id) {
                out.push(ReferenceImageClassification {
                    photo_id: photo.id.clone(),
                    sha256: photo.sha256.clone(),
                    category: parse_category(&item.category),
                    summary: item.summary.trim().to_string(),
                    suggested_label: item.suggested_label.trim().to_string(),
                    heuristic: false,
                });
            } else {
                // Model omitted an id — fall back per photo rather than failing the plan.
                tracing::warn!(
                    photo_id = %photo.id,
                    "classifier omitted photo_id; using heuristic"
                );
                out.push(heuristic_classification(photo));
            }
        }
        Ok(out)
    }
}

/// Name-based fallback when vision is unavailable or incomplete.
///
/// Mirrors cameo_bind anonymous / scene-prompt heuristics without importing
/// pipelines (agents must not depend on pipelines).
///
/// Category is decided from `character_name` only — a place-like **description**
/// must not flip a real cast label (e.g. Alice + "rainy street") into environment.
pub fn heuristic_classification(photo: &CameoPhotoEntry) -> ReferenceImageClassification {
    let name = photo.character_name.trim();
    let desc = photo.description.trim();
    let non_character = looks_non_character_label(name);
    let category = if non_character {
        if looks_place_like(name) || looks_place_like(desc) {
            ReferenceImageCategory::Environment
        } else {
            ReferenceImageCategory::Style
        }
    } else {
        ReferenceImageCategory::Character
    };
    let summary = if !desc.is_empty() {
        desc.to_string()
    } else if !name.is_empty() {
        name.to_string()
    } else {
        String::new()
    };
    ReferenceImageClassification {
        photo_id: photo.id.clone(),
        sha256: photo.sha256.clone(),
        category,
        summary,
        suggested_label: name.to_string(),
        heuristic: true,
    }
}

/// Camera stems, scene-prompt filenames, demographic shorthand → not cast identity.
pub fn looks_non_character_label(name: &str) -> bool {
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
    if lower.starts_with("角色") {
        let rest = t.trim_start_matches("角色").trim();
        if rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    if t.chars().all(|c| c.is_ascii_digit()) && t.len() >= 4 {
        return true;
    }
    if looks_like_camera_stem(&lower) {
        return true;
    }
    if looks_like_scene_prompt_stem(t) {
        return true;
    }
    if is_descriptive_role_label(t) {
        return true;
    }
    false
}

fn looks_like_camera_stem(lower: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "img_", "img-", "dsc", "dscn", "dsc_", "photo_", "pic_", "mmexport", "wx_camera_",
        "screenshot",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p))
}

fn looks_like_scene_prompt_stem(name: &str) -> bool {
    let tokens: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.len() >= 4 {
        return true;
    }
    let has_cjk = name.chars().any(is_cjk_char);
    if !has_cjk {
        let alnum_len = name.chars().filter(|c| c.is_ascii_alphanumeric()).count();
        if alnum_len >= 28 && tokens.len() >= 3 {
            return true;
        }
    } else {
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

fn is_descriptive_role_label(name: &str) -> bool {
    let t = name.trim();
    if t.is_empty() {
        return false;
    }
    const CJK_ROLE: &[&str] = &[
        "男人", "女人", "男子", "女子", "男性", "女性", "大叔", "阿姨", "男孩", "女孩", "小孩",
        "儿童", "老人", "青年", "中年", "少年", "少女",
    ];
    let has_role = CJK_ROLE.iter().any(|w| t.contains(w));
    if !has_role {
        return false;
    }
    let cjk_count = t.chars().filter(|c| is_cjk_char(*c)).count();
    cjk_count >= 2 && cjk_count <= 10
}

fn is_cjk_char(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
        || (0x3040..=0x30FF).contains(&u)
        || (0xAC00..=0xD7AF).contains(&u)
}

fn looks_place_like(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "street",
        "room",
        "interior",
        "exterior",
        "village",
        "city",
        "forest",
        "beach",
        "office",
        "kitchen",
        "alley",
        "building",
        "landscape",
        "街",
        "屋",
        "室",
        "景",
        "村",
        "巷",
        "室内",
        "室外",
        "风景",
    ];
    NEEDLES.iter().any(|n| lower.contains(n) || s.contains(n))
}

fn parse_category(raw: &str) -> ReferenceImageCategory {
    match raw.trim().to_ascii_lowercase().as_str() {
        "character" | "cast" | "person" | "people" | "portrait" | "人物" | "角色" => {
            ReferenceImageCategory::Character
        }
        "environment" | "env" | "location" | "scene" | "set" | "background" | "环境" | "场景"
        | "地点" => ReferenceImageCategory::Environment,
        "prop" | "object" | "item" | "道具" | "物件" => ReferenceImageCategory::Prop,
        "style" | "mood" | "look" | "palette" | "画风" | "风格" | "氛围" => {
            ReferenceImageCategory::Style
        }
        other => {
            tracing::warn!(category = other, "unknown reference category; defaulting to style");
            // Unknown → style (safer than binding as cast identity).
            ReferenceImageCategory::Style
        }
    }
}

fn cache_path(session_root: &Path) -> PathBuf {
    let modern = session_root.join(CLASSIFICATION_CACHE_REL);
    if modern.exists() {
        return modern;
    }
    let legacy = session_root.join(LEGACY_CLASSIFICATION_CACHE_REL);
    if legacy.exists() {
        return legacy;
    }
    modern
}

fn load_cache(session_root: &Path) -> ClassificationCacheFile {
    let path = cache_path(session_root);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return ClassificationCacheFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_cache(session_root: &Path, report: &ReferenceClassificationReport) -> VimaxResult<()> {
    let path = cache_path(session_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = ClassificationCacheFile {
        classifications: report.classifications.clone(),
    };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&file)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Load cached report if present (may be stale — prefer `classify_session`).
pub fn load_classification_report(session_root: &Path) -> ReferenceClassificationReport {
    let file = load_cache(session_root);
    ReferenceClassificationReport {
        classifications: file.classifications,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(id: &str, name: &str, desc: &str, sha: &str) -> CameoPhotoEntry {
        CameoPhotoEntry {
            id: id.into(),
            rel_path: format!("references/photos/{id}.png"),
            character_name: name.into(),
            description: desc.into(),
            sha256: sha.into(),
            width: 10,
            height: 10,
            created_at: String::new(),
            updated_at: String::new(),
            bound_identifier: None,
        }
    }

    #[test]
    fn heuristic_marks_scene_prompt_as_non_character() {
        let p = photo(
            "a",
            "cramped old style chinese workers village rental",
            "",
            "sha1",
        );
        let c = heuristic_classification(&p);
        assert!(c.category.is_world_ref());
        assert!(c.heuristic);
    }

    #[test]
    fn heuristic_keeps_named_cast_as_character() {
        let p = photo("b", "陈树生", "工人", "sha2");
        let c = heuristic_classification(&p);
        assert_eq!(c.category, ReferenceImageCategory::Character);
    }

    #[test]
    fn report_filters_character_vs_world() {
        let photos = vec![
            photo("1", "Alice", "", "s1"),
            photo("2", "street", "rainy alley", "s2"),
        ];
        let report = ReferenceClassificationReport {
            classifications: vec![
                ReferenceImageClassification {
                    photo_id: "1".into(),
                    sha256: "s1".into(),
                    category: ReferenceImageCategory::Character,
                    summary: String::new(),
                    suggested_label: String::new(),
                    heuristic: false,
                },
                ReferenceImageClassification {
                    photo_id: "2".into(),
                    sha256: "s2".into(),
                    category: ReferenceImageCategory::Environment,
                    summary: "rainy alley".into(),
                    suggested_label: String::new(),
                    heuristic: false,
                },
            ],
        };
        assert_eq!(report.character_photos(&photos).len(), 1);
        assert_eq!(report.world_photos(&photos).len(), 1);
        assert_eq!(report.world_photos(&photos)[0].0.id, "2");
    }

    #[test]
    fn parse_category_aliases() {
        assert_eq!(parse_category("人物"), ReferenceImageCategory::Character);
        assert_eq!(parse_category("场景"), ReferenceImageCategory::Environment);
        assert_eq!(parse_category("道具"), ReferenceImageCategory::Prop);
        assert_eq!(parse_category("画风"), ReferenceImageCategory::Style);
        assert_eq!(parse_category("???"), ReferenceImageCategory::Style);
    }
}
