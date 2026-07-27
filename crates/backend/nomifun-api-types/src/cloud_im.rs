//! Flowy cloud IM (customer support) DTOs — unique conversation per C-end user.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudImConversation {
    pub id: i64,
    pub user_id: i64,
    pub external_channel_code: String,
    pub app: String,
    pub status: String,
    pub assignee_sys_user_id: Option<i64>,
    pub last_seq: i64,
    pub last_message_id: Option<i64>,
    pub last_message_at: Option<String>,
    pub last_message_preview: Option<String>,
    pub last_sender_type: Option<String>,
    pub user_unread_count: i64,
    pub ops_unread_count: i64,
    pub has_unread: bool,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
}

/// Attachment snapshot used by `payload` / `logPayload`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudImAttachmentPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_key: Option<String>,
    /// CDN URL when available. Optional when `object_key` is supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub name: String,
    pub content_type: String,
    pub byte_size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudImMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub seq: i64,
    pub client_msg_id: Option<String>,
    pub sender_type: String,
    pub sender_id: Option<i64>,
    pub msg_type: String,
    pub content: String,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub duplicate: bool,
    /// Attachment snapshot for `image` / `file` messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<CloudImAttachmentPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_payload: Option<CloudImAttachmentPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudImMessageList {
    pub list: Vec<CloudImMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudImSendMessageRequest {
    pub client_msg_id: String,
    pub content: String,
    pub msg_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Required when `msg_type` is `image` / `file` (doc §5.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<CloudImAttachmentPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_payload: Option<CloudImAttachmentPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudImReadRequest {
    pub last_read_seq: i64,
}

/// Normalized response from local `/api/cloud/im/logs/upload` and
/// `/api/cloud/im/screenshots/upload`.
/// Matches FlowyClaw `/uploads/feedback/log` & `/uploads/feedback/screenshot`:
/// `data.oss_id` is required; IM attachment fields are passed through when present.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudImLogUploadResponse {
    pub oss_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub name: String,
    pub content_type: String,
    pub byte_size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn send_request_serializes_log_payload_camel_case() {
        let req = CloudImSendMessageRequest {
            client_msg_id: "c1".into(),
            content: "hello".into(),
            msg_type: "text".into(),
            app: Some("flowymes".into()),
            payload: None,
            log_payload: Some(CloudImAttachmentPayload {
                object_key: Some("claw/presigned/1/x.zip".into()),
                url: Some("https://cdn.example/x.zip".into()),
                name: "app-logs.zip".into(),
                content_type: "application/zip".into(),
                byte_size: 123,
                account: Some(json!({"userId": "u1"})),
                device: Some(json!({"platform": "win32"})),
                extra: Map::new(),
            }),
        };
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["logPayload"]["url"], "https://cdn.example/x.zip");
        assert_eq!(value["logPayload"]["contentType"], "application/zip");
        assert_eq!(value["logPayload"]["byteSize"], 123);
        assert_eq!(value["logPayload"]["account"]["userId"], "u1");
        assert_eq!(value["logPayload"]["device"]["platform"], "win32");
        assert!(value["logPayload"].get("userInfo").is_none());
        assert!(value["logPayload"].get("deviceInfo").is_none());
        assert!(value.get("payload").is_none());
    }

    #[test]
    fn send_request_serializes_image_payload_camel_case() {
        let req = CloudImSendMessageRequest {
            client_msg_id: "c2".into(),
            content: "截图说明".into(),
            msg_type: "image".into(),
            app: Some("flowymes".into()),
            payload: Some(CloudImAttachmentPayload {
                object_key: Some("claw/feedback/screenshots/20260727/x.png".into()),
                url: Some("https://cdn.example/x.png".into()),
                name: "screenshot.png".into(),
                content_type: "image/png".into(),
                byte_size: 152340,
                account: None,
                device: None,
                extra: Map::new(),
            }),
            log_payload: None,
        };
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["msgType"], "image");
        assert_eq!(value["payload"]["objectKey"], "claw/feedback/screenshots/20260727/x.png");
        assert_eq!(value["payload"]["contentType"], "image/png");
        assert_eq!(value["payload"]["byteSize"], 152340);
        assert!(value.get("logPayload").is_none());
    }
}
