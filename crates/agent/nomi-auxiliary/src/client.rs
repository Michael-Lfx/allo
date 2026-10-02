use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use nomi_types::message::{ContentBlock, Message, Role};
use tokio::time::timeout;

use super::error::{AuxiliaryError, AuxiliaryResult};
use super::task::AuxiliaryTask;

#[async_trait]
pub trait ChatLlmProvider: Send + Sync {
    async fn chat_completion(
        &self,
        messages: &[Message],
        max_tokens: Option<u32>,
        temperature: Option<f64>,
        model: Option<&str>,
    ) -> Result<String, String>;
}

/// Observer + conversation binding for one auxiliary request batch.
#[derive(Clone)]
pub struct AuxiliaryLlmObservation {
    pub observer: Arc<dyn AuxiliaryLlmObserver>,
    pub context: AuxiliaryObservationContext,
}

/// Conversation binding for auxiliary LLM observation (same JSONL file as the main agent).
#[derive(Debug, Clone, Default)]
pub struct AuxiliaryObservationContext {
    pub conversation_id: Option<String>,
    pub session_kind: Option<String>,
}

/// Records non-streaming auxiliary LLM calls into session observation.
pub trait AuxiliaryLlmObserver: Send + Sync {
    fn on_llm_request(
        &self,
        ctx: &AuxiliaryObservationContext,
        call_kind: &str,
        observation_scope: &str,
        model: &str,
        messages: &[Message],
    ) -> String;

    fn on_llm_response(
        &self,
        model_call_id: &str,
        call_kind: &str,
        text: &str,
        elapsed_ms: u64,
        error: Option<&str>,
    );
}

#[derive(Clone, Default)]
pub struct AuxiliaryRequest {
    pub task: Option<AuxiliaryTask>,
    pub messages: Vec<Message>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub timeout: Option<Duration>,
    pub observation: Option<Arc<dyn AuxiliaryLlmObserver>>,
    pub observation_context: AuxiliaryObservationContext,
    /// When set, overrides task-derived `call_kind` in observation events.
    pub observation_call_kind: Option<String>,
}

impl std::fmt::Debug for AuxiliaryRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuxiliaryRequest")
            .field("task", &self.task)
            .field("messages", &self.messages)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("timeout", &self.timeout)
            .field(
                "observation",
                &self.observation.as_ref().map(|_| "AuxiliaryLlmObserver"),
            )
            .field("observation_context", &self.observation_context)
            .field("observation_call_kind", &self.observation_call_kind)
            .finish()
    }
}

impl AuxiliaryRequest {
    pub fn new(task: AuxiliaryTask, messages: Vec<Message>) -> Self {
        Self {
            task: Some(task),
            messages,
            ..Default::default()
        }
    }

    pub fn with_temperature(mut self, t: f64) -> Self {
        self.temperature = Some(t);
        self
    }

    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    pub fn with_observation(
        mut self,
        observer: Arc<dyn AuxiliaryLlmObserver>,
        context: AuxiliaryObservationContext,
    ) -> Self {
        self.observation = Some(observer);
        self.observation_context = context;
        self
    }

    fn observation_call_kind(&self) -> String {
        if let Some(kind) = &self.observation_call_kind {
            return kind.clone();
        }
        match &self.task {
            Some(AuxiliaryTask::Custom(name)) => name.clone(),
            Some(task) => task.as_key().to_string(),
            None => "auxiliary".to_string(),
        }
    }

    fn observation_scope_key(&self) -> &'static str {
        if self
            .observation_context
            .conversation_id
            .as_deref()
            .is_some_and(|id| !id.is_empty())
        {
            "session_auxiliary"
        } else {
            "process_diagnostic"
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuxiliaryResponse {
    pub provider_label: String,
    pub model: String,
    pub text: String,
}

impl AuxiliaryResponse {
    pub fn text(&self) -> Option<&str> {
        if self.text.is_empty() {
            None
        } else {
            Some(self.text.as_str())
        }
    }
}

pub struct AuxiliaryClient {
    provider: Arc<dyn ChatLlmProvider>,
    label: String,
    default_model: String,
}

pub struct AuxiliaryClientBuilder {
    provider: Option<Arc<dyn ChatLlmProvider>>,
    label: String,
    default_model: String,
}

impl Default for AuxiliaryClientBuilder {
    fn default() -> Self {
        Self {
            provider: None,
            label: "default".into(),
            default_model: String::new(),
        }
    }
}

impl AuxiliaryClientBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn provider(mut self, provider: Arc<dyn ChatLlmProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    pub fn build(self) -> AuxiliaryResult<AuxiliaryClient> {
        let provider = self.provider.ok_or_else(|| {
            AuxiliaryError::NoProviderAvailable {
                tried: vec![self.label.clone()],
            }
        })?;
        Ok(AuxiliaryClient {
            provider,
            label: self.label,
            default_model: self.default_model,
        })
    }
}

impl AuxiliaryClient {
    pub fn builder() -> AuxiliaryClientBuilder {
        AuxiliaryClientBuilder::new()
    }

    pub async fn call(&self, request: AuxiliaryRequest) -> AuxiliaryResult<AuxiliaryResponse> {
        if request.messages.is_empty() {
            return Err(AuxiliaryError::InvalidRequest(
                "messages must not be empty".into(),
            ));
        }

        let wall = request
            .task
            .as_ref()
            .map(AuxiliaryTask::default_timeout)
            .unwrap_or_else(|| Duration::from_secs(30));
        let wall = request.timeout.unwrap_or(wall);

        let model = request
            .model
            .as_deref()
            .filter(|m| !m.is_empty())
            .or_else(|| {
                if self.default_model.is_empty() {
                    None
                } else {
                    Some(self.default_model.as_str())
                }
            });

        let model_label = model.unwrap_or("auto");
        let observation_call_kind = request.observation_call_kind();
        let model_call_id = request.observation.as_ref().map(|observer| {
            observer.on_llm_request(
                &request.observation_context,
                &observation_call_kind,
                request.observation_scope_key(),
                model_label,
                &request.messages,
            )
        });

        let started = Instant::now();
        let fut = self.provider.chat_completion(
            &request.messages,
            request.max_tokens,
            request.temperature,
            model,
        );

        let text = match timeout(wall, fut).await {
            Ok(Ok(text)) => text,
            Ok(Err(reason)) => {
                if let (Some(observer), Some(id)) = (&request.observation, model_call_id.as_deref())
                {
                    let elapsed_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    observer.on_llm_response(id, &observation_call_kind, "", elapsed_ms, Some(&reason));
                }
                return Err(AuxiliaryError::Llm {
                    provider: self.label.clone(),
                    reason,
                });
            }
            Err(_) => {
                if let (Some(observer), Some(id)) = (&request.observation, model_call_id.as_deref())
                {
                    let elapsed_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    observer.on_llm_response(
                        id,
                        &observation_call_kind,
                        "",
                        elapsed_ms,
                        Some("timeout"),
                    );
                }
                return Err(AuxiliaryError::Timeout(wall));
            }
        };

        if let (Some(observer), Some(id)) = (&request.observation, model_call_id.as_deref()) {
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            observer.on_llm_response(id, &observation_call_kind, &text, elapsed_ms, None);
        }

        Ok(AuxiliaryResponse {
            provider_label: self.label.clone(),
            model: model_label.to_string(),
            text,
        })
    }
}

pub fn text_message(role: Role, text: impl Into<String>) -> Message {
    Message::new(
        role,
        vec![ContentBlock::Text {
            text: text.into(),
        }],
    )
}
