//! Script2Video pipeline — plan text artifacts then render clips/final.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::clip_bounds::ClipBounds;
use crate::agents::{
    CharacterExtractor, CharacterPortraitsGenerator, StoryboardArtist, VoiceProfileGenerator,
    bind_location_ids, ensure_film_cover, environment_sluglines_from_dir, has_usable_portrait,
    resolve_environment_plate, world_asset_pairs,
};
use crate::domain::{Camera, CharacterInScene, ShotBriefDescription, ShotDescription};
use crate::drama::{lint_script_promise_coverage, load_drama_engine, pack_split_needles};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local;
use crate::media_local::SpliceSeam;
use crate::progress::{PendingShotReview, ProgressCallback};
use crate::session::{
    copy_json_artifact_if_readable, read_json_artifact, write_json_artifact, write_text_artifact,
};
use crate::shot_packet::{
    self, apply_packet_to_shot, archive_live_take, bind_prompt_with_override, continuity_unbound,
    load_packet, merge_live_audio_refs, merge_live_image_refs, refresh_run_state_from_disk,
    resolve_packet_duration, resolve_slot_paths, save_packet, slots_from_image_pairs,
    usable_audio_slots, usable_image_slots, RenderMode, ShotPacket,
    ShotReviewDecision, ShotRunState,
};
use crate::skills::DirectorSpec;

use super::cameo_bind::{
    apply_session_cameos, cameo_extractor_hint, classify_session_references, resolve_session_root,
    world_cameo_context,
};
use super::privacy_face::{
    ensure_ai_sanitized_privacy_face, ensure_seedance_privacy_face,
    is_seedance_privacy_image_err_text, next_privacy_tier_for_path,
    parse_seedance_flagged_content_index, privacy_repair_targets,
    PrivacyFaceOutcome, PrivacyFaceTier,
};
use super::{
    ai_face_sanitizer,
    artifact_cache::{
        load_json_if_cached, load_or_write_json_cached, plan_artifacts_sidecar_complete,
        script2video_plan_fingerprint, sidecar_matches, write_sidecar,
    },
    PipelineBackends, emit, emit_meta, emit_pct, emit_pct_meta, group_shots_into_cameras,
    resolve_film_root, safe_component, sanitize_camera_tree,
};

/// One image rewrite per shot after a privacy reject. Concat trim handles freeze-frame;
/// we do not drop the last-frame ref or fall back to text-to-video.
const MAX_PRIVACY_FACE_REPAIRS_PER_SHOT: usize = 1;

pub struct Script2VideoPipeline {
    backends: PipelineBackends,
    working_dir: PathBuf,
    character_extractor: CharacterExtractor,
    storyboard: StoryboardArtist,
}

impl Script2VideoPipeline {
    pub fn new(backends: PipelineBackends, working_dir: PathBuf) -> Self {
        let character_extractor = CharacterExtractor::new(Arc::clone(&backends.chat));
        let storyboard = StoryboardArtist::new(Arc::clone(&backends.chat), backends.clip)
            .with_voice_ref_slots(backends.max_reference_audio);
        Self {
            backends,
            working_dir,
            character_extractor,
            storyboard,
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

            emit_pct(
                &progress,
                "plan_assets_parallel",
                "正在并行生成定妆图、世界参考与分镜表",
                22.0,
            );
        }

        let storyboard = if scene_only {
            emit_pct(&progress, "design_storyboard", "正在设计分镜表", 40.0);
            self.design_storyboard(script, &characters, user_requirement, &plan_fp)
                .await?
        } else {
            let portraits_voices = async {
                emit_pct(
                    &progress,
                    "character_portraits_start",
                    "正在生成全局角色定妆图",
                    22.0,
                );
                if let Err(err) = self
                    .generate_character_portraits(&characters, &style, script, &progress)
                    .await
                {
                    emit_pct(
                        &progress,
                        "character_portraits_start",
                        "角色定妆图生成失败",
                        22.0,
                    );
                    return Err(err);
                }
                Ok(())
            };
            let world = async {
                emit_pct(
                    &progress,
                    "world_assets_start",
                    "正在生成全局环境与道具参考图",
                    30.0,
                );
                let world_planner = self.backends.world_planner(&film_root).await;
                let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
                match world_planner
                    .ensure(
                        &film_root,
                        script,
                        &style,
                        &style_refs,
                        &scene_hint,
                        &lock_token,
                    )
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(err) => {
                        // try_join siblings (voice_references_done) can clobber this stage.
                        emit_pct(
                            &progress,
                            "world_assets_start",
                            "世界参考图生成失败",
                            30.0,
                        );
                        Err(err)
                    }
                }
            };
            let board = async {
                emit_pct(&progress, "design_storyboard", "正在设计分镜表", 40.0);
                match self
                    .design_storyboard(script, &characters, user_requirement, &plan_fp)
                    .await
                {
                    Ok(rows) => Ok(rows),
                    Err(err) => {
                        // Portraits/voice emit in parallel and can clobber this stage
                        // name; restore it so a coverage failure is not reported as
                        // voice_references_start.
                        emit_pct(&progress, "design_storyboard", "分镜覆盖校验未通过", 40.0);
                        Err(err)
                    }
                }
            };
            let ((), (), storyboard) = tokio::try_join!(portraits_voices, world, board)?;
            storyboard
        };

        emit_pct(&progress, "shot_descriptions", "正在落盘镜头描述", 62.0);
        let shot_descriptions = self
            .persist_shot_descriptions(&storyboard, &characters, script, &plan_fp)
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
        let camera_tree: Vec<Camera> = read_json_artifact(&wd.join("camera_tree.json")).await?;
        let clips = clips_from_board(&storyboard, &characters);
        let (synced, shot_descriptions, idx_map) =
            commit_packed_shot_layout(wd, storyboard.clone(), clips).await;
        if super::clip_beats::storyboard_differs(&storyboard, &synced) {
            tracing::info!(
                before = storyboard.len(),
                after = synced.len(),
                "densified published storyboard idxs; clip count follows the board"
            );
            write_json_artifact(&wd.join("storyboard.json"), &synced).await?;
        }
        write_json_artifact(&wd.join("shot_descriptions.json"), &shot_descriptions).await?;
        let session_root = resolve_session_root(wd);
        let _ = crate::shot_packet::seed_packets_from_shots(
            wd,
            &session_root,
            &shot_descriptions,
            self.backends.clip,
        )
        .await;
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

    /// Like [`Self::render`]. `prior_continuity` is accepted and ignored: the
    /// previous scene's tail frame is not submitted as a reference image.
    pub async fn render_with_prior_continuity(
        &self,
        script: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
        prior_continuity: Option<&Path>,
    ) -> VimaxResult<PathBuf> {
        emit(&progress, "render_start", "开始渲染脚本成片");
        if prior_continuity.is_some() {
            tracing::info!(
                "ignoring prior scene tail frame; shots open from their own blocking text"
            );
        }
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
        let registry = self
            .generate_character_portraits(&plan.characters, &style, script, &progress)
            .await?;

        let film_root = resolve_film_root(&self.working_dir);

        emit(
            &progress,
            "world_assets_start",
            "正在确认全局环境与道具参考图",
        );
        let world_pairs = {
            let world_planner = self.backends.world_planner(&film_root).await;
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
            // Intra-scene joins share the underscore and do not trim a replayed
            // head. The scene itself opens as a hard cut.
            let refs: Vec<&Path> = clips.iter().map(|p| p.as_path()).collect();
            let entries: Vec<i32> = ordered_shots.iter().map(|s| s.cam_idx).collect();
            let exits: Vec<i32> = ordered_shots.iter().map(|s| s.exit_cam_idx()).collect();
            let opening = media_local::SpliceSeam::Cut;
            // A shot with a spoken line keeps its full length: the line finishes on
            // its own. A silent shot may lose the held frame it generated at its end.
            let seq: Vec<media_local::ConcatClip<'_>> =
                media_local::ConcatClip::scene_exits(&refs, &entries, &exits, opening)
                    .into_iter()
                    .zip(&ordered_shots)
                    .map(|(clip, shot)| {
                        if shot.has_spoken_dialogue() {
                            clip
                        } else {
                            clip.with_static_tail_trim()
                        }
                    })
                    .collect();
            media_local::concat_videos(&seq, &final_path).await?;
            emit(&progress, "concat_done", "场景成片拼接完成");
            let film_root = resolve_film_root(&self.working_dir);
            if film_root == self.working_dir {
                let session = resolve_session_root(&self.working_dir);
                let takes = crate::shot_packet::collect_shot_take_manifest(&film_root).await;
                let _ = crate::shot_packet::archive_current_film(&session, &film_root, takes).await;
            }
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
        let mut storyboard: Vec<ShotBriefDescription> =
            match load_json_if_cached::<Vec<ShotBriefDescription>>(&path, plan_fp).await {
            Some(cached) if lint_script_promise_coverage(script, &cached).is_empty() => cached,
            _ => {
                self.storyboard
                    .design_storyboard(script, characters, user_requirement)
                    .await?
            }
        };
        if ensure_brief_audio_descs(&mut storyboard) {
            tracing::info!("filled missing storyboard audio_desc with ambient defaults");
        }
        let film_root = resolve_film_root(&self.working_dir);
        let slugs = environment_sluglines_from_dir(&film_root).await;
        bind_location_ids(&mut storyboard, script, &slugs);
        let spec = DirectorSpec::load_from_dir(&self.working_dir);
        let engine = load_drama_engine(&self.working_dir);
        let split_needles = engine
            .as_ref()
            .map(pack_split_needles)
            .unwrap_or_default();
        let draft_len = storyboard.len();
        let max_shots = load_target_duration_secs(&self.working_dir)
            .await
            .map(|budget| crate::planning::max_shots_for_budget(self.backends.clip, budget));
        let packed = super::clip_beats::pack_briefs_for_publish(
            self.backends.clip,
            storyboard,
            super::clip_beats::PackOpts {
                policy: spec.pack_policy,
                split_needles,
                max_voice_ref_speakers: self.backends.max_reference_audio,
            },
            max_shots,
            spec.over_budget,
        );
        if packed.len() != draft_len {
            tracing::info!(
                before = draft_len,
                after = packed.len(),
                "normalized storyboard rows to one performance per clip"
            );
        }
        persist_published_storyboard(&self.working_dir, &path, &packed, plan_fp).await?;
        let bgm = ensure_scene_bgm_brief(&self.working_dir, &packed).await?;
        tracing::info!(bgm = %bgm, "scene BGM brief locked for shot continuity");
        Ok(packed)
    }

    async fn persist_shot_descriptions(
        &self,
        briefs: &[ShotBriefDescription],
        characters: &[CharacterInScene],
        script: &str,
        plan_fp: &str,
    ) -> VimaxResult<Vec<ShotDescription>> {
        let aggregate = self.working_dir.join("shot_descriptions.json");
        let mut briefs = briefs.to_vec();
        let film_root = resolve_film_root(&self.working_dir);
        let slugs = environment_sluglines_from_dir(&film_root).await;
        bind_location_ids(&mut briefs, script, &slugs);
        let clips = clips_from_board(&briefs, characters);
        let (synced, packed, _) =
            commit_packed_shot_layout(&self.working_dir, briefs, clips).await;
        write_json_artifact(&self.working_dir.join("storyboard.json"), &synced).await?;
        write_json_artifact(&aggregate, &packed).await?;
        super::artifact_cache::write_sidecar(&aggregate, plan_fp).await?;
        let session_root = resolve_session_root(&self.working_dir);
        let _ = crate::shot_packet::seed_packets_from_shots(
            &self.working_dir,
            &session_root,
            &packed,
            self.backends.clip,
        )
        .await;
        Ok(packed)
    }

    async fn construct_camera_tree(
        &self,
        shot_descriptions: &[ShotDescription],
        plan_fp: &str,
    ) -> VimaxResult<Vec<Camera>> {
        let path = self.working_dir.join("camera_tree.json");
        let mut cameras = load_or_write_json_cached(&path, plan_fp, || async {
            Ok(group_shots_into_cameras(shot_descriptions))
        })
        .await?;
        sanitize_camera_tree(&mut cameras);
        write_json_artifact(&path, &cameras).await?;
        super::artifact_cache::write_sidecar(&path, plan_fp).await?;
        Ok(cameras)
    }

    /// Load/create portraits only under the **film root**. Every scene/shot reuses the
    /// same registry paths so identity stays consistent across the final cut.
    ///
    /// `theme_source` is the script/story text used for THEME LOCK on wardrobe/era.
    pub(crate) async fn generate_character_portraits(
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
            if super::cameo_bind::is_body_part_character(character) {
                continue;
            }
            if !character.needs_identity_assets() {
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
            let attribution = nomi_providers::current_flowy_proxy_attribution();
            set.spawn(async move {
                nomi_providers::with_flowy_proxy_attribution(attribution, async move {
                    let _permit = permit
                        .acquire()
                        .await
                        .map_err(|_| VimaxError::msg("semaphore closed"))?;
                    portraits
                        .generate_all_views(&character, &style, &theme, &dir)
                        .await
                })
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

    /// Submit video-generation API calls one-by-one.
    /// On failure/cancel, stop immediately; already-saved clips remain for resume.
    ///
    /// Each shot is generated from its own blocking text plus cast, empty-set,
    /// and prop references. The previous clip's last frame is extracted for the
    /// filmstrip after the shot finishes, and is not submitted to the next shot.
    /// A missing previous video does not block this shot.
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
            "content-aware shot durations (audio+motion); tail frames are not references"
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
                shot_clip_start_meta(&self.working_dir, shot.idx),
            );

            // Spatial continuity is the shot's own blocking text plus the
            // empty-set plate. The previous clip's last frame is not a reference,
            // so a missing previous video does not block this shot.
            let seam = if i == 0 {
                media_local::SpliceSeam::Cut
            } else {
                media_local::SpliceSeam::within_scene(shots[i - 1].exit_cam_idx(), shot.cam_idx)
            };
            let continuity_first: Option<std::path::PathBuf> = None;

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
                    true,
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

            let packet = match refresh_shot_packet(
                &self.working_dir,
                shot,
                characters,
                registry,
                world_pairs,
                style,
                continuity_first.as_deref(),
                seam,
                duration_secs,
                self.backends.max_reference_audio,
                true,
            )
            .await
            {
                Ok(packet) => Some(packet),
                Err(e) => {
                    tracing::warn!(
                        shot = shot.idx,
                        error = %e,
                        "shot packet refresh failed; review gate still applies"
                    );
                    None
                }
            };
            let video_exists = media_local::is_usable_video_file(
                &self
                    .working_dir
                    .join("shots")
                    .join(shot.idx.to_string())
                    .join("video.mp4"),
            );
            if let Some(bridge) = &self.backends.review {
                if bridge.mode() == RenderMode::ShotReview && !video_exists {
                    let scene_root = packet.as_ref().map(|p| p.scene_root.clone()).unwrap_or_else(
                        || {
                            shot_packet::scene_root_rel(
                                &resolve_session_root(&self.working_dir),
                                &self.working_dir,
                            )
                        },
                    );
                    let pending_duration = packet
                        .as_ref()
                        .and_then(|p| p.duration_secs)
                        .filter(|secs| *secs > 0)
                        .unwrap_or(duration_secs);
                    let pending = PendingShotReview {
                        scene_root,
                        shot_idx: shot.idx,
                        scene_idx: shot_packet::scene_idx_from_dir(&self.working_dir),
                        duration_secs: pending_duration,
                        image_ref_count: packet
                            .as_ref()
                            .map(|p| usable_image_slots(p).len())
                            .unwrap_or(0),
                        audio_ref_count: packet
                            .as_ref()
                            .map(|p| usable_audio_slots(p).len())
                            .unwrap_or(0),
                    };
                    if let Ok(Some(mut p)) = load_packet(&self.working_dir, shot.idx).await {
                        p.run_state = ShotRunState::AwaitingReview;
                        let _ = save_packet(&self.working_dir, &p).await;
                    }
                    bridge.set_pending(Some(pending.clone()));
                    emit_meta(
                        progress,
                        "shot_awaiting_review",
                        &format!("镜头 {} / {} 待过审", i + 1, shots.len()),
                        serde_json::json!({
                            "shot_idx": shot.idx,
                            "scene_root": pending.scene_root,
                            "scene_idx": pending.scene_idx,
                            "duration_secs": pending.duration_secs,
                            "image_ref_count": pending.image_ref_count,
                            "audio_ref_count": pending.audio_ref_count,
                        }),
                    );
                    let cancel = self
                        .backends
                        .cancel
                        .clone()
                        .unwrap_or_else(CancellationToken::new);
                    match bridge.wait(&cancel).await? {
                        ShotReviewDecision::Approve => {}
                        ShotReviewDecision::SwitchToContinuous => {
                            // Submitters already set Continuous. Do not clobber a
                            // later header switch back to shot review.
                            if bridge.mode() == RenderMode::ShotReview {
                                bridge.set_mode(RenderMode::Continuous);
                            }
                        }
                        ShotReviewDecision::Stop => return Err(VimaxError::Cancelled),
                    }
                    bridge.set_pending(None);
                }
            }

            let shot_started = std::time::Instant::now();
            match self
                .generate_video_for_shot(
                    shot,
                    duration_secs,
                    continuity_first.as_deref(),
                    seam,
                    i.checked_sub(1).map(|p| &shots[p]),
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
                    // Keep the last working stage (video_clip_start / video_poll).
                    // A `_partial` emit would clobber it and get wrapped again at finish_job.
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

    /// Render one shot's clip.
    ///
    /// `seam` is how this shot joins the previous file for the prompt. The
    /// previous clip's last frame is not bound. A caller-supplied continuity
    /// still is kept only when a shot packet did not unbind that slot.
    async fn generate_video_for_shot(
        &self,
        shot: &ShotDescription,
        duration_secs: u32,
        continuity_first_frame: Option<&Path>,
        seam: SpliceSeam,
        prev: Option<&ShotDescription>,
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

        let session_root = resolve_session_root(&self.working_dir);
        let packet = load_packet(&self.working_dir, shot.idx).await.ok().flatten();
        let mut shot_owned = shot.clone();
        if let Some(p) = &packet {
            apply_packet_to_shot(&mut shot_owned, p);
        }
        super::clip_beats::sanitize_shot_prompt_payloads(&mut shot_owned, prev);
        let shot = &shot_owned;
        let duration_secs = packet
            .as_ref()
            .and_then(|p| p.duration_secs)
            .filter(|s| *s > 0)
            .unwrap_or(duration_secs);

        let continuity_source = if packet.as_ref().is_some_and(continuity_unbound) {
            None
        } else {
            continuity_first_frame
                .filter(|p| media_local::is_usable_image_file(p))
                .map(|p| p.to_path_buf())
        };
        let using_video_continuity = continuity_source.is_some();
        let seam = if using_video_continuity {
            seam
        } else {
            SpliceSeam::Cut
        };

        let continuity_ref = continuity_still_for_seam(seam, continuity_source.as_deref());
        let keep_env = keep_location_plate(seam, shot, prev);
        let mut ref_pairs = shot_video_ref_pairs(
            shot,
            continuity_ref,
            characters,
            registry,
            world_pairs,
            &resolve_film_root(&self.working_dir),
            keep_env,
        );
        if let Some(p) = &packet {
            if p.image_refs.iter().any(|s| s.user_override || s.unbound) {
                let from_packet = resolve_slot_paths(&session_root, &p.image_refs);
                if !from_packet.is_empty() {
                    ref_pairs = from_packet
                        .into_iter()
                        .filter(|(path, _)| media_local::is_usable_image_file(path))
                        .collect();
                }
            }
        }
        if keep_env
            && !ref_pairs
                .iter()
                .any(|(p, _)| is_environment_ref_path(p))
        {
            tracing::warn!(
                shot = shot.idx,
                location_id = %shot.location_id,
                world = world_pairs.len(),
                "shot has no environment plate in the Seedance ref strip"
            );
        }
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
        // Timbre is the written voice lock. Character reference audio is not generated or submitted.
        let ref_audio_paths: [&Path; 0] = [];
        let aspect_ratio = crate::aspect::load_aspect_from_dir(&self.working_dir).await;
        let compiled = i2v_motion_prompt(
            shot,
            characters,
            style,
            &ref_pairs,
            duration_secs,
            seam,
            scene_bgm,
            false,
            &[],
            &aspect_ratio,
        );
        let prompt = bind_prompt_with_override(
            &compiled,
            packet
                .as_ref()
                .and_then(|p| p.prompt_override.as_deref()),
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
                "Generating shot {} video ({}s; multi-ref ×{}; continuity={}; voice=text; refs=[{}])",
                shot.idx,
                duration_secs,
                ref_paths.len(),
                using_video_continuity,
                ref_names.join(", ")
            ),
            shot_clip_start_meta(&self.working_dir, shot.idx),
        );
        tracing::info!(
            shot = shot.idx,
            refs = ?ref_names,
            continuity = using_video_continuity,
            voice = "text",
            "video multi-ref R2V binding"
        );

        // Do not vision-preflight refs here (including previous-shot last frames).
        // Face inspection/repair runs only after the video model returns a
        // real-person privacy reject — see the retry loop below. Cameo plates
        // are sanitized at bind. Idea / script / novel all share this path.

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
                &ref_audio_paths,
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
                emit_meta(
                    progress,
                    "video_clip_start",
                    &format!(
                        "Shot {}: 内容安全拦截提示词（{}）。已改用软化措辞重试一次",
                        shot.idx,
                        truncate_err(&err, 100)
                    ),
                    shot_clip_start_meta(&self.working_dir, shot.idx),
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
                        &ref_audio_paths,
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
            let mut privacy_attempts: Vec<(PathBuf, PrivacyFaceTier)> = Vec::new();
            let mut privacy_resolved = false;

            for repair_i in 0..MAX_PRIVACY_FACE_REPAIRS_PER_SHOT {
                if !should_retry_seedance_without_photoreal_frame(&err)
                    && !is_seedance_privacy_image_err(&err)
                {
                    break;
                }
                let err_text = err.to_string();
                let targets = privacy_repair_targets(&err_text, &ref_pairs);
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
                    .filter_map(|&i| ref_pairs.get(i))
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
                        "Shot {}: Seedance 图片风控。正在重新检测并电影化虚构脸（{}：{}）…",
                        shot.idx,
                        if targeted { "定位" } else { "主体图扫描" },
                        labels.join(", ")
                    ),
                    shot_clip_start_meta(&self.working_dir, shot.idx),
                );

                let mut any_rewrite = false;
                let mut all_exhausted = true;
                for &slot in &targets {
                    let flagged_path = ref_pairs[slot].0.clone();
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
                                "AI sanitization failed; checking real-face gate before img2img fallback"
                            );
                            let has_real_face = ai_face_sanitizer::detect_human_face(
                                Arc::clone(&self.backends.chat),
                                &flagged_path,
                            )
                            .await
                            .unwrap_or(false);
                            if !has_real_face {
                                tracing::info!(
                                    shot = shot.idx,
                                    path = %flagged_path.display(),
                                    "privacy fallback skipped: no real photographic human face"
                                );
                                continue;
                            }
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
                    ref_pairs.iter().map(|(p, _)| p.as_path()).collect();
                let retry_prompt = i2v_motion_prompt(
                    shot,
                    characters,
                    style,
                    &ref_pairs,
                    duration_secs,
                    seam,
                    scene_bgm,
                    false,
                    &[],
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
                        &ref_audio_paths,
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
                return Err(err);
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
        if let Ok(Some(mut packet)) = load_packet(&self.working_dir, shot.idx).await {
            packet.run_state = ShotRunState::Ready;
            packet.error = None;
            if archive_live_take(
                &resolve_session_root(&self.working_dir),
                &self.working_dir,
                &mut packet,
            )
            .await
            .is_ok()
            {
                let _ = save_packet(&self.working_dir, &packet).await;
            }
        }
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

pub(crate) async fn refresh_shot_packet(
    scene_dir: &Path,
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    world_pairs: &[(PathBuf, String)],
    style: &str,
    continuity: Option<&Path>,
    seam: SpliceSeam,
    duration_secs: u32,
    max_reference_audio: usize,
    persist: bool,
) -> VimaxResult<ShotPacket> {
    let session_root = resolve_session_root(scene_dir);
    let scene_root = shot_packet::scene_root_rel(&session_root, scene_dir);
    let mut packet = match load_packet(scene_dir, shot.idx).await? {
        Some(existing) => existing,
        None => shot_packet::packet_from_shot(&scene_root, shot),
    };
    packet.scene_root = scene_root;
    packet.shot_idx = shot.idx;
    if !packet.user_edited {
        let seeded = shot_packet::packet_from_shot(&packet.scene_root, shot);
        packet.visual_desc = seeded.visual_desc;
        packet.audio_desc = seeded.audio_desc;
        packet.beats = seeded.beats;
        packet.location_id = seeded.location_id;
        packet.cam_idx = seeded.cam_idx;
    }
    let duration_secs = resolve_packet_duration(&packet, duration_secs);
    packet.duration_secs = Some(duration_secs);
    packet.seam = format!("{seam:?}").to_ascii_lowercase();

    let mut shot_owned = shot.clone();
    apply_packet_to_shot(&mut shot_owned, &packet);
    let prior = if shot.idx > 0 {
        load_shot_for_packet(scene_dir, shot.idx - 1).await.ok()
    } else {
        None
    };
    super::clip_beats::sanitize_shot_prompt_payloads(&mut shot_owned, prior.as_ref());
    let continuity = if continuity_unbound(&packet) {
        None
    } else {
        continuity
    };
    let using = continuity.filter(|p| media_local::is_usable_image_file(p));
    let seam = if using.is_some() { seam } else { SpliceSeam::Cut };
    let keep_env = keep_location_plate(seam, &shot_owned, None);
    let film_root = resolve_film_root(scene_dir);
    let image_pairs = shot_video_ref_pairs(
        &shot_owned,
        using,
        characters,
        registry,
        world_pairs,
        &film_root,
        keep_env,
    );
    merge_live_image_refs(
        &mut packet,
        slots_from_image_pairs(&session_root, &image_pairs),
    );
    let _ = max_reference_audio;
    merge_live_audio_refs(&mut packet, Vec::new());

    let bound_images = resolve_slot_paths(&session_root, &packet.image_refs)
        .into_iter()
        .filter(|(p, _)| media_local::is_usable_image_file(p))
        .collect::<Vec<_>>();
    let aspect_ratio = crate::aspect::load_aspect_from_dir(scene_dir).await;
    let scene_bgm = load_scene_bgm_paren(scene_dir).await;
    let compiled = i2v_motion_prompt(
        &shot_owned,
        characters,
        style,
        &bound_images,
        duration_secs,
        seam,
        &scene_bgm,
        false,
        &[],
        &aspect_ratio,
    );
    packet.compiled_prompt = crate::prompt_safety::sanitize_video_prompt(&compiled);
    refresh_run_state_from_disk(scene_dir, &mut packet);
    if persist {
        save_packet(scene_dir, &packet).await?;
    }
    Ok(packet)
}

/// `video_clip_start` meta: the shot index plus the scene it renders in. The
/// service freezes exactly this shot's packet while its request is in flight.
fn shot_clip_start_meta(working_dir: &Path, shot_idx: i32) -> serde_json::Value {
    let session_root = resolve_session_root(working_dir);
    serde_json::json!({
        "shot_idx": shot_idx,
        "scene_root": shot_packet::scene_root_rel(&session_root, working_dir),
    })
}

/// Disk refresh for GET packet. Does not attach the previous shot's last frame.
pub(crate) async fn refresh_shot_packet_from_disk(
    scene_dir: &Path,
    shot_idx: i32,
    max_reference_audio: usize,
    clip: ClipBounds,
) -> VimaxResult<ShotPacket> {
    let film_root = resolve_film_root(scene_dir);
    let shot = load_shot_for_packet(scene_dir, shot_idx).await?;
    let characters: Vec<CharacterInScene> =
        match read_json_artifact(&scene_dir.join("characters.json")).await {
            Ok(v) => v,
            Err(_) => read_json_artifact(&film_root.join("characters.json"))
                .await
                .unwrap_or_default(),
        };
    let registry: HashMap<String, HashMap<String, HashMap<String, String>>> =
        read_json_artifact(&film_root.join("character_portraits_registry.json"))
            .await
            .unwrap_or_default();
    let world_reg: crate::agents::WorldAssetRegistry =
        read_json_artifact(&film_root.join("world_assets_registry.json"))
            .await
            .unwrap_or_default();
    let world_pairs = world_asset_pairs(&world_reg, &film_root);
    let style = match tokio::fs::read_to_string(film_root.join("style.txt")).await {
        Ok(s) if !s.trim().is_empty() => s,
        _ => tokio::fs::read_to_string(scene_dir.join("style.txt"))
            .await
            .unwrap_or_default(),
    };
    // The previous clip's last frame stays on disk for the filmstrip. It is
    // not a reference image. A user-pinned continuity slot is preserved later
    // by merge_live_image_refs.
    let prev_idx = shot_idx.checked_sub(1);
    let seam = if prev_idx.is_some() {
        SpliceSeam::MatchCut
    } else {
        SpliceSeam::Cut
    };
    // A missing duration is this shot's own speech and action, inside the
    // selected model's window. A fixed 8s fallback made every Wan clip look
    // like 8s even though that model accepts up to 30s.
    let duration = super::clip_beats::clip_need_secs(clip, &shot);
    refresh_shot_packet(
        scene_dir,
        &shot,
        &characters,
        &registry,
        &world_pairs,
        &style,
        None,
        seam,
        duration,
        max_reference_audio.max(3),
        true,
    )
    .await
}

async fn load_shot_for_packet(scene_dir: &Path, shot_idx: i32) -> VimaxResult<ShotDescription> {
    let per = scene_dir
        .join("shots")
        .join(shot_idx.to_string())
        .join("shot_description.json");
    if per.is_file() {
        return read_json_artifact(&per).await;
    }
    if let Ok(all) =
        read_json_artifact::<Vec<ShotDescription>>(&scene_dir.join("shot_descriptions.json")).await
    {
        if let Some(shot) = all.into_iter().find(|s| s.idx == shot_idx) {
            return Ok(shot);
        }
    }
    // Filmstrip cards come from storyboard.json. Packing / a partial plan can
    // leave a row with no shot_description.json — still seed a packet so the
    // director desk can save layout instead of 400 "has no packet".
    if let Ok(board) =
        read_json_artifact::<Vec<ShotBriefDescription>>(&scene_dir.join("storyboard.json")).await
    {
        if let Some(brief) = board.into_iter().find(|row| row.idx == shot_idx) {
            return Ok(super::clip_beats::shot_from_brief(&brief));
        }
    }
    Err(VimaxError::InvalidParams(format!("shot {shot_idx} not found")))
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
                // Prefer the user three-view sheet, then Cameo, then generated front.
                // Keep both user plates when present so wardrobe + turnaround both lock identity.
                let mut pushed = false;
                for key in ["sheet", "cameo", "front"] {
                    let Some(item) = views.get(key) else {
                        continue;
                    };
                    let Some(p) = item.get("path") else {
                        continue;
                    };
                    let path = crate::session::resolve_stored_asset_path(p, film_root);
                    if !media_local::is_usable_image_file(&path) {
                        continue;
                    }
                    let file_name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("portrait.png");
                    let desc = item.get("description").cloned().unwrap_or_else(|| {
                        format!(
                            "File [{file_name}] = GLOBAL character bible for <{}>: {feats}. Lock face/hair/outfit.",
                            ch.identifier_in_scene
                        )
                    });
                    available.push((path, desc));
                    pushed = true;
                }
                if pushed {
                    continue;
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
        "CAST LOCK (must match character design sheet): {}. Do not invent new faces/outfits. ",
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

/// Seedance R2V image-array ceiling. Do not invent tighter per-category caps —
/// model capacity will move; plot + this API limit decide who is bound.
const MAX_SEEDANCE_REF_IMAGES: usize = 9;

/// Split a remaining-slot budget between two plot-relevant groups.
/// No per-group ceiling — if both fit, both go in; if not, share by count
/// (each side keeps at least one plate when the budget allows).
fn share_ref_slots(need_a: usize, need_b: usize, budget: usize) -> (usize, usize) {
    if need_a + need_b <= budget {
        return (need_a, need_b);
    }
    if need_a == 0 {
        return (0, need_b.min(budget));
    }
    if need_b == 0 {
        return (need_a.min(budget), 0);
    }
    if budget == 0 {
        return (0, 0);
    }
    if budget == 1 {
        return if need_a >= need_b { (1, 0) } else { (0, 1) };
    }
    let take_a = ((budget * need_a) / (need_a + need_b))
        .max(1)
        .min(need_a)
        .min(budget - 1);
    let take_b = (budget - take_a).min(need_b);
    (take_a, take_b)
}

/// Seedance R2V strip. The only hard cap is the API image-array limit.
/// In-shot portraits (user Cameo first) and plot-mentioned props share that
/// budget — no extra "max N faces" heuristic.
fn pick_video_assets(
    pairs: Vec<(PathBuf, String)>,
    continuity: Option<&Path>,
    keep_env: bool,
) -> Vec<(PathBuf, String)> {
    let mut portraits = Vec::new();
    let mut envs = Vec::new();
    let mut props = Vec::new();
    let mut rest = Vec::new();
    for (p, t) in pairs {
        if is_last_frame_ref_path(&p) || continuity.is_some_and(|c| p == c) {
            continue;
        }
        if is_portrait_ref_path(&p) {
            portraits.push((p, t));
        } else if is_environment_ref_path(&p) {
            envs.push((p, t));
        } else if is_prop_ref_path(&p) {
            props.push((p, t));
        } else {
            rest.push((p, t));
        }
    }
    portraits.sort_by_key(|(p, _)| {
        let s = p.to_string_lossy().to_ascii_lowercase();
        if s.contains("_cameo") || s.contains("/cameo/") {
            0u8
        } else {
            1u8
        }
    });
    let mut out = Vec::new();
    let remaining = |n: usize| MAX_SEEDANCE_REF_IMAGES.saturating_sub(n);
    let env_reserve = if keep_env && !envs.is_empty() { 1usize } else { 0 };
    let budget = remaining(out.len()).saturating_sub(env_reserve);
    let (portrait_take, prop_take) = share_ref_slots(portraits.len(), props.len(), budget);
    out.extend(portraits.drain(..portrait_take));
    out.extend(props.drain(..prop_take));
    if keep_env {
        let env_take = remaining(out.len()).min(1).min(envs.len());
        out.extend(envs.drain(..env_take));
    }
    let rest_take = remaining(out.len()).min(rest.len());
    out.extend(rest.drain(..rest_take));
    out.truncate(MAX_SEEDANCE_REF_IMAGES);
    out
}

/// Previous shot in timeline order within the same scene (by idx).
#[allow(dead_code)]
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

/// Last usable `video_last_frame.png` in a scene, for display.
/// The short-drama renderer does not submit it as the next shot's reference.
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

/// Board row → clip. One published storyboard card becomes one video job.
fn clips_from_board(
    briefs: &[ShotBriefDescription],
    characters: &[CharacterInScene],
) -> Vec<ShotDescription> {
    let mut out = super::clip_beats::shots_from_packed_briefs(briefs);
    for desc in &mut out {
        let vis = shot_cast_idxs(desc, characters);
        desc.ff_vis_char_idxs = vis.clone();
        desc.lf_vis_char_idxs = vis;
    }
    out
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
        // Leftover numbered dirs (pre-pack cache, absorbed idx) become phantom
        // last cards on the filmstrip if they stay under shots/N.
        park_or_delete_shot_dir(&entry.path()).await;
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

pub(crate) async fn load_scene_bgm_paren(working_dir: &Path) -> String {
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
    for (open, close) in [('<', '>'), ('〈', '〉')] {
        let Some(start) = text.find(open) else {
            continue;
        };
        let rest = &text[start + open.len_utf8()..];
        let Some(end) = rest.find(close) else {
            continue;
        };
        let name = rest[..end].trim();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn shot_world_query(shot: &ShotDescription) -> String {
    let mut blob = format!(
        "{} {} {} {} {}",
        shot.ff_desc.trim(),
        shot.motion_desc.trim(),
        shot.lf_desc.trim(),
        shot.visual_desc.trim(),
        shot.audio_desc.as_deref().unwrap_or("").trim()
    );
    for beat in &shot.beats {
        blob.push(' ');
        blob.push_str(beat.motion_desc.trim());
        if let Some(a) = &beat.audio_desc {
            blob.push(' ');
            blob.push_str(a.trim());
        }
    }
    blob
}

fn prop_name_needles(path: &Path, text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let push = |names: &mut Vec<String>, raw: &str| {
        let name = raw.trim().trim_matches(|c| c == '_' || c == '-');
        if name.is_empty() {
            return;
        }
        let chars = name.chars().count();
        let cjk = name.chars().filter(|c| crate::planning::is_cjk_speech_char(*c)).count();
        if cjk >= 2 || (cjk == 0 && chars >= 3) {
            names.push(name.to_string());
        }
    };
    if let Some(n) = extract_bracket_name(text) {
        push(&mut names, n);
    }
    if let Some(dir) = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
    {
        let cleaned = dir
            .split_once('_')
            .map(|(_, rest)| rest)
            .unwrap_or(dir);
        if !cleaned.eq_ignore_ascii_case("props") {
            push(&mut names, cleaned);
        }
    }
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        let cleaned = stem
            .trim_end_matches("_prop")
            .trim_end_matches("-prop");
        let cleaned = cleaned
            .split_once('_')
            .map(|(_, rest)| rest)
            .unwrap_or(cleaned);
        push(&mut names, cleaned);
    }
    names
}

fn query_mentions_prop(query: &str, path: &Path, text: &str) -> bool {
    let q = query.to_ascii_lowercase();
    prop_name_needles(path, text)
        .iter()
        .any(|n| q.contains(&n.to_ascii_lowercase()))
}

fn mentioned_prop_pairs(
    query: &str,
    world_pairs: &[(PathBuf, String)],
) -> Vec<(PathBuf, String)> {
    world_pairs
        .iter()
        .filter(|(p, t)| is_prop_ref_path(p) && query_mentions_prop(query, p, t))
        .cloned()
        .collect()
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

/// Cast that must stay on the Seedance image strip: vis idxs plus anyone named
/// in the shot text. Last-frame continuity is geography, not identity — empty
/// vis idxs used to drop portraits and leave only a prop.
fn shot_cast_idxs(shot: &ShotDescription, characters: &[CharacterInScene]) -> Vec<i32> {
    let mut idxs = Vec::new();
    let mut push = |idx: i32| {
        if !idxs.contains(&idx) {
            idxs.push(idx);
        }
    };
    for &i in &shot.ff_vis_char_idxs {
        push(i);
    }
    for &i in &shot.lf_vis_char_idxs {
        push(i);
    }
    let mut blob = String::new();
    for piece in [
        shot.visual_desc.as_str(),
        shot.motion_desc.as_str(),
        shot.ff_desc.as_str(),
        shot.lf_desc.as_str(),
        shot.audio_desc.as_deref().unwrap_or(""),
    ] {
        blob.push_str(piece);
        blob.push(' ');
    }
    for beat in &shot.beats {
        blob.push_str(&beat.motion_desc);
        blob.push(' ');
        if let Some(a) = &beat.audio_desc {
            blob.push_str(a);
            blob.push(' ');
        }
    }
    for ch in characters {
        if crate::domain::character_mentioned_in(&blob, ch, characters) {
            push(ch.idx);
        }
    }
    let picture = format!(
        "{} {} {} {}",
        shot.visual_desc, shot.motion_desc, shot.ff_desc, shot.lf_desc
    );
    if super::clip_beats::is_close_up_text(&picture) {
        let placed: Vec<i32> = characters
            .iter()
            .filter(|ch| crate::drama::name_has_own_screen_side(&picture, &ch.identifier_in_scene))
            .map(|ch| ch.idx)
            .collect();
        if !placed.is_empty() {
            idxs.retain(|idx| placed.contains(idx));
        }
    } else if super::clip_beats::shows_the_room(&picture) && text_mentions_group(&picture) {
        for ch in characters {
            if is_group_character(ch) {
                push(ch.idx);
            }
        }
    }
    idxs
}

fn text_mentions_group(text: &str) -> bool {
    const MARKS: &[&str] = &[
        "同伴", "众人", "群演", "几个人", "三四个", "打手", "随从", "伙计",
    ];
    MARKS.iter().any(|mark| text.contains(mark))
}

fn is_group_character(ch: &crate::domain::CharacterInScene) -> bool {
    const MARKS: &[&str] = &["同伴", "众人", "群演", "打手", "随从", "extras", "crowd"];
    let id = ch.identifier_in_scene.as_str();
    MARKS.iter().any(|mark| id.contains(mark))
        || ch.static_features.contains("群像")
        || ch.static_features.to_ascii_lowercase().contains("group of")
}

/// A white-background catalog shot shows the prop in the hand. When this row
/// puts that prop on a head, a table, or a temple, the catalog pulls it back
/// into the hand, so the row keeps the state in text and drops the catalog.
fn prop_catalog_conflicts(query: &str, path: &Path, label: &str) -> bool {
    const OFF_HAND: &[&str] = &[
        "头顶", "后脑", "头上", "太阳穴", "桌上", "桌面", "顶住", "顶在", "口袋", "地上", "椅背",
    ];
    prop_name_needles(path, label)
        .iter()
        .any(|name| char_window_contains(query, name, OFF_HAND))
}

fn char_window_contains(text: &str, name: &str, markers: &[&str]) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let name_chars: Vec<char> = name.chars().collect();
    if name_chars.is_empty() {
        return false;
    }
    let mut i = 0;
    while i + name_chars.len() <= chars.len() {
        if chars[i..i + name_chars.len()] == name_chars[..] {
            let start = i.saturating_sub(16);
            let end = (i + name_chars.len() + 16).min(chars.len());
            let window: String = chars[start..end].iter().collect();
            if markers.iter().any(|marker| window.contains(marker)) {
                return true;
            }
            i += name_chars.len();
        } else {
            i += 1;
        }
    }
    false
}

fn looks_cjk(text: &str) -> bool {
    text.chars()
        .filter(|c| crate::planning::is_cjk_speech_char(*c))
        .take(2)
        .count()
        >= 2
}

fn is_last_frame_ref_path(path: &Path) -> bool {
    path_key_lower(path).contains("video_last_frame")
}

fn style_short(style: &str) -> String {
    crate::planning::video_style_clause(style)
        .trim_start_matches("Look:")
        .trim()
        .trim_end_matches('.')
        .to_string()
}

fn constraint_line(cjk: bool, duration_secs: u32, aspect_ratio: &str, style: &str) -> String {
    let mut bits = Vec::new();
    bits.push(if cjk {
        "画面无任何字幕。".to_string()
    } else {
        "No on-screen subtitles.".to_string()
    });
    let ratio = aspect_ratio.trim();
    if !ratio.is_empty() {
        let ratio = crate::aspect::normalize_aspect_ratio(ratio);
        let portrait = matches!(ratio.as_str(), "9:16" | "3:4");
        bits.push(if cjk {
            if portrait {
                format!("{ratio}，主体完整入画。")
            } else {
                format!("{ratio}。")
            }
        } else if portrait {
            format!("{ratio}, keep the full subject in frame.")
        } else {
            format!("{ratio}.")
        });
    }
    bits.push(if cjk {
        format!("约{duration_secs}秒。")
    } else {
        format!("About {duration_secs}s.")
    });
    let look = style_short(style);
    if !look.is_empty() {
        bits.push(format!("{look}."));
    }
    bits.join(" ")
}

fn video_ref_role(path: &Path, text: &str, cjk: bool) -> String {
    if is_last_frame_ref_path(path) {
        return if cjk {
            "上一镜结束画面，不要复刻姿势".into()
        } else {
            "previous ending still, do not copy its pose".into()
        };
    }
    if is_portrait_ref_path(path) {
        let who = extract_bracket_name(text).unwrap_or(if cjk { "角色" } else { "cast" });
        return if cjk {
            format!("<{who}> 身份")
        } else {
            format!("<{who}> identity")
        };
    }
    if is_environment_ref_path(path) {
        return if cjk {
            "这间房的环境板，按本条站位把人放进去".into()
        } else {
            "room plate; place people from this shot's blocking".into()
        };
    }
    if is_prop_ref_path(path) {
        let who = extract_bracket_name(text).unwrap_or(if cjk { "道具" } else { "prop" });
        return if cjk {
            format!("<{who}> 道具")
        } else {
            format!("<{who}> prop")
        };
    }
    crate::planning::clip_at_break(text, 48)
}

fn video_at_image_bindings(ref_pairs: &[(PathBuf, String)], _seam: SpliceSeam, cjk: bool) -> String {
    if ref_pairs.is_empty() {
        return String::new();
    }
    let sep = if cjk { "。" } else { ". " };
    let bits: Vec<String> = ref_pairs
        .iter()
        .enumerate()
        .map(|(i, (path, text))| {
            format!("@Image{} {}", i + 1, video_ref_role(path, text, cjk))
        })
        .collect();
    let joined = bits.join(sep);
    if cjk && !joined.ends_with('。') {
        format!("{joined}。")
    } else if !cjk && !joined.ends_with('.') {
        format!("{joined}.")
    } else {
        joined
    }
}

/// Every clip opens from its own blocking text. The previous file's last frame
/// is not an input, so the line does not tell the model to resume a pose.
fn clause_has_blocking(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains("画面左")
        || text.contains("画面右")
        || text.contains("画面中")
        || text.contains("画面正中")
        || text.contains("面向")
        || text.contains("背对")
        || text.contains("前景")
        || text.contains("中景")
        || text.contains("后景")
        || text.contains("靠近镜头")
        || lower.contains("screen left")
        || lower.contains("screen right")
        || lower.contains("screen center")
        || lower.contains("facing ")
        || lower.contains("foreground")
        || lower.contains("midground")
        || lower.contains("background")
}

fn push_blocking_piece(buf: &mut String, pieces: &mut Vec<String>) {
    let piece = buf
        .trim()
        .trim_matches(|c: char| "，,。；;".contains(c))
        .trim();
    if piece.chars().count() >= 4 && clause_has_blocking(piece) {
        pieces.push(piece.to_string());
    }
    buf.clear();
}

/// Pull the clauses that actually place people and furniture, so the video
/// model reads blocking before the action. Empty when the shot text has none.
fn blocking_clauses(text: &str) -> String {
    let mut pieces = Vec::new();
    let mut buf = String::new();
    for ch in text.chars() {
        if matches!(ch, '。' | '！' | '？' | '；' | '\n' | '!' | '?' | ';') {
            push_blocking_piece(&mut buf, &mut pieces);
        } else {
            buf.push(ch);
        }
    }
    push_blocking_piece(&mut buf, &mut pieces);
    let mut refined = Vec::new();
    for piece in pieces {
        if piece.chars().count() > 80 {
            for sub in piece.split(['，', ',']) {
                let sub = sub.trim().trim_matches(|c: char| "，,。；;".contains(c));
                if sub.chars().count() >= 4 && clause_has_blocking(sub) {
                    refined.push(sub.to_string());
                }
            }
        } else {
            refined.push(piece);
        }
    }
    let mut out = String::new();
    for piece in refined {
        if out.chars().count() >= 240 {
            break;
        }
        if !out.is_empty() {
            out.push('；');
        }
        out.push_str(&piece);
    }
    out
}

fn seam_line(_seam: SpliceSeam, cjk: bool) -> &'static str {
    if cjk {
        "按本镜画面描述开场：每个人的画面左右、朝向、与场景固定陈设的相对位置以本镜文字为准，动作已经在进行。"
    } else {
        "Open from this shot's own blocking: screen side, facing, and position against the set anchors follow this shot's text, with the action already in progress."
    }
}

fn video_cast_clause(
    characters: &[CharacterInScene],
    shot: &ShotDescription,
    style: &str,
    has_identity_refs: bool,
    cjk: bool,
) -> String {
    let idxs = shot_cast_idxs(shot, characters);
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
    let child_line = if !has_child {
        ""
    } else if cjk {
        "儿童与成人同一画风。"
    } else if crate::planning::wants_stylized_non_photoreal(style) {
        "Children share the SAME animation style as adults."
    } else {
        "Children share the SAME cinematic style as adults."
    };
    if has_identity_refs {
        return child_line.to_string();
    }
    let compact = character_identity_clause(characters, &idxs, style);
    if child_line.is_empty() {
        compact
    } else if compact.is_empty() {
        child_line.to_string()
    } else {
        compact
    }
}

fn single_lens_visual(shot: &ShotDescription) -> String {
    let visual = shot.visual_desc.trim();
    if !visual.is_empty() {
        return crate::planning::clip_at_break(visual, 720);
    }
    super::clip_beats::strip_authored_timecodes(&shot.motion_desc)
}

fn clip_prompt_lenses(shot: &ShotDescription) -> Vec<super::clip_beats::PromptLens> {
    let mut lenses = super::clip_beats::prompt_lenses(shot);
    if !lenses.is_empty() {
        return lenses;
    }
    let visual = single_lens_visual(shot);
    if visual.is_empty() && shot.audio_desc.as_deref().unwrap_or("").trim().is_empty() {
        return Vec::new();
    }
    lenses.push(super::clip_beats::PromptLens {
        visual,
        audio: shot.audio_desc.clone(),
        cut: false,
        secs_hint: None,
    });
    lenses
}

fn render_lens_audio(
    raw: Option<&str>,
    motion: &str,
    visual: &str,
    scene_bgm: &str,
    essential_only: bool,
    emit_bgm: bool,
) -> (String, String) {
    let audio = raw.unwrap_or("").trim();
    let mined = if !audio.is_empty() {
        strip_conflicting_voice_color_cues(audio)
    } else if crate::planning::text_looks_like_dialogue(motion) {
        strip_conflicting_voice_color_cues(motion)
    } else if crate::planning::text_looks_like_dialogue(visual) {
        strip_conflicting_voice_color_cues(visual)
    } else {
        String::new()
    };
    let mined = if essential_only {
        strip_bgm_stage_directions(&mined)
    } else {
        mined
    };
    let (line, sfx) = if mined.is_empty() {
        (String::new(), String::new())
    } else {
        split_dialogue_and_sfx(&mined)
    };
    let sfx = super::clip_beats::strip_sfx_restating_line(&line, &sfx);
    let line = crate::dialogue::rewrite_spoken_payloads(&trim_audio_brackets(&line));
    let (line, peeled_sfx) = super::clip_beats::peel_trailing_stage_sfx(&line);
    let mut sfx = trim_audio_brackets(&sfx);
    if !peeled_sfx.is_empty() {
        sfx = if sfx.is_empty() {
            peeled_sfx
        } else {
            format!("{sfx} {peeled_sfx}")
        };
    }
    if line.is_empty() && sfx.is_empty() && emit_bgm && !essential_only {
        sfx = if looks_cjk(visual) || looks_cjk(motion) {
            "环境底噪与画面同步的拟音".into()
        } else {
            "environmental ambience and scene-matched foley matching on-screen action".into()
        };
    } else if line.is_empty() && sfx.is_empty() && essential_only && emit_bgm {
        sfx = "environmental ambience and essential on-screen foley only — no music".into();
    }
    let sfx = if emit_bgm && !essential_only {
        let bgm = crate::planning::format_scene_bgm_paren(scene_bgm);
        if sfx.is_empty() {
            bgm
        } else if sfx.contains('(') {
            replace_or_append_bgm_paren(&format!("<{sfx}>"), scene_bgm)
                .trim_matches(|c| c == '<' || c == '>')
                .trim()
                .to_string()
        } else {
            format!("<{sfx}> {bgm}")
        }
    } else if sfx.is_empty() {
        String::new()
    } else if sfx.starts_with('<') {
        sfx
    } else {
        format!("<{sfx}>")
    };
    (line, sfx)
}

pub(crate) fn i2v_motion_prompt(
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    style: &str,
    ref_pairs: &[(PathBuf, String)],
    duration_secs: u32,
    seam: SpliceSeam,
    scene_bgm: &str,
    _use_voice_audio_ref: bool,
    audio_bound_speakers: &[&str],
    aspect_ratio: &str,
) -> String {
    let lenses = clip_prompt_lenses(shot);
    let mut blob = String::new();
    blob.push_str(&shot.visual_desc);
    blob.push_str(&shot.motion_desc);
    blob.push_str(shot.audio_desc.as_deref().unwrap_or(""));
    for lens in &lenses {
        blob.push_str(&lens.visual);
        blob.push_str(lens.audio.as_deref().unwrap_or(""));
    }
    let cjk = looks_cjk(&blob);
    let has_dialogue = shot.has_spoken_dialogue();
    let has_identity_refs = ref_pairs.iter().any(|(p, _)| is_portrait_ref_path(p));
    let identities = clip_identity_names(characters, ref_pairs, &lenses);
    let mut placed_anywhere = Vec::new();
    for lens in &lenses {
        for name in names_with_screen_side(&lens.visual, &identities) {
            if !placed_anywhere.iter().any(|p| p == &name) {
                placed_anywhere.push(name);
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    parts.push(constraint_line(cjk, duration_secs, aspect_ratio, style));
    let ref_clause = video_at_image_bindings(ref_pairs, seam, cjk);
    if !ref_clause.is_empty() {
        parts.push(ref_clause);
    }
    if has_identity_refs || identities.len() >= 2 {
        parts.push(one_body_rule(cjk));
    }
    if has_dialogue {
        parts.push(if cjk {
            "音色由文字锁定：每个说话人按声线描述开口，同一角色在每条视频里保持同一音色。".into()
        } else {
            "Voice is locked in text: each speaker keeps the described voice, and the same timbre in every clip.".into()
        });
        let lock_cap = audio_bound_speakers.len().max(4);
        let lock = character_voice_lock_clause(
            characters,
            &speaker_idxs_for_shot(shot, characters),
            lock_cap,
        );
        if !lock.is_empty() {
            parts.push(lock);
        }
    }
    let join = seam_line(seam, cjk);
    if !join.is_empty() {
        parts.push(join.to_string());
    }
    let identity = video_cast_clause(characters, shot, style, has_identity_refs, cjk);
    if !identity.trim().is_empty() {
        parts.push(identity.trim().to_string());
    }

    let scene_lbl = if cjk { "画面：" } else { "Scene: " };
    let line_lbl = if cjk { "台词：" } else { "Line: " };
    let sfx_lbl = if cjk { "音效：" } else { "SFX: " };
    // Several cameras share one file. Number them in event order and lock the
    // room once. Wan accepts "Shot 1 / Shot 2" without timestamps; Seedance's
    // guide says exact seconds (0–3s) are unstable, so the duration stays a
    // parameter and the prompt never prints a clock.
    let multi = lenses.len() >= 2;
    if multi {
        if let Some(placed) = lenses.iter().find_map(|lens| {
            let placed = blocking_clauses(lens.visual.trim());
            (!placed.is_empty()).then_some(placed)
        }) {
            parts.push(if cjk {
                format!(
                    "空间站位：{placed}。环境板就是这间房，房间里没有人。后续机位不再重复房间、地面和家具。反打才对调左右。硬切，不要叠化，不要在切点之间走动或闪现。"
                )
            } else {
                format!(
                    "Blocking: {placed}. The room plate is the empty room. Later shots do not repeat the room, floor, or furniture. Only a reverse swaps screen sides. Hard cuts, no dissolve, and nobody walks or flashes across a cut."
                )
            });
        }
    }

    let mut bgm_emitted = false;
    let mut prev_visual = String::new();
    let mut prev_placed: Vec<String> = Vec::new();
    for (i, lens) in lenses.iter().enumerate() {
        let emit_bgm = !bgm_emitted;
        let (line, sfx) = render_lens_audio(
            lens.audio.as_deref(),
            &lens.visual,
            &lens.visual,
            scene_bgm,
            false,
            emit_bgm,
        );
        let mut block = String::new();
        let placed_now = names_with_screen_side(lens.visual.trim(), &identities);
        if multi {
            let reverse = i > 0
                && super::clip_beats::swapped_screen_blocking(&prev_visual, lens.visual.trim());
            let cast_changed = i > 0
                && !placed_now.is_empty()
                && !prev_placed.is_empty()
                && !same_name_set(&prev_placed, &placed_now);
            block.push_str(&camera_shot_header(i, lens.cut || reverse, reverse, cjk));
            block.push('\n');
            if i > 0 {
                block.push_str(&camera_join_line(reverse, cast_changed, lens.cut, cjk));
                block.push('\n');
            }
        }
        let roster = identity_roster_line(
            lens.visual.trim(),
            lens.audio.as_deref().unwrap_or(""),
            &identities,
            &placed_anywhere,
            cjk,
        );
        if !roster.is_empty() {
            block.push_str(&roster);
            block.push('\n');
        }
        if !lens.visual.trim().is_empty() {
            block.push_str(scene_lbl);
            block.push_str(lens.visual.trim());
            if !line.is_empty() && !visual_has_speech_cue(lens.visual.trim()) {
                let v = lens.visual.trim();
                if !v.ends_with(['。', '.', '！', '!', '？', '?']) {
                    block.push('。');
                }
                block.push_str(if cjk { "开口。" } else { " Mouth opens to speak." });
            }
            block.push('\n');
        }
        if !line.is_empty() {
            let inline = super::clip_beats::format_inline_spoken(&line);
            block.push_str(line_lbl);
            if inline.is_empty() {
                block.push('{');
                block.push_str(&line);
                block.push('}');
            } else {
                block.push_str(&inline);
            }
            block.push('\n');
            bgm_emitted |= emit_bgm && !sfx.is_empty();
        }
        if !sfx.is_empty() {
            block.push_str(sfx_lbl);
            block.push_str(&sfx);
            block.push('\n');
            bgm_emitted = true;
        }
        let block = block.trim_end();
        if !block.is_empty() {
            parts.push(block.to_string());
        }
        prev_visual = lens.visual.clone();
        prev_placed = placed_now;
    }
    if !bgm_emitted && !scene_bgm.trim().is_empty() {
        let bgm = crate::planning::format_scene_bgm_paren(scene_bgm);
        if !bgm.is_empty() {
            parts.push(format!("{}{bgm}", if cjk { "音效：" } else { "SFX: " }));
        }
    }
    parts.join("\n")
}

fn camera_join_line(reverse: bool, cast_changed: bool, cut: bool, cjk: bool) -> String {
    if cjk {
        if reverse {
            "左右对调只属于这一镜，不要让人在切点之间走动。".into()
        } else if cast_changed {
            "换了入画的人，按这一镜自己的站位摆，不要沿用上一镜的身体。".into()
        } else if cut {
            "换景别，站位不变。".into()
        } else {
            "同一机位，站位不变。".into()
        }
    } else if reverse {
        "The left/right swap belongs to this shot. Nobody walks across the cut.".into()
    } else if cast_changed {
        "The people in frame changed. Place this shot's own bodies. Do not keep the previous ones.".into()
    } else if cut {
        "New shot size, same blocking.".into()
    } else {
        "Same camera, same blocking.".into()
    }
}

fn camera_shot_header(index: usize, cut: bool, reverse: bool, cjk: bool) -> String {
    let n = index + 1;
    if cjk {
        if index == 0 {
            format!("镜头{n}")
        } else if reverse {
            format!("镜头{n} 硬切到反打")
        } else if cut {
            format!("镜头{n} 硬切到新机位")
        } else {
            format!("镜头{n}")
        }
    } else if index == 0 {
        format!("Shot {n}")
    } else if reverse {
        format!("Shot {n} hard cut to the reverse")
    } else if cut {
        format!("Shot {n} hard cut")
    } else {
        format!("Shot {n}")
    }
}

fn visual_has_speech_cue(visual: &str) -> bool {
    const CJK: &[&str] = &["开口", "说话", "说道", "念出", "喊道", "口型"];
    if CJK.iter().any(|n| visual.contains(n)) {
        return true;
    }
    let lower = visual.to_ascii_lowercase();
    lower.contains("speaks") || lower.contains("speaking") || lower.contains("mouth open")
}

fn one_body_rule(cjk: bool) -> String {
    if cjk {
        "每张身份参考图在整段视频里只对应一个身体。禁止把同一个人的脸贴到两个座位、两个身体，或没写名字的人身上。".into()
    } else {
        "Each identity reference is one body for the whole clip. Do not put the same face on two seats, two bodies, or an unnamed person.".into()
    }
}

fn clip_identity_names(
    characters: &[CharacterInScene],
    ref_pairs: &[(PathBuf, String)],
    lenses: &[super::clip_beats::PromptLens],
) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |raw: &str| {
        let name = raw
            .trim()
            .trim_matches(|c: char| matches!(c, '<' | '>' | '〈' | '〉'));
        if name.chars().count() < 2 || names.iter().any(|n| n == name) {
            return;
        }
        names.push(name.to_string());
    };
    for (path, text) in ref_pairs {
        if is_portrait_ref_path(path) {
            if let Some(name) = extract_bracket_name(text) {
                push(name);
            }
        }
    }
    let mut blob = String::new();
    for lens in lenses {
        blob.push_str(&lens.visual);
        blob.push(' ');
        blob.push_str(lens.audio.as_deref().unwrap_or(""));
        blob.push(' ');
    }
    for ch in characters {
        if crate::domain::character_mentioned_in(&blob, ch, characters) {
            push(&ch.identifier_in_scene);
        }
    }
    for lens in lenses {
        if let Some(audio) = lens.audio.as_deref() {
            for name in crate::dialogue::spoken_speaker_names(&strip_conflicting_voice_color_cues(
                audio,
            )) {
                push(&name);
            }
        }
    }
    names
}

fn names_with_screen_side(visual: &str, identities: &[String]) -> Vec<String> {
    identities
        .iter()
        .filter(|name| crate::drama::name_has_own_screen_side(visual, name))
        .cloned()
        .collect()
}

fn same_name_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|name| b.iter().any(|other| other == name))
}

/// Who may use a reference face in this beat, who already has a side in another
/// beat and must stay out, and who is named here without a side of their own.
/// The last group is not given an invented screen side.
fn identity_roster_line(
    visual: &str,
    audio: &str,
    identities: &[String],
    placed_anywhere: &[String],
    cjk: bool,
) -> String {
    if identities.is_empty() {
        return String::new();
    }
    let placed = names_with_screen_side(visual, identities);
    let speakers = crate::dialogue::spoken_speaker_names(&strip_conflicting_voice_color_cues(audio));
    let mut unplaced = Vec::new();
    let mut absent = Vec::new();
    for name in identities {
        if placed.iter().any(|p| p == name) {
            continue;
        }
        let speaks = speakers.iter().any(|s| s == name);
        let mentioned = visual.contains(name.as_str());
        if speaks || mentioned {
            unplaced.push(name.clone());
        } else if placed_anywhere.iter().any(|p| p == name) {
            absent.push(name.clone());
        }
    }
    if unplaced.is_empty() && absent.is_empty() && placed.len() < 2 {
        return String::new();
    }
    let mut parts = Vec::new();
    if cjk {
        if !placed.is_empty() {
            parts.push(format!("本拍入画：{}。每人只此一个身体。", placed.join("、")));
        }
        if !absent.is_empty() {
            parts.push(format!(
                "本拍不在画面：{}。不要用他们的脸。",
                absent.join("、")
            ));
        }
        if !unplaced.is_empty() {
            parts.push(format!(
                "本拍没写自己的站位：{}。只许各一个身体，禁止画面左右各一个，也不要把这张脸贴到没写名字的人、空位或第二个座位上。",
                unplaced.join("、")
            ));
        }
    } else {
        if !placed.is_empty() {
            parts.push(format!(
                "In frame this beat, one body each: {}.",
                placed.join(", ")
            ));
        }
        if !absent.is_empty() {
            parts.push(format!(
                "Out of this beat, do not use their face: {}.",
                absent.join(", ")
            ));
        }
        if !unplaced.is_empty() {
            parts.push(format!(
                "Named here but given no screen side: {}. One body each; do not put that face on both sides, an empty seat, or an unnamed person.",
                unplaced.join(", ")
            ));
        }
    }
    parts.join("\n")
}

/// Dialogue + essential foley only — no BGM. Used by tests of caption cleanup.
#[cfg(test)]
fn seedance_audio_caption_essential_only(
    audio_desc: Option<&str>,
    motion_desc: &str,
    visual_desc: &str,
) -> String {
    seedance_audio_caption_block(audio_desc, motion_desc, visual_desc, "", true)
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
///
/// Packed multi-shot `audio_desc` often interleaves `音效; BGM:… 角色:「台词」; BGM:…`.
/// Truncating at the first marker used to throw away later dialogue. Dialogue wins:
/// each BGM span is skipped until the next spoken line (or the end).
fn strip_bgm_stage_directions(s: &str) -> String {
    let t = strip_music_paren_segments(s);
    let mut out = String::new();
    let mut i = 0;
    while i < t.len() {
        let Some((rel, marker_len)) = next_bgm_marker(&t[i..]) else {
            out.push_str(&t[i..]);
            break;
        };
        out.push_str(&t[i..i + rel]);
        let after = i + rel + marker_len;
        let rest = &t[after..];
        match find_post_bgm_resume(rest) {
            Some(resume) => i = after + resume,
            None => break,
        }
    }
    collapse_ws(out.trim())
}

const BGM_MARKERS: &[&str] = &[
    "BGM:",
    "BGM：",
    "bgm:",
    "Bgm:",
    "背景音乐",
    "配乐：",
    "配乐:",
];

fn next_bgm_marker(s: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for marker in BGM_MARKERS {
        let Some(pos) = find_case_insensitive(s, marker) else {
            continue;
        };
        let len = marker.len();
        best = Some(match best {
            None => (pos, len),
            Some((p, l)) if pos < p || (pos == p && len > l) => (pos, len),
            Some(cur) => cur,
        });
    }
    best
}

/// Byte offset in a BGM tail where keepable audio resumes (台词 / quoted line).
fn find_post_bgm_resume(rest: &str) -> Option<usize> {
    const LINE: &[&str] = &["台词:", "台词："];
    let mut resume: Option<usize> = None;
    let consider = |cur: &mut Option<usize>, pos: usize| {
        *cur = Some(cur.map_or(pos, |p| p.min(pos)));
    };
    for marker in LINE {
        if let Some(pos) = find_case_insensitive(rest, marker) {
            consider(&mut resume, pos);
        }
    }
    if let Some(quote_at) = crate::dialogue::find_spoken_quote_byte(rest) {
        consider(&mut resume, crate::dialogue::speaker_line_start(rest, quote_at));
    }
    resume
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            out.push(ch);
        }
    }
    out
}

/// Video generation stays multi-reference. A previous clip's pixels are not
/// submitted: some models reject a frame role mixed with references, and a
/// generated frame trips face checks. The filmstrip may still store the still.
pub(crate) fn continuity_still_for_seam(
    _seam: SpliceSeam,
    _still: Option<&Path>,
) -> Option<&Path> {
    None
}

/// Multi-ref strip for Seedance R2V: optional caller still, then in-shot
/// portraits, the location plate, and mentioned props.
pub(crate) fn shot_video_ref_pairs(
    shot: &ShotDescription,
    continuity: Option<&Path>,
    characters: &[CharacterInScene],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    world_pairs: &[(PathBuf, String)],
    film_root: &Path,
    keep_env: bool,
) -> Vec<(PathBuf, String)> {
    let _ = continuity;
    let mut pairs: Vec<(PathBuf, String)> = Vec::new();
    let vis = shot_cast_idxs(shot, characters);
    pairs.extend(
        portrait_pairs(characters, &vis, registry, film_root)
            .into_iter()
            .map(|(p, t)| {
                let front = media_local::ensure_three_view_front_panel(&p);
                (front, t)
            }),
    );

    let world_query = shot_world_query(shot);
    if keep_env {
        if let Some(env) = resolve_environment_plate(&shot.location_id, world_pairs) {
            pairs.push(env);
        }
    }
    for (p, t) in mentioned_prop_pairs(&world_query, world_pairs) {
        if prop_duplicates_visible_cast(&p, &t, characters, &vis) {
            continue;
        }
        if prop_catalog_conflicts(&world_query, &p, &t) {
            continue;
        }
        if pairs.iter().any(|(have, _)| have == &p) {
            continue;
        }
        pairs.push((p, t));
    }

    let mut seen = std::collections::HashSet::new();
    pairs.retain(|(p, _)| seen.insert(p.clone()));
    pick_video_assets(pairs, continuity, keep_env)
}

/// Location bible always travels with the clip. Last-frame continuity is
/// people + framing; the empty environment plate is architecture identity.
fn keep_location_plate(
    _seam: SpliceSeam,
    _shot: &ShotDescription,
    _prev: Option<&ShotDescription>,
) -> bool {
    true
}

/// Speakers named in this clip's audio, stable by character idx (not vis order).
///
/// Visible-but-silent cast must not reshuffle `@AudioN`. If nobody is named but
/// the clip still has dialogue, fall back to visible cast sorted by idx.
fn speaker_idxs_for_shot(shot: &ShotDescription, characters: &[CharacterInScene]) -> Vec<i32> {
    let audio = shot.audio_source();
    let mut idxs: Vec<i32> = characters
        .iter()
        .filter(|ch| crate::domain::character_mentioned_in(&audio, ch, characters))
        .map(|ch| ch.idx)
        .collect();
    idxs.sort_unstable();
    idxs.dedup();
    if idxs.is_empty() && shot.has_spoken_dialogue() {
        let mut vis = shot_cast_idxs(shot, characters);
        vis.sort_unstable();
        vis.dedup();
        return vis;
    }
    idxs
}

/// Compact SPEAKER LOCK so Seedance keeps the same speaker card on every shot,
/// including characters already bound as `@AudioN`.
fn character_voice_lock_clause(
    characters: &[CharacterInScene],
    idxs: &[i32],
    max_locks: usize,
) -> String {
    let mut parts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let cap = max_locks.max(1);
    for &ci in idxs {
        if parts.len() >= cap {
            break;
        }
        let Some(ch) = characters.iter().find(|c| c.idx == ci) else {
            continue;
        };
        if !seen.insert(ch.idx) {
            continue;
        }
        if let Some(vp) = ch.voice_profile.as_ref().filter(|v| v.is_usable()) {
            parts.push(vp.compact_lock(&ch.identifier_in_scene));
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("VOICE LOCK: {}.", parts.join("; "))
}

/// Seedance 2.0 audio captions use typed brackets:
/// dialogue `{…}`, SFX `<…>`, music `(…)`.
/// Empty captions with `generate_audio=true` fail with InvalidParameter.
///
/// Resolves audio from storyboard `台词`/`音效` (or mines motion/visual). Does
/// **not** paste FIXED SPEAKER VOICE into the caption — that doubled the
/// storyboard line and fought `@AudioN`.
#[allow(dead_code)]
fn seedance_audio_caption_block(
    audio_desc: Option<&str>,
    motion_desc: &str,
    visual_desc: &str,
    scene_bgm: &str,
    essential_only: bool,
) -> String {
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
    let raw = if essential_only {
        strip_bgm_stage_directions(&raw)
    } else {
        raw
    };

    if raw.is_empty() {
        return if essential_only {
            "<environmental ambience and essential on-screen foley only — no music>".to_string()
        } else {
            let bgm = crate::planning::format_scene_bgm_paren(scene_bgm);
            format!(
                "<environmental ambience and scene-matched foley matching on-screen action> {bgm}"
            )
        };
    }

    let caption = format_storyboard_audio_caption(&raw);
    if essential_only {
        caption
    } else {
        replace_or_append_bgm_paren(&caption, scene_bgm)
    }
}

/// Turn storyboard `台词:…音效:…` (or already-typed `{…} <…>`) into Seedance captions
/// without wrapping the 台词 marker itself.
#[allow(dead_code)]
fn format_storyboard_audio_caption(raw: &str) -> String {
    const LINE: &[&str] = &["台词:", "台词："];
    let has_line_marker = LINE.iter().any(|m| find_case_insensitive(raw, m).is_some());
    let already_typed = (raw.contains('{') || raw.contains('<')) && !has_line_marker;
    if already_typed {
        return crate::dialogue::rewrite_spoken_payloads(raw.trim());
    }
    let (line, sfx) = split_dialogue_and_sfx(raw);
    let line = crate::dialogue::rewrite_spoken_payloads(&trim_audio_brackets(&line));
    let sfx = trim_audio_brackets(&sfx);
    match (line.is_empty(), sfx.is_empty()) {
        (true, true) => raw.trim().to_string(),
        (false, true) => format!("{{{line}}}"),
        (true, false) => format!("<{sfx}>"),
        (false, false) => format!("{{{line}}} <{sfx}>"),
    }
}

fn trim_audio_brackets(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '{' || c == '}' || c == '<' || c == '>')
        .trim()
        .to_string()
}

fn extract_after_marker(s: &str, markers: &[&str], stop: &[&str]) -> Option<String> {
    let (start, marker) = markers.iter().find_map(|m| {
        find_case_insensitive(s, m).map(|p| (p, *m))
    })?;
    let body_start = start + marker.len();
    let body_end = stop
        .iter()
        .filter_map(|m| find_case_insensitive(&s[body_start..], m).map(|p| body_start + p))
        .min()
        .unwrap_or(s.len());
    Some(s[body_start..body_end].trim().to_string())
}

fn split_dialogue_and_sfx(raw: &str) -> (String, String) {
    const LINE: &[&str] = &["台词:", "台词："];
    const SFX: &[&str] = &["音效:", "音效：", "SFX:", "sfx:"];
    let line = extract_after_marker(raw, LINE, SFX);
    let sfx = extract_after_marker(raw, SFX, LINE);
    match (line, sfx) {
        (Some(l), Some(x)) => (l, x),
        (Some(l), None) => (l, String::new()),
        (None, Some(x)) => (String::new(), x),
        (None, None) => crate::dialogue::split_spoken_and_sfx(raw),
    }
}

/// Force the scene-stable `(music)` caption: replace an existing music `(…)`
/// span, otherwise append. Acting/SFX parentheses (`(倒吸一口凉气)`) stay.
fn replace_or_append_bgm_paren(voiced: &str, scene_bgm: &str) -> String {
    let bgm = crate::planning::format_scene_bgm_paren(scene_bgm);
    if let Some(start) = voiced.find('(') {
        if let Some(rel_end) = voiced[start + 1..].find(')') {
            let end = start + 1 + rel_end;
            let existing = &voiced[start..=end];
            if crate::planning::paren_looks_like_bgm(existing) {
                let mut out = String::with_capacity(voiced.len() + bgm.len());
                out.push_str(&voiced[..start]);
                out.push_str(&bgm);
                out.push_str(&voiced[end + 1..]);
                return out;
            }
            return format!("{voiced} {bgm}");
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
    use super::super::privacy_face::content_index_to_image_slot;
    use crate::clip_bounds::ClipBounds;
    use crate::domain::{ShotBeat, VoiceProfile};

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
            location_id: String::new(),
            beats: Vec::new(),
        }
    }

    fn assert_not_english_runbook(prompt: &str) {
        for needle in [
            "ONE continuous take",
            "continuity reference ONLY",
            "Do NOT reproduce",
            "Keep it subtitle-free.",
            "Throughout:",
            "[0-",
            "Frame:",
            "Motion:",
            "PRODUCTION LOOK LOCK",
            "CAST LOCK",
            "REFERENCE BINDINGS",
            "CONTINUITY: Image 1",
            "PLOT LOCK:",
            "native multi-shot",
            "SAME take keeps rolling",
        ] {
            assert!(
                !prompt.contains(needle),
                "runbook leftover `{needle}`: {prompt}"
            );
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
    fn multi_ref_prompt_is_a_beat_card() {
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
            5,
            SpliceSeam::SameTake,
            "",
            false,
            &[],
            "9:16",
        );
        assert!(
            prompt.starts_with("画面无任何字幕"),
            "subtitle lock is the first line: {prompt}"
        );
        assert!(prompt.contains("约5秒"), "{prompt}");
        assert!(prompt.contains("9:16"), "{prompt}");
        assert!(prompt.contains("主体完整入画"), "{prompt}");
        assert!(prompt.contains("@Image1"));
        assert!(prompt.contains("按本镜画面描述开场"), "{prompt}");
        assert!(prompt.contains("不要复刻姿势"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(prompt.contains("@Image2"));
        assert!(prompt.contains("<林铮> 身份"), "{prompt}");
        assert!(prompt.contains("画面："), "{prompt}");
        assert!(prompt.contains("左方女性从防御起身"), "{prompt}");
        assert!(!prompt.contains("空间站位："), "{prompt}");
        assert!(!prompt.contains("16:9"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    #[test]
    fn prompt_restates_blocking_before_the_action() {
        let mut s = shot(1, 0);
        s.visual_desc = "中景平视固定。前景是铁门门框，柜台在画面右侧。<林夏>在画面左侧、铁门内侧一步，面向右，站着，比柜台更靠近镜头。她把信递过去。然后切到新机位：柜台特写，<周野>在画面右侧、柜台前，面向左，坐着，伸手接信。"
            .into();
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::MatchCut,
            "",
            false,
            &[],
            "9:16",
        );
        let first = prompt.find("空间站位：").expect(&prompt);
        let picture = prompt.find("画面：").expect(&prompt);
        assert!(first < picture, "{prompt}");
        assert_eq!(prompt.matches("空间站位：").count(), 1, "{prompt}");
        assert_eq!(prompt.matches("环境板就是这间房").count(), 1, "{prompt}");
        assert!(prompt.contains("镜头1"), "{prompt}");
        assert!(prompt.contains("镜头2"), "{prompt}");
        assert!(prompt.contains("画面左侧"), "{prompt}");
        assert!(prompt.contains("面向右"), "{prompt}");
        assert!(prompt.contains("铁门内侧一步"), "{prompt}");
        assert!(prompt.contains("硬切到新机位"), "{prompt}");
        assert!(prompt.contains("画面右侧"), "{prompt}");
        assert!(!prompt.contains("0-"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(!prompt.contains("接上一帧"), "{prompt}");
    }

    #[test]
    fn same_take_binds_last_frame_as_resume() {
        let mut s = shot(1, 0);
        s.motion_desc = "她转身".into();
        let refs = vec![(
            PathBuf::from("shots/0/video_last_frame.png"),
            "Previous ending".into(),
        )];

        let same = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            5,
            SpliceSeam::SameTake,
            "",
            false,
            &[],
            "",
        );
        assert!(same.contains("@Image1 上一镜结束画面，不要复刻姿势"), "{same}");
        assert!(same.contains("按本镜画面描述开场"), "{same}");
        assert!(!same.contains("接着演"), "{same}");
        assert_not_english_runbook(&same);
    }

    /// The anti-replay clause must hold even when no last frame was bound: a
    /// same-camera continuation that restages its opening pose reads as a freeze.
    #[test]
    fn same_take_without_a_bound_frame_still_forbids_a_replayed_pose() {
        let mut s = shot(1, 0);
        s.motion_desc = "她转身".into();
        let refs: Vec<(PathBuf, String)> = Vec::new();

        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            5,
            SpliceSeam::SameTake,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("按本镜画面描述开场"), "{prompt}");
        assert!(prompt.contains("动作已经在进行"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    #[test]
    fn match_cut_prompt_binds_last_frame_as_prior() {
        let mut s = shot(1, 1);
        s.motion_desc = "她转身".into();
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
            5,
            SpliceSeam::MatchCut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("@Image1 上一镜结束画面，不要复刻姿势"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(prompt.contains("按本镜画面描述开场"), "{prompt}");
        assert!(prompt.contains("@Image2 <林铮> 身份"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    #[test]
    fn match_cut_prompt_starts_on_a_portrait() {
        let mut s = shot(1, 0);
        s.motion_desc = "她转身".into();
        let refs = vec![(
            PathBuf::from("characters/alice_three_view.png"),
            "File [alice_three_view.png] = GLOBAL three-view character bible for <林铮>".into(),
        )];
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            5,
            SpliceSeam::MatchCut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("按本镜画面描述开场"), "{prompt}");
        assert!(prompt.contains("@Image1 <林铮> 身份"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(!prompt.contains("video_last_frame"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    /// A shot with no tail-frame reference still opens from its own blocking
    /// text, and does not mention a resume.
    #[test]
    fn a_hard_cut_prompt_opens_from_its_own_blocking() {
        let mut s = shot(1, 0);
        s.motion_desc = "她转身".into();
        let refs = vec![(
            PathBuf::from("characters/alice_three_view.png"),
            "File [alice_three_view.png] = GLOBAL three-view character bible for <林铮>".into(),
        )];
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            5,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("按本镜画面描述开场"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(!prompt.contains("切到新机位，动作已经在进行"), "{prompt}");
        assert!(prompt.contains("@Image1 <林铮> 身份"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    /// Two same-camera performances stay two videos. Neither prompt invents a
    /// second clock or holds the other row's action.
    #[test]
    fn same_camera_rows_prompt_as_two_performances() {
        let line: String = "中".chars().cycle().take(17).collect();
        let mut a = shot(0, 0);
        a.motion_desc = "她转身".into();
        a.audio_desc = Some(line);
        let mut b = shot(1, 0);
        b.motion_desc = "她走近窗边".into();
        let merged = super::super::clip_beats::pack_scene_clips(SEEDANCE, vec![a, b]);
        assert_eq!(merged.len(), 2, "same camera stays two performances");

        let first = i2v_motion_prompt(
            &merged[0],
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        let second = i2v_motion_prompt(
            &merged[1],
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(first.contains("她转身"), "{first}");
        assert!(!first.contains("她走近窗边"), "{first}");
        assert!(second.contains("她走近窗边"), "{second}");
        assert!(!second.contains("她转身"), "{second}");
        for prompt in [&first, &second] {
            assert!(!prompt.contains("镜头1"), "{prompt}");
            assert!(!prompt.contains("同一机位"), "{prompt}");
            assert!(prompt.contains("约8秒"), "{prompt}");
            assert_not_english_runbook(prompt);
        }
    }

    #[test]
    fn a_reverse_is_its_own_prompt_not_a_cut_inside_the_clip() {
        let mut first = shot(0, 0);
        first.motion_desc = "她说话".into();
        let mut second = shot(1, 1);
        second.motion_desc = "他回答".into();
        let packed = super::super::clip_beats::pack_scene_clips(SEEDANCE, vec![first, second]);
        assert_eq!(packed.len(), 2);
        assert!(!packed[0].has_camera_cuts());
        assert!(!packed[1].has_camera_cuts());

        let opening = i2v_motion_prompt(
            &packed[0],
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        let reverse = i2v_motion_prompt(
            &packed[1],
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(opening.contains("她说话"), "{opening}");
        assert!(!opening.contains("他回答"), "{opening}");
        assert!(reverse.contains("他回答"), "{reverse}");
        assert!(!reverse.contains("切到新机位"), "{reverse}");
        assert!(!reverse.contains("镜头1"), "{reverse}");
        assert_not_english_runbook(&reverse);
    }

    #[test]
    fn a_swapped_axis_is_the_next_row_and_restates_its_own_sides() {
        let mut first = shot(0, 0);
        first.visual_desc = "<林夏>在画面左侧、铁门内侧，面向右，站着。<周野>在画面右侧、柜台前，面向左，坐着。".into();
        let mut second = shot(1, 1);
        second.visual_desc = "反打。<周野>在画面左侧，面向右。<林夏>在画面右侧，面向左。".into();
        let packed = super::super::clip_beats::pack_scene_clips(SEEDANCE, vec![first, second]);
        assert_eq!(packed.len(), 2, "{packed:?}");
        let prompt = i2v_motion_prompt(
            &packed[1],
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "9:16",
        );
        assert!(prompt.contains("周野"), "{prompt}");
        assert!(prompt.contains("画面左侧"), "{prompt}");
        assert!(prompt.contains("林夏"), "{prompt}");
        assert!(prompt.contains("画面右侧"), "{prompt}");
        assert!(!prompt.contains("同一机位"), "{prompt}");
        assert!(!prompt.contains("站位保持"), "{prompt}");
        assert!(!prompt.contains("硬切到反打"), "{prompt}");
        assert!(!prompt.contains("@Audio"), "{prompt}");
    }

    /// A speaker bound as an identity, with no screen side of their own, must not
    /// be told to hold the previous beat — and must not be handed an invented side.
    #[test]
    fn an_unplaced_speaker_is_one_body_and_does_not_hold_the_previous_cast() {
        let mut s = shot(0, 0);
        s.beats = vec![
            ShotBeat {
                motion_desc: "近景俯视。<高二林>在画面中央、桌面上，面向下，趴着。<刘华强>在画面右侧、高二林身后，面向左，俯身。".into(),
                audio_desc: Some("刘华强：「把头低下。」".into()),
                cam_idx: Some(0),
            },
            ShotBeat {
                motion_desc: "中景平视固定。圆桌在画面中景。<封彪>在画面右侧、主位椅子上，面向左，坐着。<韩跃平>在画面右侧、封彪身后，面向左，站着。<刘华强>在画面中央偏左、封彪右边，面向右，站着。刘华强转头问大海，旁边坐着一个同伴。".into(),
                audio_desc: Some("刘华强：「有他吗？」大海：「有。」".into()),
                cam_idx: Some(0),
            },
        ];
        let refs = vec![
            (PathBuf::from("characters/liu_three_view.png"), "<刘华强>".into()),
            (PathBuf::from("characters/han_three_view.png"), "<韩跃平>".into()),
            (PathBuf::from("characters/hai_three_view.png"), "<大海>".into()),
            (PathBuf::from("characters/feng_three_view.png"), "<封彪>".into()),
            (PathBuf::from("characters/gao_three_view.png"), "<高二林>".into()),
        ];
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            27,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "16:9",
        );
        assert!(prompt.contains("只对应一个身体"), "{prompt}");
        assert!(prompt.contains("没写自己的站位"), "{prompt}");
        assert!(prompt.contains("大海"), "{prompt}");
        assert!(prompt.contains("换了入画的人"), "{prompt}");
        assert_eq!(prompt.matches("换了入画的人").count(), 1, "{prompt}");
        assert!(prompt.contains("不在画面"), "{prompt}");
        assert!(!prompt.contains("同一机位"), "{prompt}");
        assert!(!prompt.contains("与上一拍相同"), "{prompt}");
        assert!(!prompt.contains("站位保持"), "{prompt}");
        assert!(!prompt.contains("大海在画面"), "{prompt}");
        for line in prompt.lines() {
            if line.contains("不在画面") {
                assert!(!line.contains("大海"), "{line}");
            }
        }
    }

    #[test]
    fn the_same_placed_cast_still_holds_the_axis() {
        let mut s = shot(0, 0);
        s.beats = vec![
            ShotBeat {
                motion_desc: "<林夏>在画面左侧、门内一步，面向右，站着。<周野>在画面右侧、柜台前，面向左，坐着。林夏把信递过去。".into(),
                audio_desc: None,
                cam_idx: Some(0),
            },
            ShotBeat {
                motion_desc: "<林夏>在画面左侧、门内一步，面向右，站着。<周野>在画面右侧、柜台前，面向左，坐着。周野接过信。".into(),
                audio_desc: None,
                cam_idx: Some(0),
            },
        ];
        let refs = vec![
            (PathBuf::from("characters/lin_three_view.png"), "<林夏>".into()),
            (PathBuf::from("characters/zhou_three_view.png"), "<周野>".into()),
        ];
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &refs,
            10,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "16:9",
        );
        assert!(prompt.contains("同一机位"), "{prompt}");
        assert!(!prompt.contains("换了入画的人"), "{prompt}");
        assert!(prompt.contains("本拍入画：林夏、周野"), "{prompt}");
        assert!(prompt.contains("只对应一个身体"), "{prompt}");
    }

    /// Authored `0-4s:` windows never reach the model; the API duration does.
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
            5,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(!prompt.contains("0-4s"), "{prompt}");
        assert!(!prompt.contains("4-7s"), "{prompt}");
        assert!(!prompt.contains("镜头1"), "{prompt}");
        assert!(!prompt.contains("同一机位"), "{prompt}");
        assert_eq!(prompt.matches("画面：").count(), 1, "{prompt}");
        assert!(prompt.contains("约5秒"), "{prompt}");
        assert!(prompt.contains("摄影机固定"), "{prompt}");
        assert!(prompt.contains("他继续向右骑行"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    /// A single-beat clip keeps one 画面 line — no 镜头1 header.
    #[test]
    fn a_single_beat_clip_prompt_has_no_beat_timeline() {
        let mut s = shot(1, 0);
        s.motion_desc = "她转身".into();
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("画面：她转身"), "{prompt}");
        assert!(!prompt.contains("镜头1"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    /// A tail frame bound at Image 2+ is a previous-shot still, never a resume.
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
        let bindings = video_at_image_bindings(&refs, SpliceSeam::Cut, true);
        assert!(bindings.contains("@Image1 <林铮> 身份"), "{bindings}");
        assert!(bindings.contains("@Image2 上一镜结束画面，不要复刻姿势"), "{bindings}");
        assert!(!bindings.contains("接着演"), "{bindings}");
    }

    #[test]
    fn audio_caption_mines_motion_dialogue_and_always_has_bgm() {
        let scene_bgm = "(gentle piano motif, steady tempo, same across shots)";
        let from_motion = seedance_audio_caption_block(
            None,
            "他看着对方说道：「我们走吧」",
            "wide shot of two people",
            scene_bgm,
            false,
        );
        assert!(from_motion.contains('{'));
        assert!(from_motion.contains("我们走吧"));
        assert!(from_motion.contains("gentle piano motif"));

        let ambient = seedance_audio_caption_block(
            None,
            "slow pan across room",
            "establishing",
            scene_bgm,
            false,
        );
        assert!(ambient.contains('<') || ambient.contains("ambience") || ambient.contains("环境"));
        assert!(ambient.contains("gentle piano motif"));

        let typed = seedance_audio_caption_block(
            Some("{快跑} <脚步声>"),
            "runs",
            "chase",
            scene_bgm,
            false,
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
            scene_bgm,
            false,
        );
        assert!(
            replaced.contains("gentle piano motif") && !replaced.contains("EDM"),
            "per-shot music must be replaced by scene-stable BGM: {replaced}"
        );
    }

    #[test]
    fn audio_caption_does_not_vocalize_sfx_or_book_quotes() {
        let scene_bgm = "(gentle piano motif, steady tempo, same across shots)";
        let shot4 = seedance_audio_caption_block(
            Some("BGM:同前,弦乐渐强至收束。键盘「咔哒」一声脆响,屏幕提示音「面试课降价公告已发布」;随后是<粉总>轻轻呼出一口气的声音。"),
            "按下回车",
            "屏幕弹出面试课降价公告",
            scene_bgm,
            false,
        );
        assert!(!shot4.contains("{面试课降价公告已发布}"), "{shot4}");
        assert!(!shot4.contains("{咔哒}"), "{shot4}");
        assert!(shot4.contains('<'), "{shot4}");

        let shot10 = seedance_audio_caption_block(
            Some("手机扣在桌面的「砰」一声闷响,办公室空调低鸣。BGM:同前。"),
            "扣手机",
            "粉总坐在电脑前",
            scene_bgm,
            false,
        );
        assert!(!shot10.contains("{砰}"), "{shot10}");

        let thanks = seedance_audio_caption_block(
            Some("袁老师:「感谢『追梦人超哥』的火箭！超哥你是来听课的还是来拱火的？」"),
            "开口",
            "直播间",
            scene_bgm,
            false,
        );
        assert!(thanks.contains("追梦人超哥"), "{thanks}");
        assert!(!thanks.contains('『') && !thanks.contains('』'), "{thanks}");

        let mut s = shot(0, 0);
        s.audio_desc = Some("键盘「咔哒」一声脆响,屏幕提示音「面试课降价公告已发布」".into());
        s.visual_desc = "粉总按下回车".into();
        s.motion_desc = "按下回车".into();
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            5,
            SpliceSeam::Cut,
            scene_bgm,
            false,
            &[],
            "",
        );
        assert!(
            !prompt.contains("台词：{面试课降价公告已发布}")
                && !prompt.contains("Line: {面试课降价公告已发布}"),
            "{prompt}"
        );
        assert!(
            prompt.contains("音效") || prompt.contains("SFX") || prompt.contains('<'),
            "{prompt}"
        );
    }

    #[test]
    fn audio_caption_injects_character_voice_lock() {
        let mut vp = VoiceProfile {
            timbre: "清亮女中音".into(),
            volume: Some("normal".into()),
            pitch: Some("mid-high".into()),
            speaking_style: "语速平稳".into(),
            caption_clause: None,
            tts_voice: None,
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
            "",
            false,
        );
        assert!(caption.contains("李薇"));
        assert!(caption.contains("今晚别等我"));
        assert!(
            !caption.contains("FIXED SPEAKER VOICE"),
            "caption must round-trip storyboard dialogue, not a voice bible: {caption}"
        );
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
                location_id: String::new(),
                beats: Vec::new(),
            },
            &chars,
            "cinematic",
            &[],
            10,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("VOICE LOCK"));
        assert!(prompt.contains("清亮女中音"));
        assert!(
            !prompt.contains("FIXED SPEAKER VOICE"),
            "full voice bible must not be pasted into the clip prompt: {prompt}"
        );
        assert!(
            !prompt.contains("用低沉的声音"),
            "timbre-redefining stage directions must be stripped so VOICE LOCK wins"
        );
        assert!(prompt.contains("今晚别等我"));
        assert!(prompt.contains("台词：") || prompt.contains("Line: "), "{prompt}");
        assert!(!prompt.contains("Throughout:"), "{prompt}");
        assert!(
            !prompt.contains("Speak at a natural conversational pace"),
            "{prompt}"
        );
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
    fn audio_ref_mode_keeps_scene_bgm() {
        let s = shot(1, 0);
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "(piano motif)",
            true,
            &["阿琳"],
            "",
        );
        assert!(!prompt.contains("@Audio"));
        assert!(!prompt.contains("VOICE LOCK"));
        assert!(
            prompt.contains("piano motif"),
            "dialogue wav does not strip scene BGM: {prompt}"
        );
        assert!(
            !prompt.contains("No background music"),
            "do not tell Seedance to drop BGM: {prompt}"
        );

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
    fn audio_ref_mode_keeps_dialogue_after_interleaved_bgm() {
        let packed = "烈日下人群低语声与衣料摩擦声,霜华剑入缝时一声清越的金属嗡鸣,金色禁制符文亮起时伴随低沉的阵法轰鸣;BGM:低音弦乐与冷冽古琴的持续压迫型底乐,节奏缓慢如心跳,贯穿全场景。 萧彻:「圣子若想要,大比上自己来取。」语气不疾不徐,尾音带一丝居高临下的冷意;BGM:同前,低音弦乐与古琴的压迫型底乐持续,鼓点轻敲,节奏不变。";
        let essential = seedance_audio_caption_essential_only(Some(packed), "入剑", "演武场");
        assert!(
            essential.contains("圣子若想要") && essential.contains("大比上自己来取"),
            "packed-clip dialogue after BGM must survive: {essential}"
        );
        assert!(
            essential.contains("萧彻"),
            "speaker tag must stay with the line: {essential}"
        );
        assert!(
            essential.contains('{') && essential.contains('}'),
            "spoken line must be a Seedance dialogue caption: {essential}"
        );
        assert!(
            essential.contains("金属嗡鸣") || essential.contains("人群低语"),
            "leading foley must remain: {essential}"
        );
        assert!(
            !essential.contains("低音弦乐") && !essential.to_ascii_lowercase().contains("bgm"),
            "BGM is less important than dialogue and must be dropped: {essential}"
        );

        let between = seedance_audio_caption_essential_only(
            Some("李薇：「走吧。」BGM:钢琴铺底。阿琳：「别走。」"),
            "对峙",
            "中景",
        );
        assert!(between.contains("走吧") && between.contains("别走"), "{between}");
        assert!(!between.contains("钢琴"), "{between}");

        let mut s = shot(0, 0);
        s.audio_desc = Some(packed.to_string());
        s.motion_desc = "萧彻入剑后转身说话".into();
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            14,
            SpliceSeam::Cut,
            "",
            true,
            &["萧彻", "林昭"],
            "",
        );
        assert!(
            prompt.contains("圣子若想要"),
            "i2v prompt must carry the spoken line, not only SFX: {prompt}"
        );
        assert!(
            prompt.contains("台词：") || prompt.contains("Line: "),
            "dialogue lives in the lens, not a trailing dump: {prompt}"
        );
        assert!(!prompt.contains("Throughout:"), "{prompt}");
    }

    #[test]
    fn cut_to_prompt_interleaves_dialogue_per_lens_and_peels_bgm() {
        let mut s = shot(0, 0);
        s.visual_desc = "近景 <罗子君>正面。CUT TO <陈俊生>近景。CUT TO <唐晶>侧脸围观。".into();
        s.audio_desc = Some(
            "罗子君：「……陈俊生。」陈俊生：「不是我！我没点！」唐晶：「你别装了。」\
凌玲：「卡片上写得很清楚。」贺建军：「大家冷静。」康总：「把监控调出来。」\
弹幕刷屏；BGM:低音铺底"
                .into(),
        );
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            15,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("镜头1"), "{prompt}");
        assert!(prompt.contains("镜头2"), "{prompt}");
        assert!(prompt.contains("镜头3"), "{prompt}");
        assert!(!prompt.contains("0-"), "{prompt}");
        let parts: Vec<&str> = prompt.split("镜头").collect();
        assert!(parts.len() >= 4, "{prompt}");
        let line1 = parts[1];
        assert!(line1.contains("……陈俊生"), "lens 1 must speak first: {prompt}");
        assert!(
            !line1.contains("不是我"),
            "陈俊生's line must not dump into lens 1: {prompt}"
        );
        assert!(prompt.contains("不是我"), "{prompt}");
        assert!(prompt.contains("卡片上写得很清楚"), "{prompt}");
        assert!(prompt.contains("把监控调出来"), "{prompt}");
        let dialogue_lines: String = prompt
            .lines()
            .filter(|l| l.contains("台词：") || l.contains("Line: "))
            .collect();
        assert!(
            !dialogue_lines.to_ascii_lowercase().contains("bgm")
                && !dialogue_lines.contains("低音铺底")
                && !dialogue_lines.contains("弹幕"),
            "BGM/弹幕 belong in 音效, not {{台词}}: {prompt}"
        );
        assert!(
            prompt.contains("音效：") || prompt.contains("SFX: "),
            "{prompt}"
        );
        assert!(
            prompt.contains("开口"),
            "speaking lenses need a mouth-open cue: {prompt}"
        );
    }

    #[test]
    fn audio_ref_mode_keeps_text_lock_for_other_speakers() {
        fn vp(name: &str, timbre: &str) -> VoiceProfile {
            let mut v = VoiceProfile {
                timbre: timbre.into(),
                volume: Some("normal".into()),
                pitch: Some("mid".into()),
                speaking_style: "平稳".into(),
                caption_clause: None,
                tts_voice: None,
            };
            v.normalize(name);
            v
        }
        let chars = vec![
            CharacterInScene {
                idx: 0,
                identifier_in_scene: "李薇".into(),
                is_visible: true,
                static_features: "成年女性".into(),
                dynamic_features: None,
                voice_profile: Some(vp("李薇", "清亮女中音")),
            },
            CharacterInScene {
                idx: 1,
                identifier_in_scene: "阿琳".into(),
                is_visible: true,
                static_features: "成年女性".into(),
                dynamic_features: None,
                voice_profile: Some(vp("阿琳", "偏暖女中音")),
            },
        ];
        let mut s = shot(0, 0);
        s.audio_desc = Some("李薇：「走吧。」阿琳：「别走。」".into());
        let prompt = i2v_motion_prompt(
            &s,
            &chars,
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            true,
            &["李薇"],
            "",
        );
        assert!(!prompt.contains("@Audio"), "{prompt}");
        assert!(prompt.contains("音色由文字锁定"), "{prompt}");
        let lock_line = prompt
            .lines()
            .find(|line| line.contains("VOICE LOCK"))
            .expect("speakers keep the written voice lock");
        assert!(lock_line.contains("阿琳"), "{lock_line}");
        assert!(
            lock_line.contains("李薇"),
            "every named speaker keeps the same lock string: {lock_line}"
        );
        assert!(!prompt.contains("Throughout:"), "{prompt}");
    }

    #[test]
    fn named_cast_is_inferred_when_vis_idxs_empty() {
        let mut s = shot(0, 0);
        s.ff_vis_char_idxs.clear();
        s.lf_vis_char_idxs.clear();
        s.visual_desc = "林尘与赵无极对峙".into();
        s.audio_desc = Some("台词:林尘:「站住」;赵无极:「放下」音效:剑鸣、风声".into());
        let chars = vec![
            CharacterInScene {
                idx: 0,
                identifier_in_scene: "林尘".into(),
                is_visible: true,
                static_features: "青年男性".into(),
                dynamic_features: None,
                voice_profile: None,
            },
            CharacterInScene {
                idx: 1,
                identifier_in_scene: "赵无极".into(),
                is_visible: true,
                static_features: "中年男性".into(),
                dynamic_features: None,
                voice_profile: None,
            },
        ];
        let idxs = shot_cast_idxs(&s, &chars);
        assert!(idxs.contains(&0) && idxs.contains(&1), "{idxs:?}");
    }

    #[test]
    fn honorific_alias_binds_protagonist_portrait() {
        let mut s = shot(0, 0);
        s.ff_vis_char_idxs.clear();
        s.lf_vis_char_idxs.clear();
        s.visual_desc = "中景:老祖指了指密室逃脱的牌子。王胖子倒吸一口凉气。".into();
        s.audio_desc = Some("(倒吸一口凉气)".into());
        let chars = vec![
            CharacterInScene {
                idx: 0,
                identifier_in_scene: "玄霄老祖".into(),
                is_visible: true,
                static_features: "成年男性".into(),
                dynamic_features: None,
                voice_profile: None,
            },
            CharacterInScene {
                idx: 2,
                identifier_in_scene: "王胖子".into(),
                is_visible: true,
                static_features: "成年男性".into(),
                dynamic_features: None,
                voice_profile: None,
            },
        ];
        let idxs = shot_cast_idxs(&s, &chars);
        assert!(idxs.contains(&0), "老祖 must bind 玄霄老祖: {idxs:?}");
        assert!(idxs.contains(&2), "{idxs:?}");
    }

    #[test]
    fn sfx_paren_is_kept_when_scene_bgm_is_appended() {
        let mut s = shot(0, 0);
        s.visual_desc = "中景:<玄霄老祖>指了指牌子。<王胖子>倒吸一口凉气。".into();
        s.motion_desc = s.visual_desc.clone();
        s.audio_desc = Some("(倒吸一口凉气)".into());
        let chars = [CharacterInScene {
            idx: 0,
            identifier_in_scene: "玄霄老祖".into(),
            is_visible: true,
            static_features: "成年男性".into(),
            dynamic_features: None,
            voice_profile: None,
        }];
        let prompt = i2v_motion_prompt(
            &s,
            &chars,
            "cinematic",
            &[],
            2,
            SpliceSeam::Cut,
            "(soft continuous cinematic atmospheric underscore, same motif)",
            false,
            &[],
            "",
        );
        assert!(
            prompt.contains("倒吸一口凉气"),
            "shot SFX must not be replaced by scene BGM: {prompt}"
        );
        assert!(
            !prompt.contains("激动带哭腔"),
            "{prompt}"
        );
    }

    #[test]
    fn pick_video_assets_keeps_portraits_ahead_of_env() {
        let cont = PathBuf::from("shots/0/video_last_frame.png");
        let pairs = vec![
            (cont.clone(), "cont".into()),
            (
                PathBuf::from("character_portraits/0/lin_three_view.png"),
                "<林尘>".into(),
            ),
            (
                PathBuf::from("character_portraits/1/zhao_three_view.png"),
                "<赵无极>".into(),
            ),
            (PathBuf::from("props/token_prop.png"), "<玄铁令>".into()),
            (PathBuf::from("environments/hall.png"), "hall".into()),
        ];
        let out = pick_video_assets(pairs, Some(&cont), false);
        let blob: String = out
            .iter()
            .map(|(p, _)| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("|");
        assert!(blob.contains("lin_three_view"), "{blob}");
        assert!(blob.contains("zhao_three_view"), "{blob}");
        assert!(blob.contains("token_prop"), "{blob}");
        assert!(
            !blob.contains("environments"),
            "keep_env false drops the plate: {blob}"
        );
        assert!(
            !blob.contains("video_last_frame"),
            "a previous frame is not a video reference: {blob}"
        );
        assert!(out[0].0.to_string_lossy().contains("three_view"));
    }

    #[test]
    fn video_refs_are_multi_reference_only() {
        let still = PathBuf::from("shots/0/video_last_frame.png");
        assert_eq!(
            continuity_still_for_seam(SpliceSeam::MatchCut, Some(&still)),
            None
        );
        assert_eq!(continuity_still_for_seam(SpliceSeam::Cut, Some(&still)), None);
        assert_eq!(
            continuity_still_for_seam(SpliceSeam::SameTake, Some(&still)),
            None
        );

        let pairs = vec![
            (still.clone(), "cont".into()),
            (
                PathBuf::from("character_portraits/0/lin_three_view.png"),
                "<林尘>".into(),
            ),
            (PathBuf::from("props/token_prop.png"), "<玄铁令>".into()),
            (PathBuf::from("environments/hall.png"), "hall".into()),
        ];
        let out = pick_video_assets(pairs, Some(&still), true);
        assert!(
            out.iter()
                .all(|(p, _)| !p.to_string_lossy().contains("video_last_frame")),
            "frames stay off the reference strip: {out:?}"
        );
        assert!(
            out.iter()
                .any(|(p, _)| p.to_string_lossy().contains("environments")),
            "the room plate stays on the strip: {out:?}"
        );
    }

    #[test]
    fn same_take_still_keeps_location_plate() {
        let mut cur = shot(1, 0);
        let mut prev = shot(0, 0);
        prev.location_id = "INT. CAFE - NIGHT".into();
        cur.location_id = "INT. CAFE - NIGHT".into();
        assert!(keep_location_plate(SpliceSeam::SameTake, &cur, Some(&prev)));
        assert!(keep_location_plate(SpliceSeam::MatchCut, &cur, Some(&prev)));
        cur.location_id = "EXT. PIER - NIGHT".into();
        assert!(keep_location_plate(SpliceSeam::SameTake, &cur, Some(&prev)));
        assert!(keep_location_plate(SpliceSeam::Cut, &cur, None));
    }

    #[test]
    fn empty_location_id_still_binds_sole_environment_plate() {
        let mut s = shot(1, 0);
        s.visual_desc = "taxi back seat".into();
        s.location_id.clear();
        let env = PathBuf::from("environments/0_TAXI/TAXI_environment_plate.png");
        let world = vec![(
            env.clone(),
            "GLOBAL EMPTY environment plate: 出租车后座".into(),
        )];
        let pairs = shot_video_ref_pairs(
            &s,
            None,
            &[],
            &HashMap::new(),
            &world,
            Path::new("."),
            true,
        );
        assert!(
            pairs.iter().any(|(p, _)| p == &env),
            "sole environment plate must ride with the clip: {pairs:?}"
        );
    }

    #[test]
    fn speaker_idxs_follow_named_speakers_not_visible_order() {
        let mut s = shot(0, 0);
        s.ff_vis_char_idxs = vec![1, 0];
        s.audio_desc = Some("李薇：「走吧。」".into());
        let chars = vec![
            CharacterInScene {
                idx: 0,
                identifier_in_scene: "李薇".into(),
                is_visible: true,
                static_features: String::new(),
                dynamic_features: None,
                voice_profile: None,
            },
            CharacterInScene {
                idx: 1,
                identifier_in_scene: "阿琳".into(),
                is_visible: true,
                static_features: String::new(),
                dynamic_features: None,
                voice_profile: None,
            },
        ];
        assert_eq!(speaker_idxs_for_shot(&s, &chars), vec![0]);
    }

    #[test]
    fn pick_video_assets_binds_every_mentioned_prop_that_fits() {
        let cont = PathBuf::from("shots/0/video_last_frame.png");
        let pairs = vec![
            (cont.clone(), "cont".into()),
            (
                PathBuf::from("character_portraits/0/lin_three_view.png"),
                "<林尘>".into(),
            ),
            (
                PathBuf::from("character_portraits/1/zhao_three_view.png"),
                "<赵无极>".into(),
            ),
            (PathBuf::from("props/0_长老令/长老令_prop.png"), "<长老令>".into()),
            (PathBuf::from("props/1_霜华剑/霜华剑_prop.png"), "<霜华剑>".into()),
            (PathBuf::from("props/2_红伞/红伞_prop.png"), "<红伞>".into()),
        ];
        let out = pick_video_assets(pairs, Some(&cont), false);
        let blob: String = out
            .iter()
            .map(|(p, _)| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("|");
        assert!(blob.contains("长老令"), "{blob}");
        assert!(blob.contains("霜华剑"), "{blob}");
        assert!(blob.contains("红伞"), "{blob}");
        assert!(out.len() <= MAX_SEEDANCE_REF_IMAGES, "{}", out.len());
        assert!(blob.contains("lin_three_view"), "{blob}");
        assert!(blob.contains("zhao_three_view"), "{blob}");
    }

    #[test]
    fn pick_video_assets_keeps_all_in_shot_portraits_when_they_fit() {
        let cont = PathBuf::from("shots/0/video_last_frame.png");
        let pairs = vec![
            (cont.clone(), "cont".into()),
            (
                PathBuf::from("cameo/hero_cameo.png"),
                "<主角>".into(),
            ),
            (
                PathBuf::from("character_portraits/1/a_three_view.png"),
                "<甲>".into(),
            ),
            (
                PathBuf::from("character_portraits/2/b_three_view.png"),
                "<乙>".into(),
            ),
            (
                PathBuf::from("character_portraits/3/c_three_view.png"),
                "<丙>".into(),
            ),
            (PathBuf::from("props/0_剑/剑_prop.png"), "<霜华剑>".into()),
        ];
        let out = pick_video_assets(pairs, Some(&cont), false);
        let faces: Vec<_> = out
            .iter()
            .filter(|(p, _)| is_portrait_ref_path(p))
            .collect();
        assert_eq!(faces.len(), 4, "{out:?}");
        assert!(
            faces[0].0.to_string_lossy().contains("cameo"),
            "Cameo stays first among faces: {out:?}"
        );
    }

    #[test]
    fn pick_video_assets_does_not_starve_props_for_extra_portraits() {
        let cont = PathBuf::from("shots/0/video_last_frame.png");
        let mut pairs = vec![(cont.clone(), "cont".into())];
        for i in 0..6 {
            pairs.push((
                PathBuf::from(format!("character_portraits/{i}/c{i}_three_view.png")),
                format!("<角色{i}>"),
            ));
        }
        pairs.push((PathBuf::from("props/0_令/令牌_prop.png"), "<令牌>".into()));
        pairs.push((PathBuf::from("props/1_剑/长剑_prop.png"), "<长剑>".into()));
        pairs.push((PathBuf::from("props/2_伞/油纸伞_prop.png"), "<油纸伞>".into()));
        let out = pick_video_assets(pairs, Some(&cont), false);
        let blob: String = out
            .iter()
            .map(|(p, _)| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("|");
        assert!(blob.contains("令牌"), "plot props must survive extra faces: {blob}");
        assert!(blob.contains("长剑"), "{blob}");
        assert!(blob.contains("油纸伞"), "{blob}");
        assert_eq!(out.len(), MAX_SEEDANCE_REF_IMAGES, "{blob}");
        let face_n = out.iter().filter(|(p, _)| is_portrait_ref_path(p)).count();
        assert!(face_n > 2, "no hardcoded 2-face cap: {face_n} in {blob}");
        assert!(
            !blob.contains("video_last_frame"),
            "a previous clip's pixels are not a reference: {blob}"
        );
        assert!(is_portrait_ref_path(&out[0].0), "{out:?}");
    }

    #[test]
    fn i2v_prompt_carries_storyboard_visual_as_scene() {
        let mut s = shot(0, 0);
        s.visual_desc =
            "正午烈日下演武场人群围成半圈，萧彻单手托起霜华剑，剑身冰蓝流光游走，迈步走向剑冢入口"
                .into();
        s.motion_desc = "他迈步向前".into();
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(
            prompt.contains("画面：正午烈日下演武场"),
            "video prompt must include the storyboard 画面描述: {prompt}"
        );
        assert!(prompt.contains("霜华剑"), "{prompt}");
        assert!(!prompt.contains("Scene:"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    #[test]
    fn prop_refs_follow_shot_mentions_not_fallback() {
        let token = (
            PathBuf::from("props/0_长老令/长老令_prop.png"),
            "object only: <长老令>".into(),
        );
        let umbrella = (
            PathBuf::from("props/1_红伞/红伞_prop.png"),
            "object only: <红伞>".into(),
        );
        let world = vec![token.clone(), umbrella.clone()];
        let mentioned = mentioned_prop_pairs("萧彻举起长老令对着人群", &world);
        assert_eq!(mentioned.len(), 1, "{mentioned:?}");
        assert!(mentioned[0].0.to_string_lossy().contains("长老令"));
        assert!(!query_mentions_prop(
            "萧彻单手托起霜华剑走进剑冢",
            &token.0,
            &token.1
        ));
        assert!(query_mentions_prop(
            "他从腰间解下长老令",
            &token.0,
            &token.1
        ));
    }

    #[test]
    fn dialogue_caption_round_trips_storyboard_audio_desc() {
        let cap = seedance_audio_caption_essential_only(
            Some("台词:林尘:「站住」;赵无极:「放下」音效:剑鸣、风声"),
            "对峙",
            "中景",
        );
        assert!(cap.contains("{林尘:「站住」;赵无极:「放下」}"), "{cap}");
        assert!(cap.contains("<剑鸣、风声>"), "{cap}");
        assert!(!cap.contains("台词:"), "{cap}");
        assert!(
            !cap.contains("foley only"),
            "do not invent English foley when 音效 is present: {cap}"
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
            location_id: String::new(),
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
            8,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "16:9",
        );
        assert!(!prompt.contains("Children share"));
        assert!(!prompt.contains("CAST LOCK"));
        assert!(prompt.contains("@Image1"));
        assert!(prompt.contains("<林铮> 身份"), "{prompt}");
        assert!(prompt.contains("画面：中景"), "{prompt}");
        assert!(prompt.contains("16:9"), "{prompt}");
        assert!(!prompt.contains("vertical"), "{prompt}");
        assert!(!prompt.contains("large creatures"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    #[test]
    fn empty_aspect_omits_frame_line() {
        let s = shot(1, 0);
        let prompt = i2v_motion_prompt(
            &s,
            &[],
            "cinematic",
            &[],
            5,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(!prompt.contains("Frame:"), "{prompt}");
        assert!(!prompt.contains("16:9"), "{prompt}");
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
            location_id: String::new(),
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
    use crate::clip_bounds::ClipBounds;
    use crate::domain::ShotBriefDescription;

    const SEEDANCE: ClipBounds = ClipBounds::new(5, 15);

    fn brief(idx: i32, cam_idx: i32, visual: &str) -> ShotBriefDescription {
        ShotBriefDescription {
            idx,
            is_last: false,
            cam_idx,
            visual_desc: visual.into(),
            audio_desc: None,
            location_id: String::new(),
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
        assert_eq!(packed.len(), 4, "one performance stays one published row");
        assert!(packed.iter().all(|row| !row.is_merged()));
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
        let clips = super::super::clip_beats::shots_from_packed_briefs(&disk);
        assert!(super::super::clip_beats::clips_follow_board(&disk, &clips));
        assert_eq!(clips.len(), disk.len());
    }

    #[tokio::test]
    async fn persist_replaces_an_unpacked_draft_left_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let wd = dir.path();
        let path = wd.join("storyboard.json");
        let mut stale = micro_draft();
        stale.push(brief(9, 0, "幽灵镜头"));
        write_json_artifact(&path, &stale).await.unwrap();
        write_sidecar(&path, "plan-fp").await.unwrap();
        let packed = super::super::clip_beats::pack_scene_briefs(SEEDANCE, micro_draft());
        assert!(packed.len() < stale.len());
        assert_eq!(packed.len(), 4);
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

#[cfg(test)]
mod packet_seed_tests {
    use super::*;
    use crate::domain::{ShotBriefBeat, ShotBriefDescription};

    fn brief(idx: i32, last: bool) -> ShotBriefDescription {
        ShotBriefDescription {
            idx,
            is_last: last,
            cam_idx: 0,
            visual_desc: format!("visual {idx}"),
            audio_desc: None,
            location_id: "INT. CAFE - NIGHT".into(),
            beats: Vec::new(),
        }
    }

    #[tokio::test]
    async fn refresh_seeds_packet_from_storyboard_when_shot_files_missing() {
        let dir = tempfile::tempdir().unwrap();
        let scene = dir.path().join("idea2video").join("scene_0");
        std::fs::create_dir_all(&scene).unwrap();
        let mut packed = brief(2, true);
        packed.visual_desc = "packed row".into();
        packed.audio_desc = Some("line".into());
        packed.beats = vec![
            ShotBriefBeat {
                visual_desc: "beat a".into(),
                audio_desc: None,
                cam_idx: 0,
            },
            ShotBriefBeat {
                visual_desc: "beat b".into(),
                audio_desc: None,
                cam_idx: 1,
            },
        ];
        write_json_artifact(&scene.join("storyboard.json"), &vec![brief(0, false), packed])
            .await
            .unwrap();

        let packet = refresh_shot_packet_from_disk(&scene, 2, 3, ClipBounds::new(2, 30))
            .await
            .unwrap();
        assert_ne!(packet.duration_secs, Some(8), "a missing duration must not become a fixed 8s");
        let secs = packet.duration_secs.expect("planned duration");
        assert!((2..=30).contains(&secs), "secs={secs}");
        assert_eq!(packet.shot_idx, 2);
        assert_eq!(packet.visual_desc, "packed row");
        assert!(packet.beats.len() >= 2);
        assert!(scene.join("shots").join("2").join("shot_packet.json").is_file());
        let loaded = load_packet(&scene, 2).await.unwrap().expect("seeded packet");
        assert_eq!(loaded.shot_idx, 2);
    }

    #[tokio::test]
    async fn refresh_missing_storyboard_row_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let scene = dir.path().join("idea2video").join("scene_0");
        std::fs::create_dir_all(&scene).unwrap();
        write_json_artifact(&scene.join("storyboard.json"), &vec![brief(0, true)])
            .await
            .unwrap();
        let err = refresh_shot_packet_from_disk(&scene, 2, 3, ClipBounds::new(2, 30))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("shot 2 not found"),
            "unexpected error: {err}"
        );
        assert!(!scene.join("shots").join("2").join("shot_packet.json").is_file());
    }
}
