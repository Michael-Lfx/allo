//! Remote LLM provider backed by Flowy OpenAI-compatible `/v1` API.
//!
//! Session tracing uses `X-Flowy-Session-Id` on each completions request
//! (task-local from the conversation / ViMax / eval scope). Do not report a
//! synthetic id to `POST /v1/chat/session` — that Redis slot is shared with
//! FlowyClaw and cannot represent concurrent local sessions.

use async_trait::async_trait;
use nomi_config::ServerConfig;
use nomi_config::compat::ProviderCompat;
use nomi_providers::openai::OpenAIProvider;
use nomi_providers::{LlmProvider, ProviderError, WireFacts};
use nomi_types::llm::{LlmEvent, LlmRequest};
use tokio::sync::mpsc;

use crate::error::ServerClientError;
use crate::flowy::FlowyApiClient;
use crate::session::ServerSession;

/// Remote LLM gateway using JWT from [`ServerSession`] against Flowy `/v1/chat/completions`.
#[derive(Clone)]
pub struct ServerLlmProvider {
    config: ServerConfig,
    session: ServerSession,
}

impl ServerLlmProvider {
    pub fn new(
        config: ServerConfig,
        data_dir: impl AsRef<std::path::Path>,
    ) -> Result<Self, ServerClientError> {
        if !config.enabled {
            return Err(ServerClientError::Disabled);
        }
        if !config.api_ready() {
            return Err(ServerClientError::MissingBaseUrl);
        }
        let _ = FlowyApiClient::new(&config)?;
        Ok(Self {
            config: config.clone(),
            session: ServerSession::from_config(&config, data_dir),
        })
    }

    async fn build_inner(&self) -> Result<OpenAIProvider, ServerClientError> {
        let token = self
            .session
            .access_token()
            .await?
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                ServerClientError::AuthRequired("not logged in to Flowy server".into())
            })?;

        let base = self.config.effective_llm_base_url();
        Ok(OpenAIProvider::new(&token, &base, gateway_compat()))
    }

    async fn resolve_model(&self, model: Option<&str>) -> String {
        model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.config.effective_default_llm_model())
    }
}

fn gateway_compat() -> ProviderCompat {
    let mut compat = ProviderCompat::default();
    compat.supports_image = Some(true);
    // Mirror JWT into legacy `token` and enable X-Flowy-Turn-Id /
    // X-Flowy-Session-Id injection when a Flowy attribution is scoped.
    compat.mirror_bearer_header = Some("token".to_string());
    compat
}

#[async_trait]
impl LlmProvider for ServerLlmProvider {
    fn describe_request(&self, request: &LlmRequest) -> Option<WireFacts> {
        OpenAIProvider::new("", &self.config.effective_llm_base_url(), gateway_compat())
            .describe_request(request)
    }

    async fn stream(
        &self,
        request: &LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        let inner = self.build_inner().await.map_err(map_server_err)?;
        let mut req = request.clone();
        req.model = self.resolve_model(Some(&request.model)).await;
        inner.stream(&req).await
    }
}

fn map_server_err(err: ServerClientError) -> ProviderError {
    match err {
        ServerClientError::AuthRequired(msg) => {
            ProviderError::Connection(format!("auth required: {msg}"))
        }
        other => ProviderError::Connection(other.to_string()),
    }
}
