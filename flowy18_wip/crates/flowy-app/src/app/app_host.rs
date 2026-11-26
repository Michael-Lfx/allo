//! Process-owned execution and exclusive ownership of the application store.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flowy_agent::{AgentHandle, AgentInitialization, Model, ProviderCatalog};
use tokio::runtime::Runtime;

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
}

#[derive(Debug, Default)]
struct CloudState {
    revision: u64,
    selection: Option<SharedCloudModel>,
}

/// Window display state for the currently configured shared agent. No token
/// is copied into this projection; account credentials stay in the existing
/// account flow until the execution authority is introduced in A07.
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
        // store owner until their actual work has stopped. A12 adds the
        // bounded cooperative quit protocol before this final runtime teardown.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl gpui::Global for AppHost {}

#[derive(Debug)]
pub(super) struct ModelSignal;

struct ModelEvents(gpui::Entity<ModelSignal>);

impl gpui::Global for ModelEvents {}

impl AppHost {
    pub fn start(bootstrap: OwnedBootstrap) -> io::Result<Self> {
        Self::with_mode(bootstrap, true)
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
        Ok(Self {
            inner: Arc::new(HostInner {
                agent,
                runtime: Some(runtime),
                owner,
                cloud: Mutex::new(CloudState::default()),
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.owner.root
    }

    pub fn initialization_state(&self) -> AgentInitialization {
        self.inner.agent.initialization_state()
    }

    pub(super) fn agent(&self) -> AgentHandle {
        self.inner.agent.clone()
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
        cx.set_global(host.clone());
        Ok(host)
    }
}

#[cfg(test)]
mod tests;
