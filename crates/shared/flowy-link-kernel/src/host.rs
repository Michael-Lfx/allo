use serde::{Deserialize, Serialize};

use crate::endpoint::Endpoint;

/// Where a command runs. Only [`HostKind::RemoteAllo`] exists today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    RemoteAllo,
    OnDevice,
}

/// A named ability the active host advertises. UI hides actions the host omits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Conversation,
    Models,
    Knowledge,
    Files,
    Terminal,
    Browser,
    Computer,
    Meeting,
    Settings,
}

/// One user action. `id` is the idempotency key. `name` is the stable operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub id: String,
    pub host_id: String,
    pub name: String,
    pub body: serde_json::Value,
}

/// Saved coordinates for one host. The address list is replaceable; the fingerprint is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRecord {
    pub fingerprint: String,
    pub port: u16,
    pub name: String,
    pub endpoints: Vec<Endpoint>,
}

/// The seam a future on-device agent must implement. Methods stay synchronous so
/// the UI bindings can wrap I/O themselves.
pub trait ExecutionHost {
    fn id(&self) -> &str;
    fn kind(&self) -> HostKind;
    fn record(&self) -> &HostRecord;
    fn capabilities(&self) -> &[Capability];
}

/// PC host. Capabilities are the full installation surface.
#[derive(Debug, Clone)]
pub struct RemoteAlloHost {
    pub record: HostRecord,
}

impl ExecutionHost for RemoteAlloHost {
    fn id(&self) -> &str {
        &self.record.fingerprint
    }

    fn kind(&self) -> HostKind {
        HostKind::RemoteAllo
    }

    fn record(&self) -> &HostRecord {
        &self.record
    }

    fn capabilities(&self) -> &[Capability] {
        &[
            Capability::Conversation,
            Capability::Models,
            Capability::Knowledge,
            Capability::Files,
            Capability::Terminal,
            Capability::Browser,
            Capability::Computer,
            Capability::Meeting,
            Capability::Settings,
        ]
    }
}
