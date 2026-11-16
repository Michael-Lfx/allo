//! 037 的 `learning_course_jobs` 表在生成任务注册表（j03）接管后零消费者，
//! 已由学习域基线 §11 整体退役（x02）：全链迁移后表不存在，历史迁移文件仍
//! 保留在谱系里（发布段不可改）。

#[tokio::test]
async fn course_jobs_table_is_retired_by_the_learning_baseline() {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let tables: Vec<String> = nomifun_db::sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'learning_course_jobs'",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert!(tables.is_empty(), "learning_course_jobs must be retired: {tables:?}");
}
