use nomifun_api_types::{
    VideoGrowthEventBatchRequest, VideoGrowthEventBatchResponse, VideoGrowthMetricsResponse,
};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::FlowyApiClient;

const VIDEO_GROWTH_EVENTS_PATH: &str = "/v1/growth/video-generation/events/batch";

pub(crate) fn video_growth_metrics_path(days: u16) -> String {
    format!(
        "/v1/growth/video-generation/north-star?days={}",
        days.clamp(1, 90)
    )
}

impl FlowyApiClient {
    pub async fn upload_video_growth_events(
        &self,
        session: &ServerSession,
        request: &VideoGrowthEventBatchRequest,
    ) -> Result<VideoGrowthEventBatchResponse, ServerClientError> {
        self.post_data(VIDEO_GROWTH_EVENTS_PATH, Some(session), request)
            .await
    }

    pub async fn get_video_growth_metrics(
        &self,
        session: &ServerSession,
        days: u16,
    ) -> Result<VideoGrowthMetricsResponse, ServerClientError> {
        self.get_data(&video_growth_metrics_path(days), Some(session))
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

    #[test]
    fn metric_window_is_bounded() {
        assert_eq!(
            video_growth_metrics_path(0),
            "/v1/growth/video-generation/north-star?days=1"
        );
        assert_eq!(
            video_growth_metrics_path(120),
            "/v1/growth/video-generation/north-star?days=90"
        );
    }

    #[tokio::test]
    async fn uploads_growth_batch_with_cloud_session() {
        let server = MockServer::start().await;
        let request = VideoGrowthEventBatchRequest {
            events: vec![VideoGrowthEvent {
                event_id: "video:film_succeeded:session-1".into(),
                name: "film_succeeded".into(),
                occurred_at: "2026-08-26T00:00:00Z".into(),
                properties: BTreeMap::new(),
                cohort: Some("A".into()),
            }],
        };
        Mock::given(method("POST"))
            .and(path(VIDEO_GROWTH_EVENTS_PATH))
            .and(header("authorization", "Bearer test-token"))
            .and(body_json(&request))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"code":200,"msg":"ok","data":{"accepted":1,"duplicates":0}}"#,
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
    }
}
