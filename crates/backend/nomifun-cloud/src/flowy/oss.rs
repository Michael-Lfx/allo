//! OSS presigned PUT upload — avoid base64 frames in video create bodies.

use tracing::debug;

use crate::error::ServerClientError;
use crate::flowy::media_types::{OssPresignPutData, OssPresignPutRequest};
use crate::session::ServerSession;

use super::FlowyApiClient;

/// Default presign TTL (15 minutes); matches server default.
const DEFAULT_PRESIGN_EXPIRES_SECS: u64 = 900;

impl FlowyApiClient {
    /// `POST {业务根}/uploads/oss/presignPut` — JWT only (not API Key).
    pub async fn presign_oss_put(
        &self,
        session: &ServerSession,
        file_name: &str,
        content_type: &str,
        expires_seconds: Option<u64>,
    ) -> Result<OssPresignPutData, ServerClientError> {
        let body = OssPresignPutRequest {
            file_name: file_name.trim().to_string(),
            content_type: content_type.trim().to_string(),
            expires_seconds: Some(expires_seconds.unwrap_or(DEFAULT_PRESIGN_EXPIRES_SECS)),
        };
        self.post_data("/uploads/oss/presignPut", Some(session), &body)
            .await
    }

    /// Presign + PUT binary to OSS; returns the HTTPS `publicUrl` for video frames.
    pub async fn upload_bytes_via_oss(
        &self,
        session: &ServerSession,
        bytes: &[u8],
        file_name: &str,
        content_type: &str,
    ) -> Result<String, ServerClientError> {
        if bytes.is_empty() {
            return Err(ServerClientError::InvalidResponse(
                "refusing OSS upload of empty body".into(),
            ));
        }
        let content_type = if content_type.trim().is_empty() {
            "application/octet-stream"
        } else {
            content_type.trim()
        };
        let file_name = if file_name.trim().is_empty() {
            "upload.bin"
        } else {
            file_name.trim()
        };

        let presign = self
            .presign_oss_put(session, file_name, content_type, None)
            .await?;

        let public_url = presign
            .public_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .ok_or_else(|| {
                ServerClientError::InvalidResponse(
                    "OSS presign missing publicUrl (cdn_base_url not configured on server)".into(),
                )
            })?
            .to_string();

        self.put_presigned_object(&presign, content_type, bytes)
            .await?;
        Ok(public_url)
    }

    /// HTTP PUT to the presigned URL with `requiredHeaders` (no Flowy auth injection).
    async fn put_presigned_object(
        &self,
        presign: &OssPresignPutData,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<(), ServerClientError> {
        let method = if presign.method.trim().is_empty() {
            "PUT"
        } else {
            presign.method.trim()
        };
        if !method.eq_ignore_ascii_case("PUT") {
            return Err(ServerClientError::InvalidResponse(format!(
                "unsupported OSS upload method: {method}"
            )));
        }
        if presign.url.trim().is_empty() {
            return Err(ServerClientError::InvalidResponse(
                "OSS presign missing upload url".into(),
            ));
        }

        // Signature binds Content-Type; keep requiredHeaders, fill Content-Type if absent.
        let mut headers = presign.required_headers.clone();
        let has_content_type = headers.keys().any(|k| k.eq_ignore_ascii_case("content-type"));
        if !has_content_type {
            headers.insert("Content-Type".into(), content_type.to_string());
        }

        debug!(
            object_key = ?presign.object_key,
            bytes = bytes.len(),
            "OSS presigned PUT"
        );

        let resp = self
            .transport
            .put_bytes_absolute(&presign.url, &headers, bytes.to_vec())
            .await?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_else(|_| String::new());
        let preview: String = body.chars().take(240).collect();
        Err(ServerClientError::Http(format!(
            "OSS PUT failed: HTTP {status}: {preview}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::ServerConfig;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: &str) -> ServerConfig {
        ServerConfig {
            base_url: base_url.to_string(),
            channel: "flowy".to_string(),
            app: "flowymes".to_string(),
            ..Default::default()
        }
    }

    fn test_session(config: &ServerConfig) -> (tempfile::TempDir, ServerSession) {
        let tmp = tempfile::tempdir().expect("tmpdir");
        unsafe { std::env::set_var("NOMIFUN_SERVER_TOKEN", "jwt-test-oss") };
        let session = ServerSession::from_config(config, tmp.path());
        (tmp, session)
    }

    #[tokio::test]
    async fn upload_bytes_via_oss_presign_then_put() {
        let server = MockServer::start().await;
        let put_path = "/bucket/obj.png";
        let public_url = format!("{}/cdn/obj.png", server.uri());
        let put_url = format!("{}{}", server.uri(), put_path);

        Mock::given(method("POST"))
            .and(path("/uploads/oss/presignPut"))
            .and(body_partial_json(json!({
                "fileName": "first-frame.png",
                "contentType": "image/png",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "ok",
                "data": {
                    "method": "PUT",
                    "url": put_url,
                    "requiredHeaders": { "Content-Type": "image/png" },
                    "objectKey": "claw/presigned/1/obj.png",
                    "publicUrl": public_url,
                }
            })))
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path(put_path))
            .and(header("content-type", "image/png"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let api = FlowyApiClient::new(&config).expect("client");
        let (_tmp, session) = test_session(&config);
        let url = api
            .upload_bytes_via_oss(
                &session,
                b"\x89PNG\r\n\x1a\nfake",
                "first-frame.png",
                "image/png",
            )
            .await
            .expect("upload");
        assert_eq!(url, public_url);
    }

    #[tokio::test]
    async fn upload_bytes_via_oss_requires_public_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/uploads/oss/presignPut"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200,
                "msg": "ok",
                "data": {
                    "method": "PUT",
                    "url": format!("{}/put", server.uri()),
                    "requiredHeaders": { "Content-Type": "image/jpeg" },
                    "objectKey": "claw/x.jpg"
                }
            })))
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let api = FlowyApiClient::new(&config).expect("client");
        let (_tmp, session) = test_session(&config);
        let err = api
            .upload_bytes_via_oss(&session, b"jpeg-bytes", "a.jpg", "image/jpeg")
            .await
            .expect_err("missing publicUrl");
        assert!(
            err.to_string().contains("publicUrl"),
            "unexpected error: {err}"
        );
    }
}
