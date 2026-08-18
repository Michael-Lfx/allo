use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::FutureExt;
use serde::{Deserialize, Serialize};

use crate::backends::{VimaxChat, VimaxImage};
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::complete_and_parse_llm_json;
use crate::session::{read_json_artifact, write_json_artifact};

use super::formats::WORLD_ASSETS;

/// Vacant production-look bible — shared img2img anchor for cast, sets, and props.
pub const LOOK_PLATE_FILENAME: &str = "look_plate.png";
const LOOK_PLATE_LOCK_FILENAME: &str = "look_plate_lock.txt";

/// Extracted environment plate for global consistency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentAsset {
    #[serde(default)]
    pub idx: i32,
    /// Location label; models sometimes emit `name` / `title` instead of `slugline`.
    #[serde(default)]
    #[serde(alias = "name", alias = "title", alias = "Slugline", alias = "location")]
    pub slugline: String,
    #[serde(default)]
    pub description: String,
}

/// Key prop / object that must stay consistent across shots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropAsset {
    #[serde(default)]
    pub idx: i32,
    /// Object label; tolerate alternate keys from weaker JSON models.
    #[serde(alias = "title", alias = "label", alias = "slugline", alias = "Name")]
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldAssetsSpec {
    #[serde(default)]
    pub environments: Vec<EnvironmentAsset>,
    #[serde(default)]
    pub props: Vec<PropAsset>,
}

/// Registry consumed by frame generation: name → {path, description}.
pub type WorldAssetRegistry = HashMap<String, HashMap<String, HashMap<String, String>>>;

pub struct WorldAssetsPlanner {
    chat: Arc<dyn VimaxChat>,
    image: Arc<dyn VimaxImage>,
}

impl WorldAssetsPlanner {
    pub fn new(chat: Arc<dyn VimaxChat>, image: Arc<dyn VimaxImage>) -> Self {
        Self { chat, image }
    }

    pub async fn extract(
        &self,
        script_or_story: &str,
        style: &str,
        scene_hint: &str,
    ) -> VimaxResult<WorldAssetsSpec> {
        let style = crate::planning::resolve_visual_style(style);
        let system = include_str!(
            "../../prompts/world_assets__system_prompt_template_extract.txt"
        )
        .replace("{format_instructions}", WORLD_ASSETS);
        let text = if scene_hint.trim().is_empty() {
            script_or_story.to_string()
        } else {
            format!("{script_or_story}{scene_hint}")
        };
        let user = include_str!("../../prompts/world_assets__human_prompt_template_extract.txt")
            .replace("{style}", &style)
            .replace("{text}", &text);
        let mut spec: WorldAssetsSpec =
            complete_and_parse_llm_json(self.chat.as_ref(), &system, &user).await?;
        // Drop empty shells; backfill prop names from description when models omit `name`.
        spec.environments
            .retain(|e| !e.slugline.trim().is_empty() || !e.description.trim().is_empty());
        for e in &mut spec.environments {
            if e.slugline.trim().is_empty() {
                e.slugline = e
                    .description
                    .chars()
                    .take(48)
                    .collect::<String>()
                    .trim()
                    .to_string();
            }
        }
        for p in &mut spec.props {
            if p.name.trim().is_empty() {
                p.name = p
                    .description
                    .chars()
                    .take(32)
                    .collect::<String>()
                    .trim()
                    .to_string();
            }
        }
        spec.props
            .retain(|p| !p.name.trim().is_empty() || !p.description.trim().is_empty());
        let before_people_filter = spec.props.len();
        spec.props
            .retain(|p| !is_people_centric_prop(&p.name, &p.description));
        if spec.props.len() < before_people_filter {
            tracing::info!(
                dropped = before_people_filter - spec.props.len(),
                "dropped people-centric prop concepts (portraits / group photos)"
            );
        }
        if spec.environments.len() > 5 {
            spec.environments.truncate(5);
        }
        if spec.props.len() > 8 {
            spec.props.truncate(8);
        }
        for (i, e) in spec.environments.iter_mut().enumerate() {
            e.idx = i as i32;
            e.description = strip_people_mentions(&e.description);
        }
        for (i, p) in spec.props.iter_mut().enumerate() {
            p.idx = i as i32;
            p.description = strip_people_mentions(&p.description);
        }
        Ok(spec)
    }

    /// Vacant production-look plate used as the shared img2img style bible.
    ///
    /// Best-effort: returns an empty vec when generation fails so planning can
    /// still proceed on the text [`crate::planning::production_look_lock`].
    pub async fn look_style_refs(
        &self,
        film_root: &Path,
        style: &str,
        theme: &str,
    ) -> Vec<PathBuf> {
        match self.ensure_look_plate(film_root, style, theme).await {
            Ok(path) if crate::media_local::is_usable_image_file(&path) => vec![path],
            Ok(path) => {
                tracing::warn!(
                    path = %path.display(),
                    "production look plate unusable; bible images fall back to text lock"
                );
                Vec::new()
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "production look plate skipped; bible images fall back to text lock"
                );
                Vec::new()
            }
        }
    }

    /// Generate (or reuse) `look_plate.png` under `film_root`.
    /// Regenerates when the stored style fingerprint no longer matches.
    pub async fn ensure_look_plate(
        &self,
        film_root: &Path,
        style: &str,
        theme: &str,
    ) -> VimaxResult<PathBuf> {
        tokio::fs::create_dir_all(film_root).await?;
        let style = crate::planning::resolve_visual_style(style);
        let out = film_root.join(LOOK_PLATE_FILENAME);
        let lock_path = film_root.join(LOOK_PLATE_LOCK_FILENAME);
        let prev = tokio::fs::read_to_string(&lock_path)
            .await
            .unwrap_or_default();
        let stale_style = prev.trim() != style.trim();
        if stale_style && out.exists() {
            tracing::info!(
                film_root = %film_root.display(),
                "production look lock changed — regenerating look plate"
            );
            let _ = tokio::fs::remove_file(&out).await;
        }
        if crate::media_local::is_usable_image_file(&out) {
            if stale_style {
                let _ = crate::session::write_text_artifact(&lock_path, &style).await;
            }
            return Ok(out);
        }
        let look = crate::planning::production_look_lock(&style);
        let prompt = include_str!("../../prompts/world_assets__prompt_template_look_plate.txt")
            .replace("{theme}", theme)
            .replace("{style}", &look);
        self.generate_empty_plate_resilient(&prompt, &[], &out)
            .await?;
        let _ = write_generation_prompt_sidecar(&out, &prompt).await;
        let _ = crate::session::write_text_artifact(&lock_path, &style).await;
        Ok(out)
    }

    /// Extract (if needed) and generate missing environment / prop plates under `film_root`.
    ///
    /// When `style_refs` / `style_lock_token` come from user Cameo photos, plates are
    /// regenerated if the lock token changes so scenery stays consistent with uploads.
    pub async fn ensure(
        &self,
        film_root: &Path,
        script_or_story: &str,
        style: &str,
        style_refs: &[PathBuf],
        scene_hint: &str,
        style_lock_token: &str,
    ) -> VimaxResult<WorldAssetRegistry> {
        tokio::fs::create_dir_all(film_root).await?;
        let spec_path = film_root.join("world_assets.json");
        let registry_path = film_root.join("world_assets_registry.json");
        let lock_path = film_root.join("world_assets_cameo_lock.txt");

        let style = crate::planning::resolve_visual_style(style);
        let theme = {
            let base = theme_excerpt(script_or_story);
            if scene_hint.trim().is_empty() {
                base
            } else {
                // Keep theme short but bias toward Cameo scene cues.
                let cue: String = scene_hint
                    .split_whitespace()
                    .filter(|w| {
                        let t = w.trim_matches(|c: char| !c.is_alphanumeric());
                        t.chars().count() >= 2
                    })
                    .take(24)
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{base} | cameo-world: {}", cue.chars().take(100).collect::<String>())
            }
        };

        // Cameo set changed (or plates were made without Cameo): drop stale world plates.
        if !style_lock_token.is_empty() {
            let prev = tokio::fs::read_to_string(&lock_path)
                .await
                .unwrap_or_default();
            let prev = prev.trim().to_string();
            if prev != style_lock_token {
                tracing::info!(
                    film_root = %film_root.display(),
                    "cameo style lock changed — regenerating world asset plates"
                );
                invalidate_world_asset_artifacts(film_root).await?;
            }
        }

        let style_ref_paths: Vec<PathBuf> = {
            let mut refs = self.look_style_refs(film_root, &style, &theme).await;
            for p in style_refs {
                if crate::media_local::is_usable_image_file(p) && !refs.iter().any(|e| e == p) {
                    refs.push(p.clone());
                }
            }
            refs
        };

        let spec: WorldAssetsSpec = if spec_path.exists() {
            read_json_artifact(&spec_path).await?
        } else {
            let spec = self
                .extract(script_or_story, &style, scene_hint)
                .await?;
            write_json_artifact(&spec_path, &spec).await?;
            spec
        };

        let mut registry: WorldAssetRegistry = if registry_path.exists() {
            read_json_artifact(&registry_path).await?
        } else {
            HashMap::new()
        };

        let env_root = film_root.join("environments");
        let prop_root = film_root.join("props");
        tokio::fs::create_dir_all(&env_root).await?;
        tokio::fs::create_dir_all(&prop_root).await?;

        let mut env_map = registry.remove("environments").unwrap_or_default();
        let mut prop_map = registry.remove("props").unwrap_or_default();

        /// One env/prop plate prepared for generation + registry registration.
        struct PreparedPlate {
            group: &'static str,
            key: String,
            out: PathBuf,
            stripped_desc: String,
            registry_desc: String,
            prompt: Option<String>,
        }

        // Phase A — prepare every plate (dirs, legacy rename, prompt) and collect
        // the missing ones for parallel generation. Plates have no cross-dependency:
        // each is a self-contained vacant-set image.
        let mut prepared: Vec<PreparedPlate> = Vec::new();
        for env in &spec.environments {
            let key = if env.slugline.trim().is_empty() {
                format!("env_{}", env.idx)
            } else {
                env.slugline.trim().to_string()
            };
            let dir = env_root.join(format!("{}_{}", env.idx, safe_component(&key)));
            tokio::fs::create_dir_all(&dir).await?;
            let plate_name = format!("{}_environment_plate.png", safe_component(&key));
            let out = dir.join(&plate_name);
            let legacy = dir.join("plate.png");
            if !out.exists() && legacy.exists() {
                let _ = tokio::fs::rename(&legacy, &out).await;
            }
            let stripped_desc = strip_people_mentions(&env.description);
            let prompt = if !out.exists() {
                Some(environment_plate_prompt(
                    &theme,
                    &env.slugline,
                    &stripped_desc,
                    &style,
                ))
            } else {
                None
            };
            let detail: String = stripped_desc.chars().take(120).collect();
            let cameo_note = if style_ref_paths.is_empty() {
                String::new()
            } else {
                " Style-locked to user Cameo references.".into()
            };
            let registry_desc = format!(
                "File [{plate_name}] = GLOBAL EMPTY environment plate (no people): {key}. {detail}. Lock architecture, lighting, set dressing only.{cameo_note}"
            );
            prepared.push(PreparedPlate {
                group: "environments",
                key,
                out,
                stripped_desc,
                registry_desc,
                prompt,
            });
        }
        for prop in &spec.props {
            let key = prop.name.trim().to_string();
            if key.is_empty() {
                continue;
            }
            let dir = prop_root.join(format!("{}_{}", prop.idx, safe_component(&key)));
            tokio::fs::create_dir_all(&dir).await?;
            let plate_name = format!("{}_prop.png", safe_component(&key));
            let out = dir.join(&plate_name);
            let legacy = dir.join("prop.png");
            if !out.exists() && legacy.exists() {
                let _ = tokio::fs::rename(&legacy, &out).await;
            }
            let stripped_desc = strip_people_mentions(&prop.description);
            let prompt = if !out.exists() {
                Some(prop_plate_prompt(
                    &theme,
                    &prop.name,
                    &stripped_desc,
                    &style,
                ))
            } else {
                None
            };
            let detail: String = stripped_desc.chars().take(100).collect();
            let cameo_note = if style_ref_paths.is_empty() {
                String::new()
            } else {
                " Style-locked to user Cameo references.".into()
            };
            let registry_desc = format!(
                "File [{plate_name}] = GLOBAL prop bible (object only, no people): <{key}>. {detail}. Lock shape, materials, colors.{cameo_note}"
            );
            prepared.push(PreparedPlate {
                group: "props",
                key,
                out,
                stripped_desc,
                registry_desc,
                prompt,
            });
        }

        // Phase B — generate missing plates in parallel (semaphore 4, same as the
        // character-portrait fan-out). Each worker owns its image/chat handles.
        if prepared.iter().any(|p| p.prompt.is_some()) {
            let sem = Arc::new(tokio::sync::Semaphore::new(4));
            let mut set = tokio::task::JoinSet::new();
            for plate in &prepared {
                let Some(prompt) = plate.prompt.clone() else {
                    continue;
                };
                let image = Arc::clone(&self.image);
                let chat = Arc::clone(&self.chat);
                let style_refs = style_ref_paths.clone();
                let out = plate.out.clone();
                let permit = Arc::clone(&sem);
                set.spawn(async move {
                    let _permit = permit.acquire_owned().await.map_err(|_| {
                        VimaxError::msg("world plate semaphore closed")
                    })?;
                    let refs: Vec<&Path> =
                        style_refs.iter().map(|p| p.as_path()).collect();
                    let planner = WorldAssetsPlanner { image, chat };
                    planner
                        .generate_empty_plate_resilient(&prompt, &refs, &out)
                        .await?;
                    let _ = write_generation_prompt_sidecar(&out, &prompt).await;
                    Ok::<_, VimaxError>(())
                });
            }
            while let Some(joined) = set.join_next().await {
                joined.map_err(|e| VimaxError::msg(format!("world plate join: {e}")))??;
            }
        }

        // Phase C — register plates (skip ones already in the registry) with the
        // same per-plate checkpoint writes as before, in deterministic order.
        for plate in &prepared {
            let out = &plate.out;
            if plate.prompt.is_none() {
                let _ = ensure_world_prompt_sidecar(
                    out,
                    &theme,
                    &style,
                    match plate.group {
                        "environments" => WorldPromptKind::Environment {
                            slugline: &plate.key,
                            description: &plate.stripped_desc,
                        },
                        _ => WorldPromptKind::Prop {
                            name: &plate.key,
                            description: &plate.stripped_desc,
                        },
                    },
                )
                .await;
            }
            let already = match plate.group {
                "environments" => env_map.contains_key(&plate.key),
                _ => prop_map.contains_key(&plate.key),
            };
            if already {
                continue;
            }
            let item = asset_item(out, &plate.registry_desc);
            match plate.group {
                "environments" => env_map.insert(plate.key.clone(), item),
                _ => prop_map.insert(plate.key.clone(), item),
            };
            // Checkpoint so resume keeps completed plates even if a later one fails.
            registry.insert("environments".into(), env_map.clone());
            registry.insert("props".into(), prop_map.clone());
            let _ = write_json_artifact(&registry_path, &registry).await;
        }

        registry.insert("environments".into(), env_map);
        registry.insert("props".into(), prop_map);
        write_json_artifact(&registry_path, &registry).await?;
        if !style_lock_token.is_empty() {
            crate::session::write_text_artifact(&lock_path, style_lock_token).await?;
        }
        Ok(registry)
    }

    /// Catch panics from image/vision stacks so planning returns a real error, not a silent unwind.
    async fn generate_empty_plate_resilient(
        &self,
        prompt: &str,
        style_refs: &[&Path],
        out: &Path,
    ) -> VimaxResult<()> {
        match std::panic::AssertUnwindSafe(self.generate_empty_plate(prompt, style_refs, out))
            .catch_unwind()
            .await
        {
            Ok(r) => r,
            Err(payload) => {
                let _ = tokio::fs::remove_file(out).await;
                Err(VimaxError::from_panic_payload("world asset plate", payload))
            }
        }
    }

    /// Generate vacant plate; optional people-free style refs; fall back to text-only if people leak.
    async fn generate_empty_plate(
        &self,
        prompt: &str,
        style_refs: &[&Path],
        out: &Path,
    ) -> VimaxResult<()> {
        // Defense in depth: never feed portrait Cameo / cast sheets into vacant plates.
        let style_refs: Vec<&Path> = style_refs
            .iter()
            .copied()
            .filter(|p| is_safe_world_style_ref(p))
            .collect();
        let prompted = if style_refs.is_empty() {
            prompt.to_string()
        } else {
            format!(
                "{prompt}\n\n\
STYLE/SCENE CONTEXT from reference image(s): match era, palette, materials, lighting mood, \
and setting type from the references. Do NOT copy any person, face, body, hand, or silhouette \
from the references. Output must remain a completely unoccupied empty-set plate — never a \
group photo, portrait, selfie, or framed photo of people."
            )
        };

        self.image.generate(&prompted, &style_refs, out).await?;
        if !self.plate_has_people(out).await {
            return Ok(());
        }

        // Style refs can still leak faces — drop refs and retry.
        if !style_refs.is_empty() {
            tracing::warn!(
                path = %out.display(),
                refs = style_refs.len(),
                "world asset plate contains people with style refs; retrying without image refs"
            );
            let _ = tokio::fs::remove_file(out).await;
            let no_ref_prompt = format!(
                "{prompt}\nCRITICAL: match the era/palette/materials described in Theme, but \
                 draw a vacant plate only — ZERO people, ZERO faces, ZERO silhouettes, ZERO hands. \
                 Never output a group photo, portrait, or selfie."
            );
            self.image.generate(&no_ref_prompt, &[], out).await?;
            if !self.plate_has_people(out).await {
                return Ok(());
            }
        }

        for attempt in 1..=2 {
            tracing::warn!(
                path = %out.display(),
                attempt,
                "world asset plate contains people; regenerating empty-set"
            );
            let _ = tokio::fs::remove_file(out).await;
            let retry = if attempt == 1 {
                format!(
                    "{prompt}\nCRITICAL RETRY: previous image illegally showed humans. \
                     Vacant plate only — ZERO people, ZERO faces, ZERO silhouettes, ZERO hands."
                )
            } else {
                // Short hard prompt so safety prefix + truncate cannot bury the empty-set rule.
                format!(
                    "Wide 16:9 vacant unoccupied film location or isolated object plate. \
                     Completely empty. Zero people, zero humans, zero faces, zero silhouettes, zero hands, zero body parts. \
                     Architecture furniture props lighting only. {prompt}"
                )
            };
            self.image.generate(&retry, &[], out).await?;
            if !self.plate_has_people(out).await {
                return Ok(());
            }
        }

        // Do not keep a contaminated plate in the registry path — force caller to notice.
        tracing::error!(
            path = %out.display(),
            "world asset plate still contains people after retries; deleting bad file"
        );
        let _ = tokio::fs::remove_file(out).await;
        Err(crate::error::VimaxError::Image(format!(
            "empty-set plate still contains people after retries: {}",
            out.display()
        )))
    }

    async fn plate_has_people(&self, path: &Path) -> bool {
        // Vision check only needs a thumbnail — full 2K plates bloat multimodal payloads.
        let vision_path = match downsample_for_vision(path).await {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(error = %err, path = %path.display(), "world-asset vision downsample failed; using original");
                path.to_path_buf()
            }
        };
        let raw = match self
            .chat
            .complete_vision(
                "You are a strict image inspector. Reply with exactly YES or NO.",
                "Does this image contain any human, person, face, crowd, silhouette of a person, hand, or body part? YES or NO only.",
                &[vision_path.as_path()],
            )
            .await
        {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(error = %err, "world-asset people check failed; assuming clean");
                return false;
            }
        };
        if vision_path != path {
            let _ = tokio::fs::remove_file(&vision_path).await;
        }
        let upper = raw.trim().to_ascii_uppercase();
        let trimmed = raw.trim();
        if upper.starts_with("NO")
            || trimmed.starts_with('否')
            || trimmed.starts_with("没有")
            || trimmed.starts_with("無")
        {
            return false;
        }
        upper.starts_with("YES")
            || trimmed.starts_with('是')
            || trimmed.starts_with("有人")
    }
}

async fn downsample_for_vision(path: &Path) -> VimaxResult<PathBuf> {
    let bytes = tokio::fs::read(path).await?;
    let img = image::load_from_memory(&bytes).map_err(|e| {
        VimaxError::Media(format!("decode plate for vision {}: {e}", path.display()))
    })?;
    let thumb = img.thumbnail(768, 768);
    let out = path.with_extension("vision_thumb.jpg");
    let thumb_path = out.clone();
    tokio::task::spawn_blocking(move || {
        thumb
            .save_with_format(&out, image::ImageFormat::Jpeg)
            .map_err(|e| VimaxError::Media(format!("save vision thumb: {e}")))
    })
    .await
    .map_err(|e| VimaxError::Media(format!("vision thumb join: {e}")))??;
    Ok(thumb_path)
}

async fn invalidate_world_asset_artifacts(film_root: &Path) -> VimaxResult<()> {
    for name in ["environments", "props"] {
        let dir = film_root.join(name);
        if dir.is_dir() {
            let _ = tokio::fs::remove_dir_all(&dir).await;
        }
    }
    for name in [
        "world_assets.json",
        "world_assets_registry.json",
        "world_assets_cameo_lock.txt",
    ] {
        let p = film_root.join(name);
        if p.exists() {
            let _ = tokio::fs::remove_file(&p).await;
        }
    }
    Ok(())
}

fn asset_item(path: &Path, description: &str) -> HashMap<String, String> {
    let mut item = HashMap::new();
    item.insert("path".into(), path.to_string_lossy().to_string());
    item.insert("description".into(), description.to_string());
    item
}

enum WorldPromptKind<'a> {
    Environment { slugline: &'a str, description: &'a str },
    Prop { name: &'a str, description: &'a str },
}

async fn write_generation_prompt_sidecar(image_path: &Path, prompt: &str) -> VimaxResult<()> {
    let stem = image_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("asset");
    let parent = image_path.parent().unwrap_or_else(|| Path::new("."));
    let path = parent.join(format!("{stem}_generation_prompt.txt"));
    crate::session::write_text_artifact(&path, prompt).await
}

async fn ensure_world_prompt_sidecar(
    image_path: &Path,
    theme: &str,
    style: &str,
    kind: WorldPromptKind<'_>,
) -> VimaxResult<()> {
    let stem = image_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("asset");
    let parent = image_path.parent().unwrap_or_else(|| Path::new("."));
    let path = parent.join(format!("{stem}_generation_prompt.txt"));
    if path.is_file() {
        return Ok(());
    }
    let prompt = match kind {
        WorldPromptKind::Environment {
            slugline,
            description,
        } => environment_plate_prompt(
            theme,
            slugline,
            &strip_people_mentions(description),
            style,
        ),
        WorldPromptKind::Prop { name, description } => prop_plate_prompt(
            theme,
            name,
            &strip_people_mentions(description),
            style,
        ),
    };
    write_generation_prompt_sidecar(image_path, &prompt).await
}

fn environment_plate_prompt(theme: &str, slugline: &str, description: &str, style: &str) -> String {
    include_str!("../../prompts/world_assets__prompt_template_environment_plate.txt")
        .replace("{theme}", theme)
        .replace("{slugline}", slugline)
        .replace("{description}", description)
        .replace("{style}", &crate::planning::production_look_lock(style))
}

fn prop_plate_prompt(theme: &str, name: &str, description: &str, style: &str) -> String {
    include_str!("../../prompts/world_assets__prompt_template_prop.txt")
        .replace("{theme}", theme)
        .replace("{name}", name)
        .replace("{description}", description)
        .replace("{style}", &crate::planning::production_look_lock(style))
}

fn theme_excerpt(script_or_story: &str) -> String {
    let compact: String = script_or_story
        .split_whitespace()
        .take(40)
        .collect::<Vec<_>>()
        .join(" ");
    compact.chars().take(140).collect()
}

/// Drop human/crowd cues from LLM descriptions so image prompts stay empty-set.
fn strip_people_mentions(text: &str) -> String {
    let mut s = text.to_string();
    for p in [
        "人影", "人群", "人们", "行人", "顾客", "客人", "路人", "男人", "女人", "小孩", "儿童",
        "店员", "服务员", "职员", "乘客", "观众", "游客", "士兵", "警察", "司机", "老板", "主角",
        "角色", "身影", "背影", "侧影", "有人", "众人", "男女老少", "熙熙攘攘", "站着", "坐着的人",
        "合照", "全家福", "自拍", "人像", "肖像", "证件照", "大头照",
    ] {
        s = s.replace(p, "");
    }
    if let Ok(re) = regex::RegexBuilder::new(
        r"(?i)\b(crowds?|people|persons?|someone|somebody|pedestrians?|passers?-?by|patrons?|customers?|waiters?|waitresses?|baristas?|tourists?|staff|silhouettes?|figures?|selfies?|portraits?|headshots?|group\s+photos?|family\s+photos?|(?:a|the|several|many|few)\s+(?:man|woman|men|women|boy|girl|boys|girls|child|children|kids?))\b",
    )
    .build()
    {
        s = re.replace_all(&s, " ").into_owned();
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Props that are essentially images-of-people (would bake faces into world plates).
fn is_people_centric_prop(name: &str, desc: &str) -> bool {
    let blob = format!("{name} {desc}").to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "合照",
        "全家福",
        "自拍",
        "人像",
        "肖像",
        "证件照",
        "大头照",
        "毕业照",
        "婚纱照",
        "结婚照",
        "人物照",
        "照片墙",
        "group photo",
        "family photo",
        "wedding photo",
        "graduation photo",
        "selfie",
        "portrait",
        "headshot",
        "photo of people",
        "photo of a person",
        "picture of people",
        "framed photo of",
    ];
    NEEDLES.iter().any(|n| blob.contains(n))
}

/// Style refs safe for vacant env/prop img2img (atmosphere plates only; never cast portraits).
fn is_safe_world_style_ref(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.contains("look_plate") || name.contains("atmosphere") {
        return true;
    }
    // Explicitly reject cast identity plates.
    if name.contains("cameo")
        || name.contains("three_view")
        || name.contains("portrait")
        || name.contains("character")
    {
        return false;
    }
    true
}

fn safe_component(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || is_path_safe_ideograph(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').chars().take(80).collect::<String>();
    if out.is_empty() {
        "asset".into()
    } else {
        out
    }
}

fn is_path_safe_ideograph(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u) // CJK Unified
        || (0x3400..=0x4DBF).contains(&u) // CJK Ext A
        || (0x3040..=0x30FF).contains(&u) // Hiragana / Katakana
        || (0xAC00..=0xD7AF).contains(&u) // Hangul
}

/// Flatten registry into (path, description) pairs for frame / video reference selection.
///
/// Resolves absolute paths from another machine onto `film_root` when needed, and
/// drops entries whose files are still missing.
pub fn world_asset_pairs(
    registry: &WorldAssetRegistry,
    film_root: &Path,
) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for group in ["environments", "props"] {
        if let Some(map) = registry.get(group) {
            for item in map.values() {
                if let (Some(p), Some(d)) = (item.get("path"), item.get("description")) {
                    let path = crate::session::resolve_stored_asset_path(p, film_root);
                    if crate::media_local::is_usable_image_file(&path) {
                        out.push((path, d.clone()));
                    }
                }
            }
        }
    }
    out
}

/// Rank env/prop plates by overlap with the frame description (avoid wrong location).
pub fn rank_world_pairs_for_frame(
    frame_desc: &str,
    pairs: &[(PathBuf, String)],
    max: usize,
) -> Vec<(PathBuf, String)> {
    if pairs.is_empty() || max == 0 {
        return Vec::new();
    }
    let desc = frame_desc.to_ascii_lowercase();
    let mut scored: Vec<(i32, usize)> = pairs
        .iter()
        .enumerate()
        .map(|(i, (path, text))| {
            let blob = format!("{} {}", path.to_string_lossy(), text).to_ascii_lowercase();
            let mut score = 0i32;
            for tok in match_tokens(&blob) {
                if tok.chars().count() < 2 {
                    continue;
                }
                if desc.contains(&tok) {
                    score += if tok.chars().count() >= 4 { 3 } else { 1 };
                }
            }
            // Slight preference for environment plates over props when tied later.
            if blob.contains("environments") || blob.contains("empty environment") {
                score += 1;
            }
            (score, i)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut out = Vec::new();
    for (score, i) in scored {
        if out.len() >= max {
            break;
        }
        // Keep weak matches only for the first env fallback.
        if score <= 1 && !out.is_empty() {
            continue;
        }
        out.push(pairs[i].clone());
    }
    if out.is_empty() {
        // Fallback: first environment plate if any, else first prop.
        if let Some(env) = pairs.iter().find(|(p, _)| {
            p.to_string_lossy()
                .to_ascii_lowercase()
                .contains("environments")
        }) {
            out.push(env.clone());
        } else {
            out.push(pairs[0].clone());
        }
    }
    out
}

fn match_tokens(blob: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    for ch in blob.chars() {
        if ch.is_ascii_alphanumeric() || (ch as u32) > 127 {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            toks.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    // Also add 2-char CJK bigrams from longer CJK runs for better Chinese matching.
    let cjk: String = blob
        .chars()
        .filter(|c| {
            let u = *c as u32;
            (0x4E00..=0x9FFF).contains(&u)
        })
        .collect();
    if cjk.chars().count() >= 2 {
        let chars: Vec<char> = cjk.chars().collect();
        for w in chars.windows(2) {
            toks.push(format!("{}{}", w[0], w[1]));
        }
    }
    toks
}

#[cfg(test)]
mod tests {
    use super::{
        WorldAssetsSpec, is_people_centric_prop, is_safe_world_style_ref, rank_world_pairs_for_frame,
        environment_plate_prompt, prop_plate_prompt,
        strip_people_mentions,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn strips_people_keeps_set_words() {
        let out = strip_people_mentions(
            "A crowded coffee shop with people and permanent wood tables, a woman at the counter",
        );
        let lower = out.to_ascii_lowercase();
        assert!(lower.contains("coffee") || lower.contains("wood") || lower.contains("tables"));
        assert!(!lower.contains("people"));
        assert!(!lower.contains("woman"));
        assert!(lower.contains("permanent"));
    }

    #[test]
    fn drops_people_centric_prop_concepts() {
        assert!(is_people_centric_prop("全家福合照", "墙上的照片"));
        assert!(is_people_centric_prop("Family portrait", "framed photo"));
        assert!(!is_people_centric_prop("红伞", "油纸伞，无人物"));
    }

    #[test]
    fn rejects_portrait_cameo_as_world_style_ref() {
        assert!(!is_safe_world_style_ref(Path::new(
            "character_portraits/0_Alice/Alice_cameo.png"
        )));
        assert!(is_safe_world_style_ref(Path::new(
            "character_portraits/0_Alice/Alice_cameo_atmosphere.png"
        )));
        assert!(is_safe_world_style_ref(Path::new("look_plate.png")));
    }

    #[test]
    fn world_plate_prompts_share_production_look_lock_not_face_clause() {
        let style = "cinematic film look";
        let look = crate::planning::production_look_lock(style);
        let env = environment_plate_prompt("rainy alley", "EXT. ALLEY - NIGHT", "wet brick", style);
        let prop = prop_plate_prompt("rainy alley", "red umbrella", "oil-paper", style);
        assert!(env.contains(&look));
        assert!(prop.contains(&look));
        assert!(env.to_ascii_lowercase().contains("same production look"));
        assert!(prop.to_ascii_lowercase().contains("same production look"));
        assert!(!env.to_ascii_lowercase().contains("faces:"));
        assert!(!prop.to_ascii_lowercase().contains("faces:"));
        assert!(!env.contains("If Style is anime"));
        assert!(!prop.contains("If Style is anime"));
    }

    #[test]
    fn ranks_matching_environment_higher() {
        let pairs = vec![
            (
                PathBuf::from("environments/0_INT_OFFICE/INT_OFFICE_environment_plate.png"),
                "GLOBAL EMPTY environment plate (no people): INT. OFFICE - DAY.".into(),
            ),
            (
                PathBuf::from(
                    "environments/1_INT_COFFEE_SHOP/INT_COFFEE_SHOP_environment_plate.png",
                ),
                "GLOBAL EMPTY environment plate (no people): INT. COFFEE SHOP - NIGHT.".into(),
            ),
            (
                PathBuf::from("props/0_mug/mug_prop.png"),
                "GLOBAL prop bible (object only, no people): <mug>.".into(),
            ),
        ];
        let ranked = rank_world_pairs_for_frame(
            "Wide shot inside the coffee shop at night, steam rising",
            &pairs,
            2,
        );
        assert!(
            ranked[0]
                .0
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("coffee")
        );
    }

    #[test]
    fn world_assets_json_tolerates_name_aliases_and_defaults() {
        use crate::json_util::parse_llm_json;
        let raw = r#"{
          "environments":[{"idx":0,"name":"INT. STUDIO - DAY","description":"beige void"}],
          "props":[{"title":"red umbrella","description":"oil-paper"}]
        }"#;
        let spec: WorldAssetsSpec = parse_llm_json(raw).expect("parse");
        assert_eq!(spec.environments[0].slugline, "INT. STUDIO - DAY");
        assert_eq!(spec.props[0].name, "red umbrella");
    }
}
