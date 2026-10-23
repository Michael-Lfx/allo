//! Discover a local Herdsman engine and project it into the provider catalog.
//!
//! The engine publishes status on a Windows named pipe. This service is the
//! only writer of the reserved herdsman provider row. Each running local model
//! becomes its own chat entry, and the request uses that model's name on the
//! engine's local OpenAI endpoint. The Token Router is not used: it can hand
//! a named model to a cloud upstream. Chat surfaces already resolve
//! `provider_models` with task `chat`, so those entries show up everywhere a
//! conversation picks a model.

use std::sync::Arc;
use std::time::Duration;

use nomifun_common::{
    AppError, HERDSMAN_PLATFORM, HERDSMAN_PROVIDER_ID, decrypt_string, encrypt_string,
    is_herdsman_platform, is_herdsman_provider_id,
};
use nomifun_db::{
    CreateProviderParams, IProviderModelRepository, IProviderRepository, ProviderModelUpdate,
    UpdateProviderParams,
};
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

#[cfg(windows)]
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
#[cfg(windows)]
const MODEL_POLL_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(windows)]
const STATUS_READ_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(windows)]
const STATUS_BUFFER_LIMIT: usize = 64 * 1024;
const PROVIDER_NAME: &str = "Herdsman";
const CHAT_TASKS_JSON: &str = r#"["chat"]"#;
const EMPTY_TRAITS_JSON: &str = "[]";

/// Returned to HTTP callers that try to edit the engine-owned row.
pub(crate) const HERDSMAN_MUTATION_FORBIDDEN: &str =
    "The Herdsman provider is discovered automatically and cannot be edited";

pub(crate) fn forbid_herdsman_mutation(platform: &str, provider_id: &str) -> Result<(), AppError> {
    if is_herdsman_platform(platform) || is_herdsman_provider_id(provider_id) {
        Err(AppError::Forbidden(HERDSMAN_MUTATION_FORBIDDEN.into()))
    } else {
        Ok(())
    }
}

/// Owns the background discovery task. Dropping it stops the task.
pub struct HerdsmanConnection {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for HerdsmanConnection {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

/// Start discovery. On Windows this holds the status pipe and refreshes the
/// catalog. Other platforms only disable a leftover enabled row, because the
/// engine's status channel is a Windows named pipe.
pub fn start_herdsman_connection(
    provider_repo: Arc<dyn IProviderRepository>,
    provider_model_repo: Arc<dyn IProviderModelRepository>,
    encryption_key: [u8; 32],
    on_catalog_changed: Arc<dyn Fn() + Send + Sync>,
) -> HerdsmanConnection {
    let projector = Arc::new(Projector {
        provider_repo,
        provider_model_repo,
        encryption_key,
        on_catalog_changed,
        http: herdsman_http_client(),
        write_lock: Mutex::new(()),
    });
    let handle = tokio::spawn(async move {
        run_discovery(projector).await;
    });
    HerdsmanConnection { handle: Some(handle) }
}

fn herdsman_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

async fn run_discovery(projector: Arc<Projector>) {
    #[cfg(windows)]
    {
        windows::run(projector).await;
    }
    #[cfg(not(windows))]
    {
        // The status pipe does not exist here. A copied Windows database must
        // not keep offering a loopback endpoint this machine cannot reach.
        if let Err(error) = projector.project_offline().await {
            warn!(%error, "failed to disable herdsman provider on this platform");
        }
    }
}

struct Projector {
    provider_repo: Arc<dyn IProviderRepository>,
    provider_model_repo: Arc<dyn IProviderModelRepository>,
    encryption_key: [u8; 32],
    on_catalog_changed: Arc<dyn Fn() + Send + Sync>,
    http: reqwest::Client,
    write_lock: Mutex<()>,
}

struct OnlineProjection {
    base_url: String,
    api_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RunningModel {
    id: String,
    context_limit: Option<i64>,
}

impl Projector {
    async fn project_online(&self, projection: &OnlineProjection) -> Result<bool, AppError> {
        let _guard = self.write_lock.lock().await;
        let changed = sync_online(self.provider_repo.as_ref(), &self.encryption_key, projection)
        .await?;
        if changed {
            info!("herdsman provider is available for chat");
            (self.on_catalog_changed)();
        }
        Ok(changed)
    }

    async fn project_models(&self, models: &[RunningModel]) -> Result<bool, AppError> {
        let _guard = self.write_lock.lock().await;
        let changed = sync_running_models(
            self.provider_repo.as_ref(),
            self.provider_model_repo.as_ref(),
            models,
        )
        .await?;
        if changed {
            (self.on_catalog_changed)();
        }
        Ok(changed)
    }

    async fn project_offline(&self) -> Result<bool, AppError> {
        let _guard = self.write_lock.lock().await;
        let changed = sync_offline(self.provider_repo.as_ref()).await?;
        if changed {
            info!("herdsman provider disconnected");
            (self.on_catalog_changed)();
        }
        Ok(changed)
    }

    async fn refresh_models(&self, models_endpoint: &str) {
        let Some(url) = models_url(models_endpoint) else {
            debug!("skipped herdsman model poll because the endpoint is not loopback");
            return;
        };
        match fetch_running_models(&self.http, &url).await {
            Ok(models) => {
                if let Err(error) = self.project_models(&models).await {
                    warn!(%error, "failed to store herdsman models");
                }
            }
            Err(error) => {
                debug!(%error, "herdsman model list unavailable");
            }
        }
    }
}

async fn sync_online(
    provider_repo: &dyn IProviderRepository,
    encryption_key: &[u8; 32],
    projection: &OnlineProjection,
) -> Result<bool, AppError> {
    let existing = provider_repo.find_by_id(HERDSMAN_PROVIDER_ID).await?;
    let encrypted_key = encrypt_string(&projection.api_key, encryption_key)?;

    match existing {
        None => {
            provider_repo
                .create(CreateProviderParams {
                    provider_id: Some(HERDSMAN_PROVIDER_ID),
                    platform: HERDSMAN_PLATFORM,
                    name: PROVIDER_NAME,
                    base_url: &projection.base_url,
                    api_key_encrypted: &encrypted_key,
                    models: "[]",
                    enabled: true,
                    model_context_limits: None,
                    model_protocols: None,
                    model_descriptions: None,
                    model_enabled: None,
                    bedrock_config: None,
                    is_full_url: false,
                    sort_order: Some(0),
                })
                .await?;
            Ok(true)
        }
        Some(row) => {
            let stored_key_matches = decrypt_string(&row.api_key_encrypted, encryption_key)
                .ok()
                .as_deref()
                == Some(projection.api_key.as_str());
            let provider_matches = row.platform == HERDSMAN_PLATFORM
                && row.name == PROVIDER_NAME
                && row.base_url == projection.base_url
                && row.enabled
                && !row.is_full_url
                && stored_key_matches;
            if provider_matches {
                return Ok(false);
            }
            provider_repo
                .update(
                    HERDSMAN_PROVIDER_ID,
                    UpdateProviderParams {
                        platform: Some(HERDSMAN_PLATFORM),
                        name: Some(PROVIDER_NAME),
                        base_url: Some(&projection.base_url),
                        api_key_encrypted: Some(&encrypted_key),
                        enabled: Some(true),
                        is_full_url: Some(false),
                        ..Default::default()
                    },
                )
                .await?;
            Ok(true)
        }
    }
}

async fn sync_running_models(
    provider_repo: &dyn IProviderRepository,
    model_repo: &dyn IProviderModelRepository,
    models: &[RunningModel],
) -> Result<bool, AppError> {
    if provider_repo
        .find_by_id(HERDSMAN_PROVIDER_ID)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    let rows = model_repo.list_for_provider(HERDSMAN_PROVIDER_ID).await?;
    let current: Vec<&str> = rows.iter().map(|row| row.model.as_str()).collect();
    let desired: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
    let mut changed = false;
    if current != desired {
        let models_json = serde_json::to_string(&desired)
            .map_err(|error| AppError::Internal(format!("serialize herdsman models: {error}")))?;
        let mut contexts = serde_json::Map::new();
        for model in models {
            if let Some(limit) = model.context_limit {
                contexts.insert(model.id.clone(), serde_json::Value::from(limit));
            }
        }
        let context_json = serde_json::Value::Object(contexts).to_string();
        provider_repo
            .update(
                HERDSMAN_PROVIDER_ID,
                UpdateProviderParams {
                    models: Some(&models_json),
                    model_context_limits: Some(Some(&context_json)),
                    ..Default::default()
                },
            )
            .await?;
        changed = true;
    }
    for model in models {
        if ensure_chat_model(model_repo, &model.id, model.context_limit).await? {
            changed = true;
        }
    }
    Ok(changed)
}

async fn ensure_chat_model(
    model_repo: &dyn IProviderModelRepository,
    model_id: &str,
    context_limit: Option<i64>,
) -> Result<bool, AppError> {
    let Some(row) = model_repo.get(HERDSMAN_PROVIDER_ID, model_id).await? else {
        return Ok(false);
    };
    if chat_profile_ready(&row, model_id, context_limit) {
        return Ok(false);
    }
    model_repo
        .update(
            HERDSMAN_PROVIDER_ID,
            model_id,
            &ProviderModelUpdate {
                enabled: Some(true),
                tasks: Some(CHAT_TASKS_JSON),
                traits: Some(EMPTY_TRAITS_JSON),
                description: Some(Some(model_id)),
                source: Some("catalog"),
                context_limit: context_limit.map(Some),
                ..Default::default()
            },
        )
        .await?;
    Ok(true)
}

fn chat_profile_ready(
    row: &nomifun_db::ProviderModelRow,
    model_id: &str,
    context_limit: Option<i64>,
) -> bool {
    let context_ok = match context_limit {
        Some(limit) => row.context_limit == Some(limit),
        None => true,
    };
    row.enabled
        && row.tasks.trim() == CHAT_TASKS_JSON
        && row.traits.trim() == EMPTY_TRAITS_JSON
        && row.description.as_deref() == Some(model_id)
        && row.source == "catalog"
        && context_ok
}

async fn sync_offline(provider_repo: &dyn IProviderRepository) -> Result<bool, AppError> {
    let Some(row) = provider_repo.find_by_id(HERDSMAN_PROVIDER_ID).await? else {
        return Ok(false);
    };
    if !row.enabled {
        return Ok(false);
    }
    provider_repo
        .update(
            HERDSMAN_PROVIDER_ID,
            UpdateProviderParams {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await?;
    Ok(true)
}

#[derive(Debug, Deserialize)]
struct HerdsmanStatus {
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    openai_endpoint: Option<String>,
    #[serde(default)]
    token_router_api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HerdsmanModel {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    runtime_info: Option<HerdsmanRuntimeInfo>,
}

#[derive(Debug, Deserialize)]
struct HerdsmanRuntimeInfo {
    #[serde(default)]
    context_size: f64,
    #[serde(default)]
    run_status: String,
}

fn pop_status(buffer: &mut Vec<u8>) -> Result<Option<HerdsmanStatus>, ()> {
    let text = match std::str::from_utf8(buffer) {
        Ok(text) => text,
        // A multibyte character split across pipe reads is not a bad payload.
        Err(error) if error.error_len().is_none() => return Ok(None),
        Err(_) => return Err(()),
    };
    let start = text
        .find(|ch: char| !ch.is_whitespace())
        .unwrap_or(text.len());
    if start == text.len() {
        buffer.clear();
        return Ok(None);
    }
    let slice = &text[start..];
    let mut stream = serde_json::Deserializer::from_str(slice).into_iter::<HerdsmanStatus>();
    match stream.next() {
        Some(Ok(status)) => {
            let consumed = start + stream.byte_offset();
            buffer.drain(..consumed.min(buffer.len()));
            Ok(Some(status))
        }
        Some(Err(error)) if error.is_eof() => Ok(None),
        Some(Err(_)) => Err(()),
        None => Ok(None),
    }
}

fn sanitize_api_key(raw: &str) -> Option<String> {
    let key = raw.trim();
    if key.is_empty() || key.bytes().any(|byte| byte == b'\r' || byte == b'\n' || byte == 0) {
        None
    } else {
        Some(key.to_owned())
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "0.0.0.0" | "::1")
}

fn parse_loopback_http(raw: &str) -> Option<reqwest::Url> {
    let mut url = reqwest::Url::parse(raw.trim()).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    if !is_loopback_host(&host) {
        return None;
    }
    // Herdsman publishes the listen address `0.0.0.0`. That host is not a
    // dial target on Windows, so model polling and chat both use loopback.
    if host == "0.0.0.0" {
        url.set_host(Some("127.0.0.1")).ok()?;
    }
    Some(url)
}

/// Normalize the local OpenAI base. Empty and non-loopback URLs are rejected
/// so a status payload cannot redirect a named local model off the machine.
fn normalize_loopback_endpoint(raw: Option<&str>) -> Option<String> {
    let input = raw.map(str::trim).filter(|value| !value.is_empty())?;
    let mut url = parse_loopback_http(input)?;
    let path = url.path();
    if path.is_empty() || path == "/" {
        url.set_path("/v1");
    }
    let mut rendered = url.to_string();
    if rendered.ends_with('/') {
        rendered.pop();
    }
    Some(rendered)
}

fn models_url(endpoint: &str) -> Option<String> {
    let mut url = parse_loopback_http(endpoint)?;
    let path = url.path().trim_end_matches('/');
    let joined = if path.is_empty() {
        "/api/v1/models".to_owned()
    } else {
        format!("{path}/api/v1/models")
    };
    url.set_path(&joined);
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

fn wire_model_id(model: &HerdsmanModel) -> Option<String> {
    let name = model.name.trim();
    let id = model.id.trim();
    let raw = if !name.is_empty() { name } else { id };
    if raw.is_empty() || raw.bytes().any(|byte| byte == b'\r' || byte == b'\n' || byte == 0) {
        None
    } else {
        Some(raw.to_owned())
    }
}

fn is_running_chat_model(model: &HerdsmanModel) -> bool {
    (model.kind == "text-generation" || model.kind == "multimodal")
        && model.status == "installed"
        && model
            .runtime_info
            .as_ref()
            .is_some_and(|info| info.run_status == "running")
}

/// Selectable models are the engine's running text and multimodal models.
/// The wire id is the model name, which is what the local OpenAI endpoint
/// accepts. A failed poll must not call this with an empty stand-in; an empty
/// success means nothing is currently running.
fn running_models(models: &[HerdsmanModel]) -> Vec<RunningModel> {
    let mut by_id = std::collections::BTreeMap::<String, RunningModel>::new();
    for model in models.iter().filter(|model| is_running_chat_model(model)) {
        let Some(id) = wire_model_id(model) else {
            continue;
        };
        let context_limit = model.runtime_info.as_ref().and_then(|info| {
            let limit = info.context_size.floor() as i64;
            (limit > 0).then_some(limit)
        });
        match by_id.get(&id) {
            Some(existing) if existing.context_limit >= context_limit => {}
            _ => {
                by_id.insert(
                    id.clone(),
                    RunningModel {
                        id,
                        context_limit,
                    },
                );
            }
        }
    }
    by_id.into_values().collect()
}

fn online_projection_from_status(status: &HerdsmanStatus) -> Option<OnlineProjection> {
    // Named local models stay on the engine's OpenAI endpoint. The Token
    // Router is a hybrid front door and can forward the same model name upstream.
    let base_url = normalize_loopback_endpoint(status.openai_endpoint.as_deref())?;
    let api_key = sanitize_api_key(status.token_router_api_key.as_deref().unwrap_or(""))?;
    Some(OnlineProjection { base_url, api_key })
}

async fn fetch_running_models(http: &reqwest::Client, url: &str) -> Result<Vec<RunningModel>, String> {
    let response = http
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("model list returned {}", response.status()));
    }
    let models: Vec<HerdsmanModel> = response.json().await.map_err(|error| error.to_string())?;
    Ok(running_models(&models))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
    use tokio::sync::watch;

    const DEFAULT_PIPE: &str = r"\\.\pipe\Herdsman-status";

    fn pipe_name() -> String {
        std::env::var("HERDSMAN_STATUS_PIPE")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_PIPE.to_owned())
    }

    pub(super) async fn run(projector: Arc<Projector>) {
        loop {
            match hold_session(projector.clone()).await {
                Ok(()) => {}
                Err(error) => debug!(%error, "herdsman status pipe unavailable"),
            }
            if let Err(error) = projector.project_offline().await {
                warn!(%error, "failed to disable herdsman provider after disconnect");
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }

    async fn hold_session(projector: Arc<Projector>) -> Result<(), String> {
        let mut client = ClientOptions::new()
            .open(pipe_name())
            .map_err(|error| error.to_string())?;
        client
            .write_all(b"/status")
            .await
            .map_err(|error| error.to_string())?;

        let mut buffer = Vec::new();
        let status = read_one_status(&mut client, &mut buffer).await?;
        let (endpoint_tx, endpoint_rx) = watch::channel(status.endpoint.clone());
        apply_status(&projector, &status).await;

        let poll_projector = projector.clone();
        let mut poll_endpoint = endpoint_rx.clone();
        let poll = tokio::spawn(async move {
            loop {
                let endpoint = poll_endpoint.borrow().clone();
                if !endpoint.trim().is_empty() {
                    poll_projector.refresh_models(&endpoint).await;
                }
                tokio::select! {
                    _ = tokio::time::sleep(MODEL_POLL_INTERVAL) => {}
                    changed = poll_endpoint.changed() => {
                        if changed.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let ended = follow_status(&mut client, &mut buffer, &projector, &endpoint_tx).await;
        poll.abort();
        ended
    }

    async fn follow_status(
        client: &mut NamedPipeClient,
        buffer: &mut Vec<u8>,
        projector: &Projector,
        endpoint_tx: &watch::Sender<String>,
    ) -> Result<(), String> {
        let mut scratch = [0u8; 4096];
        loop {
            let read = client
                .read(&mut scratch)
                .await
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Ok(());
            }
            if buffer.len() + read > STATUS_BUFFER_LIMIT {
                buffer.clear();
                return Err("herdsman status payload exceeded the buffer limit".into());
            }
            buffer.extend_from_slice(&scratch[..read]);
            while let Some(status) = next_status(buffer)? {
                let _ = endpoint_tx.send(status.endpoint.clone());
                apply_status(projector, &status).await;
            }
        }
    }

    async fn read_one_status(
        client: &mut NamedPipeClient,
        buffer: &mut Vec<u8>,
    ) -> Result<HerdsmanStatus, String> {
        let mut scratch = [0u8; 4096];
        let deadline = tokio::time::Instant::now() + STATUS_READ_TIMEOUT;
        loop {
            if let Some(status) = next_status(buffer)? {
                return Ok(status);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for herdsman status".into());
            }
            let read = tokio::time::timeout(remaining, client.read(&mut scratch))
                .await
                .map_err(|_| "timed out waiting for herdsman status".to_owned())?
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("herdsman status pipe closed before a payload".into());
            }
            if buffer.len() + read > STATUS_BUFFER_LIMIT {
                return Err("herdsman status payload exceeded the buffer limit".into());
            }
            buffer.extend_from_slice(&scratch[..read]);
        }
    }

    fn next_status(buffer: &mut Vec<u8>) -> Result<Option<HerdsmanStatus>, String> {
        match pop_status(buffer) {
            Ok(status) => Ok(status),
            Err(()) => {
                buffer.clear();
                Err("herdsman status payload was not valid JSON".into())
            }
        }
    }

    async fn apply_status(projector: &Projector, status: &HerdsmanStatus) {
        let Some(projection) = online_projection_from_status(status) else {
            debug!("ignored herdsman status without a loopback local endpoint");
            if let Err(error) = projector.project_offline().await {
                warn!(%error, "failed to disable herdsman provider after a rejected status");
            }
            return;
        };
        if let Err(error) = projector.project_online(&projection).await {
            warn!(%error, "failed to project herdsman provider");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_api_types::{ModelTask, resolve_models};
    use nomifun_db::{SqliteProviderModelRepository, SqliteProviderRepository, init_database_memory};

    use crate::model_profile::ModelProfileService;
    use crate::provider::ProviderService;

    const TEST_KEY: [u8; 32] = [0x42; 32];

    fn sample_status(json: &str) -> HerdsmanStatus {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn local_endpoint_requires_loopback_and_does_not_invent_a_router() {
        assert!(normalize_loopback_endpoint(None).is_none());
        assert!(normalize_loopback_endpoint(Some("  ")).is_none());
        assert_eq!(
            normalize_loopback_endpoint(Some("http://127.0.0.1:18080")).as_deref(),
            Some("http://127.0.0.1:18080/v1")
        );
        assert_eq!(
            normalize_loopback_endpoint(Some("http://127.0.0.1:18080/v1/")).as_deref(),
            Some("http://127.0.0.1:18080/v1")
        );
        assert!(normalize_loopback_endpoint(Some("http://192.168.1.8:18080/v1")).is_none());
        assert!(normalize_loopback_endpoint(Some("file:///etc/passwd")).is_none());
        assert_eq!(
            normalize_loopback_endpoint(Some("http://0.0.0.0:8080/v1")).as_deref(),
            Some("http://127.0.0.1:8080/v1")
        );
    }

    #[test]
    fn model_poll_dials_loopback_when_herdsman_publishes_unspecified_host() {
        assert_eq!(
            models_url("http://0.0.0.0:8080").as_deref(),
            Some("http://127.0.0.1:8080/api/v1/models")
        );
        assert_eq!(
            models_url("http://127.0.0.1:8080").as_deref(),
            Some("http://127.0.0.1:8080/api/v1/models")
        );
    }

    #[test]
    fn named_local_model_uses_the_openai_endpoint() {
        let status = sample_status(
            r#"{"endpoint":"http://127.0.0.1:9","openai_endpoint":"http://127.0.0.1:18080","token_router_endpoint":"http://127.0.0.1:16621/v1","token_router_api_key":"secret"}"#,
        );
        let projection = online_projection_from_status(&status).unwrap();
        assert_eq!(projection.base_url, "http://127.0.0.1:18080/v1");
        assert_eq!(projection.api_key, "secret");
    }

    #[test]
    fn running_models_keep_each_chat_model_name_and_context() {
        let models: Vec<HerdsmanModel> = serde_json::from_str(
            r#"[
                {"id":"qwen-id","name":"Qwen2.5-7B","type":"text-generation","status":"installed","runtime_info":{"context_size":8192,"run_status":"running"}},
                {"name":"Qwen2.5-7B","type":"text-generation","status":"installed","runtime_info":{"context_size":4096,"run_status":"running"}},
                {"id":"llava-id","name":"LLaVA","type":"multimodal","status":"installed","runtime_info":{"context_size":32768,"run_status":"running"}},
                {"name":"Tiny","type":"text-generation","status":"installed","runtime_info":{"run_status":"running"}},
                {"name":"Old","type":"text-generation","status":"installed","runtime_info":{"context_size":99999,"run_status":"stopped"}},
                {"name":"embed","type":"embedding","status":"installed","runtime_info":{"context_size":4096,"run_status":"running"}}
            ]"#,
        )
        .unwrap();
        let running = running_models(&models);
        assert_eq!(
            running
                .iter()
                .map(|model| (model.id.as_str(), model.context_limit))
                .collect::<Vec<_>>(),
            vec![
                ("LLaVA", Some(32768)),
                ("Qwen2.5-7B", Some(8192)),
                ("Tiny", None),
            ]
        );
    }

    #[test]
    fn status_buffer_yields_one_object_and_waits_for_the_rest() {
        let mut buffer = br#"{"endpoint":"http://127.0.0.1:9","token_router_api_key":"secret"}"#.to_vec();
        let status = pop_status(&mut buffer).unwrap().unwrap();
        assert_eq!(status.token_router_api_key.as_deref(), Some("secret"));
        assert!(buffer.is_empty());

        let mut partial = br#"{"endpoint":"http://127."#.to_vec();
        assert!(pop_status(&mut partial).unwrap().is_none());
        assert!(!partial.is_empty());
    }

    #[test]
    fn status_without_a_local_endpoint_or_key_is_not_usable() {
        let router_only = sample_status(
            r#"{"endpoint":"http://127.0.0.1:9","token_router_endpoint":"http://127.0.0.1:16621/v1","token_router_api_key":"secret"}"#,
        );
        assert!(online_projection_from_status(&router_only).is_none());
        let missing_key = sample_status(
            r#"{"endpoint":"http://127.0.0.1:9","openai_endpoint":"http://127.0.0.1:18080/v1"}"#,
        );
        assert!(online_projection_from_status(&missing_key).is_none());
        let remote = sample_status(
            r#"{"openai_endpoint":"http://10.0.0.8:18080/v1","token_router_api_key":"secret"}"#,
        );
        assert!(online_projection_from_status(&remote).is_none());
    }

    #[tokio::test]
    async fn projection_is_idempotent_and_hides_the_engine_key() {
        let db = init_database_memory().await.unwrap();
        let provider_repo = Arc::new(SqliteProviderRepository::new(db.pool().clone()));
        let model_repo = Arc::new(SqliteProviderModelRepository::new(db.pool().clone()));
        let projection = OnlineProjection {
            base_url: "http://127.0.0.1:16621/v1".into(),
            api_key: "engine-key".into(),
        };
        assert!(
            sync_online(provider_repo.as_ref(), &TEST_KEY, &projection)
                .await
                .unwrap()
        );
        let stored = provider_repo
            .find_by_id(HERDSMAN_PROVIDER_ID)
            .await
            .unwrap()
            .unwrap();
        let ciphertext = stored.api_key_encrypted.clone();
        assert!(
            !sync_online(provider_repo.as_ref(), &TEST_KEY, &projection)
                .await
                .unwrap()
        );
        let again = provider_repo
            .find_by_id(HERDSMAN_PROVIDER_ID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again.api_key_encrypted, ciphertext);

        let running = vec![
            RunningModel {
                id: "LLaVA".into(),
                context_limit: Some(32768),
            },
            RunningModel {
                id: "Qwen2.5-7B".into(),
                context_limit: Some(8192),
            },
        ];
        assert!(
            sync_running_models(provider_repo.as_ref(), model_repo.as_ref(), &running)
                .await
                .unwrap()
        );
        assert!(
            !sync_running_models(provider_repo.as_ref(), model_repo.as_ref(), &running)
                .await
                .unwrap()
        );
        let unchanged = provider_repo
            .find_by_id(HERDSMAN_PROVIDER_ID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.api_key_encrypted, ciphertext);

        let service = ProviderService::new(provider_repo.clone(), model_repo.clone(), TEST_KEY);
        let listed = service.list().await.unwrap();
        let herdsman = listed
            .iter()
            .find(|provider| provider.provider_id == HERDSMAN_PROVIDER_ID)
            .unwrap();
        assert!(herdsman.api_key.is_empty());
        assert_eq!(
            herdsman.models,
            vec!["LLaVA".to_owned(), "Qwen2.5-7B".to_owned()]
        );
        assert_eq!(
            herdsman
                .model_descriptions
                .as_ref()
                .and_then(|map| map.get("Qwen2.5-7B"))
                .map(String::as_str),
            Some("Qwen2.5-7B")
        );
        assert_eq!(
            herdsman
                .model_context_limits
                .as_ref()
                .and_then(|map| map.get("Qwen2.5-7B"))
                .copied(),
            Some(8192)
        );
        assert!(herdsman.enabled);
        assert!(!herdsman.models.iter().any(|model| model == "auto"));

        let profiles = ModelProfileService::new(model_repo.clone())
            .list()
            .await
            .unwrap();
        let resolved = resolve_models(&listed, &profiles, ModelTask::Chat, &[]);
        assert!(resolved.iter().any(|model| {
            model.provider_id == HERDSMAN_PROVIDER_ID && model.model == "Qwen2.5-7B"
        }));
        assert!(resolved.iter().any(|model| {
            model.provider_id == HERDSMAN_PROVIDER_ID && model.model == "LLaVA"
        }));

        let qwen_only = vec![RunningModel {
            id: "Qwen2.5-7B".into(),
            context_limit: Some(16384),
        }];
        assert!(
            sync_running_models(provider_repo.as_ref(), model_repo.as_ref(), &qwen_only)
                .await
                .unwrap()
        );
        assert!(
            !sync_running_models(provider_repo.as_ref(), model_repo.as_ref(), &qwen_only)
                .await
                .unwrap()
        );
        let narrowed = service.list().await.unwrap();
        let herdsman = narrowed
            .iter()
            .find(|provider| provider.provider_id == HERDSMAN_PROVIDER_ID)
            .unwrap();
        assert_eq!(herdsman.models, vec!["Qwen2.5-7B".to_owned()]);
        assert_eq!(
            herdsman
                .model_context_limits
                .as_ref()
                .and_then(|map| map.get("Qwen2.5-7B"))
                .copied(),
            Some(16384)
        );

        assert!(
            sync_running_models(provider_repo.as_ref(), model_repo.as_ref(), &[])
                .await
                .unwrap()
        );
        let cleared = service.list().await.unwrap();
        let herdsman = cleared
            .iter()
            .find(|provider| provider.provider_id == HERDSMAN_PROVIDER_ID)
            .unwrap();
        assert!(herdsman.models.is_empty());

        assert!(sync_offline(provider_repo.as_ref()).await.unwrap());
        assert!(!sync_offline(provider_repo.as_ref()).await.unwrap());
        let after = service.list().await.unwrap();
        let disabled = after
            .iter()
            .find(|provider| provider.provider_id == HERDSMAN_PROVIDER_ID)
            .unwrap();
        assert!(!disabled.enabled);
        let hidden = resolve_models(&after, &profiles, ModelTask::Chat, &[]);
        assert!(!hidden
            .iter()
            .any(|model| model.provider_id == HERDSMAN_PROVIDER_ID));
    }
}
