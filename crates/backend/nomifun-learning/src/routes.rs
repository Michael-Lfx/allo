use std::str::FromStr;

use axum::extract::{Extension, Json, Path, Query, RawQuery, State};
use axum::routing::{delete, get, post, put};
use axum::Router;
use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::{
    AppError, LearningActivityId, LearningCourseId, LearningLessonId, LearningReviewItemId,
    UuidV7Error,
};
use serde::Deserialize;
use url::form_urlencoded;

use crate::models::{
    AnswerReviewRequest, CalendarStats, CheckinStatus, CoursePack,
    CreateCustomQuestionRequest, CreateLessonActivityRequest, DeleteCourseRequest,
    GenerateCourseRequest, GenerateLessonActivityRequest, GenerateLessonRequest,
    LearningGraphGenerationStatus, RateReviewRequest, RepairFigureRequest,
    RepairFigureResponse, ResumeLearningGraphRequest, SetTagsRequest, SubmitAttemptRequest,
    UpdateLessonProgressRequest, UpdateQuestionRequest,
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
            "/api/learning/courses/generate/resume",
            post(resume_learning_graph),
        )
        .route(
            "/api/learning/courses/generate/status",
            get(learning_graph_generation_status),
        )
        .route(
            "/api/learning/courses/generate/cancel",
            post(cancel_learning_graph_generation),
        )
        .route("/api/learning/courses/{id}", get(get_course))
        .route("/api/learning/courses/{id}", delete(delete_course))
        .route("/api/learning/courses/{id}/tags", put(set_course_tags))
        .route("/api/learning/courses/{id}/enroll", post(enroll))
        .route(
            "/api/learning/courses/{id}/diagnostic",
            get(diagnostic_plan),
        )
        .route(
            "/api/learning/lessons/{id}/progress",
            post(update_lesson_progress),
        )
        .route(
            "/api/learning/lessons/{id}/generate",
            post(generate_lesson),
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
        .route("/api/learning/stats/calendar", get(calendar_stats))
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
        .route("/api/learning/concepts", get(list_concept_refs))
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

async fn resume_learning_graph(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Json(request): Json<ResumeLearningGraphRequest>,
) -> Result<Json<ApiResponse<crate::models::CourseDetail>>, AppError> {
    // 续建失败的学习图生成:与 generate_course 同一套同步执行契约——请求
    // 中断即终止循环,过程事件经 WS 推送,终态以本响应为准。无存活草稿时
    // 返回 NotFound,前端回退全量重生成。
    Ok(Json(ApiResponse::ok(
        state
            .service
            .resume_learning_graph_course(&user.id, request.provider_id, request.model)
            .await?,
    )))
}

/// 课程生成状态（学习图与大纲流共用）：后台指示条的数据源。生成在 HTTP
/// 请求内同步执行，但创建对话框可以随时关闭——注册表让运行对外可发现
/// （主题 + 已运行时长）。
async fn learning_graph_generation_status(
    State(state): State<LearningRouterState>,
) -> Result<Json<ApiResponse<LearningGraphGenerationStatus>>, AppError> {
    Ok(Json(ApiResponse::ok(state.service.generation_status())))
}

/// 取消进行中的课程生成：置位旗标，循环在下一个 LLM 请求边界停止（取消
/// 不保留草稿，重试即全新生成）。无进行中的生成时返回 cancelled=false
/// （幂等，前端不必区分竞态）。
async fn cancel_learning_graph_generation(
    State(state): State<LearningRouterState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let cancelled = state.service.cancel_generation();
    Ok(Json(ApiResponse::ok(serde_json::json!({
        "cancelled": cancelled,
    }))))
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

async fn generate_lesson(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Json(request): Json<GenerateLessonRequest>,
) -> Result<Json<ApiResponse<crate::models::LessonView>>, AppError> {
    let id = parse_id::<LearningLessonId>(id)?;
    Ok(Json(ApiResponse::ok(
        state
            .service
            .generate_lesson_content(&user.id, &id, &request)
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
            .answer_review(&id, &user.id, request.response, request.forgot)
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
    Json(request): Json<DeleteCourseRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let id = parse_id::<LearningCourseId>(id)?;
    state
        .service
        .delete_course(&id, &user.id, request.delete_reviews)
        .await?;
    Ok(Json(ApiResponse::ok(())))
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

async fn list_concept_refs(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<ApiResponse<Vec<crate::models::ConceptRef>>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.concept_refs(&user.id).await?,
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
