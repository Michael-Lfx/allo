use super::*;

use nomifun_common::LearningSystemActionId;

/// 读侧默认截断（GET /courses/{id}/system-actions?limit=）。
pub(crate) const SYSTEM_ACTION_DEFAULT_LIMIT: i64 = 50;
/// 读侧上限（超过按上限截断，不报错——读侧宽容）。
pub(crate) const SYSTEM_ACTION_MAX_LIMIT: i64 = 200;

/// limit 归一：缺省 50、下限 1、上限 200。坏值（0/负数/超大）不是错误，
/// 按边界截断——账本读是旁路面，不因参数离谱拒绝。
pub(crate) fn system_action_limit(raw: Option<i64>) -> i64 {
    raw.unwrap_or(SYSTEM_ACTION_DEFAULT_LIMIT)
        .clamp(1, SYSTEM_ACTION_MAX_LIMIT)
}

/// 一次动作的落账输入（终态单行）。调用点决定 kind/trigger/result 与依据
/// 读数；`record_system_action` 负责身份、截断与 fail-open。
#[derive(Debug, Clone)]
pub(crate) struct NewSystemAction {
    pub course_id: LearningCourseId,
    pub kind: SystemActionKind,
    pub trigger: SystemActionTrigger,
    pub basis: String,
    pub result: SystemActionResult,
    pub reason: String,
    pub actor_user_id: Option<UserId>,
}

impl NewSystemAction {
    pub(crate) fn new(
        course_id: LearningCourseId,
        kind: SystemActionKind,
        trigger: SystemActionTrigger,
        result: SystemActionResult,
    ) -> Self {
        Self {
            course_id,
            kind,
            trigger,
            basis: String::new(),
            result,
            reason: String::new(),
            actor_user_id: None,
        }
    }

    /// 触发时的一句判据读数（写侧截断 ≤200 字）。
    pub(crate) fn basis(mut self, basis: impl Into<String>) -> Self {
        self.basis = basis.into();
        self
    }

    /// failed 原因（done 保持空串）。
    pub(crate) fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    /// 手动动作的操作者（自动动作不调用，列保持 NULL）。
    pub(crate) fn actor(mut self, user_id: &UserId) -> Self {
        self.actor_user_id = Some(user_id.clone());
        self
    }
}

impl LearningService {
    /// 落一条系统动作终态账。**fail-open**：任何写失败只 `warn!`，不返回
    /// 错误、不阻断调用方——账本是旁路，维护/生长永远不为账本让步。
    pub(crate) async fn record_system_action(&self, action: NewSystemAction) {
        let action_id = LearningSystemActionId::new().into_string();
        let basis = truncate_action_text(&action.basis, 200);
        let inserted = sqlx::query(
            "INSERT INTO learning_system_actions \
             (action_id, course_id, kind, trigger_source, basis, result, reason, \
              actor_user_id, at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&action_id)
        .bind(action.course_id.as_str())
        .bind(action.kind.as_str())
        .bind(action.trigger.as_str())
        .bind(&basis)
        .bind(action.result.as_str())
        .bind(&action.reason)
        .bind(action.actor_user_id.as_ref().map(UserId::as_str))
        .bind(now_ms())
        .execute(&self.pool)
        .await;
        if let Err(error) = inserted {
            tracing::warn!(
                course = action.course_id.as_str(),
                kind = action.kind.as_str(),
                trigger = action.trigger.as_str(),
                %error,
                "learning system action ledger write failed (fail-open)"
            );
        }
    }

    /// 课程系统动作读侧：按 `at DESC`（同毫秒按行 id 倒序）截断 limit。
    /// 课程不存在/软删 404；归档课仍可读（历史可见，不设 enabled 门）。
    pub async fn system_actions(
        &self,
        course_id: &LearningCourseId,
        limit: i64,
    ) -> Result<Vec<crate::models::SystemActionView>, AppError> {
        let deleted_at: Option<Option<i64>> =
            sqlx::query_scalar("SELECT deleted_at FROM learning_courses WHERE course_id = ?")
                .bind(course_id.as_str())
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        match deleted_at {
            Some(None) => {}
            Some(Some(_)) | None => {
                return Err(AppError::NotFound(format!("course {course_id}")));
            }
        }
        let limit = system_action_limit(Some(limit));
        let rows = sqlx::query(
            "SELECT action_id, kind, trigger_source, basis, result, reason, \
                    actor_user_id, at \
             FROM learning_system_actions WHERE course_id = ? \
             ORDER BY at DESC, id DESC LIMIT ?",
        )
        .bind(course_id.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let actions: Vec<crate::models::SystemActionView> = rows
            .iter()
            .map(|row| {
                Ok(crate::models::SystemActionView {
                    action_id: row.try_get("action_id").map_err(internal)?,
                    kind: row.try_get("kind").map_err(internal)?,
                    trigger: row.try_get("trigger_source").map_err(internal)?,
                    basis: row.try_get("basis").map_err(internal)?,
                    result: row.try_get("result").map_err(internal)?,
                    reason: row.try_get("reason").map_err(internal)?,
                    actor_user_id: row.try_get("actor_user_id").map_err(internal)?,
                    at: row.try_get("at").map_err(internal)?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        tracing::info!(
            course = course_id.as_str(),
            limit,
            returned = actions.len(),
            "learning system actions listed"
        );
        Ok(actions)
    }
}

/// basis 截断：≤max_chars 字（超出取前 max_chars-1 加省略号，仍是 ≤max）。
fn truncate_action_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_truncation_stays_within_the_char_budget() {
        assert_eq!(truncate_action_text("短判据", 200), "短判据");
        let long = "读".repeat(250);
        let truncated = truncate_action_text(&long, 200);
        assert_eq!(truncated.chars().count(), 200);
        assert!(truncated.ends_with('…'));
    }
}
