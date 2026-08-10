use std::str::FromStr;

use axum::extract::{Extension, Json, Path, Query, State};
use axum::routing::{delete, get, post, put};
use axum::Router;
use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::{
    AppError, LearningActivityId, LearningCourseId, LearningLessonId, LearningReviewItemId,
    UuidV7Error,
};
use serde::Deserialize;

use crate::models::{
    AnswerReviewRequest, CoursePack, CreateCustomQuestionRequest, DeleteCourseRequest,
    GenerateCourseRequest, RateReviewRequest, SetTagsRequest, SubmitAttemptRequest,
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
            "/api/learning/activities/{id}/attempts",
            post(submit_attempt),
        )
        .route("/api/learning/reviews/due", get(due_reviews))
        .route("/api/learning/tags", get(list_tags))
        .route("/api/learning/reviews/{id}/answer", post(answer_review))
        .route("/api/learning/reviews/{id}/rate", post(rate_review))
        .route("/api/learning/reviews/{id}/skip", post(skip_review))
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
            put(update_custom_question).delete(delete_custom_question),
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
    Extension(_user): Extension<CurrentUser>,
    Json(request): Json<GenerateCourseRequest>,
) -> Result<Json<ApiResponse<crate::models::CourseDetail>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.generate_course(request).await?,
    )))
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
            .submit_attempt(&id, &user.id, request.response)
            .await?,
    )))
}

#[derive(Debug, Deserialize)]
struct DueReviewQuery {
    #[serde(default = "default_review_limit")]
    limit: i64,
    course_id: Option<String>,
    /// Keep only reviews whose due time has passed (main review entry).
    #[serde(default)]
    due_only: bool,
    /// Keep only learner-authored questions that belong to no course.
    #[serde(default)]
    orphan: bool,
    /// Keep only items carrying at least one of these tag names.
    #[serde(default)]
    tag: Vec<String>,
}

const fn default_review_limit() -> i64 {
    30
}

async fn due_reviews(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<DueReviewQuery>,
) -> Result<Json<ApiResponse<Vec<crate::models::DueReview>>>, AppError> {
    if query.orphan && query.course_id.is_some() {
        return Err(AppError::BadRequest(
            "orphan filter is mutually exclusive with course_id".into(),
        ));
    }
    let course_id = match query.course_id {
        Some(value) => Some(parse_id::<LearningCourseId>(value)?),
        None => None,
    };
    Ok(Json(ApiResponse::ok(
        state
            .service
            .due_reviews(
                &user.id,
                query.limit,
                course_id.as_ref(),
                query.due_only,
                query.orphan,
                &query.tag,
            )
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
