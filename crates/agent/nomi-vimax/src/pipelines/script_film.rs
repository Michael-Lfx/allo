//! Multi-scene film orchestration for Script2Video.
//!
//! Reuses per-scene [`Script2VideoPipeline`] the same way Idea2Video does:
//! split screenplay → film-level cast/world → `scene_i/` plan+render → concat.
//! Single-unit scripts stay on the legacy flat film-root path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    CharacterExtractor, CharacterPortraitsGenerator, VoiceProfileGenerator, WorldAssetsPlanner,
    ensure_film_cover, has_usable_portrait,
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
    apply_session_cameos, cameo_extractor_hint, resolve_session_root, world_cameo_context,
};
use super::script2video::{resolve_scene_tail_continuity, PlanArtifacts, Script2VideoPipeline};
use super::script_scene_split::{
    apply_script_selection, build_scenes_index, resolve_script_selection, selected_script_bodies,
    split_screenplay, ScriptSelection, ScreenplayUnit,
};
use super::{
    PipelineBackends, emit_pct, emit_pct_meta, load_or_write_json, safe_component,
};

pub struct ScriptFilmPipeline {
    backends: PipelineBackends,
    working_dir: PathBuf,
    character_extractor: CharacterExtractor,
    portraits: CharacterPortraitsGenerator,
}

impl ScriptFilmPipeline {
    pub fn new(backends: PipelineBackends, working_dir: PathBuf) -> Self {
        Self {
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
        let local_portraits = scene_dir.join("character_portraits");
        if local_portraits.is_dir() {
            let _ = tokio::fs::remove_dir_all(&local_portraits).await;
        }
        Ok(())
    }

    fn scope_note(selection: &ScriptSelection, selected: &[ScreenplayUnit]) -> String {
        let headings: Vec<&str> = selected
            .iter()
            .map(|u| u.heading.as_str())
            .take(8)
            .collect();
        format!(
            "[SCRIPT_SCOPE — MUST FOLLOW]\n\
             - {}.\n\
             - This run films {} selected screenplay unit(s): {}.\n\
             - Adapt ONLY these units into shots. Do NOT invent later episodes or skipped scenes.",
            selection.note,
            selected.len(),
            headings.join(" | ")
        )
    }

    /// Plan: split → select → film cast/world → per-scene storyboards.
    ///
    /// One selected unit keeps the legacy flat `script2video/` layout so short
    /// scripts and older sessions stay simple.
    pub async fn plan_text_artifacts(
        &self,
        script: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<Option<PlanArtifacts>> {
        tokio::fs::create_dir_all(&self.working_dir).await?;
        write_text_artifact(&self.working_dir.join("script.txt"), script).await?;
        let style = crate::planning::resolve_visual_style(style);
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;

        let split = split_screenplay(script);
        let selection = resolve_script_selection(user_requirement, &split);
        let selected = apply_script_selection(&split, &selection);
        let index = build_scenes_index(&split, &selection, &selected);
        write_json_artifact(&self.working_dir.join("scenes_index.json"), &index).await?;
        write_json_artifact(&self.working_dir.join("selection.json"), &selection).await?;

        tracing::info!(
            total_units = split.units.len(),
            selected = selected.len(),
            from_user = selection.from_user,
            note = %selection.note,
            "script2video screenplay split/selection"
        );

        if split.units.len() <= 1 {
            // True single-beat script — legacy flat film-root layout.
            let s2v = Script2VideoPipeline::new(self.backends.clone(), self.working_dir.clone());
            let plan = s2v
                .plan_text_artifacts(script, user_requirement, &style, progress)
                .await?;
            write_json_artifact(
                &self.working_dir.join("script.json"),
                &vec![script.to_string()],
            )
            .await?;
            return Ok(Some(plan));
        }

        // Multi-unit screenplay: always scene_i/ layout (even when only one unit is selected).
        let bodies = selected_script_bodies(&selected);
        write_json_artifact(&self.working_dir.join("script.json"), &bodies).await?;

        let film_total = self.film_target_secs().await;
        let corpus = split.extract_corpus(&selected);
        let scope = Self::scope_note(&selection, &selected);

        emit_pct(&progress, "extract_characters", "正在从选定场次提取角色", 12.0);
        let session_root = resolve_session_root(&self.working_dir);
        let cameo_hint = cameo_extractor_hint(&session_root);
        let mut characters = load_or_write_json(&self.working_dir.join("characters.json"), || async {
            let text = format!("{corpus}{cameo_hint}");
            self.character_extractor
                .extract_characters(&text, &style)
                .await
        })
        .await?;

        emit_pct(&progress, "voice_profiles", "正在标定角色声音特征", 15.0);
        {
            let voice_gen = VoiceProfileGenerator::new(Arc::clone(&self.backends.chat));
            if voice_gen
                .ensure_voice_profiles(&mut characters, &corpus, &style)
                .await?
            {
                write_json_artifact(&self.working_dir.join("characters.json"), &characters).await?;
            }
        }

        emit_pct(
            &progress,
            "cameo_bind",
            "正在绑定用户角色参考图并做隐私安全换脸",
            18.0,
        );
        apply_session_cameos(
            &self.working_dir,
            &characters,
            Arc::clone(&self.backends.image),
        )
        .await?;

        let world_planner = WorldAssetsPlanner::new(
            Arc::clone(&self.backends.chat),
            Arc::clone(&self.backends.image),
        );
        let look_theme = crate::planning::portrait_theme_excerpt(&corpus);
        emit_pct(&progress, "look_plate_start", "正在锁定全片画风", 20.0);
        let look_refs = world_planner
            .look_style_refs(&self.working_dir, &style, &look_theme)
            .await;
        let look_ref_paths: Vec<&Path> = look_refs.iter().map(|p| p.as_path()).collect();

        emit_pct(
            &progress,
            "character_portraits_start",
            "正在生成全局角色定妆图",
            22.0,
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
                    .generate_all_views(character, &style, &corpus, &dir, &look_ref_paths)
                    .await?;
                registry.extend(entry);
                write_json_artifact(&registry_path, &registry).await?;
            }
            write_json_artifact(&registry_path, &registry).await?;
        }

        emit_pct(
            &progress,
            "world_assets_start",
            "正在生成全局环境与道具参考图",
            30.0,
        );
        {
            let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
            let _ = world_planner
                .ensure(
                    &self.working_dir,
                    &corpus,
                    &style,
                    &style_refs,
                    &scene_hint,
                    &lock_token,
                )
                .await?;
        }

        let scene_count = bodies.len().max(1);
        let budgets = film_total.map(|total| allocate_scene_budgets(total, scene_count));

        for (i, scene_script) in bodies.iter().enumerate() {
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
            &format!("正在规划 0/{scene_count} 个场次文本产物"),
            40.0,
        );
        for (i, scene_script) in bodies.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            let backends = self.backends.clone();
            let scene_script = scene_script.clone();
            let style = style.to_string();
            let budget = budgets.as_ref().and_then(|b| b.get(i).copied());
            let mut scene_req = match (budget, film_total) {
                (Some(budget), Some(film_total)) => enrich_requirement_for_scene(
                    user_requirement,
                    budget,
                    i,
                    scene_count,
                    film_total,
                ),
                _ => enrich_requirement_for_scene_model_decides(user_requirement, i, scene_count),
            };
            scene_req = format!("{scene_req}\n\n{scope}");
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
                let pct = 40.0 + 55.0 * (finished as f32 / scene_count as f32);
                emit_pct(
                    &progress,
                    "plan_scene",
                    &format!("正在规划场次文本产物（{finished}/{scene_count}）"),
                    pct,
                );
                Ok::<_, crate::error::VimaxError>(())
            });
        }
        while let Some(joined) = set.join_next().await {
            joined.map_err(|e| crate::error::VimaxError::msg(e.to_string()))??;
        }

        let synopsis = format!("{corpus}\n{user_requirement}");
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

        emit_pct(
            &progress,
            "planned",
            &format!("文本规划完成（{} 场）", scene_count),
            100.0,
        );
        Ok(None)
    }

    pub async fn render(
        &self,
        script: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<PathBuf> {
        emit_pct(&progress, "render_start", "开始渲染剧本成片", 2.0);
        let style = crate::planning::resolve_visual_style(style);
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;

        let script_json = self.working_dir.join("script.json");
        let scenes_index = self.working_dir.join("scenes_index.json");

        if !script_json.exists() || !scenes_index.exists() {
            self.plan_text_artifacts(script, user_requirement, &style, progress.clone())
                .await?;
        }

        let use_scene_dirs = self.working_dir.join("scene_0").is_dir()
            && multi_scene_or_selected_script_json(&script_json).await;

        if !use_scene_dirs {
            let s2v = Script2VideoPipeline::new(self.backends.clone(), self.working_dir.clone());
            return s2v
                .render(script, user_requirement, &style, progress)
                .await;
        }

        let scenes: Vec<String> =
            serde_json::from_str(&tokio::fs::read_to_string(&script_json).await?)?;
        let film_total = self.film_target_secs().await;
        let scene_total = scenes.len().max(1);
        let budgets = film_total.map(|total| allocate_scene_budgets(total, scene_total));

        let selection: ScriptSelection = read_json_artifact(&self.working_dir.join("selection.json"))
            .await
            .unwrap_or_else(|_| resolve_script_selection(user_requirement, &split_screenplay(script)));
        let split = split_screenplay(script);
        let selected = apply_script_selection(&split, &selection);
        let scope = Self::scope_note(&selection, &selected);

        let characters: Vec<crate::domain::CharacterInScene> = serde_json::from_str(
            &tokio::fs::read_to_string(self.working_dir.join("characters.json")).await?,
        )?;
        apply_session_cameos(
            &self.working_dir,
            &characters,
            Arc::clone(&self.backends.image),
        )
        .await?;

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
                    &format!("场次 {}/{scene_total} 已完成，跳过", i + 1),
                    20.0 + 70.0 * ((i + 1) as f32 / scene_total as f32),
                );
                prior_continuity = resolve_scene_tail_continuity(&scene_dir).await;
                scene_videos.push(scene_final);
                continue;
            }

            let budget = budgets.as_ref().and_then(|b| b.get(i).copied());
            self.prepare_scene_workspace(&scene_dir, budget).await?;
            let mut scene_req = match (budget, film_total) {
                (Some(budget), Some(film_total)) => enrich_requirement_for_scene(
                    user_requirement,
                    budget,
                    i,
                    scene_total,
                    film_total,
                ),
                _ => enrich_requirement_for_scene_model_decides(user_requirement, i, scene_total),
            };
            scene_req = format!("{scene_req}\n\n{scope}");

            let pct = 20.0 + 70.0 * (i as f32 / scene_total as f32);
            emit_pct_meta(
                &progress,
                "render_scene",
                &format!("正在渲染场次（{}/{scene_total}）", i + 1),
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
                        &format!("场次 {}/{scene_total} 渲染完成", i + 1),
                        20.0 + 70.0 * ((i + 1) as f32 / scene_total as f32),
                    );
                }
                Err(e) => {
                    return Err(crate::error::VimaxError::Video(format!(
                        "场次 {}/{scene_total} 渲染失败（已完成 {} 场，可从断点续跑）: {e}",
                        i + 1,
                        scene_videos.len()
                    )));
                }
            }
        }

        let final_path = self.working_dir.join("final_video.mp4");
        media_local::scrub_unusable_video(&final_path).await?;
        if !media_local::is_usable_video_file(&final_path) {
            emit_pct(&progress, "concat_start", "正在拼接各场次视频", 95.0);
            let refs: Vec<&Path> = scene_videos.iter().map(|p| p.as_path()).collect();
            media_local::concat_videos(&refs, &final_path).await?;
        }
        emit_pct(&progress, "render_done", "剧本成片渲染完成", 100.0);
        Ok(final_path)
    }
}

async fn multi_scene_or_selected_script_json(path: &Path) -> bool {
    let Ok(text) = tokio::fs::read_to_string(path).await else {
        return false;
    };
    let Ok(scenes) = serde_json::from_str::<Vec<String>>(&text) else {
        return false;
    };
    // scene_0 layout is used whenever we split a multi-unit bible (len >= 1 with scene dirs).
    !scenes.is_empty()
}
