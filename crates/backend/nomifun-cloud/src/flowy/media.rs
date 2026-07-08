//! Flowy image generation (sync) and Seedance video task (async) APIs.

use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;

use serde_json::{Map, Value, json};
use tracing::debug;

use crate::error::ServerClientError;
use crate::flowy::media_types::{
    CreateVideoTaskResponse, ImageGenerationRequest, VIDEO_TASK_STATUS_CANCELLED,
    VIDEO_TASK_STATUS_EXPIRED, VIDEO_TASK_STATUS_FAILED, VideoContentImage, VideoCreateParams,
    VideoTaskRecord,
};
use crate::flowy::response::FlowyEnvelope;
use crate::session::ServerSession;
use crate::transport::HttpTransport;

use super::FlowyApiClient;

const DEFAULT_VIDEO_POLL_INTERVAL_SECS: u64 = 5;
const DEFAULT_VIDEO_POLL_TIMEOUT_SECS: u64 = 600;

pub type VideoTaskProgressFn = Box<dyn FnMut(&VideoTaskRecord, u64) + Send>;

impl FlowyApiClient {
    /// `POST {LLM根}/images/generations` — upstream JSON passthrough on success.
    pub async fn images_generations(
        &self,
        session: &ServerSession,
        body: Value,
    ) -> Result<Value, ServerClientError> {
        self.post_upstream_json(&self.llm_transport, "/images/generations", session, body)
            .await
    }

    /// `POST {LLM根}/images/edits` — image-to-image / edit proxy.
    pub async fn images_edits(
        &self,
        session: &ServerSession,
        body: Value,
    ) -> Result<Value, ServerClientError> {
        self.post_upstream_json(&self.llm_transport, "/images/edits", session, body)
            .await
    }

    /// Convenience wrapper for minimal text-to-image requests.
    pub async fn generate_image(
        &self,
        session: &ServerSession,
        req: &ImageGenerationRequest,
    ) -> Result<Value, ServerClientError> {
        let mut body = Map::new();
        body.insert("model".into(), json!(req.model));
        body.insert("prompt".into(), json!(req.prompt));
        if let Some(url) = &req.image_url {
            body.insert("image_url".into(), json!(url));
        }
        if let Value::Object(extra) = &req.extra {
            for (k, v) in extra {
                body.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        let path = if req.image_url.is_some() {
            "/images/edits"
        } else {
            "/images/generations"
        };
        self.post_upstream_json(&self.llm_transport, path, session, Value::Object(body))
            .await
    }

    /// `POST {业务根}/video/generations/tasks`
    pub async fn create_video_task(
        &self,
        session: &ServerSession,
        body: Value,
    ) -> Result<CreateVideoTaskResponse, ServerClientError> {
        self.post_data("/video/generations/tasks", Some(session), &body)
            .await
    }

    /// `GET {业务根}/video/generations/tasks/:id`
    pub async fn get_video_task(
        &self,
        session: &ServerSession,
        local_id: i64,
    ) -> Result<VideoTaskRecord, ServerClientError> {
        let path = format!("/video/generations/tasks/{local_id}");
        self.get_data(&path, Some(session)).await
    }

    /// `DELETE {业务根}/video/generations/tasks/:id` — cancel an in-flight video task.
    pub async fn cancel_video_task(
        &self,
        session: &ServerSession,
        local_id: i64,
    ) -> Result<(), ServerClientError> {
        let path = format!("/video/generations/tasks/{local_id}");
        let resp = self
            .transport
            .request(Method::DELETE, &path, Some(session), None)
            .await?;
        let status = resp.status().as_u16();
        if status == 200 {
            return Ok(());
        }
        let text = resp
            .text()
            .await
            .map_err(|e| ServerClientError::Http(e.to_string()))?;
        if let Ok(env) = FlowyEnvelope::parse_body(&text) {
            return Err(ServerClientError::Api {
                code: env.code,
                msg: env.msg,
            });
        }
        Err(ServerClientError::Http(format!("HTTP {status}: {text}")))
    }

    /// Poll until the video task reaches a terminal status.
    pub async fn poll_video_task(
        &self,
        session: &ServerSession,
        local_id: i64,
        poll_interval_secs: u64,
        timeout_secs: u64,
    ) -> Result<VideoTaskRecord, ServerClientError> {
        self.poll_video_task_with_progress(
            session,
            local_id,
            poll_interval_secs,
            timeout_secs,
            None,
            None,
        )
        .await
    }

    /// Poll with optional user-visible progress (e.g. gateway status).
    pub async fn poll_video_task_with_progress(
        &self,
        session: &ServerSession,
        local_id: i64,
        poll_interval_secs: u64,
        timeout_secs: u64,
        mut on_progress: Option<VideoTaskProgressFn>,
        should_cancel: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    ) -> Result<VideoTaskRecord, ServerClientError> {
        let interval = Duration::from_secs(poll_interval_secs.max(1));
        let timeout = Duration::from_secs(timeout_secs.max(30));
        let started = std::time::Instant::now();
        let mut last_status: Option<i32> = None;
        let mut last_report = std::time::Instant::now() - Duration::from_secs(60);

        loop {
            if should_cancel.as_ref().is_some_and(|f| f()) {
                let _ = self.cancel_video_task(session, local_id).await;
                return Err(ServerClientError::InvalidResponse(format!(
                    "video task {local_id} cancelled by client"
                )));
            }
            let record = self.get_video_task(session, local_id).await?;
            let elapsed = started.elapsed().as_secs();
            let status_changed = last_status != Some(record.status);
            let report_due = last_report.elapsed() >= Duration::from_secs(30);
            if let Some(ref mut cb) = on_progress
                && (status_changed || report_due)
            {
                cb(&record, elapsed);
                last_status = Some(record.status);
                last_report = std::time::Instant::now();
            }
            if record.is_terminal() {
                return Ok(record);
            }
            if started.elapsed() >= timeout {
                return Err(ServerClientError::InvalidResponse(format!(
                    "video task {local_id} timed out after {}s (status={})",
                    timeout.as_secs(),
                    record.status
                )));
            }
            debug!(local_id, status = record.status, "polling video task");
            tokio::time::sleep(interval).await;
        }
    }

    /// Create a Seedance video task and poll until completion.
    pub async fn generate_video(
        &self,
        session: &ServerSession,
        body: Value,
    ) -> Result<VideoTaskRecord, ServerClientError> {
        self.generate_video_with_timeout(session, body, DEFAULT_VIDEO_POLL_TIMEOUT_SECS)
            .await
    }

    /// Create a Seedance video task and poll until completion with a custom timeout.
    pub async fn generate_video_with_timeout(
        &self,
        session: &ServerSession,
        body: Value,
        timeout_secs: u64,
    ) -> Result<VideoTaskRecord, ServerClientError> {
        self.generate_video_with_timeout_and_progress(session, body, timeout_secs, None)
            .await
    }

    /// Create a Seedance video task and poll with optional progress callbacks.
    pub async fn generate_video_with_timeout_and_progress(
        &self,
        session: &ServerSession,
        body: Value,
        timeout_secs: u64,
        on_progress: Option<VideoTaskProgressFn>,
    ) -> Result<VideoTaskRecord, ServerClientError> {
        self.generate_video_with_timeout_and_progress_cancellable(
            session,
            body,
            timeout_secs,
            on_progress,
            None,
            None,
        )
        .await
    }

    /// Create a Seedance video task and poll with optional progress + cancellation hooks.
    pub async fn generate_video_with_timeout_and_progress_cancellable(
        &self,
        session: &ServerSession,
        body: Value,
        timeout_secs: u64,
        on_progress: Option<VideoTaskProgressFn>,
        should_cancel: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
        on_task_created: Option<Box<dyn FnMut(i64) + Send>>,
    ) -> Result<VideoTaskRecord, ServerClientError> {
        let created: CreateVideoTaskResponse = self.create_video_task(session, body).await?;
        if let Some(mut cb) = on_task_created {
            cb(created.id);
        }
        self.poll_video_task_with_progress(
            session,
            created.id,
            DEFAULT_VIDEO_POLL_INTERVAL_SECS,
            timeout_secs.max(30),
            on_progress,
            should_cancel,
        )
        .await
    }

    /// Build an Ark-compatible video create body from high-level parameters.
    pub fn build_video_create_body(
        model: &str,
        prompt: &str,
        image_url: Option<&str>,
        duration: Option<u32>,
        aspect_ratio: &str,
        resolution: Option<&str>,
        negative_prompt: Option<&str>,
        seed: Option<i64>,
        watermark: bool,
    ) -> Value {
        Self::build_video_create_params(VideoCreateParams {
            model: model.to_string(),
            prompt: prompt.to_string(),
            duration,
            aspect_ratio: aspect_ratio.to_string(),
            resolution: resolution.map(str::to_string),
            negative_prompt: negative_prompt.map(str::to_string),
            seed,
            watermark,
            generate_audio: None,
            images: image_url
                .filter(|u| !u.trim().is_empty())
                .map(|url| VideoContentImage {
                    url: url.to_string(),
                    role: "first_frame".into(),
                })
                .into_iter()
                .collect(),
            reference_video_url: None,
            reference_audio_url: None,
        })
    }

    /// Build video task JSON from a full [`VideoCreateParams`] (multimodal).
    pub fn build_video_create_params(params: VideoCreateParams) -> Value {
        params.to_json()
    }

    pub(crate) async fn post_upstream_json(
        &self,
        transport: &HttpTransport,
        path: &str,
        session: &ServerSession,
        body: Value,
    ) -> Result<Value, ServerClientError> {
        let resp = transport.post_json(path, Some(session), body).await?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| ServerClientError::Http(e.to_string()))?;

        if status == 200 {
            return serde_json::from_str(&text).map_err(|e| {
                ServerClientError::InvalidResponse(format!("upstream image JSON: {e}"))
            });
        }

        if let Ok(env) = FlowyEnvelope::parse_body(&text) {
            return Err(ServerClientError::Api {
                code: env.code,
                msg: env.msg,
            });
        }

        Err(ServerClientError::Http(format!("HTTP {status}: {text}")))
    }
}

/// Map local video task status to a human-readable label.
pub fn video_task_status_label(status: i32) -> &'static str {
    match status {
        1 => "queued",
        2 => "running",
        3 => "cancelled",
        4 => "succeeded",
        5 => "failed",
        6 => "expired",
        _ => "unknown",
    }
}

/// User-facing Chinese status for gateway progress messages.
pub fn video_task_status_user_message_zh(status: i32, elapsed_secs: u64) -> String {
    match status {
        1 => format!("视频任务已提交，正在排队（已等待 {elapsed_secs} 秒）"),
        2 => format!("云端正在渲染视频（已等待 {elapsed_secs} 秒，通常还需 1–5 分钟）"),
        4 => "视频生成完成，正在获取下载链接…".into(),
        5 => "视频生成失败，正在整理错误信息…".into(),
        6 => "视频任务已超时，正在结束…".into(),
        3 => "视频任务已取消".into(),
        _ => format!(
            "正在处理视频任务（状态 {}，已等待 {elapsed_secs} 秒）",
            video_task_status_label(status)
        ),
    }
}

/// Error message for terminal non-success video statuses.
pub fn video_task_failure_message(record: &VideoTaskRecord) -> String {
    let detail = record
        .failure_detail()
        .map(|d| format!(": {d}"))
        .unwrap_or_default();
    match record.status {
        VIDEO_TASK_STATUS_FAILED => format!("video generation failed{detail}"),
        VIDEO_TASK_STATUS_EXPIRED => format!("video task expired{detail}"),
        VIDEO_TASK_STATUS_CANCELLED => format!("video task cancelled{detail}"),
        _ => format!(
            "video task ended with status {} ({}){detail}",
            record.status,
            video_task_status_label(record.status)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flowy::media_types::VideoTaskRecord;
    use nomi_config::ServerConfig;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: &str) -> ServerConfig {
        ServerConfig {
            base_url: base_url.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn video_task_status_user_message_zh_covers_queue_and_running() {
        assert!(video_task_status_user_message_zh(1, 5).contains("排队"));
        assert!(video_task_status_user_message_zh(2, 30).contains("渲染"));
    }

    #[test]
    fn build_video_create_body_multimodal() {
        let body = FlowyApiClient::build_video_create_params(VideoCreateParams {
            model: "AIPC-doubao-seedance".into(),
            prompt: "test".into(),
            duration: Some(5),
            aspect_ratio: "16:9".into(),
            resolution: Some("720p".into()),
            negative_prompt: None,
            seed: None,
            watermark: false,
            generate_audio: Some(true),
            images: vec![
                VideoContentImage {
                    url: "https://example.com/a.png".into(),
                    role: "first_frame".into(),
                },
                VideoContentImage {
                    url: "https://example.com/b.png".into(),
                    role: "last_frame".into(),
                },
            ],
            reference_video_url: Some("https://example.com/ref.mp4".into()),
            reference_audio_url: None,
        });
        let content = body["content"].as_array().expect("content");
        assert!(content.len() >= 4);
        assert_eq!(body["generate_audio"], true);
    }

    #[test]
    fn build_video_create_body_text_only() {
        let body = FlowyApiClient::build_video_create_body(
            "flowy/doubao-seedance-1-0-pro",
            "a cat in the sun",
            None,
            Some(5),
            "16:9",
            Some("720p"),
            None,
            None,
            false,
        );
        assert_eq!(body["model"], "flowy/doubao-seedance-1-0-pro");
        assert_eq!(body["duration"], 5);
        assert_eq!(body["ratio"], "16:9");
        assert!(body["content"].is_array());
    }

    #[test]
    fn build_video_create_body_with_first_frame() {
        let body = FlowyApiClient::build_video_create_body(
            "flowy/doubao-seedance-1-0-pro",
            "animate this",
            Some("https://example.com/frame.png"),
            Some(8),
            "9:16",
            None,
            None,
            None,
            false,
        );
        let content = body["content"].as_array().expect("content array");
        assert_eq!(content.len(), 2);
        assert_eq!(content[1]["role"], "first_frame");
    }

    #[tokio::test]
    async fn create_and_poll_video_task_success() {
        let business = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/video/generations/tasks"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"code":200,"msg":"ok","data":{"id":42}}"#),
            )
            .mount(&business)
            .await;

        let poll_count = std::sync::atomic::AtomicU32::new(0);
        Mock::given(method("GET"))
            .and(path("/video/generations/tasks/42"))
            .respond_with(move |_: &wiremock::Request| {
                let n = poll_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let status = if n >= 2 { 4 } else { 2 };
                ResponseTemplate::new(200).set_body_string(format!(
                    r#"{{"code":200,"msg":"ok","data":{{"id":42,"status":{status},"result":{{"content":{{"video_url":"https://cdn.example/v.mp4"}}}}}}}}"#
                ))
            })
            .mount(&business)
            .await;

        let config = test_config(&business.uri());
        let api = FlowyApiClient::new(&config).expect("client");
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "jwt-test") };
        let session = ServerSession::from_config(&config, tmp.path());
        let body = FlowyApiClient::build_video_create_body(
            "flowy/doubao-seedance-1-0-pro",
            "test",
            None,
            Some(5),
            "16:9",
            None,
            None,
            None,
            false,
        );
        let record = api.generate_video(&session, body).await.expect("video");
        assert!(record.is_success());
        assert_eq!(
            record.video_url().as_deref(),
            Some("https://cdn.example/v.mp4")
        );
    }

    #[test]
    fn video_task_record_terminal_detection() {
        let mut rec = VideoTaskRecord {
            id: 1,
            task_id: None,
            status: 2,
            result: None,
            created_at: None,
            updated_at: None,
        };
        assert!(!rec.is_terminal());
        rec.status = 4;
        assert!(rec.is_terminal());
        assert!(rec.is_success());
    }
}
