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
    PipelineBackends, emit, emit_pct, group_shots_into_cameras, load_or_write_json,
    resolve_film_root, safe_component, sanitize_camera_tree,
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
        let camera_gen = CameraImageGenerator::new(Arc::clone(&backends.chat));
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

    fn cancel_requested(&self) -> bool {
        self.backends.is_cancelled()
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

        emit(&progress, "frames_start", "正在按机位顺序生成关键帧");
        self.generate_frames_sequential(
            &plan.camera_tree,
            &plan.shot_descriptions,
            &plan.characters,
            &registry,
            &progress,
        )
        .await?;

        emit(&progress, "video_clips_start", "正在串行生成镜头视频（一次一个）");
        self.generate_videos_sequential(&plan.shot_descriptions, &progress)
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
        let film_root = resolve_film_root(&self.working_dir);
        let film_chars = film_root.join("characters.json");
        let path = self.working_dir.join("characters.json");
        // Always prefer the film-level cast so every scene/shot shares identifiers.
        if film_chars.exists() {
            if film_chars != path {
                tokio::fs::copy(&film_chars, &path).await?;
            }
        } else if !path.exists() {
            if let Some(parent) = self.working_dir.parent() {
                let parent_chars = parent.join("characters.json");
                if parent_chars.exists() {
                    tokio::fs::copy(&parent_chars, &path).await?;
                }
            }
        }
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
        let mut storyboard = load_or_write_json(&path, || async {
            self.storyboard
                .design_storyboard(script, characters, user_requirement)
                .await
        })
        .await?;
        let budget = load_target_duration_secs(&self.working_dir)
            .await
            .unwrap_or(crate::planning::DEFAULT_TARGET_DURATION_SECS);
        let max_shots = crate::planning::max_shots_for_budget(budget);
        if enforce_max_shots(&mut storyboard, max_shots) {
            tracing::warn!(
                max_shots,
                kept = storyboard.len(),
                "truncated storyboard to respect duration budget"
            );
            write_json_artifact(&path, &storyboard).await?;
            // Shot decompositions must be rebuilt for the truncated board.
            let decomp = self.working_dir.join("shot_descriptions.json");
            if decomp.exists() {
                let _ = tokio::fs::remove_file(&decomp).await;
            }
            let cam = self.working_dir.join("camera_tree.json");
            if cam.exists() {
                let _ = tokio::fs::remove_file(&cam).await;
            }
            let keep: std::collections::HashSet<i32> =
                storyboard.iter().map(|s| s.idx).collect();
            let shots_root = self.working_dir.join("shots");
            if shots_root.is_dir() {
                if let Ok(mut entries) = tokio::fs::read_dir(&shots_root).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        if let Ok(idx) = name.parse::<i32>() {
                            if !keep.contains(&idx) {
                                let _ = tokio::fs::remove_dir_all(entry.path()).await;
                            }
                        }
                    }
                }
            }
        }
        Ok(storyboard)
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
        let mut cameras = load_or_write_json(&path, || async {
            let cameras = group_shots_into_cameras(shot_descriptions);
            self.camera_gen
                .construct_camera_tree(&cameras, shot_descriptions)
                .await
        })
        .await?;
        // Always sanitize — cached trees from earlier LLM output may self-reference.
        sanitize_camera_tree(&mut cameras);
        write_json_artifact(&path, &cameras).await?;
        Ok(cameras)
    }

    /// Load/create portraits only under the **film root**. Every scene/shot reuses the
    /// same registry paths so identity stays consistent across the final cut.
    async fn generate_character_portraits(
        &self,
        characters: &[CharacterInScene],
        style: &str,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<HashMap<String, HashMap<String, HashMap<String, String>>>> {
        let film_root = resolve_film_root(&self.working_dir);
        let registry_path = film_root.join("character_portraits_registry.json");
        let portraits_dir = film_root.join("character_portraits");
        tokio::fs::create_dir_all(&portraits_dir).await?;

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
                &format!(
                    "generating global portraits for {}",
                    character.identifier_in_scene
                ),
            );
            let dir = portraits_dir.join(format!(
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
        write_json_artifact(&registry_path, &registry).await?;

        // Scene workspaces only keep a mirror of the global registry (paths still point
        // at film-root PNGs). Drop any stale scene-local portrait folders.
        if film_root != self.working_dir {
            write_json_artifact(
                &self.working_dir.join("character_portraits_registry.json"),
                &registry,
            )
            .await?;
            let local_portraits = self.working_dir.join("character_portraits");
            if local_portraits.is_dir() {
                let _ = tokio::fs::remove_dir_all(&local_portraits).await;
            }
        }
        Ok(registry)
    }

    /// Generate frames camera-by-camera in dependency order (parent before child).
    /// Avoids the parallel Notify race that could hang forever with no progress updates.
    async fn generate_frames_sequential(
        &self,
        cameras: &[Camera],
        shots: &[ShotDescription],
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        use std::collections::HashSet;

        // Defensive: clear self-parent edges before scheduling (also covers stale cache).
        let mut cameras = cameras.to_vec();
        sanitize_camera_tree(&mut cameras);

        let mut done_shots: HashSet<i32> = HashSet::new();
        for shot in shots {
            let ff = self
                .working_dir
                .join("shots")
                .join(shot.idx.to_string())
                .join("first_frame.png");
            if ff.exists() {
                done_shots.insert(shot.idx);
            }
        }

        let mut remaining: Vec<Camera> = cameras;
        let total = remaining.len().max(1);
        let mut finished = 0usize;

        while !remaining.is_empty() {
            if self.cancel_requested() {
                emit(
                    progress,
                    "frames_cancelled",
                    &format!("已取消关键帧生成；已完成机位 {finished}/{total}"),
                );
                return Err(VimaxError::Cancelled);
            }

            let ready_idx = remaining.iter().position(|cam| match cam.parent_shot_idx {
                None => true,
                Some(parent) => {
                    // Ready if parent frame exists, OR parent shot is owned by this
                    // camera (should already be sanitized away).
                    done_shots.contains(&parent) || cam.active_shot_idxs.contains(&parent)
                }
            });

            let Some(ready_idx) = ready_idx else {
                // Last resort: promote the first remaining camera to root and continue
                // instead of hard-failing a whole scene over a bad tree edge.
                let mut cam = remaining.remove(0);
                tracing::warn!(
                    camera = cam.idx,
                    parent_shot = ?cam.parent_shot_idx,
                    shots = ?cam.active_shot_idxs,
                    "forcing camera to root to break frame-generation deadlock"
                );
                cam.parent_cam_idx = None;
                cam.parent_shot_idx = None;
                emit(
                    progress,
                    "frame_camera_force_root",
                    &format!(
                        "机位 {} 父镜头不可达，改为独立机位继续生成",
                        cam.idx
                    ),
                );
                let pct = 35.0 + 20.0 * (finished as f32 / total as f32);
                emit_pct(
                    progress,
                    "frame_camera_start",
                    &format!(
                        "生成机位关键帧（{}/{}）· camera {} · shots {:?}",
                        finished + 1,
                        total,
                        cam.idx,
                        cam.active_shot_idxs
                    ),
                    pct,
                );
                self.generate_frames_for_camera(&cam, shots, characters, registry, progress)
                    .await?;
                for &idx in &cam.active_shot_idxs {
                    let ff = self
                        .working_dir
                        .join("shots")
                        .join(idx.to_string())
                        .join("first_frame.png");
                    if ff.exists() {
                        done_shots.insert(idx);
                    }
                }
                if let Some(&first) = cam.active_shot_idxs.first() {
                    done_shots.insert(first);
                }
                finished += 1;
                continue;
            };

            let camera = remaining.remove(ready_idx);
            let pct = 35.0 + 20.0 * (finished as f32 / total as f32);
            emit_pct(
                progress,
                "frame_camera_start",
                &format!(
                    "生成机位关键帧（{}/{}）· camera {} · shots {:?}",
                    finished + 1,
                    total,
                    camera.idx,
                    camera.active_shot_idxs
                ),
                pct,
            );

            self.generate_frames_for_camera(&camera, shots, characters, registry, progress)
                .await?;

            for &idx in &camera.active_shot_idxs {
                let ff = self
                    .working_dir
                    .join("shots")
                    .join(idx.to_string())
                    .join("first_frame.png");
                if ff.exists() {
                    done_shots.insert(idx);
                }
            }
            if let Some(&first) = camera.active_shot_idxs.first() {
                done_shots.insert(first);
            }

            finished += 1;
            emit_pct(
                progress,
                "frame_camera_done",
                &format!("机位 {} 关键帧完成（{finished}/{total}）", camera.idx),
                35.0 + 20.0 * (finished as f32 / total as f32),
            );
        }

        emit_pct(progress, "frames_done", "全部机位关键帧已就绪", 55.0);
        Ok(())
    }

    /// Submit video-generation API calls one-by-one.
    /// On failure/cancel, stop immediately; already-saved clips remain for resume.
    async fn generate_videos_sequential(
        &self,
        shots: &[ShotDescription],
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        let total = shots.len().max(1);
        let mut ok = 0usize;
        let mut errors: Vec<String> = Vec::new();
        for (i, shot) in shots.iter().enumerate() {
            if self.cancel_requested() {
                emit(
                    progress,
                    "video_clips_cancelled",
                    &format!("已取消；镜头视频成功落盘 {ok}/{}", shots.len()),
                );
                return Err(VimaxError::Cancelled);
            }
            let pct = 55.0 + 40.0 * (i as f32 / total as f32);
            emit_pct(
                progress,
                "video_clip_start",
                &format!("串行生成镜头视频（{}/{}）· 镜头 {}", i + 1, total, shot.idx),
                pct,
            );
            match self
                .generate_video_for_shot(shot, shots.len(), progress)
                .await
            {
                Ok(()) => {
                    ok += 1;
                    emit_pct(
                        progress,
                        "video_clip_done",
                        &format!("镜头 {} 已就绪（完成 {}/{}）", shot.idx, ok, total),
                        55.0 + 40.0 * ((i + 1) as f32 / total as f32),
                    );
                }
                Err(e) => {
                    errors.push(format!("镜头 {}: {e}", shot.idx));
                    emit_pct(
                        progress,
                        "video_clips_partial",
                        &format!(
                            "镜头 {} 失败；已成功 {}/{}。停止后续提交，可从断点继续。",
                            shot.idx, ok, total
                        ),
                        pct,
                    );
                    break;
                }
            }
        }

        if !errors.is_empty() {
            return Err(VimaxError::Video(format!(
                "镜头视频生成失败：成功 {ok}/{}，未再提交后续镜头。已保存成功片段，请从断点继续（不会重复扣已成功镜头的积分）。\n{}",
                shots.len(),
                errors.join("\n")
            )));
        }
        emit_pct(
            progress,
            "video_clips_done",
            &format!("全部镜头视频已就绪（{ok}/{}）", shots.len()),
            95.0,
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
                &format!("generating first frame for shot {first_shot_idx}"),
            );
            let mut available: Vec<(PathBuf, String)> =
                portrait_pairs(characters, &first_shot.ff_vis_char_idxs, registry);

            // Prefer image continuity from parent frame over a billed transition video.
            if let Some(parent_shot_idx) = camera.parent_shot_idx {
                if let Some(parent_ref) =
                    continuity_frame_path(&self.working_dir, parent_shot_idx)
                {
                    let missing = camera.missing_info.as_deref().unwrap_or("");
                    available.push((
                        parent_ref,
                        format!(
                            "Parent-camera continuity frame (most recent). Keep identity, wardrobe, lighting, and style; reframe to the NEW camera angle for this shot. Changed/missing elements vs parent: {}",
                            if missing.is_empty() {
                                "none — change framing/angle only"
                            } else {
                                missing
                            }
                        ),
                    ));
                    emit(
                        progress,
                        "frame_start",
                        &format!(
                            "shot {first_shot_idx}: using parent shot {parent_shot_idx} frame (skip transition video)"
                        ),
                    );
                }
            }

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

        // Same-camera shots chain from the previous ending frame (not the establishing first_frame).
        let mut continuity = ContinuityRef {
            path: first_ff.clone(),
            desc: first_shot.ff_desc.clone(),
        };
        let need_last = |s: &ShotDescription| {
            matches!(s.variation_type.as_str(), "medium" | "large")
        };

        if need_last(first_shot) {
            let lf = shot_dir.join("last_frame.png");
            if !lf.exists() {
                let mut available =
                    portrait_pairs(characters, &first_shot.lf_vis_char_idxs, registry);
                available.push((
                    continuity.path.clone(),
                    format!(
                        "Immediate previous frame on this camera (continuity). {}",
                        continuity.desc
                    ),
                ));
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
            if lf.exists() {
                continuity = ContinuityRef {
                    path: lf,
                    desc: first_shot.lf_desc.clone(),
                };
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
                // Always generate a distinct first frame. Byte-copying the previous
                // frame makes Seedance I2V freeze on the same opening for ~4s.
                let mut available =
                    portrait_pairs(characters, &shot.ff_vis_char_idxs, registry);
                available.push((
                    continuity.path.clone(),
                    format!(
                        "Immediate previous shot ending frame (prefer this for temporal continuity; do not reset to an older establishing shot). Evolve pose/action slightly for the new beat. {}",
                        continuity.desc
                    ),
                ));
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
                    let mut available =
                        portrait_pairs(characters, &shot.lf_vis_char_idxs, registry);
                    available.push((
                        ff.clone(),
                        format!(
                            "This shot's first frame (continuity within the shot). {}",
                            shot.ff_desc
                        ),
                    ));
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

            // Advance continuity to this shot's ending frame.
            let this_lf = sdir.join("last_frame.png");
            if this_lf.exists() {
                continuity = ContinuityRef {
                    path: this_lf,
                    desc: shot.lf_desc.clone(),
                };
            } else if ff.exists() {
                continuity = ContinuityRef {
                    path: ff,
                    desc: shot.ff_desc.clone(),
                };
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
        // Prefer shot continuity frames as Image 0 so the text-only image model
        // sees the temporal-continuity hint before the 800-char truncate.
        let mut ordered = pairs;
        ordered.sort_by_key(|(p, _)| {
            let s = p.to_string_lossy();
            if s.contains("shots") {
                0u8
            } else {
                1u8
            }
        });
        for (i, (_, text)) in ordered.iter().enumerate() {
            prefix.push_str(&format!("Image {i}: {text}\n"));
        }
        let continuity_hint = if ordered.is_empty() {
            String::new()
        } else {
            "Match identity/style of Image 0; keep temporal continuity with the latest prior frame. "
                .to_string()
        };
        // Seedance rejects photoreal faces (PrivacyInformation). Force stylized look.
        let style_guard =
            "Stylized cinematic illustration / animated film look, clearly drawn characters. ";
        let full_prompt = format!("{style_guard}{continuity_hint}{prefix}\n{prompt}");
        let refs: Vec<&Path> = ordered.iter().map(|(p, _)| p.as_path()).collect();
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
        shot_count: usize,
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
        let use_last = matches!(shot.variation_type.as_str(), "medium" | "large") && lf.exists();
        let prompt = i2v_motion_prompt(shot);
        let target = load_target_duration_secs(&self.working_dir).await;
        let duration_secs = crate::planning::clip_duration_secs(target, shot_count);
        emit(
            progress,
            "video_clip_start",
            &format!(
                "正在生成镜头 {} 视频（{}s，排队/限流中可能稍慢）",
                shot.idx, duration_secs
            ),
        );

        let last_ref = if use_last { Some(lf.as_path()) } else { None };
        let first_err = match self
            .backends
            .video
            .generate(&prompt, Some(&ff), last_ref, &[], duration_secs, &video_path)
            .await
        {
            Ok(()) => None,
            Err(err) if should_retry_seedance_without_photoreal_frame(&err) => Some(err),
            Err(err) => return Err(err),
        };

        if let Some(err) = first_err {
            emit(
                progress,
                "video_clip_start",
                &format!(
                    "镜头 {} 疑似首帧真人/隐私拦截（{}）。正在用插画风文本重绘首帧后重试…",
                    shot.idx,
                    truncate_err(&err, 120)
                ),
            );
            // Drop last_frame for retry — it often shares the same faces.
            if lf.exists() {
                let bak = shot_dir.join("last_frame.privacy_bak.png");
                let _ = tokio::fs::rename(&lf, &bak).await;
            }
            self.regenerate_stylized_first_frame(shot, &ff).await?;

            let retry_i2v = self
                .backends
                .video
                .generate(&prompt, Some(&ff), None, &[], duration_secs, &video_path)
                .await;

            if let Err(retry_err) = retry_i2v {
                if !should_retry_seedance_without_photoreal_frame(&retry_err)
                    && !is_seedance_privacy_image_err(&retry_err)
                {
                    return Err(retry_err);
                }
                // Final fallback: text-to-video without any input image (bypasses image privacy).
                emit(
                    progress,
                    "video_clip_start",
                    &format!(
                        "镜头 {} 插画首帧仍被拦截，降级为无参考图文生视频（绕过真人图检测）…",
                        shot.idx
                    ),
                );
                let t2v_prompt = format!(
                    "{}\nOpening scene (stylized animated illustration): {}",
                    prompt, shot.ff_desc
                );
                self.backends
                    .video
                    .generate(&t2v_prompt, None, None, &[], duration_secs, &video_path)
                    .await
                    .map_err(|t2v_err| {
                        VimaxError::Video(format!(
                            "镜头 {} 视频生成失败（首帧真人拦截 → 插画重绘 → 无图降级均失败）。首次：{}；最终：{t2v_err}",
                            shot.idx,
                            truncate_err(&err, 160)
                        ))
                    })?;
            }
        }

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

    /// Text-only redraw — Z-Image cannot img2img, so restyle-from-ref is useless.
    async fn regenerate_stylized_first_frame(
        &self,
        shot: &ShotDescription,
        frame_path: &Path,
    ) -> VimaxResult<()> {
        if frame_path.exists() {
            let bak = frame_path.with_extension("privacy_bak.png");
            let _ = tokio::fs::rename(frame_path, &bak).await;
        }
        let prompt = format!(
            "Stylized cinematic illustration / animated film still, clearly drawn painted characters, anime or storybook look. Wide 16:9. Scene: {}",
            shot.ff_desc
        );
        self.backends
            .image
            .generate(&prompt, &[], frame_path)
            .await
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
                for (view, item) in views {
                    if let Some(p) = item.get("path") {
                        available.push((
                            PathBuf::from(p),
                            format!(
                                "GLOBAL character bible ({view} view) for <{}>. Lock face, hair, age, body type, wardrobe colors, and art style to this sheet across every scene and shot. Do not redesign the character.",
                                ch.identifier_in_scene
                            ),
                        ));
                    }
                }
            }
        }
    }
    available
}

struct ContinuityRef {
    path: PathBuf,
    desc: String,
}

fn continuity_frame_path(working_dir: &Path, shot_idx: i32) -> Option<PathBuf> {
    let dir = working_dir.join("shots").join(shot_idx.to_string());
    let lf = dir.join("last_frame.png");
    if lf.exists() {
        return Some(lf);
    }
    let ff = dir.join("first_frame.png");
    if ff.exists() {
        Some(ff)
    } else {
        None
    }
}

async fn load_target_duration_secs(working_dir: &Path) -> Option<u32> {
    for dir in [working_dir, working_dir.parent().unwrap_or(working_dir)] {
        let p = dir.join("target_duration_secs.txt");
        if let Ok(text) = tokio::fs::read_to_string(&p).await {
            if let Ok(n) = text.trim().parse::<u32>() {
                if n > 0 {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Keep at most `max_shots` briefs; reindex and mark the final shot as `is_last`.
fn enforce_max_shots(shots: &mut Vec<ShotBriefDescription>, max_shots: usize) -> bool {
    let max_shots = max_shots.max(1);
    if shots.len() <= max_shots {
        return false;
    }
    shots.truncate(max_shots);
    let last_i = shots.len().saturating_sub(1);
    for (i, s) in shots.iter_mut().enumerate() {
        s.idx = i as i32;
        s.is_last = i == last_i;
    }
    true
}

fn i2v_motion_prompt(shot: &ShotDescription) -> String {
    let motion = shot.motion_desc.trim();
    let audio = shot.audio_desc.as_deref().unwrap_or("").trim();
    format!(
        "Continuous cinematic motion for the full clip. Camera and subjects must clearly move and progress; do not freeze or loop the opening pose; do not hold a static first-frame look.\nMotion: {motion}\nAudio: {audio}"
    )
}

fn is_seedance_privacy_image_err(err: &VimaxError) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("privacyinformation")
        || s.contains("inputimagesensitivecontent")
        || s.contains("may contain real person")
        || (s.contains("real person") && s.contains("sensitive"))
        || s.contains("含真人")
}

/// Flowy may wrap upstream 400 as opaque 502 without PrivacyInformation in the
/// client message (especially if FlowyClaw wasn't restarted). Treat those opaque
/// seedance create failures as image-reject candidates so resume can fall back.
fn should_retry_seedance_without_photoreal_frame(err: &VimaxError) -> bool {
    if is_seedance_privacy_image_err(err) {
        return true;
    }
    let s = err.to_string().to_ascii_lowercase();
    let not_other = !s.contains("insufficient")
        && !s.contains("额度")
        && !s.contains("duration")
        && !s.contains("cancelled")
        && !s.contains("取消")
        && !s.contains("timeout")
        && !s.contains("超时");
    // Match opaque gateway wraps AND explicit upstream 400 from FlowyClaw detail.
    let opaque = s.contains("视频生成服务暂时不可用")
        || s.contains("temporarily unavailable")
        || s.contains("seedance_upstream")
        || s.contains("upstream status 400")
        || (s.contains("seedance") && s.contains("badrequest"))
        || (s.contains("seedance") && s.contains(" status 400"));
    opaque && not_other
}

fn truncate_err(err: &VimaxError, max_chars: usize) -> String {
    let s = err.to_string();
    if s.chars().count() <= max_chars {
        s
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}
