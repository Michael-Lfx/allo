//! Process-owned execution and exclusive ownership of the application store.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flowy_agent::{
    AgentHandle, AgentInitialization, ExecutionDenied, ExecutionGate, Model,
    ProviderCatalog,
};
use flowy_auth::{Config, Session, SessionStore};
use flowy_automation::{AccountGeneration, GrantId, PrincipalId};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use super::automation_access::{
    Authority, AutomationLeaseRequest, GrantAuthority, current_time_ms,
};
use super::automation_grants::{
    GrantBinding, GrantError, GrantRequest, GrantStatus,
};

const QUIT_GRACE: Duration = Duration::from_millis(1_250);
const PLATFORM_QUIT_GRACE: Duration = Duration::from_millis(150);

/// Acquire before reading preferences, account state, or agent history.
/// Moving this token into a host never locks the file a second time.
#[derive(Debug)]
pub struct OwnedBootstrap {
    owner: Arc<StoreOwner>,
}

#[derive(Debug)]
struct StoreOwner {
    root: PathBuf,
    identity: PathBuf,
    // Keep the advisory lock file in place, including after normal shutdown.
    _file: File,
}

impl OwnedBootstrap {
    pub fn acquire(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = std::path::absolute(root.into())?;
        fs::create_dir_all(&root)?;
        let identity = fs::canonicalize(&root)?;
        let path = root.join("instance.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is locked by another instance", path.display()),
            ),
            std::fs::TryLockError::Error(error) => error,
        })?;
        Ok(Self {
            owner: Arc::new(StoreOwner {
                root,
                identity,
                _file: file,
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.owner.root
    }
}

/// A GPUI global retains the host even when every window has been removed.
/// Windows hold their own clone and only subscribe to its existing agent.
#[derive(Clone, Debug)]
pub struct AppHost {
    inner: Arc<HostInner>,
}

#[derive(Debug)]
struct HostInner {
    agent: AgentHandle,
    runtime: Option<Runtime>,
    owner: Arc<StoreOwner>,
    cloud: Mutex<CloudState>,
    authority: Authority,
    grants: GrantAuthority,
    automation_wake: Arc<tokio::sync::Notify>,
    pub(super) automation_notifications:
        Mutex<Option<super::automation_delivery::DesktopNotifications>>,
    manual_automation: tokio::sync::Mutex<
        Option<Arc<super::automation_host::ManualAutomation>>,
    >,
}

#[derive(Debug, Default)]
struct CloudState {
    revision: u64,
    selection: Option<SharedCloudModel>,
}

/// Window display state for the currently configured shared agent. No token
/// is copied into this projection; automation credentials stay in Authority.
#[derive(Clone, Debug)]
pub(super) struct SharedCloudModel {
    pub account: String,
    pub tier: Model,
    pub catalog: ProviderCatalog,
    pub effective_tier: u8,
}

impl Drop for HostInner {
    fn drop(&mut self) {
        // A final host may also be dropped from an async context. The agent's
        // observers, initialization worker and artifact handles retain the
        // store owner until their actual work has stopped.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl gpui::Global for AppHost {}

struct QuitFallback {
    _subscription: gpui::Subscription,
}

impl gpui::Global for QuitFallback {}

#[derive(Debug)]
pub(super) struct ModelSignal;

struct ModelEvents(gpui::Entity<ModelSignal>);

impl gpui::Global for ModelEvents {}

impl AppHost {
    pub fn start(bootstrap: OwnedBootstrap) -> io::Result<Self> {
        Self::with_mode(bootstrap, true)
    }

    pub(super) fn install(self, cx: &mut gpui::App) {
        cx.set_global(self);
        let subscription = cx.on_app_quit(|cx| {
            let shutdown = cx
                .try_global::<Self>()
                .and_then(|host| host.start_shutdown(PLATFORM_QUIT_GRACE));
            async move {
                if let Some(shutdown) = shutdown {
                    let _ = shutdown.await;
                }
            }
        });
        cx.set_global(QuitFallback {
            _subscription: subscription,
        });
    }

    /// Coordinate an explicit quit before entering the platform's short
    /// quit hook. Repeated requests share the first request's deadline.
    pub fn request_shutdown(cx: &mut gpui::App) {
        let Some(host) = cx.try_global::<Self>() else {
            cx.quit();
            return;
        };
        if let Some(shutdown) = host.start_shutdown(QUIT_GRACE) {
            cx.spawn(async move |cx| {
                let _ = shutdown.await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    }

    fn start_shutdown(&self, grace: Duration) -> Option<JoinHandle<()>> {
        let runtime = self.inner.runtime.as_ref()?;
        if !self.inner.agent.begin_shutdown() {
            return None;
        }
        let host = self.clone();
        Some(runtime.spawn(async move {
            // Stop intake and wait for the worker's real execution future.
            // A grace timeout preserves its evidence for startup recovery.
            let automation = host.inner.manual_automation.lock().await.clone();
            if let Some(automation) = automation {
                automation.request_stop();
                let _ = tokio::time::timeout(grace, automation.join()).await;
            }
            let until = grace.saturating_sub(Duration::from_millis(50));
            let finish = tokio::task::spawn_blocking(move || {
                host.inner.agent.prepare_shutdown_with_timeout(until);
            });
            match tokio::time::timeout(grace, finish).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, "application shutdown worker failed");
                }
                Err(_) => {
                    tracing::warn!("application shutdown exceeded its grace period");
                }
            }
        }))
    }

    fn with_mode(
        bootstrap: OwnedBootstrap,
        background: bool,
    ) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("flowy-agent")
            .build()?;
        let owner = bootstrap.owner;
        let stored_session = SessionStore::new(owner.root.clone()).load();
        let authority = Authority::new(
            Config::default().effective_base_url(),
            stored_session.as_ref(),
        );
        let grant_guard: Arc<dyn Send + Sync> = owner.clone();
        let grants = GrantAuthority::open_guarded(
            authority.clone(),
            &owner.root,
            grant_guard,
        );
        let guard: Arc<dyn Send + Sync> = owner.clone();
        let config = super::FlowyApp::disconnected_config();
        let agent = if background {
            AgentHandle::with_config_background_on_runtime_guarded(
                owner.root.clone(),
                config,
                runtime.handle().clone(),
                guard,
            )
        } else {
            AgentHandle::with_config_on_runtime_guarded(
                owner.root.clone(),
                config,
                runtime.handle().clone(),
                guard,
            )
        };
        agent.set_autonomous_drain(background);
        agent.set_result_viewer(authority.current_owner());
        Ok(Self {
            inner: Arc::new(HostInner {
                agent,
                runtime: Some(runtime),
                owner,
                cloud: Mutex::new(CloudState::default()),
                authority,
                grants,
                manual_automation: tokio::sync::Mutex::new(None),
                automation_wake: Arc::new(tokio::sync::Notify::new()),
                automation_notifications: Mutex::new(None),
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.owner.root
    }

    /// Explicit internal validation entry. Normal desktop startup never calls
    /// it; the caller must provide a real admission policy and an initialized
    /// account. No permissive policy is installed by the host.
    pub async fn start_manual_automation_for_validation(
        &self,
        admission: Arc<dyn flowy_automation::AdmissionPolicy>,
    ) -> Result<flowy_automation::AutomationHandle, flowy_automation::PortError>
    {
        self.start_automation(Some(admission), None).await
    }

    /// Start the desktop implementation with real, bounded storage admission.
    /// Explicit validation hosts use this until the release matrix is verified.
    pub async fn start_retained_automation(
        &self,
        limits: flowy_automation::retention::RetentionLimits,
    ) -> Result<flowy_automation::AutomationHandle, flowy_automation::PortError>
    {
        self.start_automation(None, Some(limits)).await
    }

    async fn start_automation(
        &self,
        admission: Option<Arc<dyn flowy_automation::AdmissionPolicy>>,
        limits: Option<flowy_automation::retention::RetentionLimits>,
    ) -> Result<flowy_automation::AutomationHandle, flowy_automation::PortError>
    {
        let mut installed = self.inner.manual_automation.lock().await;
        if let Some(automation) = installed.as_ref() {
            return Ok(automation.handle.clone());
        }
        let runtime = self
            .inner
            .runtime
            .as_ref()
            .ok_or(flowy_automation::PortError::Unavailable)?
            .handle()
            .clone();
        let owner: Arc<dyn Send + Sync> = self.inner.owner.clone();
        let store = flowy_automation::store::AutomationStore::open(
            self.root().to_path_buf(),
            owner,
        )
        .await
        .map_err(|_| flowy_automation::PortError::Storage)?;
        let retention = limits.map(|limits| {
            flowy_automation::retention::RetentionService::new(
                store.clone(),
                Arc::new(super::automation_retention::AgentRetention::new(
                    self.agent(),
                    self.inner.authority.clone(),
                )),
                limits,
            )
        });
        let admission = admission
            .or_else(|| {
                retention.as_ref().map(
                    |policy| -> Arc<dyn flowy_automation::AdmissionPolicy> {
                        Arc::new(policy.clone())
                    },
                )
            })
            .ok_or(flowy_automation::PortError::Unavailable)?;
        let automation = match super::automation_host::ManualAutomation::start(
            store.clone(),
            self.agent(),
            self.inner.authority.clone(),
            self.inner.grants.clone(),
            admission,
            runtime,
            self.inner.automation_wake.clone(),
            self.notifications(),
            retention,
        )
        .await
        {
            Ok(automation) => Arc::new(automation),
            Err(error) => {
                let _ = store.shutdown().await;
                return Err(error);
            }
        };
        let handle = automation.handle.clone();
        let owner: Arc<dyn Send + Sync> = self.inner.owner.clone();
        let factory =
            self.automation_caller_factory(vec![]).unwrap_or_else(|_| {
                Arc::new(|_, _| Err(flowy_automation::PortError::Forbidden))
            });
        let tools =
            match flowy_agent::automation_tools::AutomationToolBridge::open(
                self.root(),
                handle.clone(),
                factory,
                owner,
            ) {
                Ok(tools) => tools,
                Err(error) => {
                    let _ = automation.join().await;
                    return Err(error);
                }
            };
        self.agent().set_automation_tools(Some(tools));
        *installed = Some(automation);
        drop(installed);
        Ok(handle)
    }

    pub async fn stop_manual_automation(
        &self,
    ) -> Result<(), flowy_automation::PortError> {
        let mut installed = self.inner.manual_automation.lock().await;
        self.agent().set_automation_tools(None);
        if let Some(automation) = installed.as_ref() {
            automation.join().await?;
        }
        *installed = None;
        drop(installed);
        Ok(())
    }

    pub(super) fn wake_automation(&self) {
        self.inner.automation_wake.notify_one();
    }

    pub(super) fn automation_is_active(&self) -> bool {
        self.inner
            .manual_automation
            .try_lock()
            .is_ok_and(|installed| installed.is_some())
    }

    pub(super) fn automation_runtime(&self) -> Option<tokio::runtime::Handle> {
        self.inner
            .runtime
            .as_ref()
            .map(|runtime| runtime.handle().clone())
    }

    pub(super) async fn automation_handle(
        &self,
    ) -> Result<flowy_automation::AutomationHandle, flowy_automation::PortError>
    {
        self.inner
            .manual_automation
            .lock()
            .await
            .as_ref()
            .map(|automation| automation.handle.clone())
            .ok_or(flowy_automation::PortError::Unavailable)
    }

    pub(super) fn notification_slot(
        &self,
    ) -> &Mutex<Option<super::automation_delivery::DesktopNotifications>> {
        &self.inner.automation_notifications
    }

    pub(super) async fn approval_cards(
        &self,
        caller: &flowy_automation::CallerContext,
    ) -> Result<
        Vec<flowy_automation::store::ApprovalProposal>,
        flowy_automation::PortError,
    > {
        let installed = self
            .inner
            .manual_automation
            .lock()
            .await
            .clone()
            .ok_or(flowy_automation::PortError::Unavailable)?;
        let cards = installed
            .store
            .pending_approvals(
                caller.principal.clone(),
                super::automation_access::current_time_ms(),
            )
            .await
            .map_err(super::automation_host::port_error)?;
        if self.automation_account_binding()
            != Some((
                caller.principal.clone(),
                caller.account_generation.clone(),
            ))
        {
            return Err(flowy_automation::PortError::Forbidden);
        }
        Ok(cards)
    }

    pub(super) async fn resolve_approval(
        &self,
        caller: flowy_automation::CallerContext,
        proposal: flowy_automation::store::ApprovalProposal,
        approved: bool,
    ) -> Result<(), flowy_automation::PortError> {
        let installed = self
            .inner
            .manual_automation
            .lock()
            .await
            .clone()
            .ok_or(flowy_automation::PortError::Unavailable)?;
        super::automation_host::resolve_approval(
            &installed.store,
            &self.inner.grants,
            caller,
            proposal,
            Some(approved),
        )
        .await?;
        self.wake_automation();
        Ok(())
    }

    pub fn initialization_state(&self) -> AgentInitialization {
        self.inner.agent.initialization_state()
    }

    pub(super) fn agent(&self) -> AgentHandle {
        self.inner.agent.clone()
    }

    pub(super) fn automation_authority(&self) -> &Authority {
        &self.inner.authority
    }

    pub(super) fn start_automation_account(&self, session: &Session) {
        if let Some(previous) = self.inner.authority.sign_in(session) {
            self.inner.agent.cancel_automation_owner(&previous);
        }
        self.rebind_automation_tools();
        self.publish_result_viewer();
    }

    fn rebind_automation_tools(&self) {
        if let Ok(factory) = self.automation_caller_factory(vec![]) {
            self.inner.agent.rebind_automation_tools(factory);
        }
        self.wake_automation();
    }

    pub(super) fn clear_automation_account(&self) {
        if let Some(previous) = self.inner.authority.sign_out() {
            self.inner.agent.cancel_automation_owner(&previous);
        }
        self.publish_result_viewer();
        self.wake_automation();
    }

    /// Every account change advances the viewer generation, including a
    /// same-account re-login, so late result callbacks are discarded.
    fn publish_result_viewer(&self) {
        self.inner
            .agent
            .set_result_viewer(self.inner.authority.current_owner());
    }

    #[cfg(test)]
    pub(super) fn result_viewer_generation(&self) -> u64 {
        self.inner.agent.result_viewer_generation()
    }

    /// Invalidate active automation leases before signaling their attempts.
    pub fn invalidate_automation_leases(&self) {
        if let Some(previous) = self.inner.authority.revoke_leases() {
            self.inner.agent.cancel_automation_owner(&previous);
        }
        self.rebind_automation_tools();
    }

    /// The current stable cloud account, if automation can bind an owner.
    pub fn automation_account_binding(
        &self,
    ) -> Option<(PrincipalId, AccountGeneration)> {
        self.inner.authority.account_binding()
    }

    /// Bind one attempt to an immutable owner/route and an existing durable
    /// grant. Every actual dispatch rechecks the current account and ledger.
    pub fn automation_lease(
        &self,
        request: AutomationLeaseRequest,
    ) -> Result<Arc<dyn ExecutionGate>, ExecutionDenied> {
        self.inner.grants.lease(request)
    }

    pub fn automation_grants_available(&self) -> bool {
        self.inner.grants.available()
    }

    /// Call on a background worker: a successful return means the exact
    /// principal, generation, command binding and decision were persisted.
    pub fn decide_automation_grant(
        &self,
        request: GrantRequest,
        approved: bool,
    ) -> Result<GrantStatus, GrantError> {
        self.inner
            .grants
            .decide(request, approved, current_time_ms())
    }

    /// Recovery can query a prior request under a new live generation of
    /// the same principal, but cannot change its original approval binding.
    pub fn automation_grant_status(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
        request: GrantRequest,
    ) -> Result<Option<GrantStatus>, GrantError> {
        self.inner.grants.status_for_request(
            principal,
            generation,
            request,
            current_time_ms(),
        )
    }

    pub fn automation_grant_binding(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
        grant_id: &GrantId,
    ) -> Result<GrantBinding, GrantError> {
        self.inner.grants.binding(
            principal,
            generation,
            grant_id,
            current_time_ms(),
        )
    }

    /// Revocation first invalidates live leases, then persists the tombstone.
    /// Failed persistence remains an error and never reports durable success.
    pub fn revoke_automation_grant(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
        grant_id: &GrantId,
    ) -> Result<bool, GrantError> {
        let result = self.inner.grants.revoke(
            principal,
            generation,
            grant_id,
            current_time_ms(),
        );
        if !self.inner.authority.is_current(generation) {
            self.inner.agent.cancel_automation_owner(
                &flowy_agent::ExecutionOwner {
                    authority: principal.authority().to_owned(),
                    subject: principal.external_user_id().to_string(),
                },
            );
        }
        self.rebind_automation_tools();
        result
    }

    /// Historic IDs remain queryable for their current owner, including
    /// revoked records, without revealing another account's references.
    pub fn automation_grants(
        &self,
        principal: &PrincipalId,
        generation: &AccountGeneration,
    ) -> Result<Vec<GrantId>, GrantError> {
        self.inner.grants.inventory(principal, generation)
    }

    pub(super) fn model_revision(&self) -> u64 {
        self.inner.cloud.lock().unwrap().revision
    }

    pub(super) fn shared_model(&self) -> Option<SharedCloudModel> {
        self.inner.cloud.lock().unwrap().selection.clone()
    }

    pub(super) fn publish_model(&self, selection: Option<SharedCloudModel>) {
        let mut cloud = self.inner.cloud.lock().unwrap();
        cloud.revision = cloud.revision.wrapping_add(1);
        cloud.selection = selection;
    }

    pub(super) fn model_events(
        cx: &mut gpui::App,
    ) -> gpui::Entity<ModelSignal> {
        use gpui::AppContext as _;
        if let Some(events) = cx.try_global::<ModelEvents>() {
            return events.0.clone();
        }
        let signal = cx.new(|_| ModelSignal);
        cx.set_global(ModelEvents(signal.clone()));
        signal
    }

    pub(super) fn notify_model(cx: &mut gpui::App) {
        Self::model_events(cx).update(cx, |_, cx| cx.notify());
    }

    /// Every entry point, including direct library window construction, uses
    /// the same owner. A second root must use another application context.
    pub(super) fn for_root(
        root: &Path,
        cx: &mut gpui::App,
    ) -> io::Result<Self> {
        if let Some(host) = cx.try_global::<Self>() {
            if fs::canonicalize(root)? != host.inner.owner.identity {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "application host already owns a different data root",
                ));
            }
            return Ok(host.clone());
        }
        let bootstrap = OwnedBootstrap::acquire(root)?;
        let host = Self::with_mode(bootstrap, !cfg!(test))?;
        host.clone().install(cx);
        Ok(host)
    }
}

#[cfg(test)]
mod tests;

impl AppHost {
    /// Issue a conversation delegation under the current account generation.
    /// A later sign-out, switch or grant revocation invalidates this factory.
    pub fn automation_caller_factory(
        &self,
        grant_refs: Vec<GrantId>,
    ) -> Result<
        flowy_agent::automation_tools::AutomationCallerFactory,
        flowy_automation::PortError,
    > {
        super::automation_caller::factory(
            self.inner.authority.clone(),
            self.inner.grants.clone(),
            self.inner.agent.clone(),
            grant_refs,
        )
    }
}
