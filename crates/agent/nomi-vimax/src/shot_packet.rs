//! Shot production packet: the director-facing source of truth for one clip.
//!
//! Planning summaries (`storyboard.json`) stay for the filmstrip caption.
//! Render reads this packet, compiles Seedance R2V JSON, and never treats
//! a PUT here as a storyboard revision (no clip invalidation).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::domain::{ShotBeat, ShotBriefBeat, ShotBriefDescription, ShotDescription};
use crate::error::{VimaxError, VimaxResult};
use crate::media_local;
use crate::progress::PendingShotReview;
use crate::session::{read_json_artifact, write_json_artifact};

pub const PACKET_FILENAME: &str = "shot_packet.json";
pub const CURRENT_TAKE_FILENAME: &str = "current.json";
pub const TAKE_PREFIX: &str = "v";
pub const USER_REFS_DIR: &str = "user_refs";
pub const FILMS_DIR: &str = "films";
pub const FINAL_VIDEO_FILENAME: &str = "final_video.mp4";
pub const FILM_COVER_FILENAME: &str = "cover.png";
pub const MAX_SHOT_IMAGE_REFS: usize = 9;
pub const MAX_SHOT_AUDIO_REFS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    #[default]
    Continuous,
    ShotReview,
}

impl RenderMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "shot_review" | "review" => Self::ShotReview,
            _ => Self::Continuous,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::ShotReview => "shot_review",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotReviewDecision {
    Approve,
    SwitchToContinuous,
    Stop,
}

#[derive(Debug)]
pub struct ShotReviewBridge {
    notify: Notify,
    decision: StdMutex<Option<ShotReviewDecision>>,
    pub pending: StdMutex<Option<PendingShotReview>>,
    pub render_mode: StdMutex<RenderMode>,
}

impl ShotReviewBridge {
    pub fn new(mode: RenderMode) -> Self {
        Self {
            notify: Notify::new(),
            decision: StdMutex::new(None),
            pending: StdMutex::new(None),
            render_mode: StdMutex::new(mode),
        }
    }

    pub fn mode(&self) -> RenderMode {
        *self.render_mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_mode(&self, mode: RenderMode) {
        *self.render_mode.lock().unwrap_or_else(|e| e.into_inner()) = mode;
    }

    pub fn set_pending(&self, pending: Option<PendingShotReview>) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = pending;
    }

    pub fn pending(&self) -> Option<PendingShotReview> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn submit(&self, decision: ShotReviewDecision) {
        *self.decision.lock().unwrap_or_else(|e| e.into_inner()) = Some(decision);
        self.notify.notify_waiters();
    }

    pub async fn wait(&self, cancel: &CancellationToken) -> VimaxResult<ShotReviewDecision> {
        loop {
            if cancel.is_cancelled() {
                return Err(VimaxError::Cancelled);
            }
            if let Some(decision) = self
                .decision
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                return Ok(decision);
            }
            tokio::select! {
                _ = self.notify.notified() => {}
                _ = cancel.cancelled() => return Err(VimaxError::Cancelled),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShotRunState {
    #[default]
    Planned,
    AwaitingReview,
    Generating,
    Ready,
    ScriptStale,
    ContinuityStale,
    Failed,
    Skipped,
}

impl ShotRunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::AwaitingReview => "awaiting_review",
            Self::Generating => "generating",
            Self::Ready => "ready",
            Self::ScriptStale => "script_stale",
            Self::ContinuityStale => "continuity_stale",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotRefRole {
    ContinuityLastFrame,
    Portrait,
    Environment,
    Prop,
    Custom,
}

impl ShotRefRole {
    pub fn from_path(path: &Path, label: &str) -> Self {
        let blob = format!("{} {}", path.to_string_lossy(), label).to_ascii_lowercase();
        if blob.contains("video_last_frame")
            || blob.contains("last_frame")
            || blob.contains("continuity")
        {
            Self::ContinuityLastFrame
        } else if blob.contains("character_portrait")
            || blob.contains("three_view")
            || blob.contains("cameo")
        {
            Self::Portrait
        } else if blob.contains("environment") || blob.contains("empty environment") {
            Self::Environment
        } else if blob.contains("prop") {
            Self::Prop
        } else {
            Self::Custom
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShotPacketBeat {
    #[serde(default)]
    pub visual_desc: String,
    #[serde(default)]
    pub audio_desc: String,
    #[serde(default)]
    pub cam_idx: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotRefSlot {
    pub slot: u32,
    pub role: ShotRefRole,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Filled on GET only — never persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub unbound: bool,
    #[serde(default)]
    pub user_override: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotPacket {
    #[serde(default = "packet_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub scene_root: String,
    pub shot_idx: i32,
    #[serde(default)]
    pub location_id: String,
    #[serde(default)]
    pub cam_idx: i32,
    #[serde(default)]
    pub visual_desc: String,
    #[serde(default)]
    pub audio_desc: String,
    #[serde(default)]
    pub beats: Vec<ShotPacketBeat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u32>,
    #[serde(default)]
    pub seam: String,
    #[serde(default)]
    pub image_refs: Vec<ShotRefSlot>,
    #[serde(default)]
    pub audio_refs: Vec<ShotRefSlot>,
    #[serde(default)]
    pub compiled_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_override: Option<String>,
    #[serde(default)]
    pub run_state: ShotRunState,
    #[serde(default)]
    pub user_edited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_take: Option<u32>,
    #[serde(default)]
    pub take_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<ShotGraphLayout>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ShotGraphLayout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ShotGraphViewport>,
    #[serde(default)]
    pub nodes: BTreeMap<String, ShotGraphPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShotGraphViewport {
    pub x: f64,
    pub y: f64,
    pub k: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShotGraphPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

fn packet_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShotPacketPatch {
    #[serde(default)]
    pub visual_desc: Option<String>,
    #[serde(default)]
    pub audio_desc: Option<String>,
    #[serde(default)]
    pub beats: Option<Vec<ShotPacketBeat>>,
    #[serde(default)]
    pub duration_secs: Option<u32>,
    #[serde(default)]
    pub prompt_override: Option<Option<String>>,
    #[serde(default)]
    pub recompile: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<ShotGraphLayout>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PacketPatchOutcome {
    pub script_changed: bool,
    pub layout_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakePointer {
    pub take: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShotPacketView {
    #[serde(flatten)]
    pub packet: ShotPacket,
    #[serde(default)]
    pub takes: Vec<ShotTakeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotTakeInfo {
    pub take: u32,
    pub current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<i64>,
    #[serde(default)]
    pub video_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilmManifest {
    pub version: u32,
    #[serde(default)]
    pub takes: Vec<FilmShotTake>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilmShotTake {
    pub scene_root: String,
    pub shot_idx: i32,
    pub take: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilmInfo {
    pub version: u32,
    pub current: bool,
    pub video_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CreditsFile {
    #[serde(default)]
    pub credits_consumed: i64,
    #[serde(default)]
    pub credits: i64,
}

pub fn packet_path(scene_dir: &Path, shot_idx: i32) -> PathBuf {
    shot_dir(scene_dir, shot_idx).join(PACKET_FILENAME)
}

pub fn shot_dir(scene_dir: &Path, shot_idx: i32) -> PathBuf {
    scene_dir.join("shots").join(shot_idx.to_string())
}

pub fn scene_root_rel(session_root: &Path, scene_dir: &Path) -> String {
    rel_to(session_root, scene_dir)
}

pub fn rel_to(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn packet_from_shot(scene_root: &str, shot: &ShotDescription) -> ShotPacket {
    let beats = if shot.beats.is_empty() {
        vec![ShotPacketBeat {
            visual_desc: shot.visual_desc.clone(),
            audio_desc: shot.audio_desc.clone().unwrap_or_default(),
            cam_idx: shot.cam_idx,
        }]
    } else {
        shot.beats
            .iter()
            .map(|beat| ShotPacketBeat {
                visual_desc: if beat.motion_desc.trim().is_empty() {
                    shot.visual_desc.clone()
                } else {
                    beat.motion_desc.clone()
                },
                audio_desc: beat.audio_desc.clone().unwrap_or_default(),
                cam_idx: beat.cam_idx.unwrap_or(shot.cam_idx),
            })
            .collect()
    };
    ShotPacket {
        schema_version: 1,
        scene_root: scene_root.to_string(),
        shot_idx: shot.idx,
        location_id: shot.location_id.clone(),
        cam_idx: shot.cam_idx,
        visual_desc: shot.visual_desc.clone(),
        audio_desc: shot.audio_desc.clone().unwrap_or_default(),
        beats,
        duration_secs: None,
        seam: String::new(),
        image_refs: Vec::new(),
        audio_refs: Vec::new(),
        compiled_prompt: String::new(),
        prompt_override: None,
        run_state: ShotRunState::Planned,
        user_edited: false,
        current_take: None,
        take_count: 0,
        error: None,
        layout: None,
    }
}

pub fn apply_packet_to_shot(shot: &mut ShotDescription, packet: &ShotPacket) {
    shot.visual_desc = packet.visual_desc.clone();
    shot.audio_desc = if packet.audio_desc.trim().is_empty() {
        None
    } else {
        Some(packet.audio_desc.clone())
    };
    shot.location_id = packet.location_id.clone();
    if packet.beats.is_empty() {
        shot.motion_desc = packet.visual_desc.clone();
        return;
    }
    shot.motion_desc = packet
        .beats
        .first()
        .map(|b| b.visual_desc.clone())
        .unwrap_or_else(|| packet.visual_desc.clone());
    if packet.beats.len() >= 2 {
        shot.beats = packet
            .beats
            .iter()
            .map(|beat| ShotBeat {
                motion_desc: beat.visual_desc.clone(),
                audio_desc: if beat.audio_desc.trim().is_empty() {
                    None
                } else {
                    Some(beat.audio_desc.clone())
                },
                cam_idx: Some(beat.cam_idx),
            })
            .collect();
    }
}

pub async fn load_packet(scene_dir: &Path, shot_idx: i32) -> VimaxResult<Option<ShotPacket>> {
    let path = packet_path(scene_dir, shot_idx);
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(read_json_artifact(&path).await?))
}

pub async fn save_packet(scene_dir: &Path, packet: &ShotPacket) -> VimaxResult<()> {
    let path = packet_path(scene_dir, packet.shot_idx);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut persist = packet.clone();
    for slot in persist.image_refs.iter_mut().chain(persist.audio_refs.iter_mut()) {
        slot.url = None;
    }
    write_json_artifact(&path, &persist).await
}

/// Create packets for newly planned clips. Never overwrite a user-edited packet's script.
pub async fn seed_packets_from_shots(
    scene_dir: &Path,
    session_root: &Path,
    shots: &[ShotDescription],
) -> VimaxResult<()> {
    let scene_root = scene_root_rel(session_root, scene_dir);
    for shot in shots {
        let path = packet_path(scene_dir, shot.idx);
        if path.is_file() {
            if let Ok(mut existing) = read_json_artifact::<ShotPacket>(&path).await {
                if existing.user_edited || existing.prompt_override.is_some() {
                    existing.scene_root = scene_root.clone();
                    refresh_run_state_from_disk(scene_dir, &mut existing);
                    let _ = save_packet(scene_dir, &existing).await;
                    continue;
                }
            }
        }
        let mut packet = packet_from_shot(&scene_root, shot);
        refresh_run_state_from_disk(scene_dir, &mut packet);
        save_packet(scene_dir, &packet).await?;
    }
    Ok(())
}

pub fn refresh_run_state_from_disk(scene_dir: &Path, packet: &mut ShotPacket) {
    let dir = shot_dir(scene_dir, packet.shot_idx);
    let video = dir.join("video.mp4");
    let has_video = media_local::is_usable_video_file(&video);
    if matches!(packet.run_state, ShotRunState::Failed | ShotRunState::Skipped) && !has_video {
        return;
    }
    if has_video {
        if packet.run_state == ShotRunState::ScriptStale
            || packet.run_state == ShotRunState::ContinuityStale
        {
            return;
        }
        packet.run_state = ShotRunState::Ready;
        packet.error = None;
    } else if !matches!(
        packet.run_state,
        ShotRunState::AwaitingReview | ShotRunState::Generating | ShotRunState::Failed
    ) {
        packet.run_state = ShotRunState::Planned;
    }
    if let Some(current) = read_current_take_sync(&dir) {
        packet.current_take = Some(current);
    }
    packet.take_count = count_takes_sync(&dir);
}

pub fn apply_patch(packet: &mut ShotPacket, patch: &ShotPacketPatch) -> PacketPatchOutcome {
    let mut script_changed = false;
    let mut layout_changed = false;
    if let Some(visual) = &patch.visual_desc {
        if packet.visual_desc != *visual {
            packet.visual_desc = visual.clone();
            script_changed = true;
        }
    }
    if let Some(audio) = &patch.audio_desc {
        if packet.audio_desc != *audio {
            packet.audio_desc = audio.clone();
            script_changed = true;
        }
    }
    if let Some(beats) = &patch.beats {
        if packet.beats != *beats {
            packet.beats = beats.clone();
            if let Some(first) = beats.first() {
                packet.visual_desc = first.visual_desc.clone();
                packet.audio_desc = first.audio_desc.clone();
                if beats.len() > 1 {
                    packet.visual_desc = beats
                        .iter()
                        .map(|b| b.visual_desc.as_str())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" / ");
                    packet.audio_desc = beats
                        .iter()
                        .map(|b| b.audio_desc.as_str())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" / ");
                }
            }
            script_changed = true;
        }
    }
    if let Some(secs) = patch.duration_secs {
        if packet.duration_secs != Some(secs) {
            packet.duration_secs = Some(secs);
            script_changed = true;
        }
    }
    if let Some(override_opt) = &patch.prompt_override {
        if packet.prompt_override != *override_opt {
            packet.prompt_override = override_opt.clone();
            script_changed = true;
        }
    }
    if let Some(layout) = &patch.layout {
        if packet.layout.as_ref() != Some(layout) {
            packet.layout = Some(layout.clone());
            layout_changed = true;
        }
    }
    if script_changed {
        packet.user_edited = true;
    }
    PacketPatchOutcome {
        script_changed,
        layout_changed,
    }
}

pub fn next_ref_slot(slots: &[ShotRefSlot]) -> u32 {
    slots.iter().map(|slot| slot.slot).max().unwrap_or(0) + 1
}

pub fn ensure_ref_slot(
    slots: &mut Vec<ShotRefSlot>,
    slot: u32,
    kind_audio: bool,
) -> VimaxResult<usize> {
    if let Some(index) = slots.iter().position(|item| item.slot == slot) {
        return Ok(index);
    }
    let cap = if kind_audio {
        MAX_SHOT_AUDIO_REFS
    } else {
        MAX_SHOT_IMAGE_REFS
    };
    if slots.len() >= cap {
        return Err(VimaxError::InvalidParams(format!(
            "this shot already has {cap} {} refs",
            if kind_audio { "audio" } else { "image" }
        )));
    }
    slots.push(ShotRefSlot {
        slot,
        role: ShotRefRole::Custom,
        label: if kind_audio {
            "voice".into()
        } else {
            "ref".into()
        },
        character_id: None,
        path: None,
        url: None,
        unbound: true,
        user_override: true,
    });
    Ok(slots.len() - 1)
}

pub fn can_drop_ref_slot(slot: &ShotRefSlot) -> bool {
    slot.role == ShotRefRole::Custom && slot.character_id.is_none()
}

pub fn slots_from_image_pairs(
    session_root: &Path,
    pairs: &[(PathBuf, String)],
) -> Vec<ShotRefSlot> {
    pairs
        .iter()
        .enumerate()
        .map(|(i, (path, label))| ShotRefSlot {
            slot: (i as u32) + 1,
            role: ShotRefRole::from_path(path, label),
            label: label.clone(),
            character_id: None,
            path: media_local::is_usable_image_file(path).then(|| rel_to(session_root, path)),
            url: None,
            unbound: false,
            user_override: false,
        })
        .collect()
}

pub fn slots_from_audio_pairs(
    session_root: &Path,
    pairs: &[(String, PathBuf)],
) -> Vec<ShotRefSlot> {
    pairs
        .iter()
        .enumerate()
        .map(|(i, (name, path))| ShotRefSlot {
            slot: (i as u32) + 1,
            role: ShotRefRole::Custom,
            label: name.clone(),
            character_id: Some(name.clone()),
            path: path.is_file().then(|| rel_to(session_root, path)),
            url: None,
            unbound: false,
            user_override: false,
        })
        .collect()
}

/// Merge live assembled refs into a packet without clobbering user replacements.
pub fn merge_live_image_refs(packet: &mut ShotPacket, live: Vec<ShotRefSlot>) {
    if packet.image_refs.is_empty() || !packet.image_refs.iter().any(|s| s.user_override || s.unbound)
    {
        packet.image_refs = live;
        return;
    }
    let mut next = live;
    for slot in &packet.image_refs {
        if !(slot.user_override || slot.unbound) {
            continue;
        }
        if let Some(target) = next.iter_mut().find(|live| live.slot == slot.slot) {
            *target = slot.clone();
            continue;
        }
        if slot.role == ShotRefRole::ContinuityLastFrame {
            if let Some(target) = next
                .iter_mut()
                .find(|live| live.role == ShotRefRole::ContinuityLastFrame)
            {
                *target = slot.clone();
                continue;
            }
        }
        next.push(slot.clone());
    }
    packet.image_refs = next;
}

pub fn merge_live_audio_refs(packet: &mut ShotPacket, live: Vec<ShotRefSlot>) {
    if packet.audio_refs.is_empty() || !packet.audio_refs.iter().any(|s| s.user_override || s.unbound)
    {
        packet.audio_refs = live;
        return;
    }
    let mut next = live;
    for slot in &packet.audio_refs {
        if !(slot.user_override || slot.unbound) {
            continue;
        }
        if let Some(target) = next.iter_mut().find(|live| {
            live.slot == slot.slot || live.character_id == slot.character_id
        }) {
            *target = slot.clone();
        } else {
            next.push(slot.clone());
        }
    }
    packet.audio_refs = next;
}

pub fn bind_prompt_with_override(compiled: &str, override_body: Option<&str>) -> String {
    let Some(body) = override_body.map(str::trim).filter(|s| !s.is_empty()) else {
        return compiled.to_string();
    };
    let header = locked_prompt_header(compiled);
    if header.is_empty() {
        return body.to_string();
    }
    format!("{header}\n{body}")
}

fn locked_prompt_header(compiled: &str) -> String {
    let mut lines = Vec::new();
    for line in compiled.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("画面") || trimmed.starts_with("Scene:") {
            break;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_string()
}

pub fn usable_image_slots<'a>(packet: &'a ShotPacket) -> Vec<&'a ShotRefSlot> {
    packet
        .image_refs
        .iter()
        .filter(|s| !s.unbound && s.path.as_deref().is_some_and(|p| !p.is_empty()))
        .collect()
}

pub fn usable_audio_slots<'a>(packet: &'a ShotPacket) -> Vec<&'a ShotRefSlot> {
    packet
        .audio_refs
        .iter()
        .filter(|s| !s.unbound && s.path.as_deref().is_some_and(|p| !p.is_empty()))
        .collect()
}

pub fn continuity_unbound(packet: &ShotPacket) -> bool {
    packet
        .image_refs
        .iter()
        .any(|s| s.role == ShotRefRole::ContinuityLastFrame && s.unbound)
}

pub async fn sync_storyboard_row(
    scene_dir: &Path,
    packet: &ShotPacket,
) -> VimaxResult<()> {
    let path = scene_dir.join("storyboard.json");
    if !path.is_file() {
        return Ok(());
    }
    let mut board: Vec<ShotBriefDescription> = read_json_artifact(&path).await?;
    if let Some(row) = board.iter_mut().find(|row| row.idx == packet.shot_idx) {
        row.visual_desc = packet.visual_desc.clone();
        row.audio_desc = if packet.audio_desc.trim().is_empty() {
            None
        } else {
            Some(packet.audio_desc.clone())
        };
        if packet.beats.len() >= 2 {
            row.beats = packet
                .beats
                .iter()
                .map(|beat| ShotBriefBeat {
                    visual_desc: beat.visual_desc.clone(),
                    audio_desc: if beat.audio_desc.trim().is_empty() {
                        None
                    } else {
                        Some(beat.audio_desc.clone())
                    },
                    cam_idx: beat.cam_idx,
                })
                .collect();
        }
        write_json_artifact(&path, &board).await?;
    }
    let desc_path = shot_dir(scene_dir, packet.shot_idx).join("shot_description.json");
    if desc_path.is_file() {
        if let Ok(mut shot) = read_json_artifact::<ShotDescription>(&desc_path).await {
            apply_packet_to_shot(&mut shot, packet);
            let _ = write_json_artifact(&desc_path, &shot).await;
        }
    }
    let aggregate = scene_dir.join("shot_descriptions.json");
    if aggregate.is_file() {
        if let Ok(mut shots) = read_json_artifact::<Vec<ShotDescription>>(&aggregate).await {
            if let Some(shot) = shots.iter_mut().find(|s| s.idx == packet.shot_idx) {
                apply_packet_to_shot(shot, packet);
                let _ = write_json_artifact(&aggregate, &shots).await;
            }
        }
    }
    Ok(())
}

pub fn resolve_slot_paths(
    session_root: &Path,
    slots: &[ShotRefSlot],
) -> Vec<(PathBuf, String)> {
    slots
        .iter()
        .filter(|s| !s.unbound)
        .filter_map(|s| {
            let rel = s.path.as_deref()?.trim();
            if rel.is_empty() {
                return None;
            }
            let abs = session_root.join(rel.replace('\\', "/"));
            Some((abs, s.label.clone()))
        })
        .collect()
}

pub fn resolve_audio_slot_paths(
    session_root: &Path,
    slots: &[ShotRefSlot],
) -> Vec<(String, PathBuf)> {
    slots
        .iter()
        .filter(|s| !s.unbound)
        .filter_map(|s| {
            let rel = s.path.as_deref()?.trim();
            if rel.is_empty() {
                return None;
            }
            let abs = session_root.join(rel.replace('\\', "/"));
            let name = s
                .character_id
                .clone()
                .unwrap_or_else(|| s.label.clone());
            Some((name, abs))
        })
        .collect()
}

fn takes_dir(shot_dir: &Path) -> PathBuf {
    shot_dir.join("takes")
}

fn read_current_take_sync(shot_dir: &Path) -> Option<u32> {
    let path = takes_dir(shot_dir).join(CURRENT_TAKE_FILENAME);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<TakePointer>(&text)
        .ok()
        .map(|p| p.take)
}

fn count_takes_sync(shot_dir: &Path) -> u32 {
    let dir = takes_dir(shot_dir);
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return 0,
    };
    rd.filter_map(|ent| {
        let name = ent.ok()?.file_name();
        let name = name.to_string_lossy();
        name.strip_prefix(TAKE_PREFIX)?.parse::<u32>().ok()
    })
    .max()
    .unwrap_or(0)
}

pub async fn next_take_id(shot_dir: &Path) -> u32 {
    count_takes_sync(shot_dir) + 1
}

pub async fn list_takes(session_root: &Path, scene_dir: &Path, shot_idx: i32) -> Vec<ShotTakeInfo> {
    let dir = shot_dir(scene_dir, shot_idx);
    let current = read_current_take_sync(&dir);
    let takes_root = takes_dir(&dir);
    let Ok(mut rd) = tokio::fs::read_dir(&takes_root).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    while let Ok(Some(ent)) = rd.next_entry().await {
        let name = ent.file_name();
        let Some(k) = name
            .to_str()
            .and_then(|s| s.strip_prefix(TAKE_PREFIX))
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let take_dir = ent.path();
        let video = take_dir.join("video.mp4");
        if !media_local::is_usable_video_file(&video) {
            continue;
        }
        let credits = read_take_credits(&take_dir).await;
        out.push(ShotTakeInfo {
            take: k,
            current: current == Some(k),
            credits,
            video_path: rel_to(session_root, &video),
        });
    }
    out.sort_by_key(|t| t.take);
    out
}

async fn read_take_credits(take_dir: &Path) -> Option<i64> {
    let path = take_dir.join("video_credits.json");
    let file: CreditsFile = read_json_artifact(&path).await.ok()?;
    let n = if file.credits_consumed > 0 {
        file.credits_consumed
    } else {
        file.credits
    };
    (n > 0).then_some(n)
}

/// Copy the live `video.mp4` (and companions) into `takes/v{k}` and point current at it.
pub async fn archive_live_take(
    session_root: &Path,
    scene_dir: &Path,
    packet: &mut ShotPacket,
) -> VimaxResult<u32> {
    let dir = shot_dir(scene_dir, packet.shot_idx);
    let video = dir.join("video.mp4");
    if !media_local::is_usable_video_file(&video) {
        return Err(VimaxError::InvalidParams(format!(
            "shot {} has no usable video to archive",
            packet.shot_idx
        )));
    }
    let k = next_take_id(&dir).await;
    let dest = takes_dir(&dir).join(format!("{TAKE_PREFIX}{k}"));
    tokio::fs::create_dir_all(&dest).await?;
    tokio::fs::copy(&video, dest.join("video.mp4")).await?;
    let last = dir.join("video_last_frame.png");
    if media_local::is_usable_image_file(&last) {
        let _ = tokio::fs::copy(&last, dest.join("video_last_frame.png")).await;
    }
    let credits = dir.join("video_credits.json");
    if credits.is_file() {
        let _ = tokio::fs::copy(&credits, dest.join("video_credits.json")).await;
    }
    let _ = write_json_artifact(&dest.join(PACKET_FILENAME), packet).await;
    write_json_artifact(&takes_dir(&dir).join(CURRENT_TAKE_FILENAME), &TakePointer { take: k })
        .await?;
    packet.current_take = Some(k);
    packet.take_count = k;
    let _ = session_root;
    Ok(k)
}

pub async fn promote_take(
    scene_dir: &Path,
    shot_idx: i32,
    take: u32,
) -> VimaxResult<()> {
    let dir = shot_dir(scene_dir, shot_idx);
    let src = takes_dir(&dir).join(format!("{TAKE_PREFIX}{take}"));
    let video = src.join("video.mp4");
    if !media_local::is_usable_video_file(&video) {
        return Err(VimaxError::InvalidParams(format!("take v{take} has no video")));
    }
    tokio::fs::copy(&video, dir.join("video.mp4")).await?;
    let last = src.join("video_last_frame.png");
    if last.is_file() {
        let _ = tokio::fs::copy(&last, dir.join("video_last_frame.png")).await;
    }
    let credits = src.join("video_credits.json");
    if credits.is_file() {
        let _ = tokio::fs::copy(&credits, dir.join("video_credits.json")).await;
    }
    write_json_artifact(&takes_dir(&dir).join(CURRENT_TAKE_FILENAME), &TakePointer { take })
        .await?;
    Ok(())
}

pub async fn mark_continuity_stale_after(
    scene_dir: &Path,
    after_idx: i32,
) -> VimaxResult<()> {
    let shots_root = scene_dir.join("shots");
    let Ok(mut rd) = tokio::fs::read_dir(&shots_root).await else {
        return Ok(());
    };
    while let Ok(Some(ent)) = rd.next_entry().await {
        let name = ent.file_name();
        let Ok(idx) = name.to_string_lossy().parse::<i32>() else {
            continue;
        };
        if idx <= after_idx {
            continue;
        }
        let packet_file = ent.path().join(PACKET_FILENAME);
        if !packet_file.is_file() {
            continue;
        }
        if let Ok(mut packet) = read_json_artifact::<ShotPacket>(&packet_file).await {
            if packet.run_state == ShotRunState::Ready
                || packet.run_state == ShotRunState::ScriptStale
            {
                packet.run_state = ShotRunState::ContinuityStale;
                let _ = save_packet(scene_dir, &packet).await;
            }
        }
    }
    Ok(())
}

pub async fn prepare_retake(
    session_root: &Path,
    scene_dir: &Path,
    shot_idx: i32,
    cascade: bool,
) -> VimaxResult<Vec<i32>> {
    let mut targets = vec![shot_idx];
    if cascade {
        let shots_root = scene_dir.join("shots");
        if let Ok(mut rd) = tokio::fs::read_dir(&shots_root).await {
            while let Ok(Some(ent)) = rd.next_entry().await {
                let Ok(idx) = ent.file_name().to_string_lossy().parse::<i32>() else {
                    continue;
                };
                if idx > shot_idx {
                    targets.push(idx);
                }
            }
        }
        targets.sort_unstable();
        targets.dedup();
    }
    for idx in &targets {
        let dir = shot_dir(scene_dir, *idx);
        let video = dir.join("video.mp4");
        if media_local::is_usable_video_file(&video) {
            if let Ok(mut packet) = load_packet(scene_dir, *idx)
                .await
                .map(|p| p.unwrap_or_else(|| ShotPacket {
                    schema_version: 1,
                    scene_root: scene_root_rel(session_root, scene_dir),
                    shot_idx: *idx,
                    location_id: String::new(),
                    cam_idx: 0,
                    visual_desc: String::new(),
                    audio_desc: String::new(),
                    beats: Vec::new(),
                    duration_secs: None,
                    seam: String::new(),
                    image_refs: Vec::new(),
                    audio_refs: Vec::new(),
                    compiled_prompt: String::new(),
                    prompt_override: None,
                    run_state: ShotRunState::Ready,
                    user_edited: false,
                    current_take: None,
                    take_count: 0,
                    error: None,
                    layout: None,
                }))
            {
                let _ = archive_live_take(session_root, scene_dir, &mut packet).await;
                packet.run_state = ShotRunState::Planned;
                let _ = save_packet(scene_dir, &packet).await;
            }
            let _ = tokio::fs::remove_file(&video).await;
            let _ = tokio::fs::remove_file(dir.join("video_last_frame.png")).await;
        }
    }
    mark_continuity_stale_after(scene_dir, shot_idx).await?;
    let scene_final = scene_dir.join("final_video.mp4");
    if media_local::is_usable_video_file(&scene_final) {
        let _ = tokio::fs::remove_file(&scene_final).await;
    }
    Ok(targets)
}

pub fn film_root_films_dir(film_root: &Path) -> PathBuf {
    film_root.join(FILMS_DIR)
}

pub async fn archive_current_film(
    session_root: &Path,
    film_root: &Path,
    shot_takes: Vec<FilmShotTake>,
) -> VimaxResult<u32> {
    let video = live_final_video_path(film_root);
    if !media_local::is_usable_video_file(&video) {
        return Err(VimaxError::InvalidParams("no film to archive".into()));
    }
    let films = film_root_films_dir(film_root);
    tokio::fs::create_dir_all(&films).await?;
    let mut k = 1u32;
    if let Ok(mut rd) = tokio::fs::read_dir(&films).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(n) = ent
                .file_name()
                .to_str()
                .and_then(|s| s.strip_prefix(TAKE_PREFIX))
                .and_then(|s| s.parse::<u32>().ok())
            {
                k = k.max(n + 1);
            }
        }
    }
    let dest = films.join(format!("{TAKE_PREFIX}{k}"));
    tokio::fs::create_dir_all(&dest).await?;
    tokio::fs::copy(&video, dest.join(FINAL_VIDEO_FILENAME)).await?;
    let cover = film_root.join(FILM_COVER_FILENAME);
    if cover.is_file() {
        let _ = tokio::fs::copy(&cover, dest.join(FILM_COVER_FILENAME)).await;
    }
    write_json_artifact(
        &dest.join("manifest.json"),
        &FilmManifest {
            version: k,
            takes: shot_takes,
        },
    )
    .await?;
    write_json_artifact(&films.join(CURRENT_TAKE_FILENAME), &TakePointer { take: k }).await?;
    let _ = session_root;
    Ok(k)
}

pub async fn list_films(session_root: &Path, film_root: &Path) -> Vec<FilmInfo> {
    let films = film_root_films_dir(film_root);
    let current = {
        let path = films.join(CURRENT_TAKE_FILENAME);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<TakePointer>(&t).ok())
            .map(|p| p.take)
    };
    let Ok(mut rd) = tokio::fs::read_dir(&films).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    while let Ok(Some(ent)) = rd.next_entry().await {
        let Some(k) = ent
            .file_name()
            .to_str()
            .and_then(|s| s.strip_prefix(TAKE_PREFIX))
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let video = ent.path().join(FINAL_VIDEO_FILENAME);
        if !media_local::is_usable_video_file(&video) {
            continue;
        }
        out.push(FilmInfo {
            version: k,
            current: current == Some(k),
            video_path: rel_to(session_root, &video),
        });
    }
    out.sort_by_key(|f| f.version);
    out
}

pub async fn promote_film(film_root: &Path, version: u32) -> VimaxResult<PathBuf> {
    let src = film_root_films_dir(film_root)
        .join(format!("{TAKE_PREFIX}{version}"))
        .join(FINAL_VIDEO_FILENAME);
    if !media_local::is_usable_video_file(&src) {
        return Err(VimaxError::InvalidParams(format!(
            "film v{version} is missing"
        )));
    }
    let dest = live_final_video_path(film_root);
    tokio::fs::copy(&src, &dest).await?;
    restore_film_cover_from(film_root, &src);
    write_json_artifact(
        &film_root_films_dir(film_root).join(CURRENT_TAKE_FILENAME),
        &TakePointer { take: version },
    )
    .await?;
    Ok(dest)
}

pub fn live_final_video_path(film_root: &Path) -> PathBuf {
    film_root.join(FINAL_VIDEO_FILENAME)
}

fn take_version_from_name(name: &std::ffi::OsStr) -> Option<u32> {
    name.to_str()?
        .strip_prefix(TAKE_PREFIX)?
        .parse::<u32>()
        .ok()
}

fn current_film_version_sync(film_root: &Path) -> Option<u32> {
    let path = film_root_films_dir(film_root).join(CURRENT_TAKE_FILENAME);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<TakePointer>(&t).ok())
        .map(|p| p.take)
}

/// Directory of the current (or latest usable) `films/vN` archive.
pub fn archived_film_dir_sync(film_root: &Path) -> Option<PathBuf> {
    let films = film_root_films_dir(film_root);
    let preferred = current_film_version_sync(film_root);
    if let Some(k) = preferred {
        let dir = films.join(format!("{TAKE_PREFIX}{k}"));
        if media_local::is_usable_video_file(&dir.join(FINAL_VIDEO_FILENAME)) {
            return Some(dir);
        }
    }
    let rd = std::fs::read_dir(&films).ok()?;
    let mut best: Option<(u32, PathBuf)> = None;
    for ent in rd.flatten() {
        let Some(k) = take_version_from_name(&ent.file_name()) else {
            continue;
        };
        let dir = ent.path();
        if !media_local::is_usable_video_file(&dir.join(FINAL_VIDEO_FILENAME)) {
            continue;
        }
        if best.as_ref().is_none_or(|(n, _)| k > *n) {
            best = Some((k, dir));
        }
    }
    best.map(|(_, dir)| dir)
}

fn restore_film_cover_from(film_root: &Path, archived_video: &Path) {
    let live = film_root.join(FILM_COVER_FILENAME);
    if media_local::is_usable_image_file(&live) {
        return;
    }
    let src = archived_video
        .parent()
        .map(|dir| dir.join(FILM_COVER_FILENAME))
        .filter(|p| p.is_file());
    if let Some(src) = src {
        let _ = std::fs::copy(&src, &live);
    }
}

/// Copy `films/vN/final_video.mp4` back to `{film_root}/final_video.mp4` when the
/// live cut is missing. Director-desk retakes archive then delete the live file;
/// TV import used to keep only the archive, so publish / the agent film bubble
/// looked at a path that no longer existed.
pub fn ensure_live_film_sync(film_root: &Path) -> Option<PathBuf> {
    let live = live_final_video_path(film_root);
    if media_local::is_usable_video_file(&live) {
        return Some(live);
    }
    let src_dir = archived_film_dir_sync(film_root)?;
    let src = src_dir.join(FINAL_VIDEO_FILENAME);
    if live.exists() {
        let _ = std::fs::remove_file(&live);
    }
    if let Some(parent) = live.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::copy(&src, &live).ok()?;
    restore_film_cover_from(film_root, &src);
    media_local::is_usable_video_file(&live).then_some(live)
}

pub async fn ensure_live_film(film_root: &Path) -> Option<PathBuf> {
    let live = live_final_video_path(film_root);
    if media_local::is_usable_video_file(&live) {
        return Some(live);
    }
    let src_dir = archived_film_dir_sync(film_root)?;
    let src = src_dir.join(FINAL_VIDEO_FILENAME);
    if live.exists() {
        let _ = tokio::fs::remove_file(&live).await;
    }
    if let Some(parent) = live.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::copy(&src, &live).await.ok()?;
    restore_film_cover_from(film_root, &src);
    media_local::is_usable_video_file(&live).then_some(live)
}

/// Restore `{artifact_root}/final_video.mp4` (and cover when missing) and return
/// session-relative paths. Used after TV import and on idle status / publish.
pub fn restore_session_live_film(
    working_root: &Path,
    artifact_root: &str,
) -> Option<(String, Option<String>)> {
    let film_root = working_root.join(artifact_root);
    let live = ensure_live_film_sync(&film_root)?;
    let video_rel = live
        .strip_prefix(working_root)
        .unwrap_or(&live)
        .to_string_lossy()
        .replace('\\', "/");
    let cover = film_root.join(FILM_COVER_FILENAME);
    let cover_rel = media_local::is_usable_image_file(&cover)
        .then(|| format!("{artifact_root}/{FILM_COVER_FILENAME}"));
    Some((video_rel, cover_rel))
}

pub async fn collect_shot_take_manifest(film_root: &Path) -> Vec<FilmShotTake> {
    let mut out = Vec::new();
    let mut dirs = vec![film_root.to_path_buf()];
    if let Ok(mut rd) = tokio::fs::read_dir(film_root).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("scene_") {
                dirs.push(ent.path());
            }
        }
    }
    for scene_dir in dirs {
        let shots = scene_dir.join("shots");
        let Ok(mut rd) = tokio::fs::read_dir(&shots).await else {
            continue;
        };
        let scene_root = scene_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let scene_rel = if scene_root.starts_with("scene_") {
            format!(
                "{}/{}",
                film_root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("idea2video"),
                scene_root
            )
        } else {
            film_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("idea2video")
                .to_string()
        };
        while let Ok(Some(ent)) = rd.next_entry().await {
            let Ok(idx) = ent.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let take = read_current_take_sync(&ent.path()).unwrap_or(0);
            out.push(FilmShotTake {
                scene_root: scene_rel.clone(),
                shot_idx: idx,
                take,
            });
        }
    }
    out.sort_by(|a, b| a.scene_root.cmp(&b.scene_root).then(a.shot_idx.cmp(&b.shot_idx)));
    out
}

pub async fn write_user_ref_bytes(
    session_root: &Path,
    scene_dir: &Path,
    shot_idx: i32,
    kind: &str,
    slot: u32,
    bytes: &[u8],
    ext: &str,
) -> VimaxResult<String> {
    let dir = shot_dir(scene_dir, shot_idx).join(USER_REFS_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let name = format!("{kind}_{slot}.{ext}");
    let dest = dir.join(&name);
    tokio::fs::write(&dest, bytes).await?;
    Ok(rel_to(session_root, &dest))
}

pub async fn copy_user_ref_from(
    session_root: &Path,
    scene_dir: &Path,
    shot_idx: i32,
    kind: &str,
    slot: u32,
    source: &Path,
) -> VimaxResult<String> {
    let dir = shot_dir(scene_dir, shot_idx).join(USER_REFS_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let ext = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or(if kind == "audio" { "wav" } else { "png" });
    let dest = dir.join(format!("{kind}_{slot}.{ext}"));
    tokio::fs::copy(source, &dest).await?;
    Ok(rel_to(session_root, &dest))
}

pub fn attach_media_urls(session_id: &str, packet: &mut ShotPacket) {
    for slot in packet.image_refs.iter_mut().chain(packet.audio_refs.iter_mut()) {
        if let Some(path) = slot.path.as_deref().filter(|p| !p.is_empty()) {
            slot.url = Some(format!(
                "/api/vimax/sessions/{}/artifacts/{}",
                session_id,
                path
            ));
        }
    }
}

pub fn scene_idx_from_dir(scene_dir: &Path) -> Option<i32> {
    scene_dir
        .file_name()?
        .to_str()?
        .strip_prefix("scene_")?
        .parse()
        .ok()
}

/// Walk film/scene working dirs that publish a storyboard.
pub fn find_scene_dirs(session_root: &Path, work: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if work.join("storyboard.json").is_file() {
        out.push(work.to_path_buf());
    }
    if let Ok(rd) = std::fs::read_dir(work) {
        let mut kids: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        kids.sort_by_key(|e| e.file_name());
        for ent in kids {
            let path = ent.path();
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("scene_") && path.join("storyboard.json").is_file() {
                out.push(path);
            }
        }
    }
    if out.is_empty() && session_root.join("storyboard.json").is_file() {
        out.push(session_root.to_path_buf());
    }
    let mut seen = HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ShotDescription;

    fn sample_shot() -> ShotDescription {
        ShotDescription {
            idx: 1,
            is_last: false,
            cam_idx: 0,
            visual_desc: "wide cafe".into(),
            variation_type: "small".into(),
            variation_reason: String::new(),
            ff_desc: "wide cafe".into(),
            ff_vis_char_idxs: vec![],
            lf_desc: "wide cafe".into(),
            lf_vis_char_idxs: vec![],
            motion_desc: "dolly in".into(),
            audio_desc: Some("雨声".into()),
            location_id: "INT. CAFE".into(),
            beats: vec![
                ShotBeat {
                    motion_desc: "A looks up".into(),
                    audio_desc: Some("A: hi".into()),
                    cam_idx: Some(0),
                },
                ShotBeat {
                    motion_desc: "B smiles".into(),
                    audio_desc: Some("B: hey".into()),
                    cam_idx: Some(1),
                },
            ],
        }
    }

    #[test]
    fn packet_preserves_packed_beats() {
        let packet = packet_from_shot("idea2video/scene_0", &sample_shot());
        assert_eq!(packet.beats.len(), 2);
        assert_eq!(packet.beats[1].cam_idx, 1);
        assert_eq!(packet.location_id, "INT. CAFE");
    }

    #[test]
    fn patch_marks_user_edited_and_joins_beats() {
        let mut packet = packet_from_shot("idea2video/scene_0", &sample_shot());
        let changed = apply_patch(
            &mut packet,
            &ShotPacketPatch {
                beats: Some(vec![
                    ShotPacketBeat {
                        visual_desc: "new A".into(),
                        audio_desc: "line A".into(),
                        cam_idx: 0,
                    },
                    ShotPacketBeat {
                        visual_desc: "new B".into(),
                        audio_desc: "line B".into(),
                        cam_idx: 1,
                    },
                ]),
                ..Default::default()
            },
        );
        assert!(changed.script_changed);
        assert!(!changed.layout_changed);
        assert!(packet.user_edited);
        assert!(packet.visual_desc.contains("new A"));
        assert!(packet.audio_desc.contains("line B"));
    }

    #[test]
    fn override_keeps_locked_header() {
        let compiled = "8s, 16:9.\n@Image1 is the last frame.\n画面：old cafe\n台词：hi";
        let out = bind_prompt_with_override(compiled, Some("画面：new cafe\n台词：hello"));
        assert!(out.contains("@Image1"));
        assert!(out.contains("new cafe"));
        assert!(!out.contains("old cafe"));
    }

    #[test]
    fn render_mode_parses() {
        assert_eq!(RenderMode::parse("shot_review"), RenderMode::ShotReview);
        assert_eq!(RenderMode::parse(""), RenderMode::Continuous);
        assert_eq!(RenderMode::parse("continuous"), RenderMode::Continuous);
    }

    #[test]
    fn merge_keeps_unbound_continuity() {
        let mut packet = packet_from_shot("idea2video/scene_0", &sample_shot());
        packet.image_refs = vec![ShotRefSlot {
            slot: 1,
            role: ShotRefRole::ContinuityLastFrame,
            label: "prev".into(),
            character_id: None,
            path: None,
            url: None,
            unbound: true,
            user_override: true,
        }];
        merge_live_image_refs(
            &mut packet,
            vec![ShotRefSlot {
                slot: 1,
                role: ShotRefRole::ContinuityLastFrame,
                label: "live last frame".into(),
                character_id: None,
                path: Some("idea2video/scene_0/shots/0/video_last_frame.png".into()),
                url: None,
                unbound: false,
                user_override: false,
            }],
        );
        assert!(packet.image_refs[0].unbound);
        assert!(packet.image_refs[0].path.is_none());
    }

    #[tokio::test]
    async fn seed_skips_user_edited_script() {
        let tmp = tempfile::tempdir().unwrap();
        let scene = tmp.path().join("scene_0");
        let shot = sample_shot();
        let mut packet = packet_from_shot("scene_0", &shot);
        packet.user_edited = true;
        packet.visual_desc = "kept".into();
        save_packet(&scene, &packet).await.unwrap();
        let mut fresh = shot.clone();
        fresh.visual_desc = "planner rewrite".into();
        seed_packets_from_shots(&scene, tmp.path(), &[fresh])
            .await
            .unwrap();
        let loaded = load_packet(&scene, 1).await.unwrap().unwrap();
        assert_eq!(loaded.visual_desc, "kept");
    }

    #[test]
    fn layout_patch_does_not_mark_script_dirty() {
        let mut packet = packet_from_shot("idea2video/scene_0", &sample_shot());
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "video".into(),
            ShotGraphPoint {
                x: 240.0,
                y: 80.0,
                w: None,
                h: None,
            },
        );
        let outcome = apply_patch(
            &mut packet,
            &ShotPacketPatch {
                layout: Some(ShotGraphLayout {
                    viewport: Some(ShotGraphViewport {
                        x: 12.0,
                        y: 8.0,
                        k: 1.0,
                    }),
                    nodes,
                }),
                recompile: Some(false),
                ..Default::default()
            },
        );
        assert!(!outcome.script_changed);
        assert!(outcome.layout_changed);
        assert!(!packet.user_edited);
        assert!(packet.layout.is_some());
    }

    #[test]
    fn ensure_ref_slot_caps_custom_images() {
        let mut slots = Vec::new();
        for slot in 1..=MAX_SHOT_IMAGE_REFS {
            ensure_ref_slot(&mut slots, slot as u32, false).unwrap();
        }
        assert_eq!(slots.len(), MAX_SHOT_IMAGE_REFS);
        assert!(ensure_ref_slot(&mut slots, 99, false).is_err());
        assert_eq!(next_ref_slot(&slots), (MAX_SHOT_IMAGE_REFS as u32) + 1);
    }

    fn fake_mp4_bytes(len: usize) -> Vec<u8> {
        let mut v = vec![0u8; len.max(12)];
        v[0..4].copy_from_slice(&20u32.to_be_bytes());
        v[4..8].copy_from_slice(b"ftyp");
        v[8..12].copy_from_slice(b"isom");
        v
    }

    fn write_fake_mp4(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, fake_mp4_bytes(media_local::MIN_USABLE_VIDEO_BYTES as usize)).unwrap();
    }

    #[test]
    fn restore_live_film_copies_current_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let film = tmp.path().join("script2video");
        let archived = film.join("films/v2/final_video.mp4");
        write_fake_mp4(&archived);
        std::fs::write(
            film.join("films/current.json"),
            r#"{"take":2}"#,
        )
        .unwrap();
        let live = ensure_live_film_sync(&film).expect("restore");
        assert_eq!(live, film.join(FINAL_VIDEO_FILENAME));
        assert!(media_local::is_usable_video_file(&live));
        assert_eq!(
            restore_session_live_film(tmp.path(), "script2video")
                .unwrap()
                .0,
            "script2video/final_video.mp4"
        );
    }

    #[test]
    fn restore_live_film_falls_back_to_latest_usable_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let film = tmp.path().join("script2video");
        write_fake_mp4(&film.join("films/v1/final_video.mp4"));
        std::fs::create_dir_all(film.join("films/v3")).unwrap();
        std::fs::write(
            film.join("films/current.json"),
            r#"{"take":3}"#,
        )
        .unwrap();
        let live = ensure_live_film_sync(&film).expect("v1 fallback");
        assert!(media_local::is_usable_video_file(&live));
    }

    #[test]
    fn restore_live_film_is_noop_when_canonical_cut_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let film = tmp.path().join("script2video");
        let live_path = film.join(FINAL_VIDEO_FILENAME);
        let mut live_bytes = fake_mp4_bytes(media_local::MIN_USABLE_VIDEO_BYTES as usize);
        live_bytes[20] = 1;
        let mut archive_bytes = fake_mp4_bytes(media_local::MIN_USABLE_VIDEO_BYTES as usize);
        archive_bytes[20] = 2;
        std::fs::create_dir_all(film.join("films/v1")).unwrap();
        std::fs::write(&live_path, &live_bytes).unwrap();
        std::fs::write(film.join("films/v1/final_video.mp4"), &archive_bytes).unwrap();
        ensure_live_film_sync(&film).unwrap();
        assert_eq!(std::fs::read(&live_path).unwrap()[20], 1);
    }
}
