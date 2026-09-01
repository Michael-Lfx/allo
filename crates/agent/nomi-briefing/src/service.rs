use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::error::{BriefingError, BriefingResult};
use crate::ir::{BeatScript, ResearchDepth, ResearchPlan};
use crate::pipeline::{run_pipeline, PipelineOutcome};
use crate::progress::{
    briefing_event_name, BriefingTerminalTelemetry, RunSnapshot, RunStatus,
};
use crate::research::{draft_plan, SourceRetriever};
use crate::session::{SessionIndex, SessionRecord, SessionSummary};
use crate::stills::StillSynth;
use crate::voice::{TtsChoice, VoiceSynth};

pub type TerminalTelemetryHook = Arc<dyn Fn(BriefingTerminalTelemetry) + Send + Sync>;
pub type VoiceSynthHook = Arc<dyn VoiceSynth>;
pub type StillSynthHook = Arc<dyn StillSynth>;
pub type SourceRetrieverHook = Arc<dyn SourceRetriever>;

#[derive(Debug, Clone, Default)]
pub struct CreateBriefingInput {
    pub intent: String,
    pub title: Option<String>,
    pub format_secs: u32,
    pub depth: ResearchDepth,
    pub time_window_hours: u32,
    pub source_urls: Vec<String>,
    pub tts_provider_id: Option<String>,
    pub tts_model: Option<String>,
    pub tts_voice: Option<String>,
    pub image_provider_id: Option<String>,
    pub image_model: Option<String>,
}

pub struct BriefingService {
    index: SessionIndex,
    cancel: StdMutex<HashMap<String, CancellationToken>>,
    telemetry: StdMutex<Option<TerminalTelemetryHook>>,
    voice: StdMutex<Option<VoiceSynthHook>>,
    stills: StdMutex<Option<StillSynthHook>>,
    retriever: StdMutex<Option<SourceRetrieverHook>>,
}

static SERVICES: OnceLock<StdMutex<HashMap<PathBuf, Weak<BriefingService>>>> = OnceLock::new();

impl BriefingService {
    pub fn open(data_dir: &Path) -> BriefingResult<Arc<Self>> {
        let key = data_dir
            .canonicalize()
            .unwrap_or_else(|_| data_dir.to_path_buf());
        let registry = SERVICES.get_or_init(|| StdMutex::new(HashMap::new()));
        {
            let map = registry.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = map.get(&key).and_then(|weak| weak.upgrade()) {
                return Ok(existing);
            }
        }
        let index = SessionIndex::open(data_dir)?;
        let _ = index.reconcile_orphaned_active_runs();
        let service = Arc::new(Self {
            index,
            cancel: StdMutex::new(HashMap::new()),
            telemetry: StdMutex::new(None),
            voice: StdMutex::new(None),
            stills: StdMutex::new(None),
            retriever: StdMutex::new(None),
        });
        let mut map = registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = map.get(&key).and_then(|weak| weak.upgrade()) {
            return Ok(existing);
        }
        map.insert(key, Arc::downgrade(&service));
        Ok(service)
    }

    pub fn set_terminal_telemetry_hook(&self, hook: Option<TerminalTelemetryHook>) {
        *self.telemetry.lock().unwrap_or_else(|e| e.into_inner()) = hook;
    }

    pub fn set_voice_synth(&self, voice: Option<VoiceSynthHook>) {
        *self.voice.lock().unwrap_or_else(|e| e.into_inner()) = voice;
    }

    pub fn set_still_synth(&self, stills: Option<StillSynthHook>) {
        *self.stills.lock().unwrap_or_else(|e| e.into_inner()) = stills;
    }

    pub fn set_source_retriever(&self, retriever: Option<SourceRetrieverHook>) {
        *self.retriever.lock().unwrap_or_else(|e| e.into_inner()) = retriever;
    }

    pub fn list_summaries(&self) -> BriefingResult<Vec<SessionSummary>> {
        self.index.list_summaries()
    }

    pub fn get(&self, id: &str) -> BriefingResult<SessionRecord> {
        self.index.get(id)
    }

    pub fn status(&self, id: &str) -> BriefingResult<RunSnapshot> {
        let record = self.index.get(id)?;
        let mut snapshot = self.index.load_run_status(id);
        snapshot.status = record.status;
        snapshot.stage = record.stage;
        snapshot.final_video = record.final_video;
        Ok(snapshot)
    }

    pub fn create(&self, input: CreateBriefingInput) -> BriefingResult<SessionRecord> {
        let intent = input.intent.trim();
        if intent.is_empty() {
            return Err(BriefingError::InvalidParams("intent is required".into()));
        }
        let format_secs = crate::research::clamp_format_secs(input.format_secs);
        let time_window_hours = if input.time_window_hours == 0 {
            24
        } else {
            input.time_window_hours
        };
        let now = chrono::Local::now().to_rfc3339();
        let title = input
            .title
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| intent.chars().take(36).collect());
        let tts = TtsChoice::from_parts(
            input.tts_provider_id.as_deref(),
            input.tts_model.as_deref(),
            input.tts_voice.as_deref(),
        );
        let image = crate::stills::ImageChoice::from_parts(
            input.image_provider_id.as_deref(),
            input.image_model.as_deref(),
        );
        let depth = input.depth;
        let record = self.index.create(SessionRecord {
            title,
            intent: intent.to_string(),
            format_secs,
            research_depth: depth,
            time_window_hours,
            source_urls: input.source_urls,
            stage: "created".into(),
            created_at: now.clone(),
            updated_at: now,
            tts_provider_id: tts.as_ref().map(|row| row.provider_id.clone()),
            tts_model: tts.as_ref().map(|row| row.model.clone()),
            tts_voice: tts.and_then(|row| row.voice),
            image_provider_id: image.as_ref().map(|row| row.provider_id.clone()),
            image_model: image.map(|row| row.model),
            ..SessionRecord::default()
        })?;
        let plan = draft_plan(&record.intent, record.time_window_hours, depth);
        self.index.save_plan(&record.id, &plan)?;
        Ok(record)
    }

    pub fn update_models(
        &self,
        id: &str,
        tts_provider_id: Option<String>,
        tts_model: Option<String>,
        tts_voice: Option<String>,
        image_provider_id: Option<String>,
        image_model: Option<String>,
    ) -> BriefingResult<SessionRecord> {
        let mut record = self.index.get(id)?;
        if record.status.is_active() {
            return Err(BriefingError::InvalidParams(
                "cannot change models while a briefing is running".into(),
            ));
        }
        let tts = TtsChoice::from_parts(
            tts_provider_id.as_deref(),
            tts_model.as_deref(),
            tts_voice.as_deref(),
        );
        let image = crate::stills::ImageChoice::from_parts(
            image_provider_id.as_deref(),
            image_model.as_deref(),
        );
        record.tts_provider_id = tts.as_ref().map(|row| row.provider_id.clone());
        record.tts_model = tts.as_ref().map(|row| row.model.clone());
        record.tts_voice = tts.and_then(|row| row.voice);
        record.image_provider_id = image.as_ref().map(|row| row.provider_id.clone());
        record.image_model = image.map(|row| row.model);
        record.updated_at = chrono::Local::now().to_rfc3339();
        self.index.upsert(record)
    }

    pub fn confirm_plan(&self, id: &str, plan: Option<ResearchPlan>) -> BriefingResult<ResearchPlan> {
        let record = self.index.get(id)?;
        let mut stored = if let Some(plan) = plan {
            plan
        } else {
            match self.index.load_plan(id) {
                Ok(plan) => plan,
                Err(BriefingError::ArtifactNotFound(_)) => draft_plan(
                    &record.intent,
                    record.time_window_hours,
                    record.research_depth,
                ),
                Err(err) => return Err(err),
            }
        };
        stored.confirmed = true;
        self.index.save_plan(id, &stored)?;
        let mut record = record;
        record.plan_confirmed = true;
        record.updated_at = chrono::Local::now().to_rfc3339();
        self.index.upsert(record)?;
        Ok(stored)
    }

    pub fn save_script(&self, id: &str, script: BeatScript) -> BriefingResult<BeatScript> {
        self.index.save_script(id, &script)?;
        Ok(script)
    }

    pub fn load_script(&self, id: &str) -> BriefingResult<BeatScript> {
        match self.index.load_script(id) {
            Ok(script) => Ok(script),
            Err(BriefingError::ArtifactNotFound(_)) => Ok(BeatScript::default()),
            Err(err) => Err(err),
        }
    }

    pub fn load_plan(&self, id: &str) -> BriefingResult<ResearchPlan> {
        self.index.load_plan(id)
    }

    pub fn working_dir(&self, id: &str) -> BriefingResult<PathBuf> {
        self.index.working_dir(id)
    }

    pub fn delete(&self, id: &str) -> BriefingResult<()> {
        self.cancel_run(id);
        self.index.delete(id)
    }

    pub fn cancel_run(&self, id: &str) {
        if let Some(token) = self
            .cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
        {
            token.cancel();
        }
        if let Ok(mut record) = self.index.get(id) {
            if record.status.is_active() {
                record.status = RunStatus::Cancelled;
                record.summary = "cancelled".into();
                record.updated_at = chrono::Local::now().to_rfc3339();
                let _ = self.index.upsert(record);
            }
        }
    }

    pub async fn interrupt_all(&self) -> usize {
        let ids: Vec<String> = self
            .cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in &ids {
            self.cancel_run(id);
        }
        self.index.reconcile_orphaned_active_runs().unwrap_or(0)
    }

    pub fn start_run(self: &Arc<Self>, id: &str, confirm_plan: bool) -> BriefingResult<()> {
        let record = self.index.get(id)?;
        let token = CancellationToken::new();
        {
            let mut cancel = self.cancel.lock().unwrap_or_else(|e| e.into_inner());
            if cancel.contains_key(id) || record.status.is_active() {
                return Ok(());
            }
            cancel.insert(id.to_string(), token.clone());
        }
        let service = Arc::clone(self);
        let session_id = id.to_string();
        let voice = self
            .voice
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let stills = self
            .stills
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let retriever = self
            .retriever
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        tokio::spawn(async move {
            let started = Instant::now();
            let result = tokio::task::spawn_blocking({
                let index = service.index.clone();
                let session_id = session_id.clone();
                move || {
                    run_pipeline(
                        &index,
                        &session_id,
                        confirm_plan,
                        voice.as_deref(),
                        stills.as_deref(),
                        retriever.as_deref(),
                    )
                }
            })
            .await;
            service
                .cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&session_id);
            if token.is_cancelled() {
                return;
            }
            match result {
                Ok(Ok(outcome)) => service.emit_terminal(&session_id, outcome, started),
                Ok(Err(err)) => {
                    let error_code = err
                        .error_code()
                        .unwrap_or("pipeline")
                        .to_string();
                    let _ = service.fail_run(&session_id, &err);
                    service.emit_terminal(
                        &session_id,
                        PipelineOutcome {
                            status: if matches!(err, BriefingError::Hold(_)) {
                                RunStatus::Hold
                            } else {
                                RunStatus::Failed
                            },
                            error_code: Some(error_code),
                            beat_count: 0,
                            citation_count: 0,
                        },
                        started,
                    );
                }
                Err(join) => {
                    let _ = service.fail_run(
                        &session_id,
                        &BriefingError::Internal(join.to_string()),
                    );
                }
            }
        });
        Ok(())
    }

    fn fail_run(&self, id: &str, err: &BriefingError) -> BriefingResult<()> {
        let mut record = self.index.get(id)?;
        record.status = match err {
            BriefingError::Hold(_) => RunStatus::Hold,
            _ => RunStatus::Failed,
        };
        record.summary = err.to_string();
        record.updated_at = chrono::Local::now().to_rfc3339();
        self.index.upsert(record)?;
        Ok(())
    }

    fn emit_terminal(&self, id: &str, outcome: PipelineOutcome, started: Instant) {
        let Ok(record) = self.index.get(id) else {
            return;
        };
        if briefing_event_name(outcome.status).is_none() {
            return;
        }
        let payload = BriefingTerminalTelemetry {
            briefing_id: id.to_string(),
            status: outcome.status,
            research_depth: record.research_depth.as_str().to_string(),
            beat_count: outcome.beat_count,
            citation_count: outcome.citation_count,
            credits_consumed: record.credits_consumed,
            duration_ms: started.elapsed().as_millis() as i64,
            error_code: outcome.error_code,
            occurred_at: chrono::Utc::now().to_rfc3339(),
        };
        if let Some(hook) = self.telemetry.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            hook(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::PLAN_FILENAME;

    #[test]
    fn missing_script_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let service = BriefingService::open(dir.path()).unwrap();
        let created = service
            .create(CreateBriefingInput {
                intent: "今日科技".into(),
                ..Default::default()
            })
            .unwrap();
        let script = service.load_script(&created.id).unwrap();
        assert!(script.beats.is_empty());
    }

    #[test]
    fn confirm_plan_drafts_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let service = BriefingService::open(dir.path()).unwrap();
        let created = service
            .create(CreateBriefingInput {
                intent: "今日科技".into(),
                ..Default::default()
            })
            .unwrap();
        let plan_path = service.working_dir(&created.id).unwrap().join(PLAN_FILENAME);
        std::fs::remove_file(plan_path).unwrap();
        let plan = service.confirm_plan(&created.id, None).unwrap();
        assert!(plan.confirmed);
        assert_eq!(plan.questions.len(), 2);
    }
}
