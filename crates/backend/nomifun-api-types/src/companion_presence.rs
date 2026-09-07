//! Companion presence wire contract: mood/activity vocabulary, needs stats,
//! ABC interaction, agent-state mapping, and the CompanionPack runtime schema.
//!
//! Content packs declare clips and hit areas explicitly. Folder-name or
//! spritesheet-row conventions are not part of this protocol.

use std::collections::BTreeMap;

use nomifun_common::AgentExecutionStatus;
use serde::{Deserialize, Serialize};

/// Canonical desktop-companion moods. Unknown stored words fall back to Content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
#[serde(rename_all = "snake_case")]
pub enum CompanionMood {
    Happy,
    Content,
    Sleepy,
    Worried,
    Excited,
}

impl CompanionMood {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Happy => "happy",
            Self::Content => "content",
            Self::Sleepy => "sleepy",
            Self::Worried => "worried",
            Self::Excited => "excited",
        }
    }

    pub fn parse_loose(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "happy" => Self::Happy,
            "sleepy" => Self::Sleepy,
            "worried" => Self::Worried,
            "excited" => Self::Excited,
            "proud" | "playful" => Self::Happy,
            "calm" | "ok" | "content" => Self::Content,
            "sad" | "anxious" => Self::Worried,
            "tired" | "bored" => Self::Sleepy,
            _ => Self::Content,
        }
    }
}

impl std::fmt::Display for CompanionMood {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Presence activity. `thinking` is a learn-run in flight; the rest map agent
/// or interaction phases. Built-in CSS characters implement the same ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
#[serde(rename_all = "snake_case")]
pub enum CompanionActivity {
    Idle,
    Thinking,
    Busy,
    AwaitingUser,
    Review,
    Failed,
    Interacting,
}

impl CompanionActivity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Busy => "busy",
            Self::AwaitingUser => "awaiting_user",
            Self::Review => "review",
            Self::Failed => "failed",
            Self::Interacting => "interacting",
        }
    }

    pub fn parse_loose(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "idle" => Some(Self::Idle),
            "thinking" => Some(Self::Thinking),
            "busy" => Some(Self::Busy),
            "awaiting_user" | "waiting" => Some(Self::AwaitingUser),
            "review" | "reviewing" | "planning" => Some(Self::Review),
            "failed" | "fail" => Some(Self::Failed),
            "interacting" => Some(Self::Interacting),
            _ => None,
        }
    }
}

impl std::fmt::Display for CompanionActivity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// ABC interaction phase for a held pointer gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
#[serde(rename_all = "snake_case")]
pub enum InteractionPhase {
    #[default]
    Idle,
    AStart,
    BLoop,
    CEnd,
}

impl InteractionPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::AStart => "a_start",
            Self::BLoop => "b_loop",
            Self::CEnd => "c_end",
        }
    }
}

/// Hit-area intent. Table-driven packs bind rectangles to these ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
#[serde(rename_all = "snake_case")]
pub enum HitIntent {
    Head,
    Body,
    Raise,
}

impl HitIntent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Body => "body",
            Self::Raise => "raise",
        }
    }

    pub const fn clip_stem(self) -> &'static str {
        match self {
            Self::Head => "touch_head",
            Self::Body => "touch_body",
            Self::Raise => "raise",
        }
    }
}

/// Pointer event reported by the overlay ABC FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
#[serde(rename_all = "snake_case")]
pub enum PresencePointerEvent {
    Down,
    Hold,
    Up,
}

/// Normalized 0–100 needs vector. Mood is derived from this, not stored as the
/// sole source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct PresenceStats {
    pub energy: f64,
    pub affection: f64,
    pub boredom: f64,
}

impl Default for PresenceStats {
    fn default() -> Self {
        Self {
            energy: 72.0,
            affection: 58.0,
            boredom: 28.0,
        }
    }
}

impl PresenceStats {
    pub fn clamp(self) -> Self {
        Self {
            energy: clamp_stat(self.energy),
            affection: clamp_stat(self.affection),
            boredom: clamp_stat(self.boredom),
        }
    }
}

fn clamp_stat(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
}

/// Snapshot the overlay and `/nomi` cards can refetch. WS is a hint only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct PresenceState {
    pub stats: PresenceStats,
    pub mood: CompanionMood,
    pub activity: CompanionActivity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_clip: Option<String>,
    pub interaction: InteractionPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_intent: Option<HitIntent>,
    pub last_tick_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_interact_ms: Option<i64>,
}

impl Default for PresenceState {
    fn default() -> Self {
        Self {
            stats: PresenceStats::default(),
            mood: CompanionMood::Content,
            activity: CompanionActivity::Idle,
            active_clip: Some(CompanionActivity::Idle.as_str().to_owned()),
            interaction: InteractionPhase::Idle,
            interaction_intent: None,
            last_tick_ms: 0,
            last_interact_ms: None,
        }
    }
}

/// Inputs for composing activity from the companion session / AgentExecution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct PresenceAgentSnapshot {
    #[serde(default)]
    pub learn_running: bool,
    #[serde(default)]
    pub conversation_processing: bool,
    #[serde(default)]
    pub pending_confirmations: bool,
    /// `ConversationRuntimeStateKind` snake_case, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_state: Option<String>,
    /// `AgentExecutionStatus` snake_case, when a linked execution exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_status: Option<String>,
    /// Work session has this companion summoned (read-only memory; no persona takeover).
    #[serde(default)]
    pub summoned: bool,
}

/// Body for `POST /api/companion/companions/{id}/presence/interact`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct ApplyPresenceInteractionRequest {
    pub intent: HitIntent,
    pub event: PresencePointerEvent,
}

/// How a pack draws the figure. CSS packs reuse built-in React characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
#[serde(rename_all = "snake_case")]
pub enum CompanionPackRenderer {
    #[default]
    Css,
    Atlas,
}

/// One atlas animation row. `id` is an explicit clip name, never inferred from the row index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct CompanionAtlasState {
    pub id: String,
    pub row: u32,
    pub frames: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durations_ms: Option<Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct CompanionAtlas {
    pub path: String,
    pub cell: [u32; 2],
    pub cols: u32,
    #[serde(default)]
    pub states: Vec<CompanionAtlasState>,
}

/// Hit rectangle in figure-bbox fractions (origin top-left).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct CompanionHitArea {
    pub id: String,
    pub intent: HitIntent,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct CompanionAbcClip {
    pub a: String,
    pub b: String,
    pub c: String,
}

/// Catalog metadata. Not copied into the overlay runtime install.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct CompanionPackManifest {
    pub pack_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_key: Option<String>,
}

/// Host-minimum runtime contract (`runtime.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "../../../../ui/src/common/protocolBindings/")]
pub struct CompanionPackRuntime {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub renderer: CompanionPackRenderer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlas: Option<CompanionAtlas>,
    #[serde(default)]
    pub hit_areas: Vec<CompanionHitArea>,
    #[serde(default)]
    pub clips: BTreeMap<String, CompanionAbcClip>,
    /// Maps AgentExecution / conversation tokens onto presence activities.
    #[serde(default)]
    pub agent_bindings: BTreeMap<String, CompanionActivity>,
}

impl CompanionPackRuntime {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported companion pack schema_version {}",
                self.schema_version
            ));
        }
        if !simple_pack_id(&self.id) {
            return Err(format!(
                "pack id '{}' must be kebab-case, optionally 'slug--author'",
                self.id
            ));
        }
        if self.display_name.trim().is_empty() {
            return Err("pack display_name must not be empty".into());
        }
        match self.renderer {
            CompanionPackRenderer::Css => {
                if let Some(atlas) = &self.atlas {
                    return Err(format!(
                        "css pack must not declare an atlas (found {})",
                        atlas.path
                    ));
                }
            }
            CompanionPackRenderer::Atlas => {
                let Some(atlas) = &self.atlas else {
                    return Err("atlas pack requires an atlas object".into());
                };
                validate_atlas(atlas)?;
            }
        }
        for area in &self.hit_areas {
            validate_hit_area(area)?;
        }
        for (name, clip) in &self.clips {
            if name.trim().is_empty() || clip.a.trim().is_empty() || clip.b.trim().is_empty()
                || clip.c.trim().is_empty()
            {
                return Err(format!("clip '{name}' has an empty phase name"));
            }
        }
        Ok(())
    }
}

fn simple_pack_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 80 {
        return false;
    }
    let parts: Vec<&str> = id.split("--").collect();
    if parts.len() > 2 || parts.is_empty() {
        return false;
    }
    parts.iter().all(|part| kebab_slug(part))
}

fn kebab_slug(part: &str) -> bool {
    if part.is_empty() || part.starts_with('-') || part.ends_with('-') || part.contains("--") {
        return false;
    }
    part.chars()
        .all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '-'))
        && !part.contains("---")
}

fn validate_atlas(atlas: &CompanionAtlas) -> Result<(), String> {
    if atlas.path.trim().is_empty()
        || atlas.path.contains("..")
        || atlas.path.contains('/')
        || atlas.path.contains('\\')
    {
        return Err("atlas.path must be a bare filename in the pack directory".into());
    }
    if !atlas.path.ends_with(".webp") {
        return Err("atlas.path must be a .webp file".into());
    }
    if atlas.cols == 0 || atlas.cell[0] == 0 || atlas.cell[1] == 0 {
        return Err("atlas cell/cols must be positive".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for state in &atlas.states {
        if state.id.trim().is_empty() || state.frames == 0 {
            return Err(format!("atlas state '{}' is invalid", state.id));
        }
        if !seen.insert(state.id.clone()) {
            return Err(format!("duplicate atlas state id '{}'", state.id));
        }
        if let Some(durations) = &state.durations_ms
            && durations.len() != state.frames as usize
        {
            return Err(format!(
                "atlas state '{}' durations_ms length must match frames",
                state.id
            ));
        }
    }
    Ok(())
}

fn validate_hit_area(area: &CompanionHitArea) -> Result<(), String> {
    if area.id.trim().is_empty() {
        return Err("hit area id must not be empty".into());
    }
    for (name, value) in [("x", area.x), ("y", area.y), ("w", area.w), ("h", area.h)] {
        if !value.is_finite() {
            return Err(format!("hit area '{}' {name} must be finite", area.id));
        }
    }
    if area.w <= 0.0 || area.h <= 0.0 {
        return Err(format!("hit area '{}' needs positive w/h", area.id));
    }
    if area.x < 0.0 || area.y < 0.0 || area.x + area.w > 1.0 + 1e-6 || area.y + area.h > 1.0 + 1e-6
    {
        return Err(format!(
            "hit area '{}' must lie inside the unit square",
            area.id
        ));
    }
    Ok(())
}

/// Built-in hit table used when a CSS character has no pack override.
pub fn default_hit_areas() -> Vec<CompanionHitArea> {
    vec![
        CompanionHitArea {
            id: "head".into(),
            intent: HitIntent::Head,
            x: 0.22,
            y: 0.02,
            w: 0.56,
            h: 0.30,
        },
        CompanionHitArea {
            id: "body".into(),
            intent: HitIntent::Body,
            x: 0.16,
            y: 0.32,
            w: 0.68,
            h: 0.62,
        },
    ]
}

pub fn default_abc_clips() -> BTreeMap<String, CompanionAbcClip> {
    let mut clips = BTreeMap::new();
    for intent in [HitIntent::Head, HitIntent::Body, HitIntent::Raise] {
        let stem = intent.clip_stem();
        clips.insert(
            stem.to_owned(),
            CompanionAbcClip {
                a: format!("{stem}_a"),
                b: format!("{stem}_b"),
                c: format!("{stem}_c"),
            },
        );
    }
    clips
}

pub fn builtin_css_pack(character: &str, display_name: &str) -> CompanionPackRuntime {
    CompanionPackRuntime {
        schema_version: 1,
        id: format!("{character}--builtin"),
        display_name: display_name.to_owned(),
        renderer: CompanionPackRenderer::Css,
        atlas: None,
        hit_areas: default_hit_areas(),
        clips: default_abc_clips(),
        agent_bindings: default_agent_bindings(),
    }
}

pub fn default_agent_bindings() -> BTreeMap<String, CompanionActivity> {
    let mut map = BTreeMap::new();
    for (from, to) in [
        ("running", CompanionActivity::Busy),
        ("planning", CompanionActivity::Review),
        ("awaiting_approval", CompanionActivity::AwaitingUser),
        ("waiting_input", CompanionActivity::AwaitingUser),
        ("paused", CompanionActivity::Review),
        ("failed", CompanionActivity::Failed),
        ("completed_with_failures", CompanionActivity::Failed),
        ("starting", CompanionActivity::Busy),
        ("waiting_confirmation", CompanionActivity::AwaitingUser),
    ] {
        map.insert(from.to_owned(), to);
    }
    map
}

pub fn clip_for_phase(intent: HitIntent, phase: InteractionPhase) -> Option<String> {
    let stem = intent.clip_stem();
    match phase {
        InteractionPhase::Idle => None,
        InteractionPhase::AStart => Some(format!("{stem}_a")),
        InteractionPhase::BLoop => Some(format!("{stem}_b")),
        InteractionPhase::CEnd => Some(format!("{stem}_c")),
    }
}

/// Map an AgentExecution status token onto presence activity. Terminal success
/// and cancel fall through to `None` so idle/conversation state can win.
pub fn activity_from_execution_status(status: &str) -> Option<CompanionActivity> {
    if let Some(bound) = default_agent_bindings().get(status) {
        return Some(*bound);
    }
    match AgentExecutionStatus::from_str_loose(status) {
        Some(AgentExecutionStatus::Running) => Some(CompanionActivity::Busy),
        Some(AgentExecutionStatus::Planning | AgentExecutionStatus::Paused) => {
            Some(CompanionActivity::Review)
        }
        Some(AgentExecutionStatus::AwaitingApproval | AgentExecutionStatus::WaitingInput) => {
            Some(CompanionActivity::AwaitingUser)
        }
        Some(AgentExecutionStatus::Failed | AgentExecutionStatus::CompletedWithFailures) => {
            Some(CompanionActivity::Failed)
        }
        Some(
            AgentExecutionStatus::Completed
            | AgentExecutionStatus::Cancelled,
        ) => None,
        None => CompanionActivity::parse_loose(status)
            .filter(|activity| *activity != CompanionActivity::Idle),
    }
}

trait FromStrLoose {
    fn from_str_loose(raw: &str) -> Option<Self>
    where
        Self: Sized;
}

impl FromStrLoose for AgentExecutionStatus {
    fn from_str_loose(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "planning" => Some(Self::Planning),
            "awaiting_approval" => Some(Self::AwaitingApproval),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "waiting_input" => Some(Self::WaitingInput),
            "completed" => Some(Self::Completed),
            "completed_with_failures" => Some(Self::CompletedWithFailures),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Compose the visible activity. Interaction always wins; then learn; then
/// AgentExecution; then the nomi conversation runtime; then idle.
pub fn compose_activity(
    interaction: InteractionPhase,
    snapshot: &PresenceAgentSnapshot,
) -> CompanionActivity {
    if !matches!(interaction, InteractionPhase::Idle) {
        return CompanionActivity::Interacting;
    }
    if snapshot.learn_running {
        return CompanionActivity::Thinking;
    }
    if let Some(status) = snapshot.execution_status.as_deref()
        && let Some(activity) = activity_from_execution_status(status)
    {
        return activity;
    }
    if snapshot.pending_confirmations
        || snapshot
            .runtime_state
            .as_deref()
            .is_some_and(|state| state == "waiting_confirmation")
    {
        return CompanionActivity::AwaitingUser;
    }
    if snapshot.conversation_processing
        || snapshot.summoned
        || snapshot
            .runtime_state
            .as_deref()
            .is_some_and(|state| state == "running" || state == "starting")
    {
        return CompanionActivity::Busy;
    }
    CompanionActivity::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_id_accepts_slug_and_author_variant() {
        assert!(simple_pack_id("mochi--builtin"));
        assert!(simple_pack_id("firefly"));
        assert!(!simple_pack_id("Mochi"));
        assert!(!simple_pack_id("--x"));
        assert!(!simple_pack_id("a--b--c"));
    }

    #[test]
    fn builtin_css_pack_validates() {
        builtin_css_pack("mochi", "Mochi").validate().unwrap();
    }

    #[test]
    fn atlas_pack_rejects_parent_path() {
        let mut pack = builtin_css_pack("ink", "Ink");
        pack.renderer = CompanionPackRenderer::Atlas;
        pack.atlas = Some(CompanionAtlas {
            path: "../secret.webp".into(),
            cell: [192, 208],
            cols: 8,
            states: vec![CompanionAtlasState {
                id: "idle".into(),
                row: 0,
                frames: 4,
                durations_ms: None,
            }],
        });
        assert!(pack.validate().is_err());
    }

    #[test]
    fn compose_activity_priority() {
        let mut snap = PresenceAgentSnapshot {
            learn_running: true,
            conversation_processing: true,
            execution_status: Some("running".into()),
            ..PresenceAgentSnapshot::default()
        };
        assert_eq!(
            compose_activity(InteractionPhase::BLoop, &snap),
            CompanionActivity::Interacting
        );
        assert_eq!(
            compose_activity(InteractionPhase::Idle, &snap),
            CompanionActivity::Thinking
        );
        snap.learn_running = false;
        assert_eq!(
            compose_activity(InteractionPhase::Idle, &snap),
            CompanionActivity::Busy
        );
        snap.execution_status = Some("awaiting_approval".into());
        assert_eq!(
            compose_activity(InteractionPhase::Idle, &snap),
            CompanionActivity::AwaitingUser
        );
        snap.execution_status = Some("failed".into());
        assert_eq!(
            compose_activity(InteractionPhase::Idle, &snap),
            CompanionActivity::Failed
        );
        snap.execution_status = Some("completed".into());
        snap.conversation_processing = false;
        snap.runtime_state = Some("idle".into());
        assert_eq!(
            compose_activity(InteractionPhase::Idle, &snap),
            CompanionActivity::Idle
        );
        snap.summoned = true;
        assert_eq!(
            compose_activity(InteractionPhase::Idle, &snap),
            CompanionActivity::Busy
        );
    }

    #[test]
    fn hit_area_must_fit_unit_square() {
        let area = CompanionHitArea {
            id: "head".into(),
            intent: HitIntent::Head,
            x: 0.9,
            y: 0.0,
            w: 0.2,
            h: 0.2,
        };
        assert!(validate_hit_area(&area).is_err());
    }
}
