//! 概念详情 / 全局总览读侧（c7）的服务级测试：多 partner 互指全返回 / 单向
//! 提及双向正确且互指不重复计 / 跨课程使用按课程分组 / 声明锚反查 / 未知 id
//! 404 / 总览计数与 deprecated 排除 / other_courses 带 course_id 且排除本课。

use super::*;

/// 合法 concept_id（uuid v7 形状，过 registry 的 CHECK）。
fn concept_uuid() -> String {
    LearningLessonId::new().into_string()
}

/// 种子一个概念（doc_version 恒当前代际；aliases_json 由调用方给）。
async fn seed_concept(
    service: &LearningService,
    concept_id: &str,
    canonical: &str,
    definition: &str,
    aliases_json: &str,
    deprecated: bool,
) {
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, deprecated, doc_version, \
          created_at, updated_at, source) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'seed')",
    )
    .bind(concept_id)
    .bind(canonical)
    .bind(aliases_json)
    .bind(definition)
    .bind(if deprecated { 1 } else { 0 })
    .bind(crate::learning_graph::alias_records::ALIAS_DOC_VERSION)
    .bind(now_ms())
    .bind(now_ms())
    .execute(service.pool_for_tests())
    .await
    .unwrap();
}

/// 种子一门课程 + 模块 + 若干带概念引用的节点（teaches / assumes）。
async fn seed_course(
    service: &LearningService,
    title: &str,
    refs: &[(&str, &str, &str)],
) -> String {
    let course_id = LearningCourseId::new().into_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, ?, '', 'general', 1, 'learning_graph', '', '', '{}', ?, ?)",
    )
    .bind(&course_id)
    .bind(title)
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let module_id = LearningModuleId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '模块', '', 0)",
    )
    .bind(&module_id)
    .bind(&course_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    for (position, (lesson_title, concept_id, role)) in refs.iter().enumerate() {
        let lesson_id = LearningLessonId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes) \
             VALUES (?, ?, ?, '', 'p', ?, 10)",
        )
        .bind(&lesson_id)
        .bind(&module_id)
        .bind(lesson_title)
        .bind(position as i64)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_lesson_concepts (lesson_id, concept_id, role, tier) \
             VALUES (?, ?, ?, 'know')",
        )
        .bind(&lesson_id)
        .bind(concept_id)
        .bind(role)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }
    course_id
}

/// 种子一条声明锚（与 basis_tests 的同形助手；跨测试模块不互引）。
async fn seed_declaration_anchor(
    service: &LearningService,
    course: &LearningCourseId,
    doors: &[&str],
    concept_ids: &[&str],
) {
    let doors_json = serde_json::to_string(doors).unwrap();
    let concept_ids_json = serde_json::to_string(concept_ids).unwrap();
    sqlx::query(
        "INSERT INTO learning_basis_declarations \
         (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
         VALUES (?, ?, '[]', ?, ?, '')",
    )
    .bind(course.as_str())
    .bind(&doors_json)
    .bind(&concept_ids_json)
    .bind(now_ms())
    .execute(service.pool_for_tests())
    .await
    .unwrap();
}

async fn seeded_service() -> (LearningService, UserId) {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool()).await.unwrap();
    let owner = UserId::parse(&owner_id).unwrap();
    (LearningService::new(database.pool().clone()), owner)
}

/// 详情：互指（A↔B）+ 单向提及（A→C）+ 被提及 + 自指 + 跨课程使用按课程
/// 分组 + 声明锚反查。
#[tokio::test]
#[serial_test::serial]
async fn concept_detail_relations_usages_and_declared_in() {
    let (service, owner) = seeded_service().await;
    let a = concept_uuid();
    let b = concept_uuid();
    let c = concept_uuid();
    let d = concept_uuid();
    seed_concept(&service, &a, "概念甲", "概念乙与概念丙的复合判断", "[]", false).await;
    seed_concept(&service, &b, "概念乙", "回指概念甲的判定", "[]", false).await;
    seed_concept(&service, &c, "概念丙", "独立判定，不提别家", "[]", false).await;
    seed_concept(&service, &d, "概念丁", "概念丁自指自身", "[]", false).await;

    let course_x = seed_course(
        &service,
        "课程甲",
        &[("节点一", &a, "teaches"), ("节点二", &a, "assumes"), ("节点三", &b, "teaches")],
    )
    .await;
    let course_y = seed_course(&service, "课程乙", &[("节点一", &a, "teaches")]).await;
    seed_declaration_anchor(
        &service,
        &LearningCourseId::parse(&course_x).unwrap(),
        &["G1"],
        &[&a],
    )
    .await;

    let view = service.concept_detail(&owner, &a).await.unwrap();
    // 互指：A 定义含乙、B 定义含甲 → A 的互指恰 B；单向提及只剩 A→C（互指
    // partner 不重复计入 mentions）。
    assert_eq!(
        view.mutual.iter().map(|link| link.canonical.as_str()).collect::<Vec<_>>(),
        vec!["概念乙"]
    );
    assert_eq!(
        view.mentions.iter().map(|link| link.canonical.as_str()).collect::<Vec<_>>(),
        vec!["概念丙"]
    );
    // 使用分组：A 在两门课；课程甲 2 条引用、课程乙 1 条。
    assert_eq!(view.usages.len(), 2);
    let group_x = view
        .usages
        .iter()
        .find(|group| group.course_id == course_x)
        .unwrap();
    assert_eq!(group_x.course_title, "课程甲");
    assert_eq!(group_x.refs.len(), 2);
    assert!(view.usages.iter().any(|group| group.course_id == course_y));
    // 声明反查：恰课程甲。
    assert_eq!(view.declared_in.len(), 1);
    assert_eq!(view.declared_in[0].course_id, course_x);
    assert_eq!(view.declared_in[0].course_title, "课程甲");

    // B 视角：与甲互指——互指 partner 不重复计入单向 mentions/mentioned_by
    // （展示契约：互指优先，一条关系只出现一处）。
    let b_view = service.concept_detail(&owner, &b).await.unwrap();
    assert_eq!(
        b_view.mutual.iter().map(|link| link.canonical.as_str()).collect::<Vec<_>>(),
        vec!["概念甲"]
    );
    assert!(b_view.mentioned_by.is_empty());
    assert!(b_view.mentions.is_empty());

    // C 视角：单向被提及（甲）、无互指。
    let c_view = service.concept_detail(&owner, &c).await.unwrap();
    assert_eq!(
        c_view.mentioned_by.iter().map(|link| link.canonical.as_str()).collect::<Vec<_>>(),
        vec!["概念甲"]
    );
    assert!(c_view.mutual.is_empty());

    // 自指。
    let d_view = service.concept_detail(&owner, &d).await.unwrap();
    assert!(d_view.self_reference);
}

/// 详情：未知 id = 404；总览：deprecated 排除 + 计数（别名/课程/引用/声明/
/// 互指）齐面。
#[tokio::test]
#[serial_test::serial]
async fn concept_detail_unknown_id_404_and_overview_counts() {
    let (service, owner) = seeded_service().await;
    // 未知 id = NotFound（不白屏的 API 前提）。
    let result = service.concept_detail(&owner, &concept_uuid()).await;
    assert!(matches!(result, Err(AppError::NotFound(_))));

    let a = concept_uuid();
    let b = concept_uuid();
    let e = concept_uuid();
    seed_concept(
        &service,
        &a,
        "概念甲",
        "概念乙的逆命题",
        r#"[{"name":"甲别称","definition":"甲的别名定义","status":"active"}]"#,
        false,
    )
    .await;
    seed_concept(&service, &b, "概念乙", "概念甲的对偶面", "[]", false).await;
    seed_concept(&service, &e, "废弃概念", "已退役不应出现在总览", "[]", true).await;

    let course_x = seed_course(
        &service,
        "课程甲",
        &[("节点一", &a, "teaches"), ("节点二", &a, "teaches")],
    )
    .await;
    seed_course(&service, "课程乙", &[("节点一", &a, "teaches"), ("节点二", &b, "assumes")]).await;
    seed_declaration_anchor(
        &service,
        &LearningCourseId::parse(&course_x).unwrap(),
        &["G1"],
        &[&a],
    )
    .await;

    let rows = service.concepts_overview().await.unwrap();
    assert!(
        rows.iter().all(|row| row.canonical != "废弃概念"),
        "deprecated 必须被排除"
    );
    let a_row = rows.iter().find(|row| row.canonical == "概念甲").unwrap();
    assert_eq!(a_row.alias_count, 1);
    assert_eq!(a_row.course_count, 2);
    assert_eq!(a_row.ref_count, 3);
    assert_eq!(a_row.declared_course_count, 1);
    // A 定义含乙、B 定义含甲 → 互指一对。
    assert_eq!(a_row.mutual_count, 1);
    assert!(!a_row.self_reference);
    let b_row = rows.iter().find(|row| row.canonical == "概念乙").unwrap();
    assert_eq!(b_row.course_count, 1);
    assert_eq!(b_row.ref_count, 1);
    assert_eq!(b_row.declared_course_count, 0);
    assert_eq!(b_row.mutual_count, 1);
}

/// graph_concepts 的 other_courses 升级形状：带 course_id 且排除本课。
#[tokio::test]
#[serial_test::serial]
async fn graph_concepts_other_courses_carries_ids_and_excludes_self() {
    let (service, owner) = seeded_service().await;
    let a = concept_uuid();
    seed_concept(&service, &a, "概念甲", "跨课程共享的判定", "[]", false).await;
    let course_x = seed_course(&service, "课程甲", &[("节点一", &a, "teaches")]).await;
    let course_y = seed_course(&service, "课程乙", &[("节点一", &a, "teaches")]).await;

    let rows_x = service
        .graph_concepts(&LearningCourseId::parse(&course_x).unwrap(), &owner)
        .await
        .unwrap();
    let row = rows_x.iter().find(|row| row.concept_id == a).unwrap();
    assert_eq!(row.other_courses.len(), 1);
    assert_eq!(row.other_courses[0].course_id, course_y);
    assert_eq!(row.other_courses[0].title, "课程乙");

    let rows_y = service
        .graph_concepts(&LearningCourseId::parse(&course_y).unwrap(), &owner)
        .await
        .unwrap();
    let row = rows_y.iter().find(|row| row.concept_id == a).unwrap();
    assert_eq!(row.other_courses.len(), 1);
    assert_eq!(row.other_courses[0].course_id, course_x);
}
