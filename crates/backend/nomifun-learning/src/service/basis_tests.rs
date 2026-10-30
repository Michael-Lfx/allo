//! 已会声明（Declared Known，m2 / ADR-0032 移植）的服务级测试：单测清单按
//! 票 m2 验收面——doors 越界打回 / 幂等增量 noop / AND 块命中 / 命名门复用
//! （机械复用 + j04 复用裁决通道）/ 已知集并网（learnhub 506d107 同名测试
//! 移植）/ Broken 如实降级 / 撤销语义 / off 臂 409。

use super::*;

/// 一针概念产物（两概念四字段）。
const BASIS_DECLARE_CAN: &str = r#"{"concepts":[
    {"name":"门口判读","slug":"door-reading","definition":"按门口判据读出自身具备状态","source":"声明自述"},
    {"name":"第二块判读","slug":"second-block","definition":"第二块内容的判读规则","source":"声明自述"}]}"#;

/// 按序出稿的 completer（声明测试专用替身）：逐调用消费脚本，末项无限重复；
/// 记录调用数供零 LLM 断言。与 tests.rs 的 SequencedCompleter 同构（跨测试
/// 模块不互引，避免把兄弟模块的私有面拉进公共视野）。
struct SequencedCompleter {
    replies: std::sync::Mutex<Vec<String>>,
    calls: std::sync::atomic::AtomicUsize,
}

impl SequencedCompleter {
    fn new(replies: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            replies: std::sync::Mutex::new(replies),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn call_count(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn record(&self, _user: &str) -> Result<String, AppError> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut replies = self.replies.lock().unwrap();
        match replies.len() {
            0 => Err(AppError::Internal("empty script".into())),
            1 => Ok(replies[0].clone()),
            _ => Ok(replies.remove(0)),
        }
    }
}

#[async_trait::async_trait]
impl LearningCompleter for SequencedCompleter {
    async fn complete(
        &self,
        _model_override: Option<(&str, &str)>,
        _system: &str,
        user: &str,
        _max_tokens: u32,
    ) -> Result<String, AppError> {
        self.record(user)
    }

    async fn complete_json_with_thinking(
        &self,
        _model_override: Option<(&str, &str)>,
        _system: &str,
        user: &str,
        _max_tokens: u32,
        _temperature: Option<f32>,
    ) -> Result<String, AppError> {
        self.record(user)
    }
}

fn basis_request(doors: &[&str]) -> crate::models::DeclareBasisRequest {
    crate::models::DeclareBasisRequest {
        doors: doors.iter().map(|door| door.to_string()).collect(),
        provider_id: None,
        model: None,
    }
}

/// 种子：一门带地形门口（G1/G2）与 door_refs 标注块的图课程。
/// 块乙 door_refs = [G1, G2]（AND 口径的判定面）。
async fn seed_basis_course(
    completer: Arc<dyn LearningCompleter>,
) -> (LearningService, UserId, LearningCourseId) {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id =
        nomifun_db::installation_owner_id(database.pool()).await.unwrap();
    let owner = UserId::parse(&owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    *service.course_completer.write().unwrap() = Some(completer);
    let course_id = LearningCourseId::new().into_string();
    let now = now_ms();
    let envelope = serde_json::json!({
        "version": 1,
        "generated_at": now,
        "structure": {
            "goal": "已会声明冒烟目标",
            "goal_type": "capability",
            "proficiency_statement": "能独立复述声明判据",
            "domain": {
                "core_definition": "声明冒烟",
                "includes": ["判读"],
                "excludes": [],
                "prerequisite_doors": ["G1", "G2"],
                "audience": "测试",
                "scenarios": ["声明"],
                "granularity": "主题级",
            },
            "spines": [
                { "name": "主干一", "definition": "d", "key_points": ["k"], "instance": "i" }
            ],
            "blocks": [
                { "name": "块甲", "slug": "block-a", "spine": "主干一", "stage": "入门", "note": "第一块", "door_refs": ["G1"] },
                { "name": "块乙", "slug": "block-b", "spine": "主干一", "stage": "入门", "note": "第二块", "door_refs": ["G1", "G2"] }
            ],
            "edges": []
        },
        "terrain": {
            "learner_basis_assumed": "已具备基础判读",
            "disclaimer": "种子地形",
            "doors": [
                { "id": "G1", "criterion": "判据一", "kind": "hard" },
                { "id": "G2", "criterion": "判据二", "kind": "self_check" }
            ]
        },
    });
    sqlx::query(
        "INSERT INTO learning_courses \
         (course_id, title, description, domain, version, course_kind, learning_goal, \
          learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, '声明课程', '', 'general', 1, 'learning_graph', '已会声明冒烟目标', '', ?, ?, ?)",
    )
    .bind(&course_id)
    .bind(serde_json::to_string(&envelope).unwrap())
    .bind(now)
    .bind(now)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    (service, owner, LearningCourseId::parse(&course_id).unwrap())
}

async fn basis_anchor_count(service: &LearningService, course: &LearningCourseId) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM learning_basis_declarations WHERE course_id = ?")
        .bind(course.as_str())
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap()
}

async fn basis_registry_count(service: &LearningService) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM learning_concept_registry")
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap()
}

async fn basis_node_and_batch_counts(
    service: &LearningService,
    course: &LearningCourseId,
) -> (i64, i64) {
    let lessons: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(course.as_str())
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    let batches: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM learning_growth_batches WHERE course_id = ?",
    )
    .bind(course.as_str())
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    (lessons, batches)
}

async fn seed_module_with_two_lessons(
    service: &LearningService,
    course: &LearningCourseId,
    concept_id: &str,
) -> (String, String) {
    let module_id = LearningModuleId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(&module_id)
    .bind(course.as_str())
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let n1 = LearningLessonId::new().into_string();
    let n2 = LearningLessonId::new().into_string();
    for (position, (lesson_id, title)) in [(&n1, "认识入口"), (&n2, "使用多态")]
        .into_iter()
        .enumerate()
    {
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes) \
             VALUES (?, ?, ?, '', 'p', ?, 10)",
        )
        .bind(lesson_id)
        .bind(&module_id)
        .bind(title)
        .bind(position as i64)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id, role, tier) \
         VALUES (?, ?, 'teaches', 'know')",
    )
    .bind(&n1)
    .bind(concept_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id, role, tier) \
         VALUES (?, ?, 'assumes', 'know')",
    )
    .bind(&n2)
    .bind(concept_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    (n1, n2)
}

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

/// 核心语义（ADR-0032）：声明只铸概念、**不落任何学习节点**，锚行记门口 +
/// 块 + 概念（锚表即铸造出处）；概念表读侧带来源徽标与 declared 旗标。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_mints_concepts_and_never_creates_learning_nodes() {
    let completer = SequencedCompleter::new(vec![BASIS_DECLARE_CAN.to_owned()]);
    let (service, user, course) = seed_basis_course(completer.clone()).await;
    let out = service
        .declare_basis(&user, &course, basis_request(&["G1", "G2"]))
        .await
        .unwrap();

    assert_eq!(out.doors, vec!["G1".to_owned(), "G2".to_owned()]);
    assert_eq!(out.blocks, vec!["块甲".to_owned(), "块乙".to_owned()]);
    assert_eq!(out.concepts.len(), 2);
    assert!(!out.noop);
    assert_eq!((out.minted, out.reused), (2, 0));

    // 概念入册（铸名纪律路径），来源标记恒 'basis-declaration'。
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT canonical, source FROM learning_concept_registry ORDER BY canonical",
    )
    .fetch_all(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|(_, source)| source == "basis-declaration"));

    // **零图写入**：零 learning_lessons / growth_batches（计数守恒）。
    assert_eq!(basis_node_and_batch_counts(&service, &course).await, (0, 0));
    assert_eq!(basis_anchor_count(&service, &course).await, 1);

    // 锚行内容：门口 + 块 + 概念 id（与返回面一致——锚表即出处）。
    let (doors_json, blocks_json, concept_ids_json): (String, String, String) =
        sqlx::query_as(
            "SELECT doors_json, blocks_json, concept_ids_json \
             FROM learning_basis_declarations WHERE course_id = ?",
        )
        .bind(course.as_str())
        .fetch_one(service.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(doors_json, r#"["G1","G2"]"#);
    assert_eq!(blocks_json, r#"["block-a","block-b"]"#);
    let anchor_ids: Vec<String> = serde_json::from_str(&concept_ids_json).unwrap();
    assert_eq!(
        anchor_ids,
        out.concepts
            .iter()
            .map(|declared| declared.concept_id.clone())
            .collect::<Vec<_>>()
    );

    // 概念表读侧：declared 旗标 + 来源徽标（无节点引用也入表）。
    let view = service.graph_concepts(&course, &user).await.unwrap();
    assert_eq!(view.len(), 2, "声明概念无节点引用也入概念表");
    assert!(
        view.iter()
            .all(|row| row.declared && row.source.as_deref() == Some("basis-declaration"))
    );

    // 恰一次 LLM 调用（一次结构化概念清单调用；机械门零调用）。
    assert_eq!(completer.call_count(), 1);
}

/// 幂等：同门口重声明 = noop 零 LLM 零写；扩门口只处理新增差集。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_rerun_is_idempotent_noop() {
    let completer = SequencedCompleter::new(vec![BASIS_DECLARE_CAN.to_owned()]);
    let (service, user, course) = seed_basis_course(completer.clone()).await;
    service
        .declare_basis(&user, &course, basis_request(&["G1"]))
        .await
        .unwrap();
    assert_eq!(completer.call_count(), 1);

    // 重声明同一门口：noop、零 LLM、零新锚行。
    let out = service
        .declare_basis(&user, &course, basis_request(&["G1"]))
        .await
        .unwrap();
    assert!(out.noop);
    assert!(out.concepts.is_empty() && out.doors.is_empty());
    assert_eq!(completer.call_count(), 1, "noop 零 LLM");
    assert_eq!(basis_anchor_count(&service, &course).await, 1, "档案零重复");

    // 扩门口（G2）：只处理差集——锚行追加、LLM 恰一次。同一罐头再铸同名
    // 概念 → 命名门精确占用机械复用（登记表行数守恒——幂等在概念层同样成立）。
    let registry_before = basis_registry_count(&service).await;
    let out = service
        .declare_basis(&user, &course, basis_request(&["G1", "G2"]))
        .await
        .unwrap();
    assert!(!out.noop);
    assert_eq!(out.doors, vec!["G2".to_owned()]);
    assert_eq!(completer.call_count(), 2);
    assert_eq!(basis_anchor_count(&service, &course).await, 2);
    assert_eq!(basis_registry_count(&service).await, registry_before, "同名复用不重铸");
    assert_eq!((out.minted, out.reused), (0, 2));
}

/// 越出课程门口的勾选 = 输入非法，零 LLM；空勾选同。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_unknown_door_and_empty_selection_fail_without_llm() {
    let completer = SequencedCompleter::new(vec![]);
    let (service, user, course) = seed_basis_course(completer.clone()).await;
    let error = service
        .declare_basis(&user, &course, basis_request(&["G9"]))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("不在课程门口清单"), "{error}");
    let error = service
        .declare_basis(&user, &course, basis_request(&[]))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("未勾选"), "{error}");
    assert_eq!(completer.call_count(), 0, "数据性失败零 LLM");
}

/// AND 块命中：door_refs 部分落勾选集的块不算命中；勾选未命中任何块的门口
/// 组合 = 输入非法（判据在 LLM 之前，零调用）。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_and_block_semantics() {
    let completer = SequencedCompleter::new(vec![BASIS_DECLARE_CAN.to_owned()]);
    let (service, user, course) = seed_basis_course(completer).await;
    // 只勾 G1：块乙（door_refs = [G1, G2]）AND 不满足，块甲命中。
    let out = service
        .declare_basis(&user, &course, basis_request(&["G1"]))
        .await
        .unwrap();
    assert_eq!(out.blocks, vec!["块甲".to_owned()], "部分 door_refs 命中不算");

    // 只勾 G2：块乙缺 G1 不命中、块甲零命中 → 无目标块，如实拒绝。
    let completer2 = SequencedCompleter::new(vec![]);
    let (service2, user2, course2) = seed_basis_course(completer2.clone()).await;
    let error = service2
        .declare_basis(&user2, &course2, basis_request(&["G2"]))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("没有命中任何带门位标签"),
        "AND 口径无目标块：{error}"
    );
    // 零命中拒绝带出路（e2，learnhub「请改用逐个跳过」域中立化）：文案必须
    // 指向学习图逐节点跳过的既有 UI 面——死路拒绝不留出路 = 旅程 BLOCKER。
    assert!(
        error.to_string().contains("请改用逐节点跳过"),
        "零命中拒绝必须带出路：{error}"
    );
    assert_eq!(completer2.call_count(), 0, "命中判据在 LLM 之前，零调用");
}

/// 命名门集成：撞登记表现有活跃名 → 机械复用入锚，不重铸（登记表行数
/// 守恒；提名名经登记单点幂等跳过——本就是宿主活跃名）。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_reuses_existing_concept_via_naming_gate() {
    let completer = SequencedCompleter::new(vec![r#"{"concepts":[
        {"name":"加法","slug":"addition","definition":"合并两数得总数的运算","source":"声明自述"}
    ]}"#
    .to_owned()]);
    let (service, user, course) = seed_basis_course(completer).await;
    // 预置登记表概念「加法」（宿主）。
    let host_id = LearningConceptId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES (?, '加法', '[]', '求和的运算', 1, 1)",
    )
    .bind(&host_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();

    let out = service
        .declare_basis(&user, &course, basis_request(&["G1"]))
        .await
        .unwrap();

    // 复用入锚不重铸：登记表行数守恒、锚行引用宿主 id。
    assert_eq!((out.minted, out.reused), (0, 1));
    assert_eq!(basis_registry_count(&service).await, 1);
    assert_eq!(out.concepts.len(), 1);
    assert!(!out.concepts[0].minted);
    let concept_ids_json: String = sqlx::query_scalar(
        "SELECT concept_ids_json FROM learning_basis_declarations WHERE course_id = ?",
    )
    .bind(course.as_str())
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert_eq!(concept_ids_json, format!("[\"{host_id}\"]"));
}

/// 命名门 j04 复用裁决通道：名字近形歧义（非精确占用、非直定）→ 送裁决，
/// 裁 reuse = 提名名经登记单点落为宿主活跃别名，不重铸。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_ambiguous_nomination_goes_through_reuse_ruling() {
    let host_id = LearningConceptId::new().into_string();
    let ruling = format!(
        r#"{{"rulings":[{{"name":"加法运算","verdict":"reuse","concept_id":"{host_id}"}}]}}"#
    );
    let completer = SequencedCompleter::new(vec![
        r#"{"concepts":[
            {"name":"加法运算","slug":"addition-op","definition":"把两个数合并成一个总数的操作","source":"声明自述"}
        ]}"#
        .to_owned(),
        ruling,
    ]);
    let (service, user, course) = seed_basis_course(completer.clone()).await;
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES (?, '加法', '[]', '求和的运算', 1, 1)",
    )
    .bind(&host_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();

    let out = service
        .declare_basis(&user, &course, basis_request(&["G1"]))
        .await
        .unwrap();

    // 复用裁决生效：不重铸；提名名落为宿主活跃别名（登记单点）。
    assert_eq!((out.minted, out.reused), (0, 1));
    assert_eq!(basis_registry_count(&service).await, 1);
    let aliases_json: String = sqlx::query_scalar(
        "SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?",
    )
    .bind(&host_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(aliases_json.contains("加法运算"), "提名名落池：{aliases_json}");
    // 两次调用：概念清单 + 复用裁决（j04 通道）。
    assert_eq!(completer.call_count(), 2);
}

/// learnhub 506d107 同名测试移植：已会声明的概念（无学习节点）必须让下游
/// 节点判为就绪——「勾选即已会」在就绪投影上同源生效；撤销后复原。
#[tokio::test]
#[serial_test::serial]
async fn declared_basis_concept_makes_downstream_node_ready_without_any_teaching_node() {
    let completer = SequencedCompleter::new(vec![]);
    let (service, user, course) = seed_basis_course(completer).await;
    // 概念「多态」：全库没有任何节点教它（learnhub 原测试同构：n1 teaches
    // 的是另一个概念，多态只被 n2 assumes）。
    let taught_id = LearningConceptId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES (?, '入口概念', '[]', '入口层概念', 1, 1)",
    )
    .bind(&taught_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let concept_id = LearningConceptId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_concept_registry \
         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
         VALUES (?, '多态', '[]', '同一接口在不同类型上的不同行为', 1, 1)",
    )
    .bind(&concept_id)
    .execute(service.pool_for_tests())
    .await
    .unwrap();
    let (n1, n2) = seed_module_with_two_lessons(&service, &course, &concept_id).await;
    // 修正 n1 的 teaches：指「入口概念」而非多态（多态必须无人教）。
    sqlx::query("UPDATE learning_lesson_concepts SET concept_id = ? WHERE lesson_id = ?")
        .bind(&taught_id)
        .bind(&n1)
        .execute(service.pool_for_tests())
        .await
        .unwrap();
    let (n1, n2) = (
        LearningLessonId::parse(&n1).unwrap(),
        LearningLessonId::parse(&n2).unwrap(),
    );

    // 声明前：多态无人教 → n2 未就绪。
    let ready = service.ready_set_for(&course, None).await.unwrap();
    assert_eq!(ready, vec![n1.clone()], "无人教 → n2 未就绪");

    // 声明「多态」为已会（零节点，仅锚行）：就绪投影据此放行 n2。
    seed_declaration_anchor(&service, &course, &["G1"], &[&concept_id]).await;

    let ready = service.ready_set_for(&course, None).await.unwrap();
    assert_eq!(
        ready,
        vec![n1.clone(), n2.clone()],
        "声明概念进已知集 → 下游节点就绪（无任何教学节点参与）"
    );

    // 撤销：锚行删除 → 投影复原（声明可撤）。
    let revoked = service.revoke_basis(&user, &course).await.unwrap();
    assert_eq!(revoked, 1);
    let ready = service.ready_set_for(&course, None).await.unwrap();
    assert_eq!(ready, vec![n1.clone()], "撤销后 n2 失去已知供给");
    // 概念留池：登记表全局有效。
    assert_eq!(basis_registry_count(&service).await, 2, "概念不灭");
}

/// Broken 如实降级：锚行里的概念已不在登记表（被删）→ 读数披露 unresolved、
/// 已知投影与教练账面跳过它，不炸。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_broken_concept_degrades_honestly() {
    let completer = SequencedCompleter::new(vec![]);
    let (service, _user, course) = seed_basis_course(completer).await;
    let ghost = LearningConceptId::new().into_string();
    seed_declaration_anchor(&service, &course, &["G1"], &[&ghost]).await;

    // 读数披露（不炸）。
    let view = service.basis_view(&course).await.unwrap();
    assert_eq!(view.doors, vec!["G1".to_owned()]);
    assert!(view.concepts.is_empty());
    assert_eq!(view.unresolved_concept_ids, vec![ghost.clone()]);

    // 就绪投影照常工作（幽灵 id 被跳过）。
    let ready = service.ready_set_for(&course, None).await.unwrap();
    assert!(ready.is_empty());
    // 教练已教账面不收幽灵概念（声明并网查询 join 登记表，自然降级）。
    let summary = service.render_taught_summary().await.unwrap();
    assert!(!summary.contains(&ghost), "幽灵概念不进教练账面：{summary}");
}

/// 撤销语义（grill 定案「声明可撤、概念不灭」）：删锚行、收缩本课已知投影；
/// 登记表概念与来源标记保留不抹；空撤销幂等返回 0。
#[tokio::test]
#[serial_test::serial]
async fn basis_revoke_deletes_anchor_keeps_concepts_and_source_mark() {
    let completer = SequencedCompleter::new(vec![BASIS_DECLARE_CAN.to_owned()]);
    let (service, user, course) = seed_basis_course(completer).await;
    service
        .declare_basis(&user, &course, basis_request(&["G1", "G2"]))
        .await
        .unwrap();
    let registry_before = basis_registry_count(&service).await;

    let view = service.basis_view(&course).await.unwrap();
    assert_eq!(view.doors.len(), 2);
    assert_eq!(view.concepts.len(), 2);
    assert_eq!(view.declarations, 1);
    assert!(view.enabled);

    let revoked = service.revoke_basis(&user, &course).await.unwrap();
    assert_eq!(revoked, 1);
    assert_eq!(basis_anchor_count(&service, &course).await, 0);
    assert_eq!(
        basis_registry_count(&service).await,
        registry_before,
        "概念留池"
    );
    let sources: Vec<String> =
        sqlx::query("SELECT COALESCE(source, '') FROM learning_concept_registry")
            .fetch_all(service.pool_for_tests())
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.try_get::<String, _>(0).unwrap())
            .collect();
    assert!(
        sources.iter().all(|source| source == "basis-declaration"),
        "来源标记保留不抹：{sources:?}"
    );
    let view = service.basis_view(&course).await.unwrap();
    assert!(view.doors.is_empty() && view.concepts.is_empty());

    // 空撤销幂等。
    let revoked = service.revoke_basis(&user, &course).await.unwrap();
    assert_eq!(revoked, 0);
}

/// off 臂：`NOMI_LEARNING_BASIS_DECLARATION=off` → 端点语义 409 如实拒绝
/// （声明与撤销都拒）；读数面照常可读、如实标注 enabled = false。
#[tokio::test]
#[serial_test::serial]
async fn basis_declaration_switch_off_rejects_loudly() {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => unsafe {
                    std::env::set_var(
                        crate::learning_graph::basis_declaration::BASIS_DECLARATION_SWITCH_ENV,
                        value,
                    )
                },
                None => unsafe {
                    std::env::remove_var(
                        crate::learning_graph::basis_declaration::BASIS_DECLARATION_SWITCH_ENV,
                    )
                },
            }
        }
    }
    let previous = std::env::var(
        crate::learning_graph::basis_declaration::BASIS_DECLARATION_SWITCH_ENV,
    )
    .ok();
    unsafe {
        std::env::set_var(
            crate::learning_graph::basis_declaration::BASIS_DECLARATION_SWITCH_ENV,
            "off",
        );
    }
    let _restore = Restore(previous);

    let completer = SequencedCompleter::new(vec![BASIS_DECLARE_CAN.to_owned()]);
    let calls = completer.clone();
    let (service, user, course) = seed_basis_course(completer).await;
    let error = service
        .declare_basis(&user, &course, basis_request(&["G1"]))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("已会声明通道已关闭"), "{error}");
    let error = service.revoke_basis(&user, &course).await.unwrap_err();
    assert!(error.to_string().contains("已会声明通道已关闭"), "{error}");
    assert_eq!(calls.call_count(), 0, "off 臂零 LLM");
    assert_eq!(basis_anchor_count(&service, &course).await, 0);
    // 读数面照常可读（如实标注 off）。
    let view = service.basis_view(&course).await.unwrap();
    assert!(!view.enabled);
}

// ── f1：声明概念被复用批，评审挂号表非降级（批外锚放行）─────────────────

/// f1 端到端（法证案例回放，e2e2/server.log 06:46-06:47）：声明铸下的概念
/// 被生长批复用（defnear 直定宿主 + 评审 reuse 裁决同宿主）时，挂号行
/// concept 锚定登记表正名放行——评审非降级通过、零白烧重试、登记表零
/// 重复（提名名落池为宿主活跃别名）。修复前该批两连死降级（本批评审锚定
/// 全废）。
#[tokio::test]
#[serial_test::serial]
async fn growth_review_reusing_declared_concept_passes_provenance_without_degradation() {
    let declare = SequencedCompleter::new(vec![BASIS_DECLARE_CAN.to_owned()]);
    let (service, user, course) = seed_basis_course(declare).await;
    service
        .declare_basis(&user, &course, basis_request(&["G1", "G2"]))
        .await
        .unwrap();
    let declared_id: String = sqlx::query_scalar(
        "SELECT concept_id FROM learning_concept_registry WHERE canonical = '门口判读'",
    )
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    // 生长落库需要模块行（apply_batch 取首模块；声明零节点课程需补壳）。
    let module_id = LearningModuleId::new().into_string();
    sqlx::query(
        "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
         VALUES (?, ?, '学习图', '', 0)",
    )
    .bind(&module_id)
    .bind(course.as_str())
    .execute(service.pool_for_tests())
    .await
    .unwrap();

    // 生长批：同定义提名「判读进阶」→ gate 直定宿主 = 声明概念（机械锚）；
    // 评审 reuse 裁决同宿主（核验锚）；挂号行 concept 锚定声明概念正名。
    let batch_json = r#"{
        "nodes": [
            {"title": "用判读进阶重看门口", "purpose": "从声明判据切入", "est": 15,
             "teaches": [{"name": "判读进阶", "tier": "know"}], "assumes": []}
        ],
        "mints": [
            {"canonical": "判读进阶", "aliases": [], "definition": "按门口判据读出自身具备状态"}
        ],
        "new_aliases": [],
        "note": "声明复用稿"
    }"#;
    let review_json = format!(
        r#"{{"verdict":"pass","reuse":[{{"name":"判读进阶","concept_id":"{declared_id}"}}],"term_provenance":[
            {{"concept":"判读进阶","term":"具备状态","source":"常识"}},
            {{"concept":"门口判读","term":"判据","source":"已教","ref":"门口判读"}}]}}"#
    );
    let completer = SequencedCompleter::new(vec![batch_json.to_owned(), review_json]);
    *service.course_completer.write().unwrap() = Some(completer);
    service
        .kick_growth(&user, &course, true, None)
        .await
        .unwrap();
    wait_for_growth_batch(&service, course.as_str()).await;

    // 评审非降级 + 零重试 + 挂号行锚定 registry 概念（external_hits = 1）。
    let stats = service.growth_run_stats().expect("stats recorded");
    assert_eq!(stats.stop_reason, "success", "{stats:?}");
    assert!(!stats.review_provenance_degraded, "评审非降级：{stats:?}");
    assert_eq!(stats.review_provenance_invalid, 0, "{stats:?}");
    assert_eq!(stats.review_provenance_retried, 0, "零白烧重试：{stats:?}");
    assert_eq!(stats.review_provenance_rows, 2, "{stats:?}");
    assert_eq!(
        stats.review_provenance_external_hits, 1,
        "挂号行锚定 registry 概念进读数：{stats:?}"
    );

    // 登记表零重复：声明 2 + 生长 0（提名名落池为宿主活跃别名）。
    assert_eq!(basis_registry_count(&service).await, 2);
    let aliases_json: String = sqlx::query_scalar(
        "SELECT aliases_json FROM learning_concept_registry WHERE concept_id = ?",
    )
    .bind(&declared_id)
    .fetch_one(service.pool_for_tests())
    .await
    .unwrap();
    assert!(
        aliases_json.contains("判读进阶"),
        "提名名落池：{aliases_json}"
    );
}
