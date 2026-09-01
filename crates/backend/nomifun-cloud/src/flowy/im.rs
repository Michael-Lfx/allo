//! Flowy customer-support IM client (unique conversation per user).

use nomifun_api_types::{
    CloudImConversation, CloudImLogUploadResponse, CloudImMessage, CloudImMessageList,
    CloudImReadRequest, CloudImSendMessageRequest,
};
use serde_json::Value;

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::{form_urlencode, handle_http_and_envelope, FlowyApiClient};

const FEEDBACK_LOG_UPLOAD_PATH: &str = "/uploads/feedback/log";
const FEEDBACK_SCREENSHOT_UPLOAD_PATH: &str = "/uploads/feedback/screenshot";

impl FlowyApiClient {
    pub async fn get_im_conversation(
        &self,
        session: &ServerSession,
        app: Option<&str>,
    ) -> Result<CloudImConversation, ServerClientError> {
        let mut path = String::from("/im/conversation");
        if let Some(app) = app.map(str::trim).filter(|s| !s.is_empty()) {
            path.push_str("?app=");
            path.push_str(&form_urlencode(app));
        }
        self.get_data(&path, Some(session)).await
    }

    pub async fn list_im_messages(
        &self,
        session: &ServerSession,
        after_seq: Option<i64>,
        before_seq: Option<i64>,
        limit: i64,
    ) -> Result<CloudImMessageList, ServerClientError> {
        let mut pairs: Vec<(String, String)> = Vec::new();
        if let Some(after) = after_seq.filter(|v| *v > 0) {
            pairs.push(("afterSeq".into(), after.to_string()));
        } else if let Some(before) = before_seq.filter(|v| *v > 0) {
            pairs.push(("beforeSeq".into(), before.to_string()));
        }
        if limit > 0 {
            pairs.push(("limit".into(), limit.to_string()));
        }
        let path = if pairs.is_empty() {
            "/im/messages".to_string()
        } else {
            let query = pairs
                .into_iter()
                .map(|(k, v)| format!("{k}={}", form_urlencode(&v)))
                .collect::<Vec<_>>()
                .join("&");
            format!("/im/messages?{query}")
        };
        self.get_data(&path, Some(session)).await
    }

    pub async fn send_im_message(
        &self,
        session: &ServerSession,
        request: &CloudImSendMessageRequest,
    ) -> Result<CloudImMessage, ServerClientError> {
        self.post_data("/im/messages", Some(session), request).await
    }

    pub async fn mark_im_read(
        &self,
        session: &ServerSession,
        last_read_seq: i64,
    ) -> Result<CloudImConversation, ServerClientError> {
        let body = CloudImReadRequest { last_read_seq };
        self.post_data("/im/read", Some(session), &body).await
    }

    pub async fn upload_feedback_log(
        &self,
        session: &ServerSession,
        file_bytes: Vec<u8>,
        file_name: &str,
        content_type: &str,
    ) -> Result<CloudImLogUploadResponse, ServerClientError> {
        self.upload_feedback_file(
            FEEDBACK_LOG_UPLOAD_PATH,
            session,
            file_bytes,
            file_name,
            content_type,
        )
        .await
    }

    /// FlowyClaw `POST /uploads/feedback/screenshot` — multipart `file`, 返回
    /// `{ oss_id, objectKey, url, name, contentType, byteSize }`，可直接组装 IM `payload`。
    pub async fn upload_feedback_screenshot(
        &self,
        session: &ServerSession,
        file_bytes: Vec<u8>,
        file_name: &str,
        content_type: &str,
    ) -> Result<CloudImLogUploadResponse, ServerClientError> {
        self.upload_feedback_file(
            FEEDBACK_SCREENSHOT_UPLOAD_PATH,
            session,
            file_bytes,
            file_name,
            content_type,
        )
        .await
    }

    async fn upload_feedback_file(
        &self,
        upload_path: &str,
        session: &ServerSession,
        file_bytes: Vec<u8>,
        file_name: &str,
        content_type: &str,
    ) -> Result<CloudImLogUploadResponse, ServerClientError> {
        let byte_size = file_bytes.len() as i64;
        let file_part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_owned())
            .mime_str(content_type)
            .map_err(|e| ServerClientError::Http(format!("invalid MIME type: {e}")))?;
        let form = reqwest::multipart::Form::new().part("file", file_part);

        let resp = self
            .transport
            .post_multipart(upload_path, Some(session), form)
            .await?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| ServerClientError::Http(e.to_string()))?;
        let env = handle_http_and_envelope(status, &body)?;
        let data = env.into_data::<Value>()?;
        normalize_feedback_log_upload(data, file_name, content_type, byte_size)
    }
}

fn json_str_field<'a>(data: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(value) = data.get(*key).and_then(|v| v.as_str()).map(str::trim) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn json_i64_field(data: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(v) = data.get(*key) {
            if let Some(n) = v.as_i64() {
                return Some(n);
            }
            if let Some(s) = v.as_str().and_then(|s| s.trim().parse::<i64>().ok()) {
                return Some(s);
            }
        }
    }
    None
}

pub(crate) fn normalize_feedback_log_upload(
    data: Value,
    file_name: &str,
    content_type: &str,
    byte_size: i64,
) -> Result<CloudImLogUploadResponse, ServerClientError> {
    // FlowyClaw contract: `/uploads/feedback/log` returns `{ oss_id }` only.
    let oss_id = json_i64_field(&data, &["ossId", "oss_id"]).ok_or_else(|| {
        ServerClientError::InvalidResponse(
            "feedback log upload response missing oss_id".into(),
        )
    })?;
    let url = json_str_field(
        &data,
        &[
            "url",
            "fileUrl",
            "file_url",
            "ossUrl",
            "oss_url",
            "publicUrl",
            "public_url",
        ],
    )
    .map(str::to_string);

    Ok(CloudImLogUploadResponse {
        oss_id,
        url,
        name: json_str_field(&data, &["name", "fileName", "file_name"])
            .unwrap_or(file_name)
            .to_string(),
        content_type: json_str_field(&data, &["contentType", "content_type", "mimeType"])
            .unwrap_or(content_type)
            .to_string(),
        byte_size: json_i64_field(&data, &["byteSize", "byte_size", "size"]).unwrap_or(byte_size),
        object_key: json_str_field(&data, &["objectKey", "object_key"]).map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::ServerConfig;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: &str) -> ServerConfig {
        ServerConfig {
            base_url: base_url.to_string(),
            channel: "flowy".to_string(),
            app: "flowymes".to_string(),
            ..Default::default()
        }
    }

    async fn test_client_and_session(
        server: &MockServer,
    ) -> (FlowyApiClient, ServerSession, tempfile::TempDir) {
        let config = test_config(&server.uri());
        let client = FlowyApiClient::new(&config).expect("client");
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "test-token") };
        let session = ServerSession::from_config(&config, tmp.path());
        (client, session, tmp)
    }

    #[test]
    fn normalizes_oss_id_and_optional_url() {
        let data = json!({
            "file_url": "https://cdn.example/a.zip",
            "oss_id": 10002
        });
        let parsed = normalize_feedback_log_upload(data, "a.zip", "application/zip", 9).unwrap();
        assert_eq!(parsed.oss_id, 10002);
        assert_eq!(parsed.url.as_deref(), Some("https://cdn.example/a.zip"));
    }

    #[test]
    fn accepts_oss_id_only_response() {
        let data = json!({ "oss_id": 10002 });
        let parsed = normalize_feedback_log_upload(data, "a.zip", "application/zip", 9).unwrap();
        assert_eq!(parsed.oss_id, 10002);
        assert!(parsed.url.is_none());
        assert_eq!(parsed.name, "a.zip");
        assert_eq!(parsed.byte_size, 9);
    }

    #[test]
    fn rejects_missing_oss_id_response() {
        let data = json!({ "url": "https://cdn.example/a.zip" });
        let err = normalize_feedback_log_upload(data, "a.zip", "application/zip", 9).unwrap_err();
        assert!(matches!(err, ServerClientError::InvalidResponse(_)));
    }

    #[tokio::test]
    async fn gets_unique_im_conversation_with_auth() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/im/conversation"))
            .and(query_param("app", "flowymes"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "ok",
                "data": {
                    "id": 1001,
                    "userId": 20001,
                    "externalChannelCode": "flowy",
                    "app": "flowymes",
                    "status": "open",
                    "assigneeSysUserId": null,
                    "lastSeq": 12,
                    "lastMessageId": 90012,
                    "lastMessageAt": "2026-07-23T16:00:00+08:00",
                    "lastMessagePreview": "请提供订单号",
                    "lastSenderType": "sys_user",
                    "userUnreadCount": 1,
                    "opsUnreadCount": 0,
                    "hasUnread": true,
                    "createdAt": "2026-07-20T10:00:00+08:00",
                    "updatedAt": "2026-07-23T16:00:00+08:00",
                    "closedAt": null
                }
            })))
            .mount(&server)
            .await;

        let (client, session, _tmp) = test_client_and_session(&server).await;
        let result = client
            .get_im_conversation(&session, Some("flowymes"))
            .await
            .unwrap();
        assert!(result.has_unread);
        assert_eq!(result.last_seq, 12);
        assert_eq!(result.id, 1001);
    }

    #[tokio::test]
    async fn lists_im_messages_prefers_after_seq() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/im/messages"))
            .and(query_param("afterSeq", "5"))
            .and(query_param("limit", "50"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "ok",
                "data": {
                    "list": [{
                        "id": 90001,
                        "conversationId": 1001,
                        "seq": 6,
                        "clientMsgId": "c-uuid-1",
                        "senderType": "sys_user",
                        "senderId": 1,
                        "msgType": "text",
                        "content": "收到",
                        "status": "sent",
                        "createdAt": "2026-07-23T15:00:00+08:00"
                    }]
                }
            })))
            .mount(&server)
            .await;

        let (client, session, _tmp) = test_client_and_session(&server).await;
        let result = client
            .list_im_messages(&session, Some(5), Some(99), 50)
            .await
            .unwrap();
        assert_eq!(result.list.len(), 1);
        assert_eq!(result.list[0].seq, 6);
        assert_eq!(result.list[0].content, "收到");
    }

    #[tokio::test]
    async fn uploads_feedback_log_and_returns_oss_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/uploads/feedback/log"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "操作成功",
                "data": {
                    "oss_id": 10002
                }
            })))
            .mount(&server)
            .await;

        let (client, session, _tmp) = test_client_and_session(&server).await;
        let result = client
            .upload_feedback_log(
                &session,
                b"zip-bytes".to_vec(),
                "app-logs.zip",
                "application/zip",
            )
            .await
            .unwrap();
        assert_eq!(result.oss_id, 10002);
        assert!(result.url.is_none());
        assert_eq!(result.name, "app-logs.zip");
        assert_eq!(result.byte_size, 9);
    }

    #[tokio::test]
    async fn uploads_feedback_screenshot_and_returns_im_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/uploads/feedback/screenshot"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "操作成功",
                "data": {
                    "oss_id": 10001,
                    "objectKey": "claw/feedback/screenshots/20260727/x.png",
                    "url": "https://cdn.example/x.png",
                    "name": "screenshot.png",
                    "contentType": "image/png",
                    "byteSize": 9
                }
            })))
            .mount(&server)
            .await;

        let (client, session, _tmp) = test_client_and_session(&server).await;
        let result = client
            .upload_feedback_screenshot(&session, b"png-bytes".to_vec(), "screenshot.png", "image/png")
            .await
            .unwrap();
        assert_eq!(result.oss_id, 10001);
        assert_eq!(result.url.as_deref(), Some("https://cdn.example/x.png"));
        assert_eq!(
            result.object_key.as_deref(),
            Some("claw/feedback/screenshots/20260727/x.png")
        );
        assert_eq!(result.content_type, "image/png");
        assert_eq!(result.byte_size, 9);
    }

    #[tokio::test]
    async fn sends_im_message_with_log_payload_and_marks_read() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/im/messages"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "ok",
                "data": {
                    "id": 90002,
                    "conversationId": 1001,
                    "seq": 7,
                    "clientMsgId": "550e8400-e29b-41d4-a716-446655440000",
                    "senderType": "user",
                    "senderId": 20001,
                    "msgType": "text",
                    "content": "你好",
                    "status": "sent",
                    "createdAt": "2026-07-23T15:01:00+08:00",
                    "duplicate": false,
                    "logPayload": {
                        "url": "https://cdn.example/a.zip",
                        "name": "a.zip",
                        "contentType": "application/zip",
                        "byteSize": 9
                    }
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/im/read"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "ok",
                "data": {
                    "id": 1001,
                    "userId": 20001,
                    "externalChannelCode": "flowy",
                    "app": "flowymes",
                    "status": "open",
                    "assigneeSysUserId": null,
                    "lastSeq": 7,
                    "lastMessageId": 90002,
                    "lastMessageAt": "2026-07-23T15:01:00+08:00",
                    "lastMessagePreview": "你好",
                    "lastSenderType": "user",
                    "userUnreadCount": 0,
                    "opsUnreadCount": 1,
                    "hasUnread": false,
                    "createdAt": "2026-07-20T10:00:00+08:00",
                    "updatedAt": "2026-07-23T15:01:00+08:00",
                    "closedAt": null
                }
            })))
            .mount(&server)
            .await;

        let (client, session, _tmp) = test_client_and_session(&server).await;
        let sent = client
            .send_im_message(
                &session,
                &CloudImSendMessageRequest {
                    client_msg_id: "550e8400-e29b-41d4-a716-446655440000".into(),
                    content: "你好".into(),
                    msg_type: "text".into(),
                    app: Some("flowymes".into()),
                    payload: None,
                    log_payload: Some(nomifun_api_types::CloudImAttachmentPayload {
                        object_key: None,
                        url: Some("https://cdn.example/a.zip".into()),
                        oss_id: None,
                        name: "a.zip".into(),
                        content_type: "application/zip".into(),
                        byte_size: 9,
                        account: None,
                        device: None,
                        extra: Default::default(),
                    }),
                },
            )
            .await
            .unwrap();
        assert_eq!(sent.seq, 7);
        assert!(!sent.duplicate);
        assert_eq!(
            sent.log_payload
                .as_ref()
                .and_then(|p| p.url.as_deref()),
            Some("https://cdn.example/a.zip")
        );

        let read = client.mark_im_read(&session, 7).await.unwrap();
        assert!(!read.has_unread);
        assert_eq!(read.user_unread_count, 0);
    }
}
