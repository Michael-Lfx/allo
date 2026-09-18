use nomifun_api_types::{VideoGrowthEventBatchRequest, VideoGrowthEventBatchResponse};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::FlowyApiClient;

const TELEMETRY_EVENTS_PATH: &str = "/telemetry/events/batch";

impl FlowyApiClient {
    pub async fn upload_video_growth_events(
        &self,
        session: &ServerSession,
        request: &VideoGrowthEventBatchRequest,
    ) -> Result<VideoGrowthEventBatchResponse, ServerClientError> {
        self.post_data(TELEMETRY_EVENTS_PATH, Some(session), request)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nomi_config::ServerConfig;
    use nomifun_api_types::VideoGrowthEvent;
    use tempfile::tempdir;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::session::ServerTokens;

    use super::*;

    #[tokio::test]
    async fn uploads_growth_batch_with_cloud_session() {
        let server = MockServer::start().await;
        let request = VideoGrowthEventBatchRequest {
            events: vec![VideoGrowthEvent {
                event_id: "video:film_succeeded:session-1".into(),
                name: "film_succeeded".into(),
                occurred_at: "2026-08-26T00:00:00Z".into(),
                module: Some("video_generation".into()),
                properties: BTreeMap::new(),
                cohort: Some("A".into()),
            }],
            client_id: None,
            app: None,
            platform: None,
            app_version: None,
        };
        Mock::given(method("POST"))
            .and(path(TELEMETRY_EVENTS_PATH))
            .and(header("authorization", "Bearer test-token"))
            .and(body_json(&request))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"accepted":1,"duplicates":0,"rejected":0}}"#,
            ))
            .mount(&server)
            .await;

        let config = ServerConfig {
            base_url: server.uri(),
            ..Default::default()
        };
        let data_dir = tempdir().expect("temp data dir");
        let session = ServerSession::from_config(&config, data_dir.path());
        session
            .save_tokens(ServerTokens::from_jwt("test-token".into()))
            .await
            .expect("save token");
        let response = FlowyApiClient::new(&config)
            .expect("growth client")
            .upload_video_growth_events(&session, &request)
            .await
            .expect("growth upload");

        assert_eq!(response.accepted, 1);
        assert_eq!(response.duplicates, 0);
        assert_eq!(response.rejected, 0);
    }
}
