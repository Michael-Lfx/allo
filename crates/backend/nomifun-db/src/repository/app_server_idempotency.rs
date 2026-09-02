use crate::error::DbError;
use crate::models::{
    AppServerIdempotencyReceiptRow, AppServerIdempotencyScope,
    NewAppServerIdempotencyReceipt,
};

/// Result of looking up one completed App Server idempotency scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServerIdempotencyLookup {
    Missing,
    Replay(AppServerIdempotencyReceiptRow),
    Conflict(AppServerIdempotencyReceiptRow),
}

/// Result of atomically committing a completed App Server response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServerIdempotencyCommit {
    Stored(AppServerIdempotencyReceiptRow),
    Replay(AppServerIdempotencyReceiptRow),
    Conflict(AppServerIdempotencyReceiptRow),
}

/// Durable completed-response storage for App Server idempotency keys.
#[async_trait::async_trait]
pub trait IAppServerIdempotencyRepository: Send + Sync {
    /// Return a matching completed response, a conflicting receipt, or absence.
    async fn lookup(
        &self,
        scope: &AppServerIdempotencyScope,
        request_fingerprint: &str,
    ) -> Result<AppServerIdempotencyLookup, DbError>;

    /// Atomically store one completed response.
    ///
    /// A concurrent same-scope commit is classified by its stored fingerprint;
    /// uniqueness races are never exposed to callers as raw database errors.
    async fn commit_completed(
        &self,
        receipt: &NewAppServerIdempotencyReceipt,
    ) -> Result<AppServerIdempotencyCommit, DbError>;
}
