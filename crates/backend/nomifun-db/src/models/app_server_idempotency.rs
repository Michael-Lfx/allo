use serde::{Deserialize, Serialize};

/// Immutable completed-response receipt for one App Server idempotency scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppServerIdempotencyReceiptRow {
    pub id: i64,
    pub principal_id: String,
    pub client_id: String,
    pub method: String,
    pub idempotency_key: String,
    pub request_fingerprint: String,
    pub response_json: String,
    pub created_at: i64,
}

/// Composite identity used to look up one App Server idempotency receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppServerIdempotencyScope {
    pub principal_id: String,
    pub client_id: String,
    pub method: String,
    pub idempotency_key: String,
}

/// Values supplied when committing a completed App Server response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAppServerIdempotencyReceipt {
    pub scope: AppServerIdempotencyScope,
    pub request_fingerprint: String,
    pub response_json: String,
    pub created_at: i64,
}
