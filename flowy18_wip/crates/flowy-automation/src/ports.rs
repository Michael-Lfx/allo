use crate::{
    AccountGeneration, CapabilityRequest, GrantId, PortError, PortFuture,
    PrincipalId, RunId,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockReading {
    pub utc_ms: i64,
    /// Only comparable within this host lifetime; never persisted.
    pub monotonic: Duration,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> ClockReading;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessOperation {
    Command,
    Query,
    Start,
    ModelRequest,
    ModelRetry,
    ToolDispatch,
    ReadResult,
    DeliverResult,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostAccessRequest {
    pub principal: PrincipalId,
    pub account_generation: AccountGeneration,
    pub run_id: Option<RunId>,
    pub grant_ref: Option<GrantId>,
    pub operation: AccessOperation,
    pub requested: CapabilityRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedReason {
    AccountUnavailable,
    CredentialUnavailable,
    ModelUnavailable,
    ApprovalRequired,
    WorkspaceUnavailable,
    StorageUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostAccessDecision {
    Allowed,
    Blocked(BlockedReason),
    Denied,
}

/// Implementations recheck the current principal/generation/grant at every
/// actual request or retry. An Allowed value is not a reusable credential.
pub trait HostAccess: Send + Sync {
    fn check<'a>(
        &'a self,
        request: &'a HostAccessRequest,
    ) -> PortFuture<'a, Result<HostAccessDecision, PortError>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionPoint {
    CreateRun,
    StartAttempt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionRequest {
    pub principal: PrincipalId,
    pub run_id: Option<RunId>,
    pub point: AdmissionPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapacityReason {
    QuotaExceeded,
    MeasurementUnavailable,
    PolicyUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionDecision {
    Allowed,
    SuspendedStorage(CapacityReason),
}

/// Retention/storage admission only. Workspace and execution-slot tickets are
/// owned by the shared execution coordinator, not this port. No default allow
/// implementation is provided.
pub trait AdmissionPolicy: Send + Sync {
    fn check<'a>(
        &'a self,
        request: &'a AdmissionRequest,
    ) -> PortFuture<'a, Result<AdmissionDecision, PortError>>;
}
