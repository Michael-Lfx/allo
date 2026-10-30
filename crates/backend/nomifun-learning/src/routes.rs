use std::str::FromStr;

use axum::extract::{Extension, Json, Path, Query, RawQuery, State};
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::{
    AppError, LearningActivityId, LearningCourseId, LearningLessonId, LearningReviewItemId,
    ProviderId,
    UuidV7Error,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use url::form_urlencoded;

use crate::models::{
    AnswerReviewRequest, CalendarStats, CheckinStatus, CoursePack,
    CreateCustomQuestionRequest, CreateLessonActivityRequest, DeclareBasisRequest,
    GenerateCourseRequest, GenerateLessonActivityRequest,
    GenerateLessonRequest, LearningGraphGenerationStatus, MemoryCalibrationBin, MountRoundSummary,
    QuizGradeFeedback, QuizProgressView, QuizRoundReport, QuizRoundView,
    RateReviewRequest, RepairFigureRequest, RepairFigureResponse, SetTagsRequest, StructureRepairSummary, SubmitAttemptRequest,
    UpdateLessonProgressRequest, UpdateLessonSectionBodyRequest, UpdateQuestionRequest,
    XpCalendar, XpReading,
};
use crate::state::LearningRouterState;

pub fn learning_routes(state: LearningRouterState) -> Router {
    Router::new()
        .route(
            "/api/learning/courses",
            get(list_courses).post(import_course),
        )
        .route("/api/learning/courses/generate", post(generate_course))
        .route(
            "/api/learning/courses/generate/estimate",
            post(estimate_course_generation),
        )
        .route("/api/learning/courses/generate/status", get(learning_graph_generation_status))
        .route("/api/learning/generation-tasks", get(list_generation_tasks))
        .route("/api/learning/generation-tasks/{key}/cancel", post(cancel_generation_task))
        .route(
            "/api/learning/courses/generate/cancel",
            post(cancel_learning_graph_generation),
        )
        .route(
            "/api/learning/courses/{id}/graph/grow",
            post(grow_learning_graph),
        )
        .route(
            "/api/learning/courses/{id}/growth/verdict",
            get(growth_verdict),
        )
        .route(
            "/api/learning/courses/{id}/graph/history",
            get(graph_history),
        )
        .route(
            "/api/learning/courses/{id}/graph/concepts",
            get(graph_concepts),
        )
        .route(
            "/api/learning/courses/{id}/concepts/audit",
            get(concept_audit),
        )
        .route(
            "/api/learning/courses/{id}/graph/structure/regenerate",
            post(regenerate_graph_structure),
        )
        .route(
            "/api/learning/courses/{id}/graph/structure/mount",
            post(mount_structure),
        )
        .route(
            "/api/learning/courses/{id}/graph/structure/repair",
            post(repair_structure),
        )
        .route(
            "/api/learning/courses/{id}/graph/structure",
            get(get_structure_view),
        )
        // 已会声明（Declared Known，m2）：勾选门口 → 按声明铸概念（不落节点）
        // + 读数 + 撤销（概念留池）。off 开关 = 409 如实拒绝（service 侧执法）。
        .route(
            "/api/learning/courses/{id}/basis/declare",
            post(declare_basis),
        )
        .route("/api/learning/courses/{id}/basis", get(get_basis).delete(revoke_basis))
        .route("/api/learning/courses/{id}", get(get_course))
        .route("/api/learning/courses/{id}", delete(delete_course))
        .route("/api/learning/courses/{id}", patch(rename_course))
        .route("/api/learning/courses/{id}/archive", post(archive_course))
        .route("/api/learning/courses/{id}/restore", post(restore_course))
        .route("/api/learning/courses/{id}/tags", put(set_course_tags))
        .route("/api/learning/courses/{id}/enroll", post(enroll))
        .route(
            "/api/learning/courses/{id}/diagnostic",
            get(diagnostic_plan),
        )
        .route("/api/learning/lessons/{id}", get(lesson_detail))
        .route(
            "/api/learning/lessons/{id}/progress",
            post(update_lesson_progress),
        )
        .route(
            "/api/learning/lessons/{id}/generate",
            post(generate_lesson),
        )
        .route(
            "/api/learning/lessons/{id}/sections/{section_key}/rewrite",
            post(rewrite_lesson_section),
        )
        .route(
            "/api/learning/lessons/{id}/sections/{section_key}/body",
            put(update_lesson_section_body),
        )
        .route(
            "/api/learning/lessons/{id}/activities",
            post(create_lesson_activity),
        )
        .route(
            "/api/learning/lessons/{id}/activities/generate",
            post(generate_lesson_activity),
        )
        .route("/api/learning/figures/repair", post(repair_figure))
        .route(
            "/api/learning/activities/{id}/attempts",
            post(submit_attempt),
        )
        .route("/api/learning/reviews/due", get(due_reviews))
        .route("/api/learning/checkins/today", get(checkin_today))
        .route("/api/learning/xp/reading", get(xp_reading))
        .route("/api/learning/xp/calendar", get(xp_calendar))
        .route(
            "/api/learning/courses/{id}/quiz/progress",
            get(quiz_progress),
        )
        .route("/api/learning/courses/{id}/quiz/start", post(quiz_start))
        .route(
            "/api/learning/courses/{id}/quiz/rounds/{round_id}/grade",
            post(quiz_grade),
        )
        .route(
            "/api/learning/courses/{id}/quiz/rounds/{round_id}/complete",
            post(quiz_complete),
        )
        .route(
            "/api/learning/quiz/calibration",
            get(quiz_calibration),
        )
        .route("/api/learning/stats/calendar", get(calendar_stats))
        .route("/api/learning/stats/memory", get(memory_stats))
        .route("/api/learning/tags", get(list_tags))
        .route("/api/learning/reviews/{id}/answer", post(answer_review))
        .route("/api/learning/reviews/{id}/rate", post(rate_review))
        .route("/api/learning/reviews/{id}/skip", post(skip_review))
        .route("/api/learning/reviews/{id}/archive", post(archive_review))
        .route("/api/learning/reviews/{id}/unarchive", post(unarchive_review))
        .route("/api/learning/reviews/{id}/mark-edit", post(mark_review_edit))
        .route("/api/learning/reviews/{id}", get(review_question))
        .route("/api/learning/reviews/{id}", delete(delete_review_item))
        .route("/api/learning/questions", get(list_questions))
        .route(
            "/api/learning/questions/{activity_id}",
            put(update_question),
        )
        .route(
            "/api/learning/questions/{activity_id}/tags",
            put(set_question_tags),
        )
        .route(
            "/api/learning/custom-questions",
            post(create_custom_question),
        )
        .route(
            "/api/learning/custom-questions/{id}",
            put(update_custom_question).delete(delete_custom_question).get(custom_question),
        )
        .route(
            "/api/learning/custom-questions/{id}/archive",
            post(archive_custom),
        )
        .route(
            "/api/learning/custom-questions/{id}/unarchive",
            post(unarchive_custom),
        )
        .route(
            "/api/learning/custom-questions/{id}/mark-edit",
            post(mark_custom_edit),
        )
        .route(
            "/api/learning/custom-questions/{id}/tags",
            put(set_custom_question_tags),
        )
        .route(
            "/api/learning/custom-questions/{id}/answer",
            post(answer_custom_review),
        )
        .route(
            "/api/learning/custom-questions/{id}/rate",
            post(rate_custom_review),
        )
        .route(
            "/api/learning/custom-questions/{id}/skip",
            post(skip_custom_review),
        )
        .with_state(state)
}

async fn list_courses(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<crate::models::CourseSummary>>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.list_courses(&user.id).await?,
    )))
}

async fn import_course(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(pack): Json<CoursePack>,
) -> Result<Json<ApiResponse<crate::models::CourseDetail>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.import_course(pack).await?,
    )))
}

async fn generate_course(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Json(request): Json<GenerateCourseRequest>,
) -> Result<Json<ApiResponse<crate::models::CourseDetail>>, AppError> {
    // The whole generation runs synchronously inside the handler (mirroring
    // the concept graph endpoint): the loop pushes progress over the
    // best-effort WebSocket stream, and the response carries the imported
    // course. Aborting the request drops this future at the next await
    // point, which ends the loop and releases the in-flight slot.
    Ok(Json(ApiResponse::ok(
        state.service.generate_course(&user.id, request).await?,
    )))
}

/// t5 成本门估算：提交建课前对知识库流做一次规则分诊（零 LLM），返回
/// 文件数/批数/预估调用/耗时与 `needs_confirm`（阈值后端单源）。
async fn estimate_course_generation(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Json(request): Json<GenerateCourseRequest>,
) -> Result<Json<ApiResponse<crate::models::CourseGenerationEstimateView>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.estimate_course_generation(&user.id, request).await?,
    )))
}

/// 课程生成状态（学习图生长与大纲流共用）：后台指示条的数据源。
async fn learning_graph_generation_status(
    State(state): State<LearningRouterState>,
) -> Result<Json<ApiResponse<LearningGraphGenerationStatus>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.generation_status())))
}

/// 长任务快照（三轮 t2）：四类生成任务的在跑/终态一览（在跑置顶、注册序
/// 倒序），供指示条与诊断面。
async fn list_generation_tasks(
    State(state): State<LearningRouterState>,
) -> Result<Json<ApiResponse<Vec<crate::service::GenerationTaskView>>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.generation_task_snapshot())))
}

/// 统一取消（按任务键，三轮 t2 真取消）：取消胜出 = `{state: "cancelled"}`
/// （不报错误）；任务先完成 = 竞速落败如实 `{state: "done"|"failed"}`；
/// 键不存在 = 404。
async fn cancel_generation_task(
    State(state): State<LearningRouterState>,
    Path(key): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let outcome = state.service.cancel_generation_task(&key)?;
    Ok(Json(ApiResponse::ok(
        serde_json::json!({ "state": outcome }),
    )))
}

/// 取消进行中的课程生成/生长（legacy 全局端点，转发到注册表）：触发全部
/// 可取消在跑任务的 token（growth/structure/outline），任务在检查点或
/// LLM 调用内（select! 真中断）停止。无可取消任务时 cancelled=false（幂等）。
async fn cancel_learning_graph_generation(
    State(state): State<LearningRouterState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let cancelled = state.service.cancel_generation();
    Ok(Json(ApiResponse::ok(serde_json::json!({
        "cancelled": cancelled,
    }))))
}

/// 手动触发一次生长（就绪 < 目标即补到 7；已在生长中报冲突）。e3 起也是
/// 零节点课程的首批触发面：学习窗口「开始学习」调这里（force 恒过停摆臂）。
async fn grow_learning_graph(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    let kicked = state
        .service
        .kick_growth(&user.id, &course_id, true, None)
        .await?;
    Ok(Json(ApiResponse::ok(serde_json::json!({ "kicked": kicked }))))
}

/// 自动补批判定（m10 触发②）：纯读裁决——就绪存量 × 开关合力 →
/// due | below_threshold | disabled。零 LLM 零写入，Disabled 臂与停摆臂
/// 如实读数；前端课程工作区加载时据此决定是否自动调既有 POST grow。
async fn growth_verdict(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::GrowthVerdictView>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .growth_verdict(&user.id, &course_id)
            .await?,
    )))
}

async fn graph_history(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::GraphHistoryView>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.graph_history(&course_id, &user.id).await?,
    )))
}

/// 知识结构读侧投影（200 恒答、structure: null = 未生成；软删/不存在 404）。
/// 前端结构图 Tab 以 WS structure_* 事件触发重取本接口。
async fn get_structure_view(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::LearningStructureView>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.structure_view(&course_id).await?,
    )))
}

async fn graph_concepts(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<crate::models::GraphConceptRowView>>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.graph_concepts(&course_id, &user.id).await?,
    )))
}

/// 已会声明（m2）：勾选门口 → 幂等增量铸概念（零节点写）。整个管线同步跑
/// 在 handler 里（generate_course 先例）：一次结构化 LLM 调用 + 命名门，
/// 落库在单事务。
async fn declare_basis(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<DeclareBasisRequest>,
) -> Result<Json<ApiResponse<crate::models::DeclarationResultView>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .declare_basis(&user.id, &course_id, request)
            .await?,
    )))
}

/// 已会声明读数（GET basis）：已声明 doors/blocks/concepts + Broken 降级披露。
async fn get_basis(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::BasisView>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.basis_view(&course_id).await?,
    )))
}

/// 撤销本课全部已会声明（删锚行；概念留池——「声明可撤、概念不灭」）。
async fn revoke_basis(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Value>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    let revoked = state.service.revoke_basis(&user.id, &course_id).await?;
    Ok(Json(ApiResponse::ok(
        serde_json::json!({ "revoked": revoked }),
    )))
}

/// 定义审计（m9 只读，零 LLM 零写入）：登记表全量 × 用语表三层 →
/// 缺口片段/自指/互指环。「读侧先看见」——审计是缺口标记不是拦截门；
/// 概念表 Tab 的行级标记数据源。
async fn concept_audit(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::ConceptAuditReport>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.concept_audit(&course_id).await?,
    )))
}

/// 结构重生成请求（可选模型偏好；缺省用实现方的默认 provider）。
#[derive(Debug, Default, serde::Deserialize)]
struct RegenerateStructureRequest {
    #[serde(default)]
    provider_id: Option<ProviderId>,
    #[serde(default)]
    model: Option<String>,
}

/// 重跑知识结构生成（g2：重生成 = 换参考地图不换课，快照原子覆盖）；
/// 进度走 WS structure_* 事件。已在生成中报冲突。
async fn regenerate_graph_structure(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    request: Option<Json<RegenerateStructureRequest>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    if request
        .as_ref()
        .and_then(|Json(request)| request.provider_id.as_ref())
        .is_some() != request
        .as_ref()
        .and_then(|Json(request)| request.model.as_deref())
        .is_some()
    {
        return Err(AppError::BadRequest(
            "provider_id and model must be provided together".into(),
        ));
    }
    let model_override = request
        .and_then(|Json(request)| request.provider_id.zip(request.model));
    let kicked = state
        .service
        .regenerate_structure(&user.id, &course_id, model_override.as_ref().map(|(provider, model)| (provider, model.as_str())))
        .await?;
    Ok(Json(ApiResponse::ok(serde_json::json!({ "kicked": kicked }))))
}

/// 结构挂载回合（j08）：显式命令触发、单课串行、可取消；同步返回回合摘
/// 要（单 LLM 调用，秒级）。
async fn mount_structure(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    request: Option<Json<RegenerateStructureRequest>>,
) -> Result<Json<ApiResponse<MountRoundSummary>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    if request
        .as_ref()
        .and_then(|Json(request)| request.provider_id.as_ref())
        .is_some() != request
        .as_ref()
        .and_then(|Json(request)| request.model.as_deref())
        .is_some()
    {
        return Err(AppError::BadRequest(
            "provider_id and model must be provided together".into(),
        ));
    }
    let model_override = request
        .and_then(|Json(request)| request.provider_id.zip(request.model));
    let summary = state
        .service
        .run_structure_mount(
            &user.id,
            &course_id,
            model_override.as_ref().map(|(provider, model)| (provider, model.as_str())),
        )
        .await?;
    Ok(Json(ApiResponse::ok(summary)))
}

/// 结构修补回合（j09）：显式命令触发、单课串行、可取消；同步返回回合摘
/// 要（单 LLM 调用，秒级）。触发门槛在服务面（findings ∪ 挂载观察皆空 =
/// skipped）。
async fn repair_structure(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    request: Option<Json<RegenerateStructureRequest>>,
) -> Result<Json<ApiResponse<StructureRepairSummary>>, AppError> {
    let course_id = parse_id::<LearningCourseId>(id)?;
    if request
        .as_ref()
        .and_then(|Json(request)| request.provider_id.as_ref())
        .is_some() != request
        .as_ref()
        .and_then(|Json(request)| request.model.as_deref())
        .is_some()
    {
        return Err(AppError::BadRequest(
            "provider_id and model must be provided together".into(),
        ));
    }
    let model_override = request
        .and_then(|Json(request)| request.provider_id.zip(request.model));
    let summary = state
        .service
        .run_structure_repair(
            &user.id,
            &course_id,
            model_override
                .as_ref()
                .map(|(provider, model)| (provider, model.as_str())),
        )
        .await?;
    Ok(Json(ApiResponse::ok(summary)))
}

async fn get_course(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::CourseDetail>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.course_detail(&id, Some(&user.id)).await?,
    )))
}

async fn enroll(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::CourseDetail>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    state.service.enroll(&id, &user.id).await?;
    Ok(Json(ApiResponse::ok(
        state.service.course_detail(&id, Some(&user.id)).await?,
    )))
}

#[derive(Debug, Deserialize)]
struct DiagnosticQuery {
    #[serde(default = "default_diagnostic_limit")]
    limit: i64,
}

const fn default_diagnostic_limit() -> i64 {
    10
}

async fn diagnostic_plan(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Query(query): Query<DiagnosticQuery>,
) -> Result<Json<ApiResponse<crate::models::DiagnosticPlan>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .diagnostic_plan(&id, &user.id, query.limit)
            .await?,
    )))
}

async fn lesson_detail(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::LessonView>>, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.lesson_detail(&user.id, &id).await?,
    )))
}

/// `?wait=true` 同步臂查询参数（e4 双模化）：缺省 = 异步（202 + 任务键，
/// 任务脱离请求生命周期，断连不取消）；`wait=true` = 既有同步语义（分钟级
/// 阻塞直至发布——冒烟/脚本的回归面）。
#[derive(Debug, Deserialize)]
struct LessonGenerateQuery {
    wait: Option<bool>,
}

/// e4 双模化：默认异步——登记 j03 任务注册表（kind=lesson）即回 202 +
/// 任务键，生成体在脱离任务里跑完（learnhub「分钟级任务不占请求生命周期」
/// 红线）；前端轮询 `GET /api/learning/generation-tasks`（响应体 `poll`
/// 字段）或听 `learning.lesson-generation` WS 流。`?wait=true` 保留同步臂。
async fn generate_lesson(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Query(query): Query<LessonGenerateQuery>,
    Json(request): Json<GenerateLessonRequest>,
) -> Result<axum::response::Response, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    if query.wait.unwrap_or(false) {
        // 同步臂：返回已发布的课时视图（历史响应形状零变化）。
        let view = state
            .service
            .generate_lesson_content(&user.id, &id, &request)
            .await?;
        return Ok(Json(ApiResponse::ok(view)).into_response());
    }
    // 默认异步臂：202 Accepted + 任务键。
    let accepted = state
        .service
        .start_lesson_generation(&user.id, &id, &request)
        .await?;
    let mut response = Json(ApiResponse::ok(accepted)).into_response();
    *response.status_mut() = axum::http::StatusCode::ACCEPTED;
    Ok(response)
}

async fn rewrite_lesson_section(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path((id, section_key)): Path<(String, String)>,
    Json(request): Json<GenerateLessonRequest>,
) -> Result<Json<ApiResponse<crate::models::LessonView>>, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .rewrite_lesson_section(&user.id, &id, &section_key, &request)
            .await?,
    )))
}

async fn update_lesson_section_body(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path((id, section_key)): Path<(String, String)>,
    Json(request): Json<UpdateLessonSectionBodyRequest>,
) -> Result<Json<ApiResponse<crate::models::LessonView>>, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .update_lesson_section_body(&user.id, &id, &section_key, &request)
            .await?,
    )))
}

async fn create_lesson_activity(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<CreateLessonActivityRequest>,
) -> Result<Json<ApiResponse<crate::models::LessonView>>, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .create_lesson_activity(&user.id, &id, request)
            .await?,
    )))
}

async fn repair_figure(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(request): Json<RepairFigureRequest>,
) -> Result<Json<ApiResponse<RepairFigureResponse>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.repair_figure(&request).await?,
    )))
}

async fn generate_lesson_activity(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<GenerateLessonActivityRequest>,
) -> Result<Json<ApiResponse<crate::models::GeneratedLessonActivity>>, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .generate_lesson_activity(&user.id, &id, request)
            .await?,
    )))
}

async fn update_lesson_progress(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<UpdateLessonProgressRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    state
        .service
        .update_lesson_progress(&id, &user.id, request.status)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn submit_attempt(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<SubmitAttemptRequest>,
) -> Result<Json<ApiResponse<crate::models::AttemptResult>>, AppError> {
    let id = parse_id::<LearningActivityId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .submit_attempt(
                &id,
                &user.id,
                request.response,
                request.provider_id,
                request.model,
            )
            .await?,
    )))
}

#[derive(Debug, Deserialize)]
struct DueReviewQuery {
    #[serde(default = "default_review_limit")]
    limit: i64,
    /// Keep only reviews whose due time has passed (main review entry).
    #[serde(default)]
    due_only: bool,
    /// Also include learner-authored questions that belong to no course.
    #[serde(default)]
    orphan: bool,
}

const fn default_review_limit() -> i64 {
    30
}

async fn due_reviews(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<DueReviewQuery>,
    RawQuery(raw): RawQuery,
) -> Result<Json<ApiResponse<Vec<crate::models::DueReview>>>, AppError> {
    // serde_urlencoded 0.7 cannot map repeated query keys onto Vec fields
    // (it rejects them with "expected a sequence"), so the multi-valued
    // course/tag parameters are collected manually from the raw query string.
    let mut course_ids: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    if let Some(raw) = raw {
        for (key, value) in form_urlencoded::parse(raw.as_bytes()) {
            match key.as_ref() {
                "course_id" => course_ids.push(value.into_owned()),
                "tag" => tags.push(value.into_owned()),
                _ => {}
            }
        }
    }
    let course_ids = course_ids
        .into_iter()
        .map(parse_id::<LearningCourseId>)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .due_reviews(&user.id, query.limit, &course_ids, query.due_only, query.orphan, &tags)
            .await?,
    )))
}

async fn checkin_today(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<CheckinStatus>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.checkin_today(&user.id).await?,
    )))
}

/// XP 读数（j05）：今日 XP/日目标/达标 + 进度尺（结算数/总数/剩余 est/ETA）。
/// 两把尺同源回放互不换算，ETA 纯投影绝不回写。
async fn xp_reading(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<XpReading>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.xp_reading(&user.id).await?,
    )))
}

#[derive(Debug, Deserialize)]
struct XpCalendarQuery {
    /// 窗口周数（缺省 12，夹取 1..=53）；恒 weeks×7 桶、末桶 = 今日。
    weeks: Option<i64>,
}

/// XP 日历（j05）：恒 weeks×7 桶含零桶，负账如实出桶，不过课程筛。
async fn xp_calendar(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<XpCalendarQuery>,
) -> Result<Json<ApiResponse<XpCalendar>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state
            .service
            .xp_calendar(&user.id, query.weeks.unwrap_or(12))
            .await?,
    )))
}

/// quiz 触发进度读数（q1，读数面非门禁）：每课程 N/K。
async fn quiz_progress(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<QuizProgressView>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.quiz_progress(&user.id, &id).await?,
    )))
}

/// quiz 开测命令（q2 载体落位）：due 门（Conflict 附 N/K）→ 选题 → 回合
/// 行 → 脱敏题单（判底零字段）。
async fn quiz_start(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<QuizRoundView>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.quiz_start(&user.id, &id).await?,
    )))
}

#[derive(Debug, Deserialize)]
struct QuizGradeRequest {
    /// 题 ULID（须在回合计划内）。
    activity_id: String,
    /// 学习者作答（形状随题型，与课时作答同约定）。
    response: Value,
}

/// 逐题判分（q2，纯读零落账）：判底（answer/explanation）只随本反馈下发。
async fn quiz_grade(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path((id, round_id)): Path<(String, String)>,
    Json(request): Json<QuizGradeRequest>,
) -> Result<Json<ApiResponse<QuizGradeFeedback>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .quiz_grade(&user.id, &id, &round_id, &request.activity_id, &request.response)
            .await?,
    )))
}

#[derive(Debug, Deserialize)]
struct QuizCompleteRequest {
    /// 全卷作答：{activity_id: response}（quiz 必作答，缺题拒绝）。
    answers: HashMap<String, Value>,
}

/// 全卷结算（q2）：逐覆盖节点 settle_quiz 分级落账（同日折叠不计薪）+
/// 回合行首写为终。
async fn quiz_complete(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path((id, round_id)): Path<(String, String)>,
    Json(request): Json<QuizCompleteRequest>,
) -> Result<Json<ApiResponse<QuizRoundReport>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .quiz_complete(&user.id, &id, &round_id, &request.answers)
            .await?,
    )))
}

/// quiz 校准读面（q3，读数面非门禁）：该用户全部 completed 回合的校准点
/// 按预测 R 五百分点分桶聚合实际正确率（与记忆健康的复习侧校准同构）。
async fn quiz_calibration(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<MemoryCalibrationBin>>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.quiz_calibration(&user.id).await?,
    )))
}

#[derive(Debug, Deserialize)]
struct CalendarStatsQuery {
    /// Minutes east of UTC (same sign as `SchedulerSettings::tz_offset_minutes`),
    /// reported by the frontend as `-Date().getTimezoneOffset()`.
    tz_offset: i32,
    year: i64,
    /// 1..=12; absent = year view (heatmap).
    month: Option<u32>,
}

async fn calendar_stats(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<CalendarStatsQuery>,
) -> Result<Json<ApiResponse<CalendarStats>>, AppError> {
    if !(-24 * 60..=24 * 60).contains(&query.tz_offset) {
        return Err(AppError::BadRequest(format!(
            "tz_offset out of range: {}",
            query.tz_offset
        )));
    }
    if !(1900..=2999).contains(&query.year) {
        return Err(AppError::BadRequest(format!(
            "year out of range: {}",
            query.year
        )));
    }
    if let Some(month) = query.month {
        if !(1..=12).contains(&month) {
            return Err(AppError::BadRequest(format!("month out of range: {month}")));
        }
    }
    Ok(Json(ApiResponse::ok(
        state
            .service
            .calendar_stats(&user.id, query.tz_offset, query.year, query.month)
            .await?,
    )))
}

#[derive(Debug, Deserialize)]
struct MemoryStatsQuery {
    /// Minutes east of UTC (same sign as `SchedulerSettings::tz_offset_minutes`),
    /// reported by the frontend as `-Date().getTimezoneOffset()`.
    tz_offset: i32,
}

async fn memory_stats(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<MemoryStatsQuery>,
) -> Result<Json<ApiResponse<crate::models::MemoryHealthStats>>, AppError> {
    if !(-24 * 60..=24 * 60).contains(&query.tz_offset) {
        return Err(AppError::BadRequest(format!(
            "tz_offset out of range: {}",
            query.tz_offset
        )));
    }
    Ok(Json(ApiResponse::ok(
        state
            .service
            .memory_health_stats(&user.id, query.tz_offset)
            .await?,
    )))
}

async fn rate_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<RateReviewRequest>,
) -> Result<Json<ApiResponse<crate::models::ReviewResult>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .rate_review(&id, &user.id, request.rating)
            .await?,
    )))
}

async fn answer_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<AnswerReviewRequest>,
) -> Result<Json<ApiResponse<crate::models::ReviewAnswerResult>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .answer_review(
                &id,
                &user.id,
                request.response,
                request.forgot,
                request.elapsed_ms,
            )
            .await?,
    )))
}

async fn skip_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::ReviewResult>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.skip_review(&id, &user.id).await?,
    )))
}

async fn delete_course(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    state.service.delete_course(&id, &user.id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn rename_course(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<crate::models::RenameCourseRequest>,
) -> Result<Json<ApiResponse<crate::models::CourseSummary>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    state.service.rename_course(&id, &request.title).await?;
    Ok(Json(ApiResponse::ok(
        state.service.course_detail(&id, None).await?.course,
    )))
}

async fn archive_course(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::CourseSummary>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    state.service.set_course_archived(&id, true).await?;
    Ok(Json(ApiResponse::ok(
        state.service.course_detail(&id, None).await?.course,
    )))
}

async fn restore_course(
    State(state): State<LearningRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::CourseSummary>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    state.service.set_course_archived(&id, false).await?;
    Ok(Json(ApiResponse::ok(
        state.service.course_detail(&id, None).await?.course,
    )))
}

#[derive(Debug, Deserialize)]
struct QuestionListQuery {
    course_id: Option<String>,
    state: Option<String>,
    search: Option<String>,
}

async fn list_questions(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<QuestionListQuery>,
) -> Result<Json<ApiResponse<Vec<crate::models::QuestionEntry>>>, AppError> {
    let course_id = match query.course_id {
        Some(value) => Some(parse_id::<LearningCourseId>(value)?),
        None => None,
    };
    Ok(Json(ApiResponse::ok(
        state
            .service
            .question_entries(
                &user.id,
                course_id.as_ref(),
                query.state.as_deref(),
                query.search.as_deref(),
            )
            .await?,
    )))
}

async fn update_question(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<UpdateQuestionRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningActivityId>(id)?;
    state
        .service
        .update_question(&id, &user.id, request)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn delete_review_item(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state.service.delete_review_item(&id, &user.id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn archive_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state.service.archive_review_item(&id, &user.id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn unarchive_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state.service.unarchive_review_item(&id, &user.id).await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn mark_review_edit(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<crate::models::MarkEditRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state
        .service
        .mark_review_edit_pending(&id, &user.id, request.note)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn review_question(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::QuestionEntry>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.review_question_entry(&id, &user.id).await?,
    )))
}

async fn list_tags(
    State(state): State<LearningRouterState>,
) -> Result<Json<ApiResponse<Vec<String>>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.list_tags().await?)))
}

async fn set_course_tags(
    State(state): State<LearningRouterState>,
    Path(id): Path<String>,
    Json(request): Json<SetTagsRequest>,
) -> Result<Json<ApiResponse<Vec<String>>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    Ok(Json(ApiResponse::ok(
        state.service.set_course_tags(&id, request).await?,
    )))
}

async fn set_question_tags(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<SetTagsRequest>,
) -> Result<Json<ApiResponse<Vec<String>>>, AppError> {
    let id = parse_id::<LearningActivityId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .set_question_tags(&id, &user.id, request.tags)
            .await?,
    )))
}

async fn set_custom_question_tags(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<SetTagsRequest>,
) -> Result<Json<ApiResponse<Vec<String>>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .set_custom_question_tags(id.as_str(), &user.id, request.tags)
            .await?,
    )))
}

async fn create_custom_question(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Json(request): Json<CreateCustomQuestionRequest>,
) -> Result<Json<ApiResponse<String>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state
            .service
            .create_custom_question(&user.id, request)
            .await?,
    )))
}

async fn update_custom_question(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<UpdateQuestionRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state
        .service
        .update_custom_question(id.as_str(), &user.id, request)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn delete_custom_question(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state
        .service
        .delete_custom_question(id.as_str(), &user.id)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn archive_custom(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state
        .service
        .archive_custom_question(id.as_str(), &user.id)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn unarchive_custom(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state
        .service
        .unarchive_custom_question(id.as_str(), &user.id)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn mark_custom_edit(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<crate::models::MarkEditRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    state
        .service
        .mark_custom_edit_pending(id.as_str(), &user.id, request.note)
        .await?;
    Ok(Json(ApiResponse::ok(())))
}

async fn custom_question(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::QuestionEntry>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .custom_question_entry(id.as_str(), &user.id)
            .await?,
    )))
}

async fn answer_custom_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<AnswerReviewRequest>,
) -> Result<Json<ApiResponse<crate::models::ReviewAnswerResult>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .answer_custom_review(id.as_str(), &user.id, request.response, request.forgot)
            .await?,
    )))
}

async fn rate_custom_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<RateReviewRequest>,
) -> Result<Json<ApiResponse<crate::models::ReviewResult>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .rate_custom_review(id.as_str(), &user.id, request.rating)
            .await?,
    )))
}

async fn skip_custom_review(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<crate::models::ReviewResult>>, AppError> {
    let id = parse_id::<LearningReviewItemId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .skip_custom_review(id.as_str(), &user.id)
            .await?,
    )))
}

fn parse_id<T>(value: String) -> Result<T, AppError>
where
    T: FromStr<Err = UuidV7Error>,
{
    value
        .parse::<T>()
        .map_err(|error| AppError::BadRequest(error.to_string()))
}
