//! In-process eval lab: one live run at a time, isolated from user sessions.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use nomi_agent_eval::{
    cache_dir, default_trials_for_suite, list_suites, load_suite_manifest, private_corpus_dir,
    run_loaded_manifest, summarize, Case, EvalCaseTrace, EvalResult, Manifest, RunConfig,
    RunProgress, RunProgressPhase, ScorerResult, ScorerSpec, SuiteDescriptor, Summary,
    SCHEMA_VERSION,
};
use nomifun_api_types::{
    EvalArtifactView, EvalCaseFlip, EvalCaseTraceView, EvalCaseView, EvalCategoryView,
    EvalRunDiffView, EvalRunListItem, EvalRunView, EvalScorerView, EvalSuiteDescriptor,
    EvalSummaryView, EvalTrajectoryEventView, PullEvalDatasetResponse, StartEvalRunRequest,
};
use nomifun_common::AppError;
use nomifun_db::{IClientPreferenceRepository, IProviderModelRepository, IProviderRepository};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::agent_eval::live::{
    sanitize_case_dir, trace_file_name, LiveEvalTrace, LiveNomiHarness,
};
use crate::agent_eval::quality::{EvalQualityCase, EvalQualityReport, EvalQualitySink};
use crate::agent_eval::session_bridge::{eval_run_workspace_label, EvalSessionBridge};
use crate::agent_trace::developer_mode_enabled;
use crate::{AgentTraceHub, SessionObservationList};
use crate::knowledge_completer::resolve_default_model;

pub struct EvalLab {
    data_dir: PathBuf,
    provider_repo: Arc<dyn IProviderRepository>,
    provider_model_repo: Arc<dyn IProviderModelRepository>,
    encryption_key: [u8; 32],
    client_prefs: Arc<dyn IClientPreferenceRepository>,
    session_bridge: Option<Arc<dyn EvalSessionBridge>>,
    trace_hub: Option<Arc<AgentTraceHub>>,
    quality_sink: Mutex<Option<Arc<dyn EvalQualitySink>>>,
    inner: AsyncMutex<LabInner>,
}

struct LabInner {
    active: Option<ActiveRun>,
}

struct ActiveRun {
    snapshot: Arc<Mutex<EvalRunView>>,
    cancel: Arc<AtomicBool>,
    live_trace: Arc<Mutex<Option<LiveEvalTrace>>>,
}

fn lock_snapshot(snapshot: &Mutex<EvalRunView>) -> std::sync::MutexGuard<'_, EvalRunView> {
    snapshot.lock().unwrap_or_else(|e| e.into_inner())
}

impl EvalLab {
    pub fn new(
        data_dir: PathBuf,
        provider_repo: Arc<dyn IProviderRepository>,
        provider_model_repo: Arc<dyn IProviderModelRepository>,
        encryption_key: [u8; 32],
        client_prefs: Arc<dyn IClientPreferenceRepository>,
    ) -> Self {
        Self::with_session_binding(
            data_dir,
            provider_repo,
            provider_model_repo,
            encryption_key,
            client_prefs,
            None,
            None,
        )
    }

    pub fn with_session_binding(
        data_dir: PathBuf,
        provider_repo: Arc<dyn IProviderRepository>,
        provider_model_repo: Arc<dyn IProviderModelRepository>,
        encryption_key: [u8; 32],
        client_prefs: Arc<dyn IClientPreferenceRepository>,
        session_bridge: Option<Arc<dyn EvalSessionBridge>>,
        trace_hub: Option<Arc<AgentTraceHub>>,
    ) -> Self {
        Self {
            data_dir,
            provider_repo,
            provider_model_repo,
            encryption_key,
            client_prefs,
            session_bridge,
            trace_hub,
            quality_sink: Mutex::new(None),
            inner: AsyncMutex::new(LabInner { active: None }),
        }
    }

    pub fn set_quality_sink(&self, sink: Arc<dyn EvalQualitySink>) {
        *self.quality_sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    }

    pub async fn require_developer_mode(&self) -> Result<(), AppError> {
        if developer_mode_enabled(Some(&self.client_prefs)).await {
            Ok(())
        } else {
            Err(AppError::Forbidden(
                "Enable Developer Mode in Settings → System to use agent evals".into(),
            ))
        }
    }

    pub async fn list_suites(&self) -> Result<Vec<EvalSuiteDescriptor>, AppError> {
        self.require_developer_mode().await?;
        let cache = cache_dir(&self.data_dir);
        Ok(list_suites()
            .into_iter()
            .map(|suite| descriptor_view(suite, &cache))
            .collect())
    }

    pub async fn pull_dataset(
        &self,
        suite: &str,
        limit: Option<usize>,
    ) -> Result<PullEvalDatasetResponse, AppError> {
        self.require_developer_mode().await?;
        let cache = cache_dir(&self.data_dir);
        let manifest = load_suite_manifest(suite, &cache, limit)
            .await
            .map_err(dataset_error)?;
        Ok(PullEvalDatasetResponse {
            suite: manifest.suite,
            corpus_version: manifest.corpus_version,
            cases: manifest.cases.len(),
        })
    }

    pub async fn start_run(
        &self,
        request: StartEvalRunRequest,
        user_id: Option<String>,
    ) -> Result<EvalRunView, AppError> {
        self.require_developer_mode().await?;
        let suite = request.suite.trim().to_owned();
        if suite.is_empty() {
            return Err(AppError::BadRequest("suite is required".into()));
        }

        let (provider_id, model) = self.resolve_model(request.provider_id, request.model).await?;
        let mut guard = self.inner.lock().await;
        if let Some(active) = &guard.active {
            let snapshot = lock_snapshot(&active.snapshot);
            if is_in_flight(&snapshot.status) {
                return Err(AppError::Conflict(
                    "an evaluation run is already in progress".into(),
                ));
            }
        }

        let run_id = Uuid::now_v7().to_string();
        let runs_dir = self.runs_dir();
        fs::create_dir_all(&runs_dir)
            .map_err(|e| AppError::Internal(format!("eval runs dir: {e}")))?;
        let workspace_label = eval_run_workspace_label(&suite, &run_id);
        let work_root = self
            .data_dir
            .join("diagnostics/agent-evals/workspaces")
            .join(&workspace_label);
        fs::create_dir_all(&work_root)
            .map_err(|e| AppError::Internal(format!("eval work root: {e}")))?;
        let workspace_path = work_root.to_string_lossy().into_owned();

        let view = EvalRunView {
            run_id: run_id.clone(),
            status: "loading".into(),
            suite: suite.clone(),
            model: Some(model.clone()),
            provider_id: Some(provider_id.clone()),
            planned: 0,
            completed: 0,
            passed: 0,
            failed: 0,
            current_case_id: None,
            error: None,
            summary: None,
            cases: Vec::new(),
            current_trace: None,
            current_conversation_id: None,
            workspace_label: Some(workspace_label.clone()),
            workspace_path: Some(workspace_path),
        };
        let snapshot = Arc::new(Mutex::new(view.clone()));
        let cancel = Arc::new(AtomicBool::new(false));
        let live_trace = Arc::new(Mutex::new(None));
        let traces_dir = traces_dir_for(&runs_dir, &run_id);
        fs::create_dir_all(&traces_dir)
            .map_err(|e| AppError::Internal(format!("eval traces dir: {e}")))?;
        guard.active = Some(ActiveRun {
            snapshot: snapshot.clone(),
            cancel: cancel.clone(),
            live_trace: live_trace.clone(),
        });
        drop(guard);

        let lab_data_dir = self.data_dir.clone();
        let provider_repo = self.provider_repo.clone();
        let provider_model_repo = self.provider_model_repo.clone();
        let encryption_key = self.encryption_key;
        let session_bridge = self.session_bridge.clone();
        let output = runs_dir.join(format!("{run_id}.jsonl"));
        let summary_path = runs_dir.join(format!("{run_id}.summary.json"));
        let summary_path_for_fail = summary_path.clone();
        let limit = request.limit;
        let task_profile = request.task_profile.clone();
        let n_trials = request
            .n_trials
            .filter(|n| *n >= 1)
            .unwrap_or_else(|| default_trials_for_suite(&suite));
        let quality_sink = self
            .quality_sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        tokio::spawn(async move {
            let result = run_eval_job(RunEvalJob {
                data_dir: lab_data_dir,
                provider_repo,
                provider_model_repo,
                encryption_key,
                suite,
                provider_id,
                model,
                limit,
                task_profile,
                work_root,
                run_workspace_label: workspace_label,
                traces_dir,
                output,
                summary_path,
                snapshot: snapshot.clone(),
                cancel,
                live_trace,
                run_id,
                user_id,
                session_bridge,
                n_trials,
                quality_sink,
            })
            .await;
            if let Err(error) = result {
                let view = {
                    let mut view = lock_snapshot(&snapshot);
                    view.status = "failed".into();
                    view.error = Some(error.to_string());
                    view.clone()
                };
                let _ = persist_summary(&summary_path_for_fail, &view);
            }
        });

        Ok(view)
    }

    pub async fn current_or_get(&self, run_id: &str) -> Result<EvalRunView, AppError> {
        self.require_developer_mode().await?;
        let live = {
            let guard = self.inner.lock().await;
            if let Some(active) = guard.active.as_ref() {
                let snapshot = lock_snapshot(&active.snapshot).clone();
                if snapshot.run_id == run_id {
                    Some((snapshot, active.live_trace.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some((mut snapshot, live_trace)) = live {
            attach_live_trace(&mut snapshot, &live_trace);
            return Ok(snapshot);
        }
        self.load_persisted(run_id)
    }

    pub async fn latest(&self) -> Result<Option<EvalRunView>, AppError> {
        self.require_developer_mode().await?;
        let live = {
            let guard = self.inner.lock().await;
            guard.active.as_ref().map(|active| {
                (
                    lock_snapshot(&active.snapshot).clone(),
                    active.live_trace.clone(),
                )
            })
        };
        if let Some((mut snapshot, live_trace)) = live {
            attach_live_trace(&mut snapshot, &live_trace);
            return Ok(Some(snapshot));
        }
        Ok(self.load_latest_persisted())
    }

    pub async fn history(&self) -> Result<Vec<EvalRunListItem>, AppError> {
        self.require_developer_mode().await?;
        Ok(list_summary_files(&self.runs_dir())
            .into_iter()
            .rev()
            .filter_map(|path| {
                let run_id = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".summary.json"))?
                    .to_owned();
                let view: EvalRunView = fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok())?;
                Some(EvalRunListItem {
                    run_id,
                    suite: view.suite,
                    status: view.status,
                    passed: view.passed,
                    failed: view.failed,
                    pass_at_1: view.summary.as_ref().map(|s| s.pass_at_1).unwrap_or(0.0),
                })
            })
            .collect())
    }

    pub async fn diff_runs(&self, a: &str, b: &str) -> Result<EvalRunDiffView, AppError> {
        self.require_developer_mode().await?;
        let left = self.load_persisted(a)?;
        let right = self.load_persisted(b)?;
        let left_map = case_pass_map(&left.cases);
        let right_map = case_pass_map(&right.cases);
        let mut ids: Vec<_> = left_map.keys().chain(right_map.keys()).cloned().collect();
        ids.sort();
        ids.dedup();
        let flipped = ids
            .into_iter()
            .filter_map(|case_id| {
                let a_success = *left_map.get(&case_id).unwrap_or(&false);
                let b_success = *right_map.get(&case_id).unwrap_or(&false);
                (a_success != b_success).then_some(EvalCaseFlip {
                    case_id,
                    a_success,
                    b_success,
                })
            })
            .collect();
        let pass_at_1_delta = right
            .summary
            .as_ref()
            .map(|s| s.pass_at_1)
            .unwrap_or(0.0)
            - left.summary.as_ref().map(|s| s.pass_at_1).unwrap_or(0.0);
        Ok(EvalRunDiffView {
            a: a.to_owned(),
            b: b.to_owned(),
            flipped,
            pass_at_1_delta,
        })
    }

    pub async fn report_case(&self, case: EvalQualityCase) -> Result<(), AppError> {
        self.require_developer_mode().await?;
        let sink = self
            .quality_sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| AppError::BadRequest("cloud quality sink is not configured".into()))?;
        sink.report_badcase(case).await
    }

    pub async fn sync_private_corpus(&self) -> Result<usize, AppError> {
        self.require_developer_mode().await?;
        let sink = self
            .quality_sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| AppError::BadRequest("cloud quality sink is not configured".into()))?;
        let items = sink.fetch_promoted().await?;
        let dir = private_corpus_dir(&self.data_dir);
        fs::create_dir_all(&dir)
            .map_err(|e| AppError::Internal(format!("private corpus dir: {e}")))?;
        let cases: Vec<Case> = items
            .into_iter()
            .filter_map(promoted_to_case)
            .collect();
        let written = cases.len();
        let manifest = Manifest {
            schema_version: SCHEMA_VERSION,
            corpus_version: "private-promoted".into(),
            suite: "private_badcases".into(),
            cases,
        };
        fs::write(
            dir.join("promoted.json"),
            serde_json::to_string_pretty(&manifest)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        )
        .map_err(|e| AppError::Internal(format!("write private corpus: {e}")))?;
        Ok(written)
    }

    pub async fn get_case_trace(
        &self,
        run_id: &str,
        case_id: &str,
        trial: Option<u32>,
    ) -> Result<EvalCaseTraceView, AppError> {
        self.require_developer_mode().await?;
        let live = {
            let guard = self.inner.lock().await;
            match &guard.active {
                Some(active) if lock_snapshot(&active.snapshot).run_id == run_id => {
                    Some(active.live_trace.clone())
                }
                _ => None,
            }
        };
        if let Some(live) = live {
            let copy = live.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(slot) = copy.filter(|trace| trace.case_id == case_id) {
                return Ok(trace_view(slot.snapshot()));
            }
        }
        let path = resolve_trace_path(
            &traces_dir_for(&self.runs_dir(), run_id),
            case_id,
            trial,
        );
        if !path.exists() {
            return Err(AppError::NotFound(format!(
                "eval trace {run_id}/{case_id} not found"
            )));
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| AppError::Internal(format!("read eval trace: {e}")))?;
        let trace: EvalCaseTrace = serde_json::from_str(&text)
            .map_err(|e| AppError::Internal(format!("parse eval trace: {e}")))?;
        Ok(trace_view(trace))
    }

    /// Session Observation projection for one eval case (same hub as real sessions).
    pub async fn get_case_observation(
        &self,
        run_id: &str,
        case_id: &str,
        limit: Option<usize>,
    ) -> Result<SessionObservationList, AppError> {
        self.require_developer_mode().await?;
        let conversation_id = self.resolve_case_conversation_id(run_id, case_id).await?;
        let hub = self.trace_hub.as_ref().ok_or_else(|| {
            AppError::Internal("session observation hub is not configured for eval lab".into())
        })?;
        hub.list_session_observations(&conversation_id, limit.unwrap_or(50).clamp(1, 200))
            .await
            .map_err(|error| error.into_app_error())
    }

    async fn resolve_case_conversation_id(
        &self,
        run_id: &str,
        case_id: &str,
    ) -> Result<String, AppError> {
        let live = {
            let guard = self.inner.lock().await;
            match &guard.active {
                Some(active) if lock_snapshot(&active.snapshot).run_id == run_id => {
                    Some(active.live_trace.clone())
                }
                _ => None,
            }
        };
        if let Some(live) = live {
            let copy = live.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(slot) = copy.filter(|trace| trace.case_id == case_id) {
                return Ok(slot.conversation_id);
            }
        }
        let path = resolve_trace_path(
            &traces_dir_for(&self.runs_dir(), run_id),
            case_id,
            None,
        );
        if path.exists() {
            let text = fs::read_to_string(&path)
                .map_err(|e| AppError::Internal(format!("read eval trace: {e}")))?;
            let trace: EvalCaseTrace = serde_json::from_str(&text)
                .map_err(|e| AppError::Internal(format!("parse eval trace: {e}")))?;
            if let Some(conversation_id) = trace.conversation_id.filter(|id| !id.trim().is_empty()) {
                return Ok(conversation_id);
            }
        }
        let cases = load_case_views(
            &self.runs_dir().join(format!("{run_id}.jsonl")),
            &traces_dir_for(&self.runs_dir(), run_id),
        )?;
        cases
            .into_iter()
            .find(|case| case.case_id == case_id)
            .and_then(|case| case.conversation_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "eval case {run_id}/{case_id} has no conversation_id yet"
                ))
            })
    }

    pub async fn cancel(&self, run_id: &str) -> Result<EvalRunView, AppError> {
        self.require_developer_mode().await?;
        let guard = self.inner.lock().await;
        let Some(active) = guard.active.as_ref() else {
            return Err(AppError::NotFound("no evaluation run is active".into()));
        };
        let mut snapshot = lock_snapshot(&active.snapshot);
        if snapshot.run_id != run_id {
            return Err(AppError::NotFound(format!("eval run {run_id} is not active")));
        }
        if !is_in_flight(&snapshot.status) {
            return Ok(snapshot.clone());
        }
        active.cancel.store(true, Ordering::Relaxed);
        snapshot.status = "cancelling".into();
        Ok(snapshot.clone())
    }

    async fn resolve_model(
        &self,
        provider_id: Option<String>,
        model: Option<String>,
    ) -> Result<(String, String), AppError> {
        match (provider_id, model) {
            (Some(provider_id), Some(model))
                if !provider_id.trim().is_empty() && !model.trim().is_empty() =>
            {
                Ok((provider_id, model))
            }
            (None, None) => resolve_default_model(&self.provider_repo, &self.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::ProviderUnavailable("no enabled chat model is configured".into())
                }),
            _ => Err(AppError::BadRequest(
                "provider_id and model must be provided together".into(),
            )),
        }
    }

    fn runs_dir(&self) -> PathBuf {
        self.data_dir.join("diagnostics/agent-evals/runs")
    }

    fn load_persisted(&self, run_id: &str) -> Result<EvalRunView, AppError> {
        let path = self.runs_dir().join(format!("{run_id}.summary.json"));
        if !path.exists() {
            return Err(AppError::NotFound(format!("eval run {run_id} not found")));
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| AppError::Internal(format!("read eval summary: {e}")))?;
        let mut view: EvalRunView = serde_json::from_str(&text)
            .map_err(|e| AppError::Internal(format!("parse eval summary: {e}")))?;
        overlay_trace_flags(&mut view, &traces_dir_for(&self.runs_dir(), run_id));
        Ok(view)
    }

    fn load_latest_persisted(&self) -> Option<EvalRunView> {
        let dir = self.runs_dir();
        let path = list_summary_files(&dir).pop()?;
        let mut view: EvalRunView = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())?;
        if let Some(run_id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".summary.json"))
        {
            overlay_trace_flags(&mut view, &traces_dir_for(&dir, run_id));
        }
        Some(view)
    }
}

fn is_in_flight(status: &str) -> bool {
    matches!(status, "loading" | "queued" | "running" | "cancelling")
}

struct RunEvalJob {
    data_dir: PathBuf,
    provider_repo: Arc<dyn IProviderRepository>,
    provider_model_repo: Arc<dyn IProviderModelRepository>,
    encryption_key: [u8; 32],
    suite: String,
    provider_id: String,
    model: String,
    limit: Option<usize>,
    task_profile: Option<String>,
    work_root: PathBuf,
    run_workspace_label: String,
    traces_dir: PathBuf,
    output: PathBuf,
    summary_path: PathBuf,
    snapshot: Arc<Mutex<EvalRunView>>,
    cancel: Arc<AtomicBool>,
    live_trace: Arc<Mutex<Option<LiveEvalTrace>>>,
    run_id: String,
    user_id: Option<String>,
    session_bridge: Option<Arc<dyn EvalSessionBridge>>,
    n_trials: u32,
    quality_sink: Option<Arc<dyn EvalQualitySink>>,
}

async fn run_eval_job(job: RunEvalJob) -> Result<(), AppError> {
    let cache = cache_dir(&job.data_dir);
    let manifest = load_suite_manifest(&job.suite, &cache, job.limit)
        .await
        .map_err(dataset_error)?;
    {
        let mut view = lock_snapshot(&job.snapshot);
        view.status = "running".into();
        let enabled = manifest.cases.iter().filter(|c| c.enabled).count();
        view.planned = enabled.saturating_mul(job.n_trials.max(1) as usize);
    }

    let harness = Arc::new(LiveNomiHarness {
        data_dir: job.data_dir.clone(),
        provider_repo: job.provider_repo,
        provider_model_repo: job.provider_model_repo,
        encryption_key: job.encryption_key,
        work_root: job.work_root,
        run_workspace_label: job.run_workspace_label,
        traces_dir: job.traces_dir.clone(),
        provider_id: job.provider_id.clone(),
        model: job.model.clone(),
        profile_override: job.task_profile.clone(),
        live_trace: job.live_trace,
        run_id: job.run_id,
        suite: job.suite.clone(),
        user_id: job.user_id,
        session_bridge: job.session_bridge,
    });

    let snapshot = job.snapshot.clone();
    let output_for_progress = job.output.clone();
    let traces_for_progress = job.traces_dir.clone();
    let on_progress = move |progress: RunProgress| {
        let mut view = lock_snapshot(&snapshot);
        view.current_case_id = Some(progress.case_id.clone());
        view.planned = progress.total;
        match progress.phase {
            RunProgressPhase::Cancelled => {
                view.status = "cancelling".into();
            }
            RunProgressPhase::Scored => {
                if let Ok(cases) = load_case_views(&output_for_progress, &traces_for_progress) {
                    view.cases = cases;
                    view.completed = view.cases.len();
                    view.passed = view.cases.iter().filter(|c| c.success).count();
                    view.failed = view.cases.iter().filter(|c| !c.success).count();
                }
            }
            RunProgressPhase::Started => {}
        }
    };

    let report = run_loaded_manifest(
        RunConfig {
            manifest: PathBuf::from("in-memory"),
            output: job.output.clone(),
            tag: Some("live".into()),
            resume: false,
            cancel: Some(job.cancel.clone()),
            case_limit: job.limit,
            model: Some(job.model.clone()),
            provider_id: Some(job.provider_id.clone()),
            harness_profile: job.task_profile.clone().or_else(|| Some("live".into())),
            n_trials: job.n_trials.max(1),
        },
        &manifest,
        harness,
        Some(&on_progress),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let summary = summarize(&[job.output.clone()], None)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let cases = load_case_views(&job.output, &job.traces_dir)?;
    let status = if report.cancelled {
        "cancelled"
    } else {
        "completed"
    };
    let view = {
        let mut view = lock_snapshot(&job.snapshot);
        view.status = status.into();
        view.planned = report.planned;
        view.completed = report.completed;
        view.passed = report.passed;
        view.failed = report.failed;
        view.current_case_id = None;
        view.current_trace = None;
        view.current_conversation_id = None;
        view.cases = cases;
        view.summary = Some(summary_view(&summary));
        view.clone()
    };
    persist_summary(&job.summary_path, &view)?;
    prune_old_runs(job.summary_path.parent().unwrap_or(Path::new(".")), 50);
    if let Some(sink) = job.quality_sink {
        let report = quality_report_from_view(&view);
        tokio::spawn(async move {
            if let Err(error) = sink.report_failed_cases(report).await {
                tracing::warn!(error = %error, "eval quality auto-report failed");
            }
        });
    }
    Ok(())
}

fn persist_summary(path: &Path, view: &EvalRunView) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::Internal(e.to_string()))?;
    }
    let mut persisted = view.clone();
    persisted.current_trace = None;
    persisted.current_conversation_id = None;
    fs::write(
        path,
        serde_json::to_string_pretty(&persisted).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .map_err(|e| AppError::Internal(e.to_string()))
}

fn load_case_views(path: &Path, traces_dir: &Path) -> Result<Vec<EvalCaseView>, AppError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|e| AppError::Internal(e.to_string()))?;
    let mut cases = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: EvalResult =
            serde_json::from_str(trimmed).map_err(|e| AppError::Internal(e.to_string()))?;
        let trial = if row.trial == 0 { 1 } else { row.trial };
        let has_trace = resolve_trace_path(traces_dir, &row.case_id, Some(trial)).exists();
        cases.push(EvalCaseView {
            case_id: row.case_id,
            category: row.category,
            success: row.success,
            elapsed_ms: row.elapsed_ms,
            turns: row.turns,
            tool_call_count: row.tool_call_count,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            tool_error_count: row.tool_error_count,
            stop_reason: row.stop_reason,
            error: row.error,
            scorer_results: scorer_views(row.scorer_results),
            advisory_results: scorer_views(row.advisory_results),
            trial,
            prompt: Some(row.prompt),
            trajectory_event_count: row.trajectory_event_count,
            artifact_count: row.artifact_count,
            has_trace,
            conversation_id: row.conversation_id,
        });
    }
    Ok(cases)
}

fn traces_dir_for(runs_dir: &Path, run_id: &str) -> PathBuf {
    runs_dir.join(run_id).join("traces")
}

fn resolve_trace_path(dir: &Path, case_id: &str, trial: Option<u32>) -> PathBuf {
    let named = dir.join(trace_file_name(case_id, trial.unwrap_or(1)));
    if named.exists() {
        return named;
    }
    dir.join(format!("{}.json", sanitize_case_dir(case_id)))
}

fn overlay_trace_flags(view: &mut EvalRunView, traces_dir: &Path) {
    for case in &mut view.cases {
        case.has_trace = resolve_trace_path(traces_dir, &case.case_id, Some(case.trial)).exists();
    }
}

fn scorer_views(rows: Vec<ScorerResult>) -> Vec<EvalScorerView> {
    rows.into_iter()
        .map(|s| EvalScorerView {
            scorer_type: s.scorer_type,
            passed: s.passed,
            detail: s.detail,
        })
        .collect()
}

fn summary_view(summary: &Summary) -> EvalSummaryView {
    EvalSummaryView {
        total_cases: summary.total_cases,
        passed: summary.passed,
        failed: summary.failed,
        success_rate: summary.success_rate,
        avg_turns: summary.avg_turns,
        avg_elapsed_ms: summary.avg_elapsed_ms,
        avg_input_tokens: summary.avg_input_tokens,
        avg_output_tokens: summary.avg_output_tokens,
        by_category: summary
            .by_category
            .iter()
            .map(|row| EvalCategoryView {
                category: row.category.clone(),
                total: row.total,
                passed: row.passed,
                success_rate: row.success_rate,
            })
            .collect(),
        unique_cases: summary.unique_cases,
        n_trials: summary.n_trials,
        pass_at_1: summary.pass_at_1,
        pass_hat_k: summary.pass_hat_k,
    }
}

fn case_pass_map(cases: &[EvalCaseView]) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for case in cases {
        *map.entry(case.case_id.clone()).or_insert(false) |= case.success;
    }
    map
}

fn list_summary_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".summary.json"))
        })
        .collect();
    files.sort();
    files
}

fn prune_old_runs(runs_dir: &Path, keep: usize) {
    let files = list_summary_files(runs_dir);
    if files.len() <= keep {
        return;
    }
    for path in files.iter().take(files.len() - keep) {
        let Some(run_id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".summary.json"))
        else {
            continue;
        };
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(runs_dir.join(format!("{run_id}.jsonl")));
        let _ = fs::remove_dir_all(runs_dir.join(run_id));
    }
}

fn quality_report_from_view(view: &EvalRunView) -> EvalQualityReport {
    let mut seen = HashSet::new();
    let cases = view
        .cases
        .iter()
        .filter(|case| !case.success && seen.insert(case.case_id.clone()))
        .map(|case| EvalQualityCase {
            case_id: case.case_id.clone(),
            suite: view.suite.clone(),
            category: case.category.clone(),
            error: case.error.clone(),
            prompt: case.prompt.clone().unwrap_or_default(),
            scorer_json: serde_json::to_string(&case.scorer_results).unwrap_or_else(|_| "[]".into()),
        })
        .collect();
    let summary = view.summary.as_ref();
    EvalQualityReport {
        run_id: view.run_id.clone(),
        suite: view.suite.clone(),
        model: view.model.clone(),
        pass_at_1: summary.map(|s| s.pass_at_1).unwrap_or(0.0),
        pass_hat_k: summary.map(|s| s.pass_hat_k).unwrap_or(0.0),
        passed: summary.map(|s| s.passed as u32).unwrap_or(0),
        failed: summary.map(|s| s.failed as u32).unwrap_or(0),
        unique_cases: summary.map(|s| s.unique_cases as u32).unwrap_or(0),
        n_trials: summary.map(|s| s.n_trials).unwrap_or(1),
        cases,
    }
}

fn promoted_to_case(item: EvalQualityCase) -> Option<Case> {
    let prompt = item.prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    let scorers = serde_json::from_str::<Vec<ScorerSpec>>(&item.scorer_json)
        .ok()
        .filter(|specs| !specs.is_empty())
        .unwrap_or_else(|| vec![ScorerSpec::ToolCalled { name: "write".into() }]);
    let id = if item.case_id.trim().is_empty() {
        format!("promoted-{}", Uuid::now_v7())
    } else {
        item.case_id
    };
    Some(Case {
        id,
        category: if item.category.trim().is_empty() {
            "private".into()
        } else {
            item.category
        },
        prompt: prompt.to_owned(),
        enabled: true,
        budgets: Default::default(),
        scorers,
        advisory_scorers: Vec::new(),
        isolation: Some("office".into()),
        trial: 0,
        notes: Some("promoted from agent_quality".into()),
        task_profile: Some("office".into()),
        workspace_files: Default::default(),
        timeout_secs: None,
    })
}

fn attach_live_trace(view: &mut EvalRunView, live: &Mutex<Option<LiveEvalTrace>>) {
    let copy = live
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(trace) = copy {
        let case_id = trace.case_id.clone();
        view.current_conversation_id = Some(trace.conversation_id.clone());
        view.current_trace = Some(trace_view(trace.snapshot()));
        if view.current_case_id.is_none() {
            view.current_case_id = Some(case_id);
        }
    } else {
        view.current_trace = None;
        view.current_conversation_id = None;
    }
}

fn trace_view(trace: EvalCaseTrace) -> EvalCaseTraceView {
    EvalCaseTraceView {
        case_id: trace.case_id,
        live: trace.live,
        assistant_text: trace.assistant_text,
        events: trace
            .events
            .into_iter()
            .map(|event| EvalTrajectoryEventView {
                kind: event.kind,
                ts_ms: event.ts_ms,
                tool_use_id: event.tool_use_id,
                name: event.name,
                input: event.input,
                content: event.content,
                is_error: event.is_error,
            })
            .collect(),
        artifacts: trace
            .artifacts
            .into_iter()
            .map(|artifact| EvalArtifactView {
                path: artifact.path,
                size_bytes: artifact.size_bytes,
                kind: artifact.kind,
                preview: artifact.preview,
            })
            .collect(),
        conversation_id: trace.conversation_id,
    }
}

fn descriptor_view(suite: SuiteDescriptor, cache: &Path) -> EvalSuiteDescriptor {
    let cached = nomi_agent_eval::is_download_cached(&suite.id, cache);
    EvalSuiteDescriptor {
        id: suite.id,
        title: suite.title,
        kind: suite.kind,
        default_task_profile: suite.default_task_profile,
        source_url: suite.source_url,
        default_limit: suite.default_limit,
        max_limit: suite.max_limit,
        notes: suite.notes,
        requires_download: suite.requires_download,
        cached,
        tier: suite.tier,
        default_trials: suite.default_trials,
        requires_sandbox: suite.requires_sandbox,
    }
}

fn dataset_error(error: nomi_agent_eval::DatasetError) -> AppError {
    match error {
        nomi_agent_eval::DatasetError::UnknownSuite(suite) => {
            AppError::BadRequest(format!("unknown suite {suite}"))
        }
        nomi_agent_eval::DatasetError::Download { url, message } => {
            AppError::BadGateway(format!("failed to download {url}: {message}"))
        }
        nomi_agent_eval::DatasetError::RequiresSandbox(suite) => AppError::BadRequest(format!(
            "suite {suite} requires a sandbox runner (not implemented)"
        )),
        nomi_agent_eval::DatasetError::EmptySuite(suite) => {
            AppError::BadRequest(format!("suite {suite} has no cases"))
        }
        other => AppError::Internal(other.to_string()),
    }
}
