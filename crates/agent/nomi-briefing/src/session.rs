use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{BriefingError, BriefingResult};
use crate::ir::{BeatScript, Dossier, ResearchDepth, ResearchPlan};
use crate::progress::{RunSnapshot, RunStatus};

pub const RUN_STATUS_FILENAME: &str = "run_status.json";
pub const SCRIPT_FILENAME: &str = "script.json";
pub const DOSSIER_FILENAME: &str = "dossier.json";
pub const PLAN_FILENAME: &str = "research-plan.json";
pub const TIMING_FILENAME: &str = "timing.json";
pub const BEATS_FILENAME: &str = "beats.json";
pub const SOURCES_FILENAME: &str = "sources.md";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub working_dir: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub format_secs: u32,
    #[serde(default)]
    pub research_depth: ResearchDepth,
    #[serde(default)]
    pub time_window_hours: u32,
    #[serde(default)]
    pub source_urls: Vec<String>,
    #[serde(default)]
    pub plan_confirmed: bool,
    #[serde(default = "default_stage")]
    pub stage: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub status: RunStatus,
    #[serde(default)]
    pub final_video: Option<String>,
    #[serde(default)]
    pub credits_consumed: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts_voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_model: Option<String>,
}

impl SessionRecord {
    pub fn tts_choice(&self) -> Option<crate::voice::TtsChoice> {
        crate::voice::TtsChoice::from_parts(
            self.tts_provider_id.as_deref(),
            self.tts_model.as_deref(),
            self.tts_voice.as_deref(),
        )
    }

    pub fn image_choice(&self) -> Option<crate::stills::ImageChoice> {
        crate::stills::ImageChoice::from_parts(
            self.image_provider_id.as_deref(),
            self.image_model.as_deref(),
        )
    }
}

impl Default for SessionRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            working_dir: String::new(),
            title: String::new(),
            intent: String::new(),
            format_secs: 90,
            research_depth: ResearchDepth::Fast,
            time_window_hours: 24,
            source_urls: Vec::new(),
            plan_confirmed: false,
            stage: default_stage(),
            summary: String::new(),
            status: RunStatus::Idle,
            final_video: None,
            credits_consumed: 0,
            created_at: String::new(),
            updated_at: String::new(),
            tts_provider_id: None,
            tts_model: None,
            tts_voice: None,
            image_provider_id: None,
            image_model: None,
        }
    }
}

fn default_stage() -> String {
    "created".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub stage: String,
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_video: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&SessionRecord> for SessionSummary {
    fn from(record: &SessionRecord) -> Self {
        Self {
            id: record.id.clone(),
            title: record.title.clone(),
            stage: record.stage.clone(),
            status: record.status,
            final_video: record.final_video.clone(),
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionsFile {
    sessions: BTreeMap<String, SessionRecord>,
}

#[derive(Clone)]
pub struct SessionIndex {
    workspace_root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl SessionIndex {
    pub fn open(data_dir: &Path) -> BriefingResult<Self> {
        let workspace_root = data_dir.join("briefing");
        std::fs::create_dir_all(workspace_root.join("sessions"))?;
        let sessions_path = workspace_root.join("sessions.json");
        if !sessions_path.exists() {
            atomic_write_json(&sessions_path, &SessionsFile::default())?;
        }
        Ok(Self {
            workspace_root,
            lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    fn sessions_path(&self) -> PathBuf {
        self.workspace_root.join("sessions.json")
    }

    fn load(&self) -> BriefingResult<SessionsFile> {
        let raw = std::fs::read_to_string(self.sessions_path())?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn save(&self, data: &SessionsFile) -> BriefingResult<()> {
        atomic_write_json(&self.sessions_path(), data)
    }

    pub fn create(&self, mut record: SessionRecord) -> BriefingResult<SessionRecord> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let id = if record.id.trim().is_empty() {
            Uuid::new_v4().to_string()
        } else {
            record.id.clone()
        };
        let working = self.workspace_root.join("sessions").join(&id);
        std::fs::create_dir_all(&working)?;
        record.id = id.clone();
        record.working_dir = working.to_string_lossy().into_owned();
        data.sessions.insert(id, record.clone());
        self.save(&data)?;
        Ok(record)
    }

    pub fn get(&self, id: &str) -> BriefingResult<SessionRecord> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.load()?
            .sessions
            .get(id)
            .cloned()
            .ok_or_else(|| BriefingError::SessionNotFound(id.to_string()))
    }

    pub fn upsert(&self, record: SessionRecord) -> BriefingResult<SessionRecord> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        data.sessions.insert(record.id.clone(), record.clone());
        self.save(&data)?;
        Ok(record)
    }

    pub fn list_summaries(&self) -> BriefingResult<Vec<SessionSummary>> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<SessionSummary> = self
            .load()?
            .sessions
            .values()
            .map(SessionSummary::from)
            .collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(rows)
    }

    pub fn delete(&self, id: &str) -> BriefingResult<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let Some(record) = data.sessions.remove(id) else {
            return Err(BriefingError::SessionNotFound(id.to_string()));
        };
        self.save(&data)?;
        let working = PathBuf::from(&record.working_dir);
        if working.exists() {
            let _ = std::fs::remove_dir_all(working);
        }
        Ok(())
    }

    pub fn reconcile_orphaned_active_runs(&self) -> BriefingResult<usize> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut data = self.load()?;
        let now = chrono::Local::now().to_rfc3339();
        let mut n = 0usize;
        for record in data.sessions.values_mut() {
            if !record.status.is_active() {
                continue;
            }
            record.status = RunStatus::Interrupted;
            record.summary = "interrupted".into();
            record.updated_at = now.clone();
            n += 1;
        }
        if n > 0 {
            self.save(&data)?;
        }
        Ok(n)
    }

    pub fn working_dir(&self, id: &str) -> BriefingResult<PathBuf> {
        let record = self.get(id)?;
        Ok(PathBuf::from(record.working_dir))
    }

    pub fn write_json<T: Serialize>(&self, id: &str, name: &str, value: &T) -> BriefingResult<()> {
        let dir = self.working_dir(id)?;
        atomic_write_json(&dir.join(name), value)
    }

    pub fn read_json<T: for<'de> Deserialize<'de>>(
        &self,
        id: &str,
        name: &str,
    ) -> BriefingResult<T> {
        self.try_read_json(id, name)?
            .ok_or_else(|| BriefingError::ArtifactNotFound(name.to_string()))
    }

    pub fn try_read_json<T: for<'de> Deserialize<'de>>(
        &self,
        id: &str,
        name: &str,
    ) -> BriefingResult<Option<T>> {
        let path = self.working_dir(id)?.join(name);
        match std::fs::read_to_string(&path) {
            Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub fn save_run_status(&self, id: &str, snapshot: &RunSnapshot) -> BriefingResult<()> {
        self.write_json(id, RUN_STATUS_FILENAME, snapshot)
    }

    pub fn load_run_status(&self, id: &str) -> RunSnapshot {
        self.read_json(id, RUN_STATUS_FILENAME).unwrap_or_default()
    }

    pub fn save_script(&self, id: &str, script: &BeatScript) -> BriefingResult<()> {
        self.write_json(id, SCRIPT_FILENAME, script)
    }

    pub fn load_script(&self, id: &str) -> BriefingResult<BeatScript> {
        self.read_json(id, SCRIPT_FILENAME)
    }

    pub fn save_dossier(&self, id: &str, dossier: &Dossier) -> BriefingResult<()> {
        self.write_json(id, DOSSIER_FILENAME, dossier)
    }

    pub fn load_dossier(&self, id: &str) -> BriefingResult<Dossier> {
        self.read_json(id, DOSSIER_FILENAME)
    }

    pub fn save_plan(&self, id: &str, plan: &ResearchPlan) -> BriefingResult<()> {
        self.write_json(id, PLAN_FILENAME, plan)
    }

    pub fn load_plan(&self, id: &str) -> BriefingResult<ResearchPlan> {
        self.read_json(id, PLAN_FILENAME)
    }
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> BriefingResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_list_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let created = index
            .create(SessionRecord {
                title: "早报".into(),
                intent: "今日科技".into(),
                created_at: now.clone(),
                updated_at: now,
                ..SessionRecord::default()
            })
            .unwrap();
        assert!(!created.id.is_empty());
        assert_eq!(index.list_summaries().unwrap().len(), 1);
        assert_eq!(index.get(&created.id).unwrap().title, "早报");
    }

    #[test]
    fn missing_script_is_absent_not_io_noise() {
        let dir = tempfile::tempdir().unwrap();
        let index = SessionIndex::open(dir.path()).unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let created = index
            .create(SessionRecord {
                title: "早报".into(),
                intent: "今日科技".into(),
                created_at: now.clone(),
                updated_at: now,
                ..SessionRecord::default()
            })
            .unwrap();
        assert!(index.try_read_json::<serde_json::Value>(&created.id, SCRIPT_FILENAME).unwrap().is_none());
        let err = index.load_script(&created.id).unwrap_err();
        assert!(matches!(err, BriefingError::ArtifactNotFound(_)));
    }
}
