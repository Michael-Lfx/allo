//! Idea2Video — develop story → multi-scene scripts → per-scene Script2Video.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    CharacterExtractor, CharacterPortraitsGenerator, Screenwriter, VoiceProfileGenerator,
    VoiceReferenceGenerator, WorldAssetsPlanner, ensure_film_cover, has_usable_portrait,
};
use crate::error::VimaxResult;
use crate::media_local;
use crate::planning::{
    allocate_scene_budgets, enrich_requirement_for_scene,
    enrich_requirement_for_scene_model_decides, normalize_target_duration_secs,
};
use crate::progress::ProgressCallback;
use crate::session::{read_json_artifact, write_json_artifact, write_text_artifact};

use super::cameo_bind::{
    apply_session_cameos, cameo_extractor_hint, classify_session_references, resolve_session_root,
    world_cameo_context,
};
use super::script2video::{resolve_scene_tail_continuity, Script2VideoPipeline};
use super::{
    PipelineBackends, emit_pct, emit_pct_meta, load_or_write_json, load_or_write_text, safe_component,
};

pub struct Idea2VideoPipeline {
    backends: PipelineBackends,
    working_dir: PathBuf,
    screenwriter: Screenwriter,
    character_extractor: CharacterExtractor,
    portraits: CharacterPortraitsGenerator,
}

impl Idea2VideoPipeline {
    pub fn new(backends: PipelineBackends, working_dir: PathBuf) -> Self {
        Self {
            screenwriter: Screenwriter::new(Arc::clone(&backends.chat)),
            character_extractor: CharacterExtractor::new(Arc::clone(&backends.chat)),
            portraits: CharacterPortraitsGenerator::new(Arc::clone(&backends.image)),
            backends,
            working_dir,
        }
    }

    async fn film_target_secs(&self) -> Option<u32> {
        let p = self.working_dir.join("target_duration_secs.txt");
        if let Ok(text) = tokio::fs::read_to_string(&p).await {
            if let Ok(n) = text.trim().parse::<u32>() {
                if n > 0 {
                    return Some(normalize_target_duration_secs(Some(n)));
                }
            }
        }
        None
    }

    /// Share film-level cast + optional per-scene duration budget.
    async fn prepare_scene_workspace(&self, scene_dir: &Path, budget: Option<u32>) -> VimaxResult<()> {
        tokio::fs::create_dir_all(scene_dir).await?;
        if let Some(budget) = budget {
            write_text_artifact(
                &scene_dir.join("target_duration_secs.txt"),
                &budget.to_string(),
            )
            .await?;
        }

        let root_chars = self.working_dir.join("characters.json");
        let scene_chars = scene_dir.join("characters.json");
        if root_chars.exists() {
            let mut cast_changed = !scene_chars.exists();
            if scene_chars.exists() {
                let a = tokio::fs::read(&root_chars).await.unwrap_or_default();
                let b = tokio::fs::read(&scene_chars).await.unwrap_or_default();
                cast_changed = a != b;
            }
            tokio::fs::copy(&root_chars, &scene_chars).await?;
            // Scene-local storyboards / shot lists tied to a divergent cast must be rebuilt.
            if cast_changed {
                for name in [
                    "storyboard.json",
                    "shot_descriptions.json",
                    "camera_tree.json",
                ] {
                    let p = scene_dir.join(name);
                    if p.exists() {
                        let _ = tokio::fs::remove_file(&p).await;
                    }
                }
                let shots = scene_dir.join("shots");
                if shots.is_dir() {
                    let _ = tokio::fs::remove_dir_all(&shots).await;
                }
            }
        }

        let root_reg = self.working_dir.join("character_portraits_registry.json");
        if root_reg.exists() {
            tokio::fs::copy(
                &root_reg,
                scene_dir.join("character_portraits_registry.json"),
            )
            .await?;
        }
        let root_world = self.working_dir.join("world_assets_registry.json");
        if root_world.exists() {
            tokio::fs::copy(
                &root_world,
                scene_dir.join("world_assets_registry.json"),
            )
            .await?;
        }
        // Portraits live only at film root — drop any leftover scene-local sheets.
        let local_portraits = scene_dir.join("character_portraits");
        if local_portraits.is_dir() {
            let _ = tokio::fs::remove_dir_all(&local_portraits).await;
        }
        Ok(())
    }

    pub async fn plan_text_artifacts(
        &self,
        idea: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        tokio::fs::create_dir_all(&self.working_dir).await?;
        let film_total = self.film_target_secs().await;

        emit_pct(&progress, "develop_story", "正在根据灵感扩写故事", 10.0);
        let story = load_or_write_text(&self.working_dir.join("story.txt"), || async {
            self.screenwriter
                .develop_story(idea, user_requirement)
                .await
        })
        .await?;

        let style = crate::planning::resolve_visual_style(style);
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;

        let session_root = resolve_session_root(&self.working_dir);
        emit_pct(
            &progress,
            "classify_references",
            "正在识别用户上传参考图类型",
            22.0,
        );
        let _ = classify_session_references(
            &session_root,
            Arc::clone(&self.backends.chat),
        )
        .await?;

        emit_pct(&progress, "extract_characters", "正在从故事中提取角色", 25.0);
        let cameo_hint = cameo_extractor_hint(&session_root);
        let mut characters = load_or_write_json(&self.working_dir.join("characters.json"), || async {
            let story_for_extract = format!("{story}{cameo_hint}");
            self.character_extractor
                .extract_characters(&story_for_extract, &style)
                .await
        })
        .await?;

        emit_pct(&progress, "voice_profiles", "正在标定角色声音特征", 28.0);
        {
            let voice_gen = VoiceProfileGenerator::new(Arc::clone(&self.backends.chat));
            if voice_gen
                .ensure_voice_profiles(&mut characters, &story, &style)
                .await?
            {
                write_json_artifact(&self.working_dir.join("characters.json"), &characters).await?;
            }
        }

        emit_pct(
            &progress,
            "cameo_bind",
            "正在绑定用户角色参考图（有真人脸才做隐私换脸）",
            30.0,
        );
        apply_session_cameos(
            &self.working_dir,
            &characters,
            Arc::clone(&self.backends.image),
            Arc::clone(&self.backends.chat),
        )
        .await?;

        let world_planner = WorldAssetsPlanner::new(
            Arc::clone(&self.backends.chat),
            Arc::clone(&self.backends.image),
        );
        let look_theme = crate::planning::portrait_theme_excerpt(&story);
        emit_pct(&progress, "look_plate_start", "正在锁定全片画风", 33.0);
        let look_refs = world_planner
            .look_style_refs(&self.working_dir, &style, &look_theme)
            .await;
        let look_ref_paths: Vec<&Path> = look_refs.iter().map(|p| p.as_path()).collect();

        // Global cast bible during planning (before per-scene storyboards).
        emit_pct(
            &progress,
            "character_portraits_start",
            "正在生成全局角色定妆图",
            35.0,
        );
        {
            let registry_path = self.working_dir.join("character_portraits_registry.json");
            let mut registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
                if registry_path.exists() {
                    read_json_artifact(&registry_path).await.unwrap_or_default()
                } else {
                    HashMap::new()
                };
            for character in &characters {
                if !character.is_visible {
                    continue;
                }
                if has_usable_portrait(&registry, &character.identifier_in_scene) {
                    continue;
                }
                registry.remove(&character.identifier_in_scene);
                let dir = self.working_dir.join("character_portraits").join(format!(
                    "{}_{}",
                    character.idx,
                    safe_component(&character.identifier_in_scene)
                ));
                let entry = self
                    .portraits
                    .generate_all_views(character, &style, &story, &dir, &look_ref_paths)
                    .await?;
                registry.extend(entry);
                write_json_artifact(&registry_path, &registry).await?;
            }
            write_json_artifact(&registry_path, &registry).await?;
            emit_pct(
                &progress,
                "character_portraits_done",
                "全局角色定妆图已就绪",
                42.0,
            );
        }

        emit_pct(
            &progress,
            "voice_references_start",
            "正在生成角色音色参考音频",
            43.0,
        );
        if let Some(flowy) = self.backends.flowy.clone() {
            let registry_path = self.working_dir.join("character_portraits_registry.json");
            let mut registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
                if registry_path.exists() {
                    read_json_artifact(&registry_path).await.unwrap_or_default()
                } else {
                    HashMap::new()
                };
            let portraits_dir = self.working_dir.join("character_portraits");
            let voice_gen = VoiceReferenceGenerator::new(flowy);
            if let Ok(n) = voice_gen
                .ensure_voice_references(&characters, &portraits_dir, &mut registry)
                .await
            {
                let _ = write_json_artifact(&registry_path, &registry).await;
                if n > 0 {
                    emit_pct(
                        &progress,
                        "voice_references_done",
                        &format!("已生成 {n} 条音色参考"),
                        44.0,
                    );
                }
            }
        }

        emit_pct(
            &progress,
            "world_assets_start",
            "正在生成全局环境与道具参考图",
            45.0,
        );
        {
            let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
            // Prefer full story for location/prop coverage across scenes.
            // When Cameo photos exist, plates are style-locked to those uploads.
            let _ = world_planner
                .ensure(
                    &self.working_dir,
                    &story,
                    &style,
                    &style_refs,
                    &scene_hint,
                    &lock_token,
                )
                .await?;
            emit_pct(
                &progress,
                "world_assets_done",
                "全局环境与道具参考图已就绪",
                48.0,
            );
        }

        emit_pct(&progress, "write_script", "正在撰写分场剧本", 52.0);
        let mut scenes: Vec<String> =
            load_or_write_json(&self.working_dir.join("script.json"), || async {
                self.screenwriter
                    .write_script_based_on_story(&story, user_requirement)
                    .await
            })
            .await?;

        if let Some(film_total) = film_total {
            let max_scenes = crate::planning::max_scenes_for_budget(film_total);
            if scenes.len() > max_scenes {
                tracing::warn!(
                    kept = max_scenes,
                    dropped = scenes.len() - max_scenes,
                    film_total,
                    "truncated idea script scenes to respect film duration budget"
                );
                scenes.truncate(max_scenes);
                write_json_artifact(&self.working_dir.join("script.json"), &scenes).await?;
            }
        }

        let scene_count = scenes.len().max(1);
        let budgets = film_total.map(|total| allocate_scene_budgets(total, scene_count));

        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            write_text_artifact(&scene_dir.join("script.txt"), scene_script).await?;
            let budget = budgets.as_ref().and_then(|b| b.get(i).copied());
            self.prepare_scene_workspace(&scene_dir, budget).await?;
        }

        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(3));
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        emit_pct(
            &progress,
            "plan_scene",
            &format!("正在规划 0/{scene_count} 个场景文本产物"),
            55.0,
        );
        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            let backends = self.backends.clone();
            let scene_script = scene_script.clone();
            let style = style.to_string();
            let budget = budgets.as_ref().and_then(|b| b.get(i).copied());
            let scene_req = match (budget, film_total) {
                (Some(budget), Some(film_total)) => enrich_requirement_for_scene(
                    user_requirement,
                    budget,
                    i,
                    scene_count,
                    film_total,
                ),
                _ => enrich_requirement_for_scene_model_decides(user_requirement, i, scene_count),
            };
            let permit = Arc::clone(&sem);
            let progress = progress.clone();
            let done = Arc::clone(&done);
            set.spawn(async move {
                let _permit = permit
                    .acquire_owned()
                    .await
                    .map_err(|_| crate::error::VimaxError::msg("semaphore closed"))?;
                if let Some(budget) = budget {
                    write_text_artifact(
                        &scene_dir.join("target_duration_secs.txt"),
                        &budget.to_string(),
                    )
                    .await?;
                }
                let s2v = Script2VideoPipeline::new(backends, scene_dir);
                s2v.plan_text_artifacts(&scene_script, &scene_req, &style, None)
                    .await?;
                let finished = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let pct = 55.0 + 40.0 * (finished as f32 / scene_count as f32);
                emit_pct(
                    &progress,
                    "plan_scene",
                    &format!("正在规划场景文本产物（{finished}/{scene_count}）"),
                    pct,
                );
                Ok::<_, crate::error::VimaxError>(())
            });
        }
        while let Some(joined) = set.join_next().await {
            joined.map_err(|e| crate::error::VimaxError::msg(e.to_string()))??;
        }

        let synopsis = format!("{idea}\n{story}\n{user_requirement}");
        let cover_aspect = crate::aspect::load_aspect_from_dir(&self.working_dir).await;
        let _ = ensure_film_cover(
            &self.working_dir,
            Arc::clone(&self.backends.chat),
            self.backends.poster_image(&cover_aspect),
            &style,
            &synopsis,
            &progress,
        )
        .await;

        Ok(())
    }

    pub async fn render(
        &self,
        idea: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<PathBuf> {
        emit_pct(&progress, "render_start", "开始渲染灵感成片", 2.0);
        let style = crate::planning::resolve_visual_style(style);
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;
        let story_path = self.working_dir.join("story.txt");
        let script_path = self.working_dir.join("script.json");
        if story_path.exists() && script_path.exists() {
            emit_pct(
                &progress,
                "reuse_plan",
                "复用已有规划产物，跳过文本规划",
                8.0,
            );
        } else {
            self.plan_text_artifacts(idea, user_requirement, &style, progress.clone())
                .await?;
        }

        let story = tokio::fs::read_to_string(&story_path).await?;
        let characters: Vec<crate::domain::CharacterInScene> = serde_json::from_str(
            &tokio::fs::read_to_string(self.working_dir.join("characters.json")).await?,
        )?;

        apply_session_cameos(
            &self.working_dir,
            &characters,
            Arc::clone(&self.backends.image),
            Arc::clone(&self.backends.chat),
        )
        .await?;

        let world_planner = WorldAssetsPlanner::new(
            Arc::clone(&self.backends.chat),
            Arc::clone(&self.backends.image),
        );
        let look_theme = crate::planning::portrait_theme_excerpt(&story);
        let look_refs = world_planner
            .look_style_refs(&self.working_dir, &style, &look_theme)
            .await;
        let look_ref_paths: Vec<&Path> = look_refs.iter().map(|p| p.as_path()).collect();

        // Global portraits at idea root — single source of truth for all scenes.
        let registry_path = self.working_dir.join("character_portraits_registry.json");
        {
            let mut registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
                if registry_path.exists() {
                    read_json_artifact(&registry_path).await.unwrap_or_default()
                } else {
                    HashMap::new()
                };
            let mut missing = false;
            for character in &characters {
                if !character.is_visible {
                    continue;
                }
                if has_usable_portrait(&registry, &character.identifier_in_scene) {
                    continue;
                }
                missing = true;
                break;
            }
            if missing {
                emit_pct(
                    &progress,
                    "character_portraits_start",
                    "正在生成角色定妆图（图片模型）",
                    12.0,
                );
                for character in &characters {
                    if !character.is_visible {
                        continue;
                    }
                    if has_usable_portrait(&registry, &character.identifier_in_scene) {
                        continue;
                    }
                    registry.remove(&character.identifier_in_scene);
                    let dir = self.working_dir.join("character_portraits").join(format!(
                        "{}_{}",
                        character.idx,
                        safe_component(&character.identifier_in_scene)
                    ));
                    let entry = self
                        .portraits
                        .generate_all_views(character, &style, &story, &dir, &look_ref_paths)
                        .await?;
                    registry.extend(entry);
                    write_json_artifact(&registry_path, &registry).await?;
                }
                write_json_artifact(&registry_path, &registry).await?;
            }
        }

        let scenes: Vec<String> =
            serde_json::from_str(&tokio::fs::read_to_string(&script_path).await?)?;

        let film_total = self.film_target_secs().await;
        let scene_total = scenes.len().max(1);
        let budgets = film_total.map(|total| allocate_scene_budgets(total, scene_total));

        // Sequential scenes so a mid-failure surfaces immediately (no stuck JoinSet wait)
        // and progress keeps moving. Per-shot videos are also sequential + fail-fast.
        // Cross-scene: scene N+1's first shot match-cuts from scene N's last video_last_frame.
        let mut scene_videos: Vec<PathBuf> = Vec::new();
        let mut prior_continuity: Option<PathBuf> = None;
        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            let scene_final = scene_dir.join("final_video.mp4");
            media_local::scrub_unusable_video(&scene_final).await?;
            if media_local::is_usable_video_file(&scene_final) {
                emit_pct(
                    &progress,
                    "render_scene_skip",
                    &format!("场景 {}/{scene_total} 已完成，跳过", i + 1),
                    20.0 + 70.0 * ((i + 1) as f32 / scene_total as f32),
                );
                prior_continuity = resolve_scene_tail_continuity(&scene_dir).await;
                scene_videos.push(scene_final);
                continue;
            }

            let budget = budgets.as_ref().and_then(|b| b.get(i).copied());
            self.prepare_scene_workspace(&scene_dir, budget).await?;
            let scene_req = match (budget, film_total) {
                (Some(budget), Some(film_total)) => enrich_requirement_for_scene(
                    user_requirement,
                    budget,
                    i,
                    scene_total,
                    film_total,
                ),
                _ => enrich_requirement_for_scene_model_decides(user_requirement, i, scene_total),
            };

            let pct = 20.0 + 70.0 * (i as f32 / scene_total as f32);
            emit_pct_meta(
                &progress,
                "render_scene",
                &format!("正在渲染场景（{}/{scene_total}）· 含图片与视频模型", i + 1),
                pct,
                serde_json::json!({ "scene_idx": i }),
            );
            let s2v = Script2VideoPipeline::new(self.backends.clone(), scene_dir.clone());
            match s2v
                .render_with_prior_continuity(
                    scene_script,
                    &scene_req,
                    &style,
                    progress.clone(),
                    prior_continuity.as_deref(),
                )
                .await
            {
                Ok(video) => {
                    prior_continuity = resolve_scene_tail_continuity(&scene_dir).await;
                    scene_videos.push(video);
                    emit_pct(
                        &progress,
                        "render_scene_done",
                        &format!("场景 {}/{scene_total} 渲染完成", i + 1),
                        20.0 + 70.0 * ((i + 1) as f32 / scene_total as f32),
                    );
                }
                Err(e) => {
                    emit_pct(
                        &progress,
                        "render_scene_failed",
                        &format!(
                            "Scene {}/{scene_total} failed; {} scene(s) already on disk — resume from checkpoint",
                            i + 1,
                            scene_videos.len()
                        ),
                        pct,
                    );
                    return Err(crate::error::VimaxError::Video(format!(
                        "Scene {}/{scene_total} render failed ({} scene(s) already on disk — resume from checkpoint): {e}",
                        i + 1,
                        scene_videos.len()
                    )));
                }
            }
        }

        let final_path = self.working_dir.join("final_video.mp4");
        media_local::scrub_unusable_video(&final_path).await?;
        if !media_local::is_usable_video_file(&final_path) {
            emit_pct(&progress, "concat_start", "正在拼接各场景视频", 95.0);
            let refs: Vec<&Path> = scene_videos.iter().map(|p| p.as_path()).collect();
            media_local::concat_videos(&refs, &final_path).await?;
        }
        emit_pct(&progress, "render_done", "灵感成片渲染完成", 100.0);
        Ok(final_path)
    }
}
