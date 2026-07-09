//! Insights contribution HTTP DTOs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsContributionStatusResponse {
    pub enabled: bool,
    pub on_session_end: bool,
    pub auto_extract_enabled: bool,
    pub auto_extract_idle_secs: u64,
    pub min_evidence_tier: String,
    pub require_skill_binding: bool,
    pub min_work_turns: u32,
    pub redacted_body: bool,
    pub endpoint: String,
    pub auth_configured: bool,
    pub upload_ready: bool,
    pub outbox_pending: u32,
    pub outbox_failed: u32,
    pub outbox_sent: u32,
    pub installation_id: String,
    pub consent_version: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInsightsContributionRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub on_session_end: Option<bool>,
    #[serde(default)]
    pub auto_extract_enabled: Option<bool>,
    #[serde(default)]
    pub auto_extract_idle_secs: Option<u64>,
    #[serde(default)]
    pub redacted_body: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsFlushResponse {
    pub uploaded: u32,
    pub duplicates: u32,
    pub rejected: u32,
    pub skipped_no_endpoint: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsResetOutboxRequest {
    #[serde(default)]
    pub clear_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsResetOutboxResponse {
    pub affected: u32,
    pub outbox_pending: u32,
    pub outbox_failed: u32,
    pub outbox_sent: u32,
}
