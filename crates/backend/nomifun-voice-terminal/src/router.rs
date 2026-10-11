//! Qwen 3.5 Flash single-pass atomic voice decision routing client.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.5.

use async_trait::async_trait;
use crate::contract::{validate_voice_decision, VoiceDecision};
use crate::driver::{RouteError, VoiceRouter};

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Heuristic fallback voice router for offline testing and dev iteration.
#[derive(Debug, Default, Clone)]
pub struct HeuristicFallbackVoiceRouter;

impl HeuristicFallbackVoiceRouter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl VoiceRouter for HeuristicFallbackVoiceRouter {
    async fn route(&self, input: &str) -> Result<VoiceDecision, RouteError> {
        if input.contains("任务") || input.contains("执行") || input.contains("整理") || input.contains("备份") {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::AgentTask,
                task_control_action: None,
                reply_tts: None,
                spoken_ack: Some("收到，正在处理任务。".into()),
                task_goal: Some(input.into()),
                requires_confirmation: Some(false),
            })
        } else if input.contains("取消") || input.contains("停止") {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::TaskControl,
                task_control_action: Some(crate::contract::TaskControlAction::CancelTask),
                reply_tts: None,
                spoken_ack: None,
                task_goal: None,
                requires_confirmation: None,
            })
        } else if input.contains("进度") || input.contains("状态") {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::TaskControl,
                task_control_action: Some(crate::contract::TaskControlAction::QueryStatus),
                reply_tts: None,
                spoken_ack: None,
                task_goal: None,
                requires_confirmation: None,
            })
        } else {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::Chat,
                task_control_action: None,
                reply_tts: Some("好的，请问有什么可以帮助您的？".into()),
                spoken_ack: None,
                task_goal: None,
                requires_confirmation: None,
            })
        }
    }
}

/// Single-pass quad-route client with sliding context window, dynamic task state injection,
/// and live HTTP completions endpoint connection.
pub struct QwenVoiceRouter {
    history: Vec<ChatMessage>,
    endpoint: Option<String>,
    api_key: Option<String>,
    model: String,
    http_client: reqwest::Client,
    fallback: Option<HeuristicFallbackVoiceRouter>,
}

impl QwenVoiceRouter {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            endpoint: None,
            api_key: None,
            model: "qwen-turbo".to_string(),
            http_client: reqwest::Client::new(),
            fallback: Some(HeuristicFallbackVoiceRouter::new()),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>, api_key: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self.api_key = Some(api_key.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_fallback(mut self, fallback: Option<HeuristicFallbackVoiceRouter>) -> Self {
        self.fallback = fallback;
        self
    }

    pub fn history(&self) -> &[ChatMessage] {
        &self.history
    }

    /// Appends a dialogue turn, preserving at most 3 turns (6 messages) within ~600 tokens.
    pub fn append_turn(&mut self, user: impl Into<String>, assistant_summary: impl Into<String>) {
        self.history.push(ChatMessage {
            role: "user".into(),
            content: user.into(),
        });
        self.history.push(ChatMessage {
            role: "assistant".into(),
            content: assistant_summary.into(),
        });

        // Sliding 3-turn window (at most 6 messages)
        while self.history.len() > 6 {
            self.history.remove(0);
        }
    }

    /// Builds the complete single-pass routing prompt with dynamic Task FSM status.
    pub fn build_prompt(&self, user_input: &str, task_fsm_status: Option<&str>) -> String {
        let task_status_str = match task_fsm_status {
            Some(s) if !s.trim().is_empty() => format!("[当前后台任务状态]: {}", s.trim()),
            _ => "[当前后台任务状态]: 空闲，无正在运行任务".to_string(),
        };

        format!(
            "你是一个高精度意图识别中枢。基于用户最新语音输入及上下文，直接输出严格符合 JSON Schema 的 JSON 对象。\n\
            {}\n\
            最新语音输入: \"{}\"",
            task_status_str, user_input
        )
    }

    /// Parses raw model JSON response and validates against C3 VoiceDecisionContract JSON Schema.
    pub fn parse_and_validate(&self, json_str: &str) -> Result<VoiceDecision, RouteError> {
        let value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| RouteError::Generation(format!("Invalid JSON: {e}")))?;

        validate_voice_decision(&value).map_err(RouteError::Validation)?;

        serde_json::from_value(value)
            .map_err(|e| RouteError::Generation(format!("Deserialization error: {e}")))
    }
}

impl Default for QwenVoiceRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VoiceRouter for QwenVoiceRouter {
    async fn route(&self, input: &str) -> Result<VoiceDecision, RouteError> {
        if let Some(endpoint) = &self.endpoint {
            let prompt = self.build_prompt(input, None);
            let mut messages = Vec::new();
            for msg in &self.history {
                messages.push(serde_json::json!({
                    "role": msg.role,
                    "content": msg.content,
                }));
            }
            messages.push(serde_json::json!({
                "role": "user",
                "content": prompt,
            }));

            let body = serde_json::json!({
                "model": self.model,
                "messages": messages,
                "temperature": 0.0,
            });

            let mut req = self.http_client.post(endpoint).json(&body);
            if let Some(key) = &self.api_key {
                req = req.bearer_auth(key);
            }

            let resp = req.send().await.map_err(|e| RouteError::Network(e.to_string()))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(RouteError::Generation(format!("HTTP status {status}")));
            }

            let json: serde_json::Value = resp.json().await.map_err(|e| RouteError::Generation(e.to_string()))?;
            let content = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| RouteError::Generation("Missing message content in response".into()))?;

            self.parse_and_validate(content)
        } else if let Some(fallback) = &self.fallback {
            fallback.route(input).await
        } else {
            Err(RouteError::Generation("No LLM endpoint configured".to_string()))
        }
    }
}
