//! Black-box integration tests for `IOAuthTokenRepository`.
//!
//! Tests exercise the repository trait interface without knowledge of
//! the underlying SQLite implementation details.

use std::sync::Arc;

use nomifun_db::{
    DbError, IOAuthTokenRepository, SqliteOAuthTokenRepository, UpsertOAuthTokenParams, init_database_memory,
};

async fn repo() -> (Arc<dyn IOAuthTokenRepository>, nomifun_db::Database) {
    let db = init_database_memory().await.unwrap();
    let r = Arc::new(SqliteOAuthTokenRepository::new(db.pool().clone()));
    (r as Arc<dyn IOAuthTokenRepository>, db)
}

/// Baseline parameters for a **legacy / unlinked** token: no client
/// registration minted it and no principal owns it.
///
/// Both identities are `None` on purpose, and that is a legal state:
/// - `registration_id = None` is the documented legacy state (migration `052`:
///   "Legacy rows keep NULL and are treated as `requires_reauthorization` when
///   no registration can be resolved").
/// - `principal_id = None` is the only state currently produced — the column is
///   "reserved for a future multi-user owner, not a `users` row in the current
///   single-device mode" (`nomifun-db/src/id_schema_contract.rs`), and both
///   production writers (`McpOAuthService::persist_token` and the refresh path
///   in `nomifun-mcp/src/oauth_service.rs`) pass `None`.
///
/// These URL-keyed tests exercise exactly that legacy path, so they use the
/// minimal constructor rather than hand-writing the two identity fields.
fn sample_params() -> UpsertOAuthTokenParams<'static> {
    UpsertOAuthTokenParams::new(
        "https://mcp.example.com",
        "enc_access_token_123",
        Some("enc_refresh_token_456"),
        "bearer",
        Some(1700000000000),
    )
}

// -- OA-1: Unauthenticated server --

#[tokio::test]
async fn get_by_url_nonexistent_returns_none() {
    let (r, _db) = repo().await;
    assert!(r.get_by_url("https://nope.com").await.unwrap().is_none());
}

// -- OA-2: Insert and retrieve --

#[tokio::test]
async fn upsert_insert_then_get_returns_token() {
    let (r, _db) = repo().await;
    let inserted = r.upsert(sample_params()).await.unwrap();

    assert_eq!(inserted.server_url, "https://mcp.example.com");
    assert_eq!(inserted.access_token, "enc_access_token_123");
    assert_eq!(inserted.refresh_token.as_deref(), Some("enc_refresh_token_456"));
    assert_eq!(inserted.token_type, "bearer");
    assert_eq!(inserted.expires_at, Some(1700000000000));
    assert!(inserted.created_at > 0);

    let found = r.get_by_url("https://mcp.example.com").await.unwrap().unwrap();
    assert_eq!(found.access_token, "enc_access_token_123");
}

// -- Upsert updates existing --

#[tokio::test]
async fn upsert_updates_existing_token() {
    let (r, _db) = repo().await;
    let original = r.upsert(sample_params()).await.unwrap();

    let updated = r
        .upsert(UpsertOAuthTokenParams::new(
            "https://mcp.example.com",
            "new_access_token",
            None,
            "bearer",
            Some(1800000000000),
        ))
        .await
        .unwrap();

    assert_eq!(updated.server_url, original.server_url);
    assert_eq!(updated.access_token, "new_access_token");
    assert!(updated.refresh_token.is_none());
    assert_eq!(updated.expires_at, Some(1800000000000));
    // created_at preserved from original insert
    assert_eq!(updated.created_at, original.created_at);
}

// -- Upsert without optional fields --

#[tokio::test]
async fn upsert_without_refresh_token_or_expires_at() {
    let (r, _db) = repo().await;
    let token = r
        .upsert(UpsertOAuthTokenParams::new(
            "https://simple.example.com",
            "simple_token",
            None,
            "bearer",
            None,
        ))
        .await
        .unwrap();

    assert!(token.refresh_token.is_none());
    assert!(token.expires_at.is_none());
}

// -- OA-6: Delete existing --

#[tokio::test]
async fn delete_existing_token() {
    let (r, _db) = repo().await;
    r.upsert(sample_params()).await.unwrap();

    r.delete("https://mcp.example.com").await.unwrap();
    assert!(r.get_by_url("https://mcp.example.com").await.unwrap().is_none());
}

// -- OA-7: Delete idempotency (returns NotFound for nonexistent) --

#[tokio::test]
async fn delete_nonexistent_returns_not_found() {
    let (r, _db) = repo().await;
    let err = r.delete("https://nope.com").await.unwrap_err();
    assert!(matches!(err, DbError::NotFound(_)));
}

// -- OA-3: List authenticated URLs --

#[tokio::test]
async fn list_authenticated_urls_empty() {
    let (r, _db) = repo().await;
    let urls = r.list_authenticated_urls().await.unwrap();
    assert!(urls.is_empty());
}

#[tokio::test]
async fn list_authenticated_urls_returns_all() {
    let (r, _db) = repo().await;
    r.upsert(sample_params()).await.unwrap();
    r.upsert(UpsertOAuthTokenParams::new(
        "https://other.example.com",
        "token2",
        None,
        "bearer",
        None,
    ))
    .await
    .unwrap();

    let urls = r.list_authenticated_urls().await.unwrap();
    assert_eq!(urls.len(), 2);
    assert!(urls.contains(&"https://mcp.example.com".to_string()));
    assert!(urls.contains(&"https://other.example.com".to_string()));
}

// -- Delete does not affect other tokens --

#[tokio::test]
async fn delete_one_does_not_affect_others() {
    let (r, _db) = repo().await;
    r.upsert(sample_params()).await.unwrap();
    r.upsert(UpsertOAuthTokenParams::new(
        "https://other.example.com",
        "token2",
        None,
        "bearer",
        None,
    ))
    .await
    .unwrap();

    r.delete("https://mcp.example.com").await.unwrap();

    let urls = r.list_authenticated_urls().await.unwrap();
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0], "https://other.example.com");
}

// -- Full lifecycle --

#[tokio::test]
async fn full_oauth_lifecycle() {
    let (r, _db) = repo().await;

    // Initially no tokens
    assert!(r.list_authenticated_urls().await.unwrap().is_empty());
    assert!(r.get_by_url("https://mcp.example.com").await.unwrap().is_none());

    // Store token
    let token = r.upsert(sample_params()).await.unwrap();
    assert_eq!(token.access_token, "enc_access_token_123");

    // Verify stored
    let urls = r.list_authenticated_urls().await.unwrap();
    assert_eq!(urls.len(), 1);

    // Update token (refresh)
    let refreshed = r
        .upsert(UpsertOAuthTokenParams::new(
            "https://mcp.example.com",
            "refreshed_token",
            Some("new_refresh"),
            "bearer",
            Some(1900000000000),
        ))
        .await
        .unwrap();
    assert_eq!(refreshed.access_token, "refreshed_token");
    assert_eq!(refreshed.created_at, token.created_at);

    // Logout (delete)
    r.delete("https://mcp.example.com").await.unwrap();
    assert!(r.list_authenticated_urls().await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Registration / principal identity columns (migration 052)
//
// `oauth_tokens.registration_id` / `principal_id` are logical links (the v3
// schema forbids physical foreign keys) declared as non-reference id columns in
// `id_schema_contract.rs`, and the repository is the only writer. The tests
// below pin the two states the OAuth service depends on:
//   * unlinked / legacy -> NULL, invisible to registration lookup;
//   * linked            -> readable by registration id, so token refresh can
//                          reuse the original client identity.
// ---------------------------------------------------------------------------

/// A token written through the minimal constructor is a **legacy / unlinked**
/// row: NULL registration (migration `052`: legacy rows keep NULL and require
/// reauthorization when no registration resolves) and NULL principal (the only
/// state the current single-device mode produces). It stays reachable by URL
/// and is never returned by registration lookup.
#[tokio::test]
async fn legacy_token_has_null_registration_and_principal() {
    let (r, _db) = repo().await;
    let stored = r.upsert(sample_params()).await.unwrap();

    assert!(stored.registration_id.is_none(), "legacy row must not claim a registration");
    assert!(stored.principal_id.is_none(), "single-device mode never sets a principal");

    let found = r.get_by_url("https://mcp.example.com").await.unwrap().unwrap();
    assert!(found.registration_id.is_none());
    assert!(found.principal_id.is_none());

    // Nothing links to it, so registration lookup cannot resolve it.
    assert!(r.get_by_registration(1).await.unwrap().is_none());
}

/// A token minted by a client registration round-trips the link and is
/// resolvable by `get_by_registration` (the lookup the refresh path uses to
/// pick the original client identity) while staying resolvable by URL.
#[tokio::test]
async fn linked_token_round_trips_registration_identity() {
    let (r, _db) = repo().await;
    let stored = r
        .upsert(UpsertOAuthTokenParams::new(
            "https://mcp.example.com",
            "tok-a",
            Some("refresh-a"),
            "bearer",
            Some(1700000000000),
        )
        .with_registration(42))
        .await
        .unwrap();

    assert_eq!(stored.registration_id, Some(42));
    assert!(stored.principal_id.is_none());

    let linked = r.get_by_registration(42).await.unwrap().unwrap();
    assert_eq!(linked.server_url, "https://mcp.example.com");
    assert_eq!(linked.access_token, "tok-a");
    assert_eq!(linked.refresh_token.as_deref(), Some("refresh-a"));
    assert_eq!(linked.registration_id, Some(42));
    assert_eq!(linked.principal_id, None);

    // A different registration id resolves nothing; the URL lookup still works.
    assert!(r.get_by_registration(43).await.unwrap().is_none());
    assert!(r.get_by_url("https://mcp.example.com").await.unwrap().is_some());
}

/// `principal_id` is reserved for a future multi-user owner — every production
/// writer today passes `None`. The repository contract still has to carry a
/// supplied value through unchanged, otherwise the reserved column could never
/// be adopted without a schema change.
#[tokio::test]
async fn principal_id_round_trips_when_supplied() {
    let (r, _db) = repo().await;
    let mut params = UpsertOAuthTokenParams::new(
        "https://mcp.example.com",
        "tok-p",
        None,
        "bearer",
        None,
    )
    .with_registration(7);
    params.principal_id = Some("principal-1");

    let stored = r.upsert(params).await.unwrap();
    assert_eq!(stored.registration_id, Some(7));
    assert_eq!(stored.principal_id.as_deref(), Some("principal-1"));

    let found = r.get_by_url("https://mcp.example.com").await.unwrap().unwrap();
    assert_eq!(found.principal_id.as_deref(), Some("principal-1"));
    // Registration lookup is not principal-scoped: it returns the row as stored.
    let linked = r.get_by_registration(7).await.unwrap().unwrap();
    assert_eq!(linked.principal_id.as_deref(), Some("principal-1"));
}

/// Registration lookup is scoped per row: two tokens minted by different
/// registrations never resolve to each other, and each token keeps its own
/// server URL.
#[tokio::test]
async fn registration_lookup_is_scoped_per_row() {
    let (r, _db) = repo().await;
    r.upsert(
        UpsertOAuthTokenParams::new("https://a.example.com", "tok-a", None, "bearer", None)
            .with_registration(101),
    )
    .await
    .unwrap();
    r.upsert(
        UpsertOAuthTokenParams::new("https://b.example.com", "tok-b", None, "bearer", None)
            .with_registration(202),
    )
    .await
    .unwrap();
    // An unlinked row must not shadow either of them.
    r.upsert(UpsertOAuthTokenParams::new(
        "https://c.example.com",
        "tok-c",
        None,
        "bearer",
        None,
    ))
    .await
    .unwrap();

    assert_eq!(
        r.get_by_registration(101).await.unwrap().unwrap().server_url,
        "https://a.example.com"
    );
    assert_eq!(
        r.get_by_registration(202).await.unwrap().unwrap().server_url,
        "https://b.example.com"
    );
    assert!(r.get_by_registration(303).await.unwrap().is_none());
    assert_eq!(r.list_authenticated_urls().await.unwrap().len(), 3);
}

/// The upsert is a **full-row** write (`ON CONFLICT(server_url) DO UPDATE SET
/// registration_id = excluded.registration_id`), so the identity link is
/// whatever the caller passes on every write:
///   * carrying the stored fields forward (what the refresh path in
///     `nomifun-mcp/src/oauth_service.rs` does, `registration_id: row.registration_id`
///     / `principal_id: row.principal_id.as_deref()`) keeps the link intact;
///   * omitting them clears the link back to the legacy/unlinked state.
/// Both directions are pinned so a refresh can never silently orphan the
/// registration identity it must reuse.
#[tokio::test]
async fn reupsert_carries_or_clears_registration_link() {
    let (r, _db) = repo().await;
    let mut params = UpsertOAuthTokenParams::new(
        "https://mcp.example.com",
        "tok-1",
        Some("refresh-1"),
        "bearer",
        None,
    )
    .with_registration(55);
    params.principal_id = Some("principal-9");
    let first = r.upsert(params).await.unwrap();
    assert_eq!(first.registration_id, Some(55));

    // Refresh-style re-upsert: carry the row's own identity forward.
    let stored = r.get_by_registration(55).await.unwrap().unwrap();
    let mut carried = UpsertOAuthTokenParams::new(
        "https://mcp.example.com",
        "tok-2",
        Some("refresh-2"),
        "bearer",
        Some(1900000000000),
    );
    carried.registration_id = stored.registration_id;
    carried.principal_id = stored.principal_id.as_deref();
    let refreshed = r.upsert(carried).await.unwrap();

    assert_eq!(refreshed.access_token, "tok-2");
    assert_eq!(refreshed.registration_id, Some(55), "refresh must keep the registration link");
    assert_eq!(refreshed.principal_id.as_deref(), Some("principal-9"));
    assert_eq!(refreshed.created_at, first.created_at, "upsert preserves created_at");
    assert_eq!(
        r.get_by_registration(55).await.unwrap().unwrap().access_token,
        "tok-2"
    );

    // Omitting the identity fields on a later write clears the link.
    let cleared = r
        .upsert(UpsertOAuthTokenParams::new(
            "https://mcp.example.com",
            "tok-3",
            None,
            "bearer",
            None,
        ))
        .await
        .unwrap();
    assert!(cleared.registration_id.is_none());
    assert!(cleared.principal_id.is_none());
    assert!(r.get_by_registration(55).await.unwrap().is_none());
    // The row itself survives — only the link is gone.
    assert_eq!(
        r.get_by_url("https://mcp.example.com").await.unwrap().unwrap().access_token,
        "tok-3"
    );
}
