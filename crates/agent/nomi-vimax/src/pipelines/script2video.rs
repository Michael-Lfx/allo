//! Script2Video pipeline — plan text artifacts then render frames/clips/final.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    CameraImageGenerator, CharacterExtractor, CharacterPortraitsGenerator, ReferenceImageSelector,
    StoryboardArtist, VoiceProfileGenerator, WorldAssetsPlanner, ensure_film_cover,
    has_usable_portrait, rank_world_pairs_for_frame, world_asset_pairs,
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

        emit_pct(&progress, "voice_profiles", "正在标定角色声音特征", 15.0);
        let characters = self.ensure_character_voices(characters, script, &style).await?;

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

        // Poster is display-only (not muxed). Prefer film root so multi-scene shares one cover.
        let film_root = resolve_film_root(&self.working_dir);
        let synopsis = format!("{script}\n{user_requirement}");
        let cover_aspect = crate::aspect::load_aspect_from_dir(&film_root).await;
        let _ = ensure_film_cover(
            &film_root,
            Arc::clone(&self.backends.chat),
            self.backends.poster_image(&cover_aspect),
            &style,
            &synopsis,
            &progress,
        )
        .await;

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

        emit(&progress, "frames_start", "跳过镜头首尾帧生成（Seedance 多参考图生视频）");
        // Intentionally skip generate_frames_sequential: Seedance 2.0 rejects img2img
        // photoreal frames as first/last_frame. Shot video uses multi reference_image
        // (cast/env/prop + previous video_last_frame). Frame generators remain available
        // for revise/manual workflows.
        emit_pct(&progress, "frames_done", "镜头首尾帧已跳过，进入多参考图生视频", 55.0);

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

    /// Fill missing voice bibles and persist to scene + film-root characters.json.
    async fn ensure_character_voices(
        &self,
        mut characters: Vec<CharacterInScene>,
        script: &str,
        style: &str,
    ) -> VimaxResult<Vec<CharacterInScene>> {
        let voice_gen = VoiceProfileGenerator::new(Arc::clone(&self.backends.chat));
        let changed = voice_gen
            .ensure_voice_profiles(&mut characters, script, style)
            .await?;
        if changed {
            let film_root = resolve_film_root(&self.working_dir);
            let scene_path = self.working_dir.join("characters.json");
            write_json_artifact(&scene_path, &characters).await?;
            let film_chars = film_root.join("characters.json");
            if film_chars != scene_path {
                write_json_artifact(&film_chars, &characters).await?;
            }
            tracing::info!(
                count = characters.len(),
                "persisted character voice_profile bible"
            );
        }
        Ok(characters)
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
        if ensure_brief_audio_descs(&mut storyboard) {
            tracing::info!("filled missing storyboard audio_desc with ambient defaults");
            write_json_artifact(&path, &storyboard).await?;
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
            let mut desc = if path.exists() {
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
            // Resume / stale cache: prefer non-empty brief audio, else keep mined defaults.
            let desc_audio_empty = desc
                .audio_desc
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty();
            if desc_audio_empty {
                if let Some(a) = brief.audio_desc.as_ref().filter(|s| !s.trim().is_empty()) {
                    desc.audio_desc = Some(a.clone());
                    write_json_artifact(&path, &desc).await?;
                }
            }
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
    ///
    /// Not used by the default Seedance 2.0 multi-ref R2V render path (kept for revise /
    /// legacy first/last-frame workflows).
    #[allow(dead_code)]
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
    /// Within a scene, every timeline-adjacent shot passes the previous clip's
    /// `video_last_frame.png` as a `reference_image` (multi-ref R2V continuity).
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

            // Timeline-adjacent continuity: previous shot's ending still as reference_image.
            let continuity_first = if i > 0 {
                let prev = &shots[i - 1];
                match ensure_shot_video_last_frame(&self.working_dir, prev.idx, false).await {
                    Ok(Some(path)) => {
                        emit(
                            progress,
                            "video_continuity",
                            &format!(
                                "Shot {}: reference_image ← shot {} video_last_frame.png (cam {}→{})",
                                shot.idx, prev.idx, prev.cam_idx, shot.cam_idx
                            ),
                        );
                        tracing::info!(
                            shot = shot.idx,
                            prev = prev.idx,
                            continuity = %path.display(),
                            "adjacent shot multi-ref continuity locked to previous video_last_frame"
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
                            "Shot {}: failed to obtain previous shot {} video_last_frame.png: {e}",
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
                    // Prefer API return_last_frame; ffmpeg-extract if still missing.
                    let _ = ensure_shot_video_last_frame(&self.working_dir, shot.idx, false).await;
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
        let video_last_frame_path = shot_dir.join("video_last_frame.png");
        media_local::scrub_unusable_video(&video_path).await?;
        if media_local::is_usable_video_file(&video_path) {
            emit(
                progress,
                "video_clip_exists",
                &format!("Shot {} video exists — skipping (no re-bill)", shot.idx),
            );
            return Ok(());
        }

        let continuity_source = continuity_first_frame
            .filter(|p| media_local::is_usable_image_file(p))
            .map(|p| p.to_path_buf());
        let using_video_continuity = continuity_source.is_some();

        let ref_pairs = shot_video_ref_pairs(
            shot,
            continuity_source.as_deref(),
            characters,
            registry,
            world_pairs,
        );
        if ref_pairs.is_empty() {
            return Err(VimaxError::Video(format!(
                "Shot {}: no usable reference images (cast/env/prop{}) for multi-ref video",
                shot.idx,
                if using_video_continuity {
                    ""
                } else {
                    "; first shot needs world/cast assets"
                }
            )));
        }

        let ref_paths: Vec<&Path> = ref_pairs.iter().map(|(p, _)| p.as_path()).collect();
        let prompt = i2v_motion_prompt(
            shot,
            characters,
            style,
            &ref_pairs,
            duration_secs,
            using_video_continuity,
        );
        let ref_names: Vec<String> = ref_pairs
            .iter()
            .map(|(p, _)| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("ref.png")
                    .to_string()
            })
            .collect();
        emit(
            progress,
            "video_clip_start",
            &format!(
                "Generating shot {} video ({}s; multi-ref ×{}; continuity={}; refs=[{}])",
                shot.idx,
                duration_secs,
                ref_paths.len(),
                using_video_continuity,
                ref_names.join(", ")
            ),
        );
        tracing::info!(
            shot = shot.idx,
            refs = ?ref_names,
            continuity = using_video_continuity,
            "video multi-ref R2V binding"
        );

        let first_err = match self
            .backends
            .video
            .generate(
                &prompt,
                None,
                None,
                &ref_paths,
                duration_secs,
                &video_path,
                Some(&video_last_frame_path),
            )
            .await
        {
            Ok(()) => None,
            Err(err) if should_retry_seedance_without_photoreal_frame(&err) => Some(err),
            Err(err) => return Err(err),
        };

        if let Some(err) = first_err {
            // Drop continuity still (may still trip privacy on rare gateways) and retry
            // with T2I cast/env/prop refs only; then pure T2V.
            let asset_pairs: Vec<(PathBuf, String)> = ref_pairs
                .iter()
                .filter(|(p, _)| {
                    continuity_source
                        .as_ref()
                        .map(|c| p != c)
                        .unwrap_or(true)
                })
                .cloned()
                .collect();
            if using_video_continuity && !asset_pairs.is_empty() {
                emit(
                    progress,
                    "video_clip_start",
                    &format!(
                        "Shot {}: possible privacy block ({}). Retrying multi-ref without continuity still…",
                        shot.idx,
                        truncate_err(&err, 120)
                    ),
                );
                let asset_paths: Vec<&Path> =
                    asset_pairs.iter().map(|(p, _)| p.as_path()).collect();
                let retry_prompt = i2v_motion_prompt(
                    shot,
                    characters,
                    style,
                    &asset_pairs,
                    duration_secs,
                    false,
                );
                match self
                    .backends
                    .video
                    .generate(
                        &retry_prompt,
                        None,
                        None,
                        &asset_paths,
                        duration_secs,
                        &video_path,
                        Some(&video_last_frame_path),
                    )
                    .await
                {
                    Ok(()) => {
                        // success
                    }
                    Err(retry_err)
                        if should_retry_seedance_without_photoreal_frame(&retry_err)
                            || is_seedance_privacy_image_err(&retry_err) =>
                    {
                        emit(
                            progress,
                            "video_clip_start",
                            &format!(
                                "Shot {}: refs still blocked; falling back to text-to-video…",
                                shot.idx
                            ),
                        );
                        let t2v_prompt = format!(
                            "{}\n{}\nOpening scene: {}",
                            crate::planning::style_prompt_clause(style),
                            retry_prompt,
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
                                Some(&video_last_frame_path),
                            )
                            .await
                            .map_err(|t2v_err| {
                                VimaxError::Video(format!(
                                    "Shot {} video failed (multi-ref → drop continuity → text-to-video). First: {}; Final: {t2v_err}",
                                    shot.idx,
                                    truncate_err(&err, 160)
                                ))
                            })?;
                    }
                    Err(retry_err) => return Err(retry_err),
                }
            } else {
                emit(
                    progress,
                    "video_clip_start",
                    &format!(
                        "Shot {}: possible privacy block ({}). Falling back to text-to-video…",
                        shot.idx,
                        truncate_err(&err, 120)
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
                        Some(&video_last_frame_path),
                    )
                    .await
                    .map_err(|t2v_err| {
                        VimaxError::Video(format!(
                            "Shot {} video failed (multi-ref → text-to-video). First: {}; Final: {t2v_err}",
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
    /// Kept for revise / legacy first-last I2V workflows; render path no longer calls this.
    #[allow(dead_code)]
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

/// Fill empty `audio_desc` so every shot carries ambient/BGM (and keeps dialogue
/// when it was only written into `visual_desc`).
fn ensure_brief_audio_descs(shots: &mut [ShotBriefDescription]) -> bool {
    let mut changed = false;
    for shot in shots.iter_mut() {
        let empty = shot
            .audio_desc
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty();
        if !empty {
            continue;
        }
        let fill = if crate::planning::text_looks_like_dialogue(&shot.visual_desc) {
            // Prefer not dumping the whole visual brief — keep a concise audio cue.
            let snippet: String = shot.visual_desc.chars().take(200).collect();
            snippet
        } else {
            "环境底噪与轻柔电影感背景音乐，配合画面动作的细微拟音".to_string()
        };
        shot.audio_desc = Some(fill);
        changed = true;
    }
    changed
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
        let n = i + 1;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("ref.png");
        let hint: String = text.chars().take(120).collect();
        ref_bind.push_str(&format!("Image {n} ({name}): {hint}. "));
    }
    let ref_clause = if ref_bind.is_empty() {
        String::new()
    } else {
        format!(
            "REFERENCE BINDINGS (each Image N is a reference_image input — follow these roles): {ref_bind}\
Use *_three_view / cameo for cast identity, *_environment_plate for location, *_prop for objects, \
video_last_frame for match-cut continuity from the previous shot. "
        )
    };
    let continuity_clause = if from_prev_video_tail {
        "CONTINUITY: Image 1 is the previous adjacent shot's ending frame — begin motion immediately from that pose/framing; \
seamless match-cut continuation into this beat (camera/angle may already differ; keep identity and set). "
    } else {
        ""
    };
    let voice_lock = character_voice_lock_clause(characters, &shot.ff_vis_char_idxs);
    let audio_block = seedance_audio_caption_block(
        shot.audio_desc.as_deref(),
        &shot.motion_desc,
        &shot.visual_desc,
        characters,
        &shot.ff_vis_char_idxs,
    );
    format!(
        "{style_clause} {identity}{voice_lock}{ref_clause}{continuity_clause}\
DURATION: target length is about {duration_secs}s. Speak at a natural conversational pace — \
do NOT rush, speed-read, chipmunk, or time-compress dialogue to cram lines in. \
Prefer clear finished lines over packing too many words; keep motion readable for the full clip.\n\
VOICE CONTINUITY: each named speaker MUST keep the SAME timbre, pitch, and relative volume as VOICE LOCK for the full clip and across adjacent shots.\n\
PLOT LOCK: stay on this scene — {plot}.{end_plot} \
Do not invent new characters, locations, outfits, or story beats.\n\
Motion: {motion}\n\
Throughout: {audio_block}\n\
Keep it subtitle-free. Do not generate on-screen captions, logos, or watermarks."
    )
}

/// Multi-ref strip for Seedance R2V: optional previous video_last_frame + cast + env/prop.
fn shot_video_ref_pairs(
    shot: &ShotDescription,
    continuity: Option<&Path>,
    characters: &[CharacterInScene],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    world_pairs: &[(PathBuf, String)],
) -> Vec<(PathBuf, String)> {
    let mut pairs: Vec<(PathBuf, String)> = Vec::new();
    if let Some(path) = continuity.filter(|p| media_local::is_usable_image_file(p)) {
        pairs.push((
            path.to_path_buf(),
            "Previous adjacent shot ending frame — match-cut continuity; start motion immediately from this pose, framing, wardrobe, and set."
                .into(),
        ));
    }

    let mut vis: Vec<i32> = shot.ff_vis_char_idxs.clone();
    for &idx in &shot.lf_vis_char_idxs {
        if !vis.contains(&idx) {
            vis.push(idx);
        }
    }
    pairs.extend(portrait_pairs(characters, &vis, registry));

    let world_query = format!(
        "{} {} {}",
        shot.ff_desc.trim(),
        shot.motion_desc.trim(),
        shot.lf_desc.trim()
    );
    pairs.extend(rank_world_pairs_for_frame(&world_query, world_pairs, 4));

    // Dedup by path while preserving order (continuity first).
    let mut seen = std::collections::HashSet::new();
    pairs.retain(|(p, _)| seen.insert(p.clone()));

    let portrait_budget = vis.len().clamp(1, MAX_FRAME_PORTRAIT_REFS);
    // Keep continuity (if any) pinned at index 0, then apply the usual strip budget.
    if pairs
        .first()
        .map(|(p, _)| continuity.is_some_and(|c| p == c))
        .unwrap_or(false)
    {
        let continuity_pair = pairs.remove(0);
        let mut rest = pick_frame_ref_strip(pairs, portrait_budget);
        rest.insert(0, continuity_pair);
        // Cap total refs for Seedance latency/cost (continuity + assets).
        rest.truncate(MAX_FRAME_REF_IMAGES.saturating_add(1));
        rest
    } else {
        let mut out = pick_frame_ref_strip(pairs, portrait_budget);
        out.truncate(MAX_FRAME_REF_IMAGES);
        out
    }
}

/// Compact VOICE LOCK so Seedance keeps the same speaker timbre across shots.
fn character_voice_lock_clause(characters: &[CharacterInScene], idxs: &[i32]) -> String {
    let mut parts = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut push_ch = |ch: &CharacterInScene| {
        if parts.len() >= 4 || !seen.insert(ch.idx) {
            return;
        }
        if let Some(vp) = ch.voice_profile.as_ref().filter(|v| v.is_usable()) {
            parts.push(vp.seedance_clause(&ch.identifier_in_scene));
        }
    };

    for &ci in idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            push_ch(ch);
        }
    }
    for ch in characters {
        push_ch(ch);
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "VOICE LOCK (same speaker = same voice every shot): {}. ",
        parts.join(" | ")
    )
}

/// Seedance 2.0 audio captions use typed brackets:
/// dialogue `{…}`, SFX `<…>`, music `(…)`.
/// Empty captions with `generate_audio=true` fail with InvalidParameter.
///
/// Resolves audio from `audio_desc`, falling back to dialogue mined from
/// `motion_desc` / `visual_desc`. Injects per-character voice locks when the
/// text mentions a cast member, and always includes a stable BGM underscore
/// so adjacent shots share a similar music intent.
fn seedance_audio_caption_block(
    audio_desc: Option<&str>,
    motion_desc: &str,
    visual_desc: &str,
    characters: &[CharacterInScene],
    vis_idxs: &[i32],
) -> String {
    let audio = audio_desc.unwrap_or("").trim();
    let raw = if !audio.is_empty() {
        audio.to_string()
    } else if crate::planning::text_looks_like_dialogue(motion_desc) {
        motion_desc.trim().to_string()
    } else if crate::planning::text_looks_like_dialogue(visual_desc) {
        visual_desc.trim().chars().take(280).collect()
    } else {
        String::new()
    };

    let bgm = "(soft continuous cinematic atmospheric underscore, stable volume and motif across adjacent shots)";

    if raw.is_empty() {
        return format!(
            "<environmental ambience and scene-matched foley matching on-screen action> {bgm}"
        );
    }

    let has_typed = raw.contains('{')
        || raw.contains('}')
        || raw.contains('<')
        || raw.contains('>')
        || (raw.contains('(') && raw.contains(')'));

    let voiced = inject_voice_into_audio_text(&raw, characters, vis_idxs);

    if has_typed {
        let has_music = voiced.contains('(') && voiced.contains(')');
        if has_music {
            return voiced;
        }
        return format!("{voiced} {bgm}");
    }

    let looks_dialogue = crate::planning::text_looks_like_dialogue(&voiced);
    if looks_dialogue {
        format!("{{{voiced}}} <scene-matched foley> {bgm}")
    } else {
        format!("<{voiced}> {bgm}")
    }
}

/// Prefix dialogue with matched character voice clauses when names appear in the text.
fn inject_voice_into_audio_text(
    raw: &str,
    characters: &[CharacterInScene],
    vis_idxs: &[i32],
) -> String {
    if characters.is_empty() {
        return raw.to_string();
    }
    let mut matched: Vec<&CharacterInScene> = characters
        .iter()
        .filter(|c| {
            c.voice_profile.as_ref().is_some_and(|v| v.is_usable())
                && !c.identifier_in_scene.trim().is_empty()
                && raw.contains(c.identifier_in_scene.trim())
        })
        .collect();
    if matched.is_empty() {
        // Fall back to visible cast voices so unnamed dialogue still gets a lock.
        matched = characters
            .iter()
            .filter(|c| {
                c.voice_profile.as_ref().is_some_and(|v| v.is_usable())
                    && (vis_idxs.is_empty() || vis_idxs.contains(&c.idx))
            })
            .take(2)
            .collect();
    }
    if matched.is_empty() {
        return raw.to_string();
    }
    let locks: Vec<String> = matched
        .iter()
        .filter_map(|c| {
            c.voice_profile
                .as_ref()
                .map(|vp| vp.seedance_clause(&c.identifier_in_scene))
        })
        .collect();
    if locks.is_empty() {
        return raw.to_string();
    }
    format!(
        "[speakers: {}] {raw}",
        locks.join("; ")
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

    #[test]
    fn multi_ref_prompt_labels_images_one_based() {
        let s = shot(1, 0);
        let refs = vec![
            (
                PathBuf::from("shots/0/video_last_frame.png"),
                "Previous ending".into(),
            ),
            (
                PathBuf::from("characters/alice_three_view.png"),
                "Cast bible".into(),
            ),
        ];
        let prompt = i2v_motion_prompt(&s, &[], "cinematic", &refs, 5, true);
        assert!(prompt.contains("Image 1 (video_last_frame.png)"));
        assert!(prompt.contains("Image 2 (alice_three_view.png)"));
        assert!(prompt.contains("CONTINUITY: Image 1"));
        // Empty audio_desc still gets ambient + BGM captions.
        assert!(prompt.contains("Throughout:"));
        assert!(prompt.contains("underscore"));
    }

    #[test]
    fn audio_caption_mines_motion_dialogue_and_always_has_bgm() {
        let from_motion = seedance_audio_caption_block(
            None,
            "他看着对方说道：「我们走吧」",
            "wide shot of two people",
            &[],
            &[],
        );
        assert!(from_motion.contains('{'));
        assert!(from_motion.contains("我们走吧"));
        assert!(from_motion.contains('('));

        let ambient = seedance_audio_caption_block(None, "slow pan across room", "establishing", &[], &[]);
        assert!(ambient.contains('<') || ambient.contains("ambience") || ambient.contains("环境"));
        assert!(ambient.contains('('));

        let typed = seedance_audio_caption_block(
            Some("{快跑} <脚步声>"),
            "runs",
            "chase",
            &[],
            &[],
        );
        assert!(typed.contains("{快跑}"));
        assert!(typed.contains('('), "typed captions without music get a BGM underscore");
    }

    #[test]
    fn audio_caption_injects_character_voice_lock() {
        use crate::domain::VoiceProfile;
        let chars = vec![CharacterInScene {
            idx: 0,
            identifier_in_scene: "李薇".into(),
            is_visible: true,
            static_features: "年轻女性".into(),
            dynamic_features: None,
            voice_profile: Some(VoiceProfile {
                timbre: "清亮女中音".into(),
                volume: Some("normal".into()),
                pitch: Some("mid-high".into()),
                speaking_style: "语速平稳".into(),
                caption_clause: Some("李薇: 清亮女中音, mid-high, volume normal, 语速平稳".into()),
            }),
        }];
        let caption = seedance_audio_caption_block(
            Some("李薇说：「今晚别等我」"),
            "nod",
            "close-up",
            &chars,
            &[0],
        );
        assert!(caption.contains("李薇"));
        assert!(caption.contains("清亮女中音") || caption.contains("speakers:"));
        let prompt = i2v_motion_prompt(
            &ShotDescription {
                idx: 0,
                is_last: true,
                cam_idx: 0,
                visual_desc: "close-up".into(),
                variation_type: "small".into(),
                variation_reason: String::new(),
                ff_desc: "face".into(),
                ff_vis_char_idxs: vec![0],
                lf_desc: "face".into(),
                lf_vis_char_idxs: vec![0],
                motion_desc: "speaks".into(),
                audio_desc: Some("李薇说：「今晚别等我」".into()),
            },
            &chars,
            "cinematic",
            &[],
            10,
            false,
        );
        assert!(prompt.contains("VOICE LOCK"));
        assert!(prompt.contains("清亮女中音") || prompt.contains("李薇"));
    }

    #[test]
    fn ensure_brief_audio_fills_empty() {
        let mut shots = vec![ShotBriefDescription {
            idx: 0,
            is_last: true,
            cam_idx: 0,
            visual_desc: "establishing wide shot".into(),
            audio_desc: None,
        }];
        assert!(ensure_brief_audio_descs(&mut shots));
        assert!(shots[0].audio_desc.as_ref().is_some_and(|s| !s.trim().is_empty()));
        assert!(!ensure_brief_audio_descs(&mut shots));
    }
}
