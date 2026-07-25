use std::str::FromStr;

use axum::extract::{Extension, Json, Path, Query, State};
use axum::routing::{get, post};
use axum::Router;
use nomifun_api_types::ApiResponse;
use nomifun_auth::CurrentUser;
use nomifun_common::{
    AppError, LearningActivityId, LearningCourseId, LearningLessonId, LearningReviewItemId,
    UuidV7Error,
};
use serde::Deserialize;

use crate::models::{
    CoursePack, GenerateCourseRequest, RateReviewRequest, SubmitAttemptRequest,
    UpdateLessonProgressRequest,
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
        .route("/api/learning/courses/{id}/enroll", post(enroll))
        .route(
            "/api/learning/lessons/{id}/progress",
            post(update_lesson_progress),
        )
        .route(
            "/api/learning/activities/{id}/attempts",
            post(submit_attempt),
        )
        .route("/api/learning/reviews/due", get(due_reviews))
        .route("/api/learning/reviews/{id}/rate", post(rate_review))
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
}

const fn default_review_limit() -> i64 {
    30
}

async fn due_reviews(
    State(state): State<LearningRouterState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<DueReviewQuery>,
) -> Result<Json<ApiResponse<Vec<crate::models::DueReview>>>, AppError> {
    Ok(Json(ApiResponse::ok(
        state.service.due_reviews(&user.id, query.limit).await?,
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

fn parse_id<T>(value: String) -> Result<T, AppError>
where
    T: FromStr<Err = UuidV7Error>,
{
    value
        .parse::<T>()
        .map_err(|error| AppError::BadRequest(error.to_string()))
}
