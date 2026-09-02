//! Script2Video pipeline — plan text artifacts then render frames/clips/final.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    CameraImageGenerator, CharacterExtractor, CharacterPortraitsGenerator, ReferenceImageSelector,
    StoryboardArtist, VoiceProfileGenerator, VoiceReferenceGenerator, WorldAssetsPlanner,
    ensure_film_cover, has_usable_portrait, rank_world_pairs_for_frame, voice_ref_abs_path,
    world_asset_pairs,
};
use crate::clip_bounds::ClipBounds;
use crate::domain::{Camera, CharacterInScene, ShotBriefDescription, ShotDescription};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local;
use crate::media_local::SpliceSeam;
use crate::progress::ProgressCallback;
use crate::session::{
    copy_json_artifact_if_readable, read_json_artifact, write_json_artifact, write_text_artifact,
};

use super::cameo_bind::{
    apply_session_cameos, cameo_extractor_hint, classify_session_references, resolve_session_root,
    world_cameo_context,
};
use super::privacy_face::{
    content_index_to_image_slot, ensure_ai_sanitized_privacy_face, ensure_seedance_privacy_face,
    is_seedance_privacy_image_err_text, next_privacy_tier_for_path,
    parse_seedance_flagged_content_index, preflight_video_ref_privacy, privacy_repair_targets,
    PrivacyFaceOutcome, PrivacyFaceTier,
};
use super::{
    artifact_cache::{
        load_json_if_cached, load_or_write_json_cached, plan_artifacts_sidecar_complete,
        script2video_plan_fingerprint, sidecar_matches, write_sidecar,
    },
    PipelineBackends, emit, emit_meta, emit_pct, emit_pct_meta, group_shots_into_cameras,
    resolve_film_root, safe_component, sanitize_camera_tree,
};

/// Max times we rewrite flagged input images per shot before dropping continuity / T2V.
const MAX_PRIVACY_FACE_REPAIRS_PER_SHOT: usize = 3;

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
        let storyboard = StoryboardArtist::new(Arc::clone(&backends.chat), backends.clip);
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
        let style = crate::planning::resolve_visual_style(style);
        let plan_fp =
            script2video_plan_fingerprint(&self.working_dir, script, user_requirement, &style)
                .await;

        // Resume/checkpoint fast path: skip expensive planning when all artifacts
        // exist **and** sidecars match current script / requirement / style inputs.
        if plan_artifacts_sidecar_complete(
            &self.working_dir,
            &[
                "characters.json",
                "storyboard.json",
                "shot_descriptions.json",
                "camera_tree.json",
            ],
            &plan_fp,
        )
        .await
        {
            write_text_artifact(&self.working_dir.join("script.txt"), script).await?;
            let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;
            emit_pct(
                &progress,
                "reuse_plan",
                "场景规划产物已存在，跳过文本规划",
                100.0,
            );
            return self.load_plan_artifacts().await;
        }

        write_text_artifact(&self.working_dir.join("script.txt"), script).await?;
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;

        let plan_started = std::time::Instant::now();
        let film_root = resolve_film_root(&self.working_dir);
        // idea2video / script_film / novel2video fan out 3 scene planners at once.
        // Each used to rewrite film-root registries (portraits / world / cover),
        // so a sibling could read a truncated empty JSON and fail with
        // `JSON error: EOF while parsing a value at line 1 column 0`.
        let scene_only = film_root != self.working_dir;

        if !scene_only {
            let session_root = resolve_session_root(&self.working_dir);
            emit_pct(
                &progress,
                "classify_references",
                "正在识别用户上传参考图类型",
                10.0,
            );
            let _ = classify_session_references(
                &session_root,
                Arc::clone(&self.backends.chat),
            )
            .await?;
        }

        emit_pct(&progress, "extract_characters", "正在从剧本提取角色", 12.0);
        let mut characters = self
            .extract_characters(script, user_requirement, &style, &plan_fp)
            .await?;

        if !scene_only {
            emit_pct(&progress, "voice_profiles", "正在标定角色声音特征", 15.0);
            characters = self
                .ensure_character_voices(characters, script, &style)
                .await?;

            emit_pct(
                &progress,
                "cameo_bind",
                "正在绑定用户角色参考图（仅真实照片人脸才做隐私换脸）",
                18.0,
            );
            apply_session_cameos(
                &self.working_dir,
                &characters,
                Arc::clone(&self.backends.image),
                Arc::clone(&self.backends.chat),
            )
            .await?;

            // Global cast bible during planning (text-to-image only — style via prompt, not look plate).
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
                "voice_references_start",
                "正在生成角色音色参考音频",
                24.0,
            );
            self.ensure_character_voice_references(&characters, &progress)
                .await?;

            emit_pct(
                &progress,
                "world_assets_start",
                "正在生成全局环境与道具参考图",
                30.0,
            );
            {
                let world_planner = WorldAssetsPlanner::new(
                    Arc::clone(&self.backends.chat),
                    Arc::clone(&self.backends.image),
                );
                let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
                let _ = world_planner
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
        }

        emit_pct(&progress, "design_storyboard", "正在设计分镜表", 40.0);
        let storyboard = self
            .design_storyboard(script, &characters, user_requirement, &plan_fp)
            .await?;

        emit_pct(&progress, "decompose_shots", "正在分解镜头视觉描述", 62.0);
        let shot_descriptions = self
            .decompose_visual_descriptions(&storyboard, &characters, &plan_fp)
            .await?;
        let storyboard = read_json_artifact(&self.working_dir.join("storyboard.json"))
            .await
            .unwrap_or(storyboard);

        emit_pct(&progress, "construct_camera_tree", "正在构建机位树", 85.0);
        let camera_tree = self
            .construct_camera_tree(&shot_descriptions, &plan_fp)
            .await?;

        if !scene_only {
            // Poster is display-only (not muxed). Prefer film root so multi-scene shares one cover.
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
        }

        emit_pct(&progress, "planned", "文本规划完成（含全局定妆图）", 100.0);
        tracing::info!(
            phase = "plan_phase_total",
            secs = plan_started.elapsed().as_secs_f64(),
            "plan_text_artifacts total wall time"
        );
        Ok(PlanArtifacts {
            characters,
            storyboard,
            shot_descriptions,
            camera_tree,
        })
    }

    /// True when every text-planning artifact exists with a matching input sidecar.
    async fn plan_artifacts_complete(
        &self,
        script: &str,
        user_requirement: &str,
        style: &str,
    ) -> bool {
        let plan_fp =
            script2video_plan_fingerprint(&self.working_dir, script, user_requirement, style).await;
        plan_artifacts_sidecar_complete(
            &self.working_dir,
            &[
                "characters.json",
                "storyboard.json",
                "shot_descriptions.json",
                "camera_tree.json",
            ],
            &plan_fp,
        )
        .await
    }

    /// Load cached plan artifacts without re-running any LLM work. Cameo bindings
    /// are re-applied (cheap file ops) so user uploads stay fresh.
    async fn load_plan_artifacts(&self) -> VimaxResult<PlanArtifacts> {
        let wd = &self.working_dir;
        let characters: Vec<CharacterInScene> =
            read_json_artifact(&wd.join("characters.json")).await?;
        let storyboard: Vec<ShotBriefDescription> =
            read_json_artifact(&wd.join("storyboard.json")).await?;
        let shot_descriptions: Vec<ShotDescription> =
            read_json_artifact(&wd.join("shot_descriptions.json")).await?;
        let camera_tree: Vec<Camera> = read_json_artifact(&wd.join("camera_tree.json")).await?;
        let clip_count_on_disk = shot_descriptions.len();
        let (synced, shot_descriptions) =
            super::clip_beats::align_storyboard_and_clips(storyboard.clone(), shot_descriptions);
        let (synced, shot_descriptions, idx_map) =
            commit_packed_shot_layout(wd, synced, shot_descriptions).await;
        let densified = idx_map.iter().any(|(old, new)| old != new);
        if super::clip_beats::storyboard_differs(&storyboard, &synced)
            || shot_descriptions.len() != clip_count_on_disk
            || densified
        {
            tracing::info!(
                before_briefs = storyboard.len(),
                after_briefs = synced.len(),
                before_clips = clip_count_on_disk,
                after_clips = shot_descriptions.len(),
                densified,
                "aligned storyboard to clips; board never grows at video start"
            );
            write_json_artifact(&wd.join("storyboard.json"), &synced).await?;
            write_json_artifact(&wd.join("shot_descriptions.json"), &shot_descriptions).await?;
        }
        let mut camera_tree = camera_tree;
        if remap_camera_tree_shot_idxs(&mut camera_tree, &idx_map) {
            write_json_artifact(&wd.join("camera_tree.json"), &camera_tree).await?;
        }
        let storyboard = synced;
        apply_session_cameos(
            wd,
            &characters,
            Arc::clone(&self.backends.image),
            Arc::clone(&self.backends.chat),
        )
        .await?;
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
        self.render_with_prior_continuity(script, user_requirement, style, progress, None)
            .await
    }

    /// Like [`Self::render`], but the first shot of this scene match-cuts from
    /// `prior_continuity` (typically the previous scene's last shot
    /// `video_last_frame.png`) — same multi-ref Image 1 path as intra-scene chaining.
    pub async fn render_with_prior_continuity(
        &self,
        script: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
        prior_continuity: Option<&Path>,
    ) -> VimaxResult<PathBuf> {
        emit(&progress, "render_start", "开始渲染脚本成片");
        // Validate once: everything downstream (the opening shot's @Image1, the
        // prompt seam, the concat's opening fade) must agree on whether this
        // scene actually starts from the previous scene's tail frame.
        let prior_continuity = prior_continuity.filter(|p| {
            let usable = media_local::is_usable_image_file(p);
            if !usable {
                tracing::warn!(
                    continuity = %p.display(),
                    "prior scene continuity frame unusable; this scene opens as a hard cut"
                );
            }
            usable
        });
        let style = crate::planning::resolve_visual_style(style);
        let _ = write_text_artifact(&self.working_dir.join("style.txt"), &style).await;
        let final_path = self.working_dir.join("final_video.mp4");
        media_local::scrub_unusable_video(&final_path).await?;

        // Load plan to check shot completeness before deciding to skip.
        let plan = if self
            .plan_artifacts_complete(script, user_requirement, &style)
            .await
        {
            emit(
                &progress,
                "reuse_plan",
                "复用已有规划产物，跳过文本规划",
            );
            self.load_plan_artifacts().await?
        } else {
            self.plan_text_artifacts(script, user_requirement, &style, progress.clone())
                .await?
        };

        // Only skip if final_video exists AND all shot videos are present.
        if media_local::is_usable_video_file(&final_path) {
            let all_shots_complete = plan.shot_descriptions.iter().all(|shot| {
                let shot_video = self
                    .working_dir
                    .join("shots")
                    .join(shot.idx.to_string())
                    .join("video.mp4");
                media_local::is_usable_video_file(&shot_video)
            });
            if all_shots_complete {
                emit(
                    &progress,
                    "final_video_exists",
                    "场景成片及所有镜头视频均已存在，跳过本场景渲染",
                );
                return Ok(final_path);
            }
            // final_video exists but some shot videos are missing — continue generating missing shots.
            tracing::warn!(
                shots = plan.shot_descriptions.len(),
                "final_video exists but some shot videos missing — resuming"
            );
        }

        emit(
            &progress,
            "character_portraits_start",
            "正在确认全局角色定妆图",
        );
        let mut registry = self
            .generate_character_portraits(&plan.characters, &style, script, &progress)
            .await?;

        self.ensure_character_voice_references(&plan.characters, &progress)
            .await?;
        // Refresh registry after voice refs may have been added.
        let film_root = resolve_film_root(&self.working_dir);
        let registry_path = film_root.join("character_portraits_registry.json");
        if registry_path.exists() {
            registry = read_json_artifact(&registry_path).await?;
        }

        let world_pairs = {
            let world_planner = WorldAssetsPlanner::new(
                Arc::clone(&self.backends.chat),
                Arc::clone(&self.backends.image),
            );
            let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
            let reg = world_planner
                .ensure(
                    &film_root,
                    script,
                    &style,
                    &style_refs,
                    &scene_hint,
                    &lock_token,
                )
                .await?;
            world_asset_pairs(&reg, &film_root)
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
            prior_continuity,
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
            // Seams must match what the prompts asked the model for: shots that
            // reuse a camera keep rolling (their head replays and gets trimmed),
            // a camera change is a real cut. A scene that opened from a prior
            // scene's tail frame starts mid-soundtrack, so it must not fade up.
            let refs: Vec<&Path> = clips.iter().map(|p| p.as_path()).collect();
            let entries: Vec<i32> = ordered_shots.iter().map(|s| s.cam_idx).collect();
            let exits: Vec<i32> = ordered_shots.iter().map(|s| s.exit_cam_idx()).collect();
            let opening = if prior_continuity.is_some() {
                media_local::SpliceSeam::MatchCut
            } else {
                media_local::SpliceSeam::Cut
            };
            let seq = media_local::ConcatClip::scene_exits(&refs, &entries, &exits, opening);
            media_local::concat_videos(&seq, &final_path).await?;
            emit(&progress, "concat_done", "场景成片拼接完成");
        }
        emit(&progress, "render_done", "脚本成片渲染完成");
        Ok(final_path)
    }

    async fn extract_characters(
        &self,
        script: &str,
        _user_requirement: &str,
        style: &str,
        plan_fp: &str,
    ) -> VimaxResult<Vec<CharacterInScene>> {
        let film_root = resolve_film_root(&self.working_dir);
        let film_chars = film_root.join("characters.json");
        let path = self.working_dir.join("characters.json");
        // Always prefer the film-level cast so every scene/shot shares identifiers.
        // Never `tokio::fs::copy` here: it truncates the dest first, and an empty
        // film-root leftover used to fail the first scene with
        // `JSON error: EOF while parsing a value at line 1 column 0`.
        if film_chars != path && film_chars.is_file() {
            match read_json_artifact::<Vec<CharacterInScene>>(&film_chars).await {
                Ok(characters) => {
                    write_json_artifact(&path, &characters).await?;
                    super::artifact_cache::write_sidecar(&path, plan_fp).await?;
                    return Ok(characters);
                }
                Err(e) => {
                    return Err(VimaxError::msg(format!(
                        "film-root characters.json unreadable at {}: {e}",
                        film_chars.display()
                    )));
                }
            }
        } else if !path.exists() {
            if let Some(parent) = self.working_dir.parent() {
                let parent_chars = parent.join("characters.json");
                if parent_chars.exists() && parent_chars != path {
                    let _ = copy_json_artifact_if_readable(&parent_chars, &path).await?;
                }
            }
        }
        let style = style.to_string();
        let session_root = resolve_session_root(&self.working_dir);
        let script = format!("{script}{}", cameo_extractor_hint(&session_root));
        load_or_write_json_cached(&path, plan_fp, || async {
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
        plan_fp: &str,
    ) -> VimaxResult<Vec<ShotBriefDescription>> {
        let path = self.working_dir.join("storyboard.json");
        // Draft stays in RAM. `storyboard.json` is the published clip list
        // (one row = one video). Writing the LLM's micro-shots first is what
        // made 「故事分镜」 flash absorbed cards during planning.
        let mut storyboard = match load_json_if_cached(&path, plan_fp).await {
            Some(cached) => cached,
            None => {
                self.storyboard
                    .design_storyboard(script, characters, user_requirement)
                    .await?
            }
        };
        let budget = load_target_duration_secs(&self.working_dir).await;
        if let Some(budget) = budget {
            let max_shots = crate::planning::max_shots_for_budget(self.backends.clip, budget);
            if enforce_max_shots(&mut storyboard, max_shots) {
                tracing::warn!(
                    max_shots,
                    kept = storyboard.len(),
                    "truncated storyboard to respect duration budget"
                );
            }
        }
        if ensure_brief_audio_descs(&mut storyboard) {
            tracing::info!("filled missing storyboard audio_desc with ambient defaults");
        }
        // Pack here — not at render — so each storyboard row is one generated
        // video (in-file CUT allowed). Micro-shots the LLM still emitted are
        // collapsed and reindexed before the first persist.
        let draft_len = storyboard.len();
        let packed = super::clip_beats::pack_scene_briefs(self.backends.clip, storyboard);
        if packed.len() != draft_len {
            tracing::info!(
                before = draft_len,
                after = packed.len(),
                "packed storyboard into renderable clips (one row = one video)"
            );
        }
        persist_published_storyboard(&self.working_dir, &path, &packed, plan_fp).await?;
        // Density inside the packed rows — never extra rows. Same-camera
        // performance beats let clip_timeline compile a Seedance script so a
        // one-prose-line clip does not linger as a thin pose.
        let densified = self.storyboard.densify_in_clip_beats(packed).await;
        if persist_published_storyboard(&self.working_dir, &path, &densified, plan_fp).await? {
            tracing::info!(
                rows = densified.len(),
                "filled in-clip performance beats (row count unchanged)"
            );
        }
        let bgm = ensure_scene_bgm_brief(&self.working_dir, &densified).await?;
        tracing::info!(bgm = %bgm, "scene BGM brief locked for shot continuity");
        Ok(densified)
    }

    async fn decompose_visual_descriptions(
        &self,
        briefs: &[ShotBriefDescription],
        characters: &[CharacterInScene],
        plan_fp: &str,
    ) -> VimaxResult<Vec<ShotDescription>> {
        let aggregate = self.working_dir.join("shot_descriptions.json");
        if aggregate.is_file() && sidecar_matches(&aggregate, plan_fp).await {
            let packed: Vec<ShotDescription> = read_json_artifact(&aggregate).await?;
            let (synced, packed) =
                super::clip_beats::align_storyboard_and_clips(briefs.to_vec(), packed);
            let (synced, packed, _) =
                commit_packed_shot_layout(&self.working_dir, synced, packed).await;
            if super::clip_beats::storyboard_differs(briefs, &synced) {
                tracing::info!(
                    before = briefs.len(),
                    after = synced.len(),
                    "aligned cached shot_descriptions to storyboard (no extra last shot)"
                );
            }
            write_json_artifact(&self.working_dir.join("storyboard.json"), &synced).await?;
            write_json_artifact(&aggregate, &packed).await?;
            return Ok(packed);
        }
        let shots_root = self.working_dir.join("shots");
        tokio::fs::create_dir_all(&shots_root).await?;

        // Sequential by timeline idx so each shot's ff can continue from the previous lf.
        let mut ordered = briefs.to_vec();
        ordered.sort_by_key(|b| b.idx);

        let mut out: Vec<ShotDescription> = Vec::with_capacity(ordered.len());
        let mut prev_lf: Option<String> = None;
        let mut prev_cam: Option<i32> = None;
        let storyboard = StoryboardArtist::new(Arc::clone(&self.backends.chat), self.backends.clip);

        for brief in &ordered {
            let path = shots_root
                .join(brief.idx.to_string())
                .join("shot_description.json");
            // Reuse a cached decomposition when it still describes THIS brief.
            // A merged file is valid iff the brief is also packed (one row =
            // one clip). A merged file left over from the old post-decompose
            // packer must not be reused for an unmerged brief.
            let cached: Option<ShotDescription> = match path.exists() {
                true => read_json_artifact::<ShotDescription>(&path)
                    .await
                    .ok()
                    .filter(|desc| desc.is_merged() == brief.is_merged()),
                false => None,
            };
            let mut desc = if let Some(desc) = cached {
                desc
            } else {
                let desc = storyboard
                    .decompose_visual_description_with_continuity(
                        brief,
                        characters,
                        prev_lf.as_deref(),
                        prev_cam,
                    )
                    .await?;
                write_json_artifact(&path, &desc).await?;
                desc
            };
            // Cached files live under shots/{brief.idx} but may still carry an
            // older `idx` from before pack/reindex. Leaving it drifted is how
            // resume invents a phantom last storyboard row.
            desc.idx = brief.idx;
            super::clip_beats::stamp_beats_from_brief(&mut desc, brief);
            if desc.is_merged() {
                write_json_artifact(&path, &desc).await?;
            }
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
            prev_cam = Some(desc.exit_cam_idx());
            out.push(desc);
        }

        // Safety net for a stale unpacked storyboard that skipped brief packing.
        // After this, storyboard.json is rewritten to the packed list so the
        // studio never keeps empty cards for absorbed micro-shots.
        let packed = super::clip_beats::pack_scene_clips(self.backends.clip, out);
        let (synced, packed) =
            super::clip_beats::align_storyboard_and_clips(briefs.to_vec(), packed);
        let (synced, packed, _) =
            commit_packed_shot_layout(&self.working_dir, synced, packed).await;
        if super::clip_beats::storyboard_differs(briefs, &synced) {
            tracing::info!(
                before = briefs.len(),
                after = synced.len(),
                "synced storyboard after clip packing"
            );
        }
        write_json_artifact(&self.working_dir.join("storyboard.json"), &synced).await?;

        write_json_artifact(&aggregate, &packed).await?;
        super::artifact_cache::write_sidecar(&aggregate, plan_fp).await?;
        Ok(packed)
    }

    async fn construct_camera_tree(
        &self,
        shot_descriptions: &[ShotDescription],
        plan_fp: &str,
    ) -> VimaxResult<Vec<Camera>> {
        let path = self.working_dir.join("camera_tree.json");
        let mut cameras = load_or_write_json_cached(&path, plan_fp, || async {
            let cameras = group_shots_into_cameras(shot_descriptions);
            self.camera_gen
                .construct_camera_tree(&cameras, shot_descriptions)
                .await
        })
        .await?;
        // Always sanitize — cached trees from earlier LLM output may self-reference.
        sanitize_camera_tree(&mut cameras);
        write_json_artifact(&path, &cameras).await?;
        super::artifact_cache::write_sidecar(&path, plan_fp).await?;
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

    /// TTS voice-reference clips for cast members (category=8 models).
    async fn ensure_character_voice_references(
        &self,
        characters: &[CharacterInScene],
        progress: &Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        let Some(flowy) = self.backends.flowy.clone() else {
            tracing::warn!("Flowy services unavailable — skipping voice reference TTS");
            return Ok(());
        };
        let film_root = resolve_film_root(&self.working_dir);
        let portraits_dir = film_root.join("character_portraits");
        let registry_path = film_root.join("character_portraits_registry.json");
        let mut registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
            if registry_path.exists() {
                read_json_artifact(&registry_path).await?
            } else {
                HashMap::new()
            };
        let voice_gen = VoiceReferenceGenerator::new(flowy);
        let n = voice_gen
            .ensure_voice_references(characters, &portraits_dir, &mut registry)
            .await?;
        write_json_artifact(&registry_path, &registry).await?;
        if film_root != self.working_dir {
            write_json_artifact(
                &self.working_dir.join("character_portraits_registry.json"),
                &registry,
            )
            .await?;
        }
        if n > 0 {
            emit(
                progress,
                "voice_references_done",
                &format!("generated {n} voice reference clip(s)"),
            );
        }
        Ok(())
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
    /// Timeline-adjacent shots pass the previous clip's `video_last_frame.png` as a
    /// `reference_image` (multi-ref R2V continuity + prompt Image 1 binding).
    /// `prior_continuity` seeds the first shot of this scene from the previous
    /// scene's last shot (cross-scene match-cut).
    async fn generate_videos_sequential(
        &self,
        shots: &[ShotDescription],
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        world_pairs: &[(PathBuf, String)],
        style: &str,
        progress: &Option<ProgressCallback>,
        prior_continuity: Option<&Path>,
    ) -> VimaxResult<()> {
        // Always drive continuity by timeline idx, not whatever order the planner cached.
        let mut shots = shots.to_vec();
        shots.sort_by_key(|s| s.idx);
        let shots = &shots[..];

        let total = shots.len().max(1);
        let mut ok = 0usize;
        let mut errors: Vec<String> = Vec::new();
        let target = load_target_duration_secs(&self.working_dir).await;
        let clip = self.backends.clip;
        let needs: Vec<u32> = shots
            .iter()
            .map(|s| super::clip_beats::clip_need_secs(clip, s))
            .collect();
        let clip_durs = crate::planning::allocate_clip_durations_for_content(clip, target, &needs);
        let scene_bgm = load_scene_bgm_paren(&self.working_dir).await;
        tracing::info!(
            target = ?target,
            needs = ?needs,
            durations = ?clip_durs,
            scene_bgm = %scene_bgm,
            prior_continuity = ?prior_continuity.map(|p| p.display().to_string()),
            "content-aware shot durations (audio+motion)"
        );
        let phase_started = std::time::Instant::now();

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
            emit_pct_meta(
                progress,
                "video_clip_start",
                &format!("串行生成镜头视频（{}/{}）· 镜头 {}", i + 1, total, shot.idx),
                pct,
                serde_json::json!({ "shot_idx": shot.idx }),
            );

            // Timeline-adjacent continuity: previous shot (or prior scene tail) ending still.
            // The seam decides what the still MEANS to this shot — resume the
            // same take, or just carry identity across a cut — and the scene
            // concat re-derives it from the previous clip's *exit* camera.
            let seam = match i.checked_sub(1) {
                Some(prev) => media_local::SpliceSeam::within_scene(
                    shots[prev].exit_cam_idx(),
                    shot.cam_idx,
                ),
                // Cross-scene: the prompt lets the new scene change camera or
                // location, so its opening shot is a match-cut, never a resume.
                None => media_local::SpliceSeam::MatchCut,
            };
            let continuity_first = if i > 0 {
                let prev = &shots[i - 1];
                match ensure_shot_video_last_frame(&self.working_dir, prev.idx, false).await {
                    Ok(Some(path)) => {
                        let vendor_url = media_local::load_return_last_frame_url(&path);
                        emit(
                            progress,
                            "video_continuity",
                            &format!(
                                "Shot {}: reference_image ← shot {} {} (cam {}→{}, {seam:?})",
                                shot.idx,
                                prev.idx,
                                if vendor_url.is_some() {
                                    "last_frame_url"
                                } else {
                                    "video_last_frame.png"
                                },
                                prev.exit_cam_idx(),
                                shot.cam_idx
                            ),
                        );
                        tracing::info!(
                            shot = shot.idx,
                            prev = prev.idx,
                            continuity = %path.display(),
                            vendor_last_frame_url = vendor_url.is_some(),
                            ?seam,
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
            } else if let Some(path) = prior_continuity {
                emit(
                    progress,
                    "video_continuity",
                    &format!(
                        "Shot {}: reference_image ← previous scene last-shot {} ({})",
                        shot.idx,
                        if media_local::load_return_last_frame_url(&path).is_some() {
                            "last_frame_url"
                        } else {
                            "video_last_frame"
                        },
                        path.file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("video_last_frame.png")
                    ),
                );
                tracing::info!(
                    shot = shot.idx,
                    continuity = %path.display(),
                    "cross-scene multi-ref continuity locked to prior scene video_last_frame"
                );
                Some(path.to_path_buf())
            } else {
                None
            };

            let duration_secs = clip_durs.get(i).copied().unwrap_or_else(|| {
                needs.get(i).copied().unwrap_or_else(|| {
                    crate::planning::clip_duration_secs(clip, target, shots.len())
                })
            });
            emit(
                progress,
                "video_duration",
                &format!(
                    "Shot {}: render {}s (need≈{}s from audio/motion, ≤{}s model max)",
                    shot.idx,
                    duration_secs,
                    needs.get(i).copied().unwrap_or(duration_secs),
                    clip.max_secs(),
                ),
            );

            // P0-2: while this shot's create+poll is in flight, warm the OSS URL cache
            // for the NEXT shot's fixed refs (cast/env/prop — the continuity frame is
            // unknown until this shot finishes). Best-effort: a failed warm-up just
            // falls back to the in-generate upload.
            if let Some(next) = shots.get(i + 1) {
                let warm_paths: Vec<PathBuf> = shot_video_ref_pairs(
                    next,
                    None,
                    characters,
                    registry,
                    world_pairs,
                    &resolve_film_root(&self.working_dir),
                )
                .into_iter()
                .map(|(p, _)| p)
                .collect();
                if !warm_paths.is_empty()
                    && let Some(flowy) = self.backends.flowy.clone()
                {
                    let warm_paths_clone = warm_paths.clone();
                    tokio::spawn(async move {
                        for p in &warm_paths_clone {
                            if let Err(e) =
                                flowy.upload_image_public_url(p, "video_prewarm").await
                            {
                                tracing::debug!(
                                    path = %p.display(),
                                    error = %e,
                                    "video ref prewarm failed; in-generate upload will retry"
                                );
                                break;
                            }
                        }
                    });
                    tracing::debug!(
                        next_shot = next.idx,
                        prewarm_paths = warm_paths.len(),
                        "prewarming next shot OSS refs during current poll"
                    );
                }
            }

            let shot_started = std::time::Instant::now();
            match self
                .generate_video_for_shot(
                    shot,
                    duration_secs,
                    continuity_first.as_deref(),
                    seam,
                    characters,
                    registry,
                    world_pairs,
                    style,
                    &scene_bgm,
                    progress,
                )
                .await
            {
                Ok(()) => {
                    ok += 1;
                    tracing::info!(
                        phase = "video_shot",
                        shot = shot.idx,
                        secs = shot_started.elapsed().as_secs_f64(),
                        "shot video wall time"
                    );
                    // Prefer API return_last_frame; ffmpeg-extract if still missing.
                    let _ = ensure_shot_video_last_frame(&self.working_dir, shot.idx, false).await;
                    emit_pct_meta(
                        progress,
                        "video_clip_done",
                        &format!("Shot {} ready ({ok}/{total})", shot.idx),
                        55.0 + 40.0 * ((i + 1) as f32 / total as f32),
                        serde_json::json!({ "shot_idx": shot.idx }),
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
                    // User cancelled — stop gracefully without an error message.
                    if VimaxError::is_cancelled(&e) {
                        return Err(e);
                    }
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
        tracing::info!(
            phase = "video_phase_total",
            secs = phase_started.elapsed().as_secs_f64(),
            shots = shots.len(),
            "sequential video phase wall time"
        );
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
            let mut available: Vec<(PathBuf, String)> = portrait_pairs(
                characters,
                &first_shot.ff_vis_char_idxs,
                registry,
                &resolve_film_root(&self.working_dir),
            );
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
                let mut available = portrait_pairs(
                    characters,
                    &first_shot.lf_vis_char_idxs,
                    registry,
                    &resolve_film_root(&self.working_dir),
                );
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
                let mut available = portrait_pairs(
                    characters,
                    &shot.ff_vis_char_idxs,
                    registry,
                    &resolve_film_root(&self.working_dir),
                );
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
                    let mut available = portrait_pairs(
                        characters,
                        &shot.lf_vis_char_idxs,
                        registry,
                        &resolve_film_root(&self.working_dir),
                    );
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
        #[derive(serde::Deserialize)]
        struct SavedSelector {
            #[serde(default)]
            reference_image_path_and_text_pairs: Vec<(String, String)>,
            #[serde(default)]
            text_prompt: String,
            #[serde(default)]
            full_prompt: Option<String>,
            #[serde(default)]
            prompt_override: bool,
        }

        let saved_override = if selector_path.exists() {
            read_json_artifact::<SavedSelector>(&selector_path)
                .await
                .ok()
        } else {
            None
        };

        // User-edited full prompt: regenerate with the override text as-is.
        if let Some(saved) = &saved_override {
            if saved.prompt_override {
                if let Some(full_prompt) = saved
                    .full_prompt
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                {
                    let mut pairs: Vec<(PathBuf, String)> = saved
                        .reference_image_path_and_text_pairs
                        .iter()
                        .map(|(p, t)| (PathBuf::from(p), t.clone()))
                        .collect();
                    ensure_frame_refs(&mut pairs, available, characters, vis_char_idxs);
                    let refs: Vec<&Path> = pairs.iter().map(|(p, _)| p.as_path()).collect();
                    self.backends
                        .image
                        .generate(full_prompt, &refs, out_path)
                        .await?;
                    emit(
                        progress,
                        "frame_done",
                        &format!(
                            "Generated {frame_type} (prompt override) at {}",
                            out_path.display()
                        ),
                    );
                    return Ok(());
                }
            }
        }

        let (mut pairs, prompt) = if let Some(saved) = saved_override {
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

        // Persist the exact prompt used for generation so creators can fine-tune it later.
        let saved_pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(p, t)| (p.to_string_lossy().to_string(), t.clone()))
            .collect();
        let _ = write_json_artifact(
            &selector_path,
            &serde_json::json!({
                "reference_image_path_and_text_pairs": saved_pairs,
                "text_prompt": prompt,
                "full_prompt": full_prompt,
                "prompt_override": false,
            }),
        )
        .await;
        let prompt_txt = shot_dir.join(format!("{frame_type}_generation_prompt.txt"));
        let _ = crate::session::write_text_artifact(&prompt_txt, &full_prompt).await;

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

    /// Render one shot's clip.
    ///
    /// `seam` describes how this shot joins the previous one and is what keeps
    /// the prompt honest about `continuity_first_frame`: same take (resume from
    /// it) vs. new angle (identity reference only). It is downgraded to
    /// [`SpliceSeam::Cut`] when no usable frame arrives, so the prompt never
    /// talks about an `@Image1` that is not bound.
    async fn generate_video_for_shot(
        &self,
        shot: &ShotDescription,
        duration_secs: u32,
        continuity_first_frame: Option<&Path>,
        seam: SpliceSeam,
        characters: &[CharacterInScene],
        registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
        world_pairs: &[(PathBuf, String)],
        style: &str,
        scene_bgm: &str,
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
        let seam = if using_video_continuity {
            seam
        } else {
            SpliceSeam::Cut
        };

        let ref_pairs = shot_video_ref_pairs(
            shot,
            continuity_source.as_deref(),
            characters,
            registry,
            world_pairs,
            &resolve_film_root(&self.working_dir),
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
        let film_root = resolve_film_root(&self.working_dir);
        let voice_ref_path = shot_speaker_voice_ref_path(shot, characters, registry, &film_root);
        let speaker_name = shot_primary_speaker_name(shot, characters, registry, &film_root);
        // Voice clips invite invented speech on silent shots; only bind when this beat talks.
        let use_voice_audio_ref = voice_ref_path.is_some() && shot_has_spoken_dialogue(shot);
        let ref_audio = if use_voice_audio_ref {
            voice_ref_path.as_deref()
        } else {
            None
        };
        let aspect_ratio = crate::aspect::load_aspect_from_dir(&self.working_dir).await;
        let prompt = i2v_motion_prompt(
            shot,
            characters,
            style,
            &ref_pairs,
            self.backends.clip,
            duration_secs,
            seam,
            scene_bgm,
            use_voice_audio_ref,
            speaker_name.as_deref(),
            &aspect_ratio,
        );
        // P0-3: soften risky wording (motion / plot / audio captions) before the
        // first submission so Seedance content filters don't reject the prompt text.
        let prompt = crate::prompt_safety::sanitize_video_prompt(&prompt);
        let ref_names: Vec<String> = ref_pairs
            .iter()
            .map(|(p, _)| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("ref.png")
                    .to_string()
            })
            .collect();
        emit_meta(
            progress,
            "video_clip_start",
            &format!(
                "Generating shot {} video ({}s; multi-ref ×{}; continuity={}; voice_ref={}; refs=[{}])",
                shot.idx,
                duration_secs,
                ref_paths.len(),
                using_video_continuity,
                use_voice_audio_ref,
                ref_names.join(", ")
            ),
            serde_json::json!({ "shot_idx": shot.idx }),
        );
        tracing::info!(
            shot = shot.idx,
            refs = ?ref_names,
            continuity = using_video_continuity,
            "video multi-ref R2V binding"
        );

        preflight_video_ref_privacy(self.backends.image.as_ref(), &ref_paths).await?;

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
                None,
                ref_audio,
            )
            .await
        {
            Ok(()) => None,
            Err(err) if is_video_sensitive_text_err(&err) => {
                // Content filter rejected the prompt TEXT (e.g.
                // InputTextSensitiveContentDetected). Retry once with a
                // strict-softened wording on the SAME refs — the multi-ref
                // drop-continuity / T2V cascade would only repeat the risky
                // wording, so fail fast if the text is still blocked.
                let strict_prompt =
                    crate::prompt_safety::sanitize_video_prompt_strict(&prompt);
                emit(
                    progress,
                    "video_clip_start",
                    &format!(
                        "Shot {}: 内容安全拦截提示词（{}）。已改用软化措辞重试一次",
                        shot.idx,
                        truncate_err(&err, 100)
                    ),
                );
                match self
                    .backends
                    .video
                    .generate(
                        &strict_prompt,
                        None,
                        None,
                        &ref_paths,
                        duration_secs,
                        &video_path,
                        Some(&video_last_frame_path),
                        None,
                        ref_audio,
                    )
                    .await
                {
                    Ok(()) => None,
                    Err(err2) if is_video_sensitive_text_err(&err2) => {
                        tracing::warn!(
                            shot = shot.idx,
                            error = %err2,
                            "sensitive prompt text rejected after strict soften; failing shot fast"
                        );
                        return Err(err2);
                    }
                    Err(err2) if should_retry_seedance_without_photoreal_frame(&err2) => {
                        Some(err2)
                    }
                    Err(err2) => return Err(err2),
                }
            }
            Err(err) if should_retry_seedance_without_photoreal_frame(&err) => Some(err),
            Err(err) => return Err(err),
        };

        if let Some(mut err) = first_err {
            // 1) Privacy repair: locate content[N] or sweep face-bearing subject refs,
            //    force re-edit from privacy_raw (do not trust prior markers after reject).
            // 2) If still blocked, drop continuity still, then pure T2V.
            let working_pairs = ref_pairs.clone();
            let mut privacy_attempts: Vec<(PathBuf, PrivacyFaceTier)> = Vec::new();
            let mut privacy_resolved = false;

            for repair_i in 0..MAX_PRIVACY_FACE_REPAIRS_PER_SHOT {
                if !should_retry_seedance_without_photoreal_frame(&err)
                    && !is_seedance_privacy_image_err(&err)
                {
                    break;
                }
                let err_text = err.to_string();
                let targets = privacy_repair_targets(&err_text, &working_pairs);
                if targets.is_empty() {
                    tracing::info!(
                        shot = shot.idx,
                        repair_i,
                        "privacy reject with no repairable image refs; skipping face repair"
                    );
                    break;
                }

                let targeted = parse_seedance_flagged_content_index(&err_text).is_some()
                    && targets.len() == 1;
                let labels: Vec<String> = targets
                    .iter()
                    .filter_map(|&i| working_pairs.get(i))
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
                        "Shot {}: Seedance 图片风控。正在重新检测并电影化虚构脸（{}：{}）…",
                        shot.idx,
                        if targeted { "定位" } else { "主体图扫描" },
                        labels.join(", ")
                    ),
                );

                let mut any_rewrite = false;
                let mut all_exhausted = true;
                for &slot in &targets {
                    let flagged_path = working_pairs[slot].0.clone();
                    
                    // Use AI Sanitization strategy for better video model compliance
                    // This uses vision description + T2I instead of img2img
                    match ensure_ai_sanitized_privacy_face(
                        Arc::clone(&self.backends.image),
                        Arc::clone(&self.backends.chat),
                        &flagged_path,
                        style,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            tracing::info!(
                                shot = shot.idx,
                                path = %flagged_path.display(),
                                ?outcome,
                                "AI sanitization privacy repair applied"
                            );
                            if matches!(outcome, PrivacyFaceOutcome::Rewritten) {
                                any_rewrite = true;
                            }
                            privacy_attempts.push((flagged_path, PrivacyFaceTier::Soft));
                        }
                        Err(repair_err) => {
                            tracing::warn!(
                                shot = shot.idx,
                                path = %flagged_path.display(),
                                error = %repair_err,
                                "AI sanitization failed; falling back to traditional img2img"
                            );
                            // Fallback to traditional img2img
                            let Some(tier) =
                                next_privacy_tier_for_path(&flagged_path, &privacy_attempts)
                            else {
                                continue;
                            };
                            all_exhausted = false;
                            match ensure_seedance_privacy_face(
                                self.backends.image.as_ref(),
                                &flagged_path,
                                tier,
                                true,
                            )
                            .await
                            {
                                Ok(outcome) => {
                                    if matches!(outcome, PrivacyFaceOutcome::Rewritten) {
                                        any_rewrite = true;
                                    }
                                    privacy_attempts.push((flagged_path, tier));
                                }
                                Err(fallback_err) => {
                                    tracing::warn!(
                                        shot = shot.idx,
                                        path = %flagged_path.display(),
                                        error = %fallback_err,
                                        "privacy face repair failed for one ref; continuing"
                                    );
                                    privacy_attempts.push((flagged_path, tier));
                                }
                            }
                        }
                    }
                }

                if all_exhausted && !any_rewrite {
                    tracing::warn!(
                        shot = shot.idx,
                        "privacy face tiers exhausted for all targeted refs"
                    );
                    break;
                }

                let retry_paths: Vec<&Path> =
                    working_pairs.iter().map(|(p, _)| p.as_path()).collect();
                let retry_prompt = i2v_motion_prompt(
                    shot,
                    characters,
                    style,
                    &working_pairs,
                    self.backends.clip,
                    duration_secs,
                    // A privacy rewrite may have dropped the continuity still;
                    // without it bound there is nothing for the seam to mean.
                    if continuity_source
                        .as_ref()
                        .is_some_and(|c| working_pairs.iter().any(|(p, _)| p == c))
                    {
                        seam
                    } else {
                        SpliceSeam::Cut
                    },
                    scene_bgm,
                    use_voice_audio_ref,
                    speaker_name.as_deref(),
                    &aspect_ratio,
                );
                let retry_prompt = crate::prompt_safety::sanitize_video_prompt(&retry_prompt);
                match self
                    .backends
                    .video
                    .generate(
                        &retry_prompt,
                        None,
                        None,
                        &retry_paths,
                        duration_secs,
                        &video_path,
                        Some(&video_last_frame_path),
                        None,
                        ref_audio,
                    )
                    .await
                {
                    Ok(()) => {
                        privacy_resolved = true;
                        break;
                    }
                    Err(retry_err)
                        if should_retry_seedance_without_photoreal_frame(&retry_err)
                            || is_seedance_privacy_image_err(&retry_err) =>
                    {
                        err = retry_err;
                    }
                    Err(retry_err) => return Err(retry_err),
                }
            }

            if !privacy_resolved {
                // Drop continuity still (may still trip privacy on rare gateways) and retry
                // with T2I cast/env/prop refs only; then pure T2V.
                let asset_pairs: Vec<(PathBuf, String)> = working_pairs
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
                        self.backends.clip,
                        duration_secs,
                        // Retrying without the continuity still at all.
                        SpliceSeam::Cut,
                        scene_bgm,
                        use_voice_audio_ref,
                        speaker_name.as_deref(),
                        &aspect_ratio,
                    );
                    let retry_prompt = crate::prompt_safety::sanitize_video_prompt(&retry_prompt);
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
                            None,
                            ref_audio,
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
                            let t2v_prompt =
                                crate::prompt_safety::sanitize_video_prompt(&t2v_prompt);
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
                                    None,
                                    // Seedance: audio cannot be the sole reference input.
                                    None,
                                )
                                .await
                                .map_err(|t2v_err| {
                                    VimaxError::Video(format!(
                                        "Shot {} video failed (multi-ref → privacy-face → drop continuity → text-to-video). First: {}; Final: {t2v_err}",
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
                    let t2v_prompt = crate::prompt_safety::sanitize_video_prompt(&t2v_prompt);
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
                            None,
                            // Seedance: audio cannot be the sole reference input.
                            None,
                        )
                        .await
                        .map_err(|t2v_err| {
                            VimaxError::Video(format!(
                                "Shot {} video failed (multi-ref → privacy-face → text-to-video). First: {}; Final: {t2v_err}",
                                shot.idx,
                                truncate_err(&err, 160)
                            ))
                        })?;
                }
            }
        }

        if !media_local::is_usable_video_file(&video_path) {
            return Err(VimaxError::Video(format!(
                "Shot {} video file invalid after generation",
                shot.idx
            )));
        }
        emit_pct_meta(
            progress,
            "video_clip_done",
            &format!("Shot {} video saved", shot.idx),
            99.0,
            serde_json::json!({ "shot_idx": shot.idx }),
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

        let mut available = portrait_pairs(
            characters,
            vis_char_idxs,
            registry,
            &resolve_film_root(&self.working_dir),
        );
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
    film_root: &Path,
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
                        let path = crate::session::resolve_stored_asset_path(p, film_root);
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
                        let path = crate::session::resolve_stored_asset_path(p, film_root);
                        if !media_local::is_usable_image_file(&path) {
                            continue;
                        }
                        available.push((
                            path,
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
                desc.push_str(&crate::planning::clip_at_break(static_f, 120));
            }
            if !dynamic_f.is_empty() {
                if !desc.is_empty() {
                    desc.push_str("; ");
                }
                desc.push_str(&crate::planning::clip_at_break(dynamic_f, 80));
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
/// Seedance identity dilutes past ~2–3 people; keep video portraits tighter than stills.
const MAX_VIDEO_PORTRAIT_REFS: usize = 3;
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
            || s.contains("/references/by_category/character")
        {
            portraits.push((p, t));
        } else if s.contains("environments")
            || s.contains("/by_category/environment")
            || s.contains("/by_category/style")
        {
            envs.push((p, t));
        } else if s.contains("props") || s.contains("/by_category/prop") {
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
            || s.contains("/references/by_category/character")
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
        media_local::clear_return_last_frame_url(&out);
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

/// Highest shot idx that has a usable `video.mp4` under `scene_working_dir/shots/`.
async fn last_shot_idx_in_scene(scene_working_dir: &Path) -> Option<i32> {
    let desc_path = scene_working_dir.join("shot_descriptions.json");
    if let Ok(shots) = read_json_artifact::<Vec<ShotDescription>>(&desc_path).await {
        if let Some(idx) = shots.iter().map(|s| s.idx).max() {
            return Some(idx);
        }
    }
    let shots_dir = scene_working_dir.join("shots");
    let mut max_idx: Option<i32> = None;
    let Ok(mut rd) = tokio::fs::read_dir(&shots_dir).await else {
        return None;
    };
    while let Ok(Some(ent)) = rd.next_entry().await {
        let name = ent.file_name();
        let Ok(idx) = name.to_string_lossy().parse::<i32>() else {
            continue;
        };
        let video = ent.path().join("video.mp4");
        if media_local::is_usable_video_file(&video) {
            max_idx = Some(max_idx.map_or(idx, |m| m.max(idx)));
        }
    }
    max_idx
}

/// Previous scene's last-shot `video_last_frame.png` for cross-scene match-cut continuity.
pub async fn resolve_scene_tail_continuity(scene_working_dir: &Path) -> Option<PathBuf> {
    let last_idx = last_shot_idx_in_scene(scene_working_dir).await?;
    match ensure_shot_video_last_frame(scene_working_dir, last_idx, false).await {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(
                scene = %scene_working_dir.display(),
                last_idx,
                error = %e,
                "failed to resolve scene tail continuity frame"
            );
            None
        }
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

/// Drop cached decompose / camera tree and unused shot dirs after the
/// storyboard list changes (truncate or pack). Directories that already have
/// a clip are kept so resume does not delete billed work.
async fn invalidate_downstream_of_storyboard(
    working_dir: &Path,
    storyboard: &[ShotBriefDescription],
) {
    let decomp = working_dir.join("shot_descriptions.json");
    if decomp.exists() {
        let _ = tokio::fs::remove_file(&decomp).await;
    }
    let cam = working_dir.join("camera_tree.json");
    if cam.exists() {
        let _ = tokio::fs::remove_file(&cam).await;
    }
    prune_unkept_shot_dirs(working_dir, storyboard).await;
}

/// Write `storyboard.json` only as the published clip list.
///
/// Returns whether the file changed. A matching sidecar is always kept so
/// resume still skips the LLM draft; callers must pass the packed (and
/// optionally in-clip-densified) rows, never the pre-pack draft.
async fn persist_published_storyboard(
    working_dir: &Path,
    path: &Path,
    board: &[ShotBriefDescription],
    plan_fp: &str,
) -> VimaxResult<bool> {
    let on_disk = if path.is_file() {
        read_json_artifact::<Vec<ShotBriefDescription>>(path)
            .await
            .ok()
    } else {
        None
    };
    if on_disk
        .as_ref()
        .is_some_and(|disk| !super::clip_beats::storyboard_differs(disk, board))
    {
        if !sidecar_matches(path, plan_fp).await {
            write_sidecar(path, plan_fp).await?;
        }
        return Ok(false);
    }
    write_json_artifact(path, &board).await?;
    write_sidecar(path, plan_fp).await?;
    invalidate_downstream_of_storyboard(working_dir, board).await;
    Ok(true)
}

/// Persist the packed clip list as consecutive `shots/0..n` dirs.
///
/// Clip packing keeps each run's first idx, which leaves holes the UI reads
/// as skipped middles. Prune absorbed dirs first (keep billed video), then
/// rename survivors onto `0..n` and rewrite per-shot JSON.
async fn commit_packed_shot_layout(
    working_dir: &Path,
    mut synced: Vec<ShotBriefDescription>,
    mut packed: Vec<ShotDescription>,
) -> (
    Vec<ShotBriefDescription>,
    Vec<ShotDescription>,
    HashMap<i32, i32>,
) {
    packed.sort_by_key(|clip| clip.idx);
    prune_unkept_shot_dirs(working_dir, &synced).await;
    let briefs_dense = synced
        .iter()
        .enumerate()
        .all(|(i, brief)| brief.idx == i as i32);
    let map = if super::clip_beats::clip_indices_are_dense(&packed) && briefs_dense {
        packed.iter().map(|clip| (clip.idx, clip.idx)).collect()
    } else {
        let map = super::clip_beats::densify_aligned_indices(&mut synced, &mut packed);
        relocate_shot_dirs(working_dir, &map).await;
        prune_unkept_shot_dirs(working_dir, &synced).await;
        map
    };
    for clip in &packed {
        let path = working_dir
            .join("shots")
            .join(clip.idx.to_string())
            .join("shot_description.json");
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = write_json_artifact(&path, clip).await;
    }
    (synced, packed, map)
}

async fn relocate_shot_dirs(working_dir: &Path, old_to_new: &HashMap<i32, i32>) {
    let shots_root = working_dir.join("shots");
    if !shots_root.is_dir() {
        return;
    }
    let moves: Vec<(i32, i32)> = old_to_new
        .iter()
        .filter_map(|(&old, &new)| (old != new).then_some((old, new)))
        .collect();
    if moves.is_empty() {
        return;
    }

    for (old, _) in &moves {
        let from = shots_root.join(old.to_string());
        if !from.exists() {
            continue;
        }
        let tmp = shots_root.join(format!(".reloc-{old}"));
        if tmp.exists() {
            let _ = tokio::fs::remove_dir_all(&tmp).await;
        }
        if let Err(e) = tokio::fs::rename(&from, &tmp).await {
            tracing::warn!(
                from = %from.display(),
                error = %e,
                "could not park shot dir for reindex"
            );
        }
    }

    for (old, new) in &moves {
        let tmp = shots_root.join(format!(".reloc-{old}"));
        if !tmp.exists() {
            continue;
        }
        let to = shots_root.join(new.to_string());
        if to.exists() {
            park_or_delete_shot_dir(&to).await;
        }
        if let Err(e) = tokio::fs::rename(&tmp, &to).await {
            tracing::warn!(
                from = %tmp.display(),
                to = %to.display(),
                error = %e,
                "could not move shot dir onto dense idx"
            );
        }
    }
}

async fn park_or_delete_shot_dir(path: &Path) {
    let video = path.join("video.mp4");
    if media_local::is_usable_video_file(&video) {
        let Some(parent) = path.parent() else {
            return;
        };
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("shot");
        let parked = parent.join("_absorbed").join(name);
        if let Some(dir) = parked.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        if parked.exists() {
            let _ = tokio::fs::remove_dir_all(&parked).await;
        }
        if tokio::fs::rename(path, &parked).await.is_ok() {
            tracing::info!(shot = name, "parked absorbed shot dir with billed video");
        }
        return;
    }
    let _ = tokio::fs::remove_dir_all(path).await;
}

fn remap_camera_tree_shot_idxs(cameras: &mut [Camera], old_to_new: &HashMap<i32, i32>) -> bool {
    if old_to_new.iter().all(|(old, new)| old == new) {
        return false;
    }
    for camera in cameras.iter_mut() {
        camera.active_shot_idxs = camera
            .active_shot_idxs
            .iter()
            .filter_map(|idx| old_to_new.get(idx).copied())
            .collect();
        if let Some(parent) = camera.parent_shot_idx {
            camera.parent_shot_idx = old_to_new.get(&parent).copied();
        }
    }
    true
}

async fn prune_unkept_shot_dirs(working_dir: &Path, storyboard: &[ShotBriefDescription]) {
    let keep: HashSet<i32> = storyboard.iter().map(|s| s.idx).collect();
    let shots_root = working_dir.join("shots");
    if !shots_root.is_dir() {
        return;
    }
    let Ok(mut entries) = tokio::fs::read_dir(&shots_root).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(idx) = name.parse::<i32>() else {
            continue;
        };
        if keep.contains(&idx) {
            continue;
        }
        let video = entry.path().join("video.mp4");
        if video.is_file() {
            tracing::info!(
                shot = idx,
                "keeping absorbed shot dir; video already billed"
            );
            continue;
        }
        let _ = tokio::fs::remove_dir_all(entry.path()).await;
    }
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
            "环境底噪与连贯电影感背景音乐，配合画面动作的细微拟音".to_string()
        };
        shot.audio_desc = Some(fill);
        changed = true;
    }
    changed
}

/// Lock one scene-level BGM caption for every shot (written to `bgm_brief.txt`).
async fn ensure_scene_bgm_brief(
    working_dir: &Path,
    storyboard: &[ShotBriefDescription],
) -> VimaxResult<String> {
    let path = working_dir.join("bgm_brief.txt");
    let existing = match tokio::fs::read_to_string(&path).await {
        Ok(s) if !s.trim().is_empty() => Some(s),
        _ => None,
    };
    let audio_descs: Vec<Option<&str>> = storyboard
        .iter()
        .map(|s| s.audio_desc.as_deref())
        .collect();
    let bgm = crate::planning::resolve_scene_bgm_paren(existing.as_deref(), &audio_descs);
    write_text_artifact(&path, &bgm).await?;
    Ok(bgm)
}

async fn load_scene_bgm_paren(working_dir: &Path) -> String {
    let path = working_dir.join("bgm_brief.txt");
    match tokio::fs::read_to_string(&path).await {
        Ok(s) if !s.trim().is_empty() => crate::planning::format_scene_bgm_paren(&s),
        _ => crate::planning::DEFAULT_SCENE_BGM_PAREN.to_string(),
    }
}

fn path_key_lower(p: &Path) -> String {
    p.to_string_lossy().to_ascii_lowercase()
}

fn is_portrait_ref_path(p: &Path) -> bool {
    let s = path_key_lower(p);
    s.contains("character_portrait")
        || s.contains("three_view")
        || s.contains("_cameo")
        || s.contains("/cameo/")
        || s.contains("/references/by_category/character")
        || s.contains("video_front")
}

fn is_environment_ref_path(p: &Path) -> bool {
    let s = path_key_lower(p);
    s.contains("environments")
        || s.contains("/by_category/environment")
        || s.contains("environment_plate")
}

fn is_prop_ref_path(p: &Path) -> bool {
    let s = path_key_lower(p);
    s.contains("/props/")
        || s.contains("\\props\\")
        || s.contains("/by_category/prop")
        || s.contains("_prop.")
        || s.contains("_prop.png")
}

fn extract_bracket_name(text: &str) -> Option<&str> {
    let start = text.find('<')?;
    let rest = &text[start + 1..];
    let end = rest.find('>')?;
    let name = rest[..end].trim();
    (!name.is_empty()).then_some(name)
}

fn prop_duplicates_visible_cast(
    path: &Path,
    text: &str,
    characters: &[CharacterInScene],
    vis: &[i32],
) -> bool {
    let blob = format!("{} {}", path.to_string_lossy(), text);
    vis.iter().any(|&ci| {
        characters.iter().any(|ch| {
            ch.idx == ci
                && ch.identifier_in_scene.trim().chars().count() >= 2
                && blob.contains(ch.identifier_in_scene.trim())
        })
    })
}

fn shot_has_spoken_dialogue(shot: &ShotDescription) -> bool {
    crate::planning::text_looks_like_dialogue(shot.audio_desc.as_deref().unwrap_or(""))
        || crate::planning::text_looks_like_dialogue(&shot.motion_desc)
        || crate::planning::text_looks_like_dialogue(&shot.visual_desc)
}

fn beats_are_redundant(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if a.is_empty() || b.is_empty() {
        return a == b;
    }
    let norm = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).take(56).collect() };
    let na = norm(a);
    let nb = norm(b);
    if na == nb {
        return true;
    }
    let pa: String = na.chars().take(12).collect();
    let pb: String = nb.chars().take(12).collect();
    pa.chars().count() >= 8
        && pb.chars().count() >= 8
        && (na.starts_with(&pb) || nb.starts_with(&pa))
}

fn plot_beat_clause(ff: &str, lf: &str, motion: &str) -> String {
    let ff = ff.trim();
    let lf = lf.trim();
    if ff.is_empty() && lf.is_empty() {
        return String::new();
    }
    // Motion is the director's instruction; skip static recaps that duplicate it.
    if !motion.is_empty() && beats_are_redundant(ff, lf) {
        return String::new();
    }
    let start = crate::planning::clip_at_break(ff, 72);
    let end = crate::planning::clip_at_break(lf, 72);
    if start.is_empty() {
        return String::new();
    }
    if end.is_empty() || beats_are_redundant(ff, lf) {
        format!("Start: {start}.")
    } else {
        format!("Start: {start}. End: {end}.")
    }
}

/// What a continuity still is for when the camera has *changed*.
///
/// The frame still carries everything that must not change (who, wearing what,
/// lit how, standing where) but its framing is now wrong by construction, so
/// asking the model to start from it makes the new angle re-stage the beat the
/// audience just watched — the splice reads as a freeze.
const CONTINUITY_STATE_ROLE: &str =
    "previous shot's final frame — continuity reference ONLY (cast identity, wardrobe, hair, props, \
set, lighting, time of day, and who is screen-left vs screen-right). Do NOT reproduce its framing \
and do NOT replay its pose; do NOT flip left/right geography";

/// What a continuity still is for when the camera is *unchanged*.
const CONTINUITY_RESUME_ROLE: &str =
    "previous shot's final frame — the SAME take keeps rolling: this IS the first frame, so pick the \
motion up from that exact instant without re-framing or restarting";

/// How this clip must begin, given how it joins the previous one.
///
/// This is the prompt half of the anti-stutter contract; the concat half is
/// [`media_local::SpliceSeam`], which trims the replayed head of a `SameTake`
/// clip. Both are derived from the same `cam_idx` comparison, so a shot is never
/// told to replay a beat that will not be trimmed, or trimmed after being told
/// not to replay.
fn clip_opening_clause(seam: SpliceSeam) -> &'static str {
    match seam {
        SpliceSeam::Cut => "",
        SpliceSeam::SameTake => {
            "Opening: one continuous take — the clip's first frame is @Image1. Do not restart, \
re-frame, or re-establish; the motion is already underway at frame 1."
        }
        SpliceSeam::MatchCut => {
            "Opening: this is a CUT to a NEW angle. Start with the action ALREADY in progress from \
the new camera. Never re-stage, re-enter, or replay @Image1's ending pose or framing — repeating \
that beat is what makes the join look frozen. Keep screen geography: whoever is left/right in \
@Image1 stays left/right unless this clip's motion itself says CUT TO a reverse / 反打 / 过肩."
        }
    }
}

fn video_ref_role(path: &Path, text: &str, seam: SpliceSeam) -> String {
    match seam {
        SpliceSeam::SameTake => return CONTINUITY_RESUME_ROLE.into(),
        SpliceSeam::MatchCut => return CONTINUITY_STATE_ROLE.into(),
        SpliceSeam::Cut => {}
    }
    if path_key_lower(path).contains("video_last_frame") {
        // A tail frame bound anywhere but Image 1 is state, never a start frame.
        return CONTINUITY_STATE_ROLE.into();
    }
    if is_portrait_ref_path(path) {
        let who = extract_bracket_name(text).unwrap_or("cast");
        return format!("<{who}> identity (face, hair, outfit)");
    }
    if is_environment_ref_path(path) {
        return "empty location plate (set only)".into();
    }
    if is_prop_ref_path(path) {
        let who = extract_bracket_name(text).unwrap_or("prop");
        return format!("<{who}> object only");
    }
    crate::planning::clip_at_break(text, 48)
}

/// `@ImageN` role list. Image 1 is the continuity still whenever `seam` is not a
/// [`SpliceSeam::Cut`]; every other slot is described by its own path.
fn video_at_image_bindings(ref_pairs: &[(PathBuf, String)], seam: SpliceSeam) -> String {
    if ref_pairs.is_empty() {
        return String::new();
    }
    let bits: Vec<String> = ref_pairs
        .iter()
        .enumerate()
        .map(|(i, (path, text))| {
            let slot_seam = if i == 0 { seam } else { SpliceSeam::Cut };
            format!("@Image{} {}", i + 1, video_ref_role(path, text, slot_seam))
        })
        .collect();
    bits.join(". ") + "."
}

fn video_cast_clause(
    characters: &[CharacterInScene],
    shot: &ShotDescription,
    _ref_pairs: &[(PathBuf, String)],
    style: &str,
    has_identity_refs: bool,
) -> String {
    let mut idxs = shot.ff_vis_char_idxs.clone();
    for &i in &shot.lf_vis_char_idxs {
        if !idxs.contains(&i) {
            idxs.push(i);
        }
    }
    let mut has_child = false;
    for &ci in &idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            if crate::planning::looks_like_child_character(
                &ch.identifier_in_scene,
                ch.static_features.trim(),
            ) {
                has_child = true;
                break;
            }
        }
    }
    let child_line = if has_child {
        if crate::planning::wants_stylized_non_photoreal(style) {
            "Children share the SAME animation style as adults."
        } else {
            "Children share the SAME cinematic style as adults."
        }
    } else {
        ""
    };
    if has_identity_refs {
        return child_line.to_string();
    }
    // T2V / no portrait refs: identity must live in text. Keep it short.
    let compact = character_identity_clause(characters, &idxs, style);
    if child_line.is_empty() {
        compact
    } else if compact.is_empty() {
        child_line.to_string()
    } else {
        compact
    }
}

/// Timed beat layout for a clip that plays more than one beat.
///
/// One clip playing several beats must be told *when* each beat happens.
/// Without the timeline the model lingers on the first beat and the rest never
/// arrive: the splice stutter is gone, but the clip reads as padded. The windows
/// come from the duration the render actually requests, so they can never
/// contradict it. Empty for the usual single-beat clip, which keeps its plain
/// `Motion:` line.
fn clip_beat_script(timeline: &[super::clip_beats::TimedBeat]) -> String {
    if timeline.is_empty() {
        return String::new();
    }
    let cuts = timeline.windows(2).any(|pair| {
        match (pair[0].cam_idx, pair[1].cam_idx) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    });
    let mut out = if cuts {
        String::from(
            "Motion: native multi-shot inside ONE generated clip. Play these beats in order. \
At a camera change, CUT to the new angle with the action already in progress — do not morph, \
dissolve, or replay the previous beat. Framing may change; identity, wardrobe, lighting mood, \
set, and who is screen-left vs screen-right do not (unless this beat is itself a 反打 / reverse).",
        )
    } else {
        String::from(
            "Motion: ONE continuous take — camera, framing and lighting never change. \
Play these beats in order, each in its own time window:",
        )
    };
    let mut prev_cam: Option<i32> = None;
    for beat in timeline {
        let text = match beat.text.trim() {
            "" => "hold the frame, subtle living motion only",
            text => text,
        };
        let cut = match (cuts, prev_cam, beat.cam_idx) {
            (true, Some(prev), Some(cam)) if prev != cam => "CUT TO a new camera. ",
            _ => "",
        };
        out.push_str(&format!(
            "\n[{}-{}s] {cut}{text}",
            beat.start_secs, beat.end_secs
        ));
        if beat.cam_idx.is_some() {
            prev_cam = beat.cam_idx;
        }
    }
    out
}

fn i2v_motion_prompt(
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    style: &str,
    ref_pairs: &[(PathBuf, String)],
    clip: ClipBounds,
    duration_secs: u32,
    seam: SpliceSeam,
    scene_bgm: &str,
    use_voice_audio_ref: bool,
    speaker_name: Option<&str>,
    aspect_ratio: &str,
) -> String {
    // Planner-authored timecodes are guesses made before the clip length was
    // allocated; the renderer owns time, so they never reach the model.
    let motion = super::clip_beats::strip_authored_timecodes(&shot.motion_desc);
    let motion = motion.as_str();
    let timeline = super::clip_beats::clip_timeline(clip, shot, duration_secs);
    let beat_script = clip_beat_script(&timeline);
    let has_dialogue = shot_has_spoken_dialogue(shot);
    let has_identity_refs = ref_pairs.iter().any(|(p, _)| is_portrait_ref_path(p));
    let style_clause = crate::planning::video_style_clause(style);
    let identity = video_cast_clause(characters, shot, ref_pairs, style, has_identity_refs);
    let plot = plot_beat_clause(&shot.ff_desc, &shot.lf_desc, motion);
    let ref_clause = video_at_image_bindings(ref_pairs, seam);
    let seam_clause = clip_opening_clause(seam);
    let speaker_idxs = speaker_idxs_for_shot(shot, characters);
    let voice_lock = if use_voice_audio_ref || !has_dialogue {
        String::new()
    } else {
        character_voice_lock_clause(characters, &speaker_idxs)
    };
    let audio_block = if use_voice_audio_ref {
        seedance_audio_caption_essential_only(
            shot.audio_desc.as_deref(),
            motion,
            &shot.visual_desc,
        )
    } else {
        seedance_audio_caption_block(
            shot.audio_desc.as_deref(),
            motion,
            &shot.visual_desc,
            characters,
            &speaker_idxs,
            scene_bgm,
        )
    };
    let audio_ref_clause = if use_voice_audio_ref {
        let who = speaker_name.unwrap_or("the speaking character");
        format!(
            "@Audio1 is the voice timbre bible for {who} — match speaker identity for dialogue exactly. \
No background music — dialogue and essential on-screen foley only."
        )
    } else {
        String::new()
    };
    let voice_continuity = if use_voice_audio_ref || !has_dialogue || voice_lock.is_empty() {
        String::new()
    } else {
        "Keep each speaker's FIXED SPEAKER VOICE identical across shots (emotion may shift; timbre must not)."
            .to_string()
    };
    let music_continuity = if use_voice_audio_ref {
        String::new()
    } else {
        "Keep the same scene underscore motif across cuts.".to_string()
    };

    let mut parts: Vec<String> = Vec::new();
    if !beat_script.is_empty() {
        parts.push(beat_script);
    } else if !motion.is_empty() {
        parts.push(format!("Motion: {motion}"));
    } else if !shot.visual_desc.trim().is_empty() {
        parts.push(format!(
            "Motion: {}",
            crate::planning::clip_at_break(shot.visual_desc.trim(), 160)
        ));
    }
    if !plot.is_empty() {
        parts.push(plot);
    }
    if !ref_clause.is_empty() {
        parts.push(ref_clause);
    }
    if !seam_clause.is_empty() {
        parts.push(seam_clause.to_string());
    }
    if !identity.trim().is_empty() {
        parts.push(identity.trim().to_string());
    }
    parts.push(style_clause);
    if !aspect_ratio.trim().is_empty() {
        parts.push(crate::aspect::video_aspect_framing_clause(aspect_ratio));
    }
    parts.push(crate::planning::i2v_duration_pacing_clause(
        duration_secs,
        has_dialogue,
        timeline.len().max(1) as u32,
    ));
    for extra in [audio_ref_clause, voice_lock, voice_continuity, music_continuity] {
        if !extra.trim().is_empty() {
            parts.push(extra.trim().to_string());
        }
    }
    parts.push(format!("Throughout: {audio_block}"));
    parts.push("Keep it subtitle-free.".into());
    parts.join("\n")
}

fn shot_speaker_voice_ref_path(
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    film_root: &Path,
) -> Option<PathBuf> {
    let idxs = speaker_idxs_for_shot(shot, characters);
    for &ci in &idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            if let Some(p) = voice_ref_abs_path(registry, &ch.identifier_in_scene, film_root) {
                return Some(p);
            }
        }
    }
    for ch in characters {
        if idxs.contains(&ch.idx) || shot.ff_vis_char_idxs.contains(&ch.idx) {
            if let Some(p) = voice_ref_abs_path(registry, &ch.identifier_in_scene, film_root) {
                return Some(p);
            }
        }
    }
    None
}

fn shot_primary_speaker_name(
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    film_root: &Path,
) -> Option<String> {
    let idxs = speaker_idxs_for_shot(shot, characters);
    for &ci in &idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            if voice_ref_abs_path(registry, &ch.identifier_in_scene, film_root).is_some() {
                return Some(ch.identifier_in_scene.clone());
            }
        }
    }
    characters
        .iter()
        .find(|ch| voice_ref_abs_path(registry, &ch.identifier_in_scene, film_root).is_some())
        .map(|ch| ch.identifier_in_scene.clone())
}

/// Dialogue + essential foley only — no BGM, no text voice-color locks (reference_audio carries timbre).
fn seedance_audio_caption_essential_only(
    audio_desc: Option<&str>,
    motion_desc: &str,
    visual_desc: &str,
) -> String {
    let audio = audio_desc.unwrap_or("").trim();
    let raw = if !audio.is_empty() {
        strip_bgm_stage_directions(&strip_conflicting_voice_color_cues(audio))
    } else if crate::planning::text_looks_like_dialogue(motion_desc) {
        strip_bgm_stage_directions(&strip_conflicting_voice_color_cues(motion_desc.trim()))
    } else if crate::planning::text_looks_like_dialogue(visual_desc) {
        strip_bgm_stage_directions(&strip_conflicting_voice_color_cues(
            &visual_desc.trim().chars().take(280).collect::<String>(),
        ))
    } else {
        String::new()
    };

    if raw.is_empty() {
        return "<environmental ambience and essential on-screen foley only — no music>"
            .to_string();
    }

    let has_typed = raw.contains('{')
        || raw.contains('}')
        || raw.contains('<')
        || raw.contains('>')
        || (raw.contains('(') && raw.contains(')'));

    if has_typed {
        return strip_bgm_stage_directions(&raw);
    }

    let looks_dialogue = crate::planning::text_looks_like_dialogue(&raw);
    if looks_dialogue {
        format!("{{{raw}}} <essential on-screen foley only — no music>")
    } else {
        format!("<{raw}>")
    }
}

fn strip_music_paren_segments(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Drop BGM stage directions that survive outside `(music)` captions (e.g. `BGM: 低音合成器…`).
fn strip_bgm_stage_directions(s: &str) -> String {
    let mut t = strip_music_paren_segments(s);
    const MARKERS: &[&str] = &[
        "BGM:",
        "BGM：",
        "bgm:",
        "Bgm:",
        "背景音乐",
        "配乐：",
        "配乐:",
    ];
    for m in MARKERS {
        if let Some(pos) = find_case_insensitive(&t, m) {
            t = t[..pos].trim().to_string();
        }
    }
    t
}

/// Multi-ref strip for Seedance R2V: optional previous video_last_frame + cast + env/prop.
fn shot_video_ref_pairs(
    shot: &ShotDescription,
    continuity: Option<&Path>,
    characters: &[CharacterInScene],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    world_pairs: &[(PathBuf, String)],
    film_root: &Path,
) -> Vec<(PathBuf, String)> {
    let mut pairs: Vec<(PathBuf, String)> = Vec::new();
    if let Some(path) = continuity.filter(|p| media_local::is_usable_image_file(p)) {
        pairs.push((
            path.to_path_buf(),
            "Previous timeline-adjacent shot ending frame — match-cut continuity; start motion immediately from this pose, framing, wardrobe, and set."
                .into(),
        ));
    }

    let mut vis: Vec<i32> = shot.ff_vis_char_idxs.clone();
    for &idx in &shot.lf_vis_char_idxs {
        if !vis.contains(&idx) {
            vis.push(idx);
        }
    }
    pairs.extend(
        portrait_pairs(characters, &vis, registry, film_root)
            .into_iter()
            .map(|(p, t)| {
                let front = media_local::ensure_three_view_front_panel(&p);
                (front, t)
            }),
    );

    let world_query = format!(
        "{} {} {}",
        shot.ff_desc.trim(),
        shot.motion_desc.trim(),
        shot.lf_desc.trim()
    );
    let mut world = rank_world_pairs_for_frame(&world_query, world_pairs, 4);
    if continuity.is_some() {
        // Last frame already contains the set; an empty env plate fights continuity.
        world.retain(|(p, _)| !is_environment_ref_path(p));
    }
    world.retain(|(p, t)| {
        !is_prop_ref_path(p) || !prop_duplicates_visible_cast(p, t, characters, &vis)
    });
    pairs.extend(world);

    // Dedup by path while preserving order (continuity first).
    let mut seen = std::collections::HashSet::new();
    pairs.retain(|(p, _)| seen.insert(p.clone()));

    let portrait_budget = vis.len().clamp(1, MAX_VIDEO_PORTRAIT_REFS);
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

/// Prefer characters who actually speak in this shot's audio, then visible cast.
fn speaker_idxs_for_shot(shot: &ShotDescription, characters: &[CharacterInScene]) -> Vec<i32> {
    let audio = shot.audio_desc.as_deref().unwrap_or("");
    let mut idxs: Vec<i32> = Vec::new();
    let mut push = |idx: i32| {
        if !idxs.contains(&idx) {
            idxs.push(idx);
        }
    };
    for ch in characters {
        let name = ch.identifier_in_scene.trim();
        if !name.is_empty() && audio.contains(name) {
            push(ch.idx);
        }
    }
    for &idx in &shot.ff_vis_char_idxs {
        push(idx);
    }
    for &idx in &shot.lf_vis_char_idxs {
        push(idx);
    }
    idxs
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
    // Ensure every cast member with a bible is available as fallback (capped).
    for ch in characters {
        push_ch(ch);
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "VOICE LOCK (immutable speaker identity — reuse these FIXED SPEAKER VOICE clauses \
exactly on every shot; emotion may vary slightly, timbre/pitch/age/gender must not): {}. ",
        parts.join(" | ")
    )
}

/// Seedance 2.0 audio captions use typed brackets:
/// dialogue `{…}`, SFX `<…>`, music `(…)`.
/// Empty captions with `generate_audio=true` fail with InvalidParameter.
///
/// Resolves audio from `audio_desc`, falling back to dialogue mined from
/// `motion_desc` / `visual_desc`. Injects per-character voice locks when the
/// text mentions a cast member, and always forces the scene-stable BGM caption
/// so adjacent shots share the same music intent (avoids abrupt motif jumps).
fn seedance_audio_caption_block(
    audio_desc: Option<&str>,
    motion_desc: &str,
    visual_desc: &str,
    characters: &[CharacterInScene],
    vis_idxs: &[i32],
    scene_bgm: &str,
) -> String {
    let bgm = crate::planning::format_scene_bgm_paren(scene_bgm);
    let audio = audio_desc.unwrap_or("").trim();
    let raw = if !audio.is_empty() {
        strip_conflicting_voice_color_cues(audio)
    } else if crate::planning::text_looks_like_dialogue(motion_desc) {
        strip_conflicting_voice_color_cues(motion_desc.trim())
    } else if crate::planning::text_looks_like_dialogue(visual_desc) {
        strip_conflicting_voice_color_cues(&visual_desc.trim().chars().take(280).collect::<String>())
    } else {
        String::new()
    };

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

    let voiced = if crate::planning::text_looks_like_dialogue(&raw) {
        inject_voice_into_audio_text(&raw, characters, vis_idxs)
    } else {
        raw.to_string()
    };

    if has_typed {
        return replace_or_append_bgm_paren(&voiced, &bgm);
    }

    let looks_dialogue = crate::planning::text_looks_like_dialogue(&voiced);
    if looks_dialogue {
        format!("{{{voiced}}} <scene-matched foley> {bgm}")
    } else {
        format!("<{voiced}> {bgm}")
    }
}

/// Force the scene-stable `(music)` caption: replace any existing `(…)` span,
/// otherwise append.
fn replace_or_append_bgm_paren(voiced: &str, scene_bgm: &str) -> String {
    let bgm = crate::planning::format_scene_bgm_paren(scene_bgm);
    if let Some(start) = voiced.find('(') {
        if let Some(rel_end) = voiced[start + 1..].find(')') {
            let end = start + 1 + rel_end;
            let mut out = String::with_capacity(voiced.len() + bgm.len());
            out.push_str(&voiced[..start]);
            out.push_str(&bgm);
            out.push_str(&voiced[end + 1..]);
            return out;
        }
    }
    format!("{voiced} {bgm}")
}

/// Remove stage directions that redefine speaker timbre (they fight VOICE LOCK).
/// Keeps emotion intensity cues like 轻声/激动 and the actual quoted line.
fn strip_conflicting_voice_color_cues(raw: &str) -> String {
    let mut s = raw.to_string();
    const CUES: &[&str] = &[
        "用低沉的声音",
        "用沙哑的声音",
        "用尖细的声音",
        "用清脆的声音",
        "用磁性的声音",
        "用甜美的声音",
        "低沉地说",
        "沙哑地说",
        "尖声说",
        "尖细地说",
        "清脆地说",
        "低沉道",
        "沙哑道",
        "低沉嗓音",
        "沙哑嗓音",
        "尖细女声",
        "浑厚男声",
        "清亮女声",
        "in a deep voice",
        "in a raspy voice",
        "in a high voice",
        "in a breathy voice",
        "with a deep voice",
        "with a raspy voice",
        "with a high-pitched voice",
    ];
    for cue in CUES {
        while let Some(pos) = find_case_insensitive(&s, cue) {
            s.replace_range(pos..pos + cue.len(), "");
        }
    }
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim().to_string()
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    // Fast path for CJK / exact.
    if let Some(p) = haystack.find(needle) {
        return Some(p);
    }
    let hay_lower: String = haystack.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    hay_lower.find(&needle_lower)
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
        // Fall back to prioritized speaker idxs (named speakers / visible cast).
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
        "[FIXED SPEAKER VOICE for this clip — reuse verbatim: {}] {raw}",
        locks.join("; ")
    )
}

fn is_seedance_privacy_image_err(err: &VimaxError) -> bool {
    is_seedance_privacy_image_err_text(&err.to_string())
}

/// True when the video backend rejected the **prompt text** for content safety
/// (e.g. `InputTextSensitiveContentDetected`, `DataInspectionFailed`), as opposed
/// to rejecting an input frame. Retrying the same wording cannot pass; the client
/// retries once with a strict-softened prompt, then fails fast instead of burning
/// the multi-ref → drop-continuity → T2V cascade on the same risky text.
///
/// Important: `InputImageSensitiveContentDetected` also contains the substring
/// `sensitivecontent` — those must be classified as image privacy, not text.
fn is_video_sensitive_text_err(err: &VimaxError) -> bool {
    if is_seedance_privacy_image_err(err) {
        return false;
    }
    let s = err.to_string().to_ascii_lowercase();
    if s.contains("inputimagesensitive") {
        return false;
    }
    s.contains("inputtextsensitive")
        || s.contains("sensitivecontent")
        || s.contains("sensitive content")
        || s.contains("inappropriate content")
        || s.contains("datainspectionfailed")
        || s.contains("内容安全")
        || s.contains("敏感内容")
        || s.contains("不当内容")
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

    /// Window of the models integrated today (Seedance 2.0, MiniMax-H3 ⊂ 4–15s).
    const SEEDANCE: ClipBounds = ClipBounds::new(5, 15);

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
            beats: Vec::new(),
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
    fn image_privacy_err_is_not_classified_as_text_sensitive() {
        let img = VimaxError::Video(
            "API error 48: InputImageSensitiveContentDetected.PrivacyInformation \
(content[2] may contain real person)"
                .into(),
        );
        assert!(is_seedance_privacy_image_err(&img));
        assert!(!is_video_sensitive_text_err(&img));
        assert!(should_retry_seedance_without_photoreal_frame(&img));

        let text = VimaxError::Video(
            "InputTextSensitiveContentDetected: prompt violates content policy".into(),
        );
        assert!(!is_seedance_privacy_image_err(&text));
        assert!(is_video_sensitive_text_err(&text));
    }

    #[test]
    fn flagged_content_index_maps_onto_ref_strip() {
        let msg = "seedance upstream status 400: InputImageSensitiveContentDetected.\
PrivacyInformation (input image 'content[2]' may contain real person)";
        let idx = parse_seedance_flagged_content_index(msg).expect("idx");
        // content[0]=text → content[2] is the second reference image (slot 1)
        assert_eq!(content_index_to_image_slot(idx, 3), Some(1));
        let refs = vec![
            (PathBuf::from("a.png"), String::new()),
            (PathBuf::from("character_portraits/0/front.png"), String::new()),
            (PathBuf::from("env.png"), String::new()),
        ];
        assert_eq!(privacy_repair_targets(msg, &refs), vec![1]);
        // No content[N] → sweep face-bearing subject plates.
        assert_eq!(
            privacy_repair_targets("opaque seedance upstream status 400", &refs),
            vec![1]
        );
    }

    #[test]
    fn multi_ref_prompt_is_motion_first_with_at_image_bindings() {
        let mut s = shot(1, 0);
        s.motion_desc = "左方女性从防御起身，走向右侧巨兽".into();
        s.ff_desc = "中景，两人并肩站在深坑前".into();
        s.lf_desc = "中景，两人并肩站在深坑前".into();
        let refs = vec![
            (
                PathBuf::from("shots/0/video_last_frame.png"),
                "Previous ending".into(),
            ),
            (
                PathBuf::from("characters/alice_three_view.png"),
                "File [alice_three_view.png] = GLOBAL three-view character bible for <林铮>".into(),
            ),
        ];
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            SEEDANCE,
            5,
            SpliceSeam::SameTake,
            "",
            false,
            None,
            "9:16",
        );
        let motion_at = prompt.find("Motion:").expect("motion first");
        let look_at = prompt.find("Look:").expect("short look");
        assert!(motion_at < look_at, "motion must precede style lock: {prompt}");
        assert!(prompt.contains("@Image1"));
        assert!(prompt.contains("@Image2"));
        assert!(prompt.contains("<林铮>"));
        assert!(!prompt.contains("PRODUCTION LOOK LOCK"));
        assert!(!prompt.contains("CAST LOCK (must match three-view"));
        assert!(!prompt.contains("REFERENCE BINDINGS (each Image N"));
        assert!(!prompt.contains("CONTINUITY: Image 1"));
        assert!(!prompt.contains("PLOT LOCK:"));
        assert!(!prompt.contains("chipmunk"));
        assert!(!prompt.contains("Chinese ~"));
        assert!(!prompt.contains("Keep it subtitle-free. Do not generate on-screen captions"));
        assert!(prompt.contains("Keep it subtitle-free."));
        assert!(prompt.contains("Throughout:"));
        assert!(
            prompt.contains("underscore") || prompt.contains('('),
            "silent shots still get scene BGM: {prompt}"
        );
        assert!(
            !prompt.contains("Keep motion purposeful for the full clip"),
            "must not ask Seedance to pad motion to fill the clock"
        );
        assert!(prompt.contains("Frame: 9:16 vertical"), "{prompt}");
        assert!(
            prompt.contains("large creatures"),
            "portrait framing must keep giant figures inside the canvas: {prompt}"
        );
        assert!(!prompt.contains("16:9"), "{prompt}");
    }

    /// The continuity still means opposite things on the two seams, and the
    /// prompt must say which one — this is the anti-stutter instruction.
    #[test]
    fn continuity_still_is_a_start_frame_only_when_the_camera_holds() {
        let s = shot(1, 0);
        let refs = vec![(
            PathBuf::from("shots/0/video_last_frame.png"),
            "Previous ending".into(),
        )];

        let same = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            SEEDANCE,
            5,
            SpliceSeam::SameTake,
            "",
            false,
            None,
            "",
        );
        assert!(same.contains("SAME take keeps rolling"), "{same}");
        assert!(same.contains("one continuous take"), "{same}");
        assert!(!same.contains("CUT to a NEW angle"), "{same}");

        let cut = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            SEEDANCE,
            5,
            SpliceSeam::MatchCut,
            "",
            false,
            None,
            "",
        );
        assert!(cut.contains("continuity reference ONLY"), "{cut}");
        assert!(cut.contains("CUT to a NEW angle"), "{cut}");
        assert!(cut.contains("action ALREADY in progress"), "{cut}");
        assert!(
            !cut.contains("SAME take keeps rolling"),
            "a new angle must not be told to resume the previous framing: {cut}"
        );
    }

    /// Without a continuity still there is no seam to describe; the prompt must
    /// not reference an `@Image1` role that was never bound.
    #[test]
    fn a_hard_cut_prompt_has_no_opening_seam_clause() {
        let s = shot(1, 0);
        let refs = vec![(
            PathBuf::from("characters/alice_three_view.png"),
            "File [alice_three_view.png] = GLOBAL three-view character bible for <林铮>".into(),
        )];
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            SEEDANCE,
            5,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "",
        );
        assert!(!prompt.contains("Opening:"), "{prompt}");
        assert!(!prompt.contains("previous shot's final frame"), "{prompt}");
    }

    /// A clip that absorbed a same-camera neighbour must be told when each beat
    /// plays, or the model holds on the first one for the whole clip.
    #[test]
    fn a_merged_clip_prompt_lays_its_beats_on_a_timeline() {
        let line: String = "中".chars().cycle().take(17).collect();
        let mut a = shot(0, 0);
        a.motion_desc = "她转身".into();
        a.audio_desc = Some(line);
        let mut b = shot(1, 0);
        b.motion_desc = "她走近窗边".into();
        let merged = super::super::clip_beats::pack_scene_clips(SEEDANCE, vec![a, b]);
        assert_eq!(merged.len(), 1, "same camera must merge");

        let prompt = i2v_motion_prompt(
            &merged[0],
            &[],
            "cinematic",
            &[],
            SEEDANCE,
            13,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "",
        );
        assert!(prompt.starts_with("Motion: ONE continuous take"), "{prompt}");
        assert!(prompt.contains("[0-"), "beats need explicit windows: {prompt}");
        assert!(prompt.contains("她转身"), "{prompt}");
        assert!(prompt.contains("她走近窗边"), "{prompt}");
        assert!(
            !prompt.contains("Motion: 她转身, then 她走近窗边"),
            "the joined prose line must give way to the timeline: {prompt}"
        );
    }

    #[test]
    fn a_packed_reverse_prompt_asks_for_a_cut_inside_the_clip() {
        let mut first = shot(0, 0);
        first.motion_desc = "她说话".into();
        let mut second = shot(1, 1);
        second.motion_desc = "他回答".into();
        let packed = super::super::clip_beats::pack_scene_clips(SEEDANCE, vec![first, second]);
        assert_eq!(packed.len(), 1);
        assert!(packed[0].has_camera_cuts());

        let prompt = i2v_motion_prompt(
            &packed[0],
            &[],
            "cinematic",
            &[],
            SEEDANCE,
            10,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "",
        );
        assert!(prompt.starts_with("Motion: native multi-shot"), "{prompt}");
        assert!(prompt.contains("CUT TO a new camera"), "{prompt}");
        assert!(
            !prompt.contains("camera, framing and lighting never change"),
            "a reverse must not be told it is one locked take: {prompt}"
        );
    }

    /// Regression: the storyboard LLM had written `0-4s / 4-7s` into `motion_desc`
    /// (7s of beats) while the render requested a 5s clip, so the prompt argued
    /// with its own `duration`. The renderer owns time: authored seconds never
    /// reach the model, only their ratio does.
    #[test]
    fn authored_timecodes_never_contradict_the_requested_duration() {
        let mut s = shot(0, 0);
        s.motion_desc = "0-4s:摄影机固定,男生骑车从画面左侧向右横贯画面;\
4-7s:他继续向右骑行,地面落叶被风吹卷而起。"
            .into();

        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            SEEDANCE,
            5,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "",
        );
        assert!(!prompt.contains("0-4s"), "{prompt}");
        assert!(!prompt.contains("4-7s"), "{prompt}");
        // 4:3 authored pacing, relaid on the 5s clip the render asks for.
        assert!(prompt.contains("[0-3s]"), "{prompt}");
        assert!(prompt.contains("[3-5s]"), "{prompt}");
        assert!(prompt.contains("About 5s."), "{prompt}");
        assert!(
            prompt.contains("2 visual events back to back"),
            "the pacing line must count the beats it has to fit: {prompt}"
        );
    }

    /// A single-beat clip keeps the plain motion line — no beat timeline noise.
    #[test]
    fn a_single_beat_clip_prompt_has_no_beat_timeline() {
        let mut s = shot(1, 0);
        s.motion_desc = "她转身".into();
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            SEEDANCE,
            8,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "",
        );
        assert!(prompt.starts_with("Motion: 她转身"), "{prompt}");
        assert!(!prompt.contains("ONE continuous take"), "{prompt}");
    }

    /// A tail frame bound at Image 2+ describes state, never a start frame.
    #[test]
    fn a_tail_frame_outside_image_one_is_state_only() {
        let refs = vec![
            (
                PathBuf::from("characters/alice_three_view.png"),
                "bible for <林铮>".into(),
            ),
            (
                PathBuf::from("shots/0/video_last_frame.png"),
                "Previous ending".into(),
            ),
        ];
        let bindings = video_at_image_bindings(&refs, SpliceSeam::Cut);
        assert!(bindings.contains("@Image2 previous shot's final frame"), "{bindings}");
        assert!(bindings.contains("continuity reference ONLY"), "{bindings}");
        assert!(!bindings.contains("keeps rolling"), "{bindings}");
    }

    #[test]
    fn audio_caption_mines_motion_dialogue_and_always_has_bgm() {
        let scene_bgm = "(gentle piano motif, steady tempo, same across shots)";
        let from_motion = seedance_audio_caption_block(
            None,
            "他看着对方说道：「我们走吧」",
            "wide shot of two people",
            &[],
            &[],
            scene_bgm,
        );
        assert!(from_motion.contains('{'));
        assert!(from_motion.contains("我们走吧"));
        assert!(from_motion.contains("gentle piano motif"));

        let ambient = seedance_audio_caption_block(
            None,
            "slow pan across room",
            "establishing",
            &[],
            &[],
            scene_bgm,
        );
        assert!(ambient.contains('<') || ambient.contains("ambience") || ambient.contains("环境"));
        assert!(ambient.contains("gentle piano motif"));

        let typed = seedance_audio_caption_block(
            Some("{快跑} <脚步声>"),
            "runs",
            "chase",
            &[],
            &[],
            scene_bgm,
        );
        assert!(typed.contains("{快跑}"));
        assert!(
            typed.contains("gentle piano motif"),
            "typed captions without music get the scene BGM"
        );

        let replaced = seedance_audio_caption_block(
            Some("{快跑} <脚步声> (loud EDM drop)"),
            "runs",
            "chase",
            &[],
            &[],
            scene_bgm,
        );
        assert!(
            replaced.contains("gentle piano motif") && !replaced.contains("EDM"),
            "per-shot music must be replaced by scene-stable BGM: {replaced}"
        );
    }

    #[test]
    fn audio_caption_injects_character_voice_lock() {
        use crate::domain::VoiceProfile;
        let mut vp = VoiceProfile {
            timbre: "清亮女中音".into(),
            volume: Some("normal".into()),
            pitch: Some("mid-high".into()),
            speaking_style: "语速平稳".into(),
            caption_clause: None,
        };
        vp.normalize("李薇");
        let chars = vec![CharacterInScene {
            idx: 0,
            identifier_in_scene: "李薇".into(),
            is_visible: true,
            static_features: "年轻女性".into(),
            dynamic_features: None,
            voice_profile: Some(vp),
        }];
        let caption = seedance_audio_caption_block(
            Some("李薇说：「今晚别等我」"),
            "nod",
            "close-up",
            &chars,
            &[0],
            "",
        );
        assert!(caption.contains("李薇"));
        assert!(caption.contains("FIXED SPEAKER VOICE"));
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
                audio_desc: Some("李薇用低沉的声音说：「今晚别等我」".into()),
                beats: Vec::new(),
            },
            &chars,
            "cinematic",
            &[],
            SEEDANCE,
            10,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "",
        );
        assert!(prompt.contains("VOICE LOCK"));
        assert!(prompt.contains("FIXED SPEAKER VOICE"));
        assert!(prompt.contains("清亮女中音"));
        assert!(
            !prompt.contains("用低沉的声音"),
            "timbre-redefining stage directions must be stripped so VOICE LOCK wins"
        );
        assert!(prompt.contains("今晚别等我"));
        assert!(prompt.contains("Speak at a natural conversational pace"));
    }

    #[test]
    fn strip_voice_color_cues_keeps_dialogue() {
        let cleaned = strip_conflicting_voice_color_cues(
            "李薇用低沉的声音说：「今晚别等我」",
        );
        assert!(!cleaned.contains("用低沉的声音"));
        assert!(cleaned.contains("今晚别等我"));
        assert!(cleaned.contains("李薇"));
    }

    #[test]
    fn audio_ref_mode_skips_voice_lock_and_bgm() {
        let s = shot(1, 0);
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            SEEDANCE,
            8,
            SpliceSeam::Cut,
            "(piano motif)",
            true,
            Some("阿琳"),
            "",
        );
        assert!(prompt.contains("@Audio1"));
        assert!(prompt.contains("阿琳"));
        assert!(!prompt.contains("VOICE LOCK"));
        assert!(!prompt.contains("MUSIC CONTINUITY"));
        assert!(!prompt.contains("piano motif"));
        assert!(prompt.contains("no music") || prompt.contains("No background music"));

        let essential = seedance_audio_caption_essential_only(
            Some("李薇说：「走吧」。BGM: 同一低音合成器持续音与脉冲鼓点延续"),
            "walks",
            "street",
        );
        assert!(essential.contains("走吧"));
        assert!(!essential.contains("piano"));
        assert!(
            !essential.contains("低音合成器"),
            "BGM prose must be stripped when reference_audio is bound: {essential}"
        );
    }

    #[test]
    fn adult_age_does_not_inject_child_style_lock() {
        let s = ShotDescription {
            idx: 0,
            is_last: true,
            cam_idx: 0,
            visual_desc: "中景".into(),
            variation_type: "small".into(),
            variation_reason: String::new(),
            ff_desc: "林铮站在巨兽身侧".into(),
            ff_vis_char_idxs: vec![0],
            lf_desc: "林铮站在巨兽身侧".into(),
            lf_vis_char_idxs: vec![0],
            motion_desc: "林铮收脚站定".into(),
            audio_desc: Some("脚步踩在沙砾上".into()),
            beats: Vec::new(),
        };
        let chars = vec![CharacterInScene {
            idx: 0,
            identifier_in_scene: "林铮".into(),
            is_visible: true,
            static_features: "28 岁中国女性，身高约 172cm".into(),
            dynamic_features: None,
            voice_profile: None,
        }];
        let refs = vec![(
            PathBuf::from("characters/lin_three_view.png"),
            "File [lin_three_view.png] = GLOBAL three-view character bible for <林铮>".into(),
        )];
        let prompt = i2v_motion_prompt(
            &s,
            &chars,
            "cinematic",
            &refs,
            SEEDANCE,
            8,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "16:9",
        );
        assert!(!prompt.contains("Children share"));
        assert!(!prompt.contains("CAST LOCK"));
        assert!(prompt.contains("@Image1"));
        assert!(prompt.starts_with("Motion:"));
        assert!(prompt.contains("Frame: 16:9 landscape"), "{prompt}");
        assert!(!prompt.contains("vertical"), "{prompt}");
        assert!(!prompt.contains("large creatures"), "{prompt}");
    }

    #[test]
    fn empty_aspect_omits_frame_line() {
        let s = shot(1, 0);
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            SEEDANCE,
            5,
            SpliceSeam::Cut,
            "",
            false,
            None,
            "",
        );
        assert!(!prompt.contains("Frame:"), "{prompt}");
    }

    #[test]
    fn prop_that_is_also_cast_is_duplicate() {
        let chars = [CharacterInScene {
            idx: 0,
            identifier_in_scene: "硅基晶体巨兽".into(),
            is_visible: true,
            static_features: "八米高晶体".into(),
            dynamic_features: None,
            voice_profile: None,
        }];
        assert!(prop_duplicates_visible_cast(
            Path::new("props/巨兽躯干_prop.png"),
            "GLOBAL prop bible: <硅基晶体巨兽的躯干>",
            &chars,
            &[0],
        ));
        assert!(!prop_duplicates_visible_cast(
            Path::new("props/能量刃_prop.png"),
            "GLOBAL prop bible: <能量刃>",
            &chars,
            &[0],
        ));
    }

    #[test]
    fn ensure_brief_audio_fills_empty() {
        let mut shots = vec![ShotBriefDescription {
            idx: 0,
            is_last: true,
            cam_idx: 0,
            visual_desc: "establishing wide shot".into(),
            audio_desc: None,
            beats: Vec::new(),
        }];
        assert!(ensure_brief_audio_descs(&mut shots));
        assert!(shots[0].audio_desc.as_ref().is_some_and(|s| !s.trim().is_empty()));
        assert!(!ensure_brief_audio_descs(&mut shots));
    }
}

#[cfg(test)]
mod storyboard_publish_tests {
    use super::*;
    use crate::domain::ShotBriefDescription;

    const SEEDANCE: ClipBounds = ClipBounds::new(5, 15);

    fn brief(idx: i32, cam_idx: i32, visual: &str) -> ShotBriefDescription {
        ShotBriefDescription {
            idx,
            is_last: false,
            cam_idx,
            visual_desc: visual.into(),
            audio_desc: None,
            beats: Vec::new(),
        }
    }

    fn micro_draft() -> Vec<ShotBriefDescription> {
        vec![
            brief(0, 0, "她转身开门"),
            brief(1, 0, "她看见他"),
            brief(2, 1, "他抬头"),
            brief(3, 2, "窗外雨"),
        ]
    }

    #[tokio::test]
    async fn first_storyboard_write_is_the_packed_clip_list() {
        let dir = tempfile::tempdir().unwrap();
        let wd = dir.path();
        let path = wd.join("storyboard.json");
        let draft = micro_draft();
        let packed = super::super::clip_beats::pack_scene_briefs(SEEDANCE, draft);
        assert!(packed.len() < 4, "draft must collapse so this tests publish");
        assert!(!path.exists());
        let wrote = persist_published_storyboard(wd, &path, &packed, "plan-fp")
            .await
            .unwrap();
        assert!(wrote);
        let disk: Vec<ShotBriefDescription> = read_json_artifact(&path).await.unwrap();
        assert_eq!(disk.len(), packed.len());
        assert_eq!(disk[0].idx, 0);
        assert_eq!(disk.last().map(|row| row.idx), Some((packed.len() - 1) as i32));
        assert!(sidecar_matches(&path, "plan-fp").await);
    }

    #[tokio::test]
    async fn persist_replaces_an_unpacked_draft_left_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let wd = dir.path();
        let path = wd.join("storyboard.json");
        let draft = micro_draft();
        write_json_artifact(&path, &draft).await.unwrap();
        write_sidecar(&path, "plan-fp").await.unwrap();
        let packed = super::super::clip_beats::pack_scene_briefs(SEEDANCE, draft.clone());
        assert!(packed.len() < draft.len());
        let wrote = persist_published_storyboard(wd, &path, &packed, "plan-fp")
            .await
            .unwrap();
        assert!(wrote);
        let disk: Vec<ShotBriefDescription> = read_json_artifact(&path).await.unwrap();
        assert_eq!(disk.len(), packed.len());
        assert!(disk.iter().enumerate().all(|(i, row)| row.idx == i as i32));
        assert!(sidecar_matches(&path, "plan-fp").await);
        let again = persist_published_storyboard(wd, &path, &packed, "plan-fp")
            .await
            .unwrap();
        assert!(!again);
    }
}
