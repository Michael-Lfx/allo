//! Public ViMax service API used by `nomifun-vimax` HTTP routes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use futures::FutureExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::agents::{ensure_cover_from_final_video, COVER_FILENAME};
use crate::backends::FlowyVimaxServices;
use crate::domain::WorkflowKind;
use crate::error::{VimaxError, VimaxResult};
use crate::media_local;
use crate::pipelines::{
    model_supports_action_imitation, Action2VideoPipeline, Idea2VideoPipeline, Novel2VideoPipeline,
    PipelineBackends, ScriptFilmPipeline,
};
use crate::progress::{INTERRUPTED_SUMMARY, RenderStatus, RunStatus};
use crate::session::{
    ArtifactNode, CameoPhotoEntry, CameoUpdate, SessionIndex, SessionRecord, SessionSummary,
    apply_status_to_record, apply_video_task_credits, cameo, video_task_credit_delta,
};
use crate::skills::{SkillCatalog, VerticalSkillDraft, VerticalSkillSummary};

fn first_nonempty<'a>(candidates: impl IntoIterator<Item = Option<&'a str>>) -> String {
    for c in candidates {
        if let Some(s) = c {
            if !s.trim().is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

fn live_status<'a>(
    index: &SessionIndex,
    map: &'a mut HashMap<String, RenderStatus>,
    id: &str,
) -> &'a mut RenderStatus {
    map.entry(id.to_string())
        .or_insert_with(|| index.load_run_status(id).unwrap_or_default())
}

fn persist_run_status(index: &SessionIndex, id: &str, st: &RenderStatus) {
    if let Err(e) = index.save_run_status(id, st) {
        tracing::warn!(
            session_id = %id,
            error = %e,
            "failed to persist vimax run status"
        );
    }
}

fn overlay_session_record(
    out: &mut RenderStatus,
    record: &SessionRecord,
    working_abs: Option<String>,
) {
    out.working_dir_abs = working_abs.or(out.working_dir_abs.take());
    if out.cover.is_none() {
        out.cover = record.cover.clone();
    }
    if out.final_video.is_none() {
        out.final_video = record.final_video.clone();
    }
    if record.credits_consumed > out.credits_consumed {
        out.credits_consumed = record.credits_consumed;
    }
}

/// Prefer `Interrupted` when shutdown already paused the session; else user cancel.
fn mark_cancelled_or_interrupted(st: &mut RenderStatus, index: &SessionIndex, id: &str) {
    let already_interrupted = index
        .get(id)
        .ok()
        .is_some_and(|r| r.status == RunStatus::Interrupted)
        || st.status == RunStatus::Interrupted;
    if already_interrupted {
        st.status = RunStatus::Interrupted;
        if st.message.is_empty() {
            st.message = INTERRUPTED_SUMMARY.into();
        }
        st.emit_terminal("interrupted", &st.message.clone());
    } else {
        st.status = RunStatus::Cancelled;
        st.message = "cancelled".into();
        st.emit_terminal("cancelled", "cancelled");
    }
}

#[derive(Clone, Copy)]
enum JobKind {
    Plan,
    Render,
}

pub struct VimaxService {
    #[allow(dead_code)]
    data_dir: PathBuf,
    index: SessionIndex,
    skills: SkillCatalog,
    flowy: Mutex<Option<FlowyVimaxServices>>,
    /// Sync mutex so progress callbacks never drop updates via `try_lock`.
    statuses: StdMutex<HashMap<String, RenderStatus>>,
    cancels: Mutex<HashMap<String, CancellationToken>>,
}

impl VimaxService {
    pub fn start(data_dir: &Path, flowy: Option<FlowyVimaxServices>) -> VimaxResult<Arc<Self>> {
        let index = SessionIndex::open(data_dir)?;
        match index.reconcile_orphaned_active_runs() {
            Ok(n) if n > 0 => {
                tracing::info!(
                    count = n,
                    "reconciled orphaned vimax runs left active after previous exit"
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "failed to reconcile orphaned vimax runs on start"
                );
            }
        }
        Ok(Arc::new(Self {
            data_dir: data_dir.to_path_buf(),
            index,
            skills: SkillCatalog::open(data_dir)?,
            flowy: Mutex::new(flowy),
            statuses: StdMutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
        }))
    }

    /// Cancel in-flight jobs and persist interrupted state (app quit / crash path).
    /// Preserves each session's pipeline `stage` so resume can pick plan vs render.
    pub async fn interrupt_all(&self) -> usize {
        use crate::progress::INTERRUPTED_SUMMARY;
        use std::collections::HashSet;

        let tokens: Vec<CancellationToken> = {
            let mut map = self.cancels.lock().await;
            map.drain().map(|(_, t)| t).collect()
        };
        for token in &tokens {
            token.cancel();
        }

        let mut ids: HashSet<String> = HashSet::new();
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            for (id, st) in map.iter_mut() {
                if !st.status.is_active() {
                    continue;
                }
                st.status = RunStatus::Interrupted;
                st.message = INTERRUPTED_SUMMARY.into();
                st.error = None;
                st.emit_terminal("interrupted", INTERRUPTED_SUMMARY);
                persist_run_status(&self.index, id, st);
                ids.insert(id.clone());
            }
        }

        if let Ok(sessions) = self.index.list() {
            for record in sessions {
                if record.status.is_active() {
                    ids.insert(record.session_id);
                }
            }
        }

        for id in &ids {
            let _ = self.index.update_fields(id, |r| {
                r.status = RunStatus::Interrupted;
                r.summary = INTERRUPTED_SUMMARY.into();
            });
        }

        if !ids.is_empty() || !tokens.is_empty() {
            tracing::info!(
                cancelled_tokens = tokens.len(),
                interrupted = ids.len(),
                "interrupted active vimax runs for process shutdown"
            );
        }
        ids.len()
    }

    /// Replace Flowy backends after login / config reload.
    pub async fn set_flowy(&self, flowy: Option<FlowyVimaxServices>) {
        *self.flowy.lock().await = flowy;
    }

    pub fn list_sessions(&self) -> VimaxResult<Vec<SessionRecord>> {
        self.index.list()
    }

    pub fn list_session_summaries(&self) -> VimaxResult<Vec<SessionSummary>> {
        self.index.list_summaries()
    }

    pub fn create_session(
        &self,
        workflow: WorkflowKind,
        title: Option<String>,
    ) -> VimaxResult<SessionRecord> {
        self.index.create(workflow, title)
    }

    pub fn get_session(&self, id: &str) -> VimaxResult<SessionRecord> {
        self.index.get(id)
    }

    pub fn list_vertical_skills(
        &self,
        mode: Option<WorkflowKind>,
        source: Option<crate::skills::SkillSource>,
    ) -> VimaxResult<Vec<VerticalSkillSummary>> {
        self.skills.list(mode, source)
    }

    pub fn get_vertical_skill(
        &self,
        id: &str,
    ) -> VimaxResult<(crate::skills::VerticalSkill, String)> {
        let skill_id = crate::skills::SkillId::parse(id)
            .ok_or_else(|| VimaxError::InvalidParams(format!("invalid skill id: {id}")))?;
        let skill = self.skills.get(&skill_id)?;
        let manifest = self.skills.read_manifest(&skill_id)?;
        Ok((skill, manifest))
    }

    pub fn create_vertical_skill(
        &self,
        draft: VerticalSkillDraft,
    ) -> VimaxResult<crate::skills::VerticalSkill> {
        self.skills.create_user_skill(&draft)
    }

    pub fn update_vertical_skill(
        &self,
        name: &str,
        draft: VerticalSkillDraft,
    ) -> VimaxResult<crate::skills::VerticalSkill> {
        self.skills.update_user_skill(name, &draft)
    }

    pub fn delete_vertical_skill(&self, name: &str) -> VimaxResult<()> {
        self.skills.delete_user_skill(name)
    }

    pub fn publish_vertical_skill(
        &self,
        name: &str,
    ) -> VimaxResult<crate::skills::VerticalSkill> {
        self.skills.publish_user_skill(name)
    }

    pub fn unpublish_vertical_skill(&self, name: &str) -> VimaxResult<()> {
        self.skills.unpublish_skill(name)
    }

    pub fn import_vertical_skill(
        &self,
        path: &Path,
    ) -> VimaxResult<crate::skills::VerticalSkill> {
        self.skills.import_skill_dir(path)
    }

    pub fn import_vertical_skill_package(
        &self,
        path: &Path,
        cloud_id: Option<i64>,
        cloud_version: Option<&str>,
    ) -> VimaxResult<crate::skills::VerticalSkill> {
        self.skills
            .import_skill_package(path, cloud_id, cloud_version)
    }

    pub fn vertical_skill_dir(&self, id: &str) -> VimaxResult<PathBuf> {
        let skill_id = crate::skills::SkillId::parse(id)
            .ok_or_else(|| VimaxError::InvalidParams(format!("invalid skill id: {id}")))?;
        self.skills.skill_dir(&skill_id)
    }

    pub async fn status(&self, id: &str) -> VimaxResult<RenderStatus> {
        let record = self.index.get(id)?;
        let working_abs = self
            .index
            .working_dir(id)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let mut map = self
            .statuses
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let st = live_status(&self.index, &mut map, id);
        overlay_session_record(st, &record, working_abs.clone());
        if st.stage.is_empty() {
            st.stage = record.stage.clone();
        }
        if st.message.is_empty() {
            st.message = record.summary.clone();
        }
        if st.status == RunStatus::Idle && record.status != RunStatus::Idle {
            st.status = record.status;
        }
        if !record.status.is_active() && st.status.is_active() {
            st.status = record.status;
            if st.message.is_empty() {
                st.message = record.summary.clone();
            }
            let terminal = record.status.as_str();
            if !st.events.iter().any(|e| e.stage == terminal) {
                st.emit_terminal(terminal, &st.message.clone());
            }
            persist_run_status(&self.index, id, st);
        }
        if st.events.is_empty() && st.status == RunStatus::Idle {
            // Keep disk-less completed sessions honest about stage/credits.
            st.status = record.status;
            st.stage = record.stage.clone();
            st.message = record.summary.clone();
            st.updated_at = record.updated_at.clone();
        }
        Ok(st.clone())
    }

    pub async fn cancel(self: &Arc<Self>, id: &str) -> VimaxResult<()> {
        if let Some(token) = self.cancels.lock().await.get(id) {
            token.cancel();
        }
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let status = live_status(&self.index, &mut map, id);
            status.status = RunStatus::Cancelled;
            status.message = "cancelled".into();
            // Keep the last working pipeline stage so "continue from checkpoint"
            // can resume plan vs render correctly.
            status.emit_terminal("cancelled", "cancelled");
            persist_run_status(&self.index, id, status);
        }
        let _ = self.index.update_fields(id, |r| {
            r.status = RunStatus::Cancelled;
            r.summary = "cancelled".into();
        });
        Ok(())
    }

    /// Cancel any in-flight work, drop runtime state, and remove session artifacts.
    pub async fn delete_session(self: &Arc<Self>, id: &str) -> VimaxResult<()> {
        // Best-effort cancel so a running plan/render stops ASAP.
        if let Some(token) = self.cancels.lock().await.remove(id) {
            token.cancel();
        }
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            map.remove(id);
        }
        self.index.delete(id)
    }

    pub fn list_artifacts(&self, id: &str) -> VimaxResult<Vec<ArtifactNode>> {
        self.index.list_artifacts(id)
    }

    pub fn artifact_path(&self, id: &str, rel: &str) -> VimaxResult<PathBuf> {
        self.index.artifact_abs_path(id, rel)
    }

    /// Absolute session working directory (`.working_dir/<id>`).
    pub fn working_dir(&self, id: &str) -> VimaxResult<PathBuf> {
        self.index.working_dir(id)
    }

    /// Update `final_video` after an external edit (e.g. Canvas write-back).
    pub fn set_final_video(
        &self,
        id: &str,
        final_video: Option<String>,
    ) -> VimaxResult<SessionRecord> {
        self.index.update_fields(id, |record| {
            record.final_video = final_video;
        })
    }

    /// List Cameo photos for a session (scrubs orphan entries).
    pub fn list_cameos(&self, id: &str) -> VimaxResult<Vec<CameoPhotoEntry>> {
        let working = self.index.working_dir(id)?;
        cameo::list_photos(&working)
    }

    /// Upload and normalize a Cameo photo (PNG/JPEG/WEBP → PNG).
    pub async fn upload_cameo(
        self: &Arc<Self>,
        id: &str,
        bytes: Vec<u8>,
        character_name: String,
        description: String,
    ) -> VimaxResult<CameoPhotoEntry> {
        self.ensure_cameo_mutable(id).await?;
        let working = self.index.working_dir(id)?;
        tokio::task::spawn_blocking(move || {
            cameo::upload_photo(&working, &bytes, &character_name, &description)
        })
        .await
        .map_err(|e| VimaxError::msg(format!("cameo upload join error: {e}")))?
    }

    pub async fn update_cameo(
        self: &Arc<Self>,
        id: &str,
        photo_id: &str,
        update: CameoUpdate,
    ) -> VimaxResult<CameoPhotoEntry> {
        self.ensure_cameo_mutable(id).await?;
        let working = self.index.working_dir(id)?;
        let photo_id = photo_id.to_string();
        tokio::task::spawn_blocking(move || cameo::update_photo(&working, &photo_id, update))
            .await
            .map_err(|e| VimaxError::msg(format!("cameo update join error: {e}")))?
    }

    pub async fn delete_cameo(self: &Arc<Self>, id: &str, photo_id: &str) -> VimaxResult<()> {
        self.ensure_cameo_mutable(id).await?;
        let working = self.index.working_dir(id)?;
        let photo_id = photo_id.to_string();
        tokio::task::spawn_blocking(move || cameo::delete_photo(&working, &photo_id))
            .await
            .map_err(|e| VimaxError::msg(format!("cameo delete join error: {e}")))?
    }

    pub fn cameo_photo_path(&self, id: &str, photo_id: &str) -> VimaxResult<PathBuf> {
        let working = self.index.working_dir(id)?;
        cameo::photo_abs_path(&working, photo_id)
    }

    pub fn list_action_assets(&self, id: &str) -> VimaxResult<crate::session::ActionAssetsInfo> {
        let record = self.index.get(id)?;
        if !record.workflow.is_action_imitation() {
            return Err(VimaxError::InvalidParams(
                "session is not an action imitation project".into(),
            ));
        }
        let work = self
            .index
            .working_dir(id)?
            .join(record.workflow.artifact_root());
        let root = self.index.working_dir(id)?;
        let relativize = |abs: PathBuf| {
            abs.strip_prefix(&root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| abs.to_string_lossy().replace('\\', "/"))
        };
        Ok(crate::session::ActionAssetsInfo {
            character: crate::session::action_assets::character_abs(&work).map(relativize),
            reference_video: crate::session::action_assets::reference_abs(&work).map(relativize),
        })
    }

    pub async fn upload_action_assets(
        self: &Arc<Self>,
        id: &str,
        character: Option<Vec<u8>>,
        video: Option<Vec<u8>>,
    ) -> VimaxResult<crate::session::ActionAssetsInfo> {
        if character.is_none() && video.is_none() {
            return Err(VimaxError::InvalidParams(
                "upload a character image and/or a reference video".into(),
            ));
        }
        self.ensure_cameo_mutable(id).await?;
        let record = self.index.get(id)?;
        if !record.workflow.is_action_imitation() {
            return Err(VimaxError::InvalidParams(
                "session is not an action imitation project".into(),
            ));
        }
        let work = self
            .index
            .working_dir(id)?
            .join(record.workflow.artifact_root());
        tokio::fs::create_dir_all(&work).await?;
        if let Some(bytes) = character {
            crate::session::action_assets::save_character(&work, &bytes).await?;
        }
        if let Some(bytes) = video {
            crate::session::action_assets::save_reference_video(&work, &bytes).await?;
        }
        self.list_action_assets(id)
    }

    async fn ensure_cameo_mutable(&self, id: &str) -> VimaxResult<()> {
        let record = self.index.get(id)?;
        if matches!(record.status, RunStatus::Planning | RunStatus::Rendering) {
            return Err(VimaxError::InvalidParams(
                "cannot modify session inputs while the project is planning or rendering".into(),
            ));
        }
        let map = self.statuses.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = map.get(id)
            && matches!(s.status, RunStatus::Planning | RunStatus::Rendering)
        {
            return Err(VimaxError::InvalidParams(
                "cannot modify session inputs while the project is planning or rendering".into(),
            ));
        }
        Ok(())
    }

    /// Export a finished (non-running) session to a `.nomivimax` archive on disk.
    pub async fn export_session(
        self: &Arc<Self>,
        id: &str,
        dest_path: impl AsRef<Path>,
    ) -> VimaxResult<PathBuf> {
        self.ensure_not_busy(id).await?;
        let dest = dest_path.as_ref().to_path_buf();
        let index = self.index.clone();
        let session_id = id.to_string();
        tokio::task::spawn_blocking(move || index.export_to_path(&session_id, &dest))
            .await
            .map_err(|e| VimaxError::msg(format!("export join error: {e}")))?
    }

    /// Import a `.nomivimax` archive as a new session (new id; all assets preserved).
    pub async fn import_session(
        self: &Arc<Self>,
        archive_path: impl AsRef<Path>,
    ) -> VimaxResult<SessionRecord> {
        let path = archive_path.as_ref().to_path_buf();
        let index = self.index.clone();
        tokio::task::spawn_blocking(move || index.import_from_path(&path))
            .await
            .map_err(|e| VimaxError::msg(format!("import join error: {e}")))?
    }

    async fn ensure_not_busy(&self, id: &str) -> VimaxResult<()> {
        let record = self.index.get(id)?;
        if matches!(record.status, RunStatus::Planning | RunStatus::Rendering) {
            return Err(VimaxError::InvalidParams(
                "cannot export while the project is still planning or rendering".into(),
            ));
        }
        let map = self.statuses.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = map.get(id) {
            if matches!(s.status, RunStatus::Planning | RunStatus::Rendering) {
                return Err(VimaxError::InvalidParams(
                    "cannot export while the project is still planning or rendering".into(),
                ));
            }
        }
        Ok(())
    }

    pub async fn plan(
        self: &Arc<Self>,
        id: &str,
        idea: Option<String>,
        script: Option<String>,
        novel_text: Option<String>,
        user_requirement: Option<String>,
        style: Option<String>,
        vertical_skill_ids: Option<Vec<String>>,
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        target_duration_secs: Option<u32>,
        aspect_ratio: Option<String>,
        resolution: Option<String>,
        fps: Option<u32>,
    ) -> VimaxResult<()> {
        self.ensure_idle(id).await?;
        let token = CancellationToken::new();
        self.cancels
            .lock()
            .await
            .insert(id.to_string(), token.clone());
        self.set_run_status(id, RunStatus::Planning, "planning").await?;

        let svc = Arc::clone(self);
        let id = id.to_string();
        tokio::spawn(async move {
            // Attribute every Flowy LLM / image / video call in this plan run to the
            // session id so GET /credits/usageByTurn can aggregate the full flow.
            let result = nomi_providers::with_flowy_billing_turn_id(id.clone(), async {
                match std::panic::AssertUnwindSafe(
                    svc.run_plan(
                        &id,
                        idea,
                        script,
                        novel_text,
                        user_requirement,
                        style,
                        vertical_skill_ids,
                        llm_model,
                        image_model,
                        video_model,
                        target_duration_secs,
                        aspect_ratio,
                        resolution,
                        fps,
                        token.clone(),
                    ),
                )
                .catch_unwind()
                .await
                {
                    Ok(r) => r,
                    Err(payload) => Err(VimaxError::from_panic_payload("planning task", payload)),
                }
            })
            .await;
            svc.finish_job(&id, result, &token, JobKind::Plan).await;
        });
        Ok(())
    }

    pub async fn render(
        self: &Arc<Self>,
        id: &str,
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        resolution: Option<String>,
        fps: Option<u32>,
    ) -> VimaxResult<()> {
        if llm_model.is_some()
            || image_model.is_some()
            || video_model.is_some()
            || resolution.is_some()
            || fps.is_some()
        {
            let _ = self.index.update_fields(id, |r| {
                if let Some(v) = &llm_model {
                    r.llm_model = v.trim().to_string();
                }
                if let Some(v) = &image_model {
                    r.image_model = v.trim().to_string();
                }
                if let Some(v) = &video_model {
                    r.video_model = v.trim().to_string();
                }
                if let Some(v) = &resolution {
                    // Keep model-canonical casing (MiniMax-H3 uses `768P` / `2K`).
                    r.resolution = v.trim().to_string();
                }
                if let Some(v) = fps {
                    r.fps = v;
                }
            })?;
        }
        // Clamp quality against the (possibly updated) video model allow-list.
        {
            let record = self.index.get(id)?;
            let guard = self.flowy.lock().await;
            if let Some(flowy) = guard.as_ref() {
                let res = resolve_resolution_for_session(&record, &flowy.media);
                let fps_n = resolve_fps_for_session(&record, &flowy.media);
                if record.resolution != res || record.fps != fps_n {
                    let _ = self.index.update_fields(id, |r| {
                        r.resolution = res;
                        r.fps = fps_n;
                    })?;
                }
            }
        }
        self.ensure_idle(id).await?;
        let token = CancellationToken::new();
        self.cancels
            .lock()
            .await
            .insert(id.to_string(), token.clone());
        self.set_run_status(id, RunStatus::Rendering, "rendering")
            .await?;

        let svc = Arc::clone(self);
        let id = id.to_string();
        tokio::spawn(async move {
            // Same billing turn as plan: session UUID aggregates plan + render spend.
            let result = nomi_providers::with_flowy_billing_turn_id(id.clone(), async {
                match std::panic::AssertUnwindSafe(svc.run_render(&id, token.clone()))
                    .catch_unwind()
                    .await
                {
                    Ok(r) => r,
                    Err(payload) => Err(VimaxError::from_panic_payload("render task", payload)),
                }
            })
            .await;
            svc.finish_job(&id, result, &token, JobKind::Render).await;
        });
        Ok(())
    }

    pub async fn revise(
        self: &Arc<Self>,
        id: &str,
        revision_target: String,
        revision_instruction: String,
    ) -> VimaxResult<()> {
        self.ensure_artifacts_mutable(id).await?;
        let working = self.index.working_dir(id)?;
        let record = self.index.get(id)?;
        let backends = self.backends_for(&record, None).await?;
        let result = crate::revise::revise_artifact(
            &backends.chat,
            &working,
            &revision_target,
            &revision_instruction,
        )
        .await?;
        self.apply_revise_result(id, &result, "revised")?;
        Ok(())
    }

    /// Direct text/JSON overwrite of a session artifact (no LLM).
    pub async fn write_artifact_text(
        &self,
        id: &str,
        relative_path: &str,
        content: &str,
    ) -> VimaxResult<crate::revise::ReviseResult> {
        self.ensure_artifacts_mutable(id).await?;
        let working = self.index.working_dir(id)?;
        let result =
            crate::artifact_edit::write_text_artifact(&working, relative_path, content).await?;
        self.apply_revise_result(id, &result, "artifact_edited")?;
        Ok(result)
    }

    /// Replace an image artifact with uploaded bytes.
    pub async fn replace_artifact_file(
        &self,
        id: &str,
        relative_path: &str,
        bytes: Vec<u8>,
    ) -> VimaxResult<crate::revise::ReviseResult> {
        self.ensure_artifacts_mutable(id).await?;
        let working = self.index.working_dir(id)?;
        let result =
            crate::artifact_edit::replace_binary_artifact(&working, relative_path, &bytes).await?;
        self.apply_revise_result(id, &result, "artifact_replaced")?;
        Ok(result)
    }

    /// Load the editable image-generation prompt companion for a frame image.
    pub async fn get_artifact_image_prompt(
        &self,
        id: &str,
        image_path: &str,
    ) -> VimaxResult<crate::artifact_edit::ImagePromptInfo> {
        let working = self.index.working_dir(id)?;
        crate::artifact_edit::get_image_prompt(&working, image_path).await
    }

    /// Update frame prompt and drop the image so the next render regenerates it.
    pub async fn update_artifact_image_prompt(
        &self,
        id: &str,
        image_path: &str,
        prompt: &str,
    ) -> VimaxResult<crate::revise::ReviseResult> {
        self.ensure_artifacts_mutable(id).await?;
        let working = self.index.working_dir(id)?;
        let result =
            crate::artifact_edit::update_image_prompt(&working, image_path, prompt).await?;
        self.apply_revise_result(id, &result, "image_prompt_updated")?;
        Ok(result)
    }

    fn apply_revise_result(
        &self,
        id: &str,
        result: &crate::revise::ReviseResult,
        stage: &str,
    ) -> VimaxResult<()> {
        let summary = format!(
            "Updated {}; invalidated {} artifacts",
            result.revised_path,
            result.invalidated.len()
        );
        self.index.update_fields(id, |r| {
            r.stage = stage.into();
            r.summary = summary.clone();
            for key in &result.stale_keys {
                r.stale.insert(key.clone(), true);
            }
        })?;
        Ok(())
    }

    async fn ensure_artifacts_mutable(&self, id: &str) -> VimaxResult<()> {
        let record = self.index.get(id)?;
        if matches!(record.status, RunStatus::Planning | RunStatus::Rendering) {
            return Err(VimaxError::InvalidParams(
                "cannot edit artifacts while the project is planning or rendering".into(),
            ));
        }
        let map = self.statuses.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = map.get(id)
            && matches!(s.status, RunStatus::Planning | RunStatus::Rendering)
        {
            return Err(VimaxError::InvalidParams(
                "cannot edit artifacts while the project is planning or rendering".into(),
            ));
        }
        Ok(())
    }

    async fn ensure_idle(&self, id: &str) -> VimaxResult<()> {
        let _ = self.index.get(id)?;
        let map = self
            .statuses
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(s) = map.get(id)
            && matches!(s.status, RunStatus::Planning | RunStatus::Rendering)
        {
            return Err(VimaxError::InvalidParams(
                "session already has an active job".into(),
            ));
        }
        Ok(())
    }

    async fn set_run_status(&self, id: &str, status: RunStatus, message: &str) -> VimaxResult<()> {
        let prior_credits = self
            .index
            .get(id)
            .map(|r| r.credits_consumed)
            .unwrap_or(0);
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = live_status(&self.index, &mut map, id);
            st.status = status;
            st.stage = status.as_str().into();
            st.message = message.into();
            st.error = None;
            st.progress = 0.0;
            // Keep previously billed video credits visible across plan/render runs.
            if prior_credits > st.credits_consumed {
                st.credits_consumed = prior_credits;
            }
            st.emit(status.as_str(), message, None);
            persist_run_status(&self.index, id, st);
        }
        self.index.update_fields(id, |r| {
            r.status = status;
            r.stage = status.as_str().into();
            r.summary = message.into();
        })?;
        Ok(())
    }

    async fn finish_job(
        &self,
        id: &str,
        result: VimaxResult<()>,
        token: &CancellationToken,
        kind: JobKind,
    ) {
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = live_status(&self.index, &mut map, id);
            match result {
                Ok(()) => {
                    if token.is_cancelled() {
                        mark_cancelled_or_interrupted(st, &self.index, id);
                    } else {
                        match kind {
                            JobKind::Plan => {
                                // Plan done ≠ final video done — return to idle so UI can render.
                                st.status = RunStatus::Idle;
                                st.stage = "planned".into();
                                st.message = "Planning complete — ready to render".into();
                                st.progress = 100.0;
                                st.error = None;
                                st.emit("planned", "Planning complete — ready to render", None);
                            }
                            JobKind::Render => {
                                st.status = RunStatus::Succeeded;
                                if st.message.is_empty() {
                                    st.message = "render complete".into();
                                }
                                st.progress = 100.0;
                            }
                        }
                    }
                }
                Err(VimaxError::Cancelled) => {
                    mark_cancelled_or_interrupted(st, &self.index, id);
                }
                Err(e) => {
                    let detail = e.to_string();
                    let prev_stage = st.stage.clone();
                    let prev_message = st.message.clone();
                    st.status = RunStatus::Failed;
                    let composed = if prev_stage.is_empty() {
                        detail.clone()
                    } else {
                        format!(
                            "Failed at stage `{prev_stage}`\nPrevious status: {prev_message}\n\n{detail}"
                        )
                    };
                    st.error = Some(composed.clone());
                    st.message = composed.clone();
                    // Keep prev_stage on `st.stage` for resume routing.
                    st.emit_terminal("failed", &composed);
                }
            }
            let _ = self.index.update_fields(id, |r| {
                apply_status_to_record(r, st);
            });
            persist_run_status(&self.index, id, st);
        }
        self.cancels.lock().await.remove(id);
    }

    async fn backends_for(
        self: &Arc<Self>,
        record: &SessionRecord,
        cancel: Option<CancellationToken>,
    ) -> VimaxResult<PipelineBackends> {
        let guard = self.flowy.lock().await;
        let flowy = guard.as_ref().ok_or(VimaxError::NotAuthenticated)?;
        let llm = nonempty_opt(&record.llm_model);
        let image = nonempty_opt(&record.image_model);
        let video = nonempty_opt(&record.video_model);
        let aspect = resolve_aspect_for_session(record, &flowy.media);
        let resolution = resolve_resolution_for_session(record, &flowy.media);
        // Resolve from the configured id (not the catalog) so planning stays
        // synchronous; an unknown id falls back to the universally safe window.
        let clip = crate::video_quality::clip_bounds_for_model(
            video.as_deref().unwrap_or(flowy.media.video.model.trim()),
        );
        Ok(PipelineBackends {
            chat: Arc::new(flowy.chat_with_model(llm)),
            // Portraits / env plates use default Seedream 2K — do NOT bind video aspect here.
            image: Arc::new(flowy.image_with_model(image.clone())),
            // Fine-grained create / poll / download progress for the progress rail.
            video: Arc::new(flowy.video_with_session_quality(
                video,
                cancel.clone(),
                Some(aspect),
                Some(resolution),
                Some(progress_callback(Arc::clone(self), &record.session_id)),
            )),
            flowy: Some(flowy.clone()),
            image_model: image,
            clip,
            cancel,
        })
    }

    async fn run_plan(
        self: &Arc<Self>,
        id: &str,
        idea: Option<String>,
        script: Option<String>,
        novel_text: Option<String>,
        user_requirement: Option<String>,
        style: Option<String>,
        vertical_skill_ids: Option<Vec<String>>,
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        target_duration_secs: Option<u32>,
        aspect_ratio: Option<String>,
        resolution: Option<String>,
        fps: Option<u32>,
        token: CancellationToken,
    ) -> VimaxResult<()> {
        if token.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        let record = self.index.update_fields(id, |r| {
            if let Some(v) = &idea {
                r.idea = v.clone();
            }
            if let Some(v) = &script {
                r.script = v.clone();
            }
            if let Some(v) = &novel_text {
                r.novel_text = v.clone();
            }
            if let Some(v) = &user_requirement {
                r.user_requirement = v.clone();
            }
            if let Some(v) = &style {
                r.style = v.clone();
            }
            if let Some(ids) = &vertical_skill_ids {
                r.vertical_skill_ids = ids
                    .iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if let Some(v) = &llm_model {
                r.llm_model = v.trim().to_string();
            }
            if let Some(v) = &image_model {
                r.image_model = v.trim().to_string();
            }
            if let Some(v) = &video_model {
                r.video_model = v.trim().to_string();
            }
            if let Some(secs) = target_duration_secs {
                r.target_duration_secs = secs;
            }
            if let Some(ar) = &aspect_ratio {
                r.aspect_ratio = crate::aspect::normalize_aspect_ratio(ar);
            }
            if let Some(res) = &resolution {
                // Keep model-canonical casing (MiniMax-H3 uses `768P` / `2K`).
                r.resolution = res.trim().to_string();
            }
            if let Some(v) = fps {
                r.fps = v;
            }
        })?;

        // Resolve aspect (session → media default) before building backends so
        // video + cover image share the same Seedance ratio.
        let (aspect, resolution_norm, fps_norm) = {
            let guard = self.flowy.lock().await;
            let media = guard.as_ref().map(|f| &f.media);
            let media_default_aspect = media
                .map(|m| m.video.default_aspect_ratio.as_str())
                .unwrap_or(crate::aspect::DEFAULT_ASPECT_RATIO);
            let aspect = if record.aspect_ratio.trim().is_empty() {
                crate::aspect::normalize_aspect_ratio(media_default_aspect)
            } else {
                crate::aspect::normalize_aspect_ratio(&record.aspect_ratio)
            };
            let (resolution_norm, fps_norm) = if let Some(m) = media {
                (
                    resolve_resolution_for_session(&record, m),
                    resolve_fps_for_session(&record, m),
                )
            } else {
                let model = record.video_model.as_str();
                (
                    crate::video_quality::normalize_resolution_for_model(
                        model,
                        if record.resolution.trim().is_empty() {
                            crate::video_quality::DEFAULT_VIDEO_RESOLUTION
                        } else {
                            record.resolution.as_str()
                        },
                    ),
                    crate::video_quality::normalize_fps_for_model(
                        model,
                        if record.fps > 0 {
                            record.fps
                        } else {
                            crate::video_quality::DEFAULT_VIDEO_FPS
                        },
                    ),
                )
            };
            (aspect, resolution_norm, fps_norm)
        };
        let record = if record.aspect_ratio != aspect
            || record.resolution != resolution_norm
            || record.fps != fps_norm
        {
            self.index.update_fields(id, |r| {
                r.aspect_ratio = aspect.clone();
                r.resolution = resolution_norm.clone();
                r.fps = fps_norm;
            })?
        } else {
            record
        };
        ensure_action_imitation_video_model(&record)?;

        let backends = self.backends_for(&record, Some(token.clone())).await?;
        let work = self
            .index
            .working_dir(id)?
            .join(record.workflow.artifact_root());
        tokio::fs::create_dir_all(&work).await?;
        let target_secs = if record.target_duration_secs > 0 {
            Some(crate::planning::normalize_target_duration_secs(Some(
                record.target_duration_secs,
            )))
        } else {
            target_duration_secs
                .filter(|&s| s > 0)
                .map(|s| crate::planning::normalize_target_duration_secs(Some(s)))
        };
        // Persist so render / cover / child scenes share the same Seedance ratio.
        let prev_aspect_path = work.join("aspect_ratio.txt");
        let prev_aspect = tokio::fs::read_to_string(&prev_aspect_path)
            .await
            .ok()
            .map(|s| crate::aspect::normalize_aspect_ratio(&s));
        let _ = crate::session::write_text_artifact(&prev_aspect_path, &aspect).await;
        if prev_aspect.as_deref() != Some(aspect.as_str()) {
            // Aspect changed → force poster regenerate at the new canvas ratio.
            let cover = work.join(crate::agents::COVER_FILENAME);
            let _ = tokio::fs::remove_file(&cover).await;
        }
        // Idea/Novel/Script: film-level enrich. Per-scene pacing is applied inside each pipeline.
        // Language lock uses the user's creative source so Chinese ideas stay Chinese in planning.
        let lang_sources = [
            record.idea.as_str(),
            record.script.as_str(),
            record.novel_text.as_str(),
            record.user_requirement.as_str(),
        ];
        // Vertical skills inject at plan-time only — do not overwrite the user's raw requirement.
        let skill_overlay = self.skills.compose_for_plan(
            record.workflow,
            &record.vertical_skill_ids,
            &record.user_requirement,
            &record.style,
        )?;
        if !skill_overlay.applied_skill_ids.is_empty() {
            let _ = crate::session::write_text_artifact(
                &work.join("vertical_skills.txt"),
                &skill_overlay.applied_skill_ids.join("\n"),
            )
            .await;
            let _ = crate::session::write_text_artifact(
                &work.join("vertical_skill_overlay.txt"),
                &skill_overlay.user_requirement,
            )
            .await;
        }
        let req_base = crate::planning::with_language_lock(
            &skill_overlay.user_requirement,
            &lang_sources,
        );
        let req = match record.workflow {
            WorkflowKind::Script2Video
            | WorkflowKind::Idea2Video
            | WorkflowKind::Novel2Video => {
                crate::planning::enrich_requirement_for_film(
                    backends.clip,
                    &req_base,
                    target_secs,
                )
            }
            WorkflowKind::Action2Video => req_base,
        };
        // Persist an explicit budget only. Agent mode omits duration so ViMax-style
        // planning lets the model size the film from the story.
        if let Some(target_secs) = target_secs {
            let _ = crate::session::write_text_artifact(
                &work.join("target_duration_secs.txt"),
                &target_secs.to_string(),
            )
            .await;
            if record.target_duration_secs != target_secs {
                let _ = self
                    .index
                    .update_fields(id, |r| r.target_duration_secs = target_secs);
            }
        }
        let style_s = crate::planning::resolve_visual_style(if skill_overlay.style.is_empty() {
            if record.style.is_empty() {
                ""
            } else {
                record.style.as_str()
            }
        } else {
            skill_overlay.style.as_str()
        });
        let _ = crate::session::write_text_artifact(&work.join("style.txt"), &style_s).await;
        // Keep session field in sync when client omitted style (store base style, not overlays).
        if record.style.is_empty() && skill_overlay.style.is_empty() {
            let style_persist = style_s.clone();
            let _ = self
                .index
                .update_fields(id, |r| r.style = style_persist);
        }
        let progress = progress_callback(Arc::clone(self), id);
        let plan_started = std::time::Instant::now();

        match record.workflow {
            WorkflowKind::Novel2Video => {
                let novel = first_nonempty([
                    novel_text.as_deref(),
                    Some(record.novel_text.as_str()),
                    // Tolerate mis-tagged payloads from older clients.
                    idea.as_deref(),
                    script.as_deref(),
                ]);
                if novel.trim().is_empty() {
                    return Err(VimaxError::InvalidParams("novel_text required".into()));
                }
                if record.novel_text.is_empty() {
                    let _ = self.index.update_fields(id, |r| r.novel_text = novel.clone());
                }
                Novel2VideoPipeline::new(backends, work)
                    .plan_text_artifacts(&novel, &req, &style_s, Some(progress))
                    .await?;
            }
            WorkflowKind::Script2Video => {
                let script_text = first_nonempty([
                    script.as_deref(),
                    Some(record.script.as_str()),
                    idea.as_deref(),
                    novel_text.as_deref(),
                ]);
                if script_text.trim().is_empty() {
                    return Err(VimaxError::InvalidParams("script required".into()));
                }
                if record.script.is_empty() {
                    let _ = self.index.update_fields(id, |r| r.script = script_text.clone());
                }
                ScriptFilmPipeline::new(backends, work)
                    .plan_text_artifacts(&script_text, &req, &style_s, Some(progress))
                    .await?;
            }
            WorkflowKind::Idea2Video => {
                let idea_text = first_nonempty([
                    idea.as_deref(),
                    Some(record.idea.as_str()),
                    script.as_deref(),
                    novel_text.as_deref(),
                ]);
                if idea_text.trim().is_empty() {
                    return Err(VimaxError::InvalidParams("idea required".into()));
                }
                if record.idea.is_empty() {
                    let _ = self.index.update_fields(id, |r| r.idea = idea_text.clone());
                }
                Idea2VideoPipeline::new(backends, work)
                    .plan_text_artifacts(&idea_text, &req, &style_s, Some(progress))
                    .await?;
            }
            WorkflowKind::Action2Video => {
                let duration = Action2VideoPipeline::new(backends, work)
                    .prepare(Some(progress))
                    .await?;
                if record.target_duration_secs != duration {
                    let _ = self
                        .index
                        .update_fields(id, |r| r.target_duration_secs = duration);
                }
            }
        }
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = live_status(&self.index, &mut map, id);
            st.progress = 100.0;
            st.emit("planned", "规划完成，可以开始渲染", None);
            persist_run_status(&self.index, id, st);
        }
        tracing::info!(
            phase = "plan_total",
            secs = plan_started.elapsed().as_secs_f64(),
            workflow = ?record.workflow,
            "plan job wall time"
        );
        let cover_rel = self.sync_cover_from_disk(id);
        let _ = self.index.update_fields(id, |r| {
            r.stage = "planned".into();
            r.summary = "规划完成，可以开始渲染".into();
            if let Some(c) = &cover_rel {
                r.cover = Some(c.clone());
            }
        });
        if let Some(c) = cover_rel {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(st) = map.get_mut(id) {
                st.cover = Some(c);
                persist_run_status(&self.index, id, st);
            }
        }
        Ok(())
    }

    async fn run_render(self: &Arc<Self>, id: &str, token: CancellationToken) -> VimaxResult<()> {
        if token.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        let record = self.index.get(id)?;
        ensure_action_imitation_video_model(&record)?;
        let target_secs = if record.target_duration_secs > 0 {
            Some(crate::planning::normalize_target_duration_secs(Some(
                record.target_duration_secs,
            )))
        } else {
            None
        };
        let backends = self.backends_for(&record, Some(token.clone())).await?;
        let session_root = self.index.working_dir(id)?;
        // Imported projects may still carry another machine's absolute registry paths.
        // Repair before we try to upload Seedance reference images.
        match crate::session::remap_imported_working_paths(&session_root) {
            Ok(n) if n > 0 => {
                tracing::info!(
                    session_id = %id,
                    rewritten = n,
                    "remapped stale absolute asset paths before render"
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    session_id = %id,
                    error = %e,
                    "path remap before render failed"
                );
            }
        }
        let work = session_root.join(record.workflow.artifact_root());
        if let Some(target_secs) = target_secs {
            let _ = crate::session::write_text_artifact(
                &work.join("target_duration_secs.txt"),
                &target_secs.to_string(),
            )
            .await;
        }
        let req = record.user_requirement.clone();
        let style_s = crate::planning::resolve_visual_style(if record.style.is_empty() {
            ""
        } else {
            record.style.as_str()
        });
        let _ = crate::session::write_text_artifact(&work.join("style.txt"), &style_s).await;
        let progress = progress_callback(Arc::clone(self), id);

        // Periodically check cancel while waiting on long video polls is handled
        // inside FlowyVideo; also check before entering the pipeline.
        if token.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        let render_started = std::time::Instant::now();

        let final_video = match record.workflow {
            WorkflowKind::Script2Video => {
                ScriptFilmPipeline::new(backends, work)
                    .render(&record.script, &req, &style_s, Some(progress))
                    .await?
            }
            WorkflowKind::Idea2Video => {
                Idea2VideoPipeline::new(backends, work)
                    .render(&record.idea, &req, &style_s, Some(progress))
                    .await?
            }
            WorkflowKind::Novel2Video => {
                Novel2VideoPipeline::new(backends, work)
                    .render(&record.novel_text, &req, &style_s, Some(progress))
                    .await?
            }
            WorkflowKind::Action2Video => {
                Action2VideoPipeline::new(backends, work)
                    .render(Some(progress))
                    .await?
            }
        };

        let work_root = self.index.working_dir(id)?;
        let rel = final_video
            .strip_prefix(&work_root)
            .unwrap_or(&final_video)
            .to_string_lossy()
            .replace('\\', "/");

        // Display-only poster: AI cover from planning, else still from the finished film.
        let artifact_dir = final_video
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| work_root.join(record.workflow.artifact_root()));
        let _ = ensure_cover_from_final_video(&artifact_dir, &final_video).await;
        let cover_rel = self.sync_cover_from_disk(id);

        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = live_status(&self.index, &mut map, id);
            st.final_video = Some(rel.clone());
            if let Some(c) = &cover_rel {
                st.cover = Some(c.clone());
            }
            st.progress = 100.0;
            st.status = RunStatus::Succeeded;
            st.message = "render complete".into();
            st.emit("render_done", "render complete", None);
            persist_run_status(&self.index, id, st);
        }
        let _ = self.index.update_fields(id, |r| {
            r.final_video = Some(rel);
            if let Some(c) = &cover_rel {
                r.cover = Some(c.clone());
            }
            r.status = RunStatus::Succeeded;
            r.stage = "render_done".into();
            r.summary = "render complete".into();
        });
        tracing::info!(
            phase = "render_total",
            secs = render_started.elapsed().as_secs_f64(),
            workflow = ?record.workflow,
            "render job wall time"
        );
        Ok(())
    }

    /// If `{artifact_root}/cover.png` exists on disk, return its session-relative path.
    fn sync_cover_from_disk(&self, id: &str) -> Option<String> {
        let record = self.index.get(id).ok()?;
        let work_root = self.index.working_dir(id).ok()?;
        let artifact = work_root.join(record.workflow.artifact_root());
        let cover = artifact.join(COVER_FILENAME);
        if !media_local::is_usable_image_file(&cover) {
            return None;
        }
        Some(
            cover
                .strip_prefix(&work_root)
                .unwrap_or(&cover)
                .to_string_lossy()
                .replace('\\', "/"),
        )
    }
}

fn ensure_action_imitation_video_model(record: &SessionRecord) -> VimaxResult<()> {
    if !record.workflow.is_action_imitation() {
        return Ok(());
    }
    let model = record.video_model.trim();
    if model.is_empty() || !model_supports_action_imitation(model) {
        return Err(VimaxError::InvalidParams(
            "action imitation requires a MiniMax-H3 video model".into(),
        ));
    }
    Ok(())
}

fn nonempty_opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn resolve_aspect_for_session(
    record: &SessionRecord,
    media: &nomi_config::MediaGenConfig,
) -> String {
    if !record.aspect_ratio.trim().is_empty() {
        crate::aspect::normalize_aspect_ratio(&record.aspect_ratio)
    } else {
        crate::aspect::normalize_aspect_ratio(&media.video.default_aspect_ratio)
    }
}

fn resolve_resolution_for_session(
    record: &SessionRecord,
    media: &nomi_config::MediaGenConfig,
) -> String {
    let model = if record.video_model.trim().is_empty() {
        media.video.model.as_str()
    } else {
        record.video_model.as_str()
    };
    let raw = if !record.resolution.trim().is_empty() {
        record.resolution.as_str()
    } else {
        media.video.default_resolution.as_str()
    };
    crate::video_quality::normalize_resolution_for_model(model, raw)
}

fn resolve_fps_for_session(record: &SessionRecord, media: &nomi_config::MediaGenConfig) -> u32 {
    let model = if record.video_model.trim().is_empty() {
        media.video.model.as_str()
    } else {
        record.video_model.as_str()
    };
    let raw = if record.fps > 0 {
        record.fps
    } else {
        crate::video_quality::DEFAULT_VIDEO_FPS
    };
    crate::video_quality::normalize_fps_for_model(model, raw)
}

fn progress_callback(svc: Arc<VimaxService>, id: &str) -> crate::progress::ProgressCallback {
    let id = id.to_string();
    Arc::new(move |stage, message, meta| {
        let credit_delta = video_task_credit_delta(stage, meta.as_ref());

        let mut session_total: Option<i64> = None;
        if let Some((task_id, credits)) = credit_delta {
            let _ = svc.index.update_fields(&id, |r| {
                let _ = apply_video_task_credits(r, task_id, credits);
                session_total = Some(r.credits_consumed);
            });
        }

        {
            let mut map = svc.statuses.lock().unwrap_or_else(|e| e.into_inner());
            let st = live_status(&svc.index, &mut map, &id);
            if let Some(pct) = meta
                .as_ref()
                .and_then(|m| m.get("progress"))
                .and_then(|v| v.as_f64())
            {
                st.progress = pct.clamp(0.0, 100.0) as f32;
            }
            if let Some(total) = session_total {
                st.credits_consumed = total;
            }
            st.emit(stage, message, meta.clone());
            if stage != "video_poll" || session_total.is_some() {
                persist_run_status(&svc.index, &id, st);
            }
        }
        let _ = svc.index.update_fields(&id, |r| {
            r.stage = stage.to_string();
            r.summary = message.to_string();
        });
    })
}
