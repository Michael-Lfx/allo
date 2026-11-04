//! 节点容量反馈信号（y07）：per-user / per-lesson 三选一留痕的读写面。
//!
//! 硬边界（与 lesson/progress 的结算面完全隔离）：本模块只读写
//! `learning_lesson_capacity_feedback`（外加读课时/选课存在性），**不写**
//! attempts / review_events / review_log / xp_events / lesson_progress / FSRS
//! 任何列，不改课时 status，不发 WS phase，不登记生成任务。用户反馈也**不**
//! 注入共享课时生成（课程内容多用户共享，注入即串味）。

use super::*;

use crate::models::{CapacityFeedbackChoice, CapacityFeedbackView};

impl LearningService {
    /// 当前用户对某一课时的最新容量反馈。未知 choice（旧/未来版本写入）按
    /// 无回显处理——读侧宽容（ADR-0015）。
    pub(super) async fn capacity_feedback_for(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
    ) -> Result<Option<CapacityFeedbackView>, AppError> {
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT choice, updated_at FROM learning_lesson_capacity_feedback \
             WHERE user_id = ? AND lesson_id = ?",
        )
        .bind(user_id.as_str())
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        Ok(row.and_then(|(choice, at)| {
            CapacityFeedbackChoice::try_from_str(&choice)
                .map(|choice| CapacityFeedbackView { choice, at })
        }))
    }

    /// 记录一次容量反馈（latest-wins UPSERT）。
    ///
    /// 校验顺序：choice 写侧严格（空/未知 → 400）→ 课时存在 + 用户已选课
    /// （与课时生成入口同一 404 口径）。**不**按需创建 enrollment——反馈是
    /// 旁路留痕，不得顺带写进度/选课面。`feedback_id` 首写生成，覆盖不改。
    pub async fn record_capacity_feedback(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        choice: &str,
    ) -> Result<CapacityFeedbackView, AppError> {
        let choice = CapacityFeedbackChoice::try_from_str(choice.trim()).ok_or_else(|| {
            AppError::BadRequest("choice must be accept, extend or split".into())
        })?;
        let course_id: String = sqlx::query_scalar(
            "SELECT m.course_id FROM learning_modules m \
             JOIN learning_lessons l ON l.module_id = m.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;
        let enrolled: Option<String> = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments WHERE user_id = ? AND course_id = ?",
        )
        .bind(user_id.as_str())
        .bind(&course_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        if enrolled.is_none() {
            return Err(AppError::NotFound(format!("learning lesson {lesson_id}")));
        }

        let now = now_ms();
        sqlx::query(
            "INSERT INTO learning_lesson_capacity_feedback \
             (feedback_id, user_id, lesson_id, choice, note, created_at, updated_at) \
             VALUES (?, ?, ?, ?, '', ?, ?) \
             ON CONFLICT(user_id, lesson_id) DO UPDATE SET \
               choice = excluded.choice, updated_at = excluded.updated_at",
        )
        .bind(generate_id())
        .bind(user_id.as_str())
        .bind(lesson_id.as_str())
        .bind(choice.as_str())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(CapacityFeedbackView { choice, at: now })
    }
}
