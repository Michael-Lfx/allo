//! quiz 回合载体（q2，方案 A 瘦回合制——mount/repair 回合先例，无会话实体；
//! g4 会话命令面后置，载体票不依赖它）+ 质量增量（q3：弱先排序 / LLM 变体 /
//! 校准点与读面）。
//!
//! # 三句真相声明（违反即缺陷）
//!
//! 1. **判底只住服务端**：银行旧题的 answer/explanation 住
//!    `learning_activities.config_json`；本场变式的判分底住回合行
//!    `plan_json`（DB 即服务端，两者同纪律）——start 投影判底零字段，判底
//!    只随 [`quiz_grade`] 的反馈下发（判分已定，揭示不破红线）。
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
//! `quiz_start`（due 门 + 弱先选题 + 变体生成落回合）→ `quiz_grade`（逐题
//! 确定性判分 [`progress::evaluate`]，零 LLM 零落账）→ `quiz_complete`（逐
//! 覆盖节点 [`Settlement::settle_quiz`] → 一笔 `quiz-completed` 落
//! `learning_xp_events` → 逐题校准点入回合行）→ `quiz_calibration`（跨回合
//! 分桶读面）。
//!
//! # 选题：弱先七档（q3；learnhub `plan_quiz` 全序的 allo 适配）
//!
//! 候选 = 触发时刻已结算未 quiz 的节点（N ≥ K，追加序），排序全档确定性、
//! 全部账面回放派生零新存储（learnhub 的归因面 allo 未建，escalation 取
//! 「最近一次真实推进打脸」的账本等价物）：
//! escalation（最近一次真实推进 rating=1 = 上卷错题优先重出）>
//! non-retention（节点真保留率升序：`Σ(review−lapse)/Σreview`，无样本不进
//! 档）> lapses（`Σ lapse_count` 降序）> 低预测 R（节点卡最小 FSRS 预测
//! 升序，无预测不进档）> untouched（最近触及时点升序——从未触及视作最陈旧
//! 排最前）> node ULID 兜底。预算 clamp 取弱先序前 `QUIZ_QUESTION_BUDGET`
//! 个；每节点 1 道该节点 objective 旧题（LRU = 最久未出），无题节点跳过
//! （如实缩减覆盖面）。
//!
//! # 变体（q3；learnhub 变式站同职能，allo 复用课时加题引擎）
//!
//! 每覆盖节点在旧题之外生成 1 道同型变式（focus = 旧题考点换皮的提示，
//! 防复用池 = 该节点既有全部题面），过既有质量门（题型形状 + 新颖性 + 可
//! 判分）；**生成失败如实跳过**（该节点只出旧题——learnhub「quiz 无
//! abandon，失败顺延」的回合内化：不阻断回合，不降级旧题）。变式不落
//! `learning_activities`（课时内容面零污染：课时练习与教新结算的题量口径
//! 不被检测题改写），判分底随回合行落盘。作答呈现跨节点交错（learnhub 同
//! 款：同节点题不相邻，确定性槽位轮转）。
//!
//! # 校准（q3；learnhub CalibrationPoint 同形）
//!
//! complete 时逐题落一个校准点（作答时点该题卡的 FSRS 预测可提取性 vs
//! 实际对错；变式/无卡 = `None` 不入预测均值）随 `report_json` 落盘——
//! 零新存储；[`quiz_calibration`] 跨回合回放，按预测 R 五百分点分桶聚合
//! 实际正确率（与复习侧 `MemoryHealthStats.calibration` 同构，#89 归因
//! 读面的 quiz 数据源；读数面非门禁）。

use std::collections::HashMap;

use super::progress::evaluate;
use super::*;

use crate::models::{
    QuizCalibrationPoint, QuizGradeFeedback, QuizRoundNodeReport, QuizRoundQuestionView,
    QuizRoundReport, QuizRoundView,
};
use crate::settle::{Settlement, QUIZ_QUESTION_BUDGET, QUIZ_TRIGGER_SETTLEMENTS};

/// 选题快照行（plan_json 元素；kind 是审计面快照，判分与定价都以题源现行
/// 为准——单源）。`origin`/`variant` 为 q3 增量，serde default 容 q2 时代
/// 旧档（缺省 = bank 题库旧题）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct QuizPlanItem {
    node: String,
    node_name: String,
    activity: String,
    kind: String,
    #[serde(default)]
    origin: QuizItemOrigin,
    #[serde(default)]
    variant: Option<VariantPayload>,
}

/// 题源（q3）：bank 题库旧题 / variant 本场生成变式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum QuizItemOrigin {
    Bank,
    Variant,
}

impl Default for QuizItemOrigin {
    fn default() -> Self {
        Self::Bank
    }
}

/// 变式的判分底与呈现面（仅 variant 题携带；config_json 与
/// `learning_activities.config_json` 同形——StoredActivityConfig）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct VariantPayload {
    prompt: String,
    options: Vec<String>,
    matches: Vec<String>,
    config_json: String,
}

/// 候选节点的弱项读数（账面回放派生；见模块文档「弱先七档」）。
#[derive(Debug, Default)]
struct NodeSignal {
    /// 候选节点（lesson ULID；ULID 兜底破平键）。
    node: String,
    escalated: bool,
    /// 节点真保留率 `Σ(review−lapse) / Σreview`；`None` = 无推进样本。
    retention: Option<f64>,
    /// 推进样本量与失误量（保留率的分子分母原料）。
    review_total: i64,
    lapse_total: i64,
    /// Σ 失误（lapses 档）。
    lapses: i64,
    /// 节点卡最小 FSRS 预测可提取性；`None` = 无卡可预测。
    min_r: Option<f64>,
    /// 最近触及时点（作答/推进最大时戳）；`None` = 从未触及。
    touch_ms: Option<i64>,
}

impl LearningService {
    /// 开测（q2 载体 + q3 质量增量）：due 门 → 弱先选题 → 变体生成 →
    /// 既有 active 回合 supersede → 落回合行 → 脱敏投影（跨节点交错）。
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
        // 弱先排序（确定性全序）+ 预算 clamp（弱先序前者先装）。
        let settings = self.scheduler_settings().await;
        let mut signals = self
            .covered_node_signals(user_id, &nodes, now, &settings)
            .await?;
        signals.sort_by(|a, b| {
            b.escalated
                .cmp(&a.escalated)
                .then_with(|| ascending_option(&a.retention, &b.retention))
                .then_with(|| b.lapses.cmp(&a.lapses))
                .then_with(|| ascending_option(&a.min_r, &b.min_r))
                .then_with(|| stalest_first(&a.touch_ms, &b.touch_ms))
                .then_with(|| a.node.cmp(&b.node))
        });
        let selected: Vec<String> = signals
            .iter()
            .take(QUIZ_QUESTION_BUDGET)
            .map(|signal| signal.node.clone())
            .collect();
        let enrollment_id: Option<String> = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments \
             WHERE user_id = ? AND course_id = ?",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let headline: (String, String) = sqlx::query_as(
            "SELECT c.title, m.title FROM learning_courses c \
             JOIN learning_modules m ON m.course_id = c.course_id WHERE m.course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning course {course_id}")))?;
        let (course_title, module_title) = headline;
        // 变式站（q3）：管线 completer（观测档案随 Archiving 挂载走）；
        // 缺席（测试/CLI）= 全部节点只出旧题，不阻断回合。
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone();

        // 选题：每覆盖节点 1 道 objective 旧题（LRU）+ 1 道变式（尽力）。
        let mut node_plans: Vec<(Vec<QuizPlanItem>, Vec<QuizRoundQuestionView>)> = Vec::new();
        for node in &selected {
            let rows = sqlx::query(&format!(
                "SELECT a.activity_id, a.kind, a.prompt, a.config_json, l.title AS node_title, \
                        l.summary AS node_summary, \
                        (SELECT MAX(at.created_at) FROM learning_attempts at \
                          WHERE at.activity_id = a.activity_id \
                            AND at.enrollment_id = ?) AS last_attempt, \
                        (SELECT MAX(e.created_at) FROM learning_review_events e \
                          JOIN learning_review_items i ON i.review_item_id = e.item_id \
                         WHERE i.activity_id = a.activity_id AND e.user_id = ?) AS last_reviewed \
                 FROM learning_activities a \
                 JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                 WHERE a.lesson_id = ? AND a.kind IN ({OBJECTIVE_KIND_SQL_LIST})"
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
                shown(a).cmp(&shown(b)).then_with(|| {
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
            let node_summary: String = row.try_get("node_summary").map_err(internal)?;
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(|error| AppError::Internal(format!("activity config: {error}")))?;
            let lesson_id = parse_id::<LearningLessonId>(node.clone())?;
            let mut plan_items = vec![QuizPlanItem {
                node: node.clone(),
                node_name: node_name.clone(),
                activity: activity_id.clone(),
                kind: kind.as_str().to_owned(),
                origin: QuizItemOrigin::Bank,
                variant: None,
            }];
            let mut views = vec![QuizRoundQuestionView {
                activity_id: parse_id(activity_id.clone())?,
                kind,
                prompt: prompt.clone(),
                options: config.options.clone(),
                matches: config.matches.clone(),
                node_id: lesson_id.clone(),
                node_name: node_name.clone(),
                // e5 消解案的取数面：题面难度随 config_json 下发（装配面从
                // 节点 teaches 档位铸值）；存量行 None 容错。
                difficulty: config.difficulty,
            }];

            // 变式（q3）：同型换皮 + 防复用池；失败如实跳过（只出旧题）。
            if let Some(completer) = completer.as_ref() {
                if matches!(kind, ActivityKind::Matching) {
                    tracing::debug!(
                        target: "nomifun_learning",
                        node = %node,
                        "quiz variant skipped: matching kind has no variant generator"
                    );
                } else if let Some(variant) = self
                    .generate_quiz_variant(
                        completer.as_ref(),
                        node,
                        &kind,
                        &prompt,
                        &course_title,
                        &module_title,
                        &node_name,
                        &node_summary,
                        // 变式与旧题同考点同节点（e5）：继承旧题的铸值难度，
                        // 本场回合内 tier 口径一致。
                        config.difficulty,
                    )
                    .await
                {
                    let variant_activity = LearningActivityId::new();
                    plan_items.push(QuizPlanItem {
                        node: node.clone(),
                        node_name: node_name.clone(),
                        activity: variant_activity.to_string(),
                        kind: variant.kind.as_str().to_owned(),
                        origin: QuizItemOrigin::Variant,
                        variant: Some(variant.payload),
                    });
                    views.push(QuizRoundQuestionView {
                        activity_id: variant_activity,
                        kind: variant.kind,
                        prompt: variant.prompt,
                        options: variant.options,
                        matches: variant.matches,
                        node_id: lesson_id,
                        node_name,
                        difficulty: variant.difficulty,
                    });
                }
            }
            node_plans.push((plan_items, views));
        }
        let plan: Vec<QuizPlanItem> = node_plans
            .iter()
            .flat_map(|(items, _)| items.iter().cloned())
            .collect();
        if plan.is_empty() {
            return Err(AppError::Conflict(
                "quiz due but no covered node has objective questions".into(),
            ));
        }
        // 跨节点交错（learnhub 同款：第 1 槽全体节点 → 第 2 槽……同节点题
        // 不相邻；确定性）。plan 与投影同序（单一顺序源）。
        let max_slots = node_plans
            .iter()
            .map(|(items, _)| items.len())
            .max()
            .unwrap_or(0);
        let mut ordered_plan: Vec<QuizPlanItem> = Vec::new();
        let mut ordered_views: Vec<QuizRoundQuestionView> = Vec::new();
        for slot in 0..max_slots {
            for (items, views) in &node_plans {
                if let Some(item) = items.get(slot) {
                    ordered_plan.push(item.clone());
                }
                if let Some(view) = views.get(slot) {
                    ordered_views.push(view.clone());
                }
            }
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
        let plan_json = serde_json::to_string(&ordered_plan)
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
            questions: ordered_views,
        })
    }

    /// 逐题判分（纯读零落账；结算在 complete 全卷面）：题必须在回合计划内，
    /// 回合必须 active；判底随反馈下发（判分已定）。变式判分走 plan 内的
    /// 判分底（不查 `learning_activities`）。
    pub async fn quiz_grade(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        round_id: &str,
        activity_id: &str,
        response: &Value,
    ) -> Result<QuizGradeFeedback, AppError> {
        let plan = self.load_round_plan(user_id, course_id, round_id).await?;
        let item = plan
            .iter()
            .find(|item| item.activity == activity_id)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "activity {activity_id} is not part of quiz round {round_id}"
                ))
            })?;
        let (kind, config) = self.item_kind_and_config(item).await?;
        let (score, explanation) = evaluate(kind, &config, response)?;
        Ok(QuizGradeFeedback {
            activity_id: parse_id(activity_id.to_owned())?,
            correct: score >= 1.0,
            answer: config.answer,
            explanation,
            evidence: config.evidence,
        })
    }

    /// 全卷结算（q2 收口 + q3 校准）：逐覆盖节点 `settle_quiz` → 一笔
    /// `quiz-completed` 落 `learning_xp_events`（anchor 幂等 + 同日折叠）→
    /// 逐题校准点与回合行落盘（首写为终）。作答必满（quiz 必作答，测量
    /// 语义）。
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

        // 判分底批量面（bank 题的题卡状态——校准点的预测侧；变式无卡）。
        let settings = self.scheduler_settings().await;
        let card_states = self
            .card_states_for_activities(
                user_id,
                &plan.iter().map(|i| i.activity.clone()).collect::<Vec<_>>(),
            )
            .await?;

        // 逐题判分（题源现行为准），按节点聚合（保计划序 = 弱先序）。
        struct NodeAgg {
            node_name: String,
            kinds: Vec<ActivityKind>,
            correct: i64,
            answered: i64,
            points: Vec<QuizCalibrationPoint>,
        }
        let mut order: Vec<String> = Vec::new();
        let mut by_node: HashMap<String, NodeAgg> = HashMap::new();
        for item in &plan {
            let (kind, config) = self.item_kind_and_config(item).await?;
            let (score, _) = evaluate(kind, &config, &answers[&item.activity])?;
            let correct = score >= 1.0;
            // 校准点（q3）：作答时点该题卡的预测可提取性（变式/无卡 =
            // None，不入预测均值——learnhub CalibrationPoint 同形）。
            let predicted_r = match card_states.get(&item.activity) {
                Some((stability_days, Some(last_reviewed_at))) => predicted_retrievability(
                    *stability_days,
                    days_elapsed_between(*last_reviewed_at, now, settings.tz_offset_minutes),
                    &settings,
                ),
                _ => None,
            };
            let agg = by_node.entry(item.node.clone()).or_insert_with(|| {
                order.push(item.node.clone());
                NodeAgg {
                    node_name: item.node_name.clone(),
                    kinds: Vec::new(),
                    correct: 0,
                    answered: 0,
                    points: Vec::new(),
                }
            });
            agg.kinds.push(kind);
            agg.answered += 1;
            agg.correct += i64::from(correct);
            agg.points.push(QuizCalibrationPoint {
                activity_id: parse_id(item.activity.clone())?,
                correct,
                predicted_r,
            });
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
                questions: agg.points.clone(),
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

    /// 校准读面（q3，读数面非门禁）：该用户全部 completed 回合的校准点
    /// 回放，按预测 R 五百分点分桶聚合实际正确率（与复习侧
    /// `MemoryHealthStats.calibration` 同构；无样本 = 空表，不造假数据）。
    pub async fn quiz_calibration(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<MemoryCalibrationBin>, AppError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT report_json FROM learning_quiz_rounds \
             WHERE user_id = ? AND status = 'completed' ORDER BY id",
        )
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        struct Point {
            correct: bool,
            r: f64,
        }
        let mut points: Vec<Point> = Vec::new();
        for report_json in rows {
            // 宽松读面：只取校准点（旧档/异形行如实跳过，读数面不 fail loud）。
            let Ok(covered) = serde_json::from_str::<Vec<StoredRoundReport>>(&report_json) else {
                continue;
            };
            for node in covered {
                for point in node.questions {
                    if let Some(r) = point.predicted_r {
                        points.push(Point {
                            correct: point.correct,
                            r,
                        });
                    }
                }
            }
        }
        struct Bin {
            predicted: f64,
            correct: i64,
            count: i64,
        }
        let mut bins: HashMap<i64, Bin> = HashMap::new();
        for point in &points {
            let bucket = recall_bucket(point.r);
            let bin = bins.entry(bucket).or_insert(Bin {
                predicted: 0.0,
                correct: 0,
                count: 0,
            });
            bin.predicted += point.r;
            bin.correct += i64::from(point.correct);
            bin.count += 1;
        }
        let mut calibration: Vec<MemoryCalibrationBin> = bins
            .into_iter()
            .map(|(bucket, bin)| MemoryCalibrationBin {
                bucket,
                min: bucket as f64 / 20.0,
                max: (bucket + 1) as f64 / 20.0,
                predicted: bin.predicted / bin.count as f64,
                actual: Some(bin.correct as f64 / bin.count as f64),
                count: bin.count,
            })
            .collect();
        calibration.sort_by_key(|bin| bin.bucket);
        Ok(calibration)
    }

    // ------------------------------------------------------------- 内部件

    /// 弱先信号的账面回放（零新存储；七档见模块文档）。
    async fn covered_node_signals(
        &self,
        user_id: &UserId,
        nodes: &[String],
        now: i64,
        settings: &SchedulerSettings,
    ) -> Result<Vec<NodeSignal>, AppError> {
        let mut signals: HashMap<String, NodeSignal> = nodes
            .iter()
            .map(|node| {
                (
                    node.clone(),
                    NodeSignal {
                        node: node.clone(),
                        ..NodeSignal::default()
                    },
                )
            })
            .collect();
        if nodes.is_empty() {
            return Ok(signals.into_values().collect());
        }
        let placeholders = vec!["?"; nodes.len()].join(", ");
        // 题卡面：lapses / 保留率样本 / 预测 R / 最近推进评分（escalation）。
        let card_sql = format!(
            "SELECT a.lesson_id AS node, r.review_count, r.lapse_count, \
                    r.stability_days, r.last_reviewed_at, \
                    (SELECT log.rating FROM learning_review_log log \
                      WHERE log.source = 'course' AND log.item_id = r.review_item_id \
                        AND log.rating_source IN ('auto', 'self') \
                      ORDER BY log.created_at DESC, log.log_id DESC LIMIT 1) AS last_rating \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             JOIN learning_activities a ON a.activity_id = r.activity_id \
             WHERE e.user_id = ? AND r.archived_at IS NULL \
               AND a.lesson_id IN ({placeholders})"
        );
        let mut card_query = sqlx::query(&card_sql).bind(user_id.as_str());
        for node in nodes {
            card_query = card_query.bind(node);
        }
        for row in card_query
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?
        {
            let node: String = row.try_get("node").map_err(internal)?;
            let review_count: i64 = row.try_get("review_count").map_err(internal)?;
            let lapse_count: i64 = row.try_get("lapse_count").map_err(internal)?;
            let stability_days: f64 = row.try_get("stability_days").map_err(internal)?;
            let last_reviewed_at: Option<i64> =
                row.try_get("last_reviewed_at").map_err(internal)?;
            let last_rating: Option<i64> = row.try_get("last_rating").map_err(internal)?;
            let Some(signal) = signals.get_mut(&node) else {
                continue;
            };
            if last_rating == Some(1) {
                signal.escalated = true;
            }
            signal.review_total += review_count;
            signal.lapse_total += lapse_count;
            signal.lapses += lapse_count;
            if stability_days > 0.0 {
                if let Some(last) = last_reviewed_at {
                    if let Some(r) = predicted_retrievability(
                        stability_days,
                        days_elapsed_between(last, now, settings.tz_offset_minutes),
                        settings,
                    ) {
                        signal.min_r = Some(signal.min_r.map_or(r, |current| current.min(r)));
                    }
                }
            }
            if let Some(last) = last_reviewed_at {
                signal.touch_ms = Some(signal.touch_ms.map_or(last, |current| current.max(last)));
            }
        }
        // 作答面：最近触及（attempts 流）。
        let attempt_sql = format!(
            "SELECT a.lesson_id AS node, MAX(at.created_at) AS last_attempt \
             FROM learning_attempts at \
             JOIN learning_activities a ON a.activity_id = at.activity_id \
             WHERE at.enrollment_id IN \
               (SELECT enrollment_id FROM learning_enrollments WHERE user_id = ?) \
               AND a.lesson_id IN ({placeholders}) GROUP BY a.lesson_id"
        );
        let mut attempt_query = sqlx::query(&attempt_sql).bind(user_id.as_str());
        for node in nodes {
            attempt_query = attempt_query.bind(node);
        }
        for row in attempt_query
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?
        {
            let node: String = row.try_get("node").map_err(internal)?;
            let last_attempt: Option<i64> = row.try_get("last_attempt").map_err(internal)?;
            if let (Some(signal), Some(last)) = (signals.get_mut(&node), last_attempt) {
                signal.touch_ms = Some(signal.touch_ms.map_or(last, |current| current.max(last)));
            }
        }
        // 保留率聚合：Σ(review−lapse)/Σreview（无推进样本 = None 不进档）。
        for signal in signals.values_mut() {
            if signal.review_total > 0 {
                signal.retention = Some(
                    (signal.review_total - signal.lapse_total).max(0) as f64
                        / signal.review_total as f64,
                );
            }
        }
        Ok(signals.into_values().collect())
    }

    /// 变式生成（q3）：同型换皮（focus 带旧题考点 + 防复用池 = 节点全部
    /// 既有题面），质量门 = 课时加题引擎既有 validator（形状 + 新颖性 +
    /// 可判分）。Err（含 completer 缺席的上游）= None 如实跳过。
    #[allow(clippy::too_many_arguments)]
    async fn generate_quiz_variant(
        &self,
        completer: &dyn crate::completer::LearningCompleter,
        node: &str,
        kind: &ActivityKind,
        bank_prompt: &str,
        course_title: &str,
        module_title: &str,
        node_name: &str,
        node_summary: &str,
        // e5：旧题的铸值难度，变式判分底原样继承（同考点同档）。
        bank_difficulty: Option<u8>,
    ) -> Option<GeneratedVariant> {
        let existing = match self.existing_lesson_questions(
            &LearningLessonId::parse(node).ok()?,
        )
        .await
        {
            Ok(existing) => existing,
            Err(error) => {
                tracing::warn!(target: "nomifun_learning", node = %node, error = %error,
                    "quiz variant skipped: anti-reuse pool unavailable");
                return None;
            }
        };
        let focus = format!(
            "Quiz variant drill: test the same knowledge point as this existing \
             question from a fresh angle (new surface, same concept, judgeable answer): \
             \"{bank_prompt}\""
        );
        match generate_lesson_activity(
            completer,
            None,
            *kind,
            &focus,
            course_title,
            module_title,
            node_name,
            node_summary,
            // 变式以节点既有题面为地面（节点正文可能未生成）；excerpt 留空。
            "",
            &existing,
        )
        .await
        {
            Ok(pack) => {
                let config = StoredActivityConfig {
                    options: pack.options.clone(),
                    answer: pack.answer.clone(),
                    explanation: pack.explanation.clone(),
                    distractors: pack.distractors.clone(),
                    tol: None,
                    matches: Vec::new(),
                    difficulty: bank_difficulty,
                    evidence: pack
                        .evidence
                        .as_deref()
                        .map(str::trim)
                        .filter(|evidence| !evidence.is_empty())
                        .map(str::to_owned),
                };
                let config_json = serde_json::to_string(&config).ok()?;
                tracing::info!(target: "nomifun_learning", node = %node,
                    kind = %pack.kind.as_str(), "quiz variant generated");
                Some(GeneratedVariant {
                    kind: pack.kind,
                    prompt: pack.prompt.clone(),
                    options: pack.options,
                    matches: config.matches.clone(),
                    difficulty: config.difficulty,
                    payload: VariantPayload {
                        prompt: pack.prompt,
                        options: config.options.clone(),
                        matches: config.matches.clone(),
                        config_json,
                    },
                })
            }
            Err(error) => {
                tracing::warn!(target: "nomifun_learning", node = %node, error = %error,
                    "quiz variant generation failed; node keeps its bank question only");
                None
            }
        }
    }

    /// 题源的判分输入（bank = DB 现行；variant = plan 内判分底）。
    async fn item_kind_and_config(
        &self,
        item: &QuizPlanItem,
    ) -> Result<(ActivityKind, StoredActivityConfig), AppError> {
        match item.origin {
            QuizItemOrigin::Variant => {
                let payload = item.variant.as_ref().ok_or_else(|| {
                    AppError::Internal(format!(
                        "quiz variant {} missing its payload (plan defect)",
                        item.activity
                    ))
                })?;
                let config: StoredActivityConfig = serde_json::from_str(&payload.config_json)
                    .map_err(|error| AppError::Internal(format!("variant config: {error}")))?;
                let kind = ActivityKind::try_from(item.kind.as_str()).map_err(AppError::Internal)?;
                Ok((kind, config))
            }
            QuizItemOrigin::Bank => {
                let row = sqlx::query(
                    "SELECT kind, config_json FROM learning_activities WHERE activity_id = ?",
                )
                .bind(&item.activity)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?
                .ok_or_else(|| {
                    AppError::NotFound(format!("learning activity {}", item.activity))
                })?;
                let kind_text: String = row.try_get("kind").map_err(internal)?;
                let kind =
                    ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?;
                let config: StoredActivityConfig = serde_json::from_str(
                    &row.try_get::<String, _>("config_json").map_err(internal)?,
                )
                .map_err(|error| AppError::Internal(format!("activity config: {error}")))?;
                Ok((kind, config))
            }
        }
    }

    /// bank 题的题卡状态（校准点预测侧；activity_id → (stability, last)）。
    async fn card_states_for_activities(
        &self,
        user_id: &UserId,
        activities: &[String],
    ) -> Result<HashMap<String, (f64, Option<i64>)>, AppError> {
        let mut states = HashMap::new();
        if activities.is_empty() {
            return Ok(states);
        }
        let placeholders = vec!["?"; activities.len()].join(", ");
        let sql = format!(
            "SELECT r.activity_id, r.stability_days, r.last_reviewed_at \
             FROM learning_review_items r \
             JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
             WHERE e.user_id = ? AND r.archived_at IS NULL \
               AND r.activity_id IN ({placeholders})"
        );
        let mut query = sqlx::query(&sql).bind(user_id.as_str());
        for activity in activities {
            query = query.bind(activity);
        }
        for row in query.fetch_all(&self.pool).await.map_err(internal)? {
            let activity_id: String = row.try_get("activity_id").map_err(internal)?;
            let stability_days: f64 = row.try_get("stability_days").map_err(internal)?;
            let last_reviewed_at: Option<i64> =
                row.try_get("last_reviewed_at").map_err(internal)?;
            states.insert(activity_id, (stability_days, last_reviewed_at));
        }
        Ok(states)
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

/// Option 的升序比较（`Some` 在前升序、`None` 落后——弱信号有值者优先）。
fn ascending_option(a: &Option<f64>, b: &Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// 触及时点的陈旧序：从未触及（`None`）视作最陈旧排最前，其后越旧越前。
fn stalest_first(a: &Option<i64>, b: &Option<i64>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => x.cmp(y),
    }
}

struct GeneratedVariant {
    kind: ActivityKind,
    prompt: String,
    options: Vec<String>,
    matches: Vec<String>,
    /// 继承旧题的铸值难度（e5），投影面随变式题下发。
    difficulty: Option<u8>,
    payload: VariantPayload,
}

/// 存储侧 report_json 的宽松读形（校准回放只取逐题点；q2 时代无 questions
/// 字段的行照读）。
#[derive(serde::Deserialize)]
struct StoredRoundReport {
    #[serde(default)]
    questions: Vec<StoredCalibrationPoint>,
}

#[derive(serde::Deserialize)]
struct StoredCalibrationPoint {
    correct: bool,
    predicted_r: Option<f64>,
}
