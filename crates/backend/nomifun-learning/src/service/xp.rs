use super::*;

use crate::settle::{
    validate_settlement, Settlement, DEFAULT_DAILY_XP_GOAL, QUIZ_TRIGGER_SETTLEMENTS,
};

/// companion 桥接缝（j05，二轮 g1「统一账」）：学习结算落账后把净入账同步
/// 授予 companion 账户。实现方（组合根）负责事件 id 幂等去重与 companion
/// 归属解析；**companion 写失败不回滚学习账**——`learning_xp_events` 是学习
/// 线真相，companion 余额允许滞后。缺省（测试/CLI/无伙伴花名册）无桥，只落
/// 学习账。
#[async_trait::async_trait]
pub trait CompanionXpSink: Send + Sync {
    /// 幂等授予一笔已落账结算的净入账；`Ok` = 已受理（含"重复事件忽略"）。
    async fn grant_learning_xp(&self, event_id: &str, delta_xp: i64) -> Result<(), AppError>;
}

impl LearningService {

    /// 注入 companion 桥（组合根接线，接线期一次性）。缺省无桥。
    pub fn set_companion_xp_sink(&self, sink: Arc<dyn CompanionXpSink>) {
        *self
            .companion_xp_sink
            .write()
            .expect("learning companion xp sink lock poisoned") = Some(sink);
    }

    /// 教新线结算（node-completed）：课时（图节点即课时）完成。基数 = est
    /// 分钟（j01 命名：DB 列保留 `estimated_minutes`）；授予阶梯按本课时的
    /// 客观题作答事实（distinct activity 口径：有作答 = answered，有通过
    /// 作答 = passed）。幂等锚 `node:{lesson_id}`（节点一次性事实——重复
    /// 完成不重复结算，重学员走复习线计价）。
    pub(super) async fn settle_node_completed(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        now: i64,
    ) -> Result<(), AppError> {
        let row = sqlx::query(
            "SELECT l.estimated_minutes, m.course_id \
             FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("lesson {lesson_id}")))?;
        let base_est: i64 = row.try_get("estimated_minutes").map_err(internal)?;
        let course_id: String = row.try_get("course_id").map_err(internal)?;
        let enrollment_id: Option<String> = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments \
             WHERE user_id = ? AND course_id = ?",
        )
        .bind(user_id.as_str())
        .bind(&course_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        // 客观题总数只看课时；作答数按报名口径（无报名 = 零作答）。
        let total_questions: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM learning_activities \
             WHERE lesson_id = ? AND kind IN ({OBJECTIVE_KIND_SQL_LIST})"
        ))
        .bind(lesson_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let (answered, passed) = match &enrollment_id {
            Some(enrollment_id) => {
                let answered: i64 = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT activity_id) FROM learning_attempts \
                     WHERE enrollment_id = ? AND activity_id IN \
                       (SELECT activity_id FROM learning_activities WHERE lesson_id = ?)",
                )
                .bind(enrollment_id)
                .bind(lesson_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(internal)?;
                let passed: i64 = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT activity_id) FROM learning_attempts \
                     WHERE enrollment_id = ? AND passed = 1 AND activity_id IN \
                       (SELECT activity_id FROM learning_activities WHERE lesson_id = ?)",
                )
                .bind(enrollment_id)
                .bind(lesson_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(internal)?;
                (answered, passed)
            }
            None => (0, 0),
        };
        let settlement = Settlement::settle_node(base_est, total_questions, answered, passed);
        self.record_settlement(
            user_id,
            &course_id,
            Some(lesson_id.as_str()),
            "node-completed",
            &settlement,
            "lesson-progress",
            &format!("node:{lesson_id}"),
            now,
        )
        .await
    }

    /// 复习线结算（review-completed）：一次真实推进（调度面确实移动）。
    /// 基数 = max(题型当量, REVIEW_MIN_MINUTES)；授予按作答得分（自评 again
    /// = 承认遗忘 0 分）。幂等锚 `review:{course|custom}:{item_id}:{学习日}`
    /// ——同卡同学习日只结算一笔（推送门折叠保证同日只推进一次，锚兜底）。
    pub(super) async fn settle_review_push(
        &self,
        user_id: &UserId,
        source: ReviewSource,
        item_id: &str,
        kind: ActivityKind,
        course_id: Option<&LearningCourseId>,
        node_id: Option<&LearningLessonId>,
        correct: bool,
        now: i64,
        tz_offset_minutes: i32,
    ) -> Result<(), AppError> {
        let settlement = Settlement::settle_review(kind, if correct { 1.0 } else { 0.0 });
        let course_segment = match course_id {
            Some(course_id) => course_id.to_string(),
            None => "custom".to_string(),
        };
        let anchor = format!(
            "review:{course_segment}:{item_id}:{}",
            review_day_number(now, tz_offset_minutes)
        );
        self.record_settlement(
            user_id,
            course_id.map(|id| id.as_str()).unwrap_or_default(),
            node_id.map(|id| id.as_str()),
            "review-completed",
            &settlement,
            match source {
                ReviewSource::Course => "review-course",
                ReviewSource::Custom => "review-custom",
            },
            &anchor,
            now,
        )
        .await
    }

    /// 落账 + 桥的唯一写点：条款先入（违约即错误，绝不写表），INSERT OR
    /// IGNORE 兜幂等锚，净入账非零才过桥（0 值事件只落账不过桥）。桥失败
    /// 只记 warn——学习账不回滚，companion 余额滞后由下次结算自然补偿。
    pub(super) async fn record_settlement(
        &self,
        user_id: &UserId,
        course_id: &str,
        node_id: Option<&str>,
        kind: &str,
        settlement: &Settlement,
        origin: &str,
        anchor: &str,
        now: i64,
    ) -> Result<(), AppError> {
        validate_settlement(kind, settlement)
            .map_err(|message| AppError::Internal(format!("settlement clause: {message}")))?;
        let event_id = generate_id();
        let result = sqlx::query(
            "INSERT OR IGNORE INTO learning_xp_events \
             (event_id, user_id, ts, course_id, node_id, kind, earned_xp, xp_bonus, \
              basis, ratio, origin, anchor) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&event_id)
        .bind(user_id.as_str())
        .bind(now)
        .bind(if course_id.is_empty() { None::<&str> } else { Some(course_id) })
        .bind(node_id)
        .bind(kind)
        .bind(settlement.earned_xp)
        .bind(settlement.xp_bonus)
        .bind(settlement.basis.as_str())
        .bind(settlement.ratio)
        .bind(origin)
        .bind(anchor)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 0 {
            // 幂等锚已存在：重复结算（重复完成/同卡同日重推）静默跳过。
            return Ok(());
        }
        if settlement.earned_xp != 0 {
            self.bridge_grant(&event_id, settlement.earned_xp).await;
        }
        Ok(())
    }

    /// Best-effort 桥调用；无桥（测试/CLI）或有桥失败都只记日志。
    async fn bridge_grant(&self, event_id: &str, delta_xp: i64) {
        let sink = self
            .companion_xp_sink
            .read()
            .ok()
            .and_then(|guard| guard.clone());
        let Some(sink) = sink else {
            return;
        };
        if let Err(error) = sink.grant_learning_xp(event_id, delta_xp).await {
            tracing::warn!(
                target: "nomifun_learning",
                event_id, delta_xp, error = %error,
                "companion xp bridge grant failed; learning ledger stays authoritative"
            );
        }
    }

    /// 日 XP 目标（应用设置 `learning.dailyXpGoal`，缺省 40；写侧拒绝
    /// 1..=1000 之外，读侧越界值回退缺省）。
    pub(super) async fn daily_xp_goal(&self) -> i64 {
        let Ok(row) = sqlx::query_scalar::<_, String>(
            "SELECT value FROM client_preferences WHERE key = 'learning.dailyXpGoal'",
        )
        .fetch_optional(&self.pool)
        .await
        else {
            return DEFAULT_DAILY_XP_GOAL;
        };
        match row {
            Some(value) => serde_json::from_str::<Value>(&value)
                .ok()
                .and_then(|parsed| parsed.as_i64())
                .filter(|goal| (1..=1000).contains(goal))
                .unwrap_or(DEFAULT_DAILY_XP_GOAL),
            None => DEFAULT_DAILY_XP_GOAL,
        }
    }

    /// XP 读数（两把尺同源回放、互不换算）：今日 XP + 日目标（努力尺），
    /// 结算节点/总节点/剩余 est/ETA（进度尺，现存且启用同筛）。纯投影，
    /// 零新状态——ETA 绝不回写。
    pub async fn xp_reading(&self, user_id: &UserId) -> Result<XpReading, AppError> {
        let now = now_ms();
        let tz_offset_minutes = self.tz_offset_minutes().await;
        let review_day = review_day_number(now, tz_offset_minutes);
        let day_start = review_day_start_utc(now, tz_offset_minutes);
        let today_xp: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(earned_xp), 0) FROM learning_xp_events \
             WHERE user_id = ? AND ts >= ?",
        )
        .bind(user_id.as_str())
        .bind(day_start)
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let goal = self.daily_xp_goal().await;

        // 进度尺：分子分母同筛「现存且启用」（已归档 enabled = 0 与已删除
        // 墓碑的课程都不进尺；历史面日历不过筛）。
        let enabled_scope = "SELECT e.course_id FROM learning_enrollments e \
             JOIN learning_courses c ON c.course_id = e.course_id \
             WHERE e.user_id = ? AND c.deleted_at IS NULL AND c.enabled = 1";
        let total_nodes: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id IN ({enabled_scope})"
        ))
        .bind(user_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let settled_nodes: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT node_id) FROM learning_xp_events \
             WHERE user_id = ? AND kind = 'node-completed' \
               AND node_id IS NOT NULL \
               AND course_id IN (SELECT e.course_id FROM learning_enrollments e \
                 JOIN learning_courses c ON c.course_id = e.course_id \
                 WHERE e.user_id = ? AND c.deleted_at IS NULL AND c.enabled = 1)",
        )
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let remaining_est: i64 = sqlx::query_scalar(&format!(
            "SELECT COALESCE(SUM(l.estimated_minutes), 0) FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id IN ({enabled_scope}) \
               AND l.lesson_id NOT IN (SELECT node_id FROM learning_xp_events \
                 WHERE user_id = ? AND kind = 'node-completed' AND node_id IS NOT NULL)"
        ))
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;

        // ETA：过去 7 个学习日的日均 XP 为步伐（同量纲：1 XP = 1 est 分钟），
        // 步伐为零（无历史）如实返回 null；绝不回写。
        let pace_window_start = day_start - 6 * 86_400_000;
        let week_xp: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(earned_xp), 0) FROM learning_xp_events \
             WHERE user_id = ? AND ts >= ?",
        )
        .bind(user_id.as_str())
        .bind(pace_window_start)
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let pace = week_xp as f64 / 7.0;
        let eta_days = if remaining_est <= 0 {
            // 学完（有课且无剩余）= 0 天；空账（无课）没有 ETA 可言。
            (total_nodes > 0).then_some(0)
        } else if pace <= 0.0 {
            None
        } else {
            Some((remaining_est as f64 / pace).ceil() as i64)
        };

        Ok(XpReading {
            review_day,
            today_xp,
            goal,
            goal_achieved: today_xp >= goal,
            settled_nodes,
            total_nodes,
            remaining_est,
            eta_days,
        })
    }

    /// XP 日历：恒 `weeks × 7` 桶含零桶、末桶 = 今日（学习日 02:00 翻转），
    /// 负账如实出桶，不过课程筛。
    pub async fn xp_calendar(
        &self,
        user_id: &UserId,
        weeks: i64,
    ) -> Result<XpCalendar, AppError> {
        let weeks = weeks.clamp(1, 53);
        let now = now_ms();
        let tz_offset_minutes = self.tz_offset_minutes().await;
        let today = review_day_number(now, tz_offset_minutes);
        let bucket_count = weeks * 7;
        // 拉取覆盖整个窗口（外加一天缓冲吸收 02:00 翻转）的事件，按学习日
        // 聚合净入账。
        let window_start = review_day_start_utc(now, tz_offset_minutes)
            - i64::from(bucket_count - 1) * 86_400_000
            - 86_400_000;
        let rows = sqlx::query(
            "SELECT earned_xp, ts FROM learning_xp_events \
             WHERE user_id = ? AND ts >= ? ORDER BY ts",
        )
        .bind(user_id.as_str())
        .bind(window_start)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut by_day: HashMap<i64, i64> = HashMap::new();
        for row in rows {
            let earned_xp: i64 = row.try_get("earned_xp").map_err(internal)?;
            let ts: i64 = row.try_get("ts").map_err(internal)?;
            *by_day
                .entry(review_day_number(ts, tz_offset_minutes))
                .or_insert(0) += earned_xp;
        }
        let days = (0..bucket_count)
            .map(|offset| {
                let review_day = shift_review_day(today, offset + 1 - bucket_count);
                XpCalendarDay {
                    review_day,
                    xp: by_day.get(&review_day).copied().unwrap_or(0),
                }
            })
            .collect();
        Ok(XpCalendar {
            weeks,
            tz_offset: tz_offset_minutes,
            days,
        })
    }

    /// quiz 触发计数（q1 纯读投影，零新存储）：N = 距该课程**上次
    /// quiz 结算**之后的 `node-completed` 数（learnhub
    /// `settlements_since_last_quiz` 同语义——追加序为权威，账本 rowid 序
    /// 即追加序；复习/quiz 账不推进触发进度，quiz 结算即清零重新积累）。
    /// 票面 COUNT 差公式（node − quiz）只在「每次积累满 K 必 quiz」的纪律
    /// 性使用下与本语义一致；quiz 后不满 K 的场景差公式会误报 due，故取
    /// 参考实码语义。
    pub(super) async fn quiz_settled_count(
        &self,
        user_id: &UserId,
        course_id: &str,
    ) -> Result<i64, AppError> {
        Ok(self.quiz_settled_nodes(user_id, course_id).await?.len() as i64)
    }

    /// quiz 触发节点清单（q2 选题输入）：与 [`Self::quiz_settled_count`]
    /// 同语义，返回**追加序**（先学先测）的节点 id——q2 载体按它出题。
    pub(super) async fn quiz_settled_nodes(
        &self,
        user_id: &UserId,
        course_id: &str,
    ) -> Result<Vec<String>, AppError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT e.node_id FROM learning_xp_events e \
             WHERE e.user_id = ? AND e.course_id = ? AND e.kind = 'node-completed' \
               AND e.node_id IS NOT NULL \
               AND NOT EXISTS (SELECT 1 FROM learning_xp_events q \
                 WHERE q.user_id = e.user_id AND q.course_id = e.course_id \
                   AND q.kind = 'quiz-completed' AND q.rowid > e.rowid) \
             ORDER BY e.rowid",
        )
        .bind(user_id.as_str())
        .bind(course_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows)
    }

    /// 每课程 N/K 读数（`GET /courses/{id}/quiz/progress`；读数面非门禁）。
    /// 课程不在（未建或已删墓碑）= NotFound。
    pub async fn quiz_progress(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
    ) -> Result<QuizProgressView, AppError> {
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM learning_courses \
             WHERE course_id = ? AND deleted_at IS NULL",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("learning course {course_id}")));
        }
        let settled_count = self
            .quiz_settled_count(user_id, course_id.as_str())
            .await?;
        Ok(QuizProgressView {
            settled_count,
            trigger_count: QUIZ_TRIGGER_SETTLEMENTS,
            due: settled_count >= QUIZ_TRIGGER_SETTLEMENTS,
        })
    }

}

/// 学习日（YYYYMMDD）平移 `delta` 天（真实日历运算，跨月跨年正确）。
fn shift_review_day(review_day: i64, delta: i64) -> i64 {
    let date = chrono::NaiveDate::from_ymd_opt(
        (review_day / 10_000) as i32,
        ((review_day / 100) % 100) as u32,
        (review_day % 100) as u32,
    )
    .and_then(|date| date.checked_add_signed(chrono::Duration::days(delta)));
    match date {
        Some(date) => {
            i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day())
        }
        // 窗口越出日历表示（理论不可达：53 周 ≈ 一年）：原样返回。
        None => review_day,
    }
}
