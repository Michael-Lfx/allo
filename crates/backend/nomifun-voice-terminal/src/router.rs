//! Qwen 3.8 Flash single-pass atomic voice decision routing client.
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md §3.5.

use crate::contract::{VOICE_DECISION_SCHEMA, VoiceDecision};
use crate::driver::{RouteError, VoiceRouter};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Heuristic fallback voice router for offline testing and dev iteration (gated behind voice-skeleton feature).
#[cfg(any(test, feature = "voice-skeleton"))]
#[derive(Debug, Default, Clone)]
pub struct HeuristicFallbackVoiceRouter;

#[cfg(any(test, feature = "voice-skeleton"))]
impl HeuristicFallbackVoiceRouter {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
#[async_trait]
impl VoiceRouter for HeuristicFallbackVoiceRouter {
    async fn route(&self, input: &str) -> Result<VoiceDecision, RouteError> {
        self.route_with_status(input, None).await
    }

    async fn route_with_status(
        &self,
        input: &str,
        _task_status: Option<&str>,
    ) -> Result<VoiceDecision, RouteError> {
        if input.contains("任务")
            || input.contains("执行")
            || input.contains("整理")
            || input.contains("备份")
        {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::AgentTask,
                understood_request: None,
                task_control_action: None,
                reply_tts: None,
                spoken_ack: Some("收到，正在处理任务。".into()),
                task_goal: Some(input.into()),
                requires_confirmation: Some(false),
            })
        } else if input.contains("取消") || input.contains("停止") {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::TaskControl,
                understood_request: None,
                task_control_action: Some(crate::contract::TaskControlAction::CancelTask),
                reply_tts: None,
                spoken_ack: None,
                task_goal: None,
                requires_confirmation: None,
            })
        } else if input.contains("进度")
            || input.contains("状态")
            || input.contains("重听")
            || input.contains("再播一遍")
            || input.contains("再说一遍")
            || input.contains("刚才说了什么")
            || input.contains("再读一遍")
        {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::TaskControl,
                understood_request: None,
                task_control_action: Some(crate::contract::TaskControlAction::QueryStatus),
                reply_tts: None,
                spoken_ack: None,
                task_goal: None,
                requires_confirmation: None,
            })
        } else {
            Ok(VoiceDecision {
                intent_type: crate::contract::IntentType::Chat,
                understood_request: None,
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
/// Single-pass quad-route client with sliding context window, dynamic task state injection,
/// and live HTTP completions endpoint connection (supporting Flowy Cloud, DashScope, and OpenAI-compatible gateways).
#[derive(Clone)]
pub struct OpenAiCompatibleVoiceRouter {
    completion: Option<std::sync::Arc<dyn crate::completion::VoiceCompletionClient>>,
    history: Vec<ChatMessage>,
    endpoint: Option<String>,
    api_key: Option<String>,
    model: String,
    extra_headers: Vec<(String, String)>,
    http_client: reqwest::Client,
    #[cfg(any(test, feature = "voice-skeleton"))]
    fallback: Option<HeuristicFallbackVoiceRouter>,
}

/// Backward-compatible type alias.
pub type QwenVoiceRouter = OpenAiCompatibleVoiceRouter;

impl OpenAiCompatibleVoiceRouter {
    async fn route_request(&self,scope:&nomifun_api_types::voice::VoiceDialogueRequest,input:&str,task_status:Option<&str>,context:&[ChatMessage],sentences:Option<tokio::sync::mpsc::Sender<nomifun_api_types::voice::VoiceDialogueSentence>>)->(Result<VoiceDecision,RouteError>,Option<String>) {
        let Some(client) = &self.completion else {
            return self.route_with_context(input, task_status, context).await;
        };
        let mut router = self.clone();
        router.history.clear();
        for pair in context.chunks_exact(2).rev().take(3).rev() {
            router.append_turn(&pair[0].content, &pair[1].content);
        }
        let mut request = scope.clone();
        request.prompt = format!("对话上下文：\n{}\n{}",router.history.iter()
            .map(|m|format!("{}: {}",m.role,m.content)).collect::<Vec<_>>().join("\n"),router.build_prompt(input,task_status));
        let result=match sentences {Some(sender)=>client.complete_stream(&request,sender).await,None=>client.complete(&request).await};
        match result {
            Ok(content) => (self.parse_and_validate(&content),Some(content)),
            Err(RouteError::Record{code,content}) => (Err(RouteError::Record{code,content:content.clone()}),Some(content)),
            Err(error) => (Err(error),None),
        }
    }

    pub fn with_completion(
        completion: std::sync::Arc<dyn crate::completion::VoiceCompletionClient>,
    ) -> Self {
        Self {
            completion: Some(completion),
            history: Vec::new(),
            endpoint: None,
            api_key: None,
            model: String::new(),
            extra_headers: Vec::new(),
            http_client: reqwest::Client::new(),
            #[cfg(any(test, feature = "voice-skeleton"))]
            fallback: None,
        }
    }
    pub fn new() -> Self {
        Self::from_env()
    }

    pub fn with_default_model(default_model: impl Into<String>) -> Self {
        let mut router = Self::from_env();
        if std::env::var("AI_MATE_LLM_MODEL").is_err()
            && std::env::var("DASHSCOPE_MODEL").is_err()
            && std::env::var("FLOWY_LLM_MODEL").is_err()
        {
            router.model = default_model.into();
        }
        router
    }

    /// Creates a router instance specifically configured for Flowy Cloud.
    ///
    /// Flowy Cloud endpoint: `{base_url}/v1/chat/completions` (or full URL).
    /// Flowy Cloud uses the user's JWT with Bearer authorization and mirrors `token: <jwt>`.
    pub fn from_flowy_cloud(
        endpoint: impl Into<String>,
        jwt_token: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let ep = endpoint.into();
        let completions_url = if ep.ends_with("/chat/completions") {
            ep
        } else {
            format!("{}/chat/completions", ep.trim_end_matches('/'))
        };
        let token = jwt_token.into();

        Self {
            history: Vec::new(),
            endpoint: Some(completions_url),
            completion: None,
            api_key: Some(token.clone()),
            model: model.into(),
            extra_headers: vec![("token".to_string(), token)],
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            #[cfg(any(test, feature = "voice-skeleton"))]
            fallback: Some(HeuristicFallbackVoiceRouter::new()),
        }
    }

    /// Creates a router instance for a custom OpenAI-compatible endpoint with optional key.
    pub fn from_custom_endpoint(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let key = api_key.into();
        Self {
            history: Vec::new(),
            endpoint: Some(endpoint.into()),
            completion: None,
            api_key: if key.trim().is_empty() {
                None
            } else {
                Some(key)
            },
            model: model.into(),
            extra_headers: Vec::new(),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            #[cfg(any(test, feature = "voice-skeleton"))]
            fallback: Some(HeuristicFallbackVoiceRouter::new()),
        }
    }

    /// Inspects environment variables in order of priority:
    /// 1. Flowy Cloud token: `NOMIFUN_SERVER_TOKEN` or `FLOWY_SERVER_TOKEN`
    /// 2. DashScope API key: `DASHSCOPE_API_KEY`
    /// 3. OpenAI API key: `OPENAI_API_KEY`
    /// 4. Generic completions endpoint: `AI_MATE_LLM_ENDPOINT`
    /// 5. Offline fallback
    pub fn from_env() -> Self {
        if let Ok(endpoint) = std::env::var("AI_MATE_DIALOGUE_ENDPOINT") {
            let credential = std::env::var("AI_MATE_CONTROL_TOKEN").unwrap_or_default();
            if let Ok(client) = crate::completion::HostDialogueClient::new(endpoint, credential) {
                return Self::with_completion(std::sync::Arc::new(client));
            }
            // Host-managed mode must not fall back to guessing another model.
            return Self {
                completion: None,
                history: Vec::new(),
                endpoint: None,
                api_key: None,
                model: String::new(),
                extra_headers: Vec::new(),
                http_client: reqwest::Client::new(),
                #[cfg(any(test, feature = "voice-skeleton"))]
                fallback: None,
            };
        }
        let model = std::env::var("AI_MATE_LLM_MODEL")
            .or_else(|_| std::env::var("DASHSCOPE_MODEL"))
            .or_else(|_| std::env::var("FLOWY_LLM_MODEL"))
            .unwrap_or_else(|_| "qwen3.8-flash".to_string());

        // 1. Priority 1: Flowy Cloud user token
        if let Ok(token) =
            std::env::var("NOMIFUN_SERVER_TOKEN").or_else(|_| std::env::var("FLOWY_SERVER_TOKEN"))
        {
            if !token.trim().is_empty() {
                let endpoint = std::env::var("FLOWY_COMPLETIONS_URL")
                    .or_else(|_| std::env::var("AI_MATE_LLM_ENDPOINT"))
                    .unwrap_or_else(|_| {
                        "https://server.flowyaipc.com/claw/v1/chat/completions".to_string()
                    });
                return Self::from_flowy_cloud(endpoint, token, model);
            }
        }

        // 2. Priority 2: DashScope API key (direct Alibaba Cloud)
        if let Ok(key) = std::env::var("DASHSCOPE_API_KEY") {
            if !key.trim().is_empty() {
                let endpoint = std::env::var("DASHSCOPE_COMPLETIONS_URL")
                    .or_else(|_| std::env::var("AI_MATE_LLM_ENDPOINT"))
                    .unwrap_or_else(|_| {
                        "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
                            .to_string()
                    });
                return Self::from_custom_endpoint(endpoint, key, model);
            }
        }

        // 3. Priority 3: Standard OpenAI API key / Base URL
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if !key.trim().is_empty() {
                let endpoint = std::env::var("OPENAI_BASE_URL")
                    .map(|u| format!("{}/chat/completions", u.trim_end_matches('/')))
                    .or_else(|_| std::env::var("AI_MATE_LLM_ENDPOINT"))
                    .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
                return Self::from_custom_endpoint(endpoint, key, model);
            }
        }

        // 4. Priority 4: Generic AI_MATE_LLM_ENDPOINT (e.g. local Ollama / vLLM without key)
        if let Ok(endpoint) = std::env::var("AI_MATE_LLM_ENDPOINT") {
            if !endpoint.trim().is_empty() {
                let key = std::env::var("AI_MATE_LLM_KEY").unwrap_or_default();
                return Self::from_custom_endpoint(endpoint, key, model);
            }
        }

        // 5. Priority 5: Auto-detect logged-in Flowy Cloud session in local data directory
        #[cfg(not(test))]
        if let Some(token) = try_detect_local_flowy_token() {
            let endpoint = std::env::var("FLOWY_COMPLETIONS_URL")
                .or_else(|_| std::env::var("AI_MATE_LLM_ENDPOINT"))
                .unwrap_or_else(|_| {
                    "https://server.flowyaipc.com/claw/v1/chat/completions".to_string()
                });
            tracing::info!("Auto-detected Flowy Cloud user token from local session store");
            return Self::from_flowy_cloud(endpoint, token, model);
        }

        // 6. Default empty / offline fallback
        Self {
            history: Vec::new(),
            endpoint: None,
            completion: None,
            api_key: None,
            model,
            extra_headers: Vec::new(),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            #[cfg(any(test, feature = "voice-skeleton"))]
            fallback: Some(HeuristicFallbackVoiceRouter::new()),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    pub fn is_cloud_connected(&self) -> bool {
        self.endpoint.is_some()
    }

    pub fn with_endpoint(
        mut self,
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        self.endpoint = Some(endpoint.into());
        let k = api_key.into();
        self.api_key = if k.trim().is_empty() { None } else { Some(k) };
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }

    #[cfg(any(test, feature = "voice-skeleton"))]
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

    /// Builds the complete single-pass routing prompt with dynamic Task FSM status and schema instructions.
    pub fn build_prompt(&self, user_input: &str, task_fsm_status: Option<&str>) -> String {
        let task_status_str = match task_fsm_status {
            Some(s) if !s.trim().is_empty() => format!("[当前后台任务状态]: {}", s.trim()),
            _ => "[当前后台任务状态]: 空闲，无正在运行任务".to_string(),
        };

        format!(
            "你是一个高精度语音意图识别与路由中枢。基于用户最新语音输入及对话上下文，直接输出严格符合下列 JSON Schema 规范的纯 JSON 对象（不要使用 Markdown 代码块，不要包含任何前导或后置解释）：\n\
            ```json\n\
            {}\n\
            ```\n\
            字段说明与约束：\n\
            - intent_type: 必须为 \"ignore\" | \"chat\" | \"agent_task\" | \"task_control\" 之一。\n\
            - 当 intent_type 为 \"chat\" 时，必须提供 reply_tts (简短口语回答，<=60字)。\n\
            - 对可理解的输入，必须同时提供 understood_request：用一句自然语言忠实整理用户的问题或需求，保留关键对象和条件，不补造地点、日期或未说出的事实。reply_tts 直接回答该需求，不要求用户再确认；同一次输出同时返回理解和回答。\n\
            - 当 intent_type 为 \"agent_task\" 时，必须提供 task_goal (规整的目标描述) 和 spoken_ack (接单口语回执，<=25字)，requires_confirmation 标明是否涉及高危或破坏性操作。\n\
            - 当 intent_type 为 \"task_control\" 时，必须提供 task_control_action (\"query_status\" 或 \"cancel_task\")。\n\
            - 当用户输入无关噪音或无意义内容时，intent_type 为 \"ignore\"。\n\
            {}\n\
            最新语音输入: \"{}\"",
            VOICE_DECISION_SCHEMA, task_status_str, user_input
        )
    }

    /// Parses raw model JSON response and validates against C3 VoiceDecisionContract JSON Schema.
    pub fn parse_and_validate(&self, raw_str: &str) -> Result<VoiceDecision, RouteError> {
        nomifun_api_types::voice::parse_voice_decision(raw_str).map_err(RouteError::Generation)
    }

    async fn request_model_text(
        &self,
        endpoint: &str,
        input: &str,
        task_status: Option<&str>,
    ) -> Result<String, RouteError> {
        let mut messages: Vec<_> = self
            .history
            .iter()
            .map(|msg| serde_json::json!({ "role": msg.role, "content": msg.content }))
            .collect();
        messages.push(
            serde_json::json!({ "role": "user", "content": self.build_prompt(input, task_status) }),
        );
        let body =
            serde_json::json!({ "model": self.model, "messages": messages, "temperature": 0.0 });
        let mut request = self.http_client.post(endpoint).json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        for (name, value) in &self.extra_headers {
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|err| RouteError::Network(err.to_string()))?;
        if !response.status().is_success() {
            return Err(RouteError::Generation(format!(
                "HTTP status {}",
                response.status()
            )));
        }
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|err| RouteError::Generation(err.to_string()))?;
        json["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| RouteError::Generation("Missing message content in response".into()))
    }
}

impl Default for QwenVoiceRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod reply_contract_tests {
    use super::*;
    #[test]
    fn chat_without_an_actual_reply_is_rejected() {
        let router = QwenVoiceRouter::from_custom_endpoint("http://127.0.0.1:1", "", "fixture");
        assert!(
            router
                .parse_and_validate(r#"{"intent_type":"chat","reply_tts":""}"#)
                .is_err()
        );
        assert!(
            router
                .parse_and_validate(r#"{"intent_type":"chat"}"#)
                .is_err()
        );
    }
}

#[async_trait]
impl VoiceRouter for QwenVoiceRouter {
    async fn route_scoped(
        &self, scope: &nomifun_api_types::voice::VoiceDialogueRequest,
        input: &str, task_status: Option<&str>, context: &[ChatMessage],
    ) -> (Result<VoiceDecision, RouteError>, Option<String>) {
        self.route_request(scope,input,task_status,context,None).await
    }
    async fn route_scoped_stream(&self,scope:&nomifun_api_types::voice::VoiceDialogueRequest,input:&str,task_status:Option<&str>,context:&[ChatMessage],sentences:tokio::sync::mpsc::Sender<nomifun_api_types::voice::VoiceDialogueSentence>)->(Result<VoiceDecision,RouteError>,Option<String>) {
        self.route_request(scope,input,task_status,context,Some(sentences)).await
    }
    async fn route_with_context(
        &self,
        input: &str,
        task_status: Option<&str>,
        context: &[ChatMessage],
    ) -> (Result<VoiceDecision, RouteError>, Option<String>) {
        let mut request = self.clone();
        request.history.clear();
        for pair in context.chunks_exact(2).rev().take(3).rev() {
            request.append_turn(&pair[0].content, &pair[1].content);
        }
        request.route_with_output(input, task_status).await
    }
    fn is_configured(&self) -> bool {
        self.completion.is_some() || self.endpoint.is_some()
    }
    async fn route(&self, input: &str) -> Result<VoiceDecision, RouteError> {
        self.route_with_status(input, None).await
    }

    async fn route_with_status(
        &self,
        input: &str,
        task_status: Option<&str>,
    ) -> Result<VoiceDecision, RouteError> {
        self.route_with_output(input, task_status).await.0
    }

    async fn route_with_output(
        &self,
        input: &str,
        task_status: Option<&str>,
    ) -> (Result<VoiceDecision, RouteError>, Option<String>) {
        if let Some(client) = &self.completion {
            let prompt = if self.history.is_empty() {
                self.build_prompt(input, task_status)
            } else {
                format!(
                    "对话上下文：\n{}\n{}",
                    self.history
                        .iter()
                        .map(|m| format!("{}: {}", m.role, m.content))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    self.build_prompt(input, task_status)
                )
            };
            return match client.complete(&nomifun_api_types::voice::VoiceDialogueRequest::new(prompt)).await {
                Ok(content) => (self.parse_and_validate(&content), Some(content)),
                Err(error) => (Err(error), None),
            };
        }
        if let Some(endpoint) = &self.endpoint {
            match self.request_model_text(endpoint, input, task_status).await {
                Ok(content) => (self.parse_and_validate(&content), Some(content)),
                Err(error) => (Err(error), None),
            }
        } else {
            #[cfg(any(test, feature = "voice-skeleton"))]
            if let Some(fallback) = &self.fallback {
                return (fallback.route_with_status(input, task_status).await, None);
            }

            (
                Err(RouteError::Generation(
                    "No LLM endpoint configured".to_string(),
                )),
                None,
            )
        }
    }
}

/// Helper to probe the local filesystem for an active Flowy Cloud login session token.
pub fn try_detect_local_flowy_token() -> Option<String> {
    use base64::Engine;
    let mut candidate_dirs = Vec::new();
    if let Some(dir) = nomifun_common::storage_paths::resolve_data_dir_from_env() {
        candidate_dirs.push(dir);
    }
    candidate_dirs.push(nomifun_common::storage_paths::default_data_dir(""));
    candidate_dirs.push(nomifun_common::storage_paths::default_data_dir("-dev"));

    for data_dir in candidate_dirs {
        let token_path = data_dir.join("auth").join("tokens.json");
        if token_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&token_path) {
                // 1. Plaintext JSON
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(tok) = val
                        .get("server")
                        .and_then(|s| s.get("access_token"))
                        .and_then(|t| t.as_str())
                    {
                        if !tok.trim().is_empty() {
                            return Some(tok.trim().to_string());
                        }
                    }
                    if let Some(tok) = val.get("access_token").and_then(|t| t.as_str()) {
                        if !tok.trim().is_empty() {
                            return Some(tok.trim().to_string());
                        }
                    }
                }

                // 2. Encrypted envelope with companion .key file
                let key_path = token_path.with_extension("key");
                let key_opt = if let Ok(env_key) = std::env::var("NOMIFUN_TOKEN_STORE_KEY_B64") {
                    base64::engine::general_purpose::STANDARD
                        .decode(env_key.trim())
                        .ok()
                } else if key_path.exists() {
                    if let Ok(key_raw) = std::fs::read_to_string(&key_path) {
                        base64::engine::general_purpose::STANDARD
                            .decode(key_raw.trim())
                            .ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(key_bytes) = key_opt {
                    if key_bytes.len() == 32 {
                        if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&raw) {
                            if let (Some(nonce_b64), Some(cipher_b64)) = (
                                envelope.get("nonce_b64").and_then(|v| v.as_str()),
                                envelope.get("ciphertext_b64").and_then(|v| v.as_str()),
                            ) {
                                if let (Ok(nonce_bytes), Ok(cipher_bytes)) = (
                                    base64::engine::general_purpose::STANDARD.decode(nonce_b64),
                                    base64::engine::general_purpose::STANDARD.decode(cipher_b64),
                                ) {
                                    let mut combined = nonce_bytes;
                                    combined.extend_from_slice(&cipher_bytes);
                                    let combined_b64 =
                                        base64::engine::general_purpose::STANDARD.encode(combined);
                                    if let Ok(plain) =
                                        nomifun_common::decrypt_string(&combined_b64, &key_bytes)
                                    {
                                        if let Ok(val) =
                                            serde_json::from_str::<serde_json::Value>(&plain)
                                        {
                                            if let Some(tok) = val
                                                .get("server")
                                                .and_then(|s| s.get("access_token"))
                                                .and_then(|t| t.as_str())
                                            {
                                                if !tok.trim().is_empty() {
                                                    return Some(tok.trim().to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qwen_voice_router_default_model_is_qwen3_8_flash() {
        let router = QwenVoiceRouter::new();
        assert_eq!(router.model(), "qwen3.8-flash");
    }

    #[test]
    fn test_qwen_voice_router_custom_model_override() {
        let router = QwenVoiceRouter::new().with_model("custom-model");
        assert_eq!(router.model(), "custom-model");
    }

    #[test]
    fn test_flowy_cloud_router_construction() {
        let router = OpenAiCompatibleVoiceRouter::from_flowy_cloud(
            "https://server.flowyaipc.com/claw/v1",
            "fake-flowy-jwt-token",
            "qwen3.8-flash",
        );
        assert_eq!(
            router.endpoint(),
            Some("https://server.flowyaipc.com/claw/v1/chat/completions")
        );
        assert_eq!(router.model(), "qwen3.8-flash");
        assert!(router.is_cloud_connected());
        assert_eq!(
            router.extra_headers,
            vec![("token".to_string(), "fake-flowy-jwt-token".to_string())]
        );
    }
}
