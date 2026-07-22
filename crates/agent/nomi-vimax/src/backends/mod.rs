//! Flowy-only backends for chat / image / video.

mod chat;
mod image;
mod traits;
mod video;

pub use chat::FlowyChat;
pub use image::FlowyImage;
pub use traits::{VimaxChat, VimaxImage, VimaxVideo};
pub use video::FlowyVideo;

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::{GatewayConfig, MediaGenConfig, ServerConfig};
use nomifun_cloud::{FlowyApiClient, ServerSession};

/// Shared handle for Flowy-authenticated ViMax backends.
#[derive(Clone)]
pub struct FlowyVimaxServices {
    pub api: Arc<FlowyApiClient>,
    pub session: ServerSession,
    pub media: MediaGenConfig,
    pub server: ServerConfig,
    pub data_dir: PathBuf,
}

impl FlowyVimaxServices {
    pub fn try_new(config: &GatewayConfig, data_dir: &std::path::Path) -> Option<Self> {
        if !config.server.api_ready() {
            return None;
        }
        let api = FlowyApiClient::new(&config.server).ok()?;
        let session = ServerSession::from_config(&config.server, data_dir);
        Some(Self {
            api: Arc::new(api),
            session,
            media: config.media.clone(),
            server: config.server.clone(),
            data_dir: data_dir.to_path_buf(),
        })
    }

    pub async fn require_token(&self) -> Result<(), crate::error::VimaxError> {
        let tok = self
            .session
            .access_token()
            .await
            .map_err(|e| crate::error::VimaxError::msg(e.to_string()))?
            .filter(|t| !t.trim().is_empty());
        if tok.is_none() {
            return Err(crate::error::VimaxError::NotAuthenticated);
        }
        Ok(())
    }

    pub fn chat(&self) -> FlowyChat {
        FlowyChat::new(self.clone(), None)
    }

    pub fn chat_with_model(&self, model: Option<String>) -> FlowyChat {
        FlowyChat::new(self.clone(), model)
    }

    pub fn image(&self) -> FlowyImage {
        FlowyImage::new(self.clone(), None)
    }

    pub fn image_with_model(&self, model: Option<String>) -> FlowyImage {
        FlowyImage::new(self.clone(), model)
    }

    pub fn video(&self) -> FlowyVideo {
        FlowyVideo::new(self.clone(), None, None)
    }

    pub fn video_with_model(&self, model: Option<String>) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, None)
    }

    pub fn video_with_model_and_cancel(
        &self,
        model: Option<String>,
        cancel: Option<tokio_util::sync::CancellationToken>,
    ) -> FlowyVideo {
        FlowyVideo::new(self.clone(), model, cancel)
    }
}

pub(crate) fn map_server_err(err: nomifun_cloud::ServerClientError) -> crate::error::VimaxError {
    crate::error::VimaxError::msg(err.to_string())
}

/// Classify a Flowy upstream error for chat / image / video calls.
pub(crate) fn map_model_err(
    kind: &str,
    model: Option<&str>,
    stage_hint: &str,
    err: nomifun_cloud::ServerClientError,
) -> crate::error::VimaxError {
    let raw = err.to_string();
    let model_label = model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("（默认）");
    let lower = raw.to_ascii_lowercase();
    let hint = if lower.contains("datainspectionfailed")
        || lower.contains("inappropriate content")
        || lower.contains("不当内容")
        || lower.contains("内容安全")
        || lower.contains("敏感内容")
    {
        "图片提示词/结果触发了上游内容安全审核。客户端会按 词表清洗→严格清洗→LLM语义重写→极简安全兜底 自动重试；若仍失败，请弱化分镜里的暴力/敏感描写后从断点继续。"
    } else if lower.contains("all channel models failed") || lower.contains("所有渠道模型均失败")
    {
        if lower.contains("datainspection") || lower.contains("inappropriate") {
            "渠道失败由内容安全审核引起。客户端已做多级提示词安全重写；请检查分镜描述是否含暴力/敏感内容。"
        } else {
            "上游表示该模型当前所有通道均不可用（可能是内容审核、额度、熔断或临时故障）。请查看原因中的 upstream 明细，或更换模型后重试。"
        }
    } else if lower.contains("empty content") {
        if lower.contains("system_len=0") || lower.contains("user_len=0") {
            "请求侧 system/user 提示词为空，不是模型问题。请检查场景脚本是否生成成功。"
        } else if lower.contains("finish_reason=length") {
            "模型输出被截断（思考过程占满 token）。请换非思考型模型，或稍后从断点继续。"
        } else {
            "上游返回了空正文（常见于思考型模型把内容放在 reasoning 字段）。已尝试兼容解析；请换模型或从断点继续。错误详情中的 system_len/user_len 可确认请求并非空提示。"
        }
    } else if lower.contains("refusing llm call with empty prompt")
        || lower.contains("refusing multimodal llm call")
    {
        "请求侧提示词为空，不是模型返回问题。请检查上一阶段产物（如 script.txt）是否为空。"
    } else if lower.contains("privacyinformation")
        || lower.contains("inputimagesensitivecontent")
        || lower.contains("may contain real person")
    {
        "首帧/参考图被判定含真人肖像。已支持自动 stylize 重试；若仍失败，请换更卡通/插画风格后从断点继续。"
    } else if lower.contains("401") || lower.contains("unauthorized") {
        "鉴权失败，请确认已登录 Flowy 云账号。"
    } else if lower.contains("429") || lower.contains("rate limit") {
        "请求过于频繁，请稍后重试。"
    } else {
        "请检查所选模型是否可用，或稍后从断点继续。"
    };
    let kind_label = match kind {
        "image" => "图片生成",
        "video" => "视频生成",
        _ => "聊天模型（LLM）",
    };
    let msg = format!(
        "{kind_label}失败\n模型：{model_label}\n阶段：{stage_hint}\n原因：{raw}\n建议：{hint}"
    );
    match kind {
        "image" => crate::error::VimaxError::Image(msg),
        "video" => crate::error::VimaxError::Video(msg),
        _ => crate::error::VimaxError::Llm(msg),
    }
}
