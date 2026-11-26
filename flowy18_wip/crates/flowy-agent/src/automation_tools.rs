//! Trusted conversation command transport.
mod definitions;
use flowy_agent_core::{ToolCallArgs, ToolRunContext, tools::ToolRegistry};
use flowy_automation::{
    AutomationCommand, AutomationHandle, AutomationQuery, CallerContext,
    CallerSource, CommandEnvelope, CommandReceipt, PortError, PrincipalId,
    QuerySnapshot, RequestKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

pub const MAX_TOOL_REQUESTS: usize = 4096;
pub const MAX_TOOL_REQUEST_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;

/// Logical side-store references for the host audit/retention inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationRequestInventory {
    pub principal: PrincipalId,
    pub session_id: String,
    pub submission_id: String,
    pub request_key: RequestKey,
    pub expires_at_ms: i64,
}

/// Constructed by the application host, which owns current account state and
/// validates the session/submission against durable engine history.
pub type AutomationCallerFactory = Arc<
    dyn Fn(&ToolRunContext, &ToolCallArgs) -> Result<CallerContext, PortError>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct AutomationToolBridge {
    inner: Arc<BridgeInner>,
    caller: AutomationCallerFactory,
}

struct BridgeInner {
    handle: AutomationHandle,
    journal: PathBuf,
    requests: Mutex<RequestJournal>,
    _owner: Arc<dyn Send + Sync>,
    _journal_lease: Arc<()>,
}

impl std::fmt::Debug for AutomationToolBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationToolBridge")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedRequest {
    principal: PrincipalId,
    session_id: String,
    submission_id: String,
    call_id: String,
    envelope: CommandEnvelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RequestJournal {
    version: u32,
    requests: Vec<SavedRequest>,
}

impl AutomationToolBridge {
    /// One bridge per store owner; clones share the same serialization lock.
    /// Existing corrupt or future journals fail closed and remain untouched.
    pub fn open(
        root: &Path,
        handle: AutomationHandle,
        caller: AutomationCallerFactory,
        owner: Arc<dyn Send + Sync>,
    ) -> Result<Self, PortError> {
        std::fs::create_dir_all(root).map_err(|_| PortError::Storage)?;
        let root =
            std::fs::canonicalize(root).map_err(|_| PortError::Storage)?;
        let lease = journal_lease(&root)?;
        let journal = root
            .join(flowy_automation::schema_contract::TOOL_REQUEST_JOURNAL_PATH);
        if std::fs::metadata(&journal).is_ok_and(|metadata| {
            metadata.len() > MAX_TOOL_REQUEST_JOURNAL_BYTES
        }) {
            return Err(PortError::Storage);
        }
        let requests = match std::fs::read(&journal) {
            Ok(bytes) => {
                let requests: RequestJournal =
                    serde_json::from_slice(&bytes)
                        .map_err(|_| PortError::Storage)?;
                validate_journal(&requests)?;
                requests
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                RequestJournal {
                    version: 1,
                    requests: vec![],
                }
            }
            Err(_) => return Err(PortError::Storage),
        };
        Ok(Self {
            inner: Arc::new(BridgeInner {
                handle,
                journal,
                requests: Mutex::new(requests),
                _owner: owner,
                _journal_lease: lease,
            }),
            caller,
        })
    }

    #[must_use]
    pub fn with_caller(&self, caller: AutomationCallerFactory) -> Self {
        Self {
            inner: self.inner.clone(),
            caller,
        }
    }

    /// Install alongside the old Policy. The guard acts only on management
    /// tools and cannot turn an earlier Policy denial into permission.
    pub fn install_context_guard(&self, registry: &ToolRegistry) {
        let caller = self.caller.clone();
        registry.add_context_guard(Arc::new(move |context, args| {
            if !args.name.starts_with("automation_") {
                return None;
            }
            validate_model_arguments(&args.arguments)
                .and_then(|()| {
                    trusted_caller(caller.as_ref(), context, args).map(|_| ())
                })
                .err()
                .map(|_| "automation caller is not authorized".to_owned())
        }));
    }

    /// Expired entries remain until their source session is durably removed.
    /// Capacity exhaustion refuses new envelopes instead of silently renewing
    /// an old call after purging its request identity.
    pub fn request_inventory(
        &self,
    ) -> Result<Vec<AutomationRequestInventory>, PortError> {
        let journal =
            self.inner.requests.lock().map_err(|_| PortError::Storage)?;
        validate_journal(&journal)?;
        Ok(journal
            .requests
            .iter()
            .map(|saved| AutomationRequestInventory {
                principal: saved.principal.clone(),
                session_id: saved.session_id.clone(),
                submission_id: saved.submission_id.clone(),
                request_key: saved.envelope.request_key.clone(),
                expires_at_ms: saved.envelope.expires_at_ms,
            })
            .collect())
    }

    /// Drop request identities only after their source conversation is
    /// durably removed. Never prune by age: a retry of an expired call must
    /// recover the original envelope instead of receiving a fresh one.
    pub(crate) fn remove_requests_for_session(
        &self,
        principal: &PrincipalId,
        session_id: &str,
    ) -> Result<(), PortError> {
        let mut journal =
            self.inner.requests.lock().map_err(|_| PortError::Storage)?;
        validate_journal(&journal)?;
        let mut next = journal.clone();
        next.requests.retain(|request| {
            request.principal != *principal || request.session_id != session_id
        });
        if next.requests.len() == journal.requests.len() {
            return Ok(());
        }
        validate_journal(&next)?;
        crate::index::save_json(&self.inner.journal, &next)
            .map_err(|_| PortError::Storage)?;
        *journal = next;
        drop(journal);
        Ok(())
    }

    /// Persist the host-stamped envelope before transmission. Retrying the
    /// same durable call never renews its time, ID or command digest.
    pub fn envelope(
        &self,
        context: &ToolRunContext,
        args: &ToolCallArgs,
        command: AutomationCommand,
        issued_at_ms: i64,
        expires_at_ms: i64,
    ) -> Result<(CallerContext, CommandEnvelope), PortError> {
        validate_model_arguments(&args.arguments)?;
        let caller = trusted_caller(self.caller.as_ref(), context, args)?;
        let trusted = context.trusted_call(args).ok_or(PortError::Forbidden)?;
        let mut journal =
            self.inner.requests.lock().map_err(|_| PortError::Storage)?;
        if let Some(saved) = journal.requests.iter().find(|saved| {
            saved.principal == caller.principal
                && saved.session_id == trusted.session_id
                && saved.submission_id == trusted.submission_id
                && saved.call_id == trusted.call_id
        }) {
            if saved.envelope.command != command {
                return Err(PortError::RevisionConflict);
            }
            return Ok((caller, saved.envelope.clone()));
        }
        if journal.requests.len() >= MAX_TOOL_REQUESTS {
            return Err(PortError::Storage);
        }
        let key = request_key(
            &caller.principal,
            &trusted.session_id,
            &trusted.submission_id,
            &trusted.call_id,
        )?;
        let envelope =
            CommandEnvelope::new(key, issued_at_ms, expires_at_ms, command)?;
        let mut next = journal.clone();
        next.requests.push(SavedRequest {
            principal: caller.principal.clone(),
            session_id: trusted.session_id,
            submission_id: trusted.submission_id,
            call_id: trusted.call_id,
            envelope: envelope.clone(),
        });
        if u64::try_from(
            serde_json::to_vec_pretty(&next)
                .map_err(|_| PortError::Storage)?
                .len(),
        )
        .map_err(|_| PortError::Storage)?
            > MAX_TOOL_REQUEST_JOURNAL_BYTES
        {
            return Err(PortError::Storage);
        }
        crate::index::save_json(&self.inner.journal, &next)
            .map_err(|_| PortError::Storage)?;
        *journal = next;
        drop(journal);
        Ok((caller, envelope))
    }

    pub async fn command(
        &self,
        context: &ToolRunContext,
        args: &ToolCallArgs,
        command: AutomationCommand,
        issued_at_ms: i64,
        expires_at_ms: i64,
    ) -> Result<CommandReceipt, PortError> {
        let (caller, envelope) =
            self.envelope(context, args, command, issued_at_ms, expires_at_ms)?;
        self.inner.handle.command(&caller, envelope).await
    }

    pub async fn query(
        &self,
        context: &ToolRunContext,
        args: &ToolCallArgs,
        query: AutomationQuery,
    ) -> Result<QuerySnapshot, PortError> {
        validate_model_arguments(&args.arguments)?;
        let caller = trusted_caller(self.caller.as_ref(), context, args)?;
        self.inner.handle.query(&caller, query).await
    }
}

fn journal_lease(root: &Path) -> Result<Arc<()>, PortError> {
    static OWNERS: OnceLock<
        Mutex<std::collections::HashMap<PathBuf, Weak<()>>>,
    > = OnceLock::new();
    let mut owners = OWNERS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .map_err(|_| PortError::Storage)?;
    if owners.get(root).and_then(Weak::upgrade).is_some() {
        return Err(PortError::Unavailable);
    }
    owners.retain(|_, owner| owner.strong_count() != 0);
    let lease = Arc::new(());
    owners.insert(root.to_path_buf(), Arc::downgrade(&lease));
    drop(owners);
    Ok(lease)
}

fn request_key(
    principal: &PrincipalId,
    session: &str,
    submission: &str,
    call: &str,
) -> Result<RequestKey, PortError> {
    let binding = serde_json::to_vec(&(principal, session, submission, call))
        .map_err(|_| PortError::InvalidRequest)?;
    let digest = Sha256::digest(binding);
    RequestKey::parse(format!("automation:{digest:x}"))
        .map_err(|_| PortError::InvalidRequest)
}

fn validate_journal(journal: &RequestJournal) -> Result<(), PortError> {
    if journal.version != 1 || journal.requests.len() > MAX_TOOL_REQUESTS {
        return Err(PortError::Storage);
    }
    let mut keys = std::collections::HashSet::new();
    for saved in &journal.requests {
        saved
            .envelope
            .verify_digest()
            .map_err(|_| PortError::Storage)?;
        if saved.envelope.expires_at_ms <= saved.envelope.issued_at_ms
            || saved.envelope.request_key
                != request_key(
                    &saved.principal,
                    &saved.session_id,
                    &saved.submission_id,
                    &saved.call_id,
                )?
            || !keys.insert(saved.envelope.request_key.clone())
            || flowy_automation::SessionRef::parse(saved.session_id.clone())
                .is_err()
            || flowy_automation::SubmissionRef::parse(
                saved.submission_id.clone(),
            )
            .is_err()
            || RequestKey::parse(saved.call_id.clone()).is_err()
        {
            return Err(PortError::Storage);
        }
    }
    Ok(())
}

fn trusted_caller(
    factory: &(
         dyn Fn(
        &ToolRunContext,
        &ToolCallArgs,
    ) -> Result<CallerContext, PortError>
             + Send
             + Sync
     ),
    context: &ToolRunContext,
    args: &ToolCallArgs,
) -> Result<CallerContext, PortError> {
    if context.signal.is_cancelled() {
        return Err(PortError::Forbidden);
    }
    let trusted = context.trusted_call(args).ok_or(PortError::Forbidden)?;
    let caller = factory(context, args)?;
    match &caller.source {
        CallerSource::TrustedSubmission {
            session_id,
            submission_id,
            call_id,
        } if session_id.as_str() == trusted.session_id
            && submission_id.as_str() == trusted.submission_id
            && call_id == &trusted.call_id =>
        {
            Ok(caller)
        }
        _ => Err(PortError::Forbidden),
    }
}

fn validate_model_arguments(
    value: &serde_json::Value,
) -> Result<(), PortError> {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "owner"
                        | "principal"
                        | "account_generation"
                        | "generation"
                        | "authorized"
                        | "grant_refs"
                        | "caller"
                        | "request_key"
                        | "issued_at_ms"
                        | "expires_at_ms"
                        | "content_digest"
                        | "submission_id"
                        | "call_id"
                        | "session_id"
                ) {
                    return Err(PortError::InvalidRequest);
                }
                validate_model_arguments(value)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_model_arguments(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;
