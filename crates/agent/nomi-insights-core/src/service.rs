//! Contribution pipeline: domain work packages only (v3).

use std::path::{Path, PathBuf};

use nomi_config::InsightsContributionConfig;
use tracing::{debug, info, warn};

use crate::client::{ContributionClient, FlushResult};
use crate::outbox::ContributionOutbox;
use crate::paths::{append_audit_event, ensure_state_dir, outbox_path};
use crate::session_skills::record_skill_touch;
use crate::skill::{SkillChangeKind, find_skill_dir_by_slug};
use crate::types::{
    ContributionBatch, ContributionEnvelope, ContributionType, INSIGHTS_CONSENT_VERSION,
    dedupe_batch_contributions, envelope_from_value,
};
use crate::work_package::{WorkPackageBuildInput, build_domain_work_package};

pub struct ContributionService {
    data_dir: PathBuf,
    config: InsightsContributionConfig,
    outbox: ContributionOutbox,
}

impl ContributionService {
    pub fn open(data_dir: PathBuf, config: InsightsContributionConfig) -> Result<Self, String> {
        ensure_state_dir(&data_dir).map_err(|e| e.to_string())?;
        let outbox = ContributionOutbox::open(&outbox_path(&data_dir))?;
        Ok(Self {
            data_dir,
            config,
            outbox,
        })
    }

    pub fn outbox_counts(&self) -> Result<crate::outbox::OutboxCounts, String> {
        self.outbox.counts()
    }

    pub fn reset_outbox(&self, clear_all: bool) -> Result<u32, String> {
        if clear_all {
            self.outbox.clear_all()
        } else {
            self.outbox.reset_sent_to_pending()
        }
    }

    pub fn config(&self) -> &InsightsContributionConfig {
        &self.config
    }

    fn audit_drop(&self, reason: &str, detail: &str) {
        warn!(reason, detail, "insights: contribution dropped");
        append_audit_event(&self.data_dir, reason, detail);
    }

    fn try_enqueue(&self, envelope: &ContributionEnvelope) -> bool {
        match self.outbox.enqueue(envelope.clone()) {
            Ok(true) => {
                info!("insights: enqueued domain_work_package");
                true
            }
            Ok(false) => {
                info!("insights: duplicate content_hash skipped (not re-enqueued)");
                false
            }
            Err(e) => {
                warn!("insights: outbox enqueue failed: {e}");
                false
            }
        }
    }

    pub fn enqueue_work_package(&self, input: &WorkPackageBuildInput) -> bool {
        if !self.config.enabled {
            warn!(work_id = %input.work_id, "insights: contribution master switch off — skip enqueue");
            return false;
        }
        let Some(package) = build_domain_work_package(input) else {
            self.audit_drop(
                "work_package_build_failed",
                &format!("work_id={}", input.work_id),
            );
            return false;
        };
        if !package_reportable(&self.config, &package.resolution) {
            self.audit_drop(
                "resolution_gate",
                &format!(
                    "work_id={} verdict={} evidence_tier={} min_tier={}",
                    input.work_id,
                    package.resolution.verdict,
                    package.resolution.evidence_tier,
                    self.config.min_evidence_tier
                ),
            );
            return false;
        }
        let collected_at = chrono::Utc::now().to_rfc3339();
        let envelope =
            match envelope_from_value(ContributionType::DomainWorkPackage, &collected_at, &package)
            {
                Ok(e) => e,
                Err(e) => {
                    self.audit_drop("serialize_error", &e);
                    return false;
                }
            };
        info!(
            work_id = %input.work_id,
            verdict = %package.resolution.verdict,
            evidence_tier = %package.resolution.evidence_tier,
            domain_key = %package.domain_poi.domain_key,
            skill_slug = %package.skill.name_slug,
            "insights: domain work package ready for outbox"
        );
        self.try_enqueue(&envelope)
    }

    pub fn preview_work_package(
        &self,
        input: &WorkPackageBuildInput,
    ) -> Option<ContributionEnvelope> {
        let package = build_domain_work_package(input)?;
        envelope_from_value(
            ContributionType::DomainWorkPackage,
            &chrono::Utc::now().to_rfc3339(),
            &package,
        )
        .ok()
    }

    pub fn preview_batch_from_inputs(&self, inputs: &[WorkPackageBuildInput]) -> ContributionBatch {
        let mut contributions = Vec::new();
        for input in inputs {
            if let Some(env) = self.preview_work_package(input) {
                contributions.push(env);
            }
        }
        contributions = dedupe_batch_contributions(contributions);
        ContributionBatch {
            batch_id: uuid::Uuid::new_v4().to_string(),
            consent_version: INSIGHTS_CONSENT_VERSION.to_string(),
            contributions,
        }
    }

    fn rebuild_work_package_envelope(
        &self,
        envelope: &ContributionEnvelope,
    ) -> Option<ContributionEnvelope> {
        if envelope.kind != ContributionType::DomainWorkPackage.as_str() {
            return None;
        }
        let name_slug = envelope
            .payload
            .get("skill")
            .and_then(|s| s.get("name_slug"))
            .and_then(|v| v.as_str())?;
        let skills_root = self.data_dir.join("skills");
        let skill_dir = find_skill_dir_by_slug(&skills_root, name_slug)?;
        let input = work_package_input_from_envelope(
            &envelope.payload,
            skill_dir,
            skills_root,
            self.config.redacted_body,
        )?;
        let package = build_domain_work_package(&input)?;
        envelope_from_value(
            ContributionType::DomainWorkPackage,
            &envelope.collected_at,
            &package,
        )
        .ok()
    }

    pub async fn flush(&self) -> Result<FlushResult, String> {
        self.flush_prepared().await
    }

    pub async fn flush_prepared(&self) -> Result<FlushResult, String> {
        if self.config.upload_skills_refresh {
            let _ = self.refresh_pending_work_packages();
        }
        let (ids, envelopes) = self.prepare_upload_envelopes(50)?;
        let client = ContributionClient::new(self.config.clone(), self.data_dir.clone());
        client
            .upload_prepared(&self.outbox, &ids, envelopes)
            .await
            .map_err(|e| e.to_string())
    }

    fn prepare_upload_envelopes(
        &self,
        limit: usize,
    ) -> Result<(Vec<String>, Vec<ContributionEnvelope>), String> {
        let pending = self.outbox.list_pending(limit)?;
        let mut ids = Vec::with_capacity(pending.len());
        let mut envelopes = Vec::with_capacity(pending.len());
        for entry in pending {
            ids.push(entry.id.clone());
            let envelope = self
                .rebuild_work_package_envelope(&entry.envelope)
                .unwrap_or(entry.envelope);
            if entry.kind == ContributionType::DomainWorkPackage.as_str() {
                let _ = self.outbox.update_envelope(&entry.id, envelope.clone());
            }
            envelopes.push(envelope);
        }
        Ok((ids, envelopes))
    }

    pub fn refresh_pending_work_packages(&self) -> Result<u32, String> {
        let pending = self.outbox.list_pending(512)?;
        let mut updated = 0;
        for entry in pending {
            if entry.kind != ContributionType::DomainWorkPackage.as_str() {
                continue;
            }
            let Some(new_envelope) = self.rebuild_work_package_envelope(&entry.envelope) else {
                continue;
            };
            self.outbox
                .update_envelope(&entry.id, new_envelope.clone())?;
            updated += 1;
        }
        Ok(updated)
    }

    pub async fn revoke_installation(&self) -> Result<(), String> {
        let client = ContributionClient::new(self.config.clone(), self.data_dir.clone());
        client
            .revoke_installation()
            .await
            .map_err(|e| e.to_string())
    }

    pub fn spawn_work_packages(
        data_dir: PathBuf,
        config: InsightsContributionConfig,
        inputs: Vec<WorkPackageBuildInput>,
    ) {
        if !config.enabled || !config.on_session_end {
            debug!(
                enabled = config.enabled,
                on_session_end = config.on_session_end,
                "insights: spawn_work_packages skipped by config"
            );
            return;
        }
        let package_count = inputs.len();
        let upload_ready = config.upload_ready();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            info!(
                package_count,
                upload_ready, "insights: processing session-end work packages"
            );
            let Ok(svc) = ContributionService::open(data_dir.clone(), config.clone()) else {
                warn!(
                    home = %data_dir.display(),
                    "insights: failed to open ContributionService for session-end upload"
                );
                return;
            };
            let mut enqueued = 0u32;
            for input in inputs {
                if svc.enqueue_work_package(&input) {
                    enqueued += 1;
                }
            }
            info!(
                enqueued,
                package_count, "insights: session-end enqueue complete"
            );
            if config.upload_ready() {
                match svc.flush().await {
                    Ok(result) => info!(
                        uploaded = result.uploaded,
                        duplicates = result.duplicates,
                        rejected = result.rejected,
                        skipped_no_endpoint = result.skipped_no_endpoint,
                        "insights: session-end flush complete"
                    ),
                    Err(e) => warn!("insights: session-end flush failed: {e}"),
                }
            } else {
                info!("insights: upload not ready — packages remain in local outbox");
            }
        });
    }

    pub fn spawn_skill_touch(skill_dir: &Path, _kind: SkillChangeKind, created: bool) {
        let data_dir = nomi_config::data_dir();
        if let Ok(content) = std::fs::read_to_string(skill_dir.join("SKILL.md")) {
            let (name, _, _) = crate::skill::parse_frontmatter_for_slug(&content);
            record_skill_touch(&data_dir, &crate::sanitize::slugify_name(&name), created);
        }
    }
}

fn work_package_input_from_envelope(
    payload: &serde_json::Value,
    skill_dir: PathBuf,
    skills_root: PathBuf,
    include_body: bool,
) -> Option<WorkPackageBuildInput> {
    let work_id = payload.get("work_id")?.as_str()?.to_string();
    let session_id_hash = payload.get("session_id_hash")?.as_str()?.to_string();
    let domain_poi: crate::types::DomainPoiPayload =
        serde_json::from_value(payload.get("domain_poi")?.clone()).ok()?;
    let resolution: crate::types::ResolutionPayload =
        serde_json::from_value(payload.get("resolution")?.clone()).ok()?;
    let work_metrics: crate::types::WorkMetricsPayload =
        serde_json::from_value(payload.get("work_metrics")?.clone()).ok()?;
    let binding_role = payload
        .get("skill")
        .and_then(|s| s.get("binding_role"))
        .and_then(|v| v.as_str())
        .unwrap_or("primary")
        .to_string();
    Some(WorkPackageBuildInput {
        work_id,
        session_id_hash,
        domain_poi,
        resolution,
        skill_dir,
        skills_root,
        binding_role,
        include_body,
        work_metrics,
    })
}

pub use crate::session_skills::{drain_session_skills, set_active_session};

fn package_reportable(
    config: &InsightsContributionConfig,
    resolution: &crate::types::ResolutionPayload,
) -> bool {
    if resolution.verdict == "indeterminate" {
        return false;
    }
    if config.verdict_excluded(&resolution.verdict) {
        return false;
    }
    if !config.evidence_tier_meets_min(&resolution.evidence_tier) {
        return false;
    }
    true
}
