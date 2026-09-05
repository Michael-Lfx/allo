//! Script2Video pipeline — plan text artifacts then render clips/final.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    CharacterExtractor, CharacterPortraitsGenerator, StoryboardArtist, VoiceProfileGenerator,
    VoiceReferenceGenerator, WorldAssetsPlanner, ensure_film_cover, has_usable_portrait,
    rank_world_pairs_for_frame, voice_ref_abs_path, world_asset_pairs,
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
    ensure_ai_sanitized_privacy_face, ensure_seedance_privacy_face,
    is_seedance_privacy_image_err_text, next_privacy_tier_for_path,
    parse_seedance_flagged_content_index, preflight_video_ref_privacy, privacy_repair_targets,
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
        let storyboard = StoryboardArtist::new(Arc::clone(&backends.chat), backends.clip);
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
                self.generate_character_portraits(&characters, &style, script, &progress)
                    .await?;
                emit_pct(
                    &progress,
                    "voice_references_start",
                    "正在生成角色音色参考音频",
                    24.0,
                );
                self.ensure_character_voice_references(&characters, &progress)
                    .await
            };
            let world = async {
                emit_pct(
                    &progress,
                    "world_assets_start",
                    "正在生成全局环境与道具参考图",
                    30.0,
                );
                let world_planner = WorldAssetsPlanner::new(
                    Arc::clone(&self.backends.chat),
                    Arc::clone(&self.backends.image),
                );
                let (style_refs, scene_hint, lock_token) = world_cameo_context(&self.working_dir);
                world_planner
                    .ensure(
                        &film_root,
                        script,
                        &style,
                        &style_refs,
                        &scene_hint,
                        &lock_token,
                    )
                    .await?;
                Ok(())
            };
            let board = async {
                emit_pct(&progress, "design_storyboard", "正在设计分镜表", 40.0);
                self.design_storyboard(script, &characters, user_requirement, &plan_fp)
                    .await
            };
            let ((), (), storyboard) = tokio::try_join!(portraits_voices, world, board)?;
            storyboard
        };

        emit_pct(&progress, "shot_descriptions", "正在落盘镜头描述", 62.0);
        let shot_descriptions = self
            .persist_shot_descriptions(&storyboard, &characters, &plan_fp)
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
        let bgm = ensure_scene_bgm_brief(&self.working_dir, &packed).await?;
        tracing::info!(bgm = %bgm, "scene BGM brief locked for shot continuity");
        Ok(packed)
    }

    async fn persist_shot_descriptions(
        &self,
        briefs: &[ShotBriefDescription],
        characters: &[CharacterInScene],
        plan_fp: &str,
    ) -> VimaxResult<Vec<ShotDescription>> {
        let aggregate = self.working_dir.join("shot_descriptions.json");
        let clips = clips_from_board(briefs, characters);
        let (synced, packed, _) =
            commit_packed_shot_layout(&self.working_dir, briefs.to_vec(), clips).await;
        if super::clip_beats::storyboard_differs(briefs, &synced) {
            write_json_artifact(&self.working_dir.join("storyboard.json"), &synced).await?;
        }
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
    pub(crate) async fn ensure_character_voice_references(
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

        let continuity_ref = continuity_still_for_seam(seam, continuity_source.as_deref());
        let ref_pairs = shot_video_ref_pairs(
            shot,
            continuity_ref,
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
        // Voice clips invite invented speech on silent shots; only bind when this beat talks.
        let audio_ref_pairs = if shot_has_spoken_dialogue(shot) {
            shot_speaker_voice_refs(shot, characters, registry, &film_root)
        } else {
            Vec::new()
        };
        let use_voice_audio_ref = !audio_ref_pairs.is_empty();
        let audio_bound_names: Vec<&str> =
            audio_ref_pairs.iter().map(|(n, _)| n.as_str()).collect();
        let ref_audio_paths: Vec<&Path> =
            audio_ref_pairs.iter().map(|(_, p)| p.as_path()).collect();
        let aspect_ratio = crate::aspect::load_aspect_from_dir(&self.working_dir).await;
        let prompt = i2v_motion_prompt(
            shot,
            characters,
            style,
            &ref_pairs,
            duration_secs,
            seam,
            scene_bgm,
            use_voice_audio_ref,
            &audio_bound_names,
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

        preflight_video_ref_privacy(
            self.backends.image.as_ref(),
            Some(Arc::clone(&self.backends.chat)),
            &ref_paths,
        )
        .await?;

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
                    use_voice_audio_ref,
                    &audio_bound_names,
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
) -> Vec<(PathBuf, String)> {
    let mut continuity_pair = None;
    let mut portraits = Vec::new();
    let mut envs = Vec::new();
    let mut props = Vec::new();
    let mut rest = Vec::new();
    for (p, t) in pairs {
        if continuity.is_some_and(|c| p == c) {
            continuity_pair = Some((p, t));
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
    if let Some(c) = continuity_pair {
        out.push(c);
    }
    let remaining = |n: usize| MAX_SEEDANCE_REF_IMAGES.saturating_sub(n);
    // Opening continuity already carries the set; reserve one env slot only when
    // this clip has to establish the location from a plate.
    let env_reserve = if continuity.is_none() && !envs.is_empty() {
        1usize
    } else {
        0
    };
    let budget = remaining(out.len()).saturating_sub(env_reserve);
    let (portrait_take, prop_take) = share_ref_slots(portraits.len(), props.len(), budget);
    out.extend(portraits.drain(..portrait_take));
    out.extend(props.drain(..prop_take));
    if continuity.is_none() {
        let env_take = remaining(out.len()).min(1).min(envs.len());
        out.extend(envs.drain(..env_take));
    }
    let rest_take = remaining(out.len()).min(rest.len());
    out.extend(rest.drain(..rest_take));
    out.truncate(MAX_SEEDANCE_REF_IMAGES);
    out
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

fn shot_has_spoken_dialogue(shot: &ShotDescription) -> bool {
    crate::planning::text_looks_like_dialogue(&shot_audio_source(shot))
        || crate::planning::text_looks_like_dialogue(&shot.motion_desc)
        || crate::planning::text_looks_like_dialogue(&shot.visual_desc)
}

/// Storyboard `audio_desc` is the source of truth; packed beats fill in when the
/// parent row left it empty.
fn shot_audio_source(shot: &ShotDescription) -> String {
    let top = shot.audio_desc.as_deref().unwrap_or("").trim();
    if !top.is_empty() {
        return top.to_string();
    }
    let mut parts = Vec::new();
    for beat in &shot.beats {
        if let Some(a) = beat
            .audio_desc
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(a);
        }
    }
    parts.join(" ")
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
        let name = ch.identifier_in_scene.trim();
        if name.chars().count() >= 2 && blob.contains(name) {
            push(ch.idx);
        }
    }
    idxs
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

fn video_ref_role(path: &Path, text: &str, resume: bool, cjk: bool) -> String {
    if resume {
        return if cjk {
            "上一镜接着演".into()
        } else {
            "previous shot continues".into()
        };
    }
    if is_last_frame_ref_path(path) {
        return if cjk {
            "上一镜".into()
        } else {
            "previous shot".into()
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
        return if cjk { "场景".into() } else { "set".into() };
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

fn video_at_image_bindings(ref_pairs: &[(PathBuf, String)], seam: SpliceSeam, cjk: bool) -> String {
    if ref_pairs.is_empty() {
        return String::new();
    }
    let sep = if cjk { "。" } else { ". " };
    let bits: Vec<String> = ref_pairs
        .iter()
        .enumerate()
        .map(|(i, (path, text))| {
            let resume = i == 0 && seam == SpliceSeam::SameTake && is_last_frame_ref_path(path);
            format!("@Image{} {}", i + 1, video_ref_role(path, text, resume, cjk))
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

fn seam_line(seam: SpliceSeam, cjk: bool, resume_bound: bool) -> &'static str {
    match seam {
        SpliceSeam::Cut => "",
        SpliceSeam::SameTake if resume_bound => "",
        SpliceSeam::SameTake => {
            if cjk {
                "同一机位接着演。"
            } else {
                "Same camera keeps rolling."
            }
        }
        SpliceSeam::MatchCut => {
            if cjk {
                "切到新机位，动作已经在进行。"
            } else {
                "Cut to a new angle with the action already in progress."
            }
        }
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
    let line = trim_audio_brackets(&line);
    let mut sfx = trim_audio_brackets(&sfx);
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

fn i2v_motion_prompt(
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    style: &str,
    ref_pairs: &[(PathBuf, String)],
    duration_secs: u32,
    seam: SpliceSeam,
    scene_bgm: &str,
    use_voice_audio_ref: bool,
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
    let has_dialogue = shot_has_spoken_dialogue(shot);
    let has_identity_refs = ref_pairs.iter().any(|(p, _)| is_portrait_ref_path(p));
    let resume_bound = seam == SpliceSeam::SameTake
        && ref_pairs
            .first()
            .is_some_and(|(p, _)| is_last_frame_ref_path(p));

    let mut parts: Vec<String> = Vec::new();
    parts.push(constraint_line(cjk, duration_secs, aspect_ratio, style));
    let ref_clause = video_at_image_bindings(ref_pairs, seam, cjk);
    if !ref_clause.is_empty() {
        parts.push(ref_clause);
    }
    let audio_ref_clause = audio_ref_binding_clause(audio_bound_speakers, use_voice_audio_ref);
    if !audio_ref_clause.is_empty() {
        parts.push(audio_ref_clause);
    }
    if has_dialogue {
        let lock = character_voice_lock_clause(
            characters,
            &speaker_idxs_for_shot(shot, characters),
            audio_bound_speakers,
        );
        if !lock.is_empty() {
            parts.push(lock);
        }
    }
    let join = seam_line(seam, cjk, resume_bound);
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
    let shot_lbl = if cjk { "镜头" } else { "Shot " };
    let inner_cut = if cjk {
        "切到新机位。"
    } else {
        "Cut to a new camera."
    };

    let mut bgm_emitted = false;
    for (i, lens) in lenses.iter().enumerate() {
        let mut block = String::new();
        if lenses.len() > 1 {
            block.push_str(&format!("{}{}", shot_lbl, i + 1));
            block.push('\n');
        }
        if lens.cut {
            block.push_str(inner_cut);
            block.push('\n');
        }
        if !lens.visual.trim().is_empty() {
            block.push_str(scene_lbl);
            block.push_str(lens.visual.trim());
            block.push('\n');
        }
        let emit_bgm = !use_voice_audio_ref && !bgm_emitted;
        let (line, sfx) = render_lens_audio(
            lens.audio.as_deref(),
            &lens.visual,
            &lens.visual,
            scene_bgm,
            use_voice_audio_ref,
            emit_bgm,
        );
        if !line.is_empty() {
            block.push_str(line_lbl);
            block.push('{');
            block.push_str(&line);
            block.push('}');
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
    }
    parts.join("\n")
}

fn audio_ref_binding_clause(bound: &[&str], use_voice_audio_ref: bool) -> String {
    if !use_voice_audio_ref {
        return String::new();
    }
    let mut bits = Vec::new();
    if bound.is_empty() {
        bits.push(
            "@Audio1 is the voice timbre bible for the speaking character — match speaker identity for dialogue exactly."
                .to_string(),
        );
    } else {
        for (i, name) in bound.iter().enumerate() {
            bits.push(format!(
                "@Audio{} is the voice timbre bible for {name} — match speaker identity for dialogue exactly",
                i + 1
            ));
        }
    }
    bits.push(
        "No background music — dialogue and essential on-screen foley only.".into(),
    );
    bits.join(". ")
}

/// Up to 3 Seedance `reference_audio` slots, speakers in audio first then vis.
fn shot_speaker_voice_refs(
    shot: &ShotDescription,
    characters: &[CharacterInScene],
    registry: &HashMap<String, HashMap<String, HashMap<String, String>>>,
    film_root: &Path,
) -> Vec<(String, PathBuf)> {
    let idxs = speaker_idxs_for_shot(shot, characters);
    let mut out = Vec::new();
    let mut push = |ch: &CharacterInScene| {
        if out.len() >= 3 {
            return;
        }
        if out.iter().any(|(n, _)| n == &ch.identifier_in_scene) {
            return;
        }
        if let Some(p) = voice_ref_abs_path(registry, &ch.identifier_in_scene, film_root) {
            out.push((ch.identifier_in_scene.clone(), p));
        }
    };
    for &ci in &idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            push(ch);
        }
    }
    out
}

/// Dialogue + essential foley only — no BGM, no text voice-color locks (reference_audio carries timbre).
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
    if let Some(quote_at) = find_dialogue_quote_byte(rest) {
        consider(&mut resume, speaker_prefix_start(rest, quote_at));
    }
    resume
}

fn find_dialogue_quote_byte(s: &str) -> Option<usize> {
    s.find(['「', '{', '“', '"'])
}

/// Include `萧彻:` / `李薇说：` immediately before a quote so the speaker tag
/// is not left inside the discarded BGM span. Stop at punctuation/whitespace
/// so leading SFX is not swallowed into the name.
fn speaker_prefix_start(s: &str, quote_byte: usize) -> usize {
    let Some(prefix) = s.get(..quote_byte) else {
        return 0;
    };
    let chars: Vec<(usize, char)> = prefix.char_indices().collect();
    let mut i = chars.len();
    while i > 0 && chars[i - 1].1.is_whitespace() {
        i -= 1;
    }
    if i == 0 {
        return quote_byte;
    }
    let last = chars[i - 1].1;
    let tagged = if matches!(last, ':' | '：') {
        i -= 1;
        true
    } else if is_say_verb_char(last) {
        i -= 1;
        if last == '道' && i > 0 && chars[i - 1].1 == '说' {
            i -= 1;
        }
        true
    } else {
        false
    };
    if !tagged {
        return quote_byte;
    }
    while i > 0 && chars[i - 1].1.is_whitespace() {
        i -= 1;
    }
    let mut name_chars = 0u32;
    while i > 0 && is_speaker_name_char(chars[i - 1].1) && name_chars < 12 {
        i -= 1;
        name_chars += 1;
    }
    if name_chars == 0 {
        return quote_byte;
    }
    chars[i].0
}

fn is_say_verb_char(ch: char) -> bool {
    matches!(ch, '说' | '道' | '喊' | '叫' | '吼')
}

fn is_speaker_name_char(ch: char) -> bool {
    matches!(ch, '<' | '>' | '《' | '》' | '·' | '-' | '_')
        || ch.is_ascii_alphanumeric()
        || crate::planning::is_cjk_speech_char(ch)
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

/// Later shots in the same scene always bind the previous clip's last frame
/// (set/look lock). Freeze-frame at the join is handled by concat head-trim,
/// not by omitting this still.
fn continuity_still_for_seam(_seam: SpliceSeam, still: Option<&Path>) -> Option<&Path> {
    still
}

/// Multi-ref strip for Seedance R2V: SameTake last-frame (if any) + in-shot
/// portraits + every plot-mentioned prop that fits the model's 9-image budget.
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
            "Previous timeline-adjacent shot ending frame — continuity still (identity, wardrobe, set, lighting, L-R geography). Role depends on seam: same-camera resume vs match-cut state only."
                .into(),
        ));
    }

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
    let mut world = rank_world_pairs_for_frame(
        &world_query,
        world_pairs,
        MAX_SEEDANCE_REF_IMAGES,
    );
    if continuity.is_some() {
        // Last frame already contains the set; an empty env plate fights continuity.
        world.retain(|(p, _)| !is_environment_ref_path(p));
    }
    world.retain(|(p, t)| {
        if is_prop_ref_path(p) {
            query_mentions_prop(&world_query, p, t)
                && !prop_duplicates_visible_cast(p, t, characters, &vis)
        } else {
            true
        }
    });
    for (p, t) in mentioned_prop_pairs(&world_query, world_pairs) {
        if prop_duplicates_visible_cast(&p, &t, characters, &vis) {
            continue;
        }
        if world.iter().any(|(have, _)| have == &p) {
            continue;
        }
        world.push((p, t));
    }
    pairs.extend(world);

    // Dedup by path while preserving order (continuity first).
    let mut seen = std::collections::HashSet::new();
    pairs.retain(|(p, _)| seen.insert(p.clone()));
    pick_video_assets(pairs, continuity)
}

/// Prefer characters who actually speak in this shot's audio, then visible / named cast.
fn speaker_idxs_for_shot(shot: &ShotDescription, characters: &[CharacterInScene]) -> Vec<i32> {
    let audio = shot_audio_source(shot);
    let mut idxs: Vec<i32> = Vec::new();
    let mut push = |idx: i32| {
        if !idxs.contains(&idx) {
            idxs.push(idx);
        }
    };
    for ch in characters {
        let name = ch.identifier_in_scene.trim();
        if name.chars().count() >= 2 && audio.contains(name) {
            push(ch.idx);
        }
    }
    for idx in shot_cast_idxs(shot, characters) {
        push(idx);
    }
    idxs
}

/// Compact VOICE LOCK so Seedance keeps the same speaker timbre across shots.
///
/// `skip_identifiers` are characters already bound as `@AudioN`. Including them
/// in the text lock fights the wav; omitting *other* speakers is what made
/// multi-cast clips share one timbre.
fn character_voice_lock_clause(
    characters: &[CharacterInScene],
    idxs: &[i32],
    skip_identifiers: &[&str],
) -> String {
    let mut parts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let skip = |name: &str| skip_identifiers.iter().any(|n| *n == name);

    let mut push_ch = |ch: &CharacterInScene| {
        if skip(&ch.identifier_in_scene) || parts.len() >= 4 || !seen.insert(ch.idx) {
            return;
        }
        if let Some(vp) = ch.voice_profile.as_ref().filter(|v| v.is_usable()) {
            parts.push(vp.compact_lock(&ch.identifier_in_scene));
        }
    };

    for &ci in idxs {
        if let Some(ch) = characters.iter().find(|c| c.idx == ci) {
            push_ch(ch);
        }
    }
    // Unlisted-cast fallback only when no wav is carrying a primary speaker —
    // otherwise a bound AudioN character would be re-injected as text.
    if skip_identifiers.is_empty() {
        for ch in characters {
            push_ch(ch);
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
fn format_storyboard_audio_caption(raw: &str) -> String {
    const LINE: &[&str] = &["台词:", "台词："];
    let has_line_marker = LINE.iter().any(|m| find_case_insensitive(raw, m).is_some());
    let already_typed = (raw.contains('{') || raw.contains('<')) && !has_line_marker;
    if already_typed {
        return raw.trim().to_string();
    }
    let (line, sfx) = split_dialogue_and_sfx(raw);
    let line = trim_audio_brackets(&line);
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
        (None, None) => {
            if crate::planning::text_looks_like_dialogue(raw) {
                split_unmarked_dialogue_and_sfx(raw)
            } else {
                (String::new(), raw.trim().to_string())
            }
        }
    }
}

/// Unmarked `音效… 角色:「台词」` → keep the spoken span in `{…}` and the
/// leading foley in `<…>`, so Seedance does not try to vocalize ambience.
fn split_unmarked_dialogue_and_sfx(raw: &str) -> (String, String) {
    let Some(quote_at) = find_dialogue_quote_byte(raw) else {
        return (raw.trim().to_string(), String::new());
    };
    let line_start = speaker_prefix_start(raw, quote_at);
    let line = raw[line_start..].trim().to_string();
    let sfx = raw[..line_start]
        .trim()
        .trim_end_matches(['；', ';', '，', ',', '。', '.'])
        .trim()
        .to_string();
    (line, sfx)
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
    use crate::domain::VoiceProfile;

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
        assert!(prompt.contains("接着演"), "{prompt}");
        assert!(prompt.contains("@Image2"));
        assert!(prompt.contains("<林铮> 身份"), "{prompt}");
        assert!(prompt.contains("画面："), "{prompt}");
        assert!(prompt.contains("左方女性从防御起身"), "{prompt}");
        assert!(!prompt.contains("16:9"), "{prompt}");
        assert_not_english_runbook(&prompt);
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
        assert!(same.contains("@Image1 上一镜接着演"), "{same}");
        assert!(!same.contains("切到新机位"), "{same}");
        assert_not_english_runbook(&same);
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
        assert!(prompt.contains("@Image1 上一镜"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(prompt.contains("切到新机位，动作已经在进行"), "{prompt}");
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
        assert!(prompt.contains("切到新机位，动作已经在进行"), "{prompt}");
        assert!(prompt.contains("@Image1 <林铮> 身份"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(!prompt.contains("video_last_frame"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    /// Without a continuity still there is no seam to describe; the prompt must
    /// not reference an `@Image1` role that was never bound.
    #[test]
    fn a_hard_cut_prompt_has_no_opening_seam_clause() {
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
        assert!(!prompt.contains("Opening:"), "{prompt}");
        assert!(!prompt.contains("切到新机位"), "{prompt}");
        assert!(!prompt.contains("接着演"), "{prompt}");
        assert!(prompt.contains("@Image1 <林铮> 身份"), "{prompt}");
        assert_not_english_runbook(&prompt);
    }

    /// A clip that absorbed a same-camera neighbour lists each beat as its own
    /// lens — never a second-window clock.
    #[test]
    fn a_merged_clip_prompt_lists_each_lens() {
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
            13,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("镜头1"), "{prompt}");
        assert!(prompt.contains("镜头2"), "{prompt}");
        assert!(prompt.contains("她转身"), "{prompt}");
        assert!(prompt.contains("她走近窗边"), "{prompt}");
        assert_not_english_runbook(&prompt);
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
            10,
            SpliceSeam::Cut,
            "",
            false,
            &[],
            "",
        );
        assert!(prompt.contains("切到新机位"), "{prompt}");
        assert!(prompt.contains("镜头1"), "{prompt}");
        assert!(prompt.contains("镜头2"), "{prompt}");
        assert!(prompt.contains("她说话"), "{prompt}");
        assert!(prompt.contains("他回答"), "{prompt}");
        assert_not_english_runbook(&prompt);
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
        assert!(prompt.contains("镜头1"), "{prompt}");
        assert!(prompt.contains("镜头2"), "{prompt}");
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
        assert!(bindings.contains("@Image2 上一镜"), "{bindings}");
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
    fn audio_ref_mode_skips_voice_lock_and_bgm() {
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
        assert!(prompt.contains("@Audio1"), "{prompt}");
        let lock_line = prompt
            .lines()
            .find(|line| line.contains("VOICE LOCK"))
            .expect("other speakers still need a text bible");
        assert!(lock_line.contains("阿琳"), "{lock_line}");
        assert!(
            !lock_line.contains("李薇"),
            "the @Audio1 speaker must not also be text-locked: {lock_line}"
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
        let out = pick_video_assets(pairs, Some(&cont));
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
            "continuity already carries the set: {blob}"
        );
        assert_eq!(out[0].0, cont);
    }

    #[test]
    fn later_shot_keeps_the_last_frame_on_the_strip() {
        let still = PathBuf::from("shots/0/video_last_frame.png");
        assert_eq!(
            continuity_still_for_seam(SpliceSeam::MatchCut, Some(&still)),
            Some(still.as_path())
        );
        assert_eq!(
            continuity_still_for_seam(SpliceSeam::Cut, Some(&still)),
            Some(still.as_path())
        );
        assert_eq!(
            continuity_still_for_seam(SpliceSeam::SameTake, Some(&still)),
            Some(still.as_path())
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
        let out = pick_video_assets(pairs, Some(&still));
        assert_eq!(out[0].0, still);
        assert!(
            !out.iter()
                .any(|(p, _)| p.to_string_lossy().contains("environments")),
            "last-frame already carries the set: {out:?}"
        );
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
        let out = pick_video_assets(pairs, Some(&cont));
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
        let out = pick_video_assets(pairs, Some(&cont));
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
        let out = pick_video_assets(pairs, Some(&cont));
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
        assert_eq!(out[0].0, cont);
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
        let clips = super::super::clip_beats::shots_from_packed_briefs(&disk);
        assert!(super::super::clip_beats::clips_follow_board(&disk, &clips));
        assert_eq!(clips.len(), disk.len());
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
