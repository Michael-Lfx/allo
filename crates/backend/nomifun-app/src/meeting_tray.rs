//! Desktop tray / global-shortcut control surface for MeetingSession (Y1).
//!
//! Close-to-tray must not stop capture: these helpers talk to the in-process
//! [`MeetingRuntime`] without touching the webview lifecycle.

use std::path::PathBuf;
use std::sync::Arc;

use nomifun_audio::{
    CreateMeetingSessionRequest, MeetingRuntime, MeetingSessionService, MeetingSessionStatus,
    SttBackendChoice, detect_meeting_process,
};
use serde::{Deserialize, Serialize};

use crate::desktop::DesktopServer;
use crate::services::AppServices;

/// Coarse phase for tray enablement / tooltip / pause-toggle shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingTrayPhase {
    Idle,
    Recording,
    Paused,
}

/// Snapshot used by the native tray UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingTrayStatus {
    pub phase: MeetingTrayPhase,
    pub session_id: Option<String>,
    pub title: Option<String>,
    /// Y1 tip-only: human-readable meeting app name when detected.
    pub detected_app: Option<String>,
    /// U4: latest caption line while recording (partial or final).
    pub latest_caption: Option<String>,
    pub latest_caption_partial: bool,
}

impl MeetingTrayStatus {
    pub fn idle(detected_app: Option<String>) -> Self {
        Self {
            phase: MeetingTrayPhase::Idle,
            session_id: None,
            title: None,
            detected_app,
            latest_caption: None,
            latest_caption_partial: false,
        }
    }

    pub fn tooltip(&self) -> String {
        let status = match self.phase {
            MeetingTrayPhase::Idle => None,
            MeetingTrayPhase::Recording => Some("Recording"),
            MeetingTrayPhase::Paused => Some("Paused"),
        };
        let base = match (status, self.detected_app.as_deref()) {
            (None, None) => "Flowy".to_string(),
            (None, Some(app)) => format!("Flowy — {app} detected"),
            (Some(phase), None) => format!("Flowy — {phase}"),
            (Some(phase), Some(app)) => format!("Flowy — {phase} ({app})"),
        };
        match self.latest_caption.as_deref() {
            Some(line) if !line.trim().is_empty() && status.is_some() => {
                let clipped: String = line.chars().take(80).collect();
                format!("{base}: {clipped}")
            }
            _ => base,
        }
    }
}

struct MeetingTrayCtx {
    service: MeetingSessionService,
    runtime: Arc<MeetingRuntime>,
    meetings_root: PathBuf,
    user_id: Arc<str>,
}

fn meeting_ctx(services: &AppServices) -> MeetingTrayCtx {
    MeetingTrayCtx {
        service: services.meeting_service.clone(),
        runtime: services.meeting_runtime.clone(),
        meetings_root: services.data_dir.join("meetings"),
        user_id: services.authoritative_user_id.clone(),
    }
}

impl DesktopServer {
    fn meeting_ctx(&self) -> Option<MeetingTrayCtx> {
        self.app_services().map(meeting_ctx)
    }

    /// Current tray-facing meeting status (live map preferred; DB fallback).
    pub async fn meeting_tray_status(&self) -> MeetingTrayStatus {
        let detected_app = detect_meeting_process();
        let Some(ctx) = self.meeting_ctx() else {
            return MeetingTrayStatus::idle(detected_app);
        };
        resolve_status(&ctx, detected_app).await
    }

    /// Create a session if needed and start capture.
    pub async fn meeting_tray_start(&self) -> Result<MeetingTrayStatus, String> {
        let ctx = self
            .meeting_ctx()
            .ok_or_else(|| "meeting services unavailable".to_string())?;
        let detected_app = detect_meeting_process();
        let status = resolve_status(&ctx, detected_app.clone()).await;
        if matches!(
            status.phase,
            MeetingTrayPhase::Recording | MeetingTrayPhase::Paused
        ) {
            return Ok(status);
        }

        let mgr = nomifun_audio::AudioDeviceManager::new();
        let mic_available = mgr
            .list_devices(nomifun_audio::DeviceKind::Input)
            .map(|d| !d.is_empty())
            .unwrap_or(false);
        let loopback_available = mgr
            .list_devices(nomifun_audio::DeviceKind::Output)
            .map(|d| !d.is_empty())
            .unwrap_or(false);

        let session = ctx
            .service
            .create_session(CreateMeetingSessionRequest {
                user_id: ctx.user_id.as_ref().to_string(),
                title: "Meeting".to_string(),
                data_dir: ctx.meetings_root.to_string_lossy().into_owned(),
                bound_conversation_id: None,
                stt_backend: SttBackendChoice::Auto,
                mic_available,
                loopback_available,
            })
            .await?;
        ctx.runtime.start(session).await?;
        Ok(resolve_status(&ctx, detected_app).await)
    }

    pub async fn meeting_tray_pause(&self) -> Result<MeetingTrayStatus, String> {
        let ctx = self
            .meeting_ctx()
            .ok_or_else(|| "meeting services unavailable".to_string())?;
        let detected_app = detect_meeting_process();
        let status = resolve_status(&ctx, detected_app.clone()).await;
        let Some(session_id) = status.session_id.as_deref() else {
            return Ok(status);
        };
        if status.phase != MeetingTrayPhase::Recording {
            return Ok(status);
        }
        ctx.runtime.pause(session_id).await?;
        Ok(resolve_status(&ctx, detected_app).await)
    }

    pub async fn meeting_tray_resume(&self) -> Result<MeetingTrayStatus, String> {
        let ctx = self
            .meeting_ctx()
            .ok_or_else(|| "meeting services unavailable".to_string())?;
        let detected_app = detect_meeting_process();
        let status = resolve_status(&ctx, detected_app.clone()).await;
        let Some(session_id) = status.session_id.as_deref() else {
            return Ok(status);
        };
        if status.phase != MeetingTrayPhase::Paused {
            return Ok(status);
        }
        ctx.runtime.resume(session_id).await?;
        Ok(resolve_status(&ctx, detected_app).await)
    }

    /// Pause when recording; resume when paused (global shortcut Ctrl/Cmd+Shift+P).
    pub async fn meeting_tray_toggle_pause(&self) -> Result<MeetingTrayStatus, String> {
        let status = self.meeting_tray_status().await;
        match status.phase {
            MeetingTrayPhase::Recording => self.meeting_tray_pause().await,
            MeetingTrayPhase::Paused => self.meeting_tray_resume().await,
            MeetingTrayPhase::Idle => Ok(status),
        }
    }

    pub async fn meeting_tray_stop(&self) -> Result<MeetingTrayStatus, String> {
        let ctx = self
            .meeting_ctx()
            .ok_or_else(|| "meeting services unavailable".to_string())?;
        let detected_app = detect_meeting_process();
        let status = resolve_status(&ctx, detected_app.clone()).await;
        let Some(session_id) = status.session_id.as_deref() else {
            return Ok(status);
        };
        if matches!(status.phase, MeetingTrayPhase::Idle) {
            return Ok(status);
        }
        ctx.runtime.stop(session_id).await?;
        Ok(resolve_status(&ctx, detected_app).await)
    }
}

async fn resolve_status(ctx: &MeetingTrayCtx, detected_app: Option<String>) -> MeetingTrayStatus {
    if let Some(session_id) = ctx.runtime.first_live_session_id() {
        let paused = ctx.runtime.is_live_paused(&session_id).unwrap_or(false);
        let title = ctx
            .service
            .get_session(&session_id)
            .await
            .ok()
            .flatten()
            .map(|s| s.title);
        let latest = ctx.service.latest_caption(&session_id);
        let latest_caption_partial = latest.as_ref().map(|c| c.is_partial).unwrap_or(false);
        let latest_caption = latest.map(|c| {
            if c.speaker_label.trim().is_empty() {
                c.text
            } else {
                format!("{}: {}", c.speaker_label, c.text)
            }
        });
        return MeetingTrayStatus {
            phase: if paused {
                MeetingTrayPhase::Paused
            } else {
                MeetingTrayPhase::Recording
            },
            session_id: Some(session_id),
            title,
            detected_app,
            latest_caption,
            latest_caption_partial,
        };
    }

    // DB fallback when live map is empty but a row still says recording/paused.
    match ctx.service.list_sessions(ctx.user_id.as_ref(), 20).await {
        Ok(items) => {
            if let Some(snap) = items.into_iter().find(|s| {
                matches!(
                    s.status,
                    MeetingSessionStatus::Recording | MeetingSessionStatus::Paused
                )
            }) {
                let latest = ctx.service.latest_caption(&snap.session_id);
                let latest_caption_partial =
                    latest.as_ref().map(|c| c.is_partial).unwrap_or(false);
                let latest_caption = latest.map(|c| {
                    if c.speaker_label.trim().is_empty() {
                        c.text
                    } else {
                        format!("{}: {}", c.speaker_label, c.text)
                    }
                });
                return MeetingTrayStatus {
                    phase: match snap.status {
                        MeetingSessionStatus::Paused => MeetingTrayPhase::Paused,
                        _ => MeetingTrayPhase::Recording,
                    },
                    session_id: Some(snap.session_id),
                    title: Some(snap.title),
                    detected_app,
                    latest_caption,
                    latest_caption_partial,
                };
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "meeting tray: list_sessions failed");
        }
    }

    MeetingTrayStatus::idle(detected_app)
}
