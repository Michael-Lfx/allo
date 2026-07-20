//! Idea2Video — develop story → multi-scene scripts → per-scene Script2Video.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{CharacterExtractor, CharacterPortraitsGenerator, Screenwriter};
use crate::error::VimaxResult;
use crate::media_local;
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

    pub async fn plan_text_artifacts(
        &self,
        idea: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        tokio::fs::create_dir_all(&self.working_dir).await?;
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

        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            tokio::fs::create_dir_all(&scene_dir).await?;
            write_text_artifact(&scene_dir.join("script.txt"), scene_script).await?;
        }

        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(3));
        let scene_total = scenes.len().max(1);
        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            let backends = self.backends.clone();
            let scene_script = scene_script.clone();
            let user_requirement = user_requirement.to_string();
            let style = style.to_string();
            let permit = Arc::clone(&sem);
            let pct = 55.0 + 40.0 * (i as f32 / scene_total as f32);
            emit_pct(
                &progress,
                "plan_scene",
                &format!("正在规划场景文本产物（{}/{scene_total}）", i + 1),
                pct,
            );
            set.spawn(async move {
                let _permit = permit
                    .acquire_owned()
                    .await
                    .map_err(|_| crate::error::VimaxError::msg("semaphore closed"))?;
                let s2v = Script2VideoPipeline::new(backends, scene_dir);
                s2v.plan_text_artifacts(&scene_script, &user_requirement, &style, None)
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
        let characters: Vec<crate::domain::CharacterInScene> =
            serde_json::from_str(&tokio::fs::read_to_string(self.working_dir.join("characters.json")).await?)?;

        // Global portraits at idea root.
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

        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(2));
        let scene_total = scenes.len().max(1);
        let mut scene_videos: Vec<(usize, PathBuf)> = Vec::new();
        let mut pending = 0usize;
        for (i, scene_script) in scenes.iter().enumerate() {
            let scene_dir = self.working_dir.join(format!("scene_{i}"));
            let scene_final = scene_dir.join("final_video.mp4");
            media_local::scrub_unusable_video(&scene_final).await?;
            if media_local::is_usable_video_file(&scene_final) {
                emit_pct(
                    &progress,
                    "render_scene_skip",
                    &format!("场景 {}/{scene_total} 已完成，跳过", i + 1),
                    20.0 + 70.0 * (i as f32 / scene_total as f32),
                );
                scene_videos.push((i, scene_final));
                continue;
            }
            let scene_reg = scene_dir.join("character_portraits_registry.json");
            if !scene_reg.exists() && registry_path.exists() {
                tokio::fs::create_dir_all(&scene_dir).await?;
                tokio::fs::copy(&registry_path, &scene_reg).await?;
            }
            let backends = self.backends.clone();
            let scene_script = scene_script.clone();
            let user_requirement = user_requirement.to_string();
            let style = style.to_string();
            let permit = Arc::clone(&sem);
            let pct = 20.0 + 70.0 * (i as f32 / scene_total as f32);
            emit_pct(
                &progress,
                "render_scene",
                &format!("正在渲染场景（{}/{scene_total}）· 含图片与视频模型", i + 1),
                pct,
            );
            pending += 1;
            set.spawn(async move {
                let _permit = permit
                    .acquire_owned()
                    .await
                    .map_err(|_| crate::error::VimaxError::msg("semaphore closed"))?;
                let s2v = Script2VideoPipeline::new(backends, scene_dir);
                let video = s2v
                    .render(&scene_script, &user_requirement, &style, None)
                    .await?;
                Ok::<_, crate::error::VimaxError>((i, video))
            });
        }
        if pending > 0 {
            emit_pct(
                &progress,
                "render_resume",
                &format!("从断点继续：待渲染 {pending}/{scene_total} 个场景"),
                22.0,
            );
        }
        let mut scene_errors: Vec<String> = Vec::new();
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(pair)) => scene_videos.push(pair),
                Ok(Err(e)) => scene_errors.push(e.to_string()),
                Err(e) => scene_errors.push(format!("scene join: {e}")),
            }
        }
        if !scene_errors.is_empty() {
            return Err(crate::error::VimaxError::Video(format!(
                "场景渲染部分失败：成功 {}/{}，失败 {}。已完成场景已落盘，可从断点继续。\n{}",
                scene_videos.len(),
                scene_total,
                scene_errors.len(),
                scene_errors.join("\n")
            )));
        }
        scene_videos.sort_by_key(|(i, _)| *i);
        let scene_videos: Vec<PathBuf> = scene_videos.into_iter().map(|(_, p)| p).collect();

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
