//! Public ViMax service API used by `nomifun-vimax` HTTP routes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use futures::FutureExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::backends::FlowyVimaxServices;
use crate::domain::WorkflowKind;
use crate::error::{VimaxError, VimaxResult};
use crate::pipelines::{
    Idea2VideoPipeline, Novel2VideoPipeline, PipelineBackends, Script2VideoPipeline,
};
use crate::progress::{RenderStatus, RunStatus};
use crate::session::{
    ArtifactNode, SessionIndex, SessionRecord, apply_status_to_record,
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
            return Ok(out);
        }
        Ok(RenderStatus {
            status: record.status,
            stage: record.stage,
            message: record.summary,
            progress: 0.0,
            error: None,
            final_video: record.final_video,
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
                    token.clone(),
                ),
            )
            .catch_unwind()
            .await
            {
                Ok(r) => r,
                Err(_) => Err(VimaxError::msg("planning task panicked")),
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
                Err(_) => Err(VimaxError::msg("render task panicked")),
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
        let summary = format!(
            "Revised {}; invalidated {} artifacts",
            result.revised_path,
            result.invalidated.len()
        );
        self.index.update_fields(id, |r| {
            r.stage = "revised".into();
            r.summary = summary.clone();
            for key in &result.stale_keys {
                r.stale.insert(key.clone(), true);
            }
        })?;
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
                                st.message = "规划完成，可以开始渲染".into();
                                st.progress = 100.0;
                                st.error = None;
                                st.emit("planned", "规划完成，可以开始渲染", None);
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
                            "失败于步骤「{prev_stage}」\n上一状态：{prev_message}\n\n{detail}"
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
        Ok(PipelineBackends {
            chat: Arc::new(flowy.chat_with_model(llm)),
            image: Arc::new(flowy.image_with_model(image)),
            video: Arc::new(flowy.video_with_model_and_cancel(video, cancel.clone())),
            flowy: Some(flowy.clone()),
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
        })?;

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
        // Idea/Novel: film-level scene budget. Script2Video: whole target = one scene.
        let req = match record.workflow {
            WorkflowKind::Script2Video => crate::planning::enrich_requirement_for_planning(
                &record.user_requirement,
                Some(target_secs),
            ),
            WorkflowKind::Idea2Video | WorkflowKind::Novel2Video => {
                crate::planning::enrich_requirement_for_film(
                    &record.user_requirement,
                    Some(target_secs),
                )
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
        let style_s = if record.style.is_empty() {
            "cinematic".into()
        } else {
            record.style.clone()
        };
        let progress = progress_callback(Arc::clone(self), id);

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
        let _ = self.index.update_stage(id, "planned", "规划完成，可以开始渲染");
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
        let style_s = if record.style.is_empty() {
            "cinematic".into()
        } else {
            record.style.clone()
        };
        let progress = progress_callback(Arc::clone(self), id);

        // Periodically check cancel while waiting on long video polls is handled
        // inside FlowyVideo; also check before entering the pipeline.
        if token.is_cancelled() {
            return Err(VimaxError::Cancelled);
        }

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
        {
            let mut map = self
                .statuses
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = map.entry(id.to_string()).or_default();
            st.final_video = Some(rel.clone());
            st.progress = 100.0;
            st.status = RunStatus::Succeeded;
            st.message = "render complete".into();
            st.emit("render_done", "render complete", None);
        }
        let _ = self.index.update_fields(id, |r| {
            r.final_video = Some(rel);
            r.status = RunStatus::Succeeded;
            r.stage = "render_done".into();
            r.summary = "render complete".into();
        });
        Ok(())
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
