//! Task delivery transport. The host alone owns execution configuration and permissions.
use async_trait::async_trait;
use nomifun_api_types::voice::{VoiceTaskDelivery, VoiceTaskSubmitRequest,VoiceTaskControlCommand,VoiceTaskSnapshot};

#[async_trait]
pub trait VoiceTaskClient: Send + Sync {
    async fn submit(&self, request: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String>;
    async fn current(&self) -> Result<Option<VoiceTaskSnapshot>,String>;
    async fn control(&self, request:VoiceTaskControlCommand) -> Result<VoiceTaskSnapshot,String>;
}

pub struct HostTaskClient {
    endpoint: String,
    credential: String,
    client: reqwest::Client,
}

impl HostTaskClient {
    pub fn new(endpoint: String, credential: String) -> Result<Self, String> {
        let url = reqwest::Url::parse(&endpoint).map_err(|_| "invalid_host_task_endpoint")?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
            || credential.is_empty()
        { return Err("invalid_host_task_endpoint".into()); }
        let client = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15)).build()
            .map_err(|_| "host_task_client_failed")?;
        Ok(Self {endpoint, credential, client})
    }
}

#[async_trait]
impl VoiceTaskClient for HostTaskClient {
    async fn current(&self) -> Result<Option<VoiceTaskSnapshot>,String> {
        let response=self.client.get(&self.endpoint).header("x-ai-mate-control-token",&self.credential)
            .timeout(std::time::Duration::from_millis(200)).send().await.map_err(|_|"task_status_unavailable")?;
        if !response.status().is_success() {return Err("task_status_unavailable".into());}
        response.json().await.map_err(|_|"task_status_unavailable".into())
    }

    async fn control(&self, request:VoiceTaskControlCommand) -> Result<VoiceTaskSnapshot,String> {
        let endpoint=reqwest::Url::parse(&self.endpoint).and_then(|url|url.join("task-control")).map_err(|_|"invalid_host_task_endpoint")?;
        let response=self.client.post(endpoint).header("x-ai-mate-control-token",&self.credential)
            .json(&request).send().await.map_err(|_|"task_control_unknown")?;
        if !response.status().is_success() {
            return Err(response.json::<serde_json::Value>().await.ok().and_then(|value|value["code"].as_str().map(str::to_owned)).unwrap_or_else(||"task_control_unknown".into()));
        }
        let task:VoiceTaskSnapshot=response.json().await.map_err(|_|"task_control_unknown")?;
        if !request.task.matches(&task) {return Err("task_receipt_scope_changed".into());}
        Ok(task)
    }

    async fn submit(&self, request: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String> {
        // A lost response is indeterminate. Never manufacture a new command or retry a send.
        let response = self.client.post(&self.endpoint)
            .header("x-ai-mate-control-token", &self.credential).json(&request)
            .send().await.map_err(|_| "task_delivery_unknown")?;
        if !response.status().is_success() {
            return Err(response.json::<serde_json::Value>().await.ok()
                .and_then(|value| value["code"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "task_delivery_unknown".into()));
        }
        let delivery: VoiceTaskDelivery = response.json().await.map_err(|_| "task_delivery_unknown")?;
        if delivery.task.command_id != request.command.command_id
            || delivery.task.session_id != request.command.session_id
            || delivery.task.acoustic_turn_id != request.command.turn_id
        { return Err("task_receipt_scope_changed".into()); }
        Ok(delivery)
    }
}
