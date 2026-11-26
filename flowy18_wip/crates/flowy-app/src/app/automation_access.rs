//! Process-owned authorization for cloud automation runs.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent::{
    CatalogModel, ExecutionDenied, ExecutionGate, ExecutionMetadata,
    ExecutionOwner, ExecutionSource, ModelRoute, ToolCallArgs, ToolRunContext,
};
use flowy_auth::Session;
use flowy_automation::{
    AccountGeneration, CapabilityRequest, GenerationEpoch, GrantId, PrincipalId,
};

use super::automation_grants::{
    GrantBinding, GrantError, GrantLedger, GrantRequest, GrantStatus,
};

const CLOUD_PROVIDER: &str = "flowy";

/// Trusted inputs supplied by the host's run adapter. A request cannot grant
/// capabilities: it must be covered by the referenced durable approval.
#[derive(Clone, Debug)]
pub struct AutomationLeaseRequest {
    pub principal: PrincipalId,
    pub account_generation: AccountGeneration,
    pub execution: ExecutionMetadata,
    pub route: ModelRoute,
    pub capabilities: CapabilityRequest,
    pub grant: GrantBinding,
}

/// The account authority and durable ledger share the data-root owner's
/// lifetime. A bad ledger disables automation while leaving foreground chat
/// available; it is never replaced with an empty, permissive ledger.
#[derive(Clone, Debug)]
pub(super) struct GrantAuthority {
    authority: Authority,
    ledger: Option<GrantLedger>,
}

impl GrantAuthority {
    pub(super) fn open_guarded(
        authority: Authority,
        root: &Path,
        storage_owner: Arc<dyn Send + Sync>,
    ) -> Self {
        Self::from_ledger(
            authority,
            GrantLedger::open(root)
                .map(|ledger| ledger.retaining_store_owner(storage_owner)),
        )
    }

    #[cfg(test)]
    fn open(authority: Authority, root: &Path) -> Self {
        Self::from_ledger(authority, GrantLedger::open(root))
    }

    fn from_ledger(
        authority: Authority,
        ledger: Result<GrantLedger, GrantError>,
    ) -> Self {
        let ledger = match ledger {
            Ok(ledger) => Some(ledger),
            Err(error) => {
                tracing::error!(%error, "automation grant ledger is unavailable");
                None
            }
        };
        Self { authority, ledger }
    }

    pub(super) fn available(&self) -> bool {
        self.ledger.as_ref().is_some_and(GrantLedger::available)
    }

    fn ledger(&self) -> Result<&GrantLedger, GrantError> {
        self.ledger.as_ref().ok_or(GrantError::Unavailable)
    }

    pub(super) fn decide(
        &self,
        request: GrantRequest,
        approved: bool,
        now_ms: i64,
    ) -> Result<GrantStatus, GrantError> {
        // Serialize the account check and durable decision with sign-out. A
        // late approval cannot enter the ledger after sign-out has returned.
        let state = self.authority.state.lock().unwrap();
        check_caller(&state, &request.principal, &request.account_generation)?;
        self.ledger()?.decide(request, approved, now_ms)
    }

    pub(super) fn status_for_request(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
        request: GrantRequest,
        now_ms: i64,
    ) -> Result<Option<GrantStatus>, GrantError> {
        let state = self.authority.state.lock().unwrap();
        check_caller(&state, principal, generation)?;
        if principal != &request.principal {
            return Err(GrantError::OwnerMismatch);
        }
        // Existing approvals may be recovered by the same principal after a
        // restart; the old request binding itself must remain byte-for-byte.
        self.ledger()?.status_for_request(request, now_ms)
    }

    pub(super) fn revoke(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
        grant_id: &GrantId,
        now_ms: i64,
    ) -> Result<bool, GrantError> {
        let mut state = self.authority.state.lock().unwrap();
        check_caller(&state, principal, generation)?;
        let ledger = self.ledger()?;
        // Invalidate live leases before writing or signaling cancellation.
        // Even a storage failure must stop already authorized external work.
        advance(&mut state.generation);
        let result = ledger.revoke(principal, grant_id, now_ms);
        drop(state);
        result
    }

    pub(super) fn binding(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
        grant_id: &GrantId,
        now_ms: i64,
    ) -> Result<GrantBinding, GrantError> {
        let state = self.authority.state.lock().unwrap();
        check_caller(&state, principal, generation)?;
        self.ledger()?.binding(principal, grant_id, now_ms)
    }

    pub(super) fn inventory(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
    ) -> Result<Vec<GrantId>, GrantError> {
        let state = self.authority.state.lock().unwrap();
        check_caller(&state, principal, generation)?;
        Ok(self
            .ledger()?
            .inventory()
            .into_iter()
            .filter_map(|(owner, id)| (&owner == principal).then_some(id))
            .collect())
    }

    pub(super) fn lease(
        &self,
        request: AutomationLeaseRequest,
    ) -> Result<Arc<dyn ExecutionGate>, ExecutionDenied> {
        let ledger = self
            .ledger()
            .map_err(|_| ExecutionDenied::LeaseExpired)?
            .clone();
        ledger
            .check(
                &request.principal,
                &request.grant,
                &request.capabilities,
                current_time_ms(),
            )
            .map_err(|_| ExecutionDenied::LeaseExpired)?;
        let account_gate = self.authority.lease(
            &request.principal,
            &request.account_generation,
            request.execution,
            request.route,
            request.capabilities.tools.iter().cloned().collect(),
        )?;
        Ok(Arc::new(GrantExecutionGate {
            account_gate,
            ledger,
            principal: request.principal,
            grant: request.grant,
            capabilities: request.capabilities,
        }))
    }
}

struct GrantExecutionGate {
    account_gate: Arc<dyn ExecutionGate>,
    ledger: GrantLedger,
    principal: PrincipalId,
    grant: GrantBinding,
    capabilities: CapabilityRequest,
}

impl GrantExecutionGate {
    fn check(&self) -> Result<(), ExecutionDenied> {
        self.ledger
            .check(
                &self.principal,
                &self.grant,
                &self.capabilities,
                current_time_ms(),
            )
            .map_err(|_| ExecutionDenied::LeaseExpired)
    }
}

impl ExecutionGate for GrantExecutionGate {
    fn model_credential(
        &self,
        execution: &ExecutionMetadata,
        route: &ModelRoute,
    ) -> Result<String, ExecutionDenied> {
        self.check()?;
        self.account_gate.model_credential(execution, route)
    }

    fn authorize_tool(
        &self,
        context: &ToolRunContext,
        args: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied> {
        self.check()?;
        self.account_gate.authorize_tool(context, args)
    }
}

fn check_caller(
    state: &AuthorityState,
    principal: &PrincipalId,
    generation: &AccountGeneration,
) -> Result<(), GrantError> {
    if &state.generation != generation {
        return Err(GrantError::LeaseExpired);
    }
    let account = state
        .account
        .as_ref()
        .ok_or(GrantError::AccountUnavailable)?;
    if &account.principal != principal {
        return Err(GrantError::OwnerMismatch);
    }
    Ok(())
}

pub(super) fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MIN)
}

#[derive(Clone)]
pub(super) struct Authority {
    namespace: Arc<str>,
    state: Arc<Mutex<AuthorityState>>,
}

struct AuthorityState {
    generation: AccountGeneration,
    account: Option<ActiveAccount>,
}

struct ActiveAccount {
    principal: PrincipalId,
    credential: String,
    routes: BTreeSet<(String, String)>,
}

impl fmt::Debug for Authority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Authority").finish_non_exhaustive()
    }
}

impl Authority {
    pub(super) fn new(namespace: String, session: Option<&Session>) -> Self {
        let state = AuthorityState {
            generation: AccountGeneration {
                epoch: GenerationEpoch::generate(),
                sequence: 0,
            },
            account: session.and_then(|session| account(&namespace, session)),
        };
        Self {
            namespace: namespace.into(),
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub(super) fn generation(&self) -> AccountGeneration {
        self.state.lock().unwrap().generation.clone()
    }

    pub(super) fn account_binding(
        &self,
    ) -> Option<(PrincipalId, AccountGeneration)> {
        let state = self.state.lock().unwrap();
        Some((
            state.account.as_ref()?.principal.clone(),
            state.generation.clone(),
        ))
    }

    pub(super) fn is_current(&self, generation: &AccountGeneration) -> bool {
        self.state.lock().unwrap().generation == *generation
    }

    /// Result viewer for owner-aware access. Legacy sessions without a stable
    /// cloud principal have none and therefore see no owned results.
    pub(super) fn current_owner(&self) -> Option<ExecutionOwner> {
        self.state
            .lock()
            .unwrap()
            .account
            .as_ref()
            .map(|account| owner_of(&account.principal))
    }

    /// A fresh sign-in always invalidates prior leases, even for the same ID.
    pub(super) fn sign_in(&self, session: &Session) -> Option<ExecutionOwner> {
        self.replace_account(account(&self.namespace, session))
    }

    /// Invalidate the prior generation before signaling its runs.
    pub(super) fn sign_out(&self) -> Option<ExecutionOwner> {
        self.replace_account(None)
    }

    pub(super) fn revoke_leases(&self) -> Option<ExecutionOwner> {
        let mut state = self.state.lock().unwrap();
        advance(&mut state.generation);
        state
            .account
            .as_ref()
            .map(|account| owner_of(&account.principal))
    }

    pub(super) fn credential_is_current(
        &self,
        generation: &AccountGeneration,
        session: &Session,
    ) -> bool {
        let expected = principal_for(&self.namespace, session);
        let state = self.state.lock().unwrap();
        if &state.generation != generation {
            return false;
        }
        match (&state.account, expected) {
            (Some(account), Some(principal)) => {
                account.principal == principal
                    && account.credential == session.access_token
            }
            // Legacy auth can keep its foreground behavior. No automation
            // principal or credential is constructed for this session.
            (None, None) => true,
            _ => false,
        }
    }

    fn replace_account(
        &self,
        account: Option<ActiveAccount>,
    ) -> Option<ExecutionOwner> {
        let mut state = self.state.lock().unwrap();
        let previous = std::mem::replace(&mut state.account, account);
        advance(&mut state.generation);
        drop(state);
        previous.map(|account| owner_of(&account.principal))
    }

    /// A trusted durable credential refresh updates a matching live account.
    pub(super) fn refresh_credential(
        &self,
        generation: &AccountGeneration,
        session: &Session,
    ) -> bool {
        self.with_current_account(generation, session, |account| {
            account.credential.clone_from(&session.access_token);
        })
    }

    pub(super) fn replace_routes(
        &self,
        generation: &AccountGeneration,
        session: &Session,
        models: &[CatalogModel],
        effective_tier: u8,
    ) {
        self.with_current_account(generation, session, |account| {
            account.routes = models
                .iter()
                .filter(|model| model.min_tier_level <= effective_tier)
                .filter(|model| {
                    !model.id.is_empty() && !model.endpoint.is_empty()
                })
                .map(|model| (model.id.clone(), model.endpoint.clone()))
                .collect();
        });
    }

    fn with_current_account(
        &self,
        generation: &AccountGeneration,
        session: &Session,
        update: impl FnOnce(&mut ActiveAccount),
    ) -> bool {
        let expected = principal_for(&self.namespace, session);
        let mut state = self.state.lock().unwrap();
        if &state.generation != generation {
            return false;
        }
        let Some(account) = &mut state.account else {
            return false;
        };
        if expected.as_ref() != Some(&account.principal) {
            return false;
        }
        update(account);
        drop(state);
        true
    }

    fn lease(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
        execution: ExecutionMetadata,
        route: ModelRoute,
        granted_tools: BTreeSet<String>,
    ) -> Result<Arc<dyn ExecutionGate>, ExecutionDenied> {
        if !matches!(execution.source, ExecutionSource::Automation { .. })
            || execution.owner.as_ref() != Some(&owner_of(principal))
            || !execution.is_valid()
        {
            return Err(ExecutionDenied::OwnerMismatch);
        }
        let lease = ExecutionLease {
            authority: self.clone(),
            principal: principal.clone(),
            generation: generation.clone(),
            execution,
            route,
            granted_tools,
        };
        // Preflight lets a worker block rather than consume an attempt when
        // an account or route is unavailable. Dispatch always repeats it.
        lease.with_current(&lease.execution, &lease.route, |_| ())?;
        Ok(Arc::new(lease))
    }
}

/// The lease holds only immutable run scope and a pointer to host state.
/// It never owns a saved token or permits a different attempt to reuse it.
struct ExecutionLease {
    authority: Authority,
    principal: PrincipalId,
    generation: AccountGeneration,
    execution: ExecutionMetadata,
    route: ModelRoute,
    granted_tools: BTreeSet<String>,
}

impl ExecutionLease {
    fn with_current<T>(
        &self,
        execution: &ExecutionMetadata,
        route: &ModelRoute,
        access: impl FnOnce(&str) -> T,
    ) -> Result<T, ExecutionDenied> {
        if execution != &self.execution {
            return Err(ExecutionDenied::OwnerMismatch);
        }
        let state = self.authority.state.lock().unwrap();
        if state.generation != self.generation {
            return Err(ExecutionDenied::LeaseExpired);
        }
        let account = state
            .account
            .as_ref()
            .ok_or(ExecutionDenied::AccountUnavailable)?;
        if account.principal != self.principal {
            return Err(ExecutionDenied::OwnerMismatch);
        }
        if account.credential.trim().is_empty() {
            return Err(ExecutionDenied::CredentialUnavailable);
        }
        if route != &self.route
            || route.provider != CLOUD_PROVIDER
            || !account
                .routes
                .contains(&(route.model.clone(), route.base_url.clone()))
        {
            return Err(ExecutionDenied::ModelUnavailable);
        }
        let result = access(&account.credential);
        drop(state);
        Ok(result)
    }
}

impl ExecutionGate for ExecutionLease {
    fn model_credential(
        &self,
        execution: &ExecutionMetadata,
        route: &ModelRoute,
    ) -> Result<String, ExecutionDenied> {
        self.with_current(execution, route, str::to_owned)
    }

    fn authorize_tool(
        &self,
        context: &ToolRunContext,
        args: &ToolCallArgs,
    ) -> Result<(), ExecutionDenied> {
        self.with_current(&context.execution, &self.route, |_| ())?;
        if self.granted_tools.contains(&args.name) {
            Ok(())
        } else {
            Err(ExecutionDenied::ToolDenied)
        }
    }
}

fn principal_for(namespace: &str, session: &Session) -> Option<PrincipalId> {
    PrincipalId::cloud(namespace, session.user_id?).ok()
}

fn account(namespace: &str, session: &Session) -> Option<ActiveAccount> {
    Some(ActiveAccount {
        principal: principal_for(namespace, session)?,
        credential: session.access_token.clone(),
        routes: BTreeSet::new(),
    })
}

fn owner_of(principal: &PrincipalId) -> ExecutionOwner {
    ExecutionOwner {
        authority: principal.authority().to_owned(),
        subject: principal.external_user_id().to_string(),
    }
}

fn advance(generation: &mut AccountGeneration) {
    if let Some(sequence) = generation.sequence.checked_add(1) {
        generation.sequence = sequence;
    } else {
        generation.epoch = GenerationEpoch::generate();
        generation.sequence = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowy_agent::{SessionOrigin, ToolRunContext};

    fn session(id: Option<i64>, credential: &str) -> Session {
        Session {
            access_token: credential.to_owned(),
            email: "test@example.invalid".to_owned(),
            display_name: "test".to_owned(),
            user_id: id,
        }
    }

    fn route() -> ModelRoute {
        ModelRoute {
            provider: CLOUD_PROVIDER.into(),
            model: "scheduled".into(),
            base_url: "https://example.invalid/agent/v1".into(),
            ..ModelRoute::default()
        }
    }

    fn execution(principal: &PrincipalId) -> ExecutionMetadata {
        ExecutionMetadata {
            owner: Some(owner_of(principal)),
            source: ExecutionSource::Automation {
                automation_id: "automation-1".into(),
                run_id: "run-1".into(),
                attempt_id: "attempt-1".into(),
            },
        }
    }

    fn configure_routes(
        authority: &Authority,
        generation: &AccountGeneration,
        user: &Session,
        required_tier: u8,
        effective_tier: u8,
    ) {
        let route = route();
        authority.replace_routes(
            generation,
            user,
            &[CatalogModel {
                id: route.model,
                endpoint: route.base_url,
                min_tier_level: required_tier,
                ..CatalogModel::default()
            }],
            effective_tier,
        );
    }

    #[test]
    fn missing_stable_account_never_enables_automation() {
        let legacy = session(None, "legacy");
        let authority = Authority::new("account.example".into(), Some(&legacy));
        let generation = authority.generation();
        assert!(authority.credential_is_current(&generation, &legacy));
        assert!(authority.account_binding().is_none());
        let principal = PrincipalId::cloud("account.example", 23).unwrap();
        assert!(matches!(
            authority.lease(
                &principal,
                &generation,
                execution(&principal),
                route(),
                BTreeSet::new(),
            ),
            Err(ExecutionDenied::AccountUnavailable)
        ));
    }

    #[test]
    fn refresh_replaces_credential_without_resurrecting_expired_leases() {
        let old_session = session(Some(23), "old-token");
        let authority =
            Authority::new("account.example".into(), Some(&old_session));
        let (principal, generation) = authority.account_binding().unwrap();
        configure_routes(&authority, &generation, &old_session, 0, 0);
        let execution = execution(&principal);
        let route = route();
        let lease = authority
            .lease(
                &principal,
                &generation,
                execution.clone(),
                route.clone(),
                BTreeSet::new(),
            )
            .unwrap();
        assert_eq!(
            lease.model_credential(&execution, &route).unwrap(),
            "old-token"
        );
        let fresh_session = session(Some(23), "fresh-token");
        assert!(authority.refresh_credential(&generation, &fresh_session));
        assert_eq!(authority.generation(), generation);
        assert!(!authority.credential_is_current(&generation, &old_session));
        assert!(authority.credential_is_current(&generation, &fresh_session));
        assert_eq!(
            lease.model_credential(&execution, &route).unwrap(),
            "fresh-token"
        );
        let foreign_session = session(Some(24), "foreign-token");
        assert_eq!(
            authority.sign_in(&foreign_session),
            Some(owner_of(&principal))
        );
        assert!(!authority.refresh_credential(&generation, &fresh_session));
        assert_eq!(
            lease.model_credential(&execution, &route),
            Err(ExecutionDenied::LeaseExpired),
        );
        assert!(authority.sign_out().is_some());
        authority.sign_in(&fresh_session);
        let resumed_generation = authority.generation();
        assert_ne!(resumed_generation, generation);
        configure_routes(&authority, &generation, &fresh_session, 0, 0);
        assert!(matches!(
            authority.lease(
                &principal,
                &resumed_generation,
                execution.clone(),
                route.clone(),
                BTreeSet::new(),
            ),
            Err(ExecutionDenied::ModelUnavailable)
        ));
        assert_eq!(
            lease.model_credential(&execution, &route),
            Err(ExecutionDenied::LeaseExpired),
        );
    }

    #[test]
    fn routes_and_each_tool_dispatch_are_bound_to_the_authorized_attempt() {
        let user = session(Some(23), "private");
        let authority = Authority::new("account.example".into(), Some(&user));
        let (principal, generation) = authority.account_binding().unwrap();
        let execution = execution(&principal);
        let route = route();
        configure_routes(&authority, &generation, &user, 1, 0);
        assert!(matches!(
            authority.lease(
                &principal,
                &generation,
                execution.clone(),
                route.clone(),
                BTreeSet::new(),
            ),
            Err(ExecutionDenied::ModelUnavailable)
        ));
        configure_routes(&authority, &generation, &user, 1, 1);
        let lease = authority
            .lease(
                &principal,
                &generation,
                execution.clone(),
                route.clone(),
                BTreeSet::from(["read_file".to_owned()]),
            )
            .unwrap();

        let different_route = ModelRoute {
            base_url: "https://other.example.invalid/v1".into(),
            ..route
        };
        assert_eq!(
            lease.model_credential(&execution, &different_route),
            Err(ExecutionDenied::ModelUnavailable),
        );
        let mut context = ToolRunContext {
            signal: flowy_agent::CancelToken::new(),
            agent_id: None,
            cwd: None,
            todos: None,
            submission_id: None,
            session_origin: SessionOrigin::Unknown,
            execution,
            output_root: None,
            artifact_delivery_available: false,
        };
        let read = ToolCallArgs {
            call_id: "call-1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({}),
        };
        assert_eq!(lease.authorize_tool(&context, &read), Ok(()));
        let write = ToolCallArgs {
            name: "write_file".into(),
            ..read.clone()
        };
        assert_eq!(
            lease.authorize_tool(&context, &write),
            Err(ExecutionDenied::ToolDenied),
        );
        let original = context.execution.clone();
        if let ExecutionSource::Automation { attempt_id, .. } =
            &mut context.execution.source
        {
            "other-attempt".clone_into(attempt_id);
        }
        assert_eq!(
            lease.authorize_tool(&context, &read),
            Err(ExecutionDenied::OwnerMismatch),
        );
        context.execution = original;
        authority.revoke_leases();
        assert_eq!(
            lease.authorize_tool(&context, &read),
            Err(ExecutionDenied::LeaseExpired),
        );
    }

    fn grant_request(authority: &Authority) -> GrantRequest {
        let mut request = super::super::automation_grants::tests::request();
        let (principal, generation) = authority.account_binding().unwrap();
        request.principal = principal;
        request.account_generation = generation;
        let now = current_time_ms();
        request.requested_at_ms = now.saturating_sub(10);
        request.request_expires_at_ms = now.saturating_add(60_000);
        request.grant_expires_at_ms = now.saturating_add(3_600_000);
        request
    }

    fn granted_binding(
        grants: &GrantAuthority,
        request: &GrantRequest,
    ) -> GrantBinding {
        let GrantStatus::Approved(binding) = grants
            .decide(request.clone(), true, current_time_ms())
            .unwrap()
        else {
            panic!("expected durable grant");
        };
        binding
    }

    #[test]
    fn late_approval_cannot_cross_logout_switch_or_same_account_relogin() {
        let root = super::super::automation_grants::tests::TestRoot::new();
        let user = session(Some(23), "account-a-token");
        let authority = Authority::new("account.example".into(), Some(&user));
        let grants = GrantAuthority::open(authority.clone(), root.path());
        let request = grant_request(&authority);
        let (principal, generation) = authority.account_binding().unwrap();
        authority.sign_in(&session(Some(24), "account-b-token"));
        assert!(matches!(
            grants.decide(request.clone(), true, current_time_ms()),
            Err(GrantError::LeaseExpired),
        ));
        assert!(matches!(
            grants.inventory(&principal, &generation),
            Err(GrantError::LeaseExpired),
        ));
        authority.sign_out();
        authority.sign_in(&user);
        assert!(matches!(
            grants.decide(request, true, current_time_ms()),
            Err(GrantError::LeaseExpired),
        ));
        let (principal, generation) = authority.account_binding().unwrap();
        assert_eq!(grants.inventory(&principal, &generation).unwrap().len(), 0);
        assert!(!root.path().join("automation/grants.json").exists());
    }

    #[test]
    fn durable_approval_recovers_only_for_the_same_current_principal() {
        let root = super::super::automation_grants::tests::TestRoot::new();
        let user = session(Some(23), "account-a-token");
        let authority = Authority::new("account.example".into(), Some(&user));
        let grants = GrantAuthority::open(authority.clone(), root.path());
        let request = grant_request(&authority);
        let binding = granted_binding(&grants, &request);
        authority.sign_in(&session(Some(24), "account-b-token"));
        let (foreign, generation) = authority.account_binding().unwrap();
        assert!(matches!(
            grants.status_for_request(
                &foreign,
                &generation,
                request.clone(),
                current_time_ms(),
            ),
            Err(GrantError::OwnerMismatch),
        ));
        assert!(matches!(
            grants.binding(
                &foreign,
                &generation,
                &binding.grant_id,
                current_time_ms()
            ),
            Err(GrantError::UnknownGrant),
        ));
        assert_eq!(grants.inventory(&foreign, &generation).unwrap().len(), 0);
        authority.sign_in(&user);
        let (principal, generation) = authority.account_binding().unwrap();
        assert_eq!(
            grants
                .status_for_request(
                    &principal,
                    &generation,
                    request,
                    current_time_ms()
                )
                .unwrap(),
            Some(GrantStatus::Approved(binding.clone())),
        );
        let restarted_authority =
            Authority::new("account.example".into(), Some(&user));
        let reopened =
            GrantAuthority::open(restarted_authority.clone(), root.path());
        let (principal, generation) =
            restarted_authority.account_binding().unwrap();
        assert_eq!(
            reopened
                .binding(
                    &principal,
                    &generation,
                    &binding.grant_id,
                    current_time_ms()
                )
                .unwrap(),
            binding,
        );
        let stored =
            std::fs::read_to_string(root.path().join("automation/grants.json"))
                .unwrap();
        assert!(!stored.contains("account-a-token"));
        assert!(!stored.contains("account-b-token"));
    }

    #[test]
    fn revoking_a_grant_blocks_model_and_tool_dispatch_and_survives_relogin() {
        let root = super::super::automation_grants::tests::TestRoot::new();
        let user = session(Some(23), "account-a-token");
        let authority = Authority::new("account.example".into(), Some(&user));
        let grants = GrantAuthority::open(authority.clone(), root.path());
        let request = grant_request(&authority);
        let binding = granted_binding(&grants, &request);
        let (principal, generation) = authority.account_binding().unwrap();
        configure_routes(&authority, &generation, &user, 0, 0);
        let execution = execution(&principal);
        let route = route();
        let lease_request = AutomationLeaseRequest {
            principal: principal.clone(),
            account_generation: generation.clone(),
            execution: execution.clone(),
            route: route.clone(),
            capabilities: request.scope,
            grant: binding.clone(),
        };
        let lease = grants.lease(lease_request.clone()).unwrap();
        assert_eq!(
            lease.model_credential(&execution, &route).unwrap(),
            "account-a-token"
        );
        let context = ToolRunContext {
            signal: flowy_agent::CancelToken::new(),
            agent_id: None,
            cwd: None,
            todos: None,
            submission_id: None,
            session_origin: SessionOrigin::Unknown,
            execution: execution.clone(),
            output_root: None,
            artifact_delivery_available: false,
        };
        let args = ToolCallArgs {
            call_id: "read-after-approval".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({}),
        };
        assert_eq!(lease.authorize_tool(&context, &args), Ok(()));
        assert!(
            grants
                .revoke(
                    &principal,
                    &generation,
                    &binding.grant_id,
                    current_time_ms()
                )
                .unwrap()
        );
        assert_ne!(authority.generation(), generation);
        assert_eq!(
            lease.model_credential(&execution, &route),
            Err(ExecutionDenied::LeaseExpired)
        );
        assert_eq!(
            lease.authorize_tool(&context, &args),
            Err(ExecutionDenied::LeaseExpired)
        );
        authority.sign_out();
        authority.sign_in(&user);
        let generation = authority.generation();
        configure_routes(&authority, &generation, &user, 0, 0);
        assert!(matches!(
            grants.lease(AutomationLeaseRequest {
                account_generation: generation,
                ..lease_request
            }),
            Err(ExecutionDenied::LeaseExpired),
        ));
    }

    #[test]
    fn corrupt_ledger_disables_grants_without_disabling_foreground_authority() {
        let root = super::super::automation_grants::tests::TestRoot::new();
        std::fs::create_dir(root.path().join("automation")).unwrap();
        let path = root.path().join("automation/grants.json");
        std::fs::write(&path, b"corrupt-ledger").unwrap();
        let user = session(Some(23), "account-a-token");
        let authority = Authority::new("account.example".into(), Some(&user));
        let grants = GrantAuthority::open(authority.clone(), root.path());
        assert!(!grants.available());
        assert!(
            authority.credential_is_current(&authority.generation(), &user)
        );
        let request = grant_request(&authority);
        assert!(matches!(
            grants.decide(request, true, current_time_ms()),
            Err(GrantError::Unavailable)
        ));
        assert_eq!(std::fs::read(path).unwrap(), b"corrupt-ledger");
    }
}
