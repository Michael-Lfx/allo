use std::sync::Arc;

use nomifun_db::{
    AppServerIdempotencyCommit, AppServerIdempotencyLookup, AppServerIdempotencyScope, DbError,
    IAppServerIdempotencyRepository, NewAppServerIdempotencyReceipt,
    SqliteAppServerIdempotencyRepository, init_database_memory,
};

const FINGERPRINT_A: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FINGERPRINT_B: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn scope() -> AppServerIdempotencyScope {
    AppServerIdempotencyScope {
        principal_id: "principal-1".to_owned(),
        client_id: "agent-store@1".to_owned(),
        method: "agent/run".to_owned(),
        idempotency_key: "operation-1".to_owned(),
    }
}

fn receipt(
    scope: AppServerIdempotencyScope,
    fingerprint: &str,
    response_json: &str,
) -> NewAppServerIdempotencyReceipt {
    NewAppServerIdempotencyReceipt {
        scope,
        request_fingerprint: fingerprint.to_owned(),
        response_json: response_json.to_owned(),
        created_at: 1_700_000_000_000,
    }
}

async fn repository() -> (
    nomifun_db::Database,
    Arc<dyn IAppServerIdempotencyRepository>,
) {
    let database = init_database_memory().await.unwrap();
    let repository: Arc<dyn IAppServerIdempotencyRepository> = Arc::new(
        SqliteAppServerIdempotencyRepository::new(database.pool().clone()),
    );
    (database, repository)
}

#[tokio::test]
async fn missing_scope_can_be_stored_and_replayed() {
    let (_database, repository) = repository().await;
    let scope = scope();

    assert!(matches!(
        repository.lookup(&scope, FINGERPRINT_A).await.unwrap(),
        AppServerIdempotencyLookup::Missing
    ));

    let stored = repository
        .commit_completed(&receipt(
            scope.clone(),
            FINGERPRINT_A,
            r#"{"run_id":"run-1","status":"planning"}"#,
        ))
        .await
        .unwrap();
    let stored = match stored {
        AppServerIdempotencyCommit::Stored(row) => row,
        other => panic!("expected Stored, got {other:?}"),
    };
    assert!(stored.id > 0);
    assert_eq!(stored.response_json, r#"{"run_id":"run-1","status":"planning"}"#);

    let replay = repository.lookup(&scope, FINGERPRINT_A).await.unwrap();
    assert!(matches!(
        replay,
        AppServerIdempotencyLookup::Replay(row) if row.id == stored.id
    ));

    let replay = repository
        .commit_completed(&receipt(
            scope,
            FINGERPRINT_A,
            r#"{"run_id":"different-response-is-ignored"}"#,
        ))
        .await
        .unwrap();
    assert!(matches!(
        replay,
        AppServerIdempotencyCommit::Replay(row)
            if row.id == stored.id
                && row.response_json == r#"{"run_id":"run-1","status":"planning"}"#
    ));
}

#[tokio::test]
async fn reused_scope_with_different_fingerprint_is_a_conflict() {
    let (_database, repository) = repository().await;
    let scope = scope();
    repository
        .commit_completed(&receipt(scope.clone(), FINGERPRINT_A, r#"{"ok":true}"#))
        .await
        .unwrap();

    assert!(matches!(
        repository.lookup(&scope, FINGERPRINT_B).await.unwrap(),
        AppServerIdempotencyLookup::Conflict(row)
            if row.request_fingerprint == FINGERPRINT_A
    ));
    assert!(matches!(
        repository
            .commit_completed(&receipt(scope, FINGERPRINT_B, r#"{"ok":false}"#))
            .await
            .unwrap(),
        AppServerIdempotencyCommit::Conflict(row)
            if row.request_fingerprint == FINGERPRINT_A
                && row.response_json == r#"{"ok":true}"#
    ));
}

#[tokio::test]
async fn principal_client_method_and_key_each_isolate_the_scope() {
    let (_database, repository) = repository().await;
    let base = scope();
    repository
        .commit_completed(&receipt(base.clone(), FINGERPRINT_A, r#"{"scope":"base"}"#))
        .await
        .unwrap();

    let variants = [
        AppServerIdempotencyScope {
            principal_id: "principal-2".to_owned(),
            ..base.clone()
        },
        AppServerIdempotencyScope {
            client_id: "agent-store@2".to_owned(),
            ..base.clone()
        },
        AppServerIdempotencyScope {
            method: "run/run-1/cancel".to_owned(),
            ..base.clone()
        },
        AppServerIdempotencyScope {
            idempotency_key: "operation-2".to_owned(),
            ..base
        },
    ];

    for variant in variants {
        assert!(matches!(
            repository.lookup(&variant, FINGERPRINT_A).await.unwrap(),
            AppServerIdempotencyLookup::Missing
        ));
    }
}

#[tokio::test]
async fn concurrent_same_scope_commits_produce_one_stored_receipt() {
    let (_database, repository) = repository().await;
    let first = receipt(scope(), FINGERPRINT_A, r#"{"winner":"first"}"#);
    let second = receipt(scope(), FINGERPRINT_A, r#"{"winner":"second"}"#);

    let (left, right) = tokio::join!(
        repository.commit_completed(&first),
        repository.commit_completed(&second),
    );
    let results = [left.unwrap(), right.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, AppServerIdempotencyCommit::Stored(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, AppServerIdempotencyCommit::Replay(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn repository_rejects_invalid_scope_fingerprint_and_response() {
    let (_database, repository) = repository().await;

    let mut invalid_scope = scope();
    invalid_scope.client_id.clear();
    assert!(matches!(
        repository.lookup(&invalid_scope, FINGERPRINT_A).await,
        Err(DbError::Conflict(_))
    ));

    assert!(matches!(
        repository.lookup(&scope(), "sha256:ABC").await,
        Err(DbError::Conflict(_))
    ));

    assert!(matches!(
        repository
            .commit_completed(&receipt(scope(), FINGERPRINT_A, "[]"))
            .await,
        Err(DbError::Conflict(_))
    ));
    assert!(matches!(
        repository
            .commit_completed(&receipt(scope(), FINGERPRINT_A, "not-json"))
            .await,
        Err(DbError::Conflict(_))
    ));
}

#[tokio::test]
async fn stored_receipts_are_immutable_and_retained() {
    let (database, repository) = repository().await;
    repository
        .commit_completed(&receipt(scope(), FINGERPRINT_A, r#"{"ok":true}"#))
        .await
        .unwrap();

    let update = sqlx::query(
        "UPDATE app_server_idempotency_receipts SET response_json = '{\"ok\":false}'",
    )
    .execute(database.pool())
    .await;
    assert!(update.is_err());

    let delete = sqlx::query("DELETE FROM app_server_idempotency_receipts")
        .execute(database.pool())
        .await;
    assert!(delete.is_err());
}

#[tokio::test]
async fn receipts_survive_reopening_a_file_backed_database() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("app-server.sqlite");
    let database = nomifun_db::init_database(&path).await.unwrap();
    let repository = SqliteAppServerIdempotencyRepository::new(database.pool().clone());
    repository
        .commit_completed(&receipt(scope(), FINGERPRINT_A, r#"{"run_id":"public-1"}"#))
        .await
        .unwrap();
    database.close().await;

    let reopened = nomifun_db::init_database(&path).await.unwrap();
    let repository = SqliteAppServerIdempotencyRepository::new(reopened.pool().clone());
    assert!(matches!(
        repository.lookup(&scope(), FINGERPRINT_A).await.unwrap(),
        AppServerIdempotencyLookup::Replay(row)
            if row.response_json == r#"{"run_id":"public-1"}"#
    ));
    reopened.close().await;
}
