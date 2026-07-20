//! Script2Video pipeline — plan text artifacts then render frames/clips/final.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    CameraImageGenerator, CharacterExtractor, CharacterPortraitsGenerator, ReferenceImageSelector,
    StoryboardArtist,
};
use crate::domain::{Camera, CharacterInScene, ShotBriefDescription, ShotDescription};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local;
use crate::progress::ProgressCallback;
use crate::session::{read_json_artifact, write_json_artifact, write_text_artifact};

use super::{
    PipelineBackends, emit, emit_pct, group_shots_into_cameras, load_or_write_json, safe_component,
};

pub struct Script2VideoPipeline {
    backends: PipelineBackends,
    working_dir: PathBuf,
    character_extractor: CharacterExtractor,
    portraits: CharacterPortraitsGenerator,
    storyboard: StoryboardArtist,
    camera_gen: CameraImageGenerator,
    ref_selector: ReferenceImageSelector,
}

impl Script2VideoPipeline {
    pub fn new(backends: PipelineBackends, working_dir: PathBuf) -> Self {
        let character_extractor = CharacterExtractor::new(Arc::clone(&backends.chat));
        let portraits = CharacterPortraitsGenerator::new(Arc::clone(&backends.image));
        let storyboard = StoryboardArtist::new(Arc::clone(&backends.chat));
        let camera_gen =
            CameraImageGenerator::new(Arc::clone(&backends.chat), Arc::clone(&backends.video));
        let ref_selector = ReferenceImageSelector::new(Arc::clone(&backends.chat));
        Self {
            backends,
            working_dir,
            character_extractor,
            portraits,
            storyboard,
            camera_gen,
            ref_selector,
        }
    }

    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    pub async fn plan_text_artifacts(
        &self,
        script: &str,
        user_requirement: &str,
        _style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<PlanArtifacts> {
        tokio::fs::create_dir_all(&self.working_dir).await?;
        write_text_artifact(&self.working_dir.join("script.txt"), script).await?;

        emit_pct(&progress, "extract_characters", "正在从剧本提取角色", 15.0);
        let characters = self.extract_characters(script).await?;

        emit_pct(&progress, "design_storyboard", "正在设计分镜表", 35.0);
        let storyboard = self
            .design_storyboard(script, &characters, user_requirement)
            .await?;

        emit_pct(&progress, "decompose_shots", "正在分解镜头视觉描述", 60.0);
        let shot_descriptions = self
            .decompose_visual_descriptions(&storyboard, &characters)
            .await?;

        emit_pct(&progress, "construct_camera_tree", "正在构建机位树", 85.0);
        let camera_tree = self.construct_camera_tree(&shot_descriptions).await?;

        emit_pct(&progress, "planned", "文本规划完成", 100.0);
        Ok(PlanArtifacts {
            characters,
            storyboard,
            shot_descriptions,
            camera_tree,
        })
    }

    pub async fn render(
        &self,
        script: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<PathBuf> {
        emit(&progress, "render_start", "开始渲染脚本成片");
        let final_path = self.working_dir.join("final_video.mp4");
        media_local::scrub_unusable_video(&final_path).await?;
        if media_local::is_usable_video_file(&final_path) {
            emit(
                &progress,
                "final_video_exists",
                "场景成片已存在，跳过本场景渲染",
            );
            return Ok(final_path);
        }

        let plan = self
            .plan_text_artifacts(script, user_requirement, style, progress.clone())
            .await?;

        emit(
            &progress,
            "character_portraits_start",
            "正在生成角色定妆图",
        );
        let registry = self
            .generate_character_portraits(&plan.characters, style, &progress)
            .await?;

        for shot in &plan.shot_descriptions {
            let shot_dir = self.working_dir.join("shots").join(shot.idx.to_string());
            tokio::fs::create_dir_all(&shot_dir).await?;
            write_json_artifact(&shot_dir.join("shot_description.json"), shot).await?;
        }

        emit(&progress, "frames_start", "正在生成镜头关键帧");
        self.generate_frames_parallel(
            &plan.camera_tree,
            &plan.shot_descriptions,
            &plan.characters,
            &registry,
            &progress,
        )
        .await?;

        emit(&progress, "video_clips_start", "正在生成镜头视频");
        self.generate_videos_parallel(&plan.shot_descriptions, &progress)
            .await?;

        if media_local::is_usable_video_file(&final_path) {
            emit(&progress, "final_video_exists", "场景成片已存在");
        } else {
            emit(&progress, "concat_start", "正在拼接镜头视频");
            let mut clips: Vec<PathBuf> = Vec::new();
            for shot in &plan.shot_descriptions {
                let clip = self
                    .working_dir
                    .join("shots")
                    .join(shot.idx.to_string())
                    .join("video.mp4");
                if !media_local::is_usable_video_file(&clip) {
                    return Err(VimaxError::Video(format!(
                        "无法拼接：镜头 {} 视频缺失或无效（可从断点继续补生成）",
                        shot.idx
                    )));
                }
                clips.push(clip);
            }
            let refs: Vec<&Path> = clips.iter().map(|p| p.as_path()).collect();
            media_local::concat_videos(&refs, &final_path).await?;
            emit(&progress, "concat_done", "场景成片拼接完成");
        }
        emit(&progress, "render_done", "脚本成片渲染完成");
        Ok(final_path)
    }

    async fn extract_characters(&self, script: &str) -> VimaxResult<Vec<CharacterInScene>> {
        let path = self.working_dir.join("characters.json");
        load_or_write_json(&path, || async {
            self.character_extractor.extract_characters(script).await
        })
        .await
    }

    async fn design_storyboard(
        &self,
        script: &str,
        characters: &[CharacterInScene],
        user_requirement: &str,
    ) -> VimaxResult<Vec<ShotBriefDescription>> {
        let path = self.working_dir.join("storyboard.json");
        load_or_write_json(&path, || async {
            self.storyboard
                .design_storyboard(script, characters, user_requirement)
                .await
        })
        .await
    }

    async fn decompose_visual_descriptions(
        &self,
        briefs: &[ShotBriefDescription],
        characters: &[CharacterInScene],
    ) -> VimaxResult<Vec<ShotDescription>> {
        let shots_root = self.working_dir.join("shots");
        tokio::fs::create_dir_all(&shots_root).await?;

        let mut pending = Vec::new();
        let mut existing: Vec<(i32, ShotDescription)> = Vec::new();
        for brief in briefs {
            let path = shots_root
                .join(brief.idx.to_string())
                .join("shot_description.json");
            if path.exists() {
                existing.push((brief.idx, read_json_artifact(&path).await?));
            } else {
                pending.push(brief.clone());
            }
        }

        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(5));
        for brief in pending {
            let storyboard = StoryboardArtist::new(Arc::clone(&self.backends.chat));
            let characters = characters.to_vec();
            let path = shots_root
                .join(brief.idx.to_string())
                .join("shot_description.json");
            let permit = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = permit
                    .acquire()
                    .await
                    .map_err(|_| VimaxError::msg("semaphore closed"))?;
                let desc = storyboard
                    .decompose_visual_description(&brief, &characters)
                    .await?;
                write_json_artifact(&path, &desc).await?;
                Ok::<_, VimaxError>((brief.idx, desc))
            });
        }
        while let Some(joined) = set.join_next().await {
            existing.push(joined.map_err(|e| VimaxError::msg(e.to_string()))??);
        }
        existing.sort_by_key(|(idx, _)| *idx);
        let out: Vec<_> = existing.into_iter().map(|(_, d)| d).collect();
        write_json_artifact(&self.working_dir.join("shot_descriptions.json"), &out).await?;
        Ok(out)
    }

    async fn construct_camera_tree(
        &self,
        shot_descriptions: &[ShotDescription],
    ) -> VimaxResult<Vec<Camera>> {
        let path = self.working_dir.join("camera_tree.json");
        load_or_write_json(&path, || async {
            let cameras = group_shots_into_cameras(shot_descriptions);
            self.camera_gen
                .construct_camera_tree(&cameras, shot_descriptions)
                .await
        })
        .await
    }

    async fn generate_character_portraits(
        &self,
        characters: &[CharacterInScene],
        style: &str,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<HashMap<String, HashMap<String, HashMap<String, String>>>> {
        let registry_path = self.working_dir.join("character_portraits_registry.json");
        let mut registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
            if registry_path.exists() {
                read_json_artifact(&registry_path).await?
            } else {
                HashMap::new()
            };

        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(4));
        for character in characters {
            if !character.is_visible {
                continue;
            }
            if registry.contains_key(&character.identifier_in_scene) {
                continue;
            }
            emit(
                progress,
                "character_portrait_start",
                &format!("generating portraits for {}", character.identifier_in_scene),
            );
            let dir = self.working_dir.join("character_portraits").join(format!(
                "{}_{}",
                character.idx,
                safe_component(&character.identifier_in_scene)
            ));
            let portraits = CharacterPortraitsGenerator::new(Arc::clone(&self.backends.image));
            let character = character.clone();
            let style = style.to_string();
            let permit = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = permit
                    .acquire()
                    .await
                    .map_err(|_| VimaxError::msg("semaphore closed"))?;
                portraits
                    .generate_all_views(&character, &style, &dir)
                    .await
            });
        }
        while let Some(joined) = set.join_next().await {
            let entry = joined.map_err(|e| VimaxError::msg(e.to_string()))??;
            registry.extend(entry);
            write_json_artifact(&registry_path, &registry).await?;
        }
        Ok(registry)
    }

    /// Parallel camera frame generation with parent-shot gates (ViMax asyncio Events).
    async fn generate_frames_parallel(
        &self,
        cameras: &[Camera],
        shots: &[ShotDescription],
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::sync::Notify;

        type Gate = Arc<(AtomicBool, Notify)>;
        let mut gates: HashMap<i32, Gate> = HashMap::new();
        for shot in shots {
            gates.insert(
                shot.idx,
                Arc::new((AtomicBool::new(false), Notify::new())),
            );
        }
        // Mark already-existing first frames ready.
        for shot in shots {
            let ff = self
                .working_dir
                .join("shots")
                .join(shot.idx.to_string())
                .join("first_frame.png");
            if ff.exists()
                && let Some(g) = gates.get(&shot.idx)
            {
                g.0.store(true, Ordering::SeqCst);
                g.1.notify_waiters();
            }
        }

        let mut set = tokio::task::JoinSet::new();
        for camera in cameras {
            let pipe = Script2VideoPipeline::new(self.backends.clone(), self.working_dir.clone());
            let camera = camera.clone();
            let shots = shots.to_vec();
            let characters = characters.to_vec();
            let registry = registry.clone();
            let gates = gates.clone();
            let progress = progress.clone();
            set.spawn(async move {
                // Wait for parent first frame if needed.
                if let Some(parent_shot) = camera.parent_shot_idx {
                    if let Some(gate) = gates.get(&parent_shot) {
                        while !gate.0.load(Ordering::SeqCst) {
                            gate.1.notified().await;
                        }
                    }
                }
                pipe.generate_frames_for_camera(
                    &camera,
                    &shots,
                    &characters,
                    &registry,
                    &progress,
                )
                .await?;
                // Signal this camera's first shot first_frame ready.
                if let Some(&first) = camera.active_shot_idxs.first()
                    && let Some(gate) = gates.get(&first)
                {
                    gate.0.store(true, Ordering::SeqCst);
                    gate.1.notify_waiters();
                }
                // Also mark subsequent shots' first frames if generated.
                for &idx in &camera.active_shot_idxs {
                    let ff = pipe
                        .working_dir
                        .join("shots")
                        .join(idx.to_string())
                        .join("first_frame.png");
                    if ff.exists()
                        && let Some(gate) = gates.get(&idx)
                    {
                        gate.0.store(true, Ordering::SeqCst);
                        gate.1.notify_waiters();
                    }
                }
                Ok::<_, VimaxError>(())
            });
        }
        while let Some(joined) = set.join_next().await {
            joined.map_err(|e| VimaxError::msg(e.to_string()))??;
        }
        Ok(())
    }

    async fn generate_videos_parallel(
        &self,
        shots: &[ShotDescription],
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        let total = shots.len();
        // Local queueing only — FlowyVideo also serializes vendor calls globally.
        // Do NOT early-return on first error: JoinSet drop aborts siblings and
        // wastes in-flight (already billed) generations.
        let mut set = tokio::task::JoinSet::new();
        for shot in shots {
            let pipe = Script2VideoPipeline::new(self.backends.clone(), self.working_dir.clone());
            let shot = shot.clone();
            let progress = progress.clone();
            set.spawn(async move { pipe.generate_video_for_shot(&shot, &progress).await });
        }

        let mut ok = 0usize;
        let mut errors: Vec<String> = Vec::new();
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(())) => ok += 1,
                Ok(Err(e)) => errors.push(e.to_string()),
                Err(e) => errors.push(format!("video task join: {e}")),
            }
        }

        if !errors.is_empty() {
            emit(
                progress,
                "video_clips_partial",
                &format!(
                    "镜头视频部分完成：成功 {ok}/{total}，失败 {}（已落盘的片段可断点续跑）",
                    errors.len()
                ),
            );
            return Err(VimaxError::Video(format!(
                "镜头视频生成部分失败：成功 {ok}/{total}，失败 {}。已保存成功片段，请从断点继续（不会重复扣已成功镜头的积分）。\n{}",
                errors.len(),
                errors.join("\n")
            )));
        }
        emit(
            progress,
            "video_clips_done",
            &format!("全部镜头视频已就绪（{ok}/{total}）"),
        );
        Ok(())
    }

    async fn generate_frames_for_camera(
        &self,
        camera: &Camera,
        shots: &[ShotDescription],
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        if camera.active_shot_idxs.is_empty() {
            return Ok(());
        }
        let first_shot_idx = camera.active_shot_idxs[0];
        let first_shot = shots
            .iter()
            .find(|s| s.idx == first_shot_idx)
            .ok_or_else(|| VimaxError::msg(format!("missing shot {first_shot_idx}")))?;

        let shot_dir = self
            .working_dir
            .join("shots")
            .join(first_shot_idx.to_string());
        tokio::fs::create_dir_all(&shot_dir).await?;
        let first_ff = shot_dir.join("first_frame.png");

        if !first_ff.exists() {
            emit(
                progress,
                "frame_start",
                &format!("Generating first frame for shot {first_shot_idx}"),
            );
            let mut available: Vec<(PathBuf, String)> = Vec::new();
            for &ci in &first_shot.ff_vis_char_idxs {
                if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
                    if let Some(views) = registry.get(&ch.identifier_in_scene) {
                        for item in views.values() {
                            if let (Some(p), Some(d)) = (item.get("path"), item.get("description"))
                            {
                                available.push((PathBuf::from(p), d.clone()));
                            }
                        }
                    }
                }
            }

            if let Some(parent_shot_idx) = camera.parent_shot_idx {
                let parent_ff = self
                    .working_dir
                    .join("shots")
                    .join(parent_shot_idx.to_string())
                    .join("first_frame.png");
                if parent_ff.exists() {
                    let transition = shot_dir.join(format!(
                        "transition_video_from_shot_{parent_shot_idx}.mp4"
                    ));
                    if !transition.exists() {
                        let parent_shot = shots
                            .iter()
                            .find(|s| s.idx == parent_shot_idx)
                            .ok_or_else(|| {
                                VimaxError::msg(format!("missing parent shot {parent_shot_idx}"))
                            })?;
                        self.camera_gen
                            .generate_transition_video(
                                &parent_shot.visual_desc,
                                &first_shot.visual_desc,
                                &parent_ff,
                                &transition,
                            )
                            .await?;
                    }
                    let new_cam = shot_dir.join(format!("new_camera_{}.png", camera.idx));
                    if !new_cam.exists() {
                        self.camera_gen
                            .get_new_camera_image(&transition, &new_cam)
                            .await?;
                    }
                    available.push((
                        new_cam,
                        format!(
                            "The composition and background are correct but some elements may be wrong. Wrong elements: {}. You must select this image as the main reference.",
                            camera.missing_info.as_deref().unwrap_or("")
                        ),
                    ));
                }
            }

            if camera.parent_shot_idx.is_none() || camera.missing_info.is_some() {
                self.generate_frame_from_selector(
                    &shot_dir,
                    "first_frame",
                    &first_shot.ff_desc,
                    &available,
                    &first_ff,
                    progress,
                )
                .await?;
            } else if let Some((_, _)) = available.last() {
                // Fully covered by parent — copy new camera image.
                let new_cam = shot_dir.join(format!("new_camera_{}.png", camera.idx));
                if new_cam.exists() {
                    tokio::fs::copy(&new_cam, &first_ff).await?;
                } else {
                    self.generate_frame_from_selector(
                        &shot_dir,
                        "first_frame",
                        &first_shot.ff_desc,
                        &available,
                        &first_ff,
                        progress,
                    )
                    .await?;
                }
            }
        }

        // Remaining frames for this camera.
        let first_pair = (
            first_ff.clone(),
            first_shot.ff_desc.clone(),
        );
        let need_last = |s: &ShotDescription| {
            matches!(s.variation_type.as_str(), "medium" | "large")
        };

        if need_last(first_shot) {
            let lf = shot_dir.join("last_frame.png");
            if !lf.exists() {
                let mut available = portrait_pairs(characters, &first_shot.lf_vis_char_idxs, registry);
                available.push(first_pair.clone());
                self.generate_frame_from_selector(
                    &shot_dir,
                    "last_frame",
                    &first_shot.lf_desc,
                    &available,
                    &lf,
                    progress,
                )
                .await?;
            }
        }

        for &shot_idx in camera.active_shot_idxs.iter().skip(1) {
            let shot = shots
                .iter()
                .find(|s| s.idx == shot_idx)
                .ok_or_else(|| VimaxError::msg(format!("missing shot {shot_idx}")))?;
            let sdir = self.working_dir.join("shots").join(shot_idx.to_string());
            tokio::fs::create_dir_all(&sdir).await?;

            let ff = sdir.join("first_frame.png");
            if !ff.exists() {
                let mut available = portrait_pairs(characters, &shot.ff_vis_char_idxs, registry);
                available.push(first_pair.clone());
                self.generate_frame_from_selector(
                    &sdir,
                    "first_frame",
                    &shot.ff_desc,
                    &available,
                    &ff,
                    progress,
                )
                .await?;
            }
            if need_last(shot) {
                let lf = sdir.join("last_frame.png");
                if !lf.exists() {
                    let mut available = portrait_pairs(characters, &shot.lf_vis_char_idxs, registry);
                    available.push(first_pair.clone());
                    self.generate_frame_from_selector(
                        &sdir,
                        "last_frame",
                        &shot.lf_desc,
                        &available,
                        &lf,
                        progress,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn generate_frame_from_selector(
        &self,
        shot_dir: &Path,
        frame_type: &str,
        frame_desc: &str,
        available: &[(PathBuf, String)],
        out_path: &Path,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        let selector_path = shot_dir.join(format!("{frame_type}_selector_output.json"));
        let (pairs, prompt) = if selector_path.exists() {
            #[derive(serde::Deserialize)]
            struct Saved {
                reference_image_path_and_text_pairs: Vec<(String, String)>,
                text_prompt: String,
            }
            let saved: Saved = read_json_artifact(&selector_path).await?;
            (
                saved
                    .reference_image_path_and_text_pairs
                    .into_iter()
                    .map(|(p, t)| (PathBuf::from(p), t))
                    .collect::<Vec<_>>(),
                saved.text_prompt,
            )
        } else {
            emit(
                progress,
                "frame_prompt_start",
                &format!("Selecting references for {frame_type}"),
            );
            let sel = self
                .ref_selector
                .select_reference_images_and_generate_prompt(available, frame_desc)
                .await?;
            let saved_pairs: Vec<(String, String)> = sel
                .reference_image_path_and_text_pairs
                .iter()
                .map(|(p, t)| (p.to_string_lossy().to_string(), t.clone()))
                .collect();
            write_json_artifact(
                &selector_path,
                &serde_json::json!({
                    "reference_image_path_and_text_pairs": saved_pairs,
                    "text_prompt": sel.text_prompt,
                }),
            )
            .await?;
            (sel.reference_image_path_and_text_pairs, sel.text_prompt)
        };

        let mut prefix = String::new();
        for (i, (_, text)) in pairs.iter().enumerate() {
            prefix.push_str(&format!("Image {i}: {text}\n"));
        }
        let full_prompt = format!("{prefix}\n{prompt}");
        let refs: Vec<&Path> = pairs.iter().map(|(p, _)| p.as_path()).collect();
        self.backends
            .image
            .generate(&full_prompt, &refs, out_path)
            .await?;
        emit(
            progress,
            "frame_done",
            &format!("Generated {frame_type} at {}", out_path.display()),
        );
        Ok(())
    }

    async fn generate_video_for_shot(
        &self,
        shot: &ShotDescription,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        let shot_dir = self.working_dir.join("shots").join(shot.idx.to_string());
        tokio::fs::create_dir_all(&shot_dir).await?;
        let video_path = shot_dir.join("video.mp4");
        media_local::scrub_unusable_video(&video_path).await?;
        if media_local::is_usable_video_file(&video_path) {
            emit(
                progress,
                "video_clip_exists",
                &format!("镜头 {} 视频已存在，跳过生成（不重复计费）", shot.idx),
            );
            return Ok(());
        }
        let ff = shot_dir.join("first_frame.png");
        if !ff.exists() {
            return Err(VimaxError::msg(format!(
                "first_frame missing for shot {}",
                shot.idx
            )));
        }
        let lf = shot_dir.join("last_frame.png");
        let last = if matches!(shot.variation_type.as_str(), "medium" | "large") && lf.exists() {
            Some(lf.as_path())
        } else {
            None
        };
        let prompt = format!(
            "{}\n{}",
            shot.motion_desc,
            shot.audio_desc.as_deref().unwrap_or("")
        );
        emit(
            progress,
            "video_clip_start",
            &format!("正在生成镜头 {} 视频（排队/限流中可能稍慢）", shot.idx),
        );
        self.backends
            .video
            .generate(&prompt, Some(&ff), last, &[], 5, &video_path)
            .await?;
        if !media_local::is_usable_video_file(&video_path) {
            return Err(VimaxError::Video(format!(
                "镜头 {} 视频生成后文件无效",
                shot.idx
            )));
        }
        emit(
            progress,
            "video_clip_done",
            &format!("镜头 {} 视频已保存", shot.idx),
        );
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlanArtifacts {
    pub characters: Vec<CharacterInScene>,
    pub storyboard: Vec<ShotBriefDescription>,
    pub shot_descriptions: Vec<ShotDescription>,
    pub camera_tree: Vec<Camera>,
}

fn portrait_pairs(
    characters: &[CharacterInScene],
    idxs: &[i32],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
) -> Vec<(PathBuf, String)> {
    let mut available = Vec::new();
    for &ci in idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            if let Some(views) = registry.get(&ch.identifier_in_scene) {
                for item in views.values() {
                    if let (Some(p), Some(d)) = (item.get("path"), item.get("description")) {
                        available.push((PathBuf::from(p), d.clone()));
                    }
                }
            }
        }
    }
    available
}
