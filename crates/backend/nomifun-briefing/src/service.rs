use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomi_briefing::{
    briefing_event_name, BeatScript, BriefingService, BriefingTerminalTelemetry, ResearchDepth,
    ResearchPlan, RunSnapshot, SessionRecord, SessionSummary,
};
use nomifun_api_types::{
    BriefingSessionListResponse, BriefingSessionSummary, VideoGrowthEvent,
    VideoGrowthEventBatchRequest,
};
use nomifun_cloud::CloudService;
use nomifun_common::AppError;
use nomifun_db::{IClientPreferenceRepository, IProviderModelRepository};
use nomifun_model_invoke::ModelInvokeService;
use serde_json::json;
use tracing::warn;

use crate::search::DdgSourceRetriever;
use crate::stills::InvokeStillSynth;
use crate::tts::InvokeVoiceSynth;

pub struct BriefingApiService {
    inner: Arc<BriefingService>,
}

impl BriefingApiService {
    pub fn new(data_dir: PathBuf) -> Result<Self, AppError> {
        let inner =
            BriefingService::open(&data_dir).map_err(|e| AppError::Internal(e.to_string()))?;
        let hook_dir = data_dir.clone();
        inner.set_terminal_telemetry_hook(Some(Arc::new(move |payload| {
            let data_dir = hook_dir.clone();
            tokio::spawn(async move {
                if let Err(err) = upload_briefing_terminal_telemetry(data_dir, payload).await {
                    warn!(error = %err, "briefing terminal telemetry upload failed");
                }
            });
        })));
        match DdgSourceRetriever::try_new() {
            Ok(retriever) => inner.set_source_retriever(Some(Arc::new(retriever))),
            Err(err) => warn!(error = %err, "briefing web research unavailable; URLs remain optional but HOLD if coverage is short"),
        }
        Ok(Self { inner })
    }

    pub fn attach_voice(
        &self,
        invoke: Arc<ModelInvokeService>,
        prefs: Arc<dyn IClientPreferenceRepository>,
        models: Arc<dyn IProviderModelRepository>,
    ) {
        self.inner
            .set_voice_synth(Some(Arc::new(InvokeVoiceSynth::new(
                invoke.clone(),
                prefs,
                models,
            ))));
        self.inner
            .set_still_synth(Some(Arc::new(InvokeStillSynth::new(invoke))));
    }

    pub fn list_summaries(&self) -> Result<Vec<BriefingSessionSummary>, AppError> {
        self.inner
            .list_summaries()
            .map(|rows| rows.into_iter().map(summary_from).collect())
            .map_err(map_err)
    }

    pub fn list_response(&self) -> Result<BriefingSessionListResponse, AppError> {
        Ok(BriefingSessionListResponse {
            sessions: self.list_summaries()?,
        })
    }

    pub fn create(
        &self,
        intent: &str,
        title: Option<String>,
        format_secs: u32,
        depth: &str,
        time_window_hours: u32,
        source_urls: Vec<String>,
        tts_provider_id: Option<String>,
        tts_model: Option<String>,
        tts_voice: Option<String>,
        image_provider_id: Option<String>,
        image_model: Option<String>,
    ) -> Result<SessionRecord, AppError> {
        let depth = ResearchDepth::parse(depth).unwrap_or(ResearchDepth::Fast);
        self.inner
            .create(nomi_briefing::CreateBriefingInput {
                intent: intent.to_string(),
                title,
                format_secs,
                depth,
                time_window_hours,
                source_urls,
                tts_provider_id,
                tts_model,
                tts_voice,
                image_provider_id,
                image_model,
            })
            .map_err(map_err)
    }

    pub fn update_models(
        &self,
        id: &str,
        tts_provider_id: Option<String>,
        tts_model: Option<String>,
        tts_voice: Option<String>,
        image_provider_id: Option<String>,
        image_model: Option<String>,
    ) -> Result<SessionRecord, AppError> {
        self.inner
            .update_models(
                id,
                tts_provider_id,
                tts_model,
                tts_voice,
                image_provider_id,
                image_model,
            )
            .map_err(map_err)
    }

    pub fn get(&self, id: &str) -> Result<SessionRecord, AppError> {
        self.inner.get(id).map_err(map_err)
    }

    pub fn status(&self, id: &str) -> Result<RunSnapshot, AppError> {
        self.inner.status(id).map_err(map_err)
    }

    pub fn confirm_plan(&self, id: &str, plan: Option<ResearchPlan>) -> Result<ResearchPlan, AppError> {
        self.inner.confirm_plan(id, plan).map_err(map_err)
    }

    pub fn load_plan(&self, id: &str) -> Result<ResearchPlan, AppError> {
        self.inner.load_plan(id).map_err(map_err)
    }

    pub fn save_script(&self, id: &str, script: BeatScript) -> Result<BeatScript, AppError> {
        self.inner.save_script(id, script).map_err(map_err)
    }

    pub fn load_script(&self, id: &str) -> Result<BeatScript, AppError> {
        self.inner.load_script(id).map_err(map_err)
    }

    pub fn start_run(&self, id: &str, confirm_plan: bool) -> Result<(), AppError> {
        self.inner.start_run(id, confirm_plan).map_err(map_err)
    }

    pub fn cancel(&self, id: &str) {
        self.inner.cancel_run(id);
    }

    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        self.inner.delete(id).map_err(map_err)
    }

    pub fn working_dir(&self, id: &str) -> Result<PathBuf, AppError> {
        self.inner.working_dir(id).map_err(map_err)
    }

    pub async fn interrupt_all(&self) -> usize {
        self.inner.interrupt_all().await
    }
}

fn summary_from(row: SessionSummary) -> BriefingSessionSummary {
    BriefingSessionSummary {
        id: row.id,
        title: row.title,
        stage: row.stage,
        status: row.status.as_str().to_string(),
        final_video: row.final_video,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn map_err(err: nomi_briefing::BriefingError) -> AppError {
    match err {
        nomi_briefing::BriefingError::SessionNotFound(id) => {
            AppError::NotFound(format!("briefing {id}"))
        }
        nomi_briefing::BriefingError::ArtifactNotFound(name) => {
            AppError::NotFound(format!("briefing artifact {name}"))
        }
        nomi_briefing::BriefingError::InvalidParams(m) | nomi_briefing::BriefingError::Hold(m) => {
            AppError::BadRequest(m)
        }
        nomi_briefing::BriefingError::Voice { message, .. } => AppError::BadRequest(message),
        other => AppError::Internal(other.to_string()),
    }
}

async fn upload_briefing_terminal_telemetry(
    data_dir: PathBuf,
    payload: BriefingTerminalTelemetry,
) -> Result<(), AppError> {
    let Some(name) = briefing_event_name(payload.status) else {
        return Ok(());
    };
    let cloud = CloudService::new(data_dir)?;
    if !cloud.is_authenticated().await {
        return Ok(());
    }
    let briefing_id = payload.briefing_id.clone();
    let mut properties = BTreeMap::new();
    properties.insert("briefing_id".into(), json!(briefing_id.clone()));
    properties.insert("session_id".into(), json!(briefing_id.clone()));
    properties.insert("feature".into(), json!("video_generation"));
    properties.insert("mode".into(), json!("briefing"));
    properties.insert("workflow".into(), json!("news_briefing"));
    properties.insert("runtime".into(), json!("desktop"));
    properties.insert("status".into(), json!(payload.status.as_str()));
    properties.insert("research_depth".into(), json!(payload.research_depth));
    properties.insert("beat_count".into(), json!(payload.beat_count));
    properties.insert("citation_count".into(), json!(payload.citation_count));
    properties.insert("credits_consumed".into(), json!(payload.credits_consumed));
    properties.insert("duration_ms".into(), json!(payload.duration_ms));
    if let Some(error_code) = payload.error_code.filter(|value| !value.is_empty()) {
        properties.insert("error_code".into(), json!(error_code));
    }
    cloud
        .upload_video_growth_events(&VideoGrowthEventBatchRequest {
            events: vec![VideoGrowthEvent {
                event_id: format!("briefing:{name}:{briefing_id}"),
                name: name.to_string(),
                occurred_at: payload.occurred_at,
                module: Some("video_generation".into()),
                properties,
                cohort: None,
            }],
            client_id: None,
            app: None,
            platform: None,
            app_version: None,
        })
        .await?;
    Ok(())
}

pub fn safe_artifact_path(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let joined = root.join(rel);
    let canon_root = std::fs::canonicalize(root)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let canon = std::fs::canonicalize(&joined).map_err(|_| AppError::NotFound(rel.into()))?;
    if !canon.starts_with(&canon_root) {
        return Err(AppError::BadRequest("artifact path escapes working dir".into()));
    }
    Ok(canon)
}
