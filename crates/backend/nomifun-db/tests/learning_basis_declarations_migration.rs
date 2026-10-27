//! 迁移 071 已会声明锚表（`learning_basis_declarations`）的契约：一行一次
//! 声明、三张 JSON 清单（门口/命中块/概念）、声明随课删（逻辑引用 Cascade）；
//! concept_ids_json 是铸造出处留痕（KeepHistory——概念被移除后锚行原样保留，
//! 「声明可撤、概念不灭」）。v3 契约注册整库校验全绿。

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

const COURSE_ID: &str = "0190f5fe-7c00-7a00-8abc-012345678901";
const CONCEPT_A: &str = "0190f5fe-7c00-7a00-8abc-111111111111";
const CONCEPT_B: &str = "0190f5fe-7c00-7a00-8abc-222222222222";

async fn setup() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

async fn seed_course(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, '声明课', '', 'general', 1, 1000, 1000)",
    )
    .bind(COURSE_ID)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_concepts(pool: &SqlitePool) {
    for (concept_id, canonical) in [(CONCEPT_A, "加法"), (CONCEPT_B, "减法")] {
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
             VALUES (?, ?, '[]', '一句话定义', 1, 1)",
        )
        .bind(concept_id)
        .bind(canonical)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn declaration_row_accumulates_doors_blocks_and_concepts() {
    let pool = setup().await;
    seed_course(&pool).await;
    seed_concepts(&pool).await;

    let doors = serde_json::to_string(&["G1"]).unwrap();
    let blocks = serde_json::to_string(&["block-01", "block-02"]).unwrap();
    let concepts = serde_json::to_string(&[CONCEPT_A, CONCEPT_B]).unwrap();
    sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, ?, ?, ?, 2000, '')",
    )
    .bind(COURSE_ID)
    .bind(&doors)
    .bind(&blocks)
    .bind(&concepts)
    .execute(&pool)
    .await
    .unwrap();

    // 追加第二次声明（幂等增量的累计面：一行一次声明）。
    let doors2 = serde_json::to_string(&["G2"]).unwrap();
    let concepts2 = serde_json::to_string(&[CONCEPT_A]).unwrap();
    sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, ?, '[]', ?, 3000, '')",
    )
    .bind(COURSE_ID)
    .bind(&doors2)
    .bind(&concepts2)
    .execute(&pool)
    .await
    .unwrap();

    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT doors_json, concept_ids_json, declared_at FROM learning_basis_declarations \
         WHERE course_id = ? ORDER BY declared_at",
    )
    .bind(COURSE_ID)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "一行一次声明");
    assert_eq!(rows[0].0, r#"["G1"]"#);
    assert_eq!(rows[1].0, r#"["G2"]"#);

    // 概念库反查「来自哪次声明」（锚表即铸造出处）。
    let hits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_basis_declarations, \
         json_each(learning_basis_declarations.concept_ids_json) \
         WHERE json_each.value = ?",
    )
    .bind(CONCEPT_A)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hits, 2, "两次声明都铸出/复用了同一概念时都可反查");
}

#[tokio::test]
async fn declaration_rejects_malformed_lists_and_unknown_course() {
    let pool = setup().await;
    seed_course(&pool).await;
    seed_concepts(&pool).await;

    // doors_json 非数组 = 拒绝。
    let bad = sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, '{\"G1\": true}', '[]', '[]', 2000, '')",
    )
    .bind(COURSE_ID)
    .execute(&pool)
    .await;
    assert!(bad.is_err(), "doors_json 必须是 JSON 数组");

    // course_id 不在 learning_courses = 允许物理插入（无物理外键）但契约审计
    // 报孤儿；这里只验证 CHECK 形状拒绝非 UUIDv7。
    let bad_course = sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, '[]', '[]', '[]', 2000, '')",
    )
    .bind("not-a-uuid")
    .execute(&pool)
    .await;
    assert!(bad_course.is_err(), "course_id 必须是规范 UUIDv7");

    let _ = CONCEPT_B;
}

/// 声明随课删（Cascade 逻辑引用——v3 无物理外键，物理删除由应用侧
/// delete_course 承担，契约审计的修复通道兜底孤儿）；概念留池不随声明
/// 撤销/课程删除而回滚。
#[tokio::test]
async fn deleting_course_orphans_declarations_repairable_but_registry_survives() {
    let pool = setup().await;
    seed_course(&pool).await;
    seed_concepts(&pool).await;
    let doors = serde_json::to_string(&["G1"]).unwrap();
    let concepts = serde_json::to_string(&[CONCEPT_A]).unwrap();
    sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, ?, '[]', ?, 2000, '')",
    )
    .bind(COURSE_ID)
    .bind(&doors)
    .bind(&concepts)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM learning_courses WHERE course_id = ?")
        .bind(COURSE_ID)
        .execute(&pool)
        .await
        .unwrap();

    // 契约审计报孤儿（RequireParent——Cascade 引用不容缺失父行），修复通道
    // 删锚行（声明是本课事实，不随墓碑续命）。
    let repaired = nomifun_db::repair_logical_reference_orphans(&pool).await.unwrap();
    assert!(
        repaired.iter().any(|finding| {
            finding.child_table == "learning_basis_declarations"
                && finding.child_column == "course_id"
                && finding.count == 1
        }),
        "the orphaned declaration must be repaired: {repaired:?}"
    );
    let declarations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_basis_declarations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(declarations, 0, "声明锚随课删（本课事实）");
    let registry: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(registry, 2, "概念留池：登记表全局有效不随课删");
}

/// v3 契约注册：新表进 PRODUCT_TABLES + 逻辑引用（course_id Cascade）+
/// JSON 逻辑引用（concept_ids_json KeepHistory），整库契约校验全绿。
#[tokio::test]
async fn basis_declarations_are_registered_in_the_id_schema_contract() {
    let pool = setup().await;
    seed_course(&pool).await;
    seed_concepts(&pool).await;
    let doors = serde_json::to_string(&["G1"]).unwrap();
    let concepts = serde_json::to_string(&[CONCEPT_A]).unwrap();
    sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, ?, '[]', ?, 2000, '')",
    )
    .bind(COURSE_ID)
    .bind(&doors)
    .bind(&concepts)
    .execute(&pool)
    .await
    .unwrap();
    nomifun_db::validate_id_schema_contract(&pool)
        .await
        .expect("id schema contract (含 learning_basis_declarations)");

    // 出处留痕的 KeepHistory 面：概念行被移除后，契约审计容忍缺失父行
    // （AllowMissingHistoricalParent），锚行原样保留、不炸。
    sqlx::query("DELETE FROM learning_concept_registry WHERE concept_id = ?")
        .bind(CONCEPT_A)
        .execute(&pool)
        .await
        .unwrap();
    nomifun_db::validate_id_schema_contract(&pool)
        .await
        .expect("declaration provenance tolerates removed concepts (KeepHistory)");
    let row = sqlx::query(
        "SELECT concept_ids_json FROM learning_basis_declarations WHERE course_id = ?",
    )
    .bind(COURSE_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row.get::<String, _>(0),
        concepts,
        "锚行原样保留（声明可撤、概念不灭的留痕面）"
    );
}
