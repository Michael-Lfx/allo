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
    Idea2VideoPipeline, Novel2VideoPipeline, PipelineBackends, Script2VideoPipeline,
};
use crate::progress::{RenderStatus, RunStatus};
use crate::session::{
    ArtifactNode, CameoPhotoEntry, CameoUpdate, SessionIndex, SessionRecord, apply_status_to_record,
    cameo,
};

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

#[derive(Clone, Copy)]
enum JobKind {
    Plan,
    Render,
}

pub struct VimaxService {
    #[allow(dead_code)]
    data_dir: PathBuf,
    index: SessionIndex,
    flowy: Mutex<Option<FlowyVimaxServices>>,
    /// Sync mutex so progress callbacks never drop updates via `try_lock`.
    statuses: StdMutex<HashMap<String, RenderStatus>>,
    cancels: Mutex<HashMap<String, CancellationToken>>,
}

impl VimaxService {
    pub fn start(data_dir: &Path, flowy: Option<FlowyVimaxServices>) -> VimaxResult<Arc<Self>> {
        Ok(Arc::new(Self {
            data_dir: data_dir.to_path_buf(),
            index: SessionIndex::open(data_dir)?,
            flowy: Mutex::new(flowy),
            statuses: StdMutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
        }))
    }

    /// Replace Flowy backends after login / config reload.
    pub async fn set_flowy(&self, flowy: Option<FlowyVimaxServices>) {
        *self.flowy.lock().await = flowy;
    }

    pub fn list_sessions(&self) -> VimaxResult<Vec<SessionRecord>> {
        self.index.list()
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

    pub async fn status(&self, id: &str) -> VimaxResult<RenderStatus> {
        let record = self.index.get(id)?;
        let working_abs = self
            .index
            .working_dir(id)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let map = self
            .statuses
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(s) = map.get(id) {
            let mut out = s.clone();
            out.working_dir_abs = working_abs.or(out.working_dir_abs);
            if out.cover.is_none() {
                out.cover = record.cover.clone();
            }
            if out.final_video.is_none() {
                out.final_video = record.final_video.clone();
            }
            return Ok(out);
        }
        Ok(RenderStatus {
            status: record.status,
            stage: record.stage,
            message: record.summary,
            progress: 0.0,
            error: None,
            final_video: record.final_video,
            cover: record.cover,
            working_dir_abs: working_abs,
            updated_at: record.updated_at,
            events: vec![],
        })
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
            let status = map.entry(id.to_string()).or_default();
            status.status = RunStatus::Cancelled;
            status.message = "cancelled".into();
            status.emit("cancelled", "cancelled", None);
        }
        let _ = self.index.update_fields(id, |r| {
            r.status = RunStatus::Cancelled;
            r.stage = "cancelled".into();
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

    async fn ensure_cameo_mutable(&self, id: &str) -> VimaxResult<()> {
        let record = self.index.get(id)?;
        if matches!(record.status, RunStatus::Planning | RunStatus::Rendering) {
            return Err(VimaxError::InvalidParams(
                "cannot modify cameo photos while the project is planning or rendering".into(),
            ));
        }
        let map = self.statuses.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = map.get(id)
            && matches!(s.status, RunStatus::Planning | RunStatus::Rendering)
        {
            return Err(VimaxError::InvalidParams(
                "cannot modify cameo photos while the project is planning or rendering".into(),
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
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        target_duration_secs: Option<u32>,
        aspect_ratio: Option<String>,
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
            let result = match std::panic::AssertUnwindSafe(
                svc.run_plan(
                    &id,
                    idea,
                    script,
                    novel_text,
                    user_requirement,
                    style,
                    llm_model,
                    image_model,
                    video_model,
                    target_duration_secs,
                    aspect_ratio,
                    token.clone(),
                ),
            )
            .catch_unwind()
            .await
            {
                Ok(r) => r,
                Err(payload) => Err(VimaxError::from_panic_payload("planning task", payload)),
            };
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
    ) -> VimaxResult<()> {
        if llm_model.is_some() || image_model.is_some() || video_model.is_some() {
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
            })?;
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
            let result = match std::panic::AssertUnwindSafe(svc.run_render(&id, token.clone()))
                .catch_unwind()
                .await
            {
                Ok(r) => r,
                Err(payload) => Err(VimaxError::from_panic_payload("render task", payload)),
            };
            svc.finish_job(&id, result, &token, JobKind::Render).await;
        });
        Ok(())
    }

    pub async fn revise(
        &self,
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
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = map.entry(id.to_string()).or_default();
            st.status = status;
            st.stage = status.as_str().into();
            st.message = message.into();
            st.error = None;
            st.progress = 0.0;
            st.emit(status.as_str(), message, None);
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
            let st = map.entry(id.to_string()).or_default();
            match result {
                Ok(()) => {
                    if token.is_cancelled() {
                        st.status = RunStatus::Cancelled;
                        st.message = "cancelled".into();
                        st.emit("cancelled", "cancelled", None);
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
                    st.status = RunStatus::Cancelled;
                    st.message = "cancelled".into();
                    st.emit("cancelled", "cancelled", None);
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
                    st.touch();
                    st.emit("failed", &composed, None);
                }
            }
            let _ = self.index.update_fields(id, |r| {
                apply_status_to_record(r, st);
            });
        }
        self.cancels.lock().await.remove(id);
    }

    async fn backends_for(
        &self,
        record: &SessionRecord,
        cancel: Option<CancellationToken>,
    ) -> VimaxResult<PipelineBackends> {
        let guard = self.flowy.lock().await;
        let flowy = guard.as_ref().ok_or(VimaxError::NotAuthenticated)?;
        let llm = nonempty_opt(&record.llm_model);
        let image = nonempty_opt(&record.image_model);
        let video = nonempty_opt(&record.video_model);
        let aspect = resolve_aspect_for_session(record, &flowy.media);
        Ok(PipelineBackends {
            chat: Arc::new(flowy.chat_with_model(llm)),
            // Portraits / env plates use default Seedream 2K — do NOT bind video aspect here.
            image: Arc::new(flowy.image_with_model(image.clone())),
            video: Arc::new(flowy.video_with_model_cancel_and_aspect(
                video,
                cancel.clone(),
                Some(aspect),
            )),
            flowy: Some(flowy.clone()),
            image_model: image,
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
        llm_model: Option<String>,
        image_model: Option<String>,
        video_model: Option<String>,
        target_duration_secs: Option<u32>,
        aspect_ratio: Option<String>,
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
        })?;

        // Resolve aspect (session → media default) before building backends so
        // video + cover image share the same Seedance ratio.
        let aspect = {
            let guard = self.flowy.lock().await;
            let media_default = guard
                .as_ref()
                .map(|f| f.media.video.default_aspect_ratio.as_str())
                .unwrap_or(crate::aspect::DEFAULT_ASPECT_RATIO);
            if record.aspect_ratio.trim().is_empty() {
                crate::aspect::normalize_aspect_ratio(media_default)
            } else {
                crate::aspect::normalize_aspect_ratio(&record.aspect_ratio)
            }
        };
        let record = if record.aspect_ratio != aspect {
            self.index
                .update_fields(id, |r| r.aspect_ratio = aspect.clone())?
        } else {
            record
        };

        let backends = self.backends_for(&record, Some(token.clone())).await?;
        let work = self
            .index
            .working_dir(id)?
            .join(record.workflow.artifact_root());
        tokio::fs::create_dir_all(&work).await?;
        let target_secs = crate::planning::normalize_target_duration_secs(
            if record.target_duration_secs > 0 {
                Some(record.target_duration_secs)
            } else {
                target_duration_secs
            },
        );
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
        // Idea/Novel: film-level scene budget. Script2Video: whole target = one scene.
        // Language lock uses the user's creative source so Chinese ideas stay Chinese in planning.
        let lang_sources = [
            record.idea.as_str(),
            record.script.as_str(),
            record.novel_text.as_str(),
            record.user_requirement.as_str(),
        ];
        let req_base = crate::planning::with_language_lock(
            &record.user_requirement,
            &lang_sources,
        );
        let req = match record.workflow {
            WorkflowKind::Script2Video => {
                crate::planning::enrich_requirement_for_planning(&req_base, Some(target_secs))
            }
            WorkflowKind::Idea2Video | WorkflowKind::Novel2Video => {
                crate::planning::enrich_requirement_for_film(&req_base, Some(target_secs))
            }
        };
        // Persist so render / child scene dirs can allocate clip lengths.
        let _ = crate::session::write_text_artifact(
            &work.join("target_duration_secs.txt"),
            &target_secs.to_string(),
        )
        .await;
        // Also keep session field in sync when client omitted it.
        if record.target_duration_secs == 0 {
            let _ = self
                .index
                .update_fields(id, |r| r.target_duration_secs = target_secs);
        }
        let style_s = crate::planning::resolve_visual_style(if record.style.is_empty() {
            ""
        } else {
            record.style.as_str()
        });
        let _ = crate::session::write_text_artifact(&work.join("style.txt"), &style_s).await;
        // Keep session field in sync when client omitted style.
        if record.style.is_empty() {
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
                Script2VideoPipeline::new(backends, work)
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
        }
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = map.entry(id.to_string()).or_default();
            st.progress = 100.0;
            st.emit("planned", "规划完成，可以开始渲染", None);
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
            }
        }
        Ok(())
    }

    async fn run_render(self: &Arc<Self>, id: &str, token: CancellationToken) -> VimaxResult<()> {
        if token.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }
        let mut record = self.index.get(id)?;
        let target_secs = crate::planning::normalize_target_duration_secs(
            if record.target_duration_secs > 0 {
                Some(record.target_duration_secs)
            } else {
                None
            },
        );
        if record.target_duration_secs == 0 {
            let _ = self
                .index
                .update_fields(id, |r| r.target_duration_secs = target_secs);
            record.target_duration_secs = target_secs;
        }
        let backends = self.backends_for(&record, Some(token.clone())).await?;
        let work = self
            .index
            .working_dir(id)?
            .join(record.workflow.artifact_root());
        let _ = crate::session::write_text_artifact(
            &work.join("target_duration_secs.txt"),
            &target_secs.to_string(),
        )
        .await;
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
                Script2VideoPipeline::new(backends, work)
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
            let st = map.entry(id.to_string()).or_default();
            st.final_video = Some(rel.clone());
            if let Some(c) = &cover_rel {
                st.cover = Some(c.clone());
            }
            st.progress = 100.0;
            st.status = RunStatus::Succeeded;
            st.message = "render complete".into();
            st.emit("render_done", "render complete", None);
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

fn progress_callback(svc: Arc<VimaxService>, id: &str) -> crate::progress::ProgressCallback {
    let id = id.to_string();
    Arc::new(move |stage, message, meta| {
        {
            let mut map = svc.statuses.lock().unwrap_or_else(|e| e.into_inner());
            let st = map.entry(id.clone()).or_default();
            if let Some(pct) = meta
                .as_ref()
                .and_then(|m| m.get("progress"))
                .and_then(|v| v.as_f64())
            {
                st.progress = pct.clamp(0.0, 100.0) as f32;
            }
            st.emit(stage, message, meta.clone());
        }
        let _ = svc.index.update_fields(&id, |r| {
            r.stage = stage.to_string();
            r.summary = message.to_string();
        });
    })
}
