//! Structured session history owned by the host.
//!
//! The previous index stored only display lines, so tool arguments were lost on
//! restore (`{"target": ...}` was synthesized). This store keeps whole
//! [`Message`] values, including raw tool-call argument strings and tool-result
//! pairing, so a restored session can be sent to a model unchanged.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::types::{Message, TodoItem};

/// Durable origin label. Older sessions default to `Unknown`; callers must
/// opt a newly created global-home session into artifact delivery explicitly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionOrigin {
    Home,
    Project {
        project_id: Option<String>,
    },
    #[default]
    Unknown,
}

/// Stable, host-assigned owner identity. This is an external subject, never a credential.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOwner {
    pub authority: String,
    pub subject: String,
}

impl fmt::Debug for ExecutionOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExecutionOwner(<redacted>)")
    }
}

/// Why the host created a session, independent of its Home/Project origin.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionSource {
    #[default]
    Unknown,
    Conversation,
    Automation {
        automation_id: String,
        run_id: String,
        attempt_id: String,
    },
}

/// Durable execution identity. Legacy data stays unknown and unowned.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionMetadata {
    pub owner: Option<ExecutionOwner>,
    pub source: ExecutionSource,
}

impl ExecutionMetadata {
    #[must_use]
    pub const fn conversation() -> Self {
        Self {
            owner: None,
            source: ExecutionSource::Conversation,
        }
    }

    /// Check identity shape without resolving account authorization.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.owner.as_ref().is_some_and(|owner| {
            owner.authority.trim().is_empty() || owner.subject.trim().is_empty()
        }) {
            return false;
        }
        match &self.source {
            ExecutionSource::Unknown | ExecutionSource::Conversation => true,
            ExecutionSource::Automation {
                automation_id,
                run_id,
                attempt_id,
            } => {
                self.owner.is_some()
                    && !automation_id.trim().is_empty()
                    && !run_id.trim().is_empty()
                    && !attempt_id.trim().is_empty()
            }
        }
    }
}

/// Immutable binding checked when a host ensures a detached session exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSpec {
    pub id: String,
    pub created_at_ms: u64,
    pub origin: SessionOrigin,
    pub workspace: Option<SessionWorkspace>,
    pub execution: ExecutionMetadata,
    pub role: SessionRole,
    pub root_session_id: Option<String>,
    pub access_mode: WorkAccessMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionCreateDisposition {
    Created,
    Existing,
}

/// Host-assigned workspace for one session, separate from the ambient process
/// working directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionWorkspace {
    pub root: PathBuf,
}

/// The durable result of a completed turn that delivered one or more files.
/// The independent artifact-store marker remains the visibility authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryCommit {
    pub session_id: String,
    pub submission_id: String,
    /// Complete, ordered set of artifacts published by this turn.
    pub artifact_ids: Vec<String>,
    pub completed_successfully: bool,
    pub committed_at_ms: u64,
    #[serde(default)]
    pub origin: SessionOrigin,
    #[serde(default)]
    pub origin_title_snapshot: String,
    #[serde(default)]
    pub execution: ExecutionMetadata,
}

/// What execution produced, independent of artifact-marker and notification
/// delivery. A failed marker must never turn a completed model run into a new
/// execution attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnExecutionOutcome {
    Completed,
    Failed,
    Stopped,
}

/// Terminal evidence saved in the same atomic history replacement as the
/// turn's messages. Older history has no evidence and stays unknown.
///
/// `artifact_ids` is the complete group requiring the independent artifact
/// marker; it does not claim that marker has been written. An empty group
/// requires no artifact commit. The (session, submission) pair is immutable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCompletionEvidence {
    pub session_id: String,
    pub submission_id: String,
    pub outcome: TurnExecutionOutcome,
    pub artifact_ids: Vec<String>,
    pub committed_at_ms: u64,
    pub execution: ExecutionMetadata,
}

/// Host callback that makes a prepared delivery group visible in its
/// independent artifact store. Implementations must be idempotent.
#[async_trait::async_trait]
pub trait DeliveryCommitter: Send + Sync {
    async fn commit(&self, commit: &DeliveryCommit) -> Result<(), String>;
}

/// Lifecycle state of the last turn in a session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnState {
    #[default]
    Idle,
    Running,
    WaitingForUser,
    Stopping,
    Completed,
    Stopped,
    Error {
        message: String,
    },
}

impl TurnState {
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        matches!(self, Self::Running | Self::Stopping)
    }
}

/// Classification of one tracked object inside a [`WorkContract`].
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TrackedObjectKind {
    #[default]
    Deliverable,
    Record,
}

/// Confirmation status of a fact bound to a tracked object.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    #[default]
    Confirmed,
    PendingProposal,
}

/// One factual entry attached to a tracked object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactEntry {
    pub value: String,
    #[serde(default)]
    pub status: FactStatus,
    #[serde(default)]
    pub updated_at_ms: u64,
}

/// Verification status tracked per object inside a [`WorkContract`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectVerifyState {
    #[default]
    Unverified,
    Passed {
        report_id: String,
        revision: u32,
    },
    Failed {
        report_id: String,
    },
    Overridden {
        report_id: String,
    },
}

/// One addressable object tracked within a session's [`WorkContract`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedObject {
    pub object_id: String,
    #[serde(default)]
    pub kind: TrackedObjectKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub extension: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub anchor_summary: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, FactEntry>,
    #[serde(default)]
    pub verify_state: ObjectVerifyState,
}

impl TrackedObject {
    #[must_use]
    pub fn display_label(&self) -> String {
        self.filename
            .clone()
            .unwrap_or_else(|| format!("*.{}", self.extension))
    }
}

/// One candidate choice presented on a [`PendingCard::Disambiguation`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisambiguationOption {
    pub object_id: String,
    pub label: String,
    #[serde(default)]
    pub anchor_summary: String,
    #[serde(default)]
    pub extension: String,
}

/// Durable interactive card persisted on a session while [`TurnState::WaitingForUser`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingCard {
    Disambiguation {
        card_id: String,
        created_at_ms: u64,
        prompt: String,
        impact_boundary: String,
        options: Vec<DisambiguationOption>,
        #[serde(default)]
        allow_multi: bool,
    },
    ToolApproval {
        card_id: String,
        created_at_ms: u64,
        tool_name: String,
        target_summary: String,
        args_hash: String,
        risk_reason: String,
        command_preview: String,
    },
}

impl PendingCard {
    #[must_use]
    pub fn card_id(&self) -> &str {
        match self {
            Self::Disambiguation { card_id, .. }
            | Self::ToolApproval { card_id, .. } => card_id,
        }
    }
}

/// Five-element handoff manifest recorded once a cancelled turn finishes reaping.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopHandoffManifest {
    #[serde(default)]
    pub input_revision: u32,
    #[serde(default)]
    pub command_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verified_effects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_checks: Vec<String>,
    #[serde(default)]
    pub rollback_hint: String,
}

/// Three-part honest progress heartbeat tracked live per active session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchHeartbeat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_completed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_completed_at_ms: Option<u64>,
    #[serde(default)]
    pub current_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_check_hint: Option<String>,
    #[serde(default)]
    pub awaiting_evidence: bool,
}

/// Durable terminal outcome recorded per `submission_id` inside a session.
///
/// Lets crash recovery and work-status queries verify the exact outcome of an
/// individual submission instead of guessing from the latest session status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubmissionOutcome {
    Completed,
    Partial { missing: Vec<String> },
    Failed { message: String },
    Aborted,
}

/// One required deliverable item expected by a work revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredDeliverable {
    /// Explicit filename when requested (e.g. `sample.pptx`), matched case-insensitively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Lowercase extension without leading dot (e.g. `pptx`, `xlsx`, `docx`, `pdf`, `csv`).
    pub extension: String,
}

impl RequiredDeliverable {
    #[must_use]
    pub fn label(&self) -> String {
        self.filename
            .clone()
            .unwrap_or_else(|| format!("*.{}", self.extension))
    }
}

/// Host-owned, versioned work contract persisted on a session.
///
/// Inherited across follow-up revisions so narrow edits (such as "only update
/// the date") retain prior slide-count, required-deliverable, and pinned-fact
/// constraints until explicitly changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkContract {
    #[serde(default)]
    pub revision: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_items: Vec<RequiredDeliverable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_slides: Option<usize>,
    /// Factual values pinned from user briefs (e.g. `"quote"`, `"date"`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pinned_facts: BTreeMap<String, String>,
    /// Multi-object deliverables and lightweight records with isolated overrides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<TrackedObject>,
}

impl WorkContract {
    #[must_use]
    pub fn find_object(&self, object_id: &str) -> Option<&TrackedObject> {
        self.objects.iter().find(|obj| obj.object_id == object_id)
    }

    /// Writable deliverable objects (excludes read-only/archival [`TrackedObjectKind::Record`]).
    #[must_use]
    pub fn writable_deliverables(&self) -> Vec<&TrackedObject> {
        self.objects
            .iter()
            .filter(|obj| obj.kind == TrackedObjectKind::Deliverable)
            .collect()
    }

    /// Resolve effective confirmed facts for `object_id`: starts from session
    /// defaults (`pinned_facts`) and applies only [`FactStatus::Confirmed`]
    /// overrides from that object, ignoring [`FactStatus::PendingProposal`].
    #[must_use]
    pub fn confirmed_facts_for(
        &self,
        object_id: &str,
    ) -> BTreeMap<String, String> {
        let mut facts = self.pinned_facts.clone();
        if let Some(obj) = self.find_object(object_id) {
            for (key, entry) in &obj.overrides {
                if entry.status == FactStatus::Confirmed {
                    facts.insert(key.clone(), entry.value.clone());
                }
            }
        }
        facts
    }

    /// Resolve the best-matching writable deliverable for `artifact_path` and
    /// return its effective confirmed facts (falling back to session defaults).
    #[must_use]
    pub fn confirmed_facts_for_artifact(
        &self,
        artifact_path: &Path,
    ) -> BTreeMap<String, String> {
        let file_name = artifact_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !file_name.is_empty()
            && let Some(obj) = self.objects.iter().find(|obj| {
                obj.kind == TrackedObjectKind::Deliverable
                    && obj
                        .filename
                        .as_deref()
                        .is_some_and(|f| f.eq_ignore_ascii_case(file_name))
            })
        {
            return self.confirmed_facts_for(&obj.object_id);
        }
        if let Some(ext) = artifact_path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        {
            let same_ext: Vec<&TrackedObject> = self
                .objects
                .iter()
                .filter(|obj| {
                    obj.kind == TrackedObjectKind::Deliverable
                        && obj.extension.eq_ignore_ascii_case(&ext)
                })
                .collect();
            if same_ext.len() == 1 {
                return self.confirmed_facts_for(&same_ext[0].object_id);
            }
        }
        self.pinned_facts.clone()
    }
    /// Return labels of required deliverables that are not satisfied by any
    /// distinct `(display_name, delivery_key)` pair in `delivered`.
    #[must_use]
    pub fn missing_deliverables(
        &self,
        delivered: &[(&str, &str)],
    ) -> Vec<String> {
        if self.required_items.is_empty() {
            return Vec::new();
        }
        let mut used = vec![false; delivered.len()];
        let mut missing = Vec::new();
        // Match explicit filenames first so extension-only items cannot steal them.
        for req in self
            .required_items
            .iter()
            .filter(|item| item.filename.is_some())
        {
            let expected = req.filename.as_deref().unwrap_or("");
            let matched =
                delivered.iter().enumerate().find(|(idx, (name, key))| {
                    if used[*idx] {
                        return false;
                    }
                    if name.eq_ignore_ascii_case(expected) {
                        return true;
                    }
                    let actual_ext = Path::new(name)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(str::to_ascii_lowercase);
                    if actual_ext.as_deref() == Some(req.extension.as_str())
                        && !key.is_empty()
                    {
                        let keyed = if key
                            .to_ascii_lowercase()
                            .ends_with(&format!(".{}", req.extension))
                        {
                            (*key).to_owned()
                        } else {
                            format!("{key}.{}", req.extension)
                        };
                        return keyed.eq_ignore_ascii_case(expected);
                    }
                    false
                });
            if let Some((idx, _)) = matched {
                used[idx] = true;
            } else {
                missing.push(req.label());
            }
        }
        for req in self
            .required_items
            .iter()
            .filter(|item| item.filename.is_none())
        {
            let matched =
                delivered.iter().enumerate().find(|(idx, (name, _))| {
                    if used[*idx] {
                        return false;
                    }
                    Path::new(name)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| {
                            ext.eq_ignore_ascii_case(&req.extension)
                        })
                });
            if let Some((idx, _)) = matched {
                used[idx] = true;
            } else {
                missing.push(req.label());
            }
        }
        missing
    }
}

/// Role of a durable session inside the Router / Worker concurrency architecture.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    #[default]
    Standard,
    Router,
    Worker,
}

impl SessionRole {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Standard)
    }
}

/// Workspace access capability assigned to a Worker session upon creation.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkAccessMode {
    ReadOnly,
    #[default]
    ReadWrite,
}

impl WorkAccessMode {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

/// One durable FIFO user follow-up accepted while a Router session is busy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRouterFollowup {
    pub submission_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_object_id: Option<String>,
    #[serde(default)]
    pub created_at_ms: u64,
}

/// Durable idempotency record for one Router `dispatch_work` / `revise_work` request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchRecord {
    pub request_id: String,
    pub fingerprint: String,
    pub worker_id: String,
    pub receipt_id: String,
    #[serde(default)]
    pub created_at_ms: u64,
}

/// One durable session: metadata plus the full structured message history.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub titled: bool,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub status: TurnState,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub todos: Vec<TodoItem>,
    #[serde(default)]
    pub origin: SessionOrigin,
    #[serde(default)]
    pub workspace: Option<SessionWorkspace>,
    #[serde(default)]
    pub delivery_commits: Vec<DeliveryCommit>,
    /// Per-submission durable terminal outcomes (`submission-<id>` -> outcome).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub submission_outcomes: BTreeMap<String, SubmissionOutcome>,
    /// Reserved dispatches and stop reason for each host-owned submission.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub run_budget_checkpoints: BTreeMap<String, crate::RunBudgetCheckpoint>,
    /// Active versioned work contract inherited across revisions in this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_contract: Option<WorkContract>,
    /// Contract revision bound to each submission (`submission-<id>` -> revision).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub submission_revisions: BTreeMap<String, u32>,
    /// Top-level session id when spawned from a parent conversation; omitted on root sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_session_id: Option<String>,
    /// Durable interactive cards awaiting explicit user choice or approval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_cards: Vec<PendingCard>,
    /// Five-element handoff manifest recorded after a cancelled turn finishes reaping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_stop_handoff: Option<StopHandoffManifest>,
    /// User-pinned sessions float to the top of sidebar and board lists.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// Archived sessions stay durable on disk while hidden from default lists.
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
    /// Session role (`standard` for legacy/direct sessions, `router`, or `worker`).
    #[serde(default, skip_serializing_if = "SessionRole::is_default")]
    pub role: SessionRole,
    /// Workspace access mode (`read_only` or `read_write`); immutable across worker resumes.
    #[serde(default, skip_serializing_if = "WorkAccessMode::is_default")]
    pub access_mode: WorkAccessMode,
    /// Durable FIFO user follow-ups accepted while this Router session was streaming.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_router_followups: Vec<PendingRouterFollowup>,
    /// Idempotency ledger mapping `request_id` to its committed worker dispatch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dispatch_requests: BTreeMap<String, DispatchRecord>,
    #[serde(default)]
    pub turn_completions: Vec<TurnCompletionEvidence>,
    #[serde(default)]
    pub execution: ExecutionMetadata,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes the field by reference"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

impl SessionRecord {
    #[must_use]
    pub fn effective_root_session_id(&self) -> &str {
        self.root_session_id.as_deref().unwrap_or(&self.id)
    }
}

/// The whole session file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionIndex {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub sessions: Vec<SessionRecord>,
}

fn log_storage_error(operation: &'static str, path: &Path, error: &io::Error) {
    tracing::error!(
        target: "flowy.storage",
        operation,
        path = %path.display(),
        kind = ?error.kind(),
        error = %error,
        "storage operation failed"
    );
}

impl SessionIndex {
    /// Stored next to (not instead of) the legacy `index.json` until the switch.
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join("sessions").join("history.json")
    }

    pub fn load(root: &Path) -> io::Result<Self> {
        let path = Self::path(root);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                log_storage_error("load structured history", &path, &error);
                return Err(error);
            }
        };
        serde_json::from_slice(&bytes).map_err(|error| {
            let error = io::Error::new(io::ErrorKind::InvalidData, error);
            log_storage_error("parse structured history", &path, &error);
            error
        })
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        let path = Self::path(root);
        let result = self.save_to(&path);
        if let Err(error) = &result {
            log_storage_error("save structured history", &path, error);
        }
        result
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        // Serialize atomic rename on Windows so concurrent saves never race on
        // the destination path.
        static REPLACE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("missing sessions directory"))?;
        fs::create_dir_all(dir)?;
        let bytes =
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let (tmp, mut file) = loop {
            let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let tmp = dir.join(format!(
                ".history.json.{}.{}.tmp",
                std::process::id(),
                number
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
            {
                Ok(file) => break (tmp, file),
                Err(error) => {
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
            }
        };
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            let _replace = REPLACE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    pub fn upsert(&mut self, record: SessionRecord) {
        if let Some(slot) =
            self.sessions.iter_mut().find(|row| row.id == record.id)
        {
            *slot = record;
        } else {
            self.sessions.push(record);
        }
        self.sessions
            .sort_by_key(|row| std::cmp::Reverse(row.updated_at));
    }

    pub fn get(&self, id: &str) -> Option<&SessionRecord> {
        self.sessions.iter().find(|row| row.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SessionRecord> {
        self.sessions.iter_mut().find(|row| row.id == id)
    }

    pub fn remove(&mut self, id: &str) -> Option<SessionRecord> {
        let index = self.sessions.iter().position(|row| row.id == id)?;
        let record = self.sessions.remove(index);
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        Some(record)
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionIndex, SessionOrigin, SessionRecord, TurnState};
    use crate::types::{ContentBlock, Message, MessageSource, Role};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("build-test")
                .join(format!(
                    "{tag}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tool_history() -> Vec<Message> {
        vec![
            Message {
                id: "u1".into(),
                role: Role::User,
                content: vec![ContentBlock::text("read the config")],
                source: MessageSource::User,
                display_text: None,
            },
            Message {
                id: "a1".into(),
                role: Role::Assistant,
                content: vec![
                    ContentBlock::text("I will read it."),
                    ContentBlock::ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        // Exact raw argument string must survive a round trip.
                        arguments: r#"{"path":"config/app.json","limit":10}"#
                            .into(),
                    },
                ],
                source: MessageSource::Model {
                    provider: "flowy".into(),
                    model: "AIPC-deepseek-v4-flash".into(),
                },
                display_text: None,
            },
            Message {
                id: "t1".into(),
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_call_id: "call-1".into(),
                    content: vec![ContentBlock::text("{\"port\":8080}")],
                    is_error: Some(false),
                    artifact: None,
                }],
                source: MessageSource::Tool {
                    tool: "read_file".into(),
                },
                display_text: None,
            },
            Message {
                id: "a2".into(),
                role: Role::Assistant,
                content: vec![ContentBlock::text("The port is 8080.")],
                source: MessageSource::Model {
                    provider: "flowy".into(),
                    model: "AIPC-deepseek-v4-flash".into(),
                },
                display_text: None,
            },
        ]
    }

    fn record() -> SessionRecord {
        SessionRecord {
            execution: crate::session::ExecutionMetadata::default(),
            id: "session-1".into(),
            title: "config".into(),
            titled: true,
            updated_at: 42,
            status: TurnState::Completed,
            messages: tool_history(),
            todos: Vec::new(),
            origin: SessionOrigin::Unknown,
            workspace: None,
            delivery_commits: Vec::new(),
            ..SessionRecord::default()
        }
    }

    #[test]
    fn tool_arguments_and_pairing_survive_a_round_trip() {
        let scratch = Scratch::new("roundtrip");
        let mut index = SessionIndex {
            active: Some("session-1".into()),
            ..SessionIndex::default()
        };
        index.upsert(record());
        index.save(&scratch.0).unwrap();

        let restored = SessionIndex::load(&scratch.0).unwrap();
        assert_eq!(restored.active.as_deref(), Some("session-1"));
        let session = restored.get("session-1").unwrap();
        assert_eq!(session, &record());
        let Message { content, .. } = &session.messages[1];
        let ContentBlock::ToolCall { arguments, .. } = &content[1] else {
            panic!("tool call block missing after restore");
        };
        assert_eq!(arguments, r#"{"path":"config/app.json","limit":10}"#);
        let ContentBlock::ToolResult {
            tool_call_id,
            is_error,
            ..
        } = &session.messages[2].content[0]
        else {
            panic!("tool result block missing after restore");
        };
        assert_eq!(tool_call_id, "call-1");
        assert_eq!(*is_error, Some(false));
    }

    #[test]
    fn missing_file_is_an_empty_index_not_an_error() {
        let scratch = Scratch::new("missing");
        let index = SessionIndex::load(&scratch.0).unwrap();
        assert_eq!(index.sessions.as_slice(), []);
        assert!(index.active.is_none());
    }

    #[test]
    fn upsert_replaces_and_orders_by_updated_at() {
        let mut index = SessionIndex::default();
        let mut older = record();
        older.id = "older".into();
        older.updated_at = 1;
        let mut newer = record();
        newer.id = "newer".into();
        newer.updated_at = 2;
        index.upsert(older);
        index.upsert(newer);
        assert_eq!(index.sessions[0].id, "newer");

        let mut updated = record();
        updated.id = "older".into();
        updated.title = "renamed".into();
        updated.updated_at = 3;
        index.upsert(updated);
        assert_eq!(index.sessions.len(), 2);
        assert_eq!(index.get("older").unwrap().title, "renamed");
    }

    #[test]
    fn corrupted_history_is_reported_not_silently_reset() {
        let scratch = Scratch::new("corrupt");
        let dir = scratch.0.join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("history.json"), b"{broken").unwrap();
        assert!(SessionIndex::load(&scratch.0).is_err());
    }

    #[test]
    fn legacy_sessions_have_unknown_origin_and_no_delivery_authority() {
        let record: SessionRecord =
            serde_json::from_str(r#"{"id":"old-session","messages":[]}"#)
                .unwrap();
        assert_eq!(record.origin, SessionOrigin::Unknown);
        assert!(record.workspace.is_none());
        assert_eq!(record.delivery_commits.as_slice(), []);
        let result: ContentBlock = serde_json::from_str(
            r#"{"type":"tool-result","tool_call_id":"old-call","content":[],"is_error":false}"#,
        ).unwrap();
        assert!(matches!(
            result,
            ContentBlock::ToolResult { artifact: None, .. }
        ));
    }

    #[test]
    fn root_session_id_defaults_to_self_and_omits_when_none() {
        let root_record = SessionRecord {
            id: "s-root".into(),
            ..SessionRecord::default()
        };
        assert_eq!(root_record.effective_root_session_id(), "s-root");
        let serialized = serde_json::to_string(&root_record).unwrap();
        assert!(!serialized.contains("root_session_id"));

        let child_record = SessionRecord {
            id: "s-child".into(),
            root_session_id: Some("s-root".into()),
            ..SessionRecord::default()
        };
        assert_eq!(child_record.effective_root_session_id(), "s-root");
        let roundtrip: SessionRecord = serde_json::from_str(
            &serde_json::to_string(&child_record).unwrap(),
        )
        .unwrap();
        assert_eq!(roundtrip.root_session_id.as_deref(), Some("s-root"));
    }

    #[test]
    fn multi_object_work_contract_and_pending_cards_roundtrip_compatibly() {
        use super::{
            DisambiguationOption, FactEntry, FactStatus, ObjectVerifyState,
            PendingCard, StopHandoffManifest, TrackedObject, TrackedObjectKind,
            WorkContract,
        };
        use std::collections::BTreeMap;

        // Legacy WorkContract JSON without `objects` must deserialize cleanly.
        let legacy: WorkContract = serde_json::from_str(
            r#"{"revision":2,"min_slides":3,"pinned_facts":{"quote":"30万"}}"#,
        )
        .unwrap();
        assert_eq!(legacy.revision, 2);
        assert_eq!(legacy.objects, Vec::new());

        let mut ppt_overrides = BTreeMap::new();
        ppt_overrides.insert(
            "date".to_owned(),
            FactEntry {
                value: "2026-12-02".to_owned(),
                status: FactStatus::Confirmed,
                updated_at_ms: 10,
            },
        );
        ppt_overrides.insert(
            "quote".to_owned(),
            FactEntry {
                value: "35万".to_owned(),
                status: FactStatus::PendingProposal,
                updated_at_ms: 11,
            },
        );
        let contract = WorkContract {
            revision: 3,
            required_items: Vec::new(),
            min_slides: Some(3),
            pinned_facts: BTreeMap::from([
                ("quote".to_owned(), "30万".to_owned()),
                ("date".to_owned(), "2026-11-27".to_owned()),
            ]),
            objects: vec![
                TrackedObject {
                    object_id: "obj-ppt-1".to_owned(),
                    kind: TrackedObjectKind::Deliverable,
                    filename: Some("deck.pptx".to_owned()),
                    extension: "pptx".to_owned(),
                    anchor_summary: "deck.pptx · 2026-12-02".to_owned(),
                    overrides: ppt_overrides,
                    verify_state: ObjectVerifyState::Passed {
                        report_id: "rep-1".to_owned(),
                        revision: 3,
                    },
                },
                TrackedObject {
                    object_id: "obj-rec-1".to_owned(),
                    kind: TrackedObjectKind::Record,
                    filename: None,
                    extension: "md".to_owned(),
                    anchor_summary: "会议记录".to_owned(),
                    overrides: BTreeMap::new(),
                    verify_state: ObjectVerifyState::Unverified,
                },
            ],
        };
        assert_eq!(contract.writable_deliverables().len(), 1);
        let facts = contract.confirmed_facts_for("obj-ppt-1");
        assert_eq!(facts.get("date").map(String::as_str), Some("2026-12-02"));
        assert_eq!(
            facts.get("quote").map(String::as_str),
            Some("30万"),
            "PendingProposal must never override confirmed quote"
        );

        let record = SessionRecord {
            id: "s-multi".to_owned(),
            status: TurnState::WaitingForUser,
            work_contract: Some(contract),
            pending_cards: vec![PendingCard::Disambiguation {
                card_id: "card-1".to_owned(),
                created_at_ms: 100,
                prompt: "把刚才那份的报价改成 32 万元。".to_owned(),
                impact_boundary: "只改所选，其余保持原值".to_owned(),
                options: vec![DisambiguationOption {
                    object_id: "obj-ppt-1".to_owned(),
                    label: "deck.pptx".to_owned(),
                    anchor_summary: "deck.pptx · 2026-12-02".to_owned(),
                    extension: "pptx".to_owned(),
                }],
                allow_multi: true,
            }],
            last_stop_handoff: Some(StopHandoffManifest {
                input_revision: 3,
                command_summary: "cancelled".to_owned(),
                modified_files: vec!["deck.pptx".to_owned()],
                verified_effects: Vec::new(),
                pending_checks: vec!["deck.pptx".to_owned()],
                rollback_hint: "retry or restore".to_owned(),
            }),
            ..SessionRecord::default()
        };
        assert!(!record.status.is_busy());
        assert!(TurnState::Stopping.is_busy());
        let decoded: SessionRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap())
                .unwrap();
        assert_eq!(decoded, record);
    }
}
