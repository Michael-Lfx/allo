//! Multimodal classification of user-uploaded session reference photos.
//!
//! Agent-mode uploads all land in `references/`; this agent decides which are cast
//! identity plates vs environment / prop / style refs before planning assets.
//! An explicit product label or script note ("图1是产品的截图") locks the photo
//! as a prop asset even when the vision model calls it a style board.

use std::collections::{HashMap, HashSet};
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

    pub fn is_prop(self) -> bool {
        matches!(self, Self::Prop)
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
    /// User label or script note forces this photo to stay a product/prop asset.
    #[serde(default)]
    pub locked: bool,
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

        let script = planning_text_for_locks(session_root);
        apply_explicit_product_locks(&photos, &script, &mut ordered);
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
        let notes = script_image_notes(&planning_text_for_locks(session_root));
        let user = include_str!(
            "../../prompts/reference_image_classifier__human_prompt_template.txt"
        )
        .replace("{photo_meta}", &meta_lines)
        .replace("{script_image_notes}", &notes);

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
                    locked: false,
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
    if looks_like_product_label(name) || looks_like_product_label(desc) {
        let summary = if !desc.is_empty() {
            desc.to_string()
        } else {
            name.to_string()
        };
        return ReferenceImageClassification {
            photo_id: photo.id.clone(),
            sha256: photo.sha256.clone(),
            category: ReferenceImageCategory::Prop,
            summary,
            suggested_label: name.to_string(),
            heuristic: true,
            locked: true,
        };
    }
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
        locked: false,
    }
}

/// Upload label / description that names a product or prop asset, not a cast member.
pub fn looks_like_product_label(text: &str) -> bool {
    let trimmed = text.trim();
    // Upload names are short. A sentence that merely mentions "logo" is not a product plate.
    if trimmed.is_empty() || trimmed.chars().count() > 24 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "产品图",
        "产品截图",
        "产品参考",
        "产品素材",
        "产品录屏",
        "产品照",
        "产品画面",
        "界面截图",
        "ui截图",
        "ui screenshot",
        "product screenshot",
        "product photo",
        "product reference",
        "product ref",
        "包装图",
        "包装参考",
        "道具图",
        "道具参考",
    ];
    if NEEDLES
        .iter()
        .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
    {
        return true;
    }
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    let compact_l = compact.to_ascii_lowercase();
    matches!(
        compact_l.as_str(),
        "道具" | "产品" | "包装" | "logo" | "logoref" | "logoreference"
    )
}

/// 1-based upload indexes the script explicitly assigns as a product or prop.
///
/// `图1是产品的截图` and `图片1 → 产品截图` lock image 1. `图片10` does not lock image 1.
pub fn explicit_product_photo_indexes(script: &str) -> HashSet<usize> {
    let mut found = HashSet::new();
    let mut rest = script;
    while !rest.is_empty() {
        let Some(rel) = rest.find('图') else {
            break;
        };
        let after_tu = &rest[rel + '图'.len_utf8()..];
        let (num_src, mark_len) = if let Some(stripped) = after_tu.strip_prefix('片') {
            (stripped, '图'.len_utf8() + '片'.len_utf8())
        } else {
            (after_tu, '图'.len_utf8())
        };
        let digits: String = num_src
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            rest = &rest[rel + '图'.len_utf8()..];
            continue;
        }
        let index: usize = digits.parse().unwrap_or(0);
        let after_num = &num_src[digits.len()..];
        let window: String = after_num.chars().take(48).collect();
        let window_trim = window.trim_start();
        let has_role = window_trim.starts_with('是')
            || window_trim.starts_with('为')
            || window_trim.starts_with('：')
            || window_trim.starts_with(':')
            || window_trim.starts_with('→')
            || window_trim.starts_with("->")
            || window_trim.starts_with('＝')
            || window_trim.starts_with('=');
        if index >= 1 && has_role && window_has_product_role(&window) {
            found.insert(index);
        }
        let advance = rel + mark_len + digits.len();
        rest = &rest[advance..];
    }
    found
}

fn window_has_product_role(window: &str) -> bool {
    looks_like_product_label(window) || window.contains("产品") || window.contains("道具")
}

/// Short script lines the classifier should see alongside the pixels.
pub fn script_image_notes(script: &str) -> String {
    let mut lines = Vec::new();
    for line in script.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains('图') {
            continue;
        }
        let relevant = trimmed.contains("产品")
            || trimmed.contains("道具")
            || trimmed.contains("画风")
            || trimmed.contains("风格")
            || trimmed.contains("图片");
        if !relevant || trimmed.chars().count() > 160 {
            continue;
        }
        lines.push(trimmed.to_string());
        if lines.len() == 8 {
            break;
        }
    }
    if lines.is_empty() {
        "（无）".to_string()
    } else {
        lines.join("\n")
    }
}

/// Force product/prop when the upload label or the script names that image as one.
///
/// Vision may still fill `summary`. A style or character answer cannot override
/// an explicit product note. Photos the user did not mark are unlocked.
pub fn apply_explicit_product_locks(
    photos: &[CameoPhotoEntry],
    script: &str,
    items: &mut [ReferenceImageClassification],
) {
    let indexes = explicit_product_photo_indexes(script);
    for (i, photo) in photos.iter().enumerate() {
        let Some(item) = items.iter_mut().find(|c| c.photo_id == photo.id) else {
            continue;
        };
        let explicit = looks_like_product_label(&photo.character_name)
            || looks_like_product_label(&photo.description)
            || indexes.contains(&(i + 1));
        if explicit {
            item.category = ReferenceImageCategory::Prop;
            item.locked = true;
        } else {
            item.locked = false;
        }
    }
}

/// Cache hit when the sha still matches, otherwise the name heuristic, then product locks.
///
/// Does not write the cache. Callers that only read (world refs, prop pinning)
/// still honor a script note saved after the last vision pass.
pub fn resolve_reference_classifications(
    session_root: &Path,
    photos: &[CameoPhotoEntry],
) -> ReferenceClassificationReport {
    let cached = load_classification_report(session_root);
    let mut items = Vec::with_capacity(photos.len());
    for photo in photos {
        if let Some(hit) = cached.classifications.iter().find(|c| {
            c.photo_id == photo.id && !c.sha256.is_empty() && c.sha256 == photo.sha256
        }) {
            items.push(hit.clone());
        } else {
            items.push(heuristic_classification(photo));
        }
    }
    let script = planning_text_for_locks(session_root);
    apply_explicit_product_locks(photos, &script, &mut items);
    ReferenceClassificationReport {
        classifications: items,
    }
}

fn planning_text_for_locks(session_root: &Path) -> String {
    let mut chunks = Vec::new();
    let mut push_file = |path: PathBuf| {
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        if !text.trim().is_empty() {
            chunks.push(text);
        }
    };
    for name in ["script.txt", "story.txt"] {
        push_file(session_root.join(name));
    }
    if let Ok(read) = std::fs::read_dir(session_root) {
        for entry in read.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !matches!(
                name.as_ref(),
                "script2video" | "idea2video" | "novel2video" | "action2video"
            ) {
                continue;
            }
            for fname in ["script.txt", "story.txt"] {
                push_file(path.join(fname));
            }
        }
    }
    let mut out = chunks.join("\n");
    if out.chars().count() > 80_000 {
        out = out.chars().take(80_000).collect();
    }
    out
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
                    locked: false,
                },
                ReferenceImageClassification {
                    photo_id: "2".into(),
                    sha256: "s2".into(),
                    category: ReferenceImageCategory::Environment,
                    summary: "rainy alley".into(),
                    suggested_label: String::new(),
                    heuristic: false,
                    locked: false,
                },
            ],
        };
        assert_eq!(report.character_photos(&photos).len(), 1);
        assert_eq!(report.world_photos(&photos).len(), 1);
        assert_eq!(report.world_photos(&photos)[0].0.id, "2");
    }

    #[test]
    fn heuristic_locks_product_screenshot_as_prop() {
        let p = photo("p", "产品截图", "", "sha");
        let c = heuristic_classification(&p);
        assert_eq!(c.category, ReferenceImageCategory::Prop);
        assert!(c.locked);
        assert!(c.heuristic);
    }

    #[test]
    fn heuristic_does_not_treat_product_manager_as_asset() {
        let p = photo("m", "产品经理", "", "sha");
        let c = heuristic_classification(&p);
        assert_eq!(c.category, ReferenceImageCategory::Character);
        assert!(!c.locked);
    }

    #[test]
    fn script_product_note_overrides_style_classification() {
        let photos = vec![photo("1", "参考图1", "", "s1")];
        let mut items = vec![ReferenceImageClassification {
            photo_id: "1".into(),
            sha256: "s1".into(),
            category: ReferenceImageCategory::Style,
            summary: "software screenshot".into(),
            suggested_label: "product UI screenshot".into(),
            heuristic: false,
            locked: false,
        }];
        let script = "图1是产品的截图\n\n图片对照：图片1 → 产品截图";
        apply_explicit_product_locks(&photos, script, &mut items);
        assert_eq!(items[0].category, ReferenceImageCategory::Prop);
        assert!(items[0].locked);
        assert_eq!(items[0].summary, "software screenshot");
    }

    #[test]
    fn image_ten_does_not_lock_image_one() {
        let script = "图片对照：图片10 → 产品截图、图片1 → Alice";
        let indexes = explicit_product_photo_indexes(script);
        assert!(indexes.contains(&10));
        assert!(!indexes.contains(&1));
    }

    #[test]
    fn label_locks_even_when_vision_said_style() {
        let photos = vec![photo("1", "产品截图", "", "s1")];
        let mut items = vec![ReferenceImageClassification {
            photo_id: "1".into(),
            sha256: "s1".into(),
            category: ReferenceImageCategory::Style,
            summary: "A screenshot of a software application interface.".into(),
            suggested_label: "product UI screenshot".into(),
            heuristic: false,
            locked: false,
        }];
        apply_explicit_product_locks(&photos, "", &mut items);
        assert_eq!(items[0].category, ReferenceImageCategory::Prop);
        assert!(items[0].locked);
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
