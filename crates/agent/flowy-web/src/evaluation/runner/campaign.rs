//! Resumable Admission campaign coordination.

use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::evidence::{
    admission_result_file_is_complete, atomic_json_write_locked, default_safety_path,
    default_status_path, read_json,
};
use super::execution::{git_worktree_is_dirty, read_git_sha, run, select_cases};
use super::quota::QuotaLedger;
use super::scoring::{category_label, summarize_with_evidence, EvaluationSummary};
use super::{
    CaseCategory, EvaluationMode, EvaluationProfile, FetchEvaluationCase, FetchEvaluationManifest,
    PeerMode, RunConfig, RunStatus, SafetyReport, MAX_CALLS_PER_DAY, MAX_CALLS_PER_RUN,
    SCORING_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CampaignState {
    Ready,
    Running,
    PausedForQuota,
    Completed,
    InconclusiveDueToQuota,
    Incomplete,
    Reject,
}

impl CampaignState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::PausedForQuota => "paused_for_quota",
            Self::Completed => "completed",
            Self::InconclusiveDueToQuota => "inconclusive_due_to_quota",
            Self::Incomplete => "incomplete",
            Self::Reject => "reject",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CampaignBatchState {
    Pending,
    Completed,
    Incomplete,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampaignNextAction {
    Completed,
    InconclusiveDueToQuota,
    RunBatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CampaignStopReason {
    Completed,
    CampaignCapExhausted,
    QuotaExhausted,
    RateLimited,
    QuotaLedgerFailed,
    SafetyViolation,
    CampaignProvenanceMismatch,
    SetupFailed,
}

impl CampaignStopReason {
    fn from_wire(value: &str) -> Option<Self> {
        Some(match value {
            "completed" => Self::Completed,
            "campaign_cap_exhausted" => Self::CampaignCapExhausted,
            "quota_exhausted" => Self::QuotaExhausted,
            "rate_limited" => Self::RateLimited,
            "quota_ledger_failed" => Self::QuotaLedgerFailed,
            "safety_violation" => Self::SafetyViolation,
            "campaign_provenance_mismatch" => Self::CampaignProvenanceMismatch,
            "setup_failed" => Self::SetupFailed,
            _ => return None,
        })
    }
}

/// Configuration for the resumable, multi-day public Admission campaign.
///
/// The campaign deliberately owns batching and provenance rather than making
/// the CLI stitch together independent `admit` runs.  That keeps a paused
/// batch resumable without allowing partial evidence to leak into a summary.

#[derive(Debug, Clone)]
pub struct CampaignConfig {
    pub manifest: PathBuf,
    pub tag: String,
    pub batch_size: usize,
    pub pacing_ms: u64,
    pub max_calls_per_batch: u32,
    pub daily_cap: u32,
    pub campaign_cap: u32,
    pub quota_path: PathBuf,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CampaignEvidence {
    result_path: PathBuf,
    status_path: PathBuf,
    safety_path: PathBuf,
    actual_remote_calls: u32,
    run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CampaignBatchStatus {
    pub(crate) batch_index: usize,
    pub(crate) category: String,
    pub(crate) case_ids: Vec<String>,
    pub(crate) state: CampaignBatchState,
    pub(crate) attempt_index: u32,
    pub(crate) completed_evidence: Option<CampaignEvidence>,
    pub(crate) discarded_evidence: Vec<CampaignEvidence>,
    pub(crate) stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CampaignStatus {
    schema_version: u32,
    campaign_id: String,
    scoring_version: String,
    evaluation_profile: EvaluationProfile,
    git_sha: String,
    corpus_version: String,
    tag: String,
    batch_size: usize,
    max_calls_per_batch: u32,
    daily_cap: u32,
    campaign_cap: u32,
    #[serde(default)]
    quota_ledger_date: String,
    #[serde(default)]
    quota_ledger_used_calls: u32,
    actual_remote_calls: u32,
    completed_batches: usize,
    state: CampaignState,
    stop_reason: Option<String>,
    resume_at_unix: Option<i64>,
    batches: Vec<CampaignBatchStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CampaignOutcome {
    pub campaign_id: String,
    pub state: String,
    pub stop_reason: Option<String>,
    pub resume_at_unix: Option<i64>,
    pub actual_remote_calls: u32,
    pub completed_batches: usize,
    pub total_batches: usize,
    pub summary_path: Option<PathBuf>,
}

pub struct AdmissionCampaign {
    config: CampaignConfig,
    status_path: PathBuf,
    status: CampaignStatus,
    _run_lock: File,
}

impl AdmissionCampaign {
    /// Open an existing campaign or create a new one.  Admission campaigns
    /// are fail-closed: they require a clean worktree and freeze all scoring
    /// provenance before the first external call.
    pub fn open_or_create(config: CampaignConfig) -> Result<Self, Box<dyn std::error::Error>> {
        validate_campaign_config(&config)?;
        if git_worktree_is_dirty()? {
            return Err("Admission campaign requires a clean worktree".into());
        }
        Self::open_or_create_with_sha(config, read_git_sha()?)
    }

    #[cfg(test)]
    fn open_or_create_for_test(
        config: CampaignConfig,
        git_sha: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        validate_campaign_config(&config)?;
        Self::open_or_create_with_sha(config, git_sha.to_owned())
    }

    fn open_or_create_with_sha(
        config: CampaignConfig,
        git_sha: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let manifest: FetchEvaluationManifest = read_json(&config.manifest)?;
        manifest.validate()?;
        fs::create_dir_all(&config.output_dir)?;
        let run_lock_path = config.output_dir.join("campaign.run.lock");
        let run_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&run_lock_path)?;
        run_lock
            .try_lock_exclusive()
            .map_err(|_| "another process is already running this Admission campaign")?;
        let status_path = config.output_dir.join("campaign.status.json");

        if status_path.exists() {
            let mut status: CampaignStatus = read_json(&status_path)?;
            validate_campaign_provenance(&status, &config, &manifest, &git_sha)?;
            reconcile_orphan_attempts(&config, &mut status)?;
            if status.quota_ledger_date.is_empty()
                && !matches!(status.state, CampaignState::Incomplete | CampaignState::Reject)
            {
                let ledger = current_quota_ledger(&config.quota_path)?;
                status.quota_ledger_date = ledger.utc_date;
                status.quota_ledger_used_calls = ledger.used_calls;
            }
            atomic_json_write_locked(&status_path, &status)?;
            return Ok(Self {
                config,
                status_path,
                status,
                _run_lock: run_lock,
            });
        }

        let batches = build_campaign_batches(&manifest.cases, &config.tag, config.batch_size)?;
        if batches.is_empty() {
            return Err("campaign selection contains no enabled, non-stale public cases".into());
        }
        if config.tag.starts_with("admission")
            && let Some(reason) = admission_pool_shortage(&batches)
        {
            return Err(reason.into());
        }
        let quota_ledger = current_quota_ledger(&config.quota_path)?;
        let status = CampaignStatus {
            schema_version: 1,
            campaign_id: Uuid::now_v7().to_string(),
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Admission,
            git_sha,
            corpus_version: manifest.corpus_version.clone(),
            tag: config.tag.clone(),
            batch_size: config.batch_size,
            max_calls_per_batch: config.max_calls_per_batch,
            daily_cap: config.daily_cap,
            campaign_cap: config.campaign_cap,
            quota_ledger_date: quota_ledger.utc_date,
            quota_ledger_used_calls: quota_ledger.used_calls,
            actual_remote_calls: 0,
            completed_batches: 0,
            state: CampaignState::Ready,
            stop_reason: None,
            resume_at_unix: None,
            batches,
        };
        atomic_json_write_locked(&status_path, &status)?;
        Ok(Self {
            config,
            status_path,
            status,
            _run_lock: run_lock,
        })
    }

    /// Run as many complete batches as the current quota permits.  A partial
    /// batch is retained as discarded evidence and is retried as a whole on
    /// the next continuation, never mixed into a formal summary.
    pub async fn run_available(
        &mut self,
    ) -> Result<CampaignOutcome, Box<dyn std::error::Error>> {
        self.run_available_inner(false).await
    }

    #[cfg(test)]
    async fn run_available_for_test(
        &mut self,
    ) -> Result<CampaignOutcome, Box<dyn std::error::Error>> {
        self.run_available_inner(true).await
    }

    async fn run_available_inner(
        &mut self,
        allow_dirty_for_test: bool,
    ) -> Result<CampaignOutcome, Box<dyn std::error::Error>> {
        let current_sha = read_git_sha()?;
        if (!allow_dirty_for_test && git_worktree_is_dirty()?)
            || current_sha != self.status.git_sha
        {
            self.status.state = CampaignState::Reject;
            self.status.stop_reason = Some("campaign_provenance_mismatch".to_owned());
            self.persist_status()?;
            return Ok(self.outcome(None));
        }
        // The same Campaign instance may be called again after its
        // `run_available` future was cancelled.  Reconcile the persisted
        // attempt intent here as well as during open, before selecting a new
        // attempt, so an in-process cancellation cannot bypass the cap.
        reconcile_orphan_attempts(&self.config, &mut self.status)?;
        self.persist_status()?;
        if self.status.state == CampaignState::Reject {
            return Ok(self.outcome(None));
        }
        if self.status.state == CampaignState::Incomplete {
            return Ok(self.outcome(None));
        }
        if let Some(resume_at) = self.status.resume_at_unix
            && Utc::now().timestamp() < resume_at
        {
            self.status.state = CampaignState::PausedForQuota;
            self.persist_status()?;
            return Ok(self.outcome(None));
        }
        self.status.resume_at_unix = None;
        self.status.state = CampaignState::Running;
        self.status.stop_reason = None;
        self.persist_status()?;

        loop {
            let batch_index = self
                .status
                .batches
                .iter()
                .position(|batch| batch.state == CampaignBatchState::Pending);
            let remaining_calls = self
                .config
                .campaign_cap
                .saturating_sub(self.status.actual_remote_calls);
            match campaign_next_action(batch_index.is_some(), remaining_calls) {
                CampaignNextAction::Completed => {
                self.status.state = CampaignState::Completed;
                self.status.stop_reason = Some("completed".to_owned());
                self.persist_status()?;
                return Ok(self.outcome(Some(self.summary_path())));
                }
                CampaignNextAction::InconclusiveDueToQuota => {
                self.status.state = CampaignState::InconclusiveDueToQuota;
                self.status.stop_reason = Some("campaign_cap_exhausted".to_owned());
                self.status.resume_at_unix = None;
                self.persist_status()?;
                return Ok(self.outcome(None));
                }
                CampaignNextAction::RunBatch => {}
            }
            let batch_index = batch_index.expect("run_batch requires a pending batch");
            let (attempt, case_ids) = {
                let batch = &mut self.status.batches[batch_index];
                batch.attempt_index = batch.attempt_index.saturating_add(1);
                (batch.attempt_index, batch.case_ids.clone())
            };
            // Persist the attempt intent before opening the result file or
            // invoking the Runner.  If the process/future is cancelled after
            // setup, recovery advances to a fresh output path instead of
            // retrying an orphaned attempt and colliding with create_new_output.
            self.persist_status()?;
            let output = self.config.output_dir.join(format!(
                "batch-{:02}-attempt-{:02}.jsonl",
                batch_index + 1,
                attempt
            ));
            let run_config = RunConfig {
                mode: EvaluationMode::Compare,
                peer_mode: PeerMode::Cold,
                profile: EvaluationProfile::Admission,
                manifest: self.config.manifest.clone(),
                case_ids: Some(case_ids),
                category: None,
                tag: Some(self.config.tag.clone()),
                repeat: 3,
                pacing_ms: self.config.pacing_ms,
                max_calls: self.config.max_calls_per_batch.min(remaining_calls),
                daily_cap: self.config.daily_cap,
                quota_path: self.config.quota_path.clone(),
                output,
                status: None,
            };
            let batch = &mut self.status.batches[batch_index];
            let outcome = match run(run_config).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    batch.state = CampaignBatchState::Incomplete;
                    batch.stop_reason = Some("setup_failed".to_owned());
                    self.status.state = CampaignState::Incomplete;
                    self.status.stop_reason = Some(format!("setup_failed: {error}"));
                    self.persist_status()?;
                    return Ok(self.outcome(None));
                }
            };
            self.status.actual_remote_calls = self
                .status
                .actual_remote_calls
                .saturating_add(outcome.status.actual_remote_calls);
            let ledger = current_quota_ledger(&self.config.quota_path)?;
            self.status.quota_ledger_date = ledger.utc_date;
            self.status.quota_ledger_used_calls = ledger.used_calls;
            let campaign_cap_reached = self.status.actual_remote_calls >= self.config.campaign_cap;
            let evidence = CampaignEvidence {
                result_path: outcome
                    .output_path
                    .clone(),
                status_path: outcome.status_path.clone(),
                safety_path: outcome.safety_path.clone(),
                actual_remote_calls: outcome.status.actual_remote_calls,
                run_id: outcome.status.run_id.clone(),
            };
            let safety: SafetyReport = read_json(&outcome.safety_path)?;
            let stop_reason = outcome.status.stop_reason.clone();
            if !safety.all_zero {
                batch.state = CampaignBatchState::Rejected;
                batch.stop_reason = Some("safety_violation".to_owned());
                self.status.state = CampaignState::Reject;
                self.status.stop_reason = Some("safety_violation".to_owned());
                batch.discarded_evidence.push(evidence);
                self.persist_status()?;
                return Ok(self.outcome(None));
            }
            if stop_reason == "completed"
                && outcome.status.completed_attempts == outcome.status.planned_attempts
            {
                batch.state = CampaignBatchState::Completed;
                batch.stop_reason = Some(stop_reason);
                batch.completed_evidence = Some(evidence);
                self.status.completed_batches += 1;
                self.persist_status()?;
                continue;
            }
            if matches!(
                CampaignStopReason::from_wire(stop_reason.as_str()),
                Some(
                    CampaignStopReason::QuotaExhausted
                        | CampaignStopReason::RateLimited
                        | CampaignStopReason::QuotaLedgerFailed
                        | CampaignStopReason::CampaignCapExhausted,
                )
            ) {
                batch.discarded_evidence.push(evidence);
                batch.stop_reason = Some(stop_reason.clone());
                self.status.state = if campaign_cap_reached {
                    CampaignState::InconclusiveDueToQuota
                } else {
                    CampaignState::PausedForQuota
                };
                self.status.stop_reason = Some(stop_reason);
                self.status.resume_at_unix = if campaign_cap_reached {
                    None
                } else {
                    outcome
                        .status
                        .cooldown_until_unix
                        .or_else(|| Some(next_utc_midnight_unix()))
                };
                self.persist_status()?;
                return Ok(self.outcome(None));
            }
            batch.state = CampaignBatchState::Incomplete;
            batch.stop_reason = Some(stop_reason.clone());
            batch.discarded_evidence.push(evidence);
            self.status.state = CampaignState::Incomplete;
            self.status.stop_reason = Some(stop_reason);
            self.persist_status()?;
            return Ok(self.outcome(None));
        }
    }

    pub fn summarize(&self) -> Result<EvaluationSummary, Box<dyn std::error::Error>> {
        let mut inputs = Vec::new();
        let mut statuses = Vec::new();
        let mut safety = Vec::new();
        for batch in &self.status.batches {
            if let Some(evidence) = &batch.completed_evidence {
                inputs.push(evidence.result_path.clone());
                statuses.push(evidence.status_path.clone());
                safety.push(evidence.safety_path.clone());
            }
        }
        if inputs.is_empty() {
            return Err("campaign has no complete batch evidence to summarize".into());
        }
        summarize_with_evidence(&inputs, &self.summary_path(), &statuses, &safety)
    }

    fn summary_path(&self) -> PathBuf {
        self.config.output_dir.join("campaign.summary.json")
    }

    fn persist_status(&self) -> Result<(), Box<dyn std::error::Error>> {
        atomic_json_write_locked(&self.status_path, &self.status)
    }

    fn outcome(&self, summary_path: Option<PathBuf>) -> CampaignOutcome {
        CampaignOutcome {
            campaign_id: self.status.campaign_id.clone(),
            state: self.status.state.as_str().to_owned(),
            stop_reason: self.status.stop_reason.clone(),
            resume_at_unix: self.status.resume_at_unix,
            actual_remote_calls: self.status.actual_remote_calls,
            completed_batches: self.status.completed_batches,
            total_batches: self.status.batches.len(),
            summary_path,
        }
    }
}

fn current_quota_ledger(path: &PathBuf) -> Result<QuotaLedger, Box<dyn std::error::Error>> {
    Ok(read_quota_ledger(path)?.unwrap_or_else(|| QuotaLedger {
        utc_date: Utc::now().date_naive().to_string(),
        used_calls: 0,
        cooldown_until_unix: None,
    }))
}

fn read_quota_ledger(
    path: &PathBuf,
) -> Result<Option<QuotaLedger>, Box<dyn std::error::Error>> {
    if path.exists() {
        return Ok(Some(read_json(path)?));
    }
    Ok(None)
}

/// Reconcile an attempt intent left behind by a cancelled or crashed Runner.
///
/// The campaign status is written before the Runner starts, so an intent with
/// no corresponding evidence must never be silently retried: the process may
/// have reserved calls after the last status flush.  Readable evidence is
/// registered as discarded (or completed when it is a full, safe run), and a
/// same-day quota-ledger delta is used as a conservative lower bound for
/// reservations that were not reflected in the partial status file.
fn reconcile_orphan_attempts(
    config: &CampaignConfig,
    status: &mut CampaignStatus,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed_ledger = read_quota_ledger(&config.quota_path)?;
    let ledger = observed_ledger.clone().unwrap_or_else(|| QuotaLedger {
        utc_date: Utc::now().date_naive().to_string(),
        used_calls: 0,
        cooldown_until_unix: None,
    });
    let ledger_delta = if !status.quota_ledger_date.is_empty()
        && status.quota_ledger_date == ledger.utc_date
    {
        ledger
            .used_calls
            .saturating_sub(status.quota_ledger_used_calls)
    } else {
        0
    };
    let mut changed = false;
    let mut recovered_evidence_calls = 0u32;
    let campaign_git_sha = status.git_sha.clone();
    let campaign_corpus_version = status.corpus_version.clone();

    let has_unregistered_intent = status.batches.iter().any(|batch| {
        if batch.state != CampaignBatchState::Pending || batch.attempt_index == 0 {
            return false;
        }
        (1..=batch.attempt_index).any(|attempt| {
            let output_path = config.output_dir.join(format!(
                "batch-{:02}-attempt-{:02}.jsonl",
                batch.batch_index + 1,
                attempt
            ));
            !evidence_registered(batch, &output_path)
        })
    });
    if has_unregistered_intent
        && (observed_ledger.is_none()
            || status.quota_ledger_date.is_empty()
            || status.quota_ledger_date != ledger.utc_date)
    {
        // A UTC rollover resets the daily ledger, so a reservation made after
        // the last checkpoint cannot be distinguished from zero.  Refuse to
        // resume rather than risking a campaign-cap bypass.
        mark_incomplete_recovery(status);
        return Ok(());
    }

    for batch in &mut status.batches {
        if batch.state != CampaignBatchState::Pending || batch.attempt_index == 0 {
            continue;
        }
        for attempt in 1..=batch.attempt_index {
            let output_path = config.output_dir.join(format!(
                "batch-{:02}-attempt-{:02}.jsonl",
                batch.batch_index + 1,
                attempt
            ));
            if evidence_registered(batch, &output_path) {
                continue;
            }

            let status_path = default_status_path(&output_path);
            let safety_path = default_safety_path(&output_path);
            let present = [
                output_path.exists(),
                status_path.exists(),
                safety_path.exists(),
            ];
            if !present.iter().all(|value| *value) {
                mark_incomplete_recovery(status);
                return Ok(());
            }

            let run_status: RunStatus = match read_json(&status_path) {
                Ok(value) => value,
                Err(_) => {
                    mark_incomplete_recovery(status);
                    return Ok(());
                }
            };
            let safety: SafetyReport = match read_json(&safety_path) {
                Ok(value) => value,
                Err(_) => {
                    mark_incomplete_recovery(status);
                    return Ok(());
                }
            };
            match validate_orphan_evidence(
                &campaign_git_sha,
                &campaign_corpus_version,
                &run_status,
                &safety,
            ) {
                Ok(()) => {}
                Err(OrphanEvidenceFailure::SafetyViolation) => {
                    status.state = CampaignState::Reject;
                    status.stop_reason = Some("safety_violation".to_owned());
                    status.resume_at_unix = None;
                    return Ok(());
                }
                Err(OrphanEvidenceFailure::Incomplete) => {
                    mark_incomplete_recovery(status);
                    return Ok(());
                }
            }
            let evidence = CampaignEvidence {
                result_path: output_path,
                status_path,
                safety_path,
                actual_remote_calls: run_status.actual_remote_calls,
                run_id: run_status.run_id.clone(),
            };
            recovered_evidence_calls = recovered_evidence_calls
                .saturating_add(evidence.actual_remote_calls);
            let result_complete = if safety.complete
                && safety.all_zero
                && run_status.stop_reason == "completed"
                && run_status.completed_attempts == run_status.planned_attempts
            {
                let expected_category = match batch.category.as_str() {
                    "public_pdf_text" => CaseCategory::PublicPdfText,
                    "javascript_shell" => CaseCategory::JavascriptShell,
                    _ => {
                        mark_incomplete_recovery(status);
                        return Ok(());
                    }
                };
                match admission_result_file_is_complete(
                    &evidence.result_path,
                    &run_status,
                    &batch.case_ids,
                    expected_category,
                    &campaign_git_sha,
                    &campaign_corpus_version,
                ) {
                    Ok(value) => value,
                    Err(_) => {
                        mark_incomplete_recovery(status);
                        return Ok(());
                    }
                }
            } else {
                false
            };
            if result_complete
                && safety.all_zero
                && run_status.stop_reason == "completed"
                && run_status.completed_attempts == run_status.planned_attempts
            {
                batch.state = CampaignBatchState::Completed;
                batch.stop_reason = Some("completed".to_owned());
                batch.completed_evidence = Some(evidence);
                status.completed_batches = status.completed_batches.saturating_add(1);
            } else {
                batch.discarded_evidence.push(evidence);
                batch.stop_reason = Some(run_status.stop_reason.clone());
            }
            changed = true;
        }
    }

    if changed {
        // A ledger delta can include a call reserved after the last partial
        // status flush.  Taking the maximum avoids double counting calls that
        // are already present in the orphan RunStatus while remaining
        // fail-closed if the quota path is shared with another local run.
        status.actual_remote_calls = status
            .actual_remote_calls
            .saturating_add(recovered_evidence_calls.max(ledger_delta));
        status.quota_ledger_date = ledger.utc_date;
        status.quota_ledger_used_calls = ledger.used_calls;
    }
    Ok(())
}

fn evidence_registered(batch: &CampaignBatchStatus, output_path: &PathBuf) -> bool {
    batch
        .discarded_evidence
        .iter()
        .any(|evidence| evidence.result_path == *output_path)
        || batch
            .completed_evidence
            .as_ref()
            .is_some_and(|evidence| evidence.result_path == *output_path)
}

fn validate_orphan_evidence(
    campaign_git_sha: &str,
    campaign_corpus_version: &str,
    run_status: &RunStatus,
    safety: &SafetyReport,
) -> Result<(), OrphanEvidenceFailure> {
    if safety.source_mismatch_count > 0
        || safety.dropped_remote_item_count > 0
        || safety.sensitive_egress_count > 0
        || safety.retry_limit_violation_count > 0
        || safety.cancellation_late_result_count > 0
        || run_status.sensitive_egress_count > 0
        || run_status.retry_limit_violation_count > 0
    {
        return Err(OrphanEvidenceFailure::SafetyViolation);
    }
    if run_status.schema_version != 3
        || safety.schema_version != 2
        || run_status.scoring_version != SCORING_VERSION
        || safety.scoring_version != SCORING_VERSION
        || run_status.evaluation_profile != EvaluationProfile::Admission
        || safety.evaluation_profile != EvaluationProfile::Admission
        || run_status.run_id != safety.run_id
        || run_status.git_sha != campaign_git_sha
        || safety.git_sha != campaign_git_sha
        || run_status.corpus_version != campaign_corpus_version
        || safety.corpus_version != campaign_corpus_version
        || run_status.dirty_worktree
        || safety.dirty_worktree
        || run_status.actual_remote_calls != safety.actual_remote_calls
        || run_status.actual_remote_calls
            != run_status.actual_fetch_calls.saturating_add(run_status.actual_search_calls)
        || safety.actual_remote_calls
            != safety.actual_fetch_calls.saturating_add(safety.actual_search_calls)
        || run_status.actual_fetch_calls != safety.actual_fetch_calls
        || run_status.actual_search_calls != safety.actual_search_calls
        || run_status.recovery_retry_calls != safety.recovery_retry_calls
        || run_status.sensitive_egress_count != safety.sensitive_egress_count
        || run_status.retry_limit_violation_count != safety.retry_limit_violation_count
        || run_status.stop_reason != safety.stop_reason
        || (safety.complete && !safety.all_zero)
        || (!safety.complete && safety.all_zero)
    {
        return Err(OrphanEvidenceFailure::Incomplete);
    }
    Ok(())
}

#[derive(Debug)]
enum OrphanEvidenceFailure {
    SafetyViolation,
    Incomplete,
}

fn mark_incomplete_recovery(status: &mut CampaignStatus) {
    status.state = CampaignState::Incomplete;
    status.stop_reason = Some("incomplete_run".to_owned());
    status.resume_at_unix = None;
}

pub(crate) fn validate_campaign_config(config: &CampaignConfig) -> Result<(), Box<dyn std::error::Error>> {
    if config.tag.trim().is_empty() {
        return Err("campaign --tag must not be empty".into());
    }
    if config.batch_size == 0 || config.batch_size > 5 {
        return Err("campaign --batch-size must be in 1..=5".into());
    }
    if config.max_calls_per_batch == 0 || config.max_calls_per_batch > MAX_CALLS_PER_RUN {
        return Err(format!(
            "campaign --max-calls-per-batch must be in 1..={MAX_CALLS_PER_RUN}"
        )
        .into());
    }
    if config.daily_cap == 0 || config.daily_cap > MAX_CALLS_PER_DAY {
        return Err(format!(
            "campaign --daily-cap must be in 1..={MAX_CALLS_PER_DAY}"
        )
        .into());
    }
    if config.campaign_cap == 0 || config.campaign_cap > 200 {
        return Err("campaign --campaign-cap must be in 1..=200".into());
    }
    Ok(())
}

fn validate_campaign_provenance(
    status: &CampaignStatus,
    config: &CampaignConfig,
    manifest: &FetchEvaluationManifest,
    git_sha: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if status.schema_version != 1
        || status.evaluation_profile != EvaluationProfile::Admission
        || status.scoring_version != SCORING_VERSION
        || status.git_sha != git_sha
        || status.corpus_version != manifest.corpus_version
        || status.tag != config.tag
        || status.batch_size != config.batch_size
        || status.max_calls_per_batch != config.max_calls_per_batch
        || status.daily_cap != config.daily_cap
        || status.campaign_cap != config.campaign_cap
    {
        return Err("campaign provenance/configuration mismatch".into());
    }
    let expected = build_campaign_batches(&manifest.cases, &config.tag, config.batch_size)?;
    if status
        .batches
        .iter()
        .map(|batch| (&batch.category, &batch.case_ids))
        .collect::<Vec<_>>()
        != expected
            .iter()
            .map(|batch| (&batch.category, &batch.case_ids))
            .collect::<Vec<_>>()
    {
        return Err("campaign case plan changed; refusing to resume".into());
    }
    Ok(())
}

pub(crate) fn build_campaign_batches(
    cases: &[FetchEvaluationCase],
    tag: &str,
    batch_size: usize,
) -> Result<Vec<CampaignBatchStatus>, Box<dyn std::error::Error>> {
    let today = Utc::now().date_naive();
    let mut selected = select_cases(cases, None, None, Some(tag), today)?
        .into_iter()
        .filter(|case| {
            matches!(
                case.category,
                CaseCategory::PublicPdfText | CaseCategory::JavascriptShell
            )
        })
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut batches = Vec::new();
    let mut next_index = 0usize;
    for category in [CaseCategory::PublicPdfText, CaseCategory::JavascriptShell] {
        let category_cases = selected
            .iter()
            .copied()
            .filter(|case| case.category == category)
            .collect::<Vec<_>>();
        for chunk in category_cases.chunks(batch_size) {
            batches.push(CampaignBatchStatus {
                batch_index: next_index,
                category: category_label(category).to_owned(),
                case_ids: chunk.iter().map(|case| case.id.clone()).collect(),
                state: CampaignBatchState::Pending,
                attempt_index: 0,
                completed_evidence: None,
                discarded_evidence: Vec::new(),
                stop_reason: None,
            });
            next_index += 1;
        }
    }
    Ok(batches)
}

pub(crate) fn admission_pool_shortage(batches: &[CampaignBatchStatus]) -> Option<String> {
    let pdf_cases = batches
        .iter()
        .filter(|batch| batch.category == "public_pdf_text")
        .map(|batch| batch.case_ids.len())
        .sum::<usize>();
    let js_cases = batches
        .iter()
        .filter(|batch| batch.category == "javascript_shell")
        .map(|batch| batch.case_ids.len())
        .sum::<usize>();
    (pdf_cases < 15 || js_cases < 15).then(|| {
        format!(
            "candidate_pool_shortage: admission requires at least 15 cases per category (pdf={pdf_cases}, js={js_cases})"
        )
    })
}

fn next_utc_midnight_unix() -> i64 {
    let tomorrow = Utc::now().date_naive() + chrono::Duration::days(1);
    tomorrow
        .and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc().timestamp())
        .unwrap_or_else(|| Utc::now().timestamp().saturating_add(86_400))
}

pub(crate) fn campaign_next_action(
    has_pending_batch: bool,
    remaining_calls: u32,
) -> CampaignNextAction {
    if !has_pending_batch {
        CampaignNextAction::Completed
    } else if remaining_calls == 0 {
        CampaignNextAction::InconclusiveDueToQuota
    } else {
        CampaignNextAction::RunBatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::demo::demo_case;
    use super::super::{FetchEvaluationResult, QualityGrade};
    use std::path::Path;
    use tempfile::tempdir;

    fn campaign_fixture(root: &Path) -> CampaignConfig {
        let manifest_path = root.join("manifest.json");
        let mut pdf = demo_case("https://demo.example.com/pdf");
        pdf.category = CaseCategory::PublicPdfText;
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "campaign-test".to_owned(),
            cases: vec![pdf, demo_case("https://demo.example.com/js")],
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("manifest JSON"),
        )
        .expect("manifest write");
        CampaignConfig {
            manifest: manifest_path,
            tag: "demo".to_owned(),
            batch_size: 5,
            pacing_ms: 0,
            max_calls_per_batch: 25,
            daily_cap: 60,
            campaign_cap: 200,
            quota_path: root.join("quota.json"),
            output_dir: root.join("campaign"),
        }
    }

    fn complete_admission_result(
        case_id: &str,
        category: CaseCategory,
        run_id: &str,
        git_sha: &str,
        corpus_version: &str,
        attempt: u32,
    ) -> FetchEvaluationResult {
        FetchEvaluationResult {
            schema_version: 3,
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Admission,
            run_id: run_id.to_owned(),
            git_sha: git_sha.to_owned(),
            corpus_version: corpus_version.to_owned(),
            case_id: case_id.to_owned(),
            category,
            mode: if attempt == 1 {
                EvaluationMode::Compare
            } else {
                EvaluationMode::E2e
            },
            peer_mode: if attempt == 1 {
                PeerMode::Cold
            } else {
                PeerMode::Warm
            },
            attempt,
            local_failure_kind: Some("network".to_owned()),
            remote_attempted: true,
            remote_budget_skipped: false,
            remote_eligible: true,
            local_success: false,
            remote_success: true,
            effective_success: true,
            incremental_success: true,
            quality_grade: QualityGrade::Q2,
            elapsed_ms: 1,
            queue_ms: Some(0),
            call_ms: Some(1),
            local_content_chars: 0,
            remote_content_chars: 100,
            content_chars: 100,
            local_marker_hits: 0,
            remote_marker_hits: 1,
            marker_hits: 1,
            marker_count: 1,
            marker_hit_rate: 1.0,
            source_truncated: false,
            source_mismatch_count: 0,
            dropped_remote_item_count: 0,
            retry_after_ms: None,
            challenge_detected: false,
            error_class: None,
            outcome_class: "success".to_owned(),
        }
    }

    fn complete_run_status(run_id: &str, git_sha: &str, corpus_version: &str) -> RunStatus {
        RunStatus {
            schema_version: 3,
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Admission,
            run_id: run_id.to_owned(),
            git_sha: git_sha.to_owned(),
            corpus_version: corpus_version.to_owned(),
            dirty_worktree: false,
            planned_attempts: 3,
            completed_attempts: 3,
            actual_remote_calls: 0,
            actual_fetch_calls: 0,
            actual_search_calls: 0,
            recovery_retry_calls: 0,
            retry_limit_violation_count: 0,
            sensitive_egress_count: 0,
            stop_reason: "completed".to_owned(),
            retry_after_ms: None,
            cooldown_until_unix: None,
        }
    }

    fn write_result_records(path: &Path, records: &[FetchEvaluationResult]) {
        let content = records
            .iter()
            .map(|record| serde_json::to_string(record).expect("result JSON"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path, format!("{content}\n")).expect("result file");
    }

    #[test]
    fn campaign_lock_blocks_a_second_owner_before_execution() {
        let root = tempdir().expect("temporary campaign root");
        let manifest_path = root.path().join("manifest.json");
        let output_dir = root.path().join("campaign");
        let quota_path = root.path().join("quota.json");
        let mut pdf = demo_case("https://demo.example.com/pdf");
        pdf.category = CaseCategory::PublicPdfText;
        let manifest = FetchEvaluationManifest {
            schema_version: 1,
            corpus_version: "campaign-test".to_owned(),
            cases: vec![pdf, demo_case("https://demo.example.com/js")],
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("manifest JSON"),
        )
        .expect("manifest write");
        let config = CampaignConfig {
            manifest: manifest_path,
            tag: "demo".to_owned(),
            batch_size: 5,
            pacing_ms: 0,
            max_calls_per_batch: 25,
            daily_cap: 60,
            campaign_cap: 200,
            quota_path,
            output_dir,
        };

        let first = AdmissionCampaign::open_or_create_for_test(config.clone(), "test-sha")
            .expect("first campaign owner");
        let second = AdmissionCampaign::open_or_create_for_test(config.clone(), "test-sha")
            .err()
            .expect("second owner must be rejected by the run lock");
        assert!(second.to_string().contains("already running"));
        drop(first);
        let resumed = AdmissionCampaign::open_or_create_for_test(config, "test-sha")
            .expect("released campaign lock can be resumed");
        assert_eq!(resumed.status.state, CampaignState::Ready);
    }

    #[test]
    fn typed_campaign_states_keep_existing_wire_values() {
        for (state, wire) in [
            (CampaignState::Ready, "ready"),
            (CampaignState::PausedForQuota, "paused_for_quota"),
            (CampaignState::InconclusiveDueToQuota, "inconclusive_due_to_quota"),
        ] {
            let encoded = serde_json::to_string(&state).expect("state JSON");
            assert_eq!(encoded, format!("\"{wire}\""));
            assert_eq!(
                serde_json::from_str::<CampaignState>(&encoded).expect("state round trip"),
                state
            );
        }
        assert_eq!(
            serde_json::to_string(&CampaignBatchState::Pending).expect("batch JSON"),
            "\"pending\""
        );
    }

    #[test]
    fn campaign_reconciles_orphan_calls_before_resume() {
        let root = tempdir().expect("temporary campaign root");
        let config = campaign_fixture(root.path());
        let campaign = AdmissionCampaign::open_or_create_for_test(config.clone(), "test-sha")
            .expect("initial campaign");
        let status_path = campaign.status_path.clone();
        drop(campaign);

        let mut status: CampaignStatus = read_json(&status_path).expect("campaign status");
        status.batches[0].attempt_index = 1;
        status.quota_ledger_date = Utc::now().date_naive().to_string();
        status.quota_ledger_used_calls = 1;
        fs::write(
            &status_path,
            serde_json::to_vec(&status).expect("campaign status JSON"),
        )
        .expect("status update");

        let output = config.output_dir.join("batch-01-attempt-01.jsonl");
        fs::create_dir_all(&config.output_dir).expect("campaign output directory");
        fs::write(&output, b"{\"case_id\":\"orphan\"}\n").expect("orphan result");
        let run_status = RunStatus {
            schema_version: 3,
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Admission,
            run_id: "orphan-run".to_owned(),
            git_sha: "test-sha".to_owned(),
            corpus_version: "campaign-test".to_owned(),
            dirty_worktree: false,
            planned_attempts: 3,
            completed_attempts: 1,
            actual_remote_calls: 2,
            actual_fetch_calls: 2,
            actual_search_calls: 0,
            recovery_retry_calls: 0,
            retry_limit_violation_count: 0,
            sensitive_egress_count: 0,
            stop_reason: "running".to_owned(),
            retry_after_ms: None,
            cooldown_until_unix: None,
        };
        fs::write(
            default_status_path(&output),
            serde_json::to_vec(&run_status).expect("run status JSON"),
        )
        .expect("run status");
        super::super::execution::write_safety_checkpoint(
            &default_safety_path(&output),
            &run_status,
            0,
            0,
            false,
        )
        .expect("safety checkpoint");
        fs::write(
            &config.quota_path,
            serde_json::to_vec(&QuotaLedger {
                utc_date: Utc::now().date_naive().to_string(),
                used_calls: 4,
                cooldown_until_unix: None,
            })
            .expect("quota JSON"),
        )
        .expect("quota ledger");

        let resumed = AdmissionCampaign::open_or_create_for_test(config, "test-sha")
            .expect("orphan evidence is recoverable");
        assert_eq!(resumed.status.actual_remote_calls, 3);
        assert_eq!(resumed.status.batches[0].discarded_evidence.len(), 1);
        assert_eq!(resumed.status.batches[0].state, CampaignBatchState::Pending);
        assert_eq!(resumed.status.quota_ledger_used_calls, 4);
    }

    #[test]
    fn campaign_missing_orphan_evidence_stops_recovery() {
        let root = tempdir().expect("temporary campaign root");
        let config = campaign_fixture(root.path());
        let campaign = AdmissionCampaign::open_or_create_for_test(config.clone(), "test-sha")
            .expect("initial campaign");
        let status_path = campaign.status_path.clone();
        drop(campaign);

        let mut status: CampaignStatus = read_json(&status_path).expect("campaign status");
        status.batches[0].attempt_index = 1;
        fs::write(
            &status_path,
            serde_json::to_vec(&status).expect("campaign status JSON"),
        )
        .expect("status update");

        let resumed = AdmissionCampaign::open_or_create_for_test(config, "test-sha")
            .expect("incomplete recovery is recorded");
        assert_eq!(resumed.status.state, CampaignState::Incomplete);
        assert_eq!(resumed.status.stop_reason.as_deref(), Some("incomplete_run"));
    }

    #[test]
    fn campaign_does_not_migrate_empty_ledger_before_orphan_recovery() {
        let root = tempdir().expect("temporary campaign root");
        let config = campaign_fixture(root.path());
        let campaign = AdmissionCampaign::open_or_create_for_test(config.clone(), "test-sha")
            .expect("initial campaign");
        let status_path = campaign.status_path.clone();
        drop(campaign);

        let mut status: CampaignStatus = read_json(&status_path).expect("campaign status");
        status.batches[0].attempt_index = 1;
        status.quota_ledger_date.clear();
        status.quota_ledger_used_calls = 0;
        fs::write(
            &status_path,
            serde_json::to_vec(&status).expect("campaign status JSON"),
        )
        .expect("status update");

        let resumed = AdmissionCampaign::open_or_create_for_test(config, "test-sha")
            .expect("empty baseline is recorded as incomplete");
        assert_eq!(resumed.status.state, CampaignState::Incomplete);
        assert!(resumed.status.quota_ledger_date.is_empty());
        assert_eq!(resumed.status.stop_reason.as_deref(), Some("incomplete_run"));
    }

    #[test]
    fn typed_admission_evidence_rejects_invalid_jsonl_shapes() {
        let root = tempdir().expect("temporary evidence root");
        let output = root.path().join("results.jsonl");
        let run_id = "typed-evidence-run";
        let git_sha = "test-sha";
        let corpus = "campaign-test";
        let status = complete_run_status(run_id, git_sha, corpus);
        let valid = (1..=3)
            .map(|attempt| {
                complete_admission_result(
                    "pdf",
                    CaseCategory::PublicPdfText,
                    run_id,
                    git_sha,
                    corpus,
                    attempt,
                )
            })
            .collect::<Vec<_>>();

        write_result_records(&output, &valid);
        assert!(admission_result_file_is_complete(
            &output,
            &status,
            &["pdf".to_owned()],
            CaseCategory::PublicPdfText,
            git_sha,
            corpus,
        )
        .expect("valid typed evidence"));

        fs::write(&output, "{}\n{}\n{}\n").expect("untyped evidence");
        assert!(admission_result_file_is_complete(
            &output,
            &status,
            &["pdf".to_owned()],
            CaseCategory::PublicPdfText,
            git_sha,
            corpus,
        )
        .is_err());

        let mut wrong_provenance = valid.clone();
        wrong_provenance[1].git_sha = "other-sha".to_owned();
        write_result_records(&output, &wrong_provenance);
        assert!(!admission_result_file_is_complete(
            &output,
            &status,
            &["pdf".to_owned()],
            CaseCategory::PublicPdfText,
            git_sha,
            corpus,
        )
        .expect("provenance validation"));

        let mut duplicate_phase = valid.clone();
        duplicate_phase[2].attempt = 2;
        write_result_records(&output, &duplicate_phase);
        assert!(!admission_result_file_is_complete(
            &output,
            &status,
            &["pdf".to_owned()],
            CaseCategory::PublicPdfText,
            git_sha,
            corpus,
        )
        .expect("phase validation"));
    }

    #[tokio::test]
    async fn campaign_reconciles_before_same_instance_resume() {
        let root = tempdir().expect("temporary campaign root");
        let config = campaign_fixture(root.path());
        let git_sha = read_git_sha().expect("current git SHA");
        let mut campaign = AdmissionCampaign::open_or_create_for_test(config.clone(), &git_sha)
            .expect("initial campaign");
        campaign.status.batches[0].attempt_index = 1;
        campaign.status.batches[1].state = CampaignBatchState::Completed;
        campaign.status.completed_batches = 1;

        let output = config.output_dir.join("batch-01-attempt-01.jsonl");
        fs::create_dir_all(&config.output_dir).expect("campaign output directory");
        let records = (1..=3)
            .map(|attempt| {
                serde_json::to_string(&complete_admission_result(
                    "pdf",
                    CaseCategory::PublicPdfText,
                    "complete-orphan-run",
                    &git_sha,
                    "campaign-test",
                    attempt,
                ))
                .expect("orphan result JSON")
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&output, format!("{records}\n")).expect("orphan result");
        let run_status = RunStatus {
            schema_version: 3,
            scoring_version: SCORING_VERSION.to_owned(),
            evaluation_profile: EvaluationProfile::Admission,
            run_id: "complete-orphan-run".to_owned(),
            git_sha: git_sha.clone(),
            corpus_version: "campaign-test".to_owned(),
            dirty_worktree: false,
            planned_attempts: 3,
            completed_attempts: 3,
            actual_remote_calls: 0,
            actual_fetch_calls: 0,
            actual_search_calls: 0,
            recovery_retry_calls: 0,
            retry_limit_violation_count: 0,
            sensitive_egress_count: 0,
            stop_reason: "completed".to_owned(),
            retry_after_ms: None,
            cooldown_until_unix: None,
        };
        fs::write(
            default_status_path(&output),
            serde_json::to_vec(&run_status).expect("run status JSON"),
        )
        .expect("run status");
        super::super::execution::write_safety_checkpoint(
            &default_safety_path(&output),
            &run_status,
            0,
            0,
            true,
        )
        .expect("safety checkpoint");
        fs::write(
            &config.quota_path,
            serde_json::to_vec(&QuotaLedger {
                utc_date: Utc::now().date_naive().to_string(),
                used_calls: 0,
                cooldown_until_unix: None,
            })
            .expect("quota JSON"),
        )
        .expect("quota ledger");

        let outcome = campaign
            .run_available_for_test()
            .await
            .expect("same-instance recovery");
        assert_eq!(outcome.state, "completed");
        assert_eq!(campaign.status.actual_remote_calls, 0);
        assert!(campaign.status.batches[0].completed_evidence.is_some());
    }
}
