//! y07 节点容量反馈信号的服务级测试：
//! 发布尾落 `capacity_note`（缺席清 NULL）、per-user latest-wins 留痕、
//! 回显只属于当前用户、课程级超载聚合、以及**零结算零置位**硬边界
//! （一次反馈调用前后 attempts/review/xp/progress/FSRS 计数、课时 status、
//! 任务注册表与 WS phase 全不变）。

use super::*;

use crate::models::{CapacityFeedbackChoice, LessonPack, ModulePack};

/// 三门课的 pack：容量聚合测试要能区分「有自报 / 空白自报 / 无自报」。
fn capacity_pack() -> CoursePack {
    let lesson = |title: &str| LessonPack {
        title: title.into(),
        summary: String::new(),
        purpose: format!("{title}的目标"),
        est: 10,
        source: None,
        sources: Vec::new(),
        activities: Vec::new(),
        sections: Vec::new(),
    };
    CoursePack {
        title: "容量反馈课".into(),
        teaching_style: TeachingStyle::Standard,
        description: String::new(),
        domain: "math".into(),
        source_kb_id: None,
        version: 1,
        modules: vec![ModulePack {
            title: "模块一".into(),
            description: String::new(),
            lessons: vec![lesson("第一课"), lesson("第二课"), lesson("第三课")],
        }],
    }
}

async fn capacity_test_service() -> (LearningService, SqlitePool, UserId, CourseDetail) {
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let user_id = UserId::parse(owner_id).unwrap();
    let service = LearningService::new(database.pool().clone());
    // 按需课时是「骨架 + 生成」：outline 导入写蓝图快照且 content_generated=0，
    // 发布尾才会真正跑一遍（pack 导入的教程形状直接标记为已生成）。
    let blueprint = Blueprint {
        title: "容量反馈课".into(),
        description: String::new(),
        domain: "math".into(),
        version: 1,
        modules: vec![crate::generation::BlueprintModule {
            title: "模块一".into(),
            description: String::new(),
            lessons: ["第一课", "第二课", "第三课"]
                .into_iter()
                .map(|title| crate::generation::BlueprintLesson {
                    title: title.into(),
                    purpose: format!("{title}的目标"),
                    source: None,
                    sources: Vec::new(),
                    anchor: None,
                })
                .collect(),
        }],
    };
    let detail = service
        .import_course_outline(
            capacity_pack(),
            OutlineSnapshot {
                blueprint_json: serde_json::to_string(&blueprint).unwrap(),
                samples_json: "[]".into(),
                learning_goal: String::new(),
                samples_meta_json: None,
                source_kb_ids: Vec::new(),
                course_source_plan_json: None,
            },
        )
        .await
        .unwrap();
    (service, database.pool().clone(), user_id, detail)
}

/// 生成引擎替身：可改写的超容自报 + 记录收到的上下文。
struct CapacityEngine {
    note: Mutex<Option<String>>,
    contexts: Mutex<Vec<LessonGenerationContext>>,
}

impl CapacityEngine {
    fn new(note: Option<&str>) -> Arc<Self> {
        Arc::new(Self {
            note: Mutex::new(note.map(str::to_owned)),
            contexts: Mutex::new(Vec::new()),
        })
    }

    fn set_note(&self, note: Option<&str>) {
        *self.note.lock().unwrap() = note.map(str::to_owned);
    }
}

#[async_trait::async_trait]
impl LessonContentAgentEngine for CapacityEngine {
    async fn generate(
        &self,
        _user_id: &UserId,
        context: &LessonGenerationContext,
        _model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, AppError> {
        self.contexts.lock().unwrap().push(context.clone());
        Ok(LessonOutput {
            summary: "## 讲解\n占位正文。\n".into(),
            estimated_minutes: 10,
            activities: Vec::new(),
            sections: Vec::new(),
            degraded_keys: Vec::new(),
            capacity_note: self.note.lock().unwrap().clone(),
            viz_lint_events: 0,
        })
    }

    async fn resume(
        &self,
        _user_id: &UserId,
        _draft_id: &str,
        context: &LessonGenerationContext,
        _model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, AppError> {
        self.generate(_user_id, context, _model_override).await
    }
}

/// 发布尾：`output.capacity_note` 落库；下一次发布缺席（或空白）时写回
/// NULL 清旧值——内容元数据永远反映最近一次发布，不残留旧自报。
#[tokio::test]
async fn publish_persists_capacity_note_and_absent_note_clears_it() {
    let (service, pool, user_id, detail) = capacity_test_service().await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    let engine = CapacityEngine::new(Some("保覆盖必超：第三节的推导超出 25 分钟预算"));
    service.set_lesson_engine(engine.clone());
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();

    let view = service.lesson_detail(&user_id, &lesson_id).await.unwrap();
    assert_eq!(
        view.capacity_note.as_deref(),
        Some("保覆盖必超：第三节的推导超出 25 分钟预算")
    );

    // 模拟一次内容重建（重置生成标记后重发布）：engine 这次没有自报。
    engine.set_note(Some("   "));
    sqlx::query("UPDATE learning_lessons SET content_generated = 0 WHERE lesson_id = ?")
        .bind(lesson_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    service
        .generate_lesson_content(&user_id, &lesson_id, &GenerateLessonRequest::default())
        .await
        .unwrap();

    let view = service.lesson_detail(&user_id, &lesson_id).await.unwrap();
    assert_eq!(view.capacity_note, None, "缺席（空白）自报必须清掉旧值");
    let stored: Option<String> =
        sqlx::query_scalar("SELECT capacity_note FROM learning_lessons WHERE lesson_id = ?")
            .bind(lesson_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, None, "列上必须是 NULL，不是空串");
}

/// latest-wins：同用户同课时覆盖写（首写时刻保留、updated_at 刷新）；
/// 不同用户互不可见（读回只给当前用户自己那一行）。
#[tokio::test]
async fn capacity_feedback_is_per_user_latest_wins_and_not_shared() {
    let (service, pool, user_a, detail) = capacity_test_service().await;
    let course_id = detail.course.id.clone();
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service.enroll(&course_id, &user_a).await.unwrap();

    assert_eq!(
        service
            .lesson_detail(&user_a, &lesson_id)
            .await
            .unwrap()
            .capacity_feedback,
        None,
        "未反馈时无回显"
    );
    let first = service
        .record_capacity_feedback(&user_a, &lesson_id, "accept")
        .await
        .unwrap();
    assert_eq!(first.choice, CapacityFeedbackChoice::Accept);
    let first_created: i64 = sqlx::query_scalar(
        "SELECT created_at FROM learning_lesson_capacity_feedback \
         WHERE user_id = ? AND lesson_id = ?",
    )
    .bind(user_a.as_str())
    .bind(lesson_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();

    let second = service
        .record_capacity_feedback(&user_a, &lesson_id, "split")
        .await
        .unwrap();
    assert_eq!(second.choice, CapacityFeedbackChoice::Split);
    let rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_lesson_capacity_feedback")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rows, 1, "latest-wins：同键只留一行");
    let (same_created, updated_at): (i64, i64) = sqlx::query_as(
        "SELECT created_at, updated_at FROM learning_lesson_capacity_feedback \
         WHERE user_id = ? AND lesson_id = ?",
    )
    .bind(user_a.as_str())
    .bind(lesson_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(same_created, first_created, "覆盖不改首写时刻");
    assert_eq!(updated_at, second.at, "覆盖刷新 updated_at");
    let view = service.lesson_detail(&user_a, &lesson_id).await.unwrap();
    assert_eq!(
        view.capacity_feedback.map(|feedback| feedback.choice),
        Some(CapacityFeedbackChoice::Split)
    );

    // 另一个用户：先被拒（未选课 404），选课后只看得到自己的选择。
    let user_b = UserId::new();
    let error = service
        .record_capacity_feedback(&user_b, &lesson_id, "accept")
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::NotFound(_)),
        "未选课必须按 404 口径拒绝，got {error:?}"
    );
    service.enroll(&course_id, &user_b).await.unwrap();
    assert_eq!(
        service
            .lesson_detail(&user_b, &lesson_id)
            .await
            .unwrap()
            .capacity_feedback,
        None,
        "不得看到他人选择"
    );
    service
        .record_capacity_feedback(&user_b, &lesson_id, "extend")
        .await
        .unwrap();
    assert_eq!(
        service
            .lesson_detail(&user_a, &lesson_id)
            .await
            .unwrap()
            .capacity_feedback
            .map(|feedback| feedback.choice),
        Some(CapacityFeedbackChoice::Split),
        "A 的选择不被 B 覆盖"
    );
    assert_eq!(
        service
            .lesson_detail(&user_b, &lesson_id)
            .await
            .unwrap()
            .capacity_feedback
            .map(|feedback| feedback.choice),
        Some(CapacityFeedbackChoice::Extend)
    );
}

/// 写侧严格：空/未知 choice 一律 400，且零残留行；不存在的课时是 404。
#[tokio::test]
async fn capacity_feedback_rejects_unknown_choice_and_missing_lesson() {
    let (service, pool, user_id, detail) = capacity_test_service().await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service.enroll(&detail.course.id, &user_id).await.unwrap();

    for choice in ["", "   ", "later", "ACCEPT"] {
        let error = service
            .record_capacity_feedback(&user_id, &lesson_id, choice)
            .await
            .unwrap_err();
        assert!(
            matches!(error, AppError::BadRequest(_)),
            "choice={choice:?} 必须 400，got {error:?}"
        );
    }
    let rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM learning_lesson_capacity_feedback")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rows, 0, "非法 choice 零写入");

    let missing = LearningLessonId::new();
    let error = service
        .record_capacity_feedback(&user_id, &missing, "accept")
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::NotFound(_)),
        "不存在的课时必须 404，got {error:?}"
    );
}

/// 课程聚合是内容级纯读：活跃课时里 `capacity_note` 非空（trim 后）的计数；
/// 空白自报与无自报都不计入。
#[tokio::test]
async fn course_detail_counts_lessons_with_a_capacity_note() {
    let (service, pool, user_id, detail) = capacity_test_service().await;
    let lessons: Vec<String> = detail.modules[0]
        .lessons
        .iter()
        .map(|lesson| lesson.id.as_str().to_owned())
        .collect();
    for (index, note) in [(0, Some("超容：例子太多")), (1, Some("  ")), (2, None)] {
        sqlx::query("UPDATE learning_lessons SET capacity_note = ? WHERE lesson_id = ?")
            .bind(note)
            .bind(&lessons[index])
            .execute(&pool)
            .await
            .unwrap();
    }

    let detail = service
        .course_detail(&detail.course.id, Some(&user_id))
        .await
        .unwrap();
    assert_eq!(detail.overloaded_lessons, 1);
    assert_eq!(
        detail.modules[0].lessons[0].capacity_note.as_deref(),
        Some("超容：例子太多"),
        "目录视图同样携带内容元数据"
    );
}

/// 零结算零置位硬边界：反馈调用不写 attempts/review/xp/progress/FSRS，
/// 不改课时 status 与 est，不登记生成任务、不发任何 WS phase。
#[tokio::test]
async fn capacity_feedback_has_zero_settlement_side_effects() {
    let (service, pool, user_id, detail) = capacity_test_service().await;
    let lesson_id = detail.modules[0].lessons[0].id.clone();
    service.enroll(&detail.course.id, &user_id).await.unwrap();
    // 先有一条进行中的课时进度，让「status 不变」可观测。
    service
        .update_lesson_progress(&lesson_id, &user_id, LessonStatus::InProgress)
        .await
        .unwrap();

    let events = Arc::new(CapturingCapacityEvents::default());
    service.set_event_sink(LearningEventEmitter::new(
        events.clone(),
        Arc::from(user_id.as_str()),
    ));

    async fn ledger_counts(pool: &SqlitePool) -> Vec<i64> {
        let mut counts = Vec::new();
        for table in [
            "learning_attempts",
            "learning_review_events",
            "learning_review_log",
            "learning_xp_events",
            "learning_lesson_progress",
            "learning_review_items",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(pool)
                .await
                .unwrap();
            counts.push(count);
        }
        let fsrs: (i64, f64, f64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(stability_days), 0.0), \
                    COALESCE(SUM(difficulty), 0.0), COALESCE(SUM(review_count), 0) \
             FROM learning_review_items",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        counts.push(fsrs.0);
        counts.push(fsrs.1 as i64);
        counts.push(fsrs.2 as i64);
        counts.push(fsrs.3);
        counts
    }

    let before = ledger_counts(&pool).await;
    let status_before: String = sqlx::query_scalar(
        "SELECT status FROM learning_lesson_progress WHERE lesson_id = ?",
    )
    .bind(lesson_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let est_before: i64 =
        sqlx::query_scalar("SELECT estimated_minutes FROM learning_lessons WHERE lesson_id = ?")
            .bind(lesson_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();

    service
        .record_capacity_feedback(&user_id, &lesson_id, "extend")
        .await
        .unwrap();

    assert_eq!(ledger_counts(&pool).await, before, "结算/复习/进度表零变化");
    let status_after: String = sqlx::query_scalar(
        "SELECT status FROM learning_lesson_progress WHERE lesson_id = ?",
    )
    .bind(lesson_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status_after, status_before, "课时 status 不变");
    let est_after: i64 =
        sqlx::query_scalar("SELECT estimated_minutes FROM learning_lessons WHERE lesson_id = ?")
            .bind(lesson_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(est_after, est_before, "est 是内容尺寸/XP 基数/水位硬红线");
    assert!(
        service.generation_tasks.snapshot().is_empty(),
        "反馈不登记任何生成任务"
    );
    assert!(
        events.events.lock().unwrap().is_empty(),
        "反馈不发任何 WS phase"
    );
}

#[derive(Default)]
struct CapturingCapacityEvents {
    events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl nomifun_realtime::UserEventSink for CapturingCapacityEvents {
    fn send_to_user(&self, _user_id: &str, event: nomifun_api_types::WebSocketMessage<serde_json::Value>) {
        self.events.lock().unwrap().push((event.name, event.data));
    }
}
