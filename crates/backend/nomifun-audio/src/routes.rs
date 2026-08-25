//! HTTP control plane for MeetingSession (E1).

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Json, Path, Query, State};
use axum::routing::{delete, get, patch, post};
use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::AppError;
use serde::{Deserialize, Serialize};

use crate::devices::{AudioDeviceManager, DeviceKind};
use crate::process_watch::detect_meeting_process;
use crate::runtime::MeetingRuntime;
use crate::session::{
    CreateMeetingSessionRequest, GenerateMeetingNotesResult, MeetingNotesView,
    MeetingSegmentSnapshot, MeetingSessionService, MeetingSessionSnapshot, SttBackendChoice,
};
use crate::voiceprint::{FakeVoiceprintEncoder, VoiceprintStore, embedding_from_blob};

/// Router state shared by meeting HTTP handlers.
#[derive(Clone)]
pub struct MeetingRouterState {
    pub service: MeetingSessionService,
    pub runtime: Arc<MeetingRuntime>,
    /// Parent directory; each create nests `{meetings_root}/{session_id}/`.
    pub meetings_root: PathBuf,
    pub voiceprints: Arc<VoiceprintStore<FakeVoiceprintEncoder>>,
}

pub fn meeting_routes(state: MeetingRouterState) -> Router {
    Router::new()
        .route("/api/meetings", get(list_sessions).post(create_session))
        .route("/api/meetings/devices", get(list_devices))
        .route("/api/meetings/detected-apps", get(detected_apps))
        .route(
            "/api/meetings/voiceprints",
            get(list_voiceprints)
                .post(enroll_voiceprint)
                .delete(clear_voiceprints),
        )
        .route(
            "/api/meetings/voiceprints/{voiceprint_id}",
            delete(delete_voiceprint),
        )
        .route("/api/meetings/{id}", get(get_session))
        .route("/api/meetings/{id}/start", post(start_session))
        .route("/api/meetings/{id}/pause", post(pause_session))
        .route("/api/meetings/{id}/resume", post(resume_session))
        .route("/api/meetings/{id}/stop", post(stop_session))
        .route("/api/meetings/{id}/bind", post(bind_conversation))
        .route("/api/meetings/{id}/segments", get(list_segments))
        .route("/api/meetings/{id}/segments/search", get(search_segments))
        .route(
            "/api/meetings/{id}/segments/{segment_id}",
            patch(edit_segment),
        )
        .route(
            "/api/meetings/{id}/notes",
            get(get_notes),
        )
        .route(
            "/api/meetings/{id}/notes/generate",
            post(generate_notes),
        )
        .with_state(state)
}

fn map_service_err(e: String) -> AppError {
    if e.contains("not found") {
        AppError::NotFound(e)
    } else {
        AppError::Internal(e)
    }
}

async fn require_owned(
    service: &MeetingSessionService,
    user_id: &str,
    session_id: &str,
) -> Result<MeetingSessionSnapshot, AppError> {
    let session = service
        .get_session(session_id)
        .await
        .map_err(map_service_err)?
        .ok_or_else(|| AppError::NotFound("meeting".into()))?;
    if session.user_id != user_id {
        return Err(AppError::NotFound("meeting".into()));
    }
    Ok(session)
}

#[derive(Debug, Deserialize)]
struct CreateMeetingBody {
    title: Option<String>,
    bound_conversation_id: Option<String>,
    stt_backend: Option<SttBackendChoice>,
}

#[derive(Debug, Deserialize)]
struct BindBody {
    conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_search_limit")]
    limit: i64,
}

fn default_search_limit() -> i64 {
    50
}

#[derive(Debug, Deserialize)]
struct EditSegmentBody {
    text: String,
}

#[derive(Debug, Deserialize)]
struct EnrollVoiceprintBody {
    display_name: String,
    /// Placeholder / precomputed embedding (enroll without PCM).
    embedding: Option<Vec<f32>>,
    pcm: Option<Vec<f32>>,
    sample_rate: Option<u32>,
}

#[derive(Debug, Serialize)]
struct DeviceDto {
    id: String,
    name: String,
    kind: String,
    is_default: bool,
}

#[derive(Debug, Serialize)]
struct DetectedAppsDto {
    /// Y1 tip only — human-readable meeting app name when detected.
    app: Option<String>,
}

#[derive(Debug, Serialize)]
struct VoiceprintDto {
    voiceprint_id: String,
    display_name: String,
    embedding_dims: usize,
    created_at_ms: i64,
    updated_at_ms: i64,
}

async fn create_session(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<CreateMeetingBody>, JsonRejection>,
) -> Result<Json<ApiResponse<MeetingSessionSnapshot>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let mgr = AudioDeviceManager::new();
    let mic_available = mgr
        .list_devices(DeviceKind::Input)
        .map(|d| !d.is_empty())
        .unwrap_or(false);
    let loopback_available = mgr
        .list_devices(DeviceKind::Output)
        .map(|d| !d.is_empty())
        .unwrap_or(false);

    let snap = state
        .service
        .create_session(CreateMeetingSessionRequest {
            user_id: user.id.as_str().to_string(),
            title: req
                .title
                .unwrap_or_else(|| "Meeting".to_string()),
            data_dir: state.meetings_root.to_string_lossy().into_owned(),
            bound_conversation_id: req.bound_conversation_id,
            stt_backend: req.stt_backend.unwrap_or(SttBackendChoice::Auto),
            mic_available,
            loopback_available,
        })
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(snap)))
}

async fn list_sessions(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<MeetingSessionSnapshot>>>, AppError> {
    let items = state
        .service
        .list_sessions(user.id.as_str(), 100)
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(items)))
}

async fn get_session(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<MeetingSessionSnapshot>>, AppError> {
    let snap = require_owned(&state.service, user.id.as_str(), &id).await?;
    Ok(Json(ApiResponse::ok(snap)))
}

async fn start_session(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<MeetingSessionSnapshot>>, AppError> {
    let session = require_owned(&state.service, user.id.as_str(), &id).await?;
    let snap = state
        .runtime
        .start(session)
        .await
        .map_err(|e| AppError::BadRequest(e))?;
    Ok(Json(ApiResponse::ok(snap)))
}

async fn pause_session(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<MeetingSessionSnapshot>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let snap = state
        .runtime
        .pause(&id)
        .await
        .map_err(|e| AppError::BadRequest(e))?;
    Ok(Json(ApiResponse::ok(snap)))
}

async fn resume_session(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<MeetingSessionSnapshot>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let snap = state
        .runtime
        .resume(&id)
        .await
        .map_err(|e| AppError::BadRequest(e))?;
    Ok(Json(ApiResponse::ok(snap)))
}

async fn stop_session(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<MeetingSessionSnapshot>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let snap = state
        .runtime
        .stop(&id)
        .await
        .map_err(map_service_err)?;
    // N3: auto-generate notes after stop (best-effort; explicit regenerate also available).
    let service = state.service.clone();
    let session_id = id.clone();
    tokio::spawn(async move {
        if let Err(e) = service.generate_notes(&session_id).await {
            tracing::warn!(error = %e, session_id, "auto meeting notes generation failed");
        }
    });
    Ok(Json(ApiResponse::ok(snap)))
}

async fn get_notes(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<MeetingNotesView>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let view = state
        .service
        .get_notes(&id)
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(view)))
}

#[derive(Debug, Serialize)]
struct GenerateNotesDto {
    session: MeetingSessionSnapshot,
    #[serde(flatten)]
    result: GenerateMeetingNotesResult,
}

async fn generate_notes(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<GenerateNotesDto>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let (session, result) = state
        .service
        .generate_notes(&id)
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(GenerateNotesDto { session, result })))
}

async fn bind_conversation(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    body: Result<Json<BindBody>, JsonRejection>,
) -> Result<Json<ApiResponse<MeetingSessionSnapshot>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let snap = state
        .service
        .bind_conversation(&id, req.conversation_id)
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(snap)))
}

async fn list_segments(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<MeetingSegmentSnapshot>>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let items = state
        .service
        .list_segments(&id)
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(items)))
}

async fn search_segments(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<ApiResponse<Vec<MeetingSegmentSnapshot>>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let items = state
        .service
        .search_segments(&id, &query.q, query.limit.max(1).min(200))
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(items)))
}

async fn edit_segment(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path((id, segment_id)): Path<(String, String)>,
    body: Result<Json<EditSegmentBody>, JsonRejection>,
) -> Result<Json<ApiResponse<MeetingSegmentSnapshot>>, AppError> {
    let _ = require_owned(&state.service, user.id.as_str(), &id).await?;
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    let mut segments = state
        .service
        .list_segments(&id)
        .await
        .map_err(map_service_err)?;
    let Some(mut seg) = segments
        .iter_mut()
        .find(|s| s.segment_id == segment_id)
        .map(|s| s.clone())
    else {
        return Err(AppError::NotFound("meeting_segment".into()));
    };
    seg.text = req.text;
    seg.is_manual_edit = true;
    seg.is_partial = false;
    let snap = state
        .service
        .upsert_segment(seg)
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(snap)))
}

async fn list_devices(
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<DeviceDto>>>, AppError> {
    let mgr = AudioDeviceManager::new();
    let mut out = Vec::new();
    for (kind, label) in [(DeviceKind::Input, "input"), (DeviceKind::Output, "output")] {
        let devices = mgr
            .list_devices(kind)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        for d in devices {
            out.push(DeviceDto {
                id: d.id,
                name: d.name,
                kind: label.to_string(),
                is_default: d.is_default,
            });
        }
    }
    Ok(Json(ApiResponse::ok(out)))
}

async fn detected_apps(
    Extension(_user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<DetectedAppsDto>>, AppError> {
    Ok(Json(ApiResponse::ok(DetectedAppsDto {
        app: detect_meeting_process(),
    })))
}

async fn list_voiceprints(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<VoiceprintDto>>>, AppError> {
    let rows = state
        .voiceprints
        .list(user.id.as_str())
        .await
        .map_err(map_service_err)?;
    let items = rows
        .into_iter()
        .map(|row| {
            let dims = embedding_from_blob(&row.embedding_blob)
                .map(|e| e.len())
                .unwrap_or(0);
            VoiceprintDto {
                voiceprint_id: row.voiceprint_id,
                display_name: row.display_name,
                embedding_dims: dims,
                created_at_ms: row.created_at,
                updated_at_ms: row.updated_at,
            }
        })
        .collect();
    Ok(Json(ApiResponse::ok(items)))
}

async fn enroll_voiceprint(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    body: Result<Json<EnrollVoiceprintBody>, JsonRejection>,
) -> Result<Json<ApiResponse<VoiceprintDto>>, AppError> {
    let Json(req) = body.map_err(|e| AppError::BadRequest(e.to_string()))?;
    if req.display_name.trim().is_empty() {
        return Err(AppError::BadRequest("display_name required".into()));
    }
    let row = if let Some(embedding) = req.embedding {
        state
            .voiceprints
            .enroll_embedding(user.id.as_str(), &req.display_name, &embedding)
            .await
            .map_err(|e| AppError::BadRequest(e))?
    } else if let Some(pcm) = req.pcm {
        let sample_rate = req.sample_rate.unwrap_or(16_000);
        state
            .voiceprints
            .enroll(user.id.as_str(), &req.display_name, &pcm, sample_rate)
            .await
            .map_err(|e| AppError::BadRequest(e))?
    } else {
        // Placeholder embedding so the wizard can enroll a name before capture.
        let placeholder = vec![0.0f32; 16];
        state
            .voiceprints
            .enroll_embedding(user.id.as_str(), &req.display_name, &placeholder)
            .await
            .map_err(|e| AppError::BadRequest(e))?
    };
    let dims = embedding_from_blob(&row.embedding_blob)
        .map(|e| e.len())
        .unwrap_or(0);
    Ok(Json(ApiResponse::ok(VoiceprintDto {
        voiceprint_id: row.voiceprint_id,
        display_name: row.display_name,
        embedding_dims: dims,
        created_at_ms: row.created_at,
        updated_at_ms: row.updated_at,
    })))
}

async fn delete_voiceprint(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(voiceprint_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let deleted = state
        .voiceprints
        .delete(user.id.as_str(), &voiceprint_id)
        .await
        .map_err(map_service_err)?;
    if !deleted {
        return Err(AppError::NotFound("voiceprint".into()));
    }
    Ok(Json(ApiResponse::ok(())))
}

async fn clear_voiceprints(
    State(state): State<MeetingRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<u64>>, AppError> {
    let n = state
        .voiceprints
        .clear(user.id.as_str())
        .await
        .map_err(map_service_err)?;
    Ok(Json(ApiResponse::ok(n)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use nomifun_common::UserId;
    use nomifun_db::{
        DbError, IMeetingRepository, InsertMeetingSessionParams, MeetingSegmentRow,
        MeetingSessionRow, MeetingSpeakerRow, MeetingVoiceprintRow, UpdateMeetingSessionParams,
        UpsertMeetingSegmentParams, UpsertMeetingSpeakerParams, UpsertMeetingVoiceprintParams,
    };
    use std::sync::Mutex;
    use tower::ServiceExt;

    struct MemRepo {
        sessions: Mutex<Vec<MeetingSessionRow>>,
        voiceprints: Mutex<Vec<MeetingVoiceprintRow>>,
        next_id: Mutex<i64>,
    }

    impl MemRepo {
        fn new() -> Self {
            Self {
                sessions: Mutex::new(Vec::new()),
                voiceprints: Mutex::new(Vec::new()),
                next_id: Mutex::new(1),
            }
        }
    }

    #[async_trait]
    impl IMeetingRepository for MemRepo {
        async fn insert_session(
            &self,
            params: &InsertMeetingSessionParams,
        ) -> Result<MeetingSessionRow, DbError> {
            let mut id = self.next_id.lock().unwrap();
            let row = MeetingSessionRow {
                id: *id,
                session_id: params.session_id.clone(),
                user_id: params.user_id.clone(),
                title: params.title.clone(),
                status: params.status.clone(),
                bound_conversation_id: params.bound_conversation_id.clone(),
                data_dir: params.data_dir.clone(),
                mic_available: if params.mic_available { 1 } else { 0 },
                loopback_available: if params.loopback_available { 1 } else { 0 },
                stt_backend: params.stt_backend.clone(),
                started_at: params.started_at,
                ended_at: params.ended_at,
                notes_json: None,
                notes_status: "none".into(),
                created_at: params.created_at,
                updated_at: params.updated_at,
            };
            *id += 1;
            self.sessions.lock().unwrap().push(row.clone());
            Ok(row)
        }
        async fn update_session(
            &self,
            session_id: &str,
            params: &UpdateMeetingSessionParams,
        ) -> Result<Option<MeetingSessionRow>, DbError> {
            let mut sessions = self.sessions.lock().unwrap();
            let Some(row) = sessions.iter_mut().find(|s| s.session_id == session_id) else {
                return Ok(None);
            };
            if let Some(status) = &params.status {
                row.status = status.clone();
            }
            if let Some(v) = params.mic_available {
                row.mic_available = if v { 1 } else { 0 };
            }
            if let Some(v) = params.loopback_available {
                row.loopback_available = if v { 1 } else { 0 };
            }
            if let Some(bound) = &params.bound_conversation_id {
                row.bound_conversation_id = bound.clone();
            }
            if let Some(notes_json) = &params.notes_json {
                row.notes_json = notes_json.clone();
            }
            if let Some(notes_status) = &params.notes_status {
                row.notes_status = notes_status.clone();
            }
            row.updated_at = params.updated_at;
            Ok(Some(row.clone()))
        }
        async fn get_session(
            &self,
            session_id: &str,
        ) -> Result<Option<MeetingSessionRow>, DbError> {
            Ok(self
                .sessions
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.session_id == session_id)
                .cloned())
        }
        async fn list_sessions_for_owner(
            &self,
            user_id: &str,
            _limit: i64,
        ) -> Result<Vec<MeetingSessionRow>, DbError> {
            Ok(self
                .sessions
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn upsert_segment(
            &self,
            _params: &UpsertMeetingSegmentParams,
        ) -> Result<MeetingSegmentRow, DbError> {
            unimplemented!()
        }
        async fn list_segments(
            &self,
            _session_id: &str,
        ) -> Result<Vec<MeetingSegmentRow>, DbError> {
            Ok(vec![])
        }
        async fn search_segments(
            &self,
            _session_id: &str,
            _query: &str,
            _limit: i64,
        ) -> Result<Vec<MeetingSegmentRow>, DbError> {
            Ok(vec![])
        }
        async fn upsert_speaker(
            &self,
            _params: &UpsertMeetingSpeakerParams,
        ) -> Result<MeetingSpeakerRow, DbError> {
            unimplemented!()
        }
        async fn list_speakers(
            &self,
            _session_id: &str,
        ) -> Result<Vec<MeetingSpeakerRow>, DbError> {
            Ok(vec![])
        }
        async fn upsert_voiceprint(
            &self,
            params: &UpsertMeetingVoiceprintParams,
        ) -> Result<MeetingVoiceprintRow, DbError> {
            let mut id = self.next_id.lock().unwrap();
            let row = MeetingVoiceprintRow {
                id: *id,
                voiceprint_id: params.voiceprint_id.clone(),
                user_id: params.user_id.clone(),
                display_name: params.display_name.clone(),
                embedding_blob: params.embedding_blob.clone(),
                created_at: params.created_at,
                updated_at: params.updated_at,
            };
            *id += 1;
            self.voiceprints.lock().unwrap().push(row.clone());
            Ok(row)
        }
        async fn list_voiceprints(
            &self,
            user_id: &str,
        ) -> Result<Vec<MeetingVoiceprintRow>, DbError> {
            Ok(self
                .voiceprints
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn delete_voiceprint(
            &self,
            user_id: &str,
            voiceprint_id: &str,
        ) -> Result<bool, DbError> {
            let mut vps = self.voiceprints.lock().unwrap();
            let before = vps.len();
            vps.retain(|r| !(r.user_id == user_id && r.voiceprint_id == voiceprint_id));
            Ok(vps.len() != before)
        }
        async fn clear_voiceprints(&self, user_id: &str) -> Result<u64, DbError> {
            let mut vps = self.voiceprints.lock().unwrap();
            let before = vps.len();
            vps.retain(|r| r.user_id != user_id);
            Ok((before - vps.len()) as u64)
        }
    }

    fn test_user() -> CurrentUser {
        CurrentUser {
            id: UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
            username: "owner".into(),
        }
    }

    fn test_router(tmp: &std::path::Path) -> Router {
        let repo = Arc::new(MemRepo::new()) as Arc<dyn IMeetingRepository>;
        let service = MeetingSessionService::new(repo.clone());
        let runtime = Arc::new(MeetingRuntime::new(service.clone()));
        let voiceprints = Arc::new(VoiceprintStore::new(
            FakeVoiceprintEncoder::new(16),
            repo,
        ));
        meeting_routes(MeetingRouterState {
            service,
            runtime,
            meetings_root: tmp.to_path_buf(),
            voiceprints,
        })
        .layer(Extension(test_user()))
    }

    #[tokio::test]
    async fn create_and_list_meeting_sessions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let router = test_router(tmp.path());

        let create = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/meetings")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"Daily"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::OK);

        let list = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/meetings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        assert_eq!(json["data"][0]["title"], "Daily");
    }

    #[tokio::test]
    async fn detected_apps_route_ok() {
        let tmp = tempfile::TempDir::new().unwrap();
        let router = test_router(tmp.path());
        let resp = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/meetings/detected-apps")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
