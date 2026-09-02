use nomifun_db::{
    DbError, IAppServerWorkspaceRepository, SqliteAppServerWorkspaceRepository,
    validate_id_schema_contract,
};

async fn seed_user(pool: &sqlx::SqlitePool, user_id: &str) {
    sqlx::query(
        "INSERT INTO users (user_id, username, password_hash, jwt_secret, created_at, updated_at) \
         VALUES (?, ?, '', '', 0, 0)",
    )
    .bind(user_id)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed user");
}

#[tokio::test]
async fn migration_and_id_contract_include_app_server_workspaces() {
    let database = nomifun_db::init_database_memory().await.expect("database");
    validate_id_schema_contract(database.pool())
        .await
        .expect("workspace schema contract");
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'app_server_workspaces'",
    )
    .fetch_one(database.pool())
    .await
    .expect("table lookup");
    assert_eq!(exists, 1);
}

#[tokio::test]
async fn ensure_default_is_canonical_idempotent_and_owner_scoped() {
    let database = nomifun_db::init_database_memory().await.expect("database");
    let owner_a = nomifun_common::UserId::new().into_string();
    let owner_b = nomifun_common::UserId::new().into_string();
    seed_user(database.pool(), &owner_a).await;
    seed_user(database.pool(), &owner_b).await;
    let repository = SqliteAppServerWorkspaceRepository::new(database.pool().clone());
    let directory = tempfile::tempdir().expect("tempdir");
    let root = directory.path().to_str().expect("utf8 path");

    let first = repository
        .ensure_default(&owner_a, root)
        .await
        .expect("first default");
    let second = repository
        .ensure_default(&owner_a, root)
        .await
        .expect("same default");
    assert_eq!(first, second);
    assert_eq!(first.root_path, std::fs::canonicalize(root).unwrap().to_str().unwrap());
    assert_eq!(first.status, "active");
    nomifun_common::validate_uuidv7(&first.workspace_id).expect("canonical workspace id");

    assert!(repository
        .get(&owner_b, &first.workspace_id)
        .await
        .expect("cross-owner get")
        .is_none());
    let error = repository
        .register(&owner_b, &first.workspace_id, root)
        .await
        .expect_err("cross-owner registration must fail");
    assert!(matches!(error, DbError::Conflict(_)));
}

#[tokio::test]
async fn list_active_is_owner_scoped_and_excludes_revoked_rows() {
    let database = nomifun_db::init_database_memory().await.expect("database");
    let owner = nomifun_common::UserId::new().into_string();
    seed_user(database.pool(), &owner).await;
    let repository = SqliteAppServerWorkspaceRepository::new(database.pool().clone());
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path().to_str().expect("utf8 path");

    let first = repository.ensure_default(&owner, root_path).await.expect("first workspace");
    let second_root = tempfile::tempdir().expect("tempdir");
    let second = repository
        .ensure_default(&owner, second_root.path().to_str().expect("utf8 path"))
        .await
        .expect("second workspace");

    // Revoke the first workspace directly (as the schema allows).
    sqlx::query("UPDATE app_server_workspaces SET status = 'revoked' WHERE workspace_id = ?")
        .bind(&first.workspace_id)
        .execute(database.pool())
        .await
        .expect("revoke");

    let active = repository.list_active(&owner).await.expect("list active");
    let ids: Vec<&str> = active.iter().map(|row| row.workspace_id.as_str()).collect();
    assert!(ids.contains(&second.workspace_id.as_str()));
    assert!(!ids.contains(&first.workspace_id.as_str()), "revoked rows never list");
    assert!(active.iter().all(|row| row.status == "active"));
}

#[tokio::test]
async fn revoke_moves_owner_active_workspace_to_revoked_and_excludes_from_list_active() {
    let database = nomifun_db::init_database_memory().await.expect("database");
    let owner = nomifun_common::UserId::new().into_string();
    seed_user(database.pool(), &owner).await;
    let repository = SqliteAppServerWorkspaceRepository::new(database.pool().clone());
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = repository
        .ensure_default(&owner, root.path().to_str().expect("utf8 path"))
        .await
        .expect("register");

    // First revoke succeeds and the workspace leaves list_active.
    let revoked = repository
        .revoke(&owner, &workspace.workspace_id)
        .await
        .expect("revoke");
    assert!(revoked, "active owner workspace must revoke");
    let active = repository.list_active(&owner).await.expect("list active");
    assert!(
        active.iter().all(|row| row.workspace_id != workspace.workspace_id),
        "revoked workspace must not appear in list_active"
    );

    // Re-registering the same root re-activates the SAME workspace_id and
    // restores visibility (conversations keep referencing that id).
    let restored = repository
        .ensure_default(&owner, root.path().to_str().expect("utf8 path"))
        .await
        .expect("re-register");
    assert_eq!(restored.workspace_id, workspace.workspace_id, "same root must reuse the same workspace_id");
    assert_eq!(restored.status, "active");
    let active_after = repository.list_active(&owner).await.expect("list active after");
    assert!(
        active_after.iter().any(|row| row.workspace_id == workspace.workspace_id),
        "re-activated workspace must be visible again"
    );
}

#[tokio::test]
async fn revoke_is_owner_scoped_and_idempotent() {
    let database = nomifun_db::init_database_memory().await.expect("database");
    let owner = nomifun_common::UserId::new().into_string();
    let other = nomifun_common::UserId::new().into_string();
    seed_user(database.pool(), &owner).await;
    seed_user(database.pool(), &other).await;
    let repository = SqliteAppServerWorkspaceRepository::new(database.pool().clone());
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = repository
        .ensure_default(&owner, root.path().to_str().expect("utf8 path"))
        .await
        .expect("register");

    // A foreign owner cannot revoke it.
    assert!(
        !repository
            .revoke(&other, &workspace.workspace_id)
            .await
            .expect("foreign revoke must not error")
    );

    // Second revoke by the owner on the (now active) workspace succeeds once.
    assert!(repository
        .revoke(&owner, &workspace.workspace_id)
        .await
        .expect("first revoke"));
    // Repeating revoke on the already-revoked workspace is an idempotent no-op.
    assert!(!repository
        .revoke(&owner, &workspace.workspace_id)
        .await
        .expect("second revoke must be no-op"));

    // Missing workspace id is also an idempotent no-op.
    assert!(!repository.revoke(&owner, "0190f5fe-7c00-7a00-8000-000000000099").await.expect("missing revoke"));
}

#[tokio::test]
async fn register_validates_ids_and_existing_absolute_directory() {
    let database = nomifun_db::init_database_memory().await.expect("database");
    let owner = nomifun_common::UserId::new().into_string();
    seed_user(database.pool(), &owner).await;
    let repository = SqliteAppServerWorkspaceRepository::new(database.pool().clone());
    let workspace_id = uuid::Uuid::now_v7().to_string();

    for invalid_owner in ["", "0190f5fe-7c00-4a00-8000-000000000001"] {
        assert!(matches!(
            repository.register(invalid_owner, &workspace_id, "C:\\").await,
            Err(DbError::Conflict(_))
        ));
    }
    assert!(matches!(
        repository.register(&owner, "not-a-uuid", "C:\\").await,
        Err(DbError::Conflict(_))
    ));
    assert!(matches!(
        repository.register(&owner, &workspace_id, "relative").await,
        Err(DbError::Conflict(_))
    ));

    let missing = std::env::temp_dir().join(format!("missing-{}", uuid::Uuid::now_v7()));
    assert!(matches!(
        repository
            .register(&owner, &workspace_id, missing.to_str().unwrap())
            .await,
        Err(DbError::Conflict(_))
    ));
}
