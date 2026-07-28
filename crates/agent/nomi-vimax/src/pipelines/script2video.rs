//! Script2Video pipeline — plan text artifacts then render frames/clips/final.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    CameraImageGenerator, CharacterExtractor, CharacterPortraitsGenerator, ReferenceImageSelector,
    StoryboardArtist, WorldAssetsPlanner, has_usable_portrait, rank_world_pairs_for_frame,
    world_asset_pairs,
};
use crate::domain::{Camera, CharacterInScene, ShotBriefDescription, ShotDescription};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local;
use crate::progress::ProgressCallback;
use crate::session::{read_json_artifact, write_json_artifact, write_text_artifact};

use super::cameo_bind::{
    apply_session_cameos, cameo_extractor_hint, resolve_session_root, world_cameo_context,
};
use super::{
    PipelineBackends, emit, emit_pct, group_shots_into_cameras, load_or_write_json,
    resolve_film_root, safe_component, sanitize_camera_tree,
};

pub struct Script2VideoPipeline {
    backends: PipelineBackends,
    working_dir: PathBuf,
    character_extractor: CharacterExtractor,
    storyboard: StoryboardArtist,
    camera_gen: CameraImageGenerator,
    ref_selector: ReferenceImageSelector,
}

impl Script2VideoPipeline {
    pub fn new(backends: PipelineBackends, working_dir: PathBuf) -> Self {
        let character_extractor = CharacterExtractor::new(Arc::clone(&backends.chat));
        let storyboard = StoryboardArtist::new(Arc::clone(&backends.chat));
        let camera_gen = CameraImageGenerator::new(Arc::clone(&backends.chat));
        let ref_selector = ReferenceImageSelector::new(Arc::clone(&backends.chat));
        Self {
            backends,
            working_dir,
            character_extractor,
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
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<PlanArtifacts> {
        tokio::fs::create_dir_all(&self.working_dir).await?;
        write_text_artifact(&self.working_dir.join("script.txt"), script).await?;
        let style = crate::planning::resolve_visual_style(style);
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;

        emit_pct(&progress, "extract_characters", "正在从剧本提取角色", 12.0);
        let characters = self.extract_characters(script, &style).await?;

        emit_pct(&progress, "cameo_bind", "正在绑定用户角色参考图", 18.0);
        apply_session_cameos(&self.working_dir, &characters).await?;

        // Global cast bible during planning (ViMax generates before frames; we also
        // expose portraits as plan artifacts so users can review identity early).
        emit_pct(
            &progress,
            "character_portraits_start",
            "正在生成全局角色定妆图",
            22.0,
        );
        let _ = self
            .generate_character_portraits(&characters, &style, script, &progress)
            .await?;

        emit_pct(
            &progress,
            "world_assets_start",
            "正在生成全局环境与道具参考图",
            30.0,
        );
        {
            let film_root = resolve_film_root(&self.working_dir);
            let planner = WorldAssetsPlanner::new(
                Arc::clone(&self.backends.chat),
                Arc::clone(&self.backends.image),
            );
            let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
            let _ = planner
                .ensure(
                    &film_root,
                    script,
                    &style,
                    &style_refs,
                    &scene_hint,
                    &lock_token,
                )
                .await?;
        }

        emit_pct(&progress, "design_storyboard", "正在设计分镜表", 40.0);
        let storyboard = self
            .design_storyboard(script, &characters, user_requirement)
            .await?;

        emit_pct(&progress, "decompose_shots", "正在分解镜头视觉描述", 62.0);
        let shot_descriptions = self
            .decompose_visual_descriptions(&storyboard, &characters)
            .await?;

        emit_pct(&progress, "construct_camera_tree", "正在构建机位树", 85.0);
        let camera_tree = self.construct_camera_tree(&shot_descriptions).await?;

        emit_pct(&progress, "planned", "文本规划完成（含全局定妆图）", 100.0);
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
        let style = crate::planning::resolve_visual_style(style);
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;
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
            .plan_text_artifacts(script, user_requirement, &style, progress.clone())
            .await?;

        emit(
            &progress,
            "character_portraits_start",
            "正在确认全局角色定妆图",
        );
        let registry = self
            .generate_character_portraits(&plan.characters, &style, script, &progress)
            .await?;

        let world_pairs = {
            let film_root = resolve_film_root(&self.working_dir);
            let planner = WorldAssetsPlanner::new(
                Arc::clone(&self.backends.chat),
                Arc::clone(&self.backends.image),
            );
            let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
            let reg = planner
                .ensure(
                    &film_root,
                    script,
                    &style,
                    &style_refs,
                    &scene_hint,
                    &lock_token,
                )
                .await?;
            world_asset_pairs(&reg)
        };

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
            &world_pairs,
            &style,
            &progress,
        )
        .await?;

        emit(&progress, "video_clips_start", "正在串行生成镜头视频（一次一个）");
        self.generate_videos_sequential(
            &plan.shot_descriptions,
            &plan.characters,
            &registry,
            &world_pairs,
            &style,
            &progress,
        )
        .await?;

        if media_local::is_usable_video_file(&final_path) {
            emit(&progress, "final_video_exists", "场景成片已存在");
        } else {
            emit(&progress, "concat_start", "正在拼接镜头视频");
            let mut clips: Vec<PathBuf> = Vec::new();
            let mut ordered_shots = plan.shot_descriptions.clone();
            ordered_shots.sort_by_key(|s| s.idx);
            for shot in &ordered_shots {
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

    async fn extract_characters(
        &self,
        script: &str,
        style: &str,
    ) -> VimaxResult<Vec<CharacterInScene>> {
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
        let style = style.to_string();
        let session_root = resolve_session_root(&self.working_dir);
        let script = format!("{script}{}", cameo_extractor_hint(&session_root));
        load_or_write_json(&path, || async {
            self.character_extractor
                .extract_characters(&script, &style)
                .await
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

        // Sequential by timeline idx so each shot's ff can continue from the previous lf.
        let mut ordered = briefs.to_vec();
        ordered.sort_by_key(|b| b.idx);

        let mut out: Vec<ShotDescription> = Vec::with_capacity(ordered.len());
        let mut prev_lf: Option<String> = None;
        let storyboard = StoryboardArtist::new(Arc::clone(&self.backends.chat));

        for brief in &ordered {
            let path = shots_root
                .join(brief.idx.to_string())
                .join("shot_description.json");
            let desc = if path.exists() {
                read_json_artifact(&path).await?
            } else {
                let desc = storyboard
                    .decompose_visual_description_with_continuity(
                        brief,
                        characters,
                        prev_lf.as_deref(),
                    )
                    .await?;
                write_json_artifact(&path, &desc).await?;
                desc
            };
            prev_lf = Some(desc.lf_desc.clone());
            out.push(desc);
        }

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
    ///
    /// `theme_source` is the script/story text used for THEME LOCK on wardrobe/era.
    async fn generate_character_portraits(
        &self,
        characters: &[CharacterInScene],
        style: &str,
        theme_source: &str,
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

        let theme = crate::planning::portrait_theme_excerpt(theme_source);
        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(4));
        for character in characters {
            if !character.is_visible {
                continue;
            }
            // Skip when user Cameo or a usable three-view sheet already exists.
            if has_usable_portrait(&registry, &character.identifier_in_scene) {
                continue;
            }
            // Drop stale AI / unusable Cameo rows before regenerating the sheet.
            registry.remove(&character.identifier_in_scene);
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
            let theme = theme.clone();
            let permit = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = permit
                    .acquire()
                    .await
                    .map_err(|_| VimaxError::msg("semaphore closed"))?;
                portraits
                    .generate_all_views(&character, &style, &theme, &dir)
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
        world_pairs: &[(PathBuf, String)],
        style: &str,
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
                self.generate_frames_for_camera(
                    &cam,
                    shots,
                    characters,
                    registry,
                    world_pairs,
                    style,
                    progress,
                )
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

            self.generate_frames_for_camera(
                &camera,
                shots,
                characters,
                registry,
                world_pairs,
                style,
                progress,
            )
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
    ///
    /// Within a scene, every timeline-adjacent shot reuses the previous clip's
    /// extracted last frame as the next I2V first_frame (match-cut continuity).
    /// Cross-scene continuity is intentionally skipped (each scene has its own pipeline).
    async fn generate_videos_sequential(
        &self,
        shots: &[ShotDescription],
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        world_pairs: &[(PathBuf, String)],
        style: &str,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        // Always drive continuity by timeline idx, not whatever order the planner cached.
        let mut shots = shots.to_vec();
        shots.sort_by_key(|s| s.idx);
        let shots = &shots[..];

        let total = shots.len().max(1);
        let mut ok = 0usize;
        let mut errors: Vec<String> = Vec::new();
        let target = load_target_duration_secs(&self.working_dir).await;
        let needs: Vec<u32> = shots
            .iter()
            .map(|s| {
                crate::planning::estimate_shot_need_secs(
                    s.audio_desc.as_deref(),
                    &s.motion_desc,
                    &s.variation_type,
                )
            })
            .collect();
        let clip_durs = crate::planning::allocate_clip_durations_for_content(target, &needs);
        tracing::info!(
            target = ?target,
            needs = ?needs,
            durations = ?clip_durs,
            "content-aware shot durations (audio+motion)"
        );

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

            // Timeline-adjacent continuity inside this scene (any camera):
            // next I2V first_frame MUST be previous shot's video_last_frame.png.
            let continuity_first = if i > 0 {
                let prev = &shots[i - 1];
                match ensure_shot_video_last_frame(&self.working_dir, prev.idx, true).await {
                    Ok(Some(path)) => {
                        emit(
                            progress,
                            "video_continuity",
                            &format!(
                                "Shot {}: I2V first_frame ← shot {} video_last_frame.png (cam {}→{})",
                                shot.idx, prev.idx, prev.cam_idx, shot.cam_idx
                            ),
                        );
                        tracing::info!(
                            shot = shot.idx,
                            prev = prev.idx,
                            continuity = %path.display(),
                            "adjacent shot video continuity locked to previous video_last_frame"
                        );
                        Some(path)
                    }
                    Ok(None) => {
                        return Err(VimaxError::Video(format!(
                            "Shot {}: cannot continue from previous shot {} — video.mp4 missing/invalid, \
so video_last_frame.png is unavailable. Fix/regenerate shot {} first.",
                            shot.idx, prev.idx, prev.idx
                        )));
                    }
                    Err(e) => {
                        return Err(VimaxError::Video(format!(
                            "Shot {}: failed to extract previous shot {} video_last_frame.png: {e}",
                            shot.idx, prev.idx
                        )));
                    }
                }
            } else {
                None
            };

            let duration_secs = clip_durs.get(i).copied().unwrap_or_else(|| {
                needs.get(i).copied().unwrap_or_else(|| {
                    crate::planning::clip_duration_secs(target, shots.len())
                })
            });
            emit(
                progress,
                "video_duration",
                &format!(
                    "Shot {}: render {}s (need≈{}s from audio/motion)",
                    shot.idx,
                    duration_secs,
                    needs.get(i).copied().unwrap_or(duration_secs)
                ),
            );

            match self
                .generate_video_for_shot(
                    shot,
                    duration_secs,
                    continuity_first.as_deref(),
                    characters,
                    registry,
                    world_pairs,
                    style,
                    progress,
                )
                .await
            {
                Ok(()) => {
                    ok += 1;
                    // Always refresh tail for the next adjacent shot.
                    let _ = ensure_shot_video_last_frame(&self.working_dir, shot.idx, true).await;
                    emit_pct(
                        progress,
                        "video_clip_done",
                        &format!("Shot {} ready ({ok}/{total})", shot.idx),
                        55.0 + 40.0 * ((i + 1) as f32 / total as f32),
                    );
                }
                Err(e) => {
                    errors.push(format!("Shot {}: {e}", shot.idx));
                    emit_pct(
                        progress,
                        "video_clips_partial",
                        &format!(
                            "Shot {} failed; succeeded {ok}/{total}. Stopping further submits — resume from checkpoint.",
                            shot.idx
                        ),
                        pct,
                    );
                    break;
                }
            }
        }

        if !errors.is_empty() {
            return Err(VimaxError::Video(format!(
                "Shot video generation failed: succeeded {ok}/{}; further shots not submitted. Successful clips were kept — resume from checkpoint (no re-bill for those).\n{}",
                shots.len(),
                errors.join("\n")
            )));
        }
        emit_pct(
            progress,
            "video_clips_done",
            &format!("All shot videos ready ({ok}/{})", shots.len()),
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
        world_pairs: &[(PathBuf, String)],
        style: &str,
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
        restore_canonical_frame_from_privacy_bak(&first_ff).await;
        restore_canonical_frame_from_privacy_bak(&shot_dir.join("last_frame.png")).await;

        if !first_ff.exists() {
            emit(
                progress,
                "frame_start",
                &format!("generating first frame for shot {first_shot_idx}"),
            );
            let mut available: Vec<(PathBuf, String)> =
                portrait_pairs(characters, &first_shot.ff_vis_char_idxs, registry);
            available.extend(rank_world_pairs_for_frame(
                &first_shot.ff_desc,
                world_pairs,
                4,
            ));

            // Timeline-adjacent predecessor in this scene (any camera).
            // Cross-scene continuity is intentionally skipped (separate working dirs).
            if let Some(prev) = timeline_predecessor(shots, first_shot_idx) {
                if let Some(prev_path) = continuity_frame_path(&self.working_dir, prev.idx) {
                    available.push((
                        prev_path,
                        format!(
                            "Immediate previous shot ending frame in this scene (timeline continuity). \
Evolve framing/pose for the new beat while keeping identity, wardrobe, lighting, and set. Previous ending: {}. New beat: {}",
                            prev.lf_desc, first_shot.ff_desc
                        ),
                    ));
                    emit(
                        progress,
                        "frame_start",
                        &format!(
                            "shot {first_shot_idx}: timeline continuity from previous shot {}",
                            prev.idx
                        ),
                    );
                }
            }

            // Prefer image continuity from parent frame over a billed transition video.
            if let Some(parent_shot_idx) = camera.parent_shot_idx {
                if let Some(parent_ref) =
                    continuity_frame_path(&self.working_dir, parent_shot_idx)
                {
                    let already = available.iter().any(|(p, _)| p == &parent_ref);
                    if !already {
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
            }

            self.generate_frame_from_selector(
                &shot_dir,
                "first_frame",
                &first_shot.ff_desc,
                &available,
                characters,
                &first_shot.ff_vis_char_idxs,
                style,
                &first_ff,
                progress,
            )
            .await?;
        }

        // Same-camera shots chain from the previous ending frame (not the establishing first_frame).
        // Always generate last_frame so Seedance flf2v and next-shot continuity can use it.
        let mut continuity = ContinuityRef {
            path: first_ff.clone(),
            desc: first_shot.ff_desc.clone(),
        };

        {
            let lf = shot_dir.join("last_frame.png");
            if !lf.exists() {
                let mut available =
                    portrait_pairs(characters, &first_shot.lf_vis_char_idxs, registry);
                available.extend(rank_world_pairs_for_frame(
                    &first_shot.lf_desc,
                    world_pairs,
                    4,
                ));
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
                    characters,
                    &first_shot.lf_vis_char_idxs,
                    style,
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
            let lf = sdir.join("last_frame.png");
            restore_canonical_frame_from_privacy_bak(&ff).await;
            restore_canonical_frame_from_privacy_bak(&lf).await;
            if !ff.exists() {
                // Always generate a distinct first frame. Byte-copying the previous
                // frame makes Seedance I2V freeze on the same opening for ~4s.
                let mut available =
                    portrait_pairs(characters, &shot.ff_vis_char_idxs, registry);
                available.extend(rank_world_pairs_for_frame(&shot.ff_desc, world_pairs, 4));
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
                    characters,
                    &shot.ff_vis_char_idxs,
                    style,
                    &ff,
                    progress,
                )
                .await?;
            }

            {
                let lf = sdir.join("last_frame.png");
                if !lf.exists() {
                    let mut available =
                        portrait_pairs(characters, &shot.lf_vis_char_idxs, registry);
                    available.extend(rank_world_pairs_for_frame(&shot.lf_desc, world_pairs, 4));
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
                        characters,
                        &shot.lf_vis_char_idxs,
                        style,
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
        characters: &[CharacterInScene],
        vis_char_idxs: &[i32],
        style: &str,
        out_path: &Path,
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        let selector_path = shot_dir.join(format!("{frame_type}_selector_output.json"));
        let (mut pairs, prompt) = if selector_path.exists() {
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

        // Always keep cast portraits + matched empty-set / prop plates (selector may drop them).
        ensure_frame_refs(&mut pairs, available, characters, vis_char_idxs);

        // Order for ref strip compose: portraits → env/prop → continuity shots.
        pairs.sort_by_key(|(p, _)| {
            let s = p.to_string_lossy().to_ascii_lowercase();
            if s.contains("character_portrait") || s.contains("three_view") {
                0u8
            } else if s.contains("environments") || s.contains("props") {
                1u8
            } else if s.contains("shots") {
                2u8
            } else {
                3u8
            }
        });
        let portrait_budget = vis_char_idxs.len().clamp(1, MAX_FRAME_PORTRAIT_REFS);
        pairs = pick_frame_ref_strip(pairs, portrait_budget);

        let identity = character_identity_clause(characters, vis_char_idxs, style);
        let style_clause = crate::planning::style_prompt_clause(style);
        let plot_lock: String = frame_desc.chars().take(220).collect();
        let set_lock = pairs
            .iter()
            .find(|(p, _)| {
                p.to_string_lossy()
                    .to_ascii_lowercase()
                    .contains("environments")
            })
            .map(|(_, t)| t.chars().take(90).collect::<String>())
            .unwrap_or_default();
        let prop_lock = pairs
            .iter()
            .find(|(p, _)| p.to_string_lossy().to_ascii_lowercase().contains("props"))
            .map(|(_, t)| t.chars().take(60).collect::<String>())
            .unwrap_or_default();
        let mut prefix = String::new();
        for (i, (path, text)) in pairs.iter().enumerate() {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("ref.png");
            let hint: String = text.chars().take(100).collect();
            prefix.push_str(&format!("[{name}] (image #{i}): {hint}. "));
        }
        let continuity_hint = if pairs
            .iter()
            .any(|(p, _)| p.to_string_lossy().to_ascii_lowercase().contains("shots"))
        {
            "Keep temporal continuity with the latest prior shot frame. "
        } else {
            ""
        };
        let multi_ref_hint = if pairs.len() > 1 {
            "Multi-reference img2img: each [filename] above is a separate input image — match faces/wardrobe from *_three_view.png, architecture/lighting from *_environment_plate.png, objects from *_prop.png, and continuity from shot frames. Place the cast INTO the referenced set; do not invent a new location or new characters. "
        } else if pairs.iter().any(|(p, _)| {
            let s = p.to_string_lossy().to_ascii_lowercase();
            s.contains("character_portrait") || s.contains("three_view")
        }) {
            "Match face/hair/outfit from the cast three-view reference file named above. "
        } else {
            ""
        };
        let set_clause = if set_lock.is_empty() {
            String::new()
        } else {
            format!("SET LOCK: {set_lock}. ")
        };
        let prop_clause = if prop_lock.is_empty() {
            String::new()
        } else {
            format!("PROP LOCK: {prop_lock}. ")
        };
        // Plot + identity + set first — selector text alone often drifts off story.
        let full_prompt = format!(
            "{style_clause} PLOT LOCK (must depict): {plot_lock}. {identity}{set_clause}{prop_clause}{multi_ref_hint}{continuity_hint}{prefix}Scene: {prompt}"
        );
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
        duration_secs: u32,
        continuity_first_frame: Option<&Path>,
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        world_pairs: &[(PathBuf, String)],
        style: &str,
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
                &format!("Shot {} video exists — skipping (no re-bill)", shot.idx),
            );
            return Ok(());
        }
        let planned_ff = shot_dir.join("first_frame.png");
        let lf = shot_dir.join("last_frame.png");
        // Older privacy retries renamed multi-ref frames to *.privacy_bak.png and
        // overwrote the canonical paths with text-only drawings — restore them.
        restore_canonical_frame_from_privacy_bak(&planned_ff).await;
        restore_canonical_frame_from_privacy_bak(&lf).await;
        if !planned_ff.exists() {
            return Err(VimaxError::msg(format!(
                "first_frame missing for shot {}",
                shot.idx
            )));
        }
        // Prefer previous adjacent shot's video_last_frame.png for temporal continuity.
        // Keep planned first_frame.png on disk unchanged (resume / revise / UI reference art).
        let continuity_source = continuity_first_frame
            .filter(|p| media_local::is_usable_image_file(p))
            .map(|p| p.to_path_buf());
        let using_video_continuity = continuity_source.is_some();
        let ff: PathBuf = continuity_source
            .clone()
            .unwrap_or_else(|| planned_ff.clone());
        // Audit copy: what Seedance actually received as first_frame for this shot video.
        let i2v_ff_audit = shot_dir.join("i2v_first_frame.png");
        if let Err(e) = tokio::fs::copy(&ff, &i2v_ff_audit).await {
            tracing::warn!(
                shot = shot.idx,
                from = %ff.display(),
                error = %e,
                "failed to write i2v_first_frame.png audit copy"
            );
        }
        // Always pass last_frame when available so Seedance flf2v constrains the ending.
        let use_last = lf.exists();
        // Seedance forbids mixing first/last_frame with reference_image. Cast identity is
        // locked by the frame(s) + text identity clause — do NOT attach three-views here.
        let prompt = i2v_motion_prompt(
            shot,
            characters,
            style,
            &[],
            duration_secs,
            using_video_continuity,
        );
        emit(
            progress,
            "video_clip_start",
            &format!(
                "Generating shot {} video ({}s; I2V first←{}; continuity={}; last_frame={}; may queue / rate-limit)",
                shot.idx,
                duration_secs,
                ff.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                using_video_continuity,
                use_last
            ),
        );
        tracing::info!(
            shot = shot.idx,
            i2v_first = %ff.display(),
            continuity = using_video_continuity,
            last_frame = use_last,
            "video I2V frame binding"
        );

        let last_ref = if use_last { Some(lf.as_path()) } else { None };
        let first_err = match self
            .backends
            .video
            .generate(
                &prompt,
                Some(ff.as_path()),
                last_ref,
                &[],
                duration_secs,
                &video_path,
            )
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
                    "Shot {}: possible real-person / privacy block on frame ({}). Drawing stylized I2V frames (canonical multi-ref frames kept; continuity source preserved)…",
                    shot.idx,
                    truncate_err(&err, 120)
                ),
            );
            // Privacy retry MUST keep adjacent continuity: stylize the same continuity
            // source (previous video_last_frame) — never fall back to this shot's planned_ff.
            let stylized_ff = self
                .ensure_stylized_i2v_frame(
                    shot,
                    if using_video_continuity {
                        "continuity_first_frame"
                    } else {
                        "first_frame"
                    },
                    &ff,
                    &shot.ff_desc,
                    &shot.ff_vis_char_idxs,
                    style,
                    characters,
                    registry,
                    world_pairs,
                )
                .await?;
            let _ = tokio::fs::copy(&stylized_ff, &i2v_ff_audit).await;
            let stylized_lf = if lf.exists() {
                Some(
                    self.ensure_stylized_i2v_frame(
                        shot,
                        "last_frame",
                        &lf,
                        &shot.lf_desc,
                        &shot.lf_vis_char_idxs,
                        style,
                        characters,
                        registry,
                        world_pairs,
                    )
                    .await?,
                )
            } else {
                None
            };

            let retry_i2v = self
                .backends
                .video
                .generate(
                    &prompt,
                    Some(&stylized_ff),
                    stylized_lf.as_deref(),
                    &[],
                    duration_secs,
                    &video_path,
                )
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
                        "Shot {}: stylized frame still blocked; falling back to text-to-video…",
                        shot.idx
                    ),
                );
                let t2v_prompt = format!(
                    "{}\n{}\nOpening scene: {}",
                    crate::planning::style_prompt_clause(style),
                    prompt,
                    shot.ff_desc
                );
                self.backends
                    .video
                    .generate(
                        &t2v_prompt,
                        None,
                        None,
                        &[],
                        duration_secs,
                        &video_path,
                    )
                    .await
                    .map_err(|t2v_err| {
                        VimaxError::Video(format!(
                            "Shot {} video failed (privacy → stylize → text-to-video). First: {}; Final: {t2v_err}",
                            shot.idx,
                            truncate_err(&err, 160)
                        ))
                    })?;
            }
        }

        if !media_local::is_usable_video_file(&video_path) {
            return Err(VimaxError::Video(format!(
                "Shot {} video file invalid after generation",
                shot.idx
            )));
        }
        emit(
            progress,
            "video_clip_done",
            &format!("Shot {} video saved", shot.idx),
        );
        Ok(())
    }

    /// Privacy-safe I2V sidecar frame. Never overwrites the multi-ref canonical path.
    /// Still binds cast / env / prop plates so identity stays consistent.
    async fn ensure_stylized_i2v_frame(
        &self,
        shot: &ShotDescription,
        frame_type: &str,
        canonical_path: &Path,
        frame_desc: &str,
        vis_char_idxs: &[i32],
        style: &str,
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        world_pairs: &[(PathBuf, String)],
    ) -> VimaxResult<PathBuf> {
        let stylized_path = frame_i2v_stylized_path(canonical_path);
        media_local::scrub_unusable_image(&stylized_path)?;
        if media_local::is_usable_image_file(&stylized_path) {
            return Ok(stylized_path);
        }

        let mut available = portrait_pairs(characters, vis_char_idxs, registry);
        available.extend(rank_world_pairs_for_frame(frame_desc, world_pairs, 4));
        // Prefer composition layout from the multi-ref canonical without treating it as
        // a photoreal face bible (portraits already cover identity).
        if media_local::is_usable_image_file(canonical_path) {
            available.push((
                canonical_path.to_path_buf(),
                format!(
                    "Composition / blocking layout for this {frame_type} — restyle faces heavily; keep pose, framing, and set."
                ),
            ));
        }
        let catalog = available.clone();
        let mut pairs = available;
        ensure_frame_refs(&mut pairs, &catalog, characters, vis_char_idxs);
        pairs.sort_by_key(|(p, _)| {
            let s = p.to_string_lossy().to_ascii_lowercase();
            if s.contains("character_portrait") || s.contains("three_view") || s.contains("cameo") {
                0u8
            } else if s.contains("environments") || s.contains("props") {
                1u8
            } else {
                2u8
            }
        });
        let portrait_budget = vis_char_idxs.len().clamp(1, MAX_FRAME_PORTRAIT_REFS);
        let pairs = pick_frame_ref_strip(pairs, portrait_budget);

        let style_clause = crate::planning::style_prompt_clause(style);
        let identity = character_identity_clause(characters, vis_char_idxs, style);
        let plot_lock: String = frame_desc.chars().take(220).collect();
        let mut prefix = String::new();
        for (i, (path, text)) in pairs.iter().enumerate() {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("ref.png");
            let hint: String = text.chars().take(90).collect();
            prefix.push_str(&format!("[{name}] (image #{i}): {hint}. "));
        }
        let continuity_lock = if frame_type.contains("continuity") {
            "CONTINUITY LOCK: this sidecar is the previous shot's ending frame — preserve the exact pose, framing, wardrobe, lighting, and set; only stylize faces for privacy. Do NOT invent a new establishing composition. "
        } else {
            ""
        };
        let prompt = format!(
            "{style_clause} PRIVACY SAFE REDRAW for video I2V (sidecar only). \
Heavily stylize all faces — clearly fictional / non-photoreal; no real-person likeness. \
Keep wardrobe, pose, framing, set, and props locked to the references. \
{continuity_lock}PLOT LOCK: {plot_lock}. {identity}{prefix}Wide 16:9. Scene: {frame_desc}"
        );
        let refs: Vec<&Path> = pairs.iter().map(|(p, _)| p.as_path()).collect();
        tracing::info!(
            shot = shot.idx,
            frame_type,
            refs = refs.len(),
            out = %stylized_path.display(),
            "generating stylized I2V sidecar (canonical multi-ref frame preserved)"
        );
        self.backends
            .image
            .generate(&prompt, &refs, &stylized_path)
            .await?;
        if !media_local::is_usable_image_file(&stylized_path) {
            return Err(VimaxError::msg(format!(
                "stylized {frame_type} I2V sidecar missing after generation for shot {}",
                shot.idx
            )));
        }
        Ok(stylized_path)
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
                let feats = ch.static_features.trim();
                // Prefer user Cameo, then the single three-view sheet.
                let preferred = views
                    .get("cameo")
                    .or_else(|| views.get("sheet"))
                    .or_else(|| views.get("front"));
                if let Some(sheet) = preferred {
                    if let Some(p) = sheet.get("path") {
                        let path = PathBuf::from(p);
                        if media_local::is_usable_image_file(&path) {
                            let file_name = path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("portrait.png");
                            let desc = sheet.get("description").cloned().unwrap_or_else(|| {
                                format!(
                                    "File [{file_name}] = GLOBAL character bible for <{}>: {feats}. Lock face/hair/outfit.",
                                    ch.identifier_in_scene
                                )
                            });
                            available.push((path, desc));
                            continue;
                        }
                    }
                }
                for (view, item) in views {
                    if view == "cameo" || view == "sheet" || view == "front" {
                        continue;
                    }
                    if let Some(p) = item.get("path") {
                        available.push((
                            PathBuf::from(p),
                            format!(
                                "GLOBAL character bible ({view}) <{}>: {feats}.",
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

/// Compact character identity text for Z-Image (refs often ignored — features must be in prompt).
fn character_identity_clause(characters: &[CharacterInScene], idxs: &[i32], style: &str) -> String {
    let mut parts = Vec::new();
    let mut has_child = false;
    for &ci in idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            if !ch.is_visible {
                continue;
            }
            let static_f = ch.static_features.trim();
            let dynamic_f = ch.dynamic_features.as_deref().unwrap_or("").trim();
            if crate::planning::looks_like_child_character(&ch.identifier_in_scene, static_f) {
                has_child = true;
            }
            let mut desc = String::new();
            if !static_f.is_empty() {
                desc.push_str(&static_f.chars().take(120).collect::<String>());
            }
            if !dynamic_f.is_empty() {
                if !desc.is_empty() {
                    desc.push_str("; ");
                }
                desc.push_str(&dynamic_f.chars().take(80).collect::<String>());
            }
            if desc.is_empty() {
                parts.push(format!("<{}>", ch.identifier_in_scene));
            } else {
                parts.push(format!("<{}>: {desc}", ch.identifier_in_scene));
            }
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "CAST LOCK (must match three-view bible): {}. Do not invent new faces/outfits. ",
        parts.join("; ")
    );
    if has_child {
        if crate::planning::wants_stylized_non_photoreal(style) {
            out.push_str(
                "Children share the SAME animation/illustration Style as adults (do not mix photoreal). ",
            );
        } else {
            out.push_str(
                "Children share the SAME cinematic style as adults (not anime/chibi). ",
            );
        }
    }
    out
}

/// Build multi-ref strip for frame img2img: cast bibles + env/prop plates + continuity.
/// Seedream-class models accept an image URL array; keep a hard cap for latency/cost.
const MAX_FRAME_REF_IMAGES: usize = 8;
const MAX_FRAME_PORTRAIT_REFS: usize = 4;
const MAX_FRAME_ENV_REFS: usize = 2;
const MAX_FRAME_PROP_REFS: usize = 1;

fn pick_frame_ref_strip(
    pairs: Vec<(PathBuf, String)>,
    portrait_budget: usize,
) -> Vec<(PathBuf, String)> {
    let portrait_budget = portrait_budget.clamp(1, MAX_FRAME_PORTRAIT_REFS);
    let mut portraits = Vec::new();
    let mut envs = Vec::new();
    let mut props = Vec::new();
    let mut rest = Vec::new();
    for (p, t) in pairs {
        let s = p.to_string_lossy().to_ascii_lowercase();
        if s.contains("character_portrait")
            || s.contains("three_view")
            || s.contains("_cameo")
            || s.contains("/cameo/")
        {
            portraits.push((p, t));
        } else if s.contains("environments") {
            envs.push((p, t));
        } else if s.contains("props") {
            props.push((p, t));
        } else {
            rest.push((p, t));
        }
    }
    // Keep user Cameo plates ahead of AI three-views within the portrait budget.
    portraits.sort_by_key(|(p, _)| {
        let s = p.to_string_lossy().to_ascii_lowercase();
        if s.contains("_cameo") || s.contains("/cameo/") {
            0u8
        } else {
            1u8
        }
    });
    let mut out = Vec::new();
    out.extend(portraits.drain(..).take(portrait_budget));
    out.extend(envs.drain(..).take(MAX_FRAME_ENV_REFS));
    if out.len() < MAX_FRAME_REF_IMAGES {
        out.extend(
            props
                .drain(..)
                .take(MAX_FRAME_PROP_REFS.min(MAX_FRAME_REF_IMAGES - out.len())),
        );
    }
    if out.len() < MAX_FRAME_REF_IMAGES {
        out.extend(rest.drain(..).take(MAX_FRAME_REF_IMAGES - out.len()));
    }
    if out.len() < MAX_FRAME_REF_IMAGES {
        out.extend(portraits.drain(..).take(MAX_FRAME_REF_IMAGES - out.len()));
    }
    out
}

/// Ensure each visible cast portrait and at least one world plate survive selector drops.
fn ensure_frame_refs(
    pairs: &mut Vec<(PathBuf, String)>,
    available: &[(PathBuf, String)],
    characters: &[CharacterInScene],
    vis_char_idxs: &[i32],
) {
    let path_key = |p: &Path| p.to_string_lossy().to_ascii_lowercase();
    let is_portrait = |p: &Path| {
        let s = path_key(p);
        s.contains("character_portrait")
            || s.contains("three_view")
            || s.contains("_cameo")
            || s.contains("/cameo/")
    };
    let mentions_id = |text: &str, id: &str| text.to_ascii_lowercase().contains(&id.to_ascii_lowercase());

    // Re-insert missing three-views for every visible cast member.
    for &ci in vis_char_idxs {
        let Some(ch) = characters.iter().find(|c| c.idx == ci) else {
            continue;
        };
        let id = &ch.identifier_in_scene;
        let already = pairs.iter().any(|(p, t)| {
            is_portrait(p) && (mentions_id(t, id) || path_key(p).contains(&id.to_ascii_lowercase()))
        });
        if already {
            continue;
        }
        if let Some((p, t)) = available.iter().find(|(p, t)| {
            is_portrait(p) && (mentions_id(t, id) || path_key(p).contains(&id.to_ascii_lowercase()))
        }) {
            pairs.insert(0, (p.clone(), t.clone()));
        }
    }
    // Fallback: at least one portrait if none survived.
    if !vis_char_idxs.is_empty() && !pairs.iter().any(|(p, _)| is_portrait(p)) {
        if let Some((p, t)) = available.iter().find(|(p, _)| is_portrait(p)) {
            pairs.insert(0, (p.clone(), t.clone()));
        }
    }

    if !pairs
        .iter()
        .any(|(p, _)| path_key(p).contains("environments"))
    {
        if let Some((p, t)) = available.iter().find(|(p, _)| path_key(p).contains("environments"))
        {
            pairs.push((p.clone(), t.clone()));
        }
    }
    if !pairs.iter().any(|(p, _)| path_key(p).contains("props")) {
        if let Some((p, t)) = available.iter().find(|(p, _)| path_key(p).contains("props")) {
            pairs.push((p.clone(), t.clone()));
        }
    }
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

/// `first_frame.png` → `first_frame.privacy_bak.png` (legacy privacy overwrite backup).
fn frame_privacy_bak_path(canonical: &Path) -> PathBuf {
    canonical.with_extension("privacy_bak.png")
}

/// `first_frame.png` → `first_frame.i2v_stylized.png` (privacy-safe I2V sidecar).
fn frame_i2v_stylized_path(canonical: &Path) -> PathBuf {
    canonical.with_extension("i2v_stylized.png")
}

/// Restore multi-ref canonical frames that an older privacy retry renamed to `*.privacy_bak.png`.
async fn restore_canonical_frame_from_privacy_bak(canonical: &Path) {
    let bak = frame_privacy_bak_path(canonical);
    if !media_local::is_usable_image_file(&bak) {
        return;
    }
    let stylized = frame_i2v_stylized_path(canonical);
    if media_local::is_usable_image_file(canonical) {
        // Keep the text-only overwrite as an I2V sidecar if one isn't already present.
        if !media_local::is_usable_image_file(&stylized) {
            if let Err(e) = tokio::fs::rename(canonical, &stylized).await {
                tracing::warn!(
                    from = %canonical.display(),
                    to = %stylized.display(),
                    error = %e,
                    "failed to move overwritten frame aside before privacy_bak restore"
                );
                return;
            }
        } else {
            let _ = tokio::fs::remove_file(canonical).await;
        }
    }
    match tokio::fs::rename(&bak, canonical).await {
        Ok(()) => tracing::info!(
            path = %canonical.display(),
            "restored multi-ref frame from privacy_bak"
        ),
        Err(e) => tracing::warn!(
            from = %bak.display(),
            to = %canonical.display(),
            error = %e,
            "failed to restore multi-ref frame from privacy_bak"
        ),
    }
}

/// Previous shot in timeline order within the same scene (by idx).
fn timeline_predecessor<'a>(
    shots: &'a [ShotDescription],
    shot_idx: i32,
) -> Option<&'a ShotDescription> {
    shots
        .iter()
        .filter(|s| s.idx < shot_idx)
        .max_by_key(|s| s.idx)
}

/// Extract (or reuse) the last frame of a finished shot video for next-shot continuity.
/// When `force` is true, always re-extract from video.mp4 so the tail matches the latest clip.
async fn ensure_shot_video_last_frame(
    working_dir: &Path,
    shot_idx: i32,
    force: bool,
) -> VimaxResult<Option<PathBuf>> {
    let dir = working_dir.join("shots").join(shot_idx.to_string());
    let video = dir.join("video.mp4");
    if !media_local::is_usable_video_file(&video) {
        return Ok(None);
    }
    let out = dir.join("video_last_frame.png");
    if force {
        let _ = tokio::fs::remove_file(&out).await;
    } else if media_local::is_usable_image_file(&out) {
        return Ok(Some(out));
    }
    media_local::extract_last_frame(&video, &out).await?;
    if media_local::is_usable_image_file(&out) {
        Ok(Some(out))
    } else {
        Ok(None)
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

fn i2v_motion_prompt(
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    style: &str,
    ref_pairs: &[(PathBuf, String)],
    duration_secs: u32,
    from_prev_video_tail: bool,
) -> String {
    let motion = shot.motion_desc.trim();
    let style_clause = crate::planning::style_prompt_clause(style);
    let identity = character_identity_clause(characters, &shot.ff_vis_char_idxs, style);
    let plot: String = shot.ff_desc.chars().take(180).collect();
    let end_plot: String = if shot.lf_desc.trim().is_empty() {
        String::new()
    } else {
        format!(
            " End beat: {}.",
            shot.lf_desc.chars().take(120).collect::<String>()
        )
    };
    let mut ref_bind = String::new();
    for (i, (path, text)) in ref_pairs.iter().enumerate() {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("ref.png");
        let hint: String = text.chars().take(90).collect();
        ref_bind.push_str(&format!("[{name}] (reference_image #{i}): {hint}. "));
    }
    let ref_clause = if ref_bind.is_empty() {
        String::new()
    } else {
        format!(
            "REFERENCE BINDINGS (match these named reference images): {ref_bind}\
Use *_three_view.png for cast identity, *_environment_plate.png for location, *_prop.png for objects. "
        )
    };
    let continuity_clause = if from_prev_video_tail {
        "CONTINUITY: opening frame is the previous adjacent shot's ending in this scene — begin motion immediately; \
do not hold or re-establish the same pose; seamless match-cut continuation into this beat \
(camera/angle may already differ; keep identity and set). "
    } else {
        ""
    };
    let audio_block = seedance_audio_caption_block(shot.audio_desc.as_deref());
    format!(
        "{style_clause} {identity}{ref_clause}{continuity_clause}\
DURATION LOCK: this clip is {duration_secs}s — complete the dialogue/action inside {duration_secs}s; \
no unfinished lines; sustain clear motion for the FULL {duration_secs}s (do not freeze or loop the opening pose).\n\
PLOT LOCK: stay on this scene — {plot}.{end_plot} \
Do not invent new characters, locations, outfits, or story beats.\n\
Motion: {motion}\n\
Throughout: {audio_block}\n\
Keep it subtitle-free. Do not generate on-screen captions, logos, or watermarks."
    )
}

/// Seedance 2.0 audio captions use typed brackets:
/// dialogue `{…}`, SFX `<…>`, music `(…)`.
/// Empty captions with `generate_audio=true` fail with InvalidParameter.
fn seedance_audio_caption_block(audio_desc: Option<&str>) -> String {
    let raw = audio_desc.unwrap_or("").trim();
    if raw.is_empty() {
        return "<subtle environmental ambience and soft foley matching on-screen action> \
(quiet atmospheric underscore)"
            .to_string();
    }
    // Already Seedance-typed — keep as-is.
    let has_typed = raw.contains('{')
        || raw.contains('}')
        || raw.contains('<')
        || raw.contains('>')
        || (raw.contains('(') && raw.contains(')'));
    if has_typed {
        return raw.to_string();
    }

    let lower = raw.to_ascii_lowercase();
    let looks_dialogue = raw.contains('「')
        || raw.contains('」')
        || raw.contains('"')
        || raw.contains('“')
        || raw.contains('”')
        || raw.contains('\'')
        || lower.contains("says")
        || lower.contains("dialogue")
        || lower.contains("speech")
        || lower.contains("voice")
        || lower.contains("台词")
        || lower.contains("说道")
        || lower.contains("喊道")
        || lower.contains("怒吼")
        || lower.contains("嘶吼")
        || lower.contains("说话");
    if looks_dialogue {
        format!(
            "{{{raw}}} <scene-matched foley> (soft atmospheric underscore)"
        )
    } else {
        format!("<{raw}> (soft atmospheric underscore)")
    }
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
    // Caption/audio-schema failures must not trigger frame redraw.
    let s = err.to_string().to_ascii_lowercase();
    if s.contains("captions are not enough")
        || (s.contains("caption") && s.contains("empty"))
        || (s.contains("caption") && s.contains("not enough"))
    {
        return false;
    }
    let not_other = !s.contains("insufficient")
        && !s.contains("额度")
        && !s.contains("duration")
        && !s.contains("cancelled")
        && !s.contains("取消")
        && !s.contains("timeout")
        && !s.contains("超时")
        && !s.contains("invalidparameter")
        && !s.contains("cannot be mixed");
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

#[cfg(test)]
mod continuity_tests {
    use super::*;

    fn shot(idx: i32, cam_idx: i32) -> ShotDescription {
        ShotDescription {
            idx,
            is_last: false,
            cam_idx,
            visual_desc: String::new(),
            variation_type: "small".into(),
            variation_reason: String::new(),
            ff_desc: format!("ff{idx}"),
            ff_vis_char_idxs: vec![],
            lf_desc: format!("lf{idx}"),
            lf_vis_char_idxs: vec![],
            motion_desc: String::new(),
            audio_desc: None,
        }
    }

    #[test]
    fn timeline_predecessor_finds_previous_idx_across_cameras() {
        let shots = vec![shot(0, 0), shot(1, 1), shot(2, 0)];
        let prev = timeline_predecessor(&shots, 2).expect("prev");
        assert_eq!(prev.idx, 1);
        assert_eq!(prev.cam_idx, 1);
        assert!(timeline_predecessor(&shots, 0).is_none());
    }

    #[test]
    fn privacy_bak_and_stylized_sidecar_paths() {
        let canonical = PathBuf::from("shots/0/first_frame.png");
        assert_eq!(
            frame_privacy_bak_path(&canonical),
            PathBuf::from("shots/0/first_frame.privacy_bak.png")
        );
        assert_eq!(
            frame_i2v_stylized_path(&canonical),
            PathBuf::from("shots/0/first_frame.i2v_stylized.png")
        );
    }
}
