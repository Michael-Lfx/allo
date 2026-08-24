pub(super) use std::cmp::Ordering;
pub(super) use std::collections::{HashMap, HashSet};
pub(super) use std::path::PathBuf;
pub(super) use std::str::FromStr;
pub(super) use std::sync::{Arc, RwLock};

pub(super) use chrono::Datelike;
pub(super) use nomifun_common::{
    AppError, KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningConceptGraphId,
    LearningConceptId, LearningCourseId, LearningEnrollmentId, LearningLessonId,
    LearningModuleId, LearningReviewItemId, LearningTagId, ProviderId, UserId, UuidV7Error,
    generate_id, now_ms,
};
pub(super) use nomifun_db::SqlitePool;
pub(super) use nomifun_knowledge::{KnowledgeCompleter, KnowledgeService};
pub(super) use serde_json::Value;
pub(super) use sqlx::{Row, Sqlite, Transaction};

pub(super) use crate::generation::{Blueprint, generate_lesson, generate_lesson_activity};
pub(super) use crate::models::{
    ActivityKind, ActivityView, AttemptResult, CalendarCourseRef, CalendarDayStats,
    CalendarLessonRef, CalendarStats, CheckinStatus, ConceptPack, ConceptRef, ConceptView,
    CourseDetail, CourseJobSource, CourseJobStatus, CourseJobView, CoursePack, CourseSummary,
    CreateCustomQuestionRequest, CreateLessonActivityRequest, DiagnosticItem, DiagnosticPlan,
    DueReview, GenerateCourseRequest, GenerateLessonActivityRequest, GenerateLessonRequest,
    GeneratedLessonActivity, LessonStatus, LessonView, ModuleView, QuestionEntry,
    RetryCourseJobRequest, ReviewAnswerResult, ReviewQuestion, ReviewRating, ReviewResult,
    ReviewSource, SetTagsRequest, SourceSpan, StoredActivityConfig, UpdateQuestionRequest,
};
pub(super) use crate::generation_job::GenerationJobRunner;
pub(super) use crate::scheduler::{
    SchedulerSettings, first_review_due_at, review_day_number, review_day_start_utc, schedule_review,
};
#[derive(Clone)]
pub struct LearningService {
    pool: SqlitePool,
    knowledge_service: Arc<RwLock<Option<Arc<KnowledgeService>>>>,
    course_completer: Arc<RwLock<Option<Arc<dyn KnowledgeCompleter>>>>,
    concept_graph_dir: Arc<RwLock<Option<PathBuf>>>,
}

impl LearningService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            knowledge_service: Arc::new(RwLock::new(None)),
            course_completer: Arc::new(RwLock::new(None)),
            concept_graph_dir: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_generation_dependencies(
        &self,
        knowledge_service: Arc<KnowledgeService>,
        completer: Arc<dyn KnowledgeCompleter>,
    ) {
        *self
            .knowledge_service
            .write()
            .expect("learning knowledge service lock poisoned") = Some(knowledge_service);
        *self
            .course_completer
            .write()
            .expect("learning course completer lock poisoned") = Some(completer);
    }

    /// Test-only access to the underlying pool (e.g. to simulate a stale
    /// seeded course version).
    #[cfg(test)]
    pub(crate) fn pool_for_tests(&self) -> &SqlitePool {
        &self.pool
    }

    /// The injected knowledge service, or a conflict when the learning
    /// service was constructed without generation dependencies (used by the
    /// tutorial seed, which needs knowledge-base registration and file IO).
    pub(crate) fn injected_knowledge_service(&self) -> Result<Arc<KnowledgeService>, AppError> {
        self.knowledge_service
            .read()
            .map_err(|_| AppError::Internal("learning knowledge service lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed learning is not configured".into())
            })
    }

    async fn lesson_concepts(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<Vec<LearningConceptId>, AppError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT concept_id FROM learning_lesson_concepts WHERE lesson_id = ? ORDER BY concept_id",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        ids.into_iter().map(parse_id).collect()
    }

    async fn lesson_activities(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<Vec<ActivityView>, AppError> {
        let rows = sqlx::query(
            "SELECT activity_id, kind, prompt, config_json, position FROM learning_activities \
             WHERE lesson_id = ? ORDER BY position, activity_id",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut activities = Vec::with_capacity(rows.len());
        for row in rows {
            let id: LearningActivityId =
                parse_id(row.try_get("activity_id").map_err(internal)?)?;
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(internal)?;
            let concept_ids: Vec<String> = sqlx::query_scalar(
                "SELECT concept_id FROM learning_activity_concepts \
                 WHERE activity_id = ? ORDER BY concept_id",
            )
            .bind(id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            activities.push(ActivityView {
                id,
                kind: ActivityKind::try_from(kind_text.as_str())
                    .map_err(AppError::Internal)?,
                prompt: row.try_get("prompt").map_err(internal)?,
                options: config.options,
                position: row.try_get("position").map_err(internal)?,
                concepts: concept_ids
                    .into_iter()
                    .map(parse_id)
                    .collect::<Result<_, _>>()?,
            });
        }
        Ok(activities)
    }

    async fn course_concepts(
        &self,
        course_id: &LearningCourseId,
        enrollment_id: Option<&LearningEnrollmentId>,
    ) -> Result<Vec<ConceptView>, AppError> {
        let rows = sqlx::query(
            "SELECT c.concept_id, c.concept_key, c.title, c.description, m.mastery \
             FROM learning_concepts c \
             LEFT JOIN learning_mastery_states m \
               ON m.concept_id = c.concept_id AND m.enrollment_id = ? \
             WHERE c.course_id = ? ORDER BY c.concept_key, c.concept_id",
        )
        .bind(enrollment_id.map(LearningEnrollmentId::as_str))
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut concepts = Vec::with_capacity(rows.len());
        for row in rows {
            let id: LearningConceptId =
                parse_id(row.try_get("concept_id").map_err(internal)?)?;
            let prerequisites: Vec<String> = sqlx::query_scalar(
                "SELECT prerequisite_concept_id FROM learning_concept_prerequisites \
                 WHERE concept_id = ? ORDER BY prerequisite_concept_id",
            )
            .bind(id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            concepts.push(ConceptView {
                id,
                key: row.try_get("concept_key").map_err(internal)?,
                title: row.try_get("title").map_err(internal)?,
                description: row.try_get("description").map_err(internal)?,
                prerequisites: prerequisites
                    .into_iter()
                    .map(parse_id)
                    .collect::<Result<_, _>>()?,
                mastery: row.try_get("mastery").map_err(internal)?,
            });
        }
        Ok(concepts)
    }

}

fn parse_id<T>(value: String) -> Result<T, AppError>
where
    T: FromStr<Err = UuidV7Error>,
{
    value
        .parse()
        .map_err(|error| AppError::Internal(format!("invalid persisted ID {value}: {error}")))
}

fn internal(error: impl std::fmt::Display) -> AppError {
    AppError::Internal(error.to_string())
}

mod checkin;
mod concept_graph;
mod course;
mod course_job;
mod diagnostic;
mod lesson;
mod progress;
mod review;
mod tags;

#[cfg(test)]
mod tests;

pub(crate) use self::course::validate_pack;
use self::lesson::validate_question_payload;
use self::progress::{ensure_review_item, evaluate, update_activity_mastery, update_mastery_and_review};
