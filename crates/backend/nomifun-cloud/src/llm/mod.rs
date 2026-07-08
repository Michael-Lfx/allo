//! Remote LLM provider backed by Flowy OpenAI-compatible `/v1` API.

use std::sync::Arc;

use async_trait::async_trait;
use nomi_config::ServerConfig;
use nomi_config::compat::ProviderCompat;
use nomi_providers::openai::OpenAIProvider;
use nomi_providers::{LlmProvider, ProviderError};
use nomi_types::llm::{LlmEvent, LlmRequest};
use tokio::sync::{Mutex, mpsc};
use tracing::warn;

use crate::error::ServerClientError;
use crate::flowy::FlowyApiClient;
use crate::session::ServerSession;

/// Remote LLM gateway using JWT from [`ServerSession`] against Flowy `/v1/chat/completions`.
pub struct ServerLlmProvider {
    config: ServerConfig,
    session: ServerSession,
    api: FlowyApiClient,
    chat_session_id: Arc<Mutex<Option<String>>>,
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
        let api = FlowyApiClient::new(&config)?;
        Ok(Self {
            config: config.clone(),
            session: ServerSession::from_config(&config, data_dir),
            api,
            chat_session_id: Arc::new(Mutex::new(None)),
        })
    }

    async fn ensure_chat_session(&self) {
        let mut guard = self.chat_session_id.lock().await;
        if guard.is_some() {
            return;
        }
        let session_id = format!("nomifun-{}", uuid::Uuid::new_v4());
        match self
            .api
            .report_chat_session(&self.session, &session_id)
            .await
        {
            Ok(resp) if resp.stored => {
                *guard = Some(session_id);
            }
            Ok(_) => {
                warn!("chat session report returned stored=false; continuing anyway");
                *guard = Some(session_id);
            }
            Err(err) => {
                warn!(error = %err, "chat session report failed; continuing with LLM call");
            }
        }
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
        let mut compat = ProviderCompat::default();
        compat.supports_image = Some(true);
        Ok(OpenAIProvider::new(&token, &base, compat))
    }

    async fn resolve_model(&self, model: Option<&str>) -> String {
        model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.config.effective_default_llm_model())
    }
}

#[async_trait]
impl LlmProvider for ServerLlmProvider {
    async fn stream(
        &self,
        request: &LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        self.ensure_chat_session().await;
        let inner = self.build_inner().await.map_err(map_server_err)?;
        let mut req = request.clone();
        req.model = self.resolve_model(Some(&request.model)).await;
        inner.stream(&req).await
    }
}

impl Clone for ServerLlmProvider {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            session: self.session.clone(),
            api: FlowyApiClient::new(&self.config).expect("flowy client"),
            chat_session_id: Arc::clone(&self.chat_session_id),
        }
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
