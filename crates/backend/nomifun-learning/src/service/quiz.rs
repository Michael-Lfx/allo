//! quiz 回合载体（q2，方案 A 瘦回合制——mount/repair 回合先例，无会话实体；
//! g4 会话命令面后置，载体票不依赖它）。
//!
//! # 三句真相声明（违反即缺陷）
//!
//! 1. **判底只住题库**：`learning_activities.config_json` 是 answer/
//!    explanation 的唯一居所（服务端域持答案纪律）；回合表 plan_json 只存
//!    选题快照（activity + kind 审计面），start 投影判底零字段，判底只随
//!    [`quiz_grade`] 的反馈下发（判分已定，揭示不破红线）。
//! 2. **首写为终**：completed 是单向转移（`UPDATE … WHERE status =
//!    'active'` 兜并发——后到者的结算被 XP 账本 anchor 吸收，回合行不被
//!    折叠后事实改写）；被新回合取代的 active 回合标 abandoned（learnhub
//!    内存会话簿「重启即弃」的落表显式化）。
//! 3. **同日折叠**（learnhub `quiz_settled_on_day` 语义）：同课程同节点同
//!    学习日只结算一笔（anchor = `quiz:{course}:{node}:{day}`）——coverage
//!    保持、earned 空，不重复计薪；判据 = 账本 anchor 存在性。
//!
//! # 链路
//!
//! `quiz_start`（due 门 + 选题落回合）→ `quiz_grade`（逐题确定性判分
//! [`progress::evaluate`]，零 LLM 零落账）→ `quiz_complete`（逐覆盖节点
//! [`Settlement::settle_quiz`] → 一笔 `quiz-completed` 落
//! `learning_xp_events` → 回合行落对账与总额）。
//!
//! # 选题（q2 最小面；弱先排序与变式归 q3）
//!
//! 覆盖节点 = 触发时刻已结算但未 quiz 的节点（N ≥ K，追加序取**最近的**
//! `QUIZ_QUESTION_BUDGET` 个——触发量的是新学内容的自动性债务，最新节点
//! 巩固最浅）；每节点 1 道该节点 objective 题（LRU = 最久未出：作答流水
//! attempts ∪ 复习作答事件的最近时点，从未出过最先，activity_id 破平）；
//! 无题可选的节点跳过（如实缩减覆盖面）。

use std::collections::HashMap;

use super::progress::evaluate;
use super::*;

use crate::models::{
    QuizGradeFeedback, QuizRoundNodeReport, QuizRoundQuestionView, QuizRoundReport, QuizRoundView,
};
use crate::settle::{Settlement, QUIZ_QUESTION_BUDGET, QUIZ_TRIGGER_SETTLEMENTS};

/// quiz 结算的客观题白名单（与教新线 `settle_node_completed` 同筛：AI 批改
/// 轨不入 quiz——确定性判分是测量面，AI 批改轨随 q3 裁决）。
const QUIZ_OBJECTIVE_KINDS: &str = "('single_choice', 'true_false', 'fill_in_blank', \
     'multi_choice', 'numeric', 'ordering', 'matching')";

/// 选题快照行（plan_json 元素；kind 是审计面快照，判分与定价都以
/// `learning_activities` 现行为准——单源）。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct QuizPlanItem {
    node: String,
    node_name: String,
    activity: String,
    kind: String,
}

impl LearningService {
    /// 开测（q2 载体落位，替换 q1 的 501 占位）：due 门（N ≥ K）→ 选题 →
    /// 既有 active 回合 supersede → 落回合行 → 脱敏投影。
    pub async fn quiz_start(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
    ) -> Result<QuizRoundView, AppError> {
        let progress = self.quiz_progress(user_id, course_id).await?;
        if !progress.due {
            return Err(AppError::Conflict(format!(
                "quiz not due: {}/{} settlements since last quiz",
                progress.settled_count, progress.trigger_count
            )));
        }
        let now = now_ms();
        let nodes = self.quiz_settled_nodes(user_id, course_id.as_str()).await?;
        // 预算 clamp：取最近的 budget 个（新学内容的自动性债务优先；q3 换
        // 弱先全序时此处同步重裁）。
        let selected: Vec<String> = if nodes.len() > QUIZ_QUESTION_BUDGET {
            nodes[nodes.len() - QUIZ_QUESTION_BUDGET..].to_vec()
        } else {
            nodes
        };
        let enrollment_id: Option<String> = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments \
             WHERE user_id = ? AND course_id = ?",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;

        // 选题：每覆盖节点 1 道 objective 题（LRU），无题节点跳过。
        let mut plan: Vec<QuizPlanItem> = Vec::new();
        let mut questions: Vec<QuizRoundQuestionView> = Vec::new();
        for node in &selected {
            let rows = sqlx::query(&format!(
                "SELECT a.activity_id, a.kind, a.prompt, a.config_json, l.title AS node_title, \
                        (SELECT MAX(at.created_at) FROM learning_attempts at \
                          WHERE at.activity_id = a.activity_id \
                            AND at.enrollment_id = ?) AS last_attempt, \
                        (SELECT MAX(e.created_at) FROM learning_review_events e \
                          JOIN learning_review_items i ON i.review_item_id = e.item_id \
                         WHERE i.activity_id = a.activity_id AND e.user_id = ?) AS last_reviewed \
                 FROM learning_activities a \
                 JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                 WHERE a.lesson_id = ? AND a.kind IN {QUIZ_OBJECTIVE_KINDS}"
            ))
            .bind(enrollment_id.as_deref().unwrap_or_default())
            .bind(user_id.as_str())
            .bind(node)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            // LRU：从未出过（两流皆空）最先；其后越久未出越前；全平 =
            // activity_id 序（确定性破平）。Option 的自然序恰合：None <
            // Some(旧) < Some(新)。
            let row = rows.iter().min_by(|a, b| {
                let shown = |r: &sqlx::sqlite::SqliteRow| -> Option<i64> {
                    let attempt: Option<i64> = r.try_get("last_attempt").unwrap_or(None);
                    let reviewed: Option<i64> = r.try_get("last_reviewed").unwrap_or(None);
                    attempt.or(reviewed)
                };
                shown(a)
                    .cmp(&shown(b))
                    .then_with(|| {
                        a.try_get::<String, _>("activity_id")
                            .unwrap_or_default()
                            .cmp(&b.try_get::<String, _>("activity_id").unwrap_or_default())
                    })
            });
            let Some(row) = row else {
                // 无题可选的节点跳过（如实缩减覆盖面）。
                continue;
            };
            let activity_id: String = row.try_get("activity_id").map_err(internal)?;
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            let kind = ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?;
            let prompt: String = row.try_get("prompt").map_err(internal)?;
            let node_name: String = row.try_get("node_title").map_err(internal)?;
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(|error| AppError::Internal(format!("activity config: {error}")))?;
            plan.push(QuizPlanItem {
                node: node.clone(),
                node_name: node_name.clone(),
                activity: activity_id.clone(),
                kind: kind.as_str().to_owned(),
            });
            questions.push(QuizRoundQuestionView {
                activity_id: parse_id(activity_id.clone())?,
                kind,
                prompt,
                options: config.options,
                matches: config.matches,
                node_id: parse_id(node.clone())?,
                node_name,
            });
        }
        if plan.is_empty() {
            return Err(AppError::Conflict(
                "quiz due but no covered node has objective questions".into(),
            ));
        }

        // supersede：既有 active 回合标 abandoned（重开 = 新回合；被取代
        // 回合的作答/结算零痕迹是合法态——learnhub「重启即弃」同义）。
        sqlx::query(
            "UPDATE learning_quiz_rounds SET status = 'abandoned', finished_at = ? \
             WHERE user_id = ? AND course_id = ? AND status = 'active'",
        )
        .bind(now)
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        let round_id = generate_id();
        let plan_json = serde_json::to_string(&plan)
            .map_err(|error| AppError::Internal(format!("quiz plan: {error}")))?;
        let inserted = sqlx::query(
            "INSERT INTO learning_quiz_rounds \
             (round_id, user_id, course_id, started_at, status, plan_json) \
             VALUES (?, ?, ?, ?, 'active', ?)",
        )
        .bind(&round_id)
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .bind(now)
        .bind(&plan_json)
        .execute(&self.pool)
        .await;
        if let Err(error) = inserted {
            // 部分唯一索引 idx_learning_quiz_rounds_active：并发 start 竞速
            // 落败方如实 Conflict（赢方回合在场）。
            if error
                .as_database_error()
                .is_some_and(|e| e.is_unique_violation())
            {
                return Err(AppError::Conflict(format!(
                    "an active quiz round already exists for course {course_id}"
                )));
            }
            return Err(internal(error));
        }
        Ok(QuizRoundView {
            round_id,
            course_id: course_id.clone(),
            status: "active".into(),
            settled_count: progress.settled_count,
            trigger_count: QUIZ_TRIGGER_SETTLEMENTS,
            budget: QUIZ_QUESTION_BUDGET,
            questions,
        })
    }

    /// 逐题判分（纯读零落账；结算在 complete 全卷面）：题必须在回合计划内，
    /// 回合必须 active；判底随反馈下发（判分已定）。
    pub async fn quiz_grade(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        round_id: &str,
        activity_id: &str,
        response: &Value,
    ) -> Result<QuizGradeFeedback, AppError> {
        let plan = self.load_round_plan(user_id, course_id, round_id).await?;
        if !plan.iter().any(|item| item.activity == activity_id) {
            return Err(AppError::BadRequest(format!(
                "activity {activity_id} is not part of quiz round {round_id}"
            )));
        }
        let row = sqlx::query(
            "SELECT kind, config_json FROM learning_activities WHERE activity_id = ?",
        )
        .bind(activity_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning activity {activity_id}")))?;
        let kind_text: String = row.try_get("kind").map_err(internal)?;
        let kind = ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?;
        let config: StoredActivityConfig = serde_json::from_str(
            &row.try_get::<String, _>("config_json").map_err(internal)?,
        )
        .map_err(|error| AppError::Internal(format!("activity config: {error}")))?;
        let (score, explanation) = evaluate(kind, &config, response)?;
        Ok(QuizGradeFeedback {
            activity_id: parse_id(activity_id.to_owned())?,
            correct: score >= 1.0,
            answer: config.answer,
            explanation,
        })
    }

    /// 全卷结算（q2 收口）：逐覆盖节点 `settle_quiz` → 一笔
    /// `quiz-completed` 落 `learning_xp_events`（anchor 幂等 + 同日折叠）→
    /// 回合行落对账与总额（首写为终）。作答必满（quiz 必作答，测量语义）。
    pub async fn quiz_complete(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        round_id: &str,
        answers: &HashMap<String, Value>,
    ) -> Result<QuizRoundReport, AppError> {
        let now = now_ms();
        let (status, plan_json) = self.load_round_row(user_id, course_id, round_id).await?;
        if status != "active" {
            return Err(AppError::Conflict(format!(
                "quiz round {round_id} is already {status} (first write wins)"
            )));
        }
        let plan: Vec<QuizPlanItem> = serde_json::from_str(&plan_json)
            .map_err(|error| AppError::Internal(format!("quiz plan: {error}")))?;
        for item in &plan {
            if !answers.contains_key(&item.activity) {
                return Err(AppError::BadRequest(format!(
                    "quiz requires an answer for activity {} (measurement: all planned questions are answered)",
                    item.activity
                )));
            }
        }

        // 逐题判分（现行为准：判分与定价同一 kind 源），按节点聚合（保计划序）。
        struct NodeAgg {
            node_name: String,
            kinds: Vec<ActivityKind>,
            correct: i64,
            answered: i64,
        }
        let mut order: Vec<String> = Vec::new();
        let mut by_node: HashMap<String, NodeAgg> = HashMap::new();
        for item in &plan {
            let row = sqlx::query(
                "SELECT kind, config_json FROM learning_activities WHERE activity_id = ?",
            )
            .bind(&item.activity)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?
            .ok_or_else(|| AppError::NotFound(format!("learning activity {}", item.activity)))?;
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            let kind = ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?;
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(|error| AppError::Internal(format!("activity config: {error}")))?;
            let (score, _) = evaluate(kind, &config, &answers[&item.activity])?;
            let agg = by_node.entry(item.node.clone()).or_insert_with(|| {
                order.push(item.node.clone());
                NodeAgg {
                    node_name: item.node_name.clone(),
                    kinds: Vec::new(),
                    correct: 0,
                    answered: 0,
                }
            });
            agg.kinds.push(kind);
            agg.answered += 1;
            agg.correct += i64::from(score >= 1.0);
        }

        // 逐覆盖节点结算：同日折叠（anchor 在场 = coverage 保持不重复计薪）。
        let tz_offset_minutes = self.tz_offset_minutes().await;
        let day = review_day_number(now, tz_offset_minutes);
        let mut covered = Vec::new();
        let mut earned_total = 0i64;
        for node in &order {
            let agg = &by_node[node];
            let anchor = format!("quiz:{}:{node}:{day}", course_id.as_str());
            let settled: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM learning_xp_events WHERE user_id = ? AND anchor = ?",
            )
            .bind(user_id.as_str())
            .bind(&anchor)
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?;
            let earned_xp = if settled > 0 {
                None
            } else {
                let settlement = Settlement::settle_quiz(&agg.kinds, agg.correct, agg.answered)
                    .map_err(|message| AppError::Internal(format!("quiz settlement: {message}")))?;
                self.record_settlement(
                    user_id,
                    course_id.as_str(),
                    Some(node),
                    "quiz-completed",
                    &settlement,
                    "quiz",
                    &anchor,
                    now,
                )
                .await?;
                Some(settlement.earned_xp)
            };
            earned_total += earned_xp.unwrap_or(0);
            covered.push(QuizRoundNodeReport {
                node_id: parse_id(node.clone())?,
                node_name: agg.node_name.clone(),
                answered: agg.answered,
                correct: agg.correct,
                earned_xp,
            });
        }

        // 首写为终：只有 active → completed 的单向转移；并发落败方拿
        // Conflict（其结算已被 anchor 吸收，账本仍一致）。
        let report_json = serde_json::to_string(&covered)
            .map_err(|error| AppError::Internal(format!("quiz report: {error}")))?;
        let answers_json = serde_json::to_string(answers)
            .map_err(|error| AppError::Internal(format!("quiz answers: {error}")))?;
        let result = sqlx::query(
            "UPDATE learning_quiz_rounds SET status = 'completed', finished_at = ?, \
             answers_json = ?, report_json = ?, earned_xp = ? \
             WHERE round_id = ? AND status = 'active'",
        )
        .bind(now)
        .bind(&answers_json)
        .bind(&report_json)
        .bind(earned_total)
        .bind(round_id)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 0 {
            return Err(AppError::Conflict(format!(
                "quiz round {round_id} is already completed (first write wins)"
            )));
        }
        Ok(QuizRoundReport {
            round_id: round_id.to_owned(),
            course_id: course_id.clone(),
            earned_xp: earned_total,
            covered,
        })
    }

    /// 读回合计划（grade 用；回合必须 active——终态回合不再判分）。
    async fn load_round_plan(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        round_id: &str,
    ) -> Result<Vec<QuizPlanItem>, AppError> {
        let (status, plan_json) = self.load_round_row(user_id, course_id, round_id).await?;
        if status != "active" {
            return Err(AppError::Conflict(format!(
                "quiz round {round_id} is {status}; grading needs an active round"
            )));
        }
        serde_json::from_str(&plan_json)
            .map_err(|error| AppError::Internal(format!("quiz plan: {error}")))
    }

    /// 读回合行（用户×课程×回合三键；不属于该用户/课程 = NotFound）。
    async fn load_round_row(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        round_id: &str,
    ) -> Result<(String, String), AppError> {
        let row = sqlx::query(
            "SELECT status, plan_json FROM learning_quiz_rounds \
             WHERE round_id = ? AND user_id = ? AND course_id = ?",
        )
        .bind(round_id)
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("quiz round {round_id}")))?;
        let status: String = row.try_get("status").map_err(internal)?;
        let plan_json: String = row.try_get("plan_json").map_err(internal)?;
        Ok((status, plan_json))
    }
}
