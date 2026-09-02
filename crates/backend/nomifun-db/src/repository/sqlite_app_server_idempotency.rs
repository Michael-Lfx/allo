use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::{
    AppServerIdempotencyReceiptRow, AppServerIdempotencyScope,
    NewAppServerIdempotencyReceipt,
};
use crate::repository::app_server_idempotency::{
    AppServerIdempotencyCommit, AppServerIdempotencyLookup,
    IAppServerIdempotencyRepository,
};

const PRINCIPAL_ID_MAX_BYTES: usize = 256;
const CLIENT_ID_MAX_BYTES: usize = 256;
const METHOD_MAX_BYTES: usize = 512;
const IDEMPOTENCY_KEY_MAX_BYTES: usize = 1024;

#[derive(Clone, Debug)]
pub struct SqliteAppServerIdempotencyRepository {
    pool: SqlitePool,
}

impl SqliteAppServerIdempotencyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn validate_bounded_text(field: &str, value: &str, max_bytes: usize) -> Result<(), DbError> {
    if value.is_empty() || value.len() > max_bytes {
        return Err(DbError::Conflict(format!(
            "App Server idempotency {field} must contain between 1 and {max_bytes} bytes"
        )));
    }
    Ok(())
}

fn validate_scope(scope: &AppServerIdempotencyScope) -> Result<(), DbError> {
    validate_bounded_text("principal_id", &scope.principal_id, PRINCIPAL_ID_MAX_BYTES)?;
    validate_bounded_text("client_id", &scope.client_id, CLIENT_ID_MAX_BYTES)?;
    validate_bounded_text("method", &scope.method, METHOD_MAX_BYTES)?;
    validate_bounded_text(
        "idempotency_key",
        &scope.idempotency_key,
        IDEMPOTENCY_KEY_MAX_BYTES,
    )
}

fn validate_fingerprint(request_fingerprint: &str) -> Result<(), DbError> {
    let Some(digest) = request_fingerprint.strip_prefix("sha256:") else {
        return Err(DbError::Conflict(
            "App Server request fingerprint must be sha256: plus 64 lowercase hex characters"
                .to_owned(),
        ));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(DbError::Conflict(
            "App Server request fingerprint must be sha256: plus 64 lowercase hex characters"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_response_json(response_json: &str) -> Result<(), DbError> {
    let value: serde_json::Value = serde_json::from_str(response_json).map_err(|error| {
        DbError::Conflict(format!(
            "App Server idempotency response payload is invalid JSON: {error}"
        ))
    })?;
    if !value.is_object() {
        return Err(DbError::Conflict(
            "App Server idempotency response payload must be a JSON object".to_owned(),
        ));
    }
    Ok(())
}

fn classify_lookup(
    row: AppServerIdempotencyReceiptRow,
    request_fingerprint: &str,
) -> AppServerIdempotencyLookup {
    if row.request_fingerprint == request_fingerprint {
        AppServerIdempotencyLookup::Replay(row)
    } else {
        AppServerIdempotencyLookup::Conflict(row)
    }
}

fn classify_commit(
    row: AppServerIdempotencyReceiptRow,
    request_fingerprint: &str,
) -> AppServerIdempotencyCommit {
    if row.request_fingerprint == request_fingerprint {
        AppServerIdempotencyCommit::Replay(row)
    } else {
        AppServerIdempotencyCommit::Conflict(row)
    }
}

async fn load_receipt(
    pool: &SqlitePool,
    scope: &AppServerIdempotencyScope,
) -> Result<Option<AppServerIdempotencyReceiptRow>, DbError> {
    sqlx::query_as::<_, AppServerIdempotencyReceiptRow>(
        "SELECT * FROM app_server_idempotency_receipts \
         WHERE principal_id = ? AND client_id = ? AND method = ? AND idempotency_key = ?",
    )
    .bind(&scope.principal_id)
    .bind(&scope.client_id)
    .bind(&scope.method)
    .bind(&scope.idempotency_key)
    .fetch_optional(pool)
    .await
    .map_err(DbError::Query)
}

#[async_trait::async_trait]
impl IAppServerIdempotencyRepository for SqliteAppServerIdempotencyRepository {
    async fn lookup(
        &self,
        scope: &AppServerIdempotencyScope,
        request_fingerprint: &str,
    ) -> Result<AppServerIdempotencyLookup, DbError> {
        validate_scope(scope)?;
        validate_fingerprint(request_fingerprint)?;
        Ok(match load_receipt(&self.pool, scope).await? {
            Some(row) => classify_lookup(row, request_fingerprint),
            None => AppServerIdempotencyLookup::Missing,
        })
    }

    async fn commit_completed(
        &self,
        receipt: &NewAppServerIdempotencyReceipt,
    ) -> Result<AppServerIdempotencyCommit, DbError> {
        validate_scope(&receipt.scope)?;
        validate_fingerprint(&receipt.request_fingerprint)?;
        validate_response_json(&receipt.response_json)?;

        let inserted = sqlx::query_as::<_, AppServerIdempotencyReceiptRow>(
            "INSERT INTO app_server_idempotency_receipts (\
                principal_id, client_id, method, idempotency_key, \
                request_fingerprint, response_json, created_at\
             ) VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(principal_id, client_id, method, idempotency_key) DO NOTHING \
             RETURNING *",
        )
        .bind(&receipt.scope.principal_id)
        .bind(&receipt.scope.client_id)
        .bind(&receipt.scope.method)
        .bind(&receipt.scope.idempotency_key)
        .bind(&receipt.request_fingerprint)
        .bind(&receipt.response_json)
        .bind(receipt.created_at)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = inserted {
            return Ok(AppServerIdempotencyCommit::Stored(row));
        }

        let existing = load_receipt(&self.pool, &receipt.scope)
            .await?
            .ok_or_else(|| {
                DbError::Init(
                    "App Server idempotency conflict did not resolve to a stored receipt"
                        .to_owned(),
                )
            })?;
        Ok(classify_commit(existing, &receipt.request_fingerprint))
    }
}
