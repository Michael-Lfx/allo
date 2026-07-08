//! Auxiliary client entry point (provider wiring lives in nomi-agent).

use std::time::Duration;

use nomi_types::message::Message;
use serde_json::Value;

use super::error::{AuxiliaryError, AuxiliaryResult};
use super::task::AuxiliaryTask;

#[derive(Debug, Clone, Default)]
pub struct AuxiliaryRequest {
    pub task: Option<AuxiliaryTask>,
    pub messages: Vec<Message>,
    pub tools: Vec<Value>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub timeout: Option<Duration>,
    pub extra_body: Option<Value>,
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
    pub text_content: String,
}

impl AuxiliaryResponse {
    pub fn text(&self) -> Option<&str> {
        if self.text_content.is_empty() {
            None
        } else {
            Some(&self.text_content)
        }
    }
}

/// Routes auxiliary LLM calls. The full provider chain is wired by nomi-agent;
/// this stub keeps nomi-poi compilable before agent integration lands.
#[derive(Debug, Default, Clone)]
pub struct AuxiliaryClient;

impl AuxiliaryClient {
    pub async fn call(&self, _request: AuxiliaryRequest) -> AuxiliaryResult<AuxiliaryResponse> {
        Err(AuxiliaryError::NoProviders)
    }
}
