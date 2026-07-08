use std::sync::Arc;
use std::time::Duration;

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

#[derive(Debug, Clone, Default)]
pub struct AuxiliaryRequest {
    pub task: Option<AuxiliaryTask>,
    pub messages: Vec<Message>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub timeout: Option<Duration>,
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

        let fut = self.provider.chat_completion(
            &request.messages,
            request.max_tokens,
            request.temperature,
            model,
        );

        let text = match timeout(wall, fut).await {
            Ok(Ok(text)) => text,
            Ok(Err(reason)) => {
                return Err(AuxiliaryError::Llm {
                    provider: self.label.clone(),
                    reason,
                });
            }
            Err(_) => return Err(AuxiliaryError::Timeout(wall)),
        };

        Ok(AuxiliaryResponse {
            provider_label: self.label.clone(),
            model: model.unwrap_or("auto").to_string(),
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
