use super::completer::complete;
use super::parser::{normalize_prompt, parse_json_object};
use super::*;

/// 题面防雷同门的运维开关（k04，learnhub #148 qb.similarity）：置
/// off/0/false 退回旧「规范化精确相等或子串包含」门（旧行为臂）。
pub const QB_SIMILARITY_SWITCH_ENV: &str = "NOMI_LEARNING_QB_SIMILARITY";

/// 开关取值判定（off/0/false；大小写与首尾空白宽容）。
pub fn is_qb_similarity_disabled_value(value: &str) -> bool {
    crate::is_off_switch_value(value)
}

/// 三口径相似度门是否开启（未设/其它值 = 开启）。
pub fn qb_similarity_enabled() -> bool {
    std::env::var(QB_SIMILARITY_SWITCH_ENV)
        .map(|value| !is_qb_similarity_disabled_value(&value))
        .unwrap_or(true)
}

/// 题面防雷同门的确定性计数器（k04 冒烟读数面）：进程内累计，冒烟档在
/// 回合末打印快照。`pool_checks` = 过池比对次数，`sim_hits` = 命中次数，
/// `dropped` = 题级丢弃条数，`retries` = 因门拒/破地板的反馈重试次数。
pub mod qb_readings {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static POOL_CHECKS: AtomicUsize = AtomicUsize::new(0);
    static SIM_HITS: AtomicUsize = AtomicUsize::new(0);
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    static RETRIES: AtomicUsize = AtomicUsize::new(0);

    /// 一次快照（冒烟读数行的数据源）。
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct QbSimilarityReadings {
        pub pool_checks: usize,
        pub sim_hits: usize,
        pub dropped: usize,
        pub retries: usize,
    }

    pub fn snapshot() -> QbSimilarityReadings {
        QbSimilarityReadings {
            pool_checks: POOL_CHECKS.load(Ordering::Relaxed),
            sim_hits: SIM_HITS.load(Ordering::Relaxed),
            dropped: DROPPED.load(Ordering::Relaxed),
            retries: RETRIES.load(Ordering::Relaxed),
        }
    }

    pub fn reset() {
        POOL_CHECKS.store(0, Ordering::Relaxed);
        SIM_HITS.store(0, Ordering::Relaxed);
        DROPPED.store(0, Ordering::Relaxed);
        RETRIES.store(0, Ordering::Relaxed);
    }

    pub(crate) fn record_pool_checks(count: usize) {
        POOL_CHECKS.fetch_add(count, Ordering::Relaxed);
    }
    pub(crate) fn record_sim_hit() {
        SIM_HITS.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn record_dropped(count: usize) {
        DROPPED.fetch_add(count, Ordering::Relaxed);
    }
    pub(crate) fn record_retry() {
        RETRIES.fetch_add(1, Ordering::Relaxed);
    }
}

/// 一次题级丢弃的留痕（生成报告面 + 观察条共用）。
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedActivity {
    /// 被丢弃题面的摘录（前 60 字）。
    pub prompt: String,
    /// 命中的池内题面摘录（前 60 字）。
    pub hit_prompt: String,
    /// 命中相似度分值。
    pub score: f64,
}

impl DroppedActivity {
    /// 报告面一行（反馈重试与日志共用，指名可追）。
    pub(crate) fn report_line(&self) -> String {
        format!(
            "题面「{}」与既有题面「{}」相似度 {:.2}（≥{:.2}）",
            self.prompt,
            self.hit_prompt,
            self.score,
            crate::learning_graph::defnear::STEM_SIMILARITY_THRESHOLD
        )
    }
}

/// 单题相似度命中：返回池内最佳分与命中题面（平手取先入池者——确定性）。
pub(crate) fn similarity_hit<'a>(prompt: &str, pool: &'a [String]) -> Option<(&'a str, f64)> {
    if normalize_prompt(prompt).is_empty() {
        return None;
    }
    qb_readings::record_pool_checks(pool.len());
    let mut best: Option<(&str, f64)> = None;
    for candidate in pool {
        if normalize_prompt(candidate).is_empty() {
            continue;
        }
        let score = crate::learning_graph::defnear::stem_similarity(prompt, candidate);
        if score >= crate::learning_graph::defnear::STEM_SIMILARITY_THRESHOLD
            && best.is_none_or(|(_, best_score)| score > best_score)
        {
            best = Some((candidate.as_str(), score));
        }
    }
    if best.is_some() {
        qb_readings::record_sim_hit();
    }
    best
}

/// 题级丢弃（k04）：批内逐题过池——池 = 既有题面 + 已接受的批内兄弟
/// （两两互查，先入者留）。命中 → 剔题并记留痕；数量地板由调用方在丢弃
/// 后校验（破地板 = 整稿回灌带指名）。
pub(crate) fn filter_novel_activities(
    activities: Vec<ActivityPack>,
    existing_prompts: &[String],
) -> (Vec<ActivityPack>, Vec<DroppedActivity>) {
    let mut pool: Vec<String> = existing_prompts.to_vec();
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for activity in activities {
        let prompt = activity.prompt.trim().to_owned();
        match similarity_hit(&prompt, &pool) {
            Some((hit, score)) => dropped.push(DroppedActivity {
                prompt: truncate_excerpt(&prompt),
                hit_prompt: truncate_excerpt(hit),
                score,
            }),
            None => {
                pool.push(prompt);
                kept.push(activity);
            }
        }
    }
    qb_readings::record_dropped(dropped.len());
    (kept, dropped)
}

fn truncate_excerpt(text: &str) -> String {
    text.chars().take(60).collect()
}

/// 旧门（开关 off 臂原样保留）：规范化后精确相等或子串包含命中。
pub(crate) fn legacy_duplicate_hit<'a>(
    prompt: &str,
    pool: &'a [String],
) -> Option<&'a str> {
    let normalized = normalize_prompt(prompt);
    pool.iter().find_map(|candidate| {
        let candidate_normalized = normalize_prompt(candidate);
        let trivial = normalized.len().min(candidate_normalized.len()) <= 16;
        if normalized == candidate_normalized
            || (!trivial
                && (normalized.contains(&candidate_normalized)
                    || candidate_normalized.contains(&normalized)))
        {
            Some(candidate.as_str())
        } else {
            None
        }
    })
}


/// Stage 2 of one lesson: produce `est` + activities as a
/// small JSON object, grounded in the finished document.
pub(super) async fn generate_lesson_activities(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    prompt: &str,
) -> Result<ActivitiesOutput, String> {
    let enabled = qb_similarity_enabled();
    let mut last_error = String::new();
    for attempt in 0..2 {
        if attempt > 0 {
            qb_readings::record_retry();
        }
        let user = if attempt == 0 {
            prompt.to_owned()
        } else {
            format!(
                "{prompt}\n\nThe previous activities JSON was rejected: {last_error}\n\
                 Return a corrected JSON now, keeping every activity field complete."
            )
        };
        let raw = complete(
            completer,
            model_override,
            LESSON_SYSTEM,
            &user,
            ACTIVITIES_MAX_TOKENS,
        )
        .await
        .map_err(|error| error.to_string())?;
        match parse_json_object::<ActivitiesOutput>(&raw) {
            Ok(mut output) => {
                // 题面防雷同（k04）：on 臂三口径相似门 + 题级丢弃留痕；
                // off 臂走旧「精确/子串」整批判定（旧行为）。
                let dropped = if enabled {
                    let (kept, dropped) = filter_novel_activities(output.activities, &[]);
                    output.activities = kept;
                    dropped
                } else {
                    Vec::new()
                };
                match validate_lesson_activities(&output.activities) {
                    Ok(()) => {
                        output.dropped = dropped;
                        return Ok(output);
                    }
                    Err(error) => {
                        // 破地板：整稿回灌带指名（丢弃条目逐行进反馈）。
                        let named = dropped
                            .iter()
                            .map(DroppedActivity::report_line)
                            .collect::<Vec<_>>()
                            .join("；");
                        last_error = if named.is_empty() {
                            error
                        } else {
                            format!("{error}；已丢弃的雷同题：{named}")
                        };
                    }
                }
            }
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

/// objective 题的证据回引硬门（t2）：主生成面（正文在手）缺 evidence = 违约
/// 拒绝重试——flash 对「可选」条款遵循率实测为 0，条款必须可执法。
pub(crate) fn evidence_missing(activity: &ActivityPack) -> bool {
    activity.kind.is_objective()
        && activity
            .evidence
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
}


/// One question already present in the lesson, listed in the generation
/// prompt so the model produces something novel instead of a paraphrase.
#[derive(Debug, Clone)]
pub(crate) struct ExistingLessonQuestion {
    pub kind: ActivityKind,
    pub prompt: String,
    pub answer: serde_json::Value,
    pub explanation: String,
}


/// Generates ONE additional activity for an already-generated lesson. The
/// lesson body is fixed, so unlike [`generate_lesson`] there is no document
/// stage: a single call produces one activity of the learner-chosen kind,
/// grounded in the finished document plus the cited excerpt, and validated
/// for shape and novelty against the questions the lesson already has.
/// The result is a draft for preview — nothing is persisted here.
pub(crate) async fn generate_lesson_activity(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    kind: ActivityKind,
    focus: &str,
    course_title: &str,
    module_title: &str,
    lesson_title: &str,
    summary: &str,
    excerpt: &str,
    existing_questions: &[ExistingLessonQuestion],
) -> Result<ActivityPack, String> {
    let prompt = build_lesson_activity_prompt(
        kind,
        focus,
        course_title,
        module_title,
        lesson_title,
        summary,
        excerpt,
        existing_questions,
    );
    let mut last_error = String::new();
    for attempt in 0..2 {
        if attempt > 0 {
            qb_readings::record_retry();
        }
        let user = if attempt == 0 {
            prompt.clone()
        } else {
            format!(
                "{prompt}\n\nThe previous activity JSON was rejected: {last_error}\n\
                 Return a corrected JSON now, keeping every field complete."
            )
        };
        let raw = complete(
            completer,
            model_override,
            LESSON_ACTIVITY_SYSTEM,
            &user,
            SINGLE_ACTIVITY_MAX_TOKENS,
        )
        .await
        .map_err(|error| error.to_string())?;
        match parse_json_object::<ActivityPack>(&raw) {
            Ok(activity) => {
                match validate_generated_activity(&activity, kind, existing_questions) {
                    Ok(()) => return Ok(activity),
                    Err(error) => last_error = error,
                }
            }
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}


/// Prompt for one additional activity: the finished lesson document in full,
/// the cited excerpt, the learner's optional focus hint, and every existing
/// question — with a hard novelty requirement.
fn build_lesson_activity_prompt(
    kind: ActivityKind,
    focus: &str,
    course_title: &str,
    module_title: &str,
    lesson_title: &str,
    summary: &str,
    excerpt: &str,
    existing_questions: &[ExistingLessonQuestion],
) -> String {
    let mut prompt = format!(
        "Course: {}\nModule: {}\nLesson: {}\n",
        course_title, module_title, lesson_title
    );
    prompt.push_str("Finished lesson document (design the question to verify exactly what it teaches):\n");
    prompt.push_str("--- DOCUMENT START ---\n");
    prompt.push_str(summary);
    prompt.push_str("\n--- DOCUMENT END ---\n\n");
    prompt.push_str(&format!(
        "Cited file excerpt (the question must stay grounded in it):\n--- FILE: ---\n{excerpt}\n\n"
    ));
    if !focus.trim().is_empty() {
        prompt.push_str(&format!(
            "Focus hint from the learner: {}\n",
            focus.trim()
        ));
    }
    if existing_questions.is_empty() {
        prompt.push_str("Existing questions in this lesson: none yet — you write the first.\n");
    } else {
        prompt.push_str(&format!(
            "Existing questions in this lesson ({}):\n",
            existing_questions.len()
        ));
        for (index, question) in existing_questions.iter().enumerate() {
            prompt.push_str(&format!(
                "{}. [{}] {}\n   answer: {}\n   explanation: {}\n",
                index + 1,
                question.kind.as_str(),
                question.prompt.trim(),
                question.answer,
                question.explanation.trim()
            ));
        }
        prompt.push_str(
            "The new question must NOT repeat or closely resemble any of these — no same \
             question with different numbers: change the numbers, change the scenario, or \
             change the representation channel; if the evidence quote comes from the same \
             sentence, keep it verbatim and ask about a different point of it.\n",
        );
    }
    prompt.push_str(&format!(
        "Design ONE new {} activity as JSON now.",
        kind.as_str()
    ));
    prompt
}


/// Single-activity validation: the requested kind's exact shape plus a
/// novelty check against the lesson's existing questions.
fn validate_generated_activity(
    activity: &ActivityPack,
    kind: ActivityKind,
    existing_questions: &[ExistingLessonQuestion],
) -> Result<(), String> {
    if activity.kind != kind {
        return Err(format!(
            "expected a {} activity, got {}",
            kind.as_str(),
            activity.kind.as_str()
        ));
    }
    if activity.prompt.trim().is_empty() {
        return Err("activity prompt is empty".into());
    }
    // The single-addition flow accepts the learner-chosen kind with the
    // manual-authoring option bounds (2-5) — narrower than the generation
    // stage, wider than a hand-authored single choice.
    activity.validate_shape((2, 5), true)?;
    // 新颖性门（k04）：on 臂三口径相似度 ≥0.7 命中（含换数/换情境的雷同），
    // 消息带命中题面与分值；off 臂退回旧「规范化精确/子串」判定。
    if qb_similarity_enabled() {
        let pool: Vec<String> = existing_questions
            .iter()
            .map(|question| question.prompt.clone())
            .collect();
        if let Some((hit, score)) = similarity_hit(&activity.prompt, &pool) {
            return Err(format!(
                "the question duplicates or closely resembles an existing question: \
                 \"{hit}\" (similarity {score:.2} ≥ {:.2})",
                crate::learning_graph::defnear::STEM_SIMILARITY_THRESHOLD
            ));
        }
    } else {
        let pool: Vec<String> = existing_questions
            .iter()
            .map(|question| question.prompt.clone())
            .collect();
        if let Some(hit) = legacy_duplicate_hit(&activity.prompt, &pool) {
            return Err(format!(
                "the question duplicates or closely resembles an existing question: \"{hit}\""
            ));
        }
    }
    Ok(())
}


/// Activity-stage validation: count floors and per-kind shape rules —
/// everything that made the historical single-call validator reject weak
/// activity output. The lesson draft audit (`lesson_draft.rs`) inlines the
/// same rules with position-aware messages; keep the two in sync when
/// changing either.
pub(super) fn validate_lesson_activities(activities: &[ActivityPack]) -> Result<(), String> {
    if activities.len() < LESSON_MIN_ACTIVITIES {
        return Err(format!(
            "lesson has {} activities, expected at least {LESSON_MIN_ACTIVITIES}",
            activities.len()
        ));
    }
    let objective = activities
        .iter()
        .filter(|activity| activity.kind.is_objective())
        .count();
    if objective < LESSON_MIN_OBJECTIVE_ACTIVITIES {
        return Err(format!(
            "lesson has {objective} objective activities, expected at least {LESSON_MIN_OBJECTIVE_ACTIVITIES}"
        ));
    }
    let ai_graded = activities.len() - objective;
    if ai_graded > LESSON_MAX_REFLECTION_ACTIVITIES {
        return Err(format!(
            "lesson has {ai_graded} AI-graded questions (reflection/open_question), expected at most {LESSON_MAX_REFLECTION_ACTIVITIES}"
        ));
    }
    for activity in activities {
        if activity.prompt.trim().is_empty() {
            return Err("activity prompt is empty".into());
        }
        if evidence_missing(activity) {
            return Err(format!(
                "objective activity \"{}\" is missing its \"evidence\" — a verbatim quote (<=120 chars) from the section body or excerpt; add it to every objective question",
                activity.prompt.trim()
            ));
        }
        activity.validate_shape((3, 5), true)?;
    }
    Ok(())
}

