//! REST client for ops server batch ingest and installation revocation.

use std::time::Duration;

use nomi_config::InsightsContributionConfig;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use tracing::{debug, warn};

use crate::paths::{last_batch_path, load_or_create_installation_id};
use crate::response::parse_batch_upload_response;
use crate::types::ContributionEnvelope;
use crate::types::{BatchUploadResponse, ContributionBatch, dedupe_batch_contributions};

#[derive(Debug, Clone, Default)]
pub struct FlushResult {
    pub uploaded: u32,
    pub duplicates: u32,
    pub rejected: u32,
    pub skipped_no_endpoint: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ContributionClientError {
    #[error("contribution endpoint not configured")]
    NoEndpoint,
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("server returned {status}: {body}")]
    Server { status: u16, body: String },
}

pub struct ContributionClient {
    http: reqwest::Client,
    config: InsightsContributionConfig,
    data_dir: std::path::PathBuf,
}

impl ContributionClient {
    pub fn new(config: InsightsContributionConfig, data_dir: std::path::PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("nomifun/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            config,
            data_dir,
        }
    }

    pub fn upload_ready(&self) -> bool {
        self.config.upload_ready()
    }

    fn build_headers(&self) -> Result<HeaderMap, ContributionClientError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let installation_id = load_or_create_installation_id(&self.data_dir)
            .map_err(|e| ContributionClientError::Http(format!("installation id: {e}")))?;
        headers.insert(
            "X-Installation-Id",
            HeaderValue::from_str(&installation_id)
                .map_err(|e| ContributionClientError::Http(format!("installation header: {e}")))?,
        );
        headers.insert(
            "X-Client-Version",
            HeaderValue::from_str(&format!("nomifun/{}", env!("CARGO_PKG_VERSION")))
                .map_err(|e| ContributionClientError::Http(e.to_string()))?,
        );
        if let Some(token) = self.config.effective_token() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|e| ContributionClientError::Http(format!("auth header: {e}")))?,
            );
        }
        Ok(headers)
    }

    /// POST `/v1/insights/batch` (full URL from config `endpoint`).
    pub async fn upload_batch(
        &self,
        batch: &ContributionBatch,
    ) -> Result<BatchUploadResponse, ContributionClientError> {
        let endpoint = self.config.endpoint.trim();
        if endpoint.is_empty() {
            return Err(ContributionClientError::NoEndpoint);
        }
        let headers = self.build_headers()?;
        debug!(endpoint = %endpoint, count = batch.contributions.len(), "insights upload batch");
        let resp = self
            .http
            .post(endpoint)
            .headers(headers)
            .json(batch)
            .send()
            .await
            .map_err(|e| ContributionClientError::Http(e.to_string()))?;
        let status = resp.status();
        if status.as_u16() == 409 {
            // Idempotent duplicate batch — treat as success.
            return Ok(BatchUploadResponse {
                accepted: batch.contributions.len() as u32,
                duplicates: batch.contributions.len() as u32,
                rejected: vec![],
            });
        }
        let status_code = status.as_u16();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ContributionClientError::Server {
                status: status_code,
                body,
            });
        }
        parse_batch_upload_response(&body, batch.contributions.len()).map_err(|e| {
            ContributionClientError::Http(format!(
                "HTTP {status_code} but response body is not a recognized batch JSON: {e}"
            ))
        })
    }

    /// DELETE `/v1/installations/{id}` — derived from batch endpoint base URL.
    pub async fn revoke_installation(&self) -> Result<(), ContributionClientError> {
        let endpoint = self.config.endpoint.trim();
        if endpoint.is_empty() {
            return Err(ContributionClientError::NoEndpoint);
        }
        let base = endpoint
            .trim_end_matches("/v1/insights/batch")
            .trim_end_matches('/');
        let installation_id = load_or_create_installation_id(&self.data_dir)
            .map_err(|e| ContributionClientError::Http(format!("installation id: {e}")))?;
        let url = format!("{base}/v1/installations/{installation_id}");
        let headers = self.build_headers()?;
        let resp = self
            .http
            .delete(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| ContributionClientError::Http(e.to_string()))?;
        if resp.status().is_success() || resp.status().as_u16() == 404 {
            Ok(())
        } else {
            let status_code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(ContributionClientError::Server {
                status: status_code,
                body,
            })
        }
    }
}

fn log_skill_upload_payloads(contributions: &[ContributionEnvelope]) {
    for env in contributions {
        if env.kind != "skill_pattern" {
            continue;
        }
        let refs = env
            .payload
            .get("references_redacted")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let name = env
            .payload
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        debug!(
            display_name = name,
            refs_count = refs,
            content_hash = %env.content_hash,
            "insights upload skill_pattern payload"
        );
    }
}

impl ContributionClient {
    fn write_last_batch_debug(&self, batch: &ContributionBatch) {
        let path = last_batch_path(&self.data_dir);
        if let Ok(json) = serde_json::to_string_pretty(batch) {
            if let Err(e) = std::fs::write(&path, json) {
                debug!(path = %path.display(), "insights last_batch write failed: {e}");
            } else {
                debug!(path = %path.display(), "insights wrote last_batch.json");
            }
        }
    }

    /// Upload prepared envelopes (used after disk rebuild in [`ContributionService::flush`]).
    pub async fn upload_prepared(
        &self,
        outbox: &crate::outbox::ContributionOutbox,
        ids: &[String],
        contributions: Vec<ContributionEnvelope>,
    ) -> Result<FlushResult, ContributionClientError> {
        if !self.upload_ready() {
            return Ok(FlushResult {
                skipped_no_endpoint: true,
                ..Default::default()
            });
        }
        if contributions.is_empty() {
            return Ok(FlushResult::default());
        }
        let batch = ContributionBatch {
            batch_id: uuid::Uuid::new_v4().to_string(),
            consent_version: crate::types::INSIGHTS_CONSENT_VERSION.to_string(),
            contributions: dedupe_batch_contributions(contributions),
        };
        log_skill_upload_payloads(&batch.contributions);
        self.write_last_batch_debug(&batch);
        match self.upload_batch(&batch).await {
            Ok(resp) => {
                outbox
                    .mark_sent(ids)
                    .map_err(|e| ContributionClientError::Http(e))?;
                Ok(FlushResult {
                    uploaded: resp.accepted,
                    duplicates: resp.duplicates,
                    rejected: resp.rejected.len() as u32,
                    skipped_no_endpoint: false,
                })
            }
            Err(e) => {
                warn!("insights flush failed: {e}");
                let _ = outbox.mark_failed(ids);
                Err(e)
            }
        }
    }

    pub async fn flush_outbox(
        &self,
        outbox: &crate::outbox::ContributionOutbox,
        batch_size: usize,
    ) -> Result<FlushResult, ContributionClientError> {
        if !self.upload_ready() {
            return Ok(FlushResult {
                skipped_no_endpoint: true,
                ..Default::default()
            });
        }
        let pending = outbox
            .list_pending(batch_size)
            .map_err(|e| ContributionClientError::Http(e))?;
        if pending.is_empty() {
            return Ok(FlushResult::default());
        }
        let ids: Vec<String> = pending.iter().map(|e| e.id.clone()).collect();
        let contributions: Vec<ContributionEnvelope> =
            pending.into_iter().map(|e| e.envelope).collect();
        self.upload_prepared(outbox, &ids, contributions).await
    }
}
