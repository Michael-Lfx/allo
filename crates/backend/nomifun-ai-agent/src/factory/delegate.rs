//! Host-backed `nomi_delegate` wiring: the per-conversation sink provider and the
//! late-binding slot that carries it into the agent factory.
//!
//! Why a slot: `AppServices` must build the Agent factory before the process has
//! finished constructing its Agent Execution facade (`router::state::
//! build_agent_execution_engine`). The factory receives a clone of this slot at
//! construction time; the composition root installs exactly one provider once the
//! facade exists.
//!
//! Unlike the browser-lane slot this one is **not** fail-closed: a host that never
//! installs a provider simply has no host-backed delegate, which is a legitimate
//! composition (the embedded deployment covers CLI hosts). The security-relevant
//! direction is the opposite one — an installed provider must never be silently
//! replaced, so [`DelegateSinkProviderSlot::install`] accepts only the first call.

use std::fmt;
use std::sync::{Arc, OnceLock};

use nomifun_common::AppError;

/// Re-exported for backend composition (the single-bridge rule): a host that
/// implements this seam must not depend on the agent crates directly.
pub use nomi_agent::host_delegate_tool::{HostDelegateSink, HostDelegateTool};

/// One host-backed delegate sink per conversation.
///
/// Bound `(owner_id, conversation_id)` exactly like the cron and meeting sinks, so
/// the model cannot address a conversation other than the one it is running in.
pub trait DelegateSinkProvider: Send + Sync {
    fn sink_for(
        &self,
        owner_id: &str,
        conversation_id: &str,
    ) -> Arc<dyn nomi_agent::host_delegate_tool::HostDelegateSink>;
}

/// Late-wired [`DelegateSinkProvider`] handle passed to the agent factory.
#[derive(Clone, Default)]
pub struct DelegateSinkProviderSlot {
    provider: Arc<OnceLock<Arc<dyn DelegateSinkProvider>>>,
}

impl DelegateSinkProviderSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the process's single provider. A second install is a conflict
    /// rather than a silent replacement: two providers would mean two different
    /// execution facades behind the same tool name.
    pub fn install(&self, provider: Arc<dyn DelegateSinkProvider>) -> Result<(), AppError> {
        self.provider.set(provider).map_err(|_| {
            AppError::Conflict("the host-backed delegate provider is already installed".to_owned())
        })
    }

    /// `None` until the composition root installs the provider.
    pub fn get(&self) -> Option<Arc<dyn DelegateSinkProvider>> {
        self.provider.get().cloned()
    }
}

impl fmt::Debug for DelegateSinkProviderSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DelegateSinkProviderSlot")
            .field("installed", &self.provider.get().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubProvider;

    impl DelegateSinkProvider for StubProvider {
        fn sink_for(&self, _owner_id: &str, _conversation_id: &str) -> Arc<dyn HostDelegateSink> {
            unimplemented!("the slot test never asks for a sink")
        }
    }

    #[test]
    fn a_fresh_slot_is_empty_and_installs_exactly_once() {
        let slot = DelegateSinkProviderSlot::new();
        // Empty is the "this host has no durable facade to delegate to" state, not
        // an error: the factory then simply registers no such tool.
        assert!(slot.get().is_none());

        assert!(slot.install(Arc::new(StubProvider)).is_ok());
        assert!(slot.get().is_some());

        // A second provider would mean two execution facades behind one tool name.
        let second = slot.install(Arc::new(StubProvider));
        assert!(matches!(second, Err(AppError::Conflict(_))), "{second:?}");
    }

    /// The slot is cloned into the already-built factory, so installing through one
    /// clone must be visible through the other — that is the whole late-binding
    /// contract.
    #[test]
    fn installation_is_visible_through_a_clone() {
        let slot = DelegateSinkProviderSlot::new();
        let factory_handle = slot.clone();
        assert!(factory_handle.get().is_none());

        slot.install(Arc::new(StubProvider)).unwrap();
        assert!(factory_handle.get().is_some());
    }
}
