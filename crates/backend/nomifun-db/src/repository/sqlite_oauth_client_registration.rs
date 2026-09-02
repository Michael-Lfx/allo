use sqlx::SqlitePool;

use crate::error::DbError;
use crate::models::OAuthClientRegistrationRow;
use crate::repository::oauth_client_registration::{
    IOAuthClientRegistrationRepository, UpsertOAuthClientRegistrationParams,
};

/// SQLite-backed implementation of [`IOAuthClientRegistrationRepository`].
#[derive(Clone, Debug)]
pub struct SqliteOAuthClientRegistrationRepository {
    pool: SqlitePool,
}

impl SqliteOAuthClientRegistrationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl IOAuthClientRegistrationRepository for SqliteOAuthClientRegistrationRepository {
    async fn get_by_identity_key(
        &self,
        mcp_server_url: &str,
        resource_identifier: &str,
        authorization_server_issuer: &str,
        redirect_uri: &str,
    ) -> Result<Option<OAuthClientRegistrationRow>, DbError> {
        let row = sqlx::query_as::<_, OAuthClientRegistrationRow>(
            "SELECT * FROM oauth_client_registrations \
             WHERE mcp_server_url = ? AND resource_identifier = ? \
               AND authorization_server_issuer = ? AND redirect_uri = ?",
        )
        .bind(mcp_server_url)
        .bind(resource_identifier)
        .bind(authorization_server_issuer)
        .bind(redirect_uri)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn get_by_id(&self, id: i64) -> Result<Option<OAuthClientRegistrationRow>, DbError> {
        let row = sqlx::query_as::<_, OAuthClientRegistrationRow>(
            "SELECT * FROM oauth_client_registrations WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn insert(
        &self,
        params: UpsertOAuthClientRegistrationParams<'_>,
    ) -> Result<OAuthClientRegistrationRow, DbError> {
        let now = nomifun_common::now_ms();
        sqlx::query(
            "INSERT INTO oauth_client_registrations \
                (mcp_server_url, resource_identifier, authorization_server_issuer, \
                 redirect_uri, registration_mode, client_id, client_secret_ref, \
                 registration_access_token_ref, client_id_issued_at, \
                 client_secret_expires_at, registration_client_uri, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(params.mcp_server_url)
        .bind(params.resource_identifier)
        .bind(params.authorization_server_issuer)
        .bind(params.redirect_uri)
        .bind(params.registration_mode)
        .bind(params.client_id)
        .bind(params.client_secret_ref)
        .bind(params.registration_access_token_ref)
        .bind(params.client_id_issued_at)
        .bind(params.client_secret_expires_at)
        .bind(params.registration_client_uri)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Fetch the row to return the generated id.
        let row = self
            .get_by_identity_key(
                params.mcp_server_url,
                params.resource_identifier,
                params.authorization_server_issuer,
                params.redirect_uri,
            )
            .await?
            .ok_or_else(|| DbError::Init("Registered row not found after insert".to_string()))?;
        Ok(row)
    }

    async fn upsert(
        &self,
        params: UpsertOAuthClientRegistrationParams<'_>,
    ) -> Result<OAuthClientRegistrationRow, DbError> {
        let now = nomifun_common::now_ms();
        sqlx::query(
            "INSERT INTO oauth_client_registrations \
                (mcp_server_url, resource_identifier, authorization_server_issuer, \
                 redirect_uri, registration_mode, client_id, client_secret_ref, \
                 registration_access_token_ref, client_id_issued_at, \
                 client_secret_expires_at, registration_client_uri, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(mcp_server_url, resource_identifier, authorization_server_issuer, redirect_uri) \
             DO UPDATE SET \
                registration_mode = excluded.registration_mode, \
                client_id = excluded.client_id, \
                client_secret_ref = excluded.client_secret_ref, \
                registration_access_token_ref = excluded.registration_access_token_ref, \
                client_id_issued_at = excluded.client_id_issued_at, \
                client_secret_expires_at = excluded.client_secret_expires_at, \
                registration_client_uri = excluded.registration_client_uri, \
                updated_at = excluded.updated_at",
        )
        .bind(params.mcp_server_url)
        .bind(params.resource_identifier)
        .bind(params.authorization_server_issuer)
        .bind(params.redirect_uri)
        .bind(params.registration_mode)
        .bind(params.client_id)
        .bind(params.client_secret_ref)
        .bind(params.registration_access_token_ref)
        .bind(params.client_id_issued_at)
        .bind(params.client_secret_expires_at)
        .bind(params.registration_client_uri)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let row = self
            .get_by_identity_key(
                params.mcp_server_url,
                params.resource_identifier,
                params.authorization_server_issuer,
                params.redirect_uri,
            )
            .await?
            .ok_or_else(|| DbError::Init("Upserted registration row not found".to_string()))?;
        Ok(row)
    }

    async fn list_by_server_url(
        &self,
        mcp_server_url: &str,
    ) -> Result<Vec<OAuthClientRegistrationRow>, DbError> {
        let rows = sqlx::query_as::<_, OAuthClientRegistrationRow>(
            "SELECT * FROM oauth_client_registrations WHERE mcp_server_url = ? \
             ORDER BY created_at ASC",
        )
        .bind(mcp_server_url)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn delete(&self, id: i64) -> Result<(), DbError> {
        let result = sqlx::query("DELETE FROM oauth_client_registrations WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError::NotFound(format!(
                "OAuth client registration '{id}' not found"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_database_memory;

    async fn setup() -> (SqliteOAuthClientRegistrationRepository, crate::Database) {
        let db = init_database_memory().await.unwrap();
        let repo = SqliteOAuthClientRegistrationRepository::new(db.pool().clone());
        (repo, db)
    }

    fn sample<'a>() -> UpsertOAuthClientRegistrationParams<'a> {
        UpsertOAuthClientRegistrationParams {
            mcp_server_url: "https://mcp.example.com/mcp",
            resource_identifier: "https://mcp.example.com/.well-known/oauth-protected-resource/mcp/",
            authorization_server_issuer: "https://auth.example.com",
            redirect_uri: "http://127.0.0.1:12345/callback",
            registration_mode: "dynamic",
            client_id: "dyn-client-123",
            client_secret_ref: None,
            registration_access_token_ref: Some("secret:v1:rat-1"),
            client_id_issued_at: Some(1700000000000),
            client_secret_expires_at: None,
            registration_client_uri: Some("https://auth.example.com/register/dyn-client-123"),
        }
    }

    #[tokio::test]
    async fn insert_then_get_by_identity_key() {
        let (repo, _db) = setup().await;
        let row = repo.insert(sample()).await.unwrap();
        assert_eq!(row.client_id, "dyn-client-123");
        assert_eq!(row.registration_mode, "dynamic");
        assert!(row.id > 0);
        assert_eq!(row.client_secret_expires_at, None);

        let found = repo
            .get_by_identity_key(
                "https://mcp.example.com/mcp",
                "https://mcp.example.com/.well-known/oauth-protected-resource/mcp/",
                "https://auth.example.com",
                "http://127.0.0.1:12345/callback",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, row.id);
        assert_eq!(found.client_id, "dyn-client-123");
    }

    #[tokio::test]
    async fn identity_key_is_exact() {
        let (repo, _db) = setup().await;
        repo.insert(sample()).await.unwrap();
        // Different resource or issuer or redirect → different identity.
        assert!(repo
            .get_by_identity_key(
                "https://mcp.example.com/mcp",
                "other-resource",
                "https://auth.example.com",
                "http://127.0.0.1:12345/callback",
            )
            .await
            .unwrap()
            .is_none());
        assert!(repo
            .get_by_identity_key(
                "https://mcp.example.com/mcp",
                "https://mcp.example.com/.well-known/oauth-protected-resource/mcp/",
                "https://other.example.com",
                "http://127.0.0.1:12345/callback",
            )
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn upsert_updates_existing_identity() {
        let (repo, _db) = setup().await;
        let original = repo.insert(sample()).await.unwrap();
        let mut updated = sample();
        updated.client_id = "dyn-client-replaced";
        let row = repo.upsert(updated).await.unwrap();
        assert_eq!(row.id, original.id);
        assert_eq!(row.client_id, "dyn-client-replaced");
        // created_at preserved, updated_at advanced
        assert_eq!(row.created_at, original.created_at);
        assert!(row.updated_at >= original.updated_at);
    }

    #[tokio::test]
    async fn get_by_id_and_list_and_delete() {
        let (repo, _db) = setup().await;
        let row = repo.insert(sample()).await.unwrap();

        let by_id = repo.get_by_id(row.id).await.unwrap().unwrap();
        assert_eq!(by_id.client_id, row.client_id);

        let list = repo
            .list_by_server_url("https://mcp.example.com/mcp")
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        repo.delete(row.id).await.unwrap();
        assert!(repo.get_by_id(row.id).await.unwrap().is_none());
        assert!(matches!(
            repo.delete(row.id).await.unwrap_err(),
            DbError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn pre_registered_mode_round_trip() {
        let (repo, _db) = setup().await;
        let mut params = sample();
        params.registration_mode = "pre_registered";
        params.client_secret_ref = Some("secret:v1:cs-1");
        let row = repo.insert(params).await.unwrap();
        assert_eq!(row.registration_mode, "pre_registered");
        assert_eq!(row.client_secret_ref.as_deref(), Some("secret:v1:cs-1"));
    }
}