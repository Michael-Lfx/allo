//! Idea2Video — develop story → multi-scene scripts → per-scene Script2Video.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{CharacterExtractor, CharacterPortraitsGenerator, Screenwriter};
use crate::error::VimaxResult;
use crate::media_local;
use crate::planning::{
    allocate_scene_budgets, enrich_requirement_for_scene, normalize_target_duration_secs,
    DEFAULT_TARGET_DURATION_SECS,
};
use crate::progress::ProgressCallback;
use crate::session::{write_json_artifact, write_text_artifact};

use super::script2video::Script2VideoPipeline;
use super::{PipelineBackends, emit_pct, load_or_write_json, load_or_write_text, safe_component};

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

    async fn film_target_secs(&self) -> u32 {
        let p = self.working_dir.join("target_duration_secs.txt");
        if let Ok(text) = tokio::fs::read_to_string(&p).await {
            if let Ok(n) = text.trim().parse::<u32>() {
                if n > 0 {
                    return normalize_target_duration_secs(Some(n));
                }
            }
        }
        DEFAULT_TARGET_DURATION_SECS
    }

    /// Share film-level cast + per-scene duration budget so storyboards stay consistent.
    async fn prepare_scene_workspace(&self, scene_dir: &Path, budget: u32) -> VimaxResult<()> {
        tokio::fs::create_dir_all(scene_dir).await?;
        write_text_artifact(
            &scene_dir.join("target_duration_secs.txt"),
            &budget.to_string(),
        )
        .await?;

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

        emit_pct(&progress, "extract_characters", "正在从故事中提取角色", 30.0);
        let _characters = load_or_write_json(&self.working_dir.join("characters.json"), || async {
            self.character_extractor.extract_characters(&story).await
        })
        .await?;

        emit_pct(&progress, "write_script", "正在撰写分场剧本", 50.0);
        let scenes: Vec<String> =
            load_or_write_json(&self.working_dir.join("script.json"), || async {
                self.screenwriter
                    .write_script_based_on_story(&story, user_requirement)
                    .await
            })
            .await?;

        let scene_count = scenes.len().max(1);
        let budgets = allocate_scene_budgets(film_total, scene_count);

        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            write_text_artifact(&scene_dir.join("script.txt"), scene_script).await?;
            let budget = budgets
                .get(i)
                .copied()
                .unwrap_or(DEFAULT_TARGET_DURATION_SECS);
            self.prepare_scene_workspace(&scene_dir, budget).await?;
        }

        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(3));
        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            let backends = self.backends.clone();
            let scene_script = scene_script.clone();
            let style = style.to_string();
            let budget = budgets
                .get(i)
                .copied()
                .unwrap_or(DEFAULT_TARGET_DURATION_SECS);
            // Scene-level constraint only (film block already applied to story/script).
            let scene_req = enrich_requirement_for_scene(
                user_requirement,
                budget,
                i,
                scene_count,
                film_total,
            );
            let permit = Arc::clone(&sem);
            let pct = 55.0 + 40.0 * (i as f32 / scene_count as f32);
            emit_pct(
                &progress,
                "plan_scene",
                &format!("正在规划场景文本产物（{}/{scene_count}）", i + 1),
                pct,
            );
            set.spawn(async move {
                let _permit = permit
                    .acquire_owned()
                    .await
                    .map_err(|_| crate::error::VimaxError::msg("semaphore closed"))?;
                // Re-copy cast/budget in the worker in case of races with parallel prep.
                write_text_artifact(
                    &scene_dir.join("target_duration_secs.txt"),
                    &budget.to_string(),
                )
                .await?;
                let s2v = Script2VideoPipeline::new(backends, scene_dir);
                s2v.plan_text_artifacts(&scene_script, &scene_req, &style, None)
                    .await?;
                Ok::<_, crate::error::VimaxError>(())
            });
        }
        while let Some(joined) = set.join_next().await {
            joined.map_err(|e| crate::error::VimaxError::msg(e.to_string()))??;
        }
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
            self.plan_text_artifacts(idea, user_requirement, style, progress.clone())
                .await?;
        }

        let story = tokio::fs::read_to_string(&story_path).await?;
        let characters: Vec<crate::domain::CharacterInScene> = serde_json::from_str(
            &tokio::fs::read_to_string(self.working_dir.join("characters.json")).await?,
        )?;

        // Global portraits at idea root — single source of truth for all scenes.
        let registry_path = self.working_dir.join("character_portraits_registry.json");
        if !registry_path.exists() {
            emit_pct(
                &progress,
                "character_portraits_start",
                "正在生成角色定妆图（图片模型）",
                12.0,
            );
            let mut registry = serde_json::Map::new();
            for character in &characters {
                if !character.is_visible {
                    continue;
                }
                let dir = self.working_dir.join("character_portraits").join(format!(
                    "{}_{}",
                    character.idx,
                    safe_component(&character.identifier_in_scene)
                ));
                let entry = self
                    .portraits
                    .generate_all_views(character, style, &dir)
                    .await?;
                for (k, v) in entry {
                    registry.insert(k, serde_json::to_value(v)?);
                }
            }
            write_json_artifact(&registry_path, &registry).await?;
        }
        let _ = story;

        let scenes: Vec<String> =
            serde_json::from_str(&tokio::fs::read_to_string(&script_path).await?)?;

        let film_total = self.film_target_secs().await;
        let scene_total = scenes.len().max(1);
        let budgets = allocate_scene_budgets(film_total, scene_total);

        // Sequential scenes so a mid-failure surfaces immediately (no stuck JoinSet wait)
        // and progress keeps moving. Per-shot videos are also sequential + fail-fast.
        let mut scene_videos: Vec<PathBuf> = Vec::new();
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
                scene_videos.push(scene_final);
                continue;
            }

            let budget = budgets
                .get(i)
                .copied()
                .unwrap_or(DEFAULT_TARGET_DURATION_SECS);
            self.prepare_scene_workspace(&scene_dir, budget).await?;
            let scene_req = enrich_requirement_for_scene(
                user_requirement,
                budget,
                i,
                scene_total,
                film_total,
            );

            let pct = 20.0 + 70.0 * (i as f32 / scene_total as f32);
            emit_pct(
                &progress,
                "render_scene",
                &format!("正在渲染场景（{}/{scene_total}）· 含图片与视频模型", i + 1),
                pct,
            );
            let s2v = Script2VideoPipeline::new(self.backends.clone(), scene_dir);
            match s2v
                .render(scene_script, &scene_req, style, progress.clone())
                .await
            {
                Ok(video) => {
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
                            "场景 {}/{scene_total} 失败；已完成 {} 个场景已落盘，可从断点继续",
                            i + 1,
                            scene_videos.len()
                        ),
                        pct,
                    );
                    return Err(crate::error::VimaxError::Video(format!(
                        "场景 {}/{scene_total} 渲染失败（此前已完成 {} 个场景已落盘，可从断点继续）：{e}",
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
