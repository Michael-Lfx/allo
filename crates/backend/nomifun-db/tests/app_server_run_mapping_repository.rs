use std::sync::Arc;

use nomifun_db::{
    DbError, IAppServerRunMappingRepository, SqliteAppServerRunMappingRepository,
    init_database_memory_with_owner,
};

const OWNER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000001";
const OTHER_OWNER_ID: &str = "0190f5fe-7c00-7a00-8000-000000000002";
const EXECUTION_ID: &str = "0190f5fe-7c00-7a00-8000-000000000003";

async fn fixture() -> (
    nomifun_db::Database,
    Arc<dyn IAppServerRunMappingRepository>,
) {
    let database = init_database_memory_with_owner(
        nomifun_common::UserId::parse(OWNER_ID).expect("owner UUIDv7"),
    )
    .await
    .expect("database");
    sqlx::query(
        "INSERT INTO users (user_id, username, password_hash, created_at, updated_at) \
         VALUES (?, 'other-owner', 'fixture', 1, 1)",
    )
    .bind(OTHER_OWNER_ID)
    .execute(database.pool())
    .await
    .expect("other owner");
    sqlx::query(
        "INSERT INTO agent_executions (\
             execution_id, user_id, goal, status, plan_gate, adaptation_policy, \
             decision_policy, delegation_policy, max_parallel, initial_plan_input, \
             created_at, updated_at\
         ) VALUES (?, ?, 'fixture goal', 'planning', 'automatic', 'fixed', \
                   'automatic', 'disabled', 1, '{}', 1, 1)",
    )
    .bind(EXECUTION_ID)
    .bind(OWNER_ID)
    .execute(database.pool())
    .await
    .expect("execution");

    let repository: Arc<dyn IAppServerRunMappingRepository> = Arc::new(
        SqliteAppServerRunMappingRepository::new(database.pool().clone()),
    );
    (database, repository)
}

#[tokio::test]
async fn creates_stable_uuidv7_mapping_and_resolves_both_directions() {
    let (_database, repository) = fixture().await;

    let created = repository
        .create_mapping(EXECUTION_ID, OWNER_ID)
        .await
        .expect("mapping");
    nomifun_common::validate_uuidv7(&created.public_run_id).expect("public UUIDv7");
    assert_eq!(created.execution_id, EXECUTION_ID);
    assert_eq!(created.user_id, OWNER_ID);

    let repeated = repository
        .create_mapping(EXECUTION_ID, OWNER_ID)
        .await
        .expect("stable mapping");
    assert_eq!(repeated, created);
    assert_eq!(
        repository
            .get_by_public_id(&created.public_run_id, OWNER_ID)
            .await
            .unwrap(),
        Some(created.clone())
    );
    assert_eq!(
        repository
            .get_by_execution_id(EXECUTION_ID, OWNER_ID)
            .await
            .unwrap(),
        Some(created)
    );
}

#[tokio::test]
async fn owner_scope_hides_mappings_and_rejects_foreign_execution_creation() {
    let (_database, repository) = fixture().await;
    let created = repository
        .create_mapping(EXECUTION_ID, OWNER_ID)
        .await
        .unwrap();

    assert!(
        repository
            .get_by_public_id(&created.public_run_id, OTHER_OWNER_ID)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .get_by_execution_id(EXECUTION_ID, OTHER_OWNER_ID)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        repository
            .create_mapping(EXECUTION_ID, OTHER_OWNER_ID)
            .await,
        Err(DbError::NotFound(_))
    ));
}

#[tokio::test]
async fn repository_rejects_noncanonical_ids() {
    let (_database, repository) = fixture().await;

    assert!(matches!(
        repository.create_mapping("not-an-id", OWNER_ID).await,
        Err(DbError::Conflict(_))
    ));
    assert!(matches!(
        repository.get_by_public_id("not-an-id", OWNER_ID).await,
        Err(DbError::Conflict(_))
    ));
    assert!(matches!(
        repository.get_by_execution_id(EXECUTION_ID, "not-an-owner").await,
        Err(DbError::Conflict(_))
    ));
}
