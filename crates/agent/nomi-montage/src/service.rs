//! `MontageService` — the single public entry point used by the API/UI layer.
//!
//! Owns the embedded pipeline/artifact/style registries (loaded once), the
//! current Flowy media session (swappable via [`MontageService::set_media`]),
//! and the map of in-flight project jobs. Every project mutation is a plain
//! file write under `{data_dir}/montage/projects/<id>/`; the only in-memory
//! state is "is a job currently running for this id" (the [`CancellationToken`]
//! map), so a process restart loses nothing but the ability to cancel a run
//! that was already interrupted anyway.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use nomi_config::GatewayConfig;
use nomi_media_backends::FlowyMediaServices;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::artifacts::ArtifactRegistry;
use crate::checkpoint::{Checkpoint, CheckpointStore, ProjectRunStatus};
use crate::config::MontageRuntimeConfig;
use crate::creative::{CreativeFilm, scan_project};
use crate::error::{MontageError, MontageResult};
use crate::events::{self, EventKind, EventRecord};
use crate::orchestrator::{
    ApprovalDecision, ApprovalRequest, EpRunParams, ProviderMenu, apply_approval,
    build_provider_menu, missing_tools_for_pipeline, run_project,
};
use crate::pipeline::{PipelineRegistry, PipelineSummary};
use crate::project::{
    BoardState, CreateProjectRequest, ProjectRecord, ProjectStore, export_project_zip,
    import_project_zip,
};
use crate::styles::StyleRegistry;
use crate::tools::{ToolContext, ToolRegistry, build_default_registry};

/// Compact run-status projection for polling endpoints (see [`BoardState`] for
/// the fuller per-stage view).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatus {
    pub status: String,
    pub current_stage: String,
    pub awaiting_human_stage: Option<String>,
    pub last_error: Option<String>,
    pub is_job_running: bool,
    /// Relative path when a finished cut exists (typically `renders/final.mp4`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_video: Option<String>,
}

/// `project.json` plus its current checkpoint (`None` before the first
/// `start()`, though `create_project` always writes an initial one).
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetail {
    pub record: ProjectRecord,
    pub checkpoint: Option<Checkpoint>,
}

/// Mutable half of the runtime: the current Flowy session and the tool
/// registry derived from it. Kept together so they can never disagree about
/// whether Flowy-backed tools should be registered.
struct MediaState {
    media: Option<FlowyMediaServices>,
    tools: Arc<ToolRegistry>,
}

pub struct MontageService {
    data_dir: PathBuf,
    config: MontageRuntimeConfig,
    pipelines: Arc<PipelineRegistry>,
    artifacts: Arc<ArtifactRegistry>,
    #[allow(dead_code)] // kept for future style-aware prompt assembly / API surface
    styles: Arc<StyleRegistry>,
    media_state: Mutex<MediaState>,
    projects: ProjectStore,
    /// Project id → cancellation token for its currently running EP job.
    jobs: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl MontageService {
    /// Loads the embedded pipeline/artifact/style assets and binds the
    /// current Flowy session (if any is configured). Returns `None` only when
    /// the embedded assets themselves fail to load — a build-time invariant
    /// violation, not a "not configured yet" condition — so callers should
    /// treat `None` as "montage runtime unavailable, check logs at startup".
    pub fn try_new(config: &GatewayConfig, data_dir: &Path) -> Option<Self> {
        let pipelines = match PipelineRegistry::load_embedded() {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "failed to load embedded montage pipeline_defs");
                return None;
            }
        };
        let artifacts = match ArtifactRegistry::load_embedded() {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(error = %e, "failed to load embedded montage artifact schemas");
                return None;
            }
        };
        let styles = match StyleRegistry::load_embedded() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "failed to load embedded montage style playbooks");
                return None;
            }
        };

        let media = FlowyMediaServices::try_new(config, data_dir);
        let tools = Arc::new(build_default_registry(media.is_some()));

        Some(Self {
            data_dir: data_dir.to_path_buf(),
            config: MontageRuntimeConfig::from(config),
            pipelines: Arc::new(pipelines),
            artifacts: Arc::new(artifacts),
            styles: Arc::new(styles),
            media_state: Mutex::new(MediaState { media, tools }),
            projects: ProjectStore::new(data_dir),
            jobs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Rebind the Flowy session (sign-in / sign-out / config reload) and
    /// re-derive tool availability so `provider_menu` and the next `start()`
    /// immediately reflect it.
    pub fn set_media(&self, media: Option<FlowyMediaServices>) {
        let tools = Arc::new(build_default_registry(media.is_some()));
        let mut guard = self.media_state.lock().unwrap_or_else(|e| e.into_inner());
        guard.media = media;
        guard.tools = tools;
    }

    pub fn list_pipelines(&self) -> Vec<PipelineSummary> {
        self.pipelines.list().into_iter().map(PipelineSummary::from).collect()
    }

    pub fn get_pipeline(&self, name: &str) -> MontageResult<crate::pipeline::PipelineManifest> {
        Ok(self.pipelines.require(name)?.clone())
    }

    pub fn provider_menu(&self) -> ProviderMenu {
        let guard = self.media_state.lock().unwrap_or_else(|e| e.into_inner());
        build_provider_menu(&guard.tools, guard.media.is_some())
    }

    /// Absolute project root (`…/montage/projects/<id>/`), after verifying the project exists.
    pub fn project_paths(&self, id: &str) -> MontageResult<crate::paths::ProjectPaths> {
        self.projects.load(id)?;
        Ok(self.projects.paths(id))
    }

    /// JSON artifact file names present under the project's `artifacts/` dir (no extension).
    pub fn list_artifacts(&self, id: &str) -> MontageResult<Vec<String>> {
        self.projects.load(id)?;
        let dir = self.projects.paths(id).artifacts_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !stem.is_empty() {
                names.push(stem);
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn recent_events(&self, id: &str, limit: usize) -> MontageResult<Vec<EventRecord>> {
        self.projects.load(id)?;
        let path = self.projects.paths(id).events_jsonl();
        events::read_recent_events(&path, limit)
    }

    /// Tools a specific pipeline needs that the current build/session cannot
    /// provide right now (e.g. Flowy tools before sign-in).
    pub fn missing_tools(&self, pipeline: &str) -> MontageResult<Vec<String>> {
        let manifest = self.pipelines.require(pipeline)?;
        let guard = self.media_state.lock().unwrap_or_else(|e| e.into_inner());
        Ok(missing_tools_for_pipeline(manifest, &guard.tools))
    }

    pub async fn list_projects(&self) -> MontageResult<Vec<ProjectRecord>> {
        self.projects.list()
    }

    pub async fn create_project(&self, req: CreateProjectRequest) -> MontageResult<ProjectRecord> {
        let manifest = self.pipelines.require(&req.pipeline)?;
        let mode = manifest.mode;
        let first_stage = manifest
            .first_stage()
            .ok_or_else(|| MontageError::ManifestInvalid(manifest.name.clone(), "pipeline has no stages".into()))?
            .name
            .clone();

        let record = ProjectRecord::new(req, mode, self.config.default_budget_credits);
        self.projects.save(&record)?;

        let paths = self.projects.paths(&record.id);
        let checkpoint_store = CheckpointStore::new(&paths);
        checkpoint_store.write(&Checkpoint::new(&record.id, &record.pipeline, &first_stage))?;

        let _ = events::append_event(
            &paths.events_jsonl(),
            &EventRecord::new(
                record.id.clone(),
                EventKind::ProjectCreated,
                format!("project '{}' created on pipeline '{}'", record.title, record.pipeline),
            ),
        );
        Ok(record)
    }

    pub async fn get_project(&self, id: &str) -> MontageResult<ProjectDetail> {
        let record = self.projects.load(id)?;
        let paths = self.projects.paths(id);
        let checkpoint = CheckpointStore::new(&paths).read()?;
        Ok(ProjectDetail { record, checkpoint })
    }

    pub async fn delete_project(&self, id: &str) -> MontageResult<()> {
        self.ensure_not_running(id)?;
        self.projects.delete(id)
    }

    /// Kick off (or resume) the Executive Producer loop for `id` in the
    /// background. Returns as soon as the job is scheduled — poll `status`,
    /// `board_state`, or watch `events.jsonl` for progress.
    pub async fn start(&self, id: &str) -> MontageResult<()> {
        self.ensure_not_running(id)?;
        let record = self.projects.load(id)?;
        let manifest = self.pipelines.require(&record.pipeline)?.clone();

        let paths = Arc::new(self.projects.paths(id));
        paths.ensure_dirs()?;
        if CheckpointStore::new(&paths).read()?.is_none() {
            return Err(MontageError::msg(format!(
                "project '{id}' has no checkpoint; it may not have been created correctly"
            )));
        }

        let (media, tools) = {
            let guard = self.media_state.lock().unwrap_or_else(|e| e.into_inner());
            (guard.media.clone(), Arc::clone(&guard.tools))
        };
        let media = media.ok_or(MontageError::NotAuthenticated)?;

        let cancel = CancellationToken::new();
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string(), cancel.clone());

        let ctx_template = ToolContext {
            project_id: id.to_string(),
            stage: String::new(),
            paths: Arc::clone(&paths),
            artifact_registry: Arc::clone(&self.artifacts),
            media: Some(media.clone()),
            cancel: cancel.clone(),
            events_path: paths.events_jsonl(),
            user_prompt: record.prompt.clone(),
            style_playbook: record.style_playbook.clone(),
            models: record.models.clone(),
            output: record.output.clone(),
            budget_credits: record.budget_credits,
        };

        // Persist aspect hint for any local helpers that read aspect from the project dir.
        let aspect_path = paths.root.join("aspect.txt");
        let _ = std::fs::write(&aspect_path, format!("{}\n", record.output.aspect.trim()));

        let chat_model = record.models.chat.clone();
        let checkpoint_policy = record.checkpoint_policy;
        let max_tool_turns = self.config.orchestrator.max_tool_turns_per_stage;
        let jobs = Arc::clone(&self.jobs);
        let id_owned = id.to_string();

        tokio::spawn(async move {
            let chat = media.chat_with_model(chat_model);
            let checkpoint_store = CheckpointStore::new(&paths);

            let run = std::panic::AssertUnwindSafe(run_project(EpRunParams {
                manifest: &manifest,
                chat: &chat,
                registry: tools.as_ref(),
                ctx_template,
                checkpoint_store: &checkpoint_store,
                checkpoint_policy,
                max_tool_turns,
            }))
            .catch_unwind()
            .await;

            match run {
                Ok(Ok(())) => {}
                Ok(Err(MontageError::Cancelled)) => {
                    tracing::info!(project_id = %id_owned, "montage project run cancelled");
                }
                Ok(Err(e)) => {
                    tracing::warn!(project_id = %id_owned, error = %e, "montage project run ended with error");
                }
                Err(payload) => {
                    let e = MontageError::from_panic_payload("montage run_project", payload);
                    tracing::error!(project_id = %id_owned, error = %e, "montage project task panicked");
                }
            }
            jobs.lock().unwrap_or_else(|e| e.into_inner()).remove(&id_owned);
        });

        Ok(())
    }

    /// Request cancellation of a running job (best-effort, cooperative — the
    /// EP loop checks the token between stages/tool calls).
    pub async fn cancel(&self, id: &str) -> MontageResult<()> {
        if let Some(token) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            token.cancel();
        }
        Ok(())
    }

    pub async fn status(&self, id: &str) -> MontageResult<RunStatus> {
        self.projects.load(id)?; // validates existence
        let paths = self.projects.paths(id);
        let checkpoint = CheckpointStore::new(&paths).read()?;
        let is_job_running = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id);
        let (status, current_stage, awaiting_human_stage, last_error) = match &checkpoint {
            Some(cp) => (
                cp.status.as_str().to_string(),
                cp.current_stage.clone(),
                cp.awaiting_human_stage.clone(),
                cp.last_error.clone(),
            ),
            None => (ProjectRunStatus::Idle.as_str().to_string(), String::new(), None, None),
        };
        Ok(RunStatus {
            status,
            current_stage,
            awaiting_human_stage,
            last_error,
            is_job_running,
            final_video: paths.final_video_relpath_if_present(),
        })
    }

    pub async fn board_state(&self, id: &str) -> MontageResult<BoardState> {
        let record = self.projects.load(id)?;
        let manifest = self.pipelines.require(&record.pipeline)?;
        let paths = self.projects.paths(id);
        let checkpoint = CheckpointStore::new(&paths)
            .read()?
            .ok_or_else(|| MontageError::msg(format!("project '{id}' has no checkpoint yet")))?;
        let recent_events = events::read_recent_events(&paths.events_jsonl(), 50)?;
        Ok(BoardState::build(manifest, &checkpoint, recent_events).with_media(
            paths.final_video_relpath_if_present(),
            paths.list_media_clips(),
        ))
    }

    /// Apply a human approval/reject/send-back decision, then — if the
    /// project is ready to keep going (`Idle`, not `AwaitingHuman`/`Failed`)
    /// — transparently resume the run so the caller does not have to issue a
    /// separate `start()`.
    pub async fn approve(&self, id: &str, request: ApprovalRequest) -> MontageResult<BoardState> {
        let record = self.projects.load(id)?;
        let manifest = self.pipelines.require(&record.pipeline)?.clone();
        let paths = self.projects.paths(id);
        let store = CheckpointStore::new(&paths);
        let mut checkpoint = store
            .read()?
            .ok_or_else(|| MontageError::msg(format!("project '{id}' has no checkpoint yet")))?;

        apply_approval(&mut checkpoint, &manifest, &request)?;
        store.write(&checkpoint)?;

        let event_kind = match request.decision {
            ApprovalDecision::Approve => EventKind::Approved,
            ApprovalDecision::Reject => EventKind::Rejected,
            ApprovalDecision::SendBack => EventKind::SendBack,
        };
        let _ = events::append_event(
            &paths.events_jsonl(),
            &EventRecord::new(id.to_string(), event_kind, format!("human {:?} on stage '{}'", request.decision, request.stage))
                .with_stage(request.stage.clone()),
        );

        let should_resume = checkpoint.status == ProjectRunStatus::Idle;
        let recent_events = events::read_recent_events(&paths.events_jsonl(), 50)?;
        let board = BoardState::build(&manifest, &checkpoint, recent_events).with_media(
            paths.final_video_relpath_if_present(),
            paths.list_media_clips(),
        );

        if should_resume
            && let Err(e) = self.start(id).await
        {
            tracing::warn!(project_id = %id, error = %e, "failed to auto-resume montage project after approval");
        }
        Ok(board)
    }

    pub async fn get_artifact(&self, id: &str, name: &str) -> MontageResult<serde_json::Value> {
        let paths = self.projects.paths(id);
        let path = paths.artifact_path(name);
        if !path.is_file() {
            return Err(MontageError::msg(format!("artifact '{name}' not found for project '{id}'")));
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Direct artifact write (no LLM) — schema-validated when a schema exists
    /// for `name`, otherwise written as-is (covers ad hoc/derived JSON).
    pub async fn put_artifact(&self, id: &str, name: &str, value: serde_json::Value) -> MontageResult<()> {
        let paths = self.projects.paths(id);
        paths.ensure_dirs()?;
        if self.artifacts.has(name) {
            self.artifacts.validate(name, &value)?;
        }
        let pretty = serde_json::to_string_pretty(&value)?;
        std::fs::write(paths.artifact_path(name), pretty)?;
        let _ = events::append_event(
            &paths.events_jsonl(),
            &EventRecord::new(id.to_string(), EventKind::ArtifactWritten, format!("artifact '{name}' written via API")),
        );
        Ok(())
    }

    pub async fn export_zip(&self, id: &str, dest_path: &Path) -> MontageResult<PathBuf> {
        self.ensure_not_running(id)?;
        let paths = self.projects.paths(id);
        export_project_zip(&paths, id, dest_path)
    }

    /// Import a `.nomimontage` archive as a brand-new project (fresh id, so
    /// repeated imports of the same archive never collide).
    pub async fn import_project(&self, archive_path: &Path) -> MontageResult<ProjectRecord> {
        let new_id = uuid::Uuid::new_v4().to_string();
        let paths = self.projects.paths(&new_id);
        import_project_zip(archive_path, &paths.root)?;

        let mut record: ProjectRecord = serde_json::from_str(&std::fs::read_to_string(paths.project_json())?)?;
        record.id = new_id.clone();
        self.projects.save(&record)?;

        let store = CheckpointStore::new(&paths);
        if let Some(mut checkpoint) = store.read()? {
            checkpoint.project_id = new_id;
            store.write(&checkpoint)?;
        }
        Ok(record)
    }

    /// Best-effort read-only projection of the project's current media for
    /// Canvas materialization. Never fails just because a stage hasn't run
    /// yet — returns an emptier film instead.
    pub async fn scan_creative_film(&self, id: &str) -> MontageResult<CreativeFilm> {
        let record = self.projects.load(id)?;
        let paths = self.projects.paths(id);
        scan_project(&paths, &record)
    }

    fn ensure_not_running(&self, id: &str) -> MontageResult<()> {
        if self.jobs.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id) {
            return Err(MontageError::Busy(id.to_string()));
        }
        Ok(())
    }
}
