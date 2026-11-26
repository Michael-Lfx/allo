//! Durable, account-owned approvals for background automation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flowy_automation::{
    AccountGeneration, CapabilityRequest, GrantId, PrincipalId, RequestKey,
    Revision,
};
use serde::{Deserialize, Serialize};

const FORMAT_VERSION: u32 = 1;
const RELATIVE_PATH: &str = "automation/grants.json";

/// A host-stamped request. The command receipt can recover only from this
/// same principal, request key, digest, and revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRequest {
    pub principal: PrincipalId,
    pub account_generation: AccountGeneration,
    pub request_key: RequestKey,
    pub content_digest: [u8; 32],
    pub expected_revision: Option<Revision>,
    pub requested_at_ms: i64,
    pub request_expires_at_ms: i64,
    pub grant_expires_at_ms: i64,
    pub scope: CapabilityRequest,
}

/// A lease binds the grant version that was current when the run started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantBinding {
    pub grant_id: GrantId,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantStatus {
    Approved(GrantBinding),
    Denied,
    Revoked,
    Expired,
}

#[derive(Debug, thiserror::Error)]
pub enum GrantError {
    #[error("automation grant storage unavailable")]
    Storage(#[source] io::Error),
    #[error("automation grant storage is unavailable")]
    Unavailable,
    #[error("automation grant ledger is invalid")]
    Corrupt,
    #[error("automation grant ledger version is unsupported")]
    UnsupportedVersion,
    #[error("automation grant request parameters are invalid")]
    InvalidRequest,
    #[error("automation grant request has expired")]
    RequestExpired,
    #[error("automation grant request binding conflicts")]
    RequestConflict,
    #[error("automation grant is unknown")]
    UnknownGrant,
    #[error("automation grant has been revoked")]
    Revoked,
    #[error("automation grant has expired")]
    Expired,
    #[error("automation grant version is no longer current")]
    VersionMismatch,
    #[error("automation grant does not cover the requested scope")]
    ScopeDenied,
    #[error("automation account is unavailable")]
    AccountUnavailable,
    #[error("automation grant belongs to another account")]
    OwnerMismatch,
    #[error("automation approval callback is no longer current")]
    LeaseExpired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Decision {
    Approved {
        grant_id: GrantId,
        version: u64,
        revoked_at_ms: Option<i64>,
        expired_at_ms: Option<i64>,
    },
    Denied,
}

impl Decision {
    const fn was_approved(&self) -> bool {
        matches!(self, Self::Approved { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    request: GrantRequest,
    decided_at_ms: i64,
    decision: Decision,
}

impl Entry {
    fn status(&self, now_ms: i64) -> GrantStatus {
        match &self.decision {
            Decision::Denied => GrantStatus::Denied,
            Decision::Approved {
                revoked_at_ms: Some(_),
                ..
            } => GrantStatus::Revoked,
            Decision::Approved {
                expired_at_ms: Some(_),
                ..
            } => GrantStatus::Expired,
            Decision::Approved {
                grant_id, version, ..
            } => {
                if now_ms >= self.request.grant_expires_at_ms
                    || now_ms < self.decided_at_ms
                {
                    return GrantStatus::Expired;
                }
                GrantStatus::Approved(GrantBinding {
                    grant_id: grant_id.clone(),
                    version: *version,
                })
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format_version: u32,
    entries: Vec<Entry>,
}

#[derive(Clone, Default)]
struct LedgerState {
    decisions: BTreeMap<(PrincipalId, RequestKey), Entry>,
    grants: BTreeMap<GrantId, (PrincipalId, RequestKey)>,
    // A failed revocation cannot permit further work in this host lifetime.
    // Its storage error is still returned; no durable success is invented.
    unavailable: bool,
}

/// One `AppHost` owns this ledger under its existing exclusive data-root lock.
/// Each transaction publishes a new in-memory snapshot only after storage
/// accepts it, so failed writes never acknowledge an approval or revocation.
#[derive(Clone)]
pub(super) struct GrantLedger {
    path: Arc<PathBuf>,
    state: Arc<Mutex<LedgerState>>,
    // A gate may outlive the AppHost handle. Keep its root lock alive while
    // any clone can still check or persist an expiry/revocation tombstone.
    storage_owner: Option<Arc<dyn Send + Sync>>,
}

impl fmt::Debug for GrantLedger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GrantLedger")
            .finish_non_exhaustive()
    }
}

impl GrantLedger {
    pub(super) fn retaining_store_owner(
        mut self,
        storage_owner: Arc<dyn Send + Sync>,
    ) -> Self {
        self.storage_owner = Some(storage_owner);
        self
    }

    pub(super) fn available(&self) -> bool {
        !self.state.lock().unwrap().unavailable
    }

    pub(super) fn open(root: &Path) -> Result<Self, GrantError> {
        let path = root.join(RELATIVE_PATH);
        let state = match fs::read(&path) {
            Ok(bytes) => decode(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Windows may report PathNotFound for a file occupying a
                // parent component. That is not an absent, usable ledger.
                match fs::metadata(root.join("automation")) {
                    Ok(metadata) if metadata.is_dir() => {}
                    Ok(_) => {
                        return Err(GrantError::Storage(io::Error::new(
                            io::ErrorKind::NotADirectory,
                            "automation grant parent is not a directory",
                        )));
                    }
                    Err(parent_error)
                        if parent_error.kind() == io::ErrorKind::NotFound => {}
                    Err(parent_error) => {
                        return Err(GrantError::Storage(parent_error));
                    }
                }
                LedgerState::default()
            }
            Err(error) => return Err(GrantError::Storage(error)),
        };
        Ok(Self {
            path: Arc::new(path),
            state: Arc::new(Mutex::new(state)),
            storage_owner: None,
        })
    }

    /// Approve or decline a request exactly once. A matching replay reads
    /// current status; it cannot reset the expiry or undo a revocation.
    pub(super) fn decide(
        &self,
        mut request: GrantRequest,
        approved: bool,
        now_ms: i64,
    ) -> Result<GrantStatus, GrantError> {
        normalize_request(&mut request)?;
        if approved && !scope_allowed(&request.scope) {
            return Err(GrantError::ScopeDenied);
        }
        let key = request_key(&request);
        let mut state = self.state.lock().unwrap();
        state.ensure_available()?;
        self.expire(&mut state, now_ms)?;
        if let Some(entry) = state.decisions.get(&key) {
            verify_request(entry, &request)?;
            if entry.decision.was_approved() != approved {
                return Err(GrantError::RequestConflict);
            }
            return Ok(entry.status(now_ms));
        }
        if now_ms < request.requested_at_ms {
            return Err(GrantError::InvalidRequest);
        }
        if now_ms >= request.request_expires_at_ms
            || now_ms >= request.grant_expires_at_ms
        {
            return Err(GrantError::RequestExpired);
        }
        if state.decisions.len() >= 10_000 {
            return Err(GrantError::Unavailable);
        }

        let mut next = state.clone();
        let decision = if approved {
            let grant_id = GrantId::generate();
            if next.grants.insert(grant_id.clone(), key.clone()).is_some() {
                return Err(GrantError::Corrupt);
            }
            Decision::Approved {
                grant_id,
                version: 1,
                revoked_at_ms: None,
                expired_at_ms: None,
            }
        } else {
            Decision::Denied
        };
        let entry = Entry {
            request,
            decided_at_ms: now_ms,
            decision,
        };
        let status = entry.status(now_ms);
        next.decisions.insert(key, entry);
        self.persist(&next)?;
        *state = next;
        drop(state);
        Ok(status)
    }

    /// Look up the exact prior decision for cross-store receipt recovery.
    pub(super) fn status_for_request(
        &self,
        mut request: GrantRequest,
        now_ms: i64,
    ) -> Result<Option<GrantStatus>, GrantError> {
        normalize_request(&mut request)?;
        let mut state = self.state.lock().unwrap();
        state.ensure_available()?;
        self.expire(&mut state, now_ms)?;
        let Some(entry) = state.decisions.get(&request_key(&request)) else {
            return Ok(None);
        };
        verify_request(entry, &request)?;
        let status = entry.status(now_ms);
        drop(state);
        Ok(Some(status))
    }

    /// Recover the exact already-written decision after an account epoch changes.
    pub(super) fn recover_command_decision(
        &self,
        mut request: GrantRequest,
        now_ms: i64,
    ) -> Result<Option<GrantStatus>, GrantError> {
        normalize_request(&mut request)?;
        let mut state = self.state.lock().unwrap();
        state.ensure_available()?;
        self.expire(&mut state, now_ms)?;
        let Some(entry) = state.decisions.get(&request_key(&request)) else {
            return Ok(None);
        };
        request.account_generation = entry.request.account_generation.clone();
        verify_request(entry, &request)?;
        let status = entry.status(now_ms);
        drop(state);
        Ok(Some(status))
    }

    pub(super) fn revoke(
        &self,
        principal: &PrincipalId,
        grant_id: &GrantId,
        now_ms: i64,
    ) -> Result<bool, GrantError> {
        let mut state = self.state.lock().unwrap();
        state.ensure_available()?;
        let key = state.key_for(principal, grant_id)?.clone();
        let mut next = state.clone();
        let entry = next.decisions.get_mut(&key).ok_or(GrantError::Corrupt)?;
        let Decision::Approved {
            version,
            revoked_at_ms,
            ..
        } = &mut entry.decision
        else {
            return Err(GrantError::Corrupt);
        };
        if revoked_at_ms.is_some() {
            return Ok(false);
        }
        if now_ms < entry.decided_at_ms {
            return Err(GrantError::InvalidRequest);
        }
        *version = version.checked_add(1).ok_or(GrantError::Corrupt)?;
        *revoked_at_ms = Some(now_ms);
        if let Err(error) = self.persist(&next) {
            state.unavailable = true;
            return Err(error);
        }
        *state = next;
        drop(state);
        Ok(true)
    }

    /// Resolve the current binding from a durable reference before lease
    /// issuance; each dispatch must still use check with this exact version.
    pub(super) fn binding(
        &self,
        principal: &PrincipalId,
        grant_id: &GrantId,
        now_ms: i64,
    ) -> Result<GrantBinding, GrantError> {
        let mut state = self.state.lock().unwrap();
        state.ensure_available()?;
        self.expire(&mut state, now_ms)?;
        state.binding(principal, grant_id, now_ms)
    }

    /// Recheck same owner, version, expiry, revocation and scope each time
    /// the host starts externally visible model or tool work.
    pub(super) fn check(
        &self,
        principal: &PrincipalId,
        binding: &GrantBinding,
        requested: &CapabilityRequest,
        now_ms: i64,
    ) -> Result<(), GrantError> {
        let mut state = self.state.lock().unwrap();
        state.ensure_available()?;
        self.expire(&mut state, now_ms)?;
        let current = state.binding(principal, &binding.grant_id, now_ms)?;
        if current.version != binding.version {
            return Err(GrantError::VersionMismatch);
        }
        let key = state.key_for(principal, &binding.grant_id)?;
        let entry = state.decisions.get(key).ok_or(GrantError::Corrupt)?;
        if requested
            .grant_ref
            .as_ref()
            .is_some_and(|id| id != &binding.grant_id)
            || !covers(&entry.request.scope, requested)
        {
            return Err(GrantError::ScopeDenied);
        }
        drop(state);
        Ok(())
    }

    /// Include revoked records as historic references in the SQLite audit.
    pub(super) fn inventory(&self) -> Vec<(PrincipalId, GrantId)> {
        self.state
            .lock()
            .unwrap()
            .grants
            .iter()
            .map(|(id, (owner, _))| (owner.clone(), id.clone()))
            .collect()
    }

    pub(super) fn prune_retired(
        &self,
        principal: &PrincipalId,
        refs: &std::collections::BTreeSet<GrantId>,
        now_ms: i64,
    ) -> Result<(), GrantError> {
        let mut state = self.state.lock().unwrap();
        state.ensure_available()?;
        self.expire(&mut state, now_ms)?;
        let cutoff = now_ms.saturating_sub(7 * 86_400_000);
        let retired = state
            .decisions
            .iter()
            .filter_map(|(key, entry)| {
                if &entry.request.principal != principal
                    || entry.request.request_expires_at_ms > cutoff
                {
                    return None;
                }
                let grant = match &entry.decision {
                    Decision::Approved {
                        grant_id,
                        revoked_at_ms,
                        expired_at_ms,
                        ..
                    } if revoked_at_ms
                        .or(*expired_at_ms)
                        .is_some_and(|at| at <= cutoff)
                        && !refs.contains(grant_id) =>
                    {
                        Some(grant_id.clone())
                    }
                    Decision::Denied if entry.decided_at_ms <= cutoff => None,
                    _ => return None,
                };
                Some((key.clone(), grant))
            })
            .collect::<Vec<_>>();
        if retired.is_empty() {
            return Ok(());
        }
        let mut next = state.clone();
        for (key, grant) in retired {
            next.decisions.remove(&key);
            if let Some(grant) = grant {
                next.grants.remove(&grant);
            }
        }
        self.persist(&next)?;
        *state = next;
        drop(state);
        Ok(())
    }

    /// Expiry observed by this host is a durable tombstone, so rewinding the
    /// wall clock or restarting cannot revive an already expired decision.
    fn expire(
        &self,
        state: &mut LedgerState,
        now_ms: i64,
    ) -> Result<(), GrantError> {
        let mut next = None;
        for (key, entry) in &state.decisions {
            if now_ms >= entry.request.grant_expires_at_ms
                && matches!(
                    entry.decision,
                    Decision::Approved {
                        revoked_at_ms: None,
                        expired_at_ms: None,
                        ..
                    }
                )
            {
                let next = next.get_or_insert_with(|| state.clone());
                let Some(Entry {
                    decision: Decision::Approved { expired_at_ms, .. },
                    ..
                }) = next.decisions.get_mut(key)
                else {
                    return Err(GrantError::Corrupt);
                };
                *expired_at_ms = Some(now_ms);
            }
        }
        if let Some(next) = next {
            if let Err(error) = self.persist(&next) {
                state.unavailable = true;
                return Err(error);
            }
            *state = next;
        }
        Ok(())
    }

    fn persist(&self, state: &LedgerState) -> Result<(), GrantError> {
        let document = Document {
            format_version: FORMAT_VERSION,
            entries: state.decisions.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|_| GrantError::Corrupt)?;
        crate::disk::commit(&self.path, &bytes).map_err(GrantError::Storage)
    }
}

impl LedgerState {
    const fn ensure_available(&self) -> Result<(), GrantError> {
        if self.unavailable {
            Err(GrantError::Unavailable)
        } else {
            Ok(())
        }
    }

    fn key_for(
        &self,
        principal: &PrincipalId,
        grant_id: &GrantId,
    ) -> Result<&(PrincipalId, RequestKey), GrantError> {
        self.grants
            .get(grant_id)
            .filter(|(owner, _)| owner == principal)
            .ok_or(GrantError::UnknownGrant)
    }

    fn binding(
        &self,
        principal: &PrincipalId,
        grant_id: &GrantId,
        now_ms: i64,
    ) -> Result<GrantBinding, GrantError> {
        let key = self.key_for(principal, grant_id)?;
        let entry = self.decisions.get(key).ok_or(GrantError::Corrupt)?;
        match entry.status(now_ms) {
            GrantStatus::Approved(binding) => Ok(binding),
            GrantStatus::Revoked => Err(GrantError::Revoked),
            GrantStatus::Expired => Err(GrantError::Expired),
            GrantStatus::Denied => Err(GrantError::Corrupt),
        }
    }
}

fn request_key(request: &GrantRequest) -> (PrincipalId, RequestKey) {
    (request.principal.clone(), request.request_key.clone())
}

fn verify_request(
    entry: &Entry,
    request: &GrantRequest,
) -> Result<(), GrantError> {
    if &entry.request == request {
        Ok(())
    } else {
        Err(GrantError::RequestConflict)
    }
}

fn normalize_request(request: &mut GrantRequest) -> Result<(), GrantError> {
    if request.requested_at_ms < 0
        || request.request_expires_at_ms <= request.requested_at_ms
        || request.grant_expires_at_ms <= request.requested_at_ms
        || request.scope.grant_ref.is_some()
    {
        return Err(GrantError::InvalidRequest);
    }
    normalize_ids(&mut request.scope.tools)?;
    normalize_ids(&mut request.scope.resources)
}

fn normalize_ids(values: &mut Vec<String>) -> Result<(), GrantError> {
    if values.iter().any(|value| {
        value.is_empty()
            || value.trim() != value
            || value.chars().any(char::is_control)
    }) {
        return Err(GrantError::InvalidRequest);
    }
    values.sort_unstable();
    values.dedup();
    Ok(())
}

fn scope_allowed(scope: &CapabilityRequest) -> bool {
    scope.tools.iter().all(|tool| {
        !matches!(
            tool.as_str(),
            "shell" | "bash" | "sh" | "powershell" | "pwsh"
        ) && !tool.starts_with("automation_")
    })
}

fn covers(scope: &CapabilityRequest, requested: &CapabilityRequest) -> bool {
    requested
        .tools
        .iter()
        .all(|tool| scope.tools.binary_search(tool).is_ok())
        && requested
            .resources
            .iter()
            .all(|resource| scope.resources.binary_search(resource).is_ok())
        && scope_allowed(requested)
}

fn decode(bytes: &[u8]) -> Result<LedgerState, GrantError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| GrantError::Corrupt)?;
    match value
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
    {
        Some(version) if version == u64::from(FORMAT_VERSION) => {}
        Some(_) => return Err(GrantError::UnsupportedVersion),
        None => return Err(GrantError::Corrupt),
    }
    let document: Document =
        serde_json::from_value(value).map_err(|_| GrantError::Corrupt)?;
    let mut state = LedgerState::default();
    let mut grant_ids = BTreeSet::new();
    for mut entry in document.entries {
        normalize_request(&mut entry.request)
            .map_err(|_| GrantError::Corrupt)?;
        if entry.decided_at_ms < entry.request.requested_at_ms
            || entry.decided_at_ms >= entry.request.request_expires_at_ms
            || entry.decided_at_ms >= entry.request.grant_expires_at_ms
        {
            return Err(GrantError::Corrupt);
        }
        let key = request_key(&entry.request);
        if let Decision::Approved {
            grant_id,
            version,
            revoked_at_ms,
            expired_at_ms,
        } = &entry.decision
        {
            if !scope_allowed(&entry.request.scope)
                || !grant_ids.insert(grant_id.clone())
                || match revoked_at_ms {
                    None => *version != 1,
                    Some(at) => *version != 2 || *at < entry.decided_at_ms,
                }
                || expired_at_ms
                    .is_some_and(|at| at < entry.request.grant_expires_at_ms)
            {
                return Err(GrantError::Corrupt);
            }
            state.grants.insert(grant_id.clone(), key.clone());
        }
        if state.decisions.insert(key, entry).is_some() {
            return Err(GrantError::Corrupt);
        }
    }
    Ok(state)
}

#[cfg(test)]
pub(super) mod tests;
