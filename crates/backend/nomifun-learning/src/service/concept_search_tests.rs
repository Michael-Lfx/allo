//! 概念登记表只读检索的服务级测试（y03）：deprecated 排除 / 别名
//! `doc_version` 不匹配降级 / detail 全别名与 404 / 只读自证（调用前后
//! 登记表行与内容零变化）。

use super::*;

use crate::models::ConceptSearchMode;

const ACTIVE: &str = "0190f5fe-7c00-7a00-8abc-111111111111";
const DEPRECATED: &str = "0190f5fe-7c00-7a00-8abc-222222222222";
const FUTURE_DOC: &str = "0190f5fe-7c00-7a00-8abc-333333333333";
const MANY_ALIASES: &str = "0190f5fe-7c00-7a00-8abc-444444444444";

async fn search_service() -> LearningService {
    let database = nomifun_db::init_database_memory().await.unwrap();
    LearningService::new(database.pool().clone())
}

#[allow(clippy::too_many_arguments)]
async fn insert_concept(
    service: &LearningService,
    concept_id: &str,
    canonical: &str,
    definition: &str,
    aliases_json: &str,
    doc_version: i64,
    deprecated: i64,
) {
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, deprecated, doc_version, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 1, 1)",
    )
    .bind(concept_id)
    .bind(canonical)
    .bind(aliases_json)
    .bind(definition)
    .bind(deprecated)
    .bind(doc_version)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
}

/// 登记表全量快照（只读自证的比对面：行数 + 各行内容 + 时间戳）。
async fn registry_snapshot(
    service: &LearningService,
) -> Vec<(String, String, String, String, i64, i64, i64)> {
    sqlx::query_as(
        "SELECT concept_id, canonical, aliases_json, definition, deprecated, doc_version, updated_at \
         FROM learning_concept_registry ORDER BY concept_id",
    )
    .fetch_all(service.pool_for_tests())
    .await
    .unwrap()
}

async fn seed_registry(service: &LearningService) {
    insert_concept(
        service,
        ACTIVE,
        "极限",
        "无限逼近的值：函数值趋近于某个定值",
        r#"[{"name":"Limits","status":"active"},{"name":"旧叫法","status":"retired"}]"#,
        crate::learning_graph::alias_records::ALIAS_DOC_VERSION,
        0,
    )
    .await;
    insert_concept(
        service,
        DEPRECATED,
        "导数",
        "瞬时变化率",
        "[]",
        crate::learning_graph::alias_records::ALIAS_DOC_VERSION,
        1,
    )
    .await;
    insert_concept(
        service,
        FUTURE_DOC,
        "矩阵",
        "按行列排布的二维数组",
        r#"[{"name":"数表","status":"active"}]"#,
        crate::learning_graph::alias_records::ALIAS_DOC_VERSION + 98,
        0,
    )
    .await;
}

/// 四通道 + deprecated 排除 + 别名池只含活跃名 + 只读自证：search/detail
/// 调用前后登记表行数与内容逐字不变。
#[tokio::test]
async fn search_excludes_deprecated_and_leaves_the_registry_untouched() {
    let service = search_service().await;
    seed_registry(&service).await;
    let before = registry_snapshot(&service).await;

    // exact 通道 + 活跃别名随命中返回（退役名不占池）。
    let exact = service
        .search_concepts("极限", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert!(!exact.not_found);
    assert_eq!(exact.hits.len(), 1);
    assert_eq!(exact.hits[0].concept_id, ACTIVE);
    assert_eq!(exact.hits[0].matched_by, "exact");
    assert_eq!(exact.hits[0].aliases, vec!["Limits".to_owned()]);

    // alias 通道（大小写不敏感）。
    let alias = service
        .search_concepts("LIMITS", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert_eq!(alias.hits.len(), 1);
    assert_eq!(alias.hits[0].matched_by, "alias");

    // 退役别名不可检索。
    let retired = service
        .search_concepts("旧叫法", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert!(retired.not_found);
    assert!(retired.hits.is_empty());

    // fulltext：定义子串命中。
    let fulltext = service
        .search_concepts("趋近", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert_eq!(fulltext.hits[0].concept_id, ACTIVE);
    assert_eq!(fulltext.hits[0].matched_by, "fulltext");

    // deprecated 概念不检索、不 detail。
    let gone = service
        .search_concepts("导数", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert!(gone.not_found);
    let detail_gone = service
        .search_concepts("导数", ConceptSearchMode::Detail, Some(DEPRECATED))
        .await;
    assert!(matches!(detail_gone, Err(AppError::NotFound(_))));

    // detail：完整定义 + 活跃别名。
    let detail = service
        .search_concepts("极限", ConceptSearchMode::Detail, Some(ACTIVE))
        .await
        .unwrap();
    assert!(!detail.not_found);
    assert_eq!(detail.hits.len(), 1);
    assert_eq!(
        detail.hits[0].definition,
        "无限逼近的值：函数值趋近于某个定值"
    );
    assert_eq!(detail.hits[0].aliases, vec!["Limits".to_owned()]);
    assert_eq!(detail.hits[0].matched_by, "exact");

    let after = registry_snapshot(&service).await;
    assert_eq!(before, after, "search/detail 只读：登记表行与内容零变化");
}

/// 别名 `doc_version` 不匹配：该行别名池降级为空 + warn，canonical/定义
/// 照常可查（render_registry_excerpt 同口径）。
#[tokio::test]
async fn unsupported_alias_doc_version_degrades_to_an_empty_pool() {
    let service = search_service().await;
    seed_registry(&service).await;

    let alias = service
        .search_concepts("数表", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert!(alias.not_found, "未知代际的别名不参与检索");

    let canonical = service
        .search_concepts("矩阵", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert_eq!(canonical.hits.len(), 1);
    assert_eq!(canonical.hits[0].matched_by, "exact");
    assert!(canonical.hits[0].aliases.is_empty(), "别名池降级为空");

    let detail = service
        .search_concepts("矩阵", ConceptSearchMode::Detail, Some(FUTURE_DOC))
        .await
        .unwrap();
    assert!(detail.hits[0].aliases.is_empty());
}

/// detail 不受 top-5 限制，返回全部活跃别名；入参校验（缺 query / 缺
/// concept_id / 查无 id）分别是 400 与 404。
#[tokio::test]
async fn detail_returns_every_active_alias_and_validates_input() {
    let service = search_service().await;
    let aliases: Vec<String> = (1..=6).map(|i| format!(r#"{{"name":"别名{i}"}}"#)).collect();
    insert_concept(
        &service,
        MANY_ALIASES,
        "复合函数",
        &"是由内层与外层函数复合而成的函数。".repeat(20),
        &format!("[{}]", aliases.join(",")),
        crate::learning_graph::alias_records::ALIAS_DOC_VERSION,
        0,
    )
    .await;

    let detail = service
        .search_concepts("复合函数", ConceptSearchMode::Detail, Some(MANY_ALIASES))
        .await
        .unwrap();
    assert_eq!(detail.hits.len(), 1);
    assert_eq!(detail.hits[0].aliases.len(), 6, "detail 不受 top-5 限制");
    assert_eq!(
        detail.hits[0].definition.chars().count(),
        "是由内层与外层函数复合而成的函数。".repeat(20).chars().count(),
        "全定义不截断"
    );

    // 空 query（含纯空白）→ 400。
    for blank in ["", "   "] {
        let error = service
            .search_concepts(blank, ConceptSearchMode::Search, None)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::BadRequest(_)), "{error}");
    }
    // detail 缺 concept_id / 空白 concept_id → 400。
    for missing in [None, Some("  ")] {
        let error = service
            .search_concepts("复合函数", ConceptSearchMode::Detail, missing)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::BadRequest(_)), "{error}");
    }
    // detail 查无此 id → 404。
    let unknown = service
        .search_concepts(
            "复合函数",
            ConceptSearchMode::Detail,
            Some("0190f5fe-7c00-7a00-8abc-999999999999"),
        )
        .await
        .unwrap_err();
    assert!(matches!(unknown, AppError::NotFound(_)), "{unknown}");
}

/// search 截 top-5（DB 取行 → 纯排序去重整链），零命中返回 `not_found`
/// 不抛错。
#[tokio::test]
async fn search_truncates_to_top5_and_zero_hit_is_not_an_error() {
    let service = search_service().await;
    for index in 0..6 {
        insert_concept(
            &service,
            &format!("0190f5fe-7c00-7a00-8abc-00000000000{index}"),
            &format!("概念{index}"),
            "矩阵是线性代数的核心对象",
            "[]",
            crate::learning_graph::alias_records::ALIAS_DOC_VERSION,
            0,
        )
        .await;
    }
    let results = service
        .search_concepts("矩阵", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert!(!results.not_found);
    assert_eq!(results.hits.len(), 5, "六条全文命中截 top-5");

    let miss = service
        .search_concepts("量子纠缠", ConceptSearchMode::Search, None)
        .await
        .unwrap();
    assert!(miss.not_found);
    assert!(miss.hits.is_empty());
}
