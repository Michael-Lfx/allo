// JSON stream protocol for host ↔ agent communication.
// Contains: events (agent→host), commands (host→agent), approval manager.

pub mod commands;
pub mod events;
pub mod reader;
pub mod writer;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;

use tokio::sync::oneshot;

use crate::commands::{ApprovalScope, SessionMode};
use crate::events::ToolCategory;

/// Result of a tool approval request
pub enum ToolApprovalResult {
    Approved,
    Denied { reason: String },
}

/// Identity of one concrete pending approval registration.
///
/// Call IDs come from providers and may be reused.  A timeout cleanup must
/// therefore remove only the registration that timed out, not a newer request
/// that happens to use the same ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolApprovalToken(u64);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScopedToolApproval {
    pub approval_id: String,
    pub msg_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub expires_at_ms: i64,
}

#[cfg(test)]
mod scoped_approval_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn exact_message_and_call_are_required_and_the_approval_is_single_use() {
        let manager=ToolApprovalManager::new();
        let (mut rx,_)=manager.request_scoped_approval_with_token("call",&ToolCategory::Edit,"actual-message","Write",serde_json::json!({"file_path":"owned"}),Duration::from_secs(8));
        let id=manager.pending_scoped_approvals()[0].approval_id.clone();
        assert!(!manager.resolve_scoped_once("old-message","call",&id,true));
        assert!(!manager.resolve_scoped_once("actual-message","old-call",&id,true));
        assert!(matches!(rx.try_recv(),Err(oneshot::error::TryRecvError::Empty)));
        manager.approve("call",ApprovalScope::Always);
        manager.resolve("call",ToolApprovalResult::Approved);
        assert!(matches!(rx.try_recv(),Err(oneshot::error::TryRecvError::Empty)),"unscoped approval must not bypass the host scope");
        assert!(manager.resolve_scoped_once("actual-message","call",&id,true));
        assert!(matches!(rx.await,Ok(ToolApprovalResult::Approved)));
        assert!(!manager.resolve_scoped_once("actual-message","call",&id,true));
        assert!(!manager.is_auto_approved("edit"));
    }

    #[tokio::test]
    async fn expiry_refuses_execution_and_does_not_publish_a_live_approval() {
        let manager=ToolApprovalManager::new();
        let (rx,_)=manager.request_scoped_approval_with_token("call",&ToolCategory::Exec,"message","Bash",serde_json::json!({"command":"test"}),Duration::from_millis(10));
        let id=manager.pending_scoped_approvals()[0].approval_id.clone();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(manager.pending_scoped_approvals().is_empty());
        assert!(!manager.resolve_scoped_once("message","call",&id,true));
        assert!(matches!(rx.await,Ok(ToolApprovalResult::Denied {..})));
    }

    #[tokio::test]
    async fn a_reused_call_id_cannot_consume_the_new_messages_approval() {
        let manager=ToolApprovalManager::new();
        let (old,_)=manager.request_scoped_approval_with_token("reused",&ToolCategory::Edit,"message-a","Write",serde_json::json!({}),Duration::from_secs(8));
        let (new,_)=manager.request_scoped_approval_with_token("reused",&ToolCategory::Edit,"message-b","Write",serde_json::json!({}),Duration::from_secs(8));
        let id=manager.pending_scoped_approvals()[0].approval_id.clone();
        assert!(old.await.is_err());
        assert!(!manager.resolve_scoped_once("message-a","reused",&id,true));
        assert!(manager.resolve_scoped_once("message-b","reused",&id,false));
        assert!(matches!(new.await,Ok(ToolApprovalResult::Denied {..})));
    }

    #[tokio::test]
    async fn a_reused_call_with_the_same_message_still_requires_the_new_registration() {
        let manager=ToolApprovalManager::new();
        let (old,_)=manager.request_scoped_approval_with_token("call",&ToolCategory::Edit,"same-message","Write",serde_json::json!({}),Duration::from_secs(8));
        let old_id=manager.pending_scoped_approvals()[0].approval_id.clone();
        let (new,_)=manager.request_scoped_approval_with_token("call",&ToolCategory::Edit,"same-message","Write",serde_json::json!({}),Duration::from_secs(8));
        let new_id=manager.pending_scoped_approvals()[0].approval_id.clone();
        assert_ne!(old_id,new_id);assert!(old.await.is_err());
        assert!(!manager.resolve_scoped_once("same-message","call",&old_id,true));
        assert!(manager.resolve_scoped_once("same-message","call",&new_id,false));
        assert!(matches!(new.await,Ok(ToolApprovalResult::Denied {..})));
    }
}

struct PendingApproval {
    tx: oneshot::Sender<ToolApprovalResult>,
    category: String,
    token: ToolApprovalToken,
    scope: Option<ScopedToolApproval>,
    expires: Option<std::time::Instant>,
}

/// Manages pending tool approval requests using oneshot channels.
///
/// Each pending request also stores its tool category so a client approval with
/// `ApprovalScope::Always` can persist auto-approval for future requests in the
/// same category.
///
/// Also holds the current `SessionMode` which determines which tool categories
/// are auto-approved based on the active approval policy.
pub struct ToolApprovalManager {
    pending: Mutex<HashMap<String, PendingApproval>>,
    auto_approved: Mutex<HashSet<String>>,
    session_mode: Mutex<SessionMode>,
    next_token: Mutex<u64>,
    scope_instance: String,
}

impl ToolApprovalManager {
    pub fn new() -> Self {
        static INSTANCES: std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        let epoch=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let instance=INSTANCES.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        Self {
            pending: Mutex::new(HashMap::new()),
            auto_approved: Mutex::new(HashSet::new()),
            session_mode: Mutex::new(SessionMode::Default),
            next_token: Mutex::new(0),
            scope_instance: format!("{epoch}-{instance}"),
        }
    }

    pub fn request_approval(
        &self,
        call_id: &str,
        category: &ToolCategory,
    ) -> oneshot::Receiver<ToolApprovalResult> {
        self.request_approval_with_token(call_id, category).0
    }

    /// Register an approval and return both its receiver and an exact
    /// registration token for conditional cleanup.
    pub fn request_approval_with_token(
        &self,
        call_id: &str,
        category: &ToolCategory,
    ) -> (
        oneshot::Receiver<ToolApprovalResult>,
        ToolApprovalToken,
    ) {
        self.insert_approval(call_id, category, None, None)
    }

    fn insert_approval(&self, call_id: &str, category: &ToolCategory, mut scope: Option<ScopedToolApproval>, expires: Option<std::time::Instant>) -> (oneshot::Receiver<ToolApprovalResult>, ToolApprovalToken) {
        let (tx, rx) = oneshot::channel();
        let token = {
            let mut next = self.next_token.lock().unwrap_or_else(|e| e.into_inner());
            *next = next.wrapping_add(1);
            ToolApprovalToken(*next)
        };
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(scope)=&mut scope {scope.approval_id=format!("voice:{}:{}:{}",self.scope_instance,token.0,scope.msg_id);}
            pending.insert(
                call_id.to_string(),
                PendingApproval {
                    tx,
                    category: category.to_string(),
                    token,
                    scope,
                    expires,
                },
            );
        }
        (rx, token)
    }

    pub fn request_scoped_approval_with_token(&self, call_id: &str, category: &ToolCategory, msg_id: &str, tool_name: &str, arguments: serde_json::Value, timeout: std::time::Duration) -> (oneshot::Receiver<ToolApprovalResult>, ToolApprovalToken) {
        let expires=std::time::Instant::now()+timeout;
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let scope=ScopedToolApproval {approval_id:String::new(),msg_id:msg_id.into(),call_id:call_id.into(),tool_name:tool_name.into(),arguments,expires_at_ms:(now+timeout.as_millis()).min(i64::MAX as u128) as i64};
        self.insert_approval(call_id,category,Some(scope),Some(expires))
    }

    pub fn pending_scoped_approvals(&self) -> Vec<ScopedToolApproval> {
        self.pending.lock().map(|pending| pending.values().filter(|entry| entry.expires.is_some_and(|expires| expires>std::time::Instant::now())).filter_map(|entry| entry.scope.clone()).collect()).unwrap_or_default()
    }

    pub fn has_pending(&self, call_id: &str) -> bool {
        self.pending.lock().is_ok_and(|pending| pending.contains_key(call_id))
    }

    pub fn is_scoped(&self, call_id: &str) -> bool {
        self.pending.lock().is_ok_and(|pending| pending.get(call_id).is_some_and(|entry| entry.scope.is_some()))
    }

    pub fn resolve_scoped_once(&self, msg_id: &str, call_id: &str, approval_id: &str, approved: bool) -> bool {
        let entry = self.pending.lock().ok().and_then(|mut pending| {
            let entry=pending.get(call_id)?;
            let scope=entry.scope.as_ref()?;
            if scope.msg_id != msg_id || scope.approval_id != approval_id { return None; }
            pending.remove(call_id)
        });
        let Some(entry)=entry else {return false;};
        if entry.expires.is_none_or(|expires| expires<=std::time::Instant::now()) {
            let _=entry.tx.send(ToolApprovalResult::Denied {reason:"approval_expired".into()});
            return false;
        }
        let _=entry.tx.send(if approved {ToolApprovalResult::Approved} else {ToolApprovalResult::Denied {reason:"user_denied".into()}});
        true
    }

    pub fn approve(&self, call_id: &str, scope: ApprovalScope) {
        let pending = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| {
                if pending.get(call_id).is_some_and(|entry| entry.scope.is_some()) {return None;}
                pending.remove(call_id)
            });

        if let Some(pending) = pending {
            if matches!(scope, ApprovalScope::Always) {
                self.add_auto_approve(&pending.category);
            }
            let _ = pending.tx.send(ToolApprovalResult::Approved);
        }
    }

    pub fn resolve(&self, call_id: &str, result: ToolApprovalResult) {
        if let Some(pending) = self.pending.lock().ok().and_then(|mut pending| {
            if matches!(&result,ToolApprovalResult::Approved) && pending.get(call_id).is_some_and(|entry| entry.scope.is_some()) {return None;}
            pending.remove(call_id)
        })
        {
            let _ = pending.tx.send(result);
        }
    }

    pub fn is_auto_approved(&self, category: &str) -> bool {
        // Check session mode first
        let mode_approved = self
            .session_mode
            .lock()
            .map(|mode| match *mode {
                SessionMode::Yolo => true,
                SessionMode::AutoEdit => category == "info" || category == "edit",
                SessionMode::Default => false,
            })
            .unwrap_or(false);

        if mode_approved {
            return true;
        }

        // Fall back to per-category "always" approvals
        self.auto_approved
            .lock()
            .map(|auto| auto.contains(category))
            .unwrap_or(false)
    }

    /// Set the session approval mode. Takes effect immediately.
    pub fn set_mode(&self, mode: SessionMode) {
        if let Ok(mut current) = self.session_mode.lock() {
            *current = mode;
        }
    }

    /// **P3-X1: does the *current* session mode bypass tool-execution approval entirely?**
    ///
    /// This is the LIVE, runtime-flippable analogue of `config.tools.auto_approve`: it reads
    /// the current `session_mode` (mutated by [`Self::set_mode`], which takes effect
    /// immediately) and answers whether *every* tool category — including
    /// [`ToolCategory::Irreversible`] — is auto-approved.
    ///
    /// **Mapping (the F1-sec redline direction, authoritative here so it lives in one place):**
    /// - [`SessionMode::Yolo`] → `true` (bypasses approval for all categories, including
    ///   irreversible — this is exactly when the browser facade's independent fail-closed
    ///   redline gate must arm);
    /// - [`SessionMode::AutoEdit`] → **`false`** — auto-edit auto-approves only `info`/`edit`,
    ///   **never** `exec`/`mcp`/irreversible, so the tool-execution approval gate still fires for
    ///   an irreversible web action; it does NOT bypass approval and must NOT arm the redline gate;
    /// - [`SessionMode::Default`] → `false`.
    ///
    /// This mirrors the `Yolo => true` arm of [`Self::is_auto_approved`] (which is `true` for
    /// every category iff yolo), keeping "bypasses approval" === "yolo" in a single definition.
    /// Per-category user "always" approvals are intentionally NOT consulted: a manual
    /// `add_auto_approve("exec")` is a scoped grant, not a wholesale approval bypass, so it must
    /// not arm the facade redline gate against irreversible actions.
    pub fn session_bypasses_approval(&self) -> bool {
        self.session_mode
            .lock()
            .map(|mode| matches!(*mode, SessionMode::Yolo))
            .unwrap_or(false)
    }

    /// Return the current session mode as a string for capability reporting.
    pub fn current_mode(&self) -> String {
        self.session_mode
            .lock()
            .map(|mode| match *mode {
                SessionMode::Default => "default",
                SessionMode::AutoEdit => "auto_edit",
                SessionMode::Yolo => "yolo",
            })
            .unwrap_or("default")
            .to_string()
    }

    pub fn drop_pending(&self, call_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(call_id);
        }
    }

    /// Remove a pending approval only if it is still the registration
    /// represented by `token`.
    pub fn drop_pending_if(&self, call_id: &str, token: ToolApprovalToken) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let matches = pending
            .get(call_id)
            .map(|entry| entry.token == token)
            .unwrap_or(false);
        if matches {
            pending.remove(call_id);
        }
        matches
    }

    pub fn add_auto_approve(&self, category: &str) {
        if let Ok(mut auto) = self.auto_approved.lock() {
            auto.insert(category.to_string());
        }
    }
}

impl Default for ToolApprovalManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SessionMode: default mode ---

    #[test]
    fn default_mode_does_not_auto_approve_any_category() {
        let mgr = ToolApprovalManager::new();
        assert!(!mgr.is_auto_approved("info"));
        assert!(!mgr.is_auto_approved("edit"));
        assert!(!mgr.is_auto_approved("exec"));
        assert!(!mgr.is_auto_approved("mcp"));
    }

    #[test]
    fn default_mode_current_mode_string() {
        let mgr = ToolApprovalManager::new();
        assert_eq!(mgr.current_mode(), "default");
    }

    // --- SessionMode: auto_edit mode ---

    #[test]
    fn auto_edit_mode_approves_info_and_edit() {
        let mgr = ToolApprovalManager::new();
        mgr.set_mode(SessionMode::AutoEdit);
        assert!(mgr.is_auto_approved("info"));
        assert!(mgr.is_auto_approved("edit"));
    }

    #[test]
    fn auto_edit_mode_requires_approval_for_exec_and_mcp() {
        let mgr = ToolApprovalManager::new();
        mgr.set_mode(SessionMode::AutoEdit);
        assert!(!mgr.is_auto_approved("exec"));
        assert!(!mgr.is_auto_approved("mcp"));
    }

    #[test]
    fn auto_edit_mode_current_mode_string() {
        let mgr = ToolApprovalManager::new();
        mgr.set_mode(SessionMode::AutoEdit);
        assert_eq!(mgr.current_mode(), "auto_edit");
    }

    // --- SessionMode: yolo mode ---

    #[test]
    fn yolo_mode_approves_all_categories() {
        let mgr = ToolApprovalManager::new();
        mgr.set_mode(SessionMode::Yolo);
        assert!(mgr.is_auto_approved("info"));
        assert!(mgr.is_auto_approved("edit"));
        assert!(mgr.is_auto_approved("exec"));
        assert!(mgr.is_auto_approved("mcp"));
    }

    #[test]
    fn yolo_mode_current_mode_string() {
        let mgr = ToolApprovalManager::new();
        mgr.set_mode(SessionMode::Yolo);
        assert_eq!(mgr.current_mode(), "yolo");
    }

    // --- Mode switching ---

    #[test]
    fn switching_mode_changes_approval_behavior() {
        let mgr = ToolApprovalManager::new();

        // Start in default
        assert!(!mgr.is_auto_approved("edit"));

        // Switch to auto_edit
        mgr.set_mode(SessionMode::AutoEdit);
        assert!(mgr.is_auto_approved("edit"));
        assert!(!mgr.is_auto_approved("exec"));

        // Switch to yolo
        mgr.set_mode(SessionMode::Yolo);
        assert!(mgr.is_auto_approved("exec"));

        // Switch back to default
        mgr.set_mode(SessionMode::Default);
        assert!(!mgr.is_auto_approved("edit"));
        assert!(!mgr.is_auto_approved("exec"));
    }

    // --- Mode + user "always" approval coexistence ---

    #[test]
    fn user_always_approval_persists_across_mode_changes() {
        let mgr = ToolApprovalManager::new();

        // User manually approves "exec" category with "always"
        mgr.add_auto_approve("exec");
        assert!(mgr.is_auto_approved("exec"));

        // Switch to auto_edit: exec still approved via user "always"
        mgr.set_mode(SessionMode::AutoEdit);
        assert!(mgr.is_auto_approved("exec"));
        assert!(mgr.is_auto_approved("info")); // from mode

        // Switch back to default: exec still approved via user "always"
        mgr.set_mode(SessionMode::Default);
        assert!(mgr.is_auto_approved("exec"));
        assert!(!mgr.is_auto_approved("info")); // mode no longer provides this
    }

    // --- P3-X1: session_bypasses_approval (the LIVE redline-arming query) ---

    #[test]
    fn session_bypasses_approval_only_yolo_bypasses() {
        let mgr = ToolApprovalManager::new();
        // Default: never bypasses.
        assert!(!mgr.session_bypasses_approval());

        // AutoEdit auto-approves info/edit only — it does NOT bypass approval (irreversible
        // still gated). The facade redline gate must NOT arm here.
        mgr.set_mode(SessionMode::AutoEdit);
        assert!(!mgr.session_bypasses_approval());

        // Yolo bypasses approval for every category, including irreversible → arms the gate.
        mgr.set_mode(SessionMode::Yolo);
        assert!(mgr.session_bypasses_approval());

        // Flipping back to default un-arms it (LIVE — set_mode takes effect immediately).
        mgr.set_mode(SessionMode::Default);
        assert!(!mgr.session_bypasses_approval());
    }

    #[test]
    fn session_bypasses_approval_ignores_per_category_always_grants() {
        let mgr = ToolApprovalManager::new();
        // A scoped "always exec" grant is NOT a wholesale approval bypass — it must not arm the
        // facade redline gate against irreversible actions (only yolo does).
        mgr.add_auto_approve("exec");
        assert!(mgr.is_auto_approved("exec"));
        assert!(!mgr.session_bypasses_approval());
    }
}
