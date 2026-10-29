//! 开放题评审判分执行器（n5，learnhub ADR-0037 评审制域中立移植）：装配
//! 判底（题目自己的 explanation，n4 D1(a)——存量即刻受益，零契约变更）+
//! 结构化补全（`complete_json_with_thinking`，语义裁决档思考开 n4 D7）+
//! schema 机械校验（`{score, feedback, missing_points}`，判对-缺失互斥的
//! 连续分适配 n4 D3）+ 重试环 N=2 带原因回灌 + 90s 显式超时（n4 D2 修
//! 三重脆弱）。
//!
//! # 纪律落点
//!
//! - **fail-loud 出口保留**（n4 D2）：重试预算耗尽 = [`OpenJudgeExhausted`]
//!   如实上抛为 `AppError::Internal`——评审失败绝不静默判错，也绝不静默
//!   吞掉（learnhub 机械兜底层不移植：allo 无 keywords，无存在条件）。
//! - **显式超时**：每次调用独立限时（[`OPEN_JUDGE_TIMEOUT`]）；超时按可
//!   重试失败处理（tokio future 被丢弃 = 只弃其果，天然 learnhub「线程
//!   分离不可撤销」语义的异步形态）。
//! - **思考通道降级可观测**（n4 D7）：`complete_json_with_thinking` 不可
//!   用时降级普通 `complete_json` 通道 + `thinking_degraded` 观测标记 +
//!   warn 留痕；不是静默换道。
//! - **判分口径域中立**（ADR-0037 决议 6 同义移植）：作答相对判底的学科
//!   内容语义正确性——同义改写合法、照抄模仿合法、空答/抄题/离题判错；
//!   措辞零学科词面。`config.answer` 对开放题契约性为 null 的现状不动
//!   （validate_shape 反转 = D1(b) 后置票）。
//! - **分数形态保留连续分**（n4 D3）：open_question 0–10（≥6 及格）、
//!   reflection 0–1（≥0.6 及格）；`score ≥ 及格 ⇔ missing_points 必空`
//!   双向机械校验（互斥适配），feedback/missing_points 纯反馈面零置位。
//! - **EvidenceTier 不引入**（n4 D5 显式豁免）：题型白名单已实现自动性
//!   证据纯度，不动 015 migration 高风险线；missing_points 走 API 层不
//!   落新列（零迁移）。
//! - **观测面脱敏**（n4 D9）：本执行器只在 tracing 留计数与原因（零判底
//!   文本）；归档脱敏在 `ArchivingCompleter` 侧按注册占位替换（smoke
//!   grade 档），归档失败降级不阻断判分。

use std::time::Duration;

use serde::Deserialize;

use super::{ActivityKind, ProviderId};
use crate::completer::LearningCompleter;
use crate::generation::parse_json_object;
use nomifun_common::AppError;

/// 评审判分重试预算：首次 + 2 次重试（重试环 N=2，带原因回灌）。
pub(crate) const OPEN_JUDGE_ATTEMPTS: usize = 3;

/// 单次评审调用显式超时（语义裁决档 + 思考开；判果质量优先于延迟，但
/// 不无限等——provider 挂起时请求在 90s 内如实失败进重试环）。
pub(crate) const OPEN_JUDGE_TIMEOUT: Duration = Duration::from_secs(90);

/// 判分专项输出预算（n4 D7）：思考开时推理与 JSON 共享此上限——判分是
/// 语义裁决档（learnhub 同族评审调用观察到 13k+ 推理先行），旧 no-thinking
/// 预算 2048 会被推理吃穿确定性截断；取概念评审档先例 16384（教练档
/// 32768 的一半：单题裁决轻于整批规划），对照卡 thinking on/off 双跑
/// 即为该预算的校准点。
pub(crate) const OPEN_JUDGE_MAX_TOKENS: u32 = 16384;

/// 判底观测面占位（smoke grade 档注册进 `ArchivingCompleter` 时引用——
/// 占位文本单源，判底不进观测面，n4 D9）。
pub const REDACTED_GROUNDING_PLACEHOLDER: &str = "(已脱敏:判底)";

/// 作答原文观测面占位（同上；feedback 是反馈面原文保留——审计面读得到
/// 评审说了什么）。
pub const REDACTED_RESPONSE_PLACEHOLDER: &str = "(已脱敏:作答原文)";

/// 一次评审的输入（判底 = 题目自己的 explanation；作答原文为不可信数据，
/// 装配进数据位，注入防面在 system 条款）。
pub(crate) struct OpenJudgeInput<'a> {
    pub kind: ActivityKind,
    /// 题面。
    pub prompt: &'a str,
    /// 学习者作答原文。
    pub answer: &'a str,
    pub lesson_title: &'a str,
    pub lesson_summary: &'a str,
    /// 判底（n4 D1(a)）。`Some("")` = 存量题 explanation 为空（装配容错
    /// 占位「以课时上下文为准」）；评审链恒传 `Some`，旧对照臂（off 臂）
    /// 传 `None`（不装配判底槽，prompt 保旧形）。
    pub explanation: Option<&'a str>,
}

/// 一次评审结论（已过机械校验；feedback/missing_points 纯反馈面，零置位
/// 零落账——attempts 表只存 score/passed/feedback 原样）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OpenJudgeVerdict {
    /// 原生分数形态（open_question 0–10 / reflection 0–1，n4 D3 保留
    /// 连续分；归一化到 0–1 在调用方）。
    pub score: f64,
    pub feedback: String,
    /// 缺失要点（判错时逐条短评；判对恒空——判对-缺失互斥）。
    pub missing_points: Vec<String>,
}

/// 评审结果 + 通道观测（n4 D7：thinking 不可用降级普通 json 通道时置位）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OpenJudgeOutcome {
    pub verdict: OpenJudgeVerdict,
    pub thinking_degraded: bool,
}

/// 评审失败（重试预算耗尽；调用方如实上抛，绝不静默判错，n4 D2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenJudgeExhausted {
    pub attempts: usize,
    pub last: String,
}

impl std::fmt::Display for OpenJudgeExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "open-answer judge exhausted after {} attempts, last failure: {}",
            self.attempts, self.last
        )
    }
}

/// System prompt for AI open-question judging: a comprehensive-application
/// question scored on a 0-10 scale (≥6 passes), graded on the semantic
/// correctness of the answer relative to the question's own explanation
/// (the grounding) and its lesson topic — ADR-0037 judging口径, domain
/// neutral. The score/missing_points contract is mutually exclusive (n4 D3).
const OPEN_QUESTION_JUDGE_SYSTEM: &str = r#"You are a strict but encouraging learning coach judging a learner's open-ended answer to a comprehensive course question.

Grounding: the question's own explanation and its lesson topic define what a correct answer must convey. Judge the semantic correctness of the answer relative to that grounding:
- Semantic coverage wins over wording: a paraphrase, a different order, or a close restatement of the grounding is a legal answer and must not be punished.
- An answer that merely restates the question itself, copies the prompt, or wanders off the question's topic is incorrect.
- Application matters: the answer should show the learner can use the ideas, not just recite unrelated facts.

Score the answer from 0.0 to 10.0 (6.0 is passing) and report the missing points:
- score must be a finite number between 0.0 and 10.0.
- missing_points must list, as short concrete phrases in the learner's language, every key point of the grounding the answer fails to convey. The contract is mutually exclusive: a passing score (>= 6.0) requires an empty array; a failing score (< 6.0) requires a non-empty array naming what the answer lacks.

Reply with ONLY one JSON object matching this shape:
{
  "score": 6.5,
  "feedback": "markdown text",
  "missing_points": []
}
Rules:
- feedback must be non-empty Markdown that helps the learner improve: what the answer conveys or misses against the grounding, and how to close the gap.
- Write the feedback and the missing points in the same language as the learner's answer.
- Output JSON only, without Markdown fences or commentary."#;

/// System prompt for AI reflection judging: the learner reconstructs what the
/// lesson changed for them, scored on a 0-1 scale (≥0.6 passes). Same
/// grounding/judging口径 and mutually exclusive contract as the open
/// question channel (n4 D8: reflection rides the same rewrite, 0-1 form
/// preserved).
const REFLECTION_JUDGE_SYSTEM: &str = r#"You are a strict but encouraging learning coach judging a learner's reflection answer for a course exercise.

Grounding: the question's own explanation and its lesson topic define what a complete reflection must touch. Judge the semantic correctness of the reflection relative to that grounding:
- Semantic coverage wins over wording: a paraphrase, a different order, or a close restatement of the grounding is a legal answer and must not be punished.
- A reflection that merely restates the question, copies the prompt, or wanders off the topic is incorrect.
- Substance matters: generic praise or filler with no reconstructed idea is incomplete.

Score the reflection from 0.0 to 1.0 (0.6 is passing) and report the missing points:
- score must be a finite number between 0.0 and 1.0.
- missing_points must list, as short concrete phrases in the learner's language, every part of the ask the reflection fails to cover. The contract is mutually exclusive: a passing score (>= 0.6) requires an empty array; a failing score (< 0.6) requires a non-empty array naming what the reflection lacks.

Reply with ONLY one JSON object matching this shape:
{
  "score": 0.75,
  "feedback": "markdown text",
  "missing_points": []
}
Rules:
- feedback must be Markdown with two parts: (1) an evaluation of the reflection, (2) concrete improvement suggestions.
- Write the feedback and the missing points in the same language as the learner's answer.
- Output JSON only, without Markdown fences or commentary."#;

/// 评审判分一次（重试环 + 超时 + 通道降级；生产恒 [`OPEN_JUDGE_TIMEOUT`]）。
/// 失败 = [`OpenJudgeExhausted`]，绝不静默出结论。
pub(crate) async fn judge_open(
    completer: &dyn LearningCompleter,
    model_override: Option<(&str, &str)>,
    input: &OpenJudgeInput<'_>,
) -> Result<OpenJudgeOutcome, OpenJudgeExhausted> {
    judge_open_with_timeout(completer, model_override, input, OPEN_JUDGE_TIMEOUT).await
}

/// [`judge_open`] 的超时可注入变体（测试面；生产恒 [`OPEN_JUDGE_TIMEOUT`]）。
async fn judge_open_with_timeout(
    completer: &dyn LearningCompleter,
    model_override: Option<(&str, &str)>,
    input: &OpenJudgeInput<'_>,
    timeout: Duration,
) -> Result<OpenJudgeOutcome, OpenJudgeExhausted> {
    let system = judge_system(input.kind);
    let mut retry_feedback: Option<String> = None;
    let mut thinking_degraded = false;
    let mut last_error = String::new();
    for attempt in 1..=OPEN_JUDGE_ATTEMPTS {
        let user = super::progress::build_open_grading_prompt(
            input.kind,
            input.prompt,
            input.answer,
            input.lesson_title,
            input.lesson_summary,
            input.explanation,
            retry_feedback.as_deref(),
        );
        let started = std::time::Instant::now();
        let outcome = if thinking_degraded {
            // 一次定臂（评审裁定）：通道不可用一经判定，后续尝试直走普通
            // json 通道，不再重复探测 thinking 通道。
            call_plain_json(completer, model_override, system, &user, timeout)
                .await
                .map_err(|json_error| format!("degraded json channel failed ({json_error})"))
                .and_then(|raw| validate_judge_json(&raw, input.kind))
        } else {
            match call_with_thinking(completer, model_override, system, &user, timeout).await {
                Ok(raw) => validate_judge_json(&raw, input.kind),
                // n4 D7 降级收窄（评审裁定）：只对「通道/能力不可用」类错误
                // 降级普通 json 通道 + 观测标记（不是静默换道——warn 留痕 +
                // outcome.thinking_degraded 置位）。超时是判分自己的 90s 预
                // 算守卫；provider 错误统一 BadGateway 字符串（能力拒绝与瞬
                // 时 5xx 同形，不可靠区分）——都不降级，按可重试失败留在
                // thinking 通道走重试环（保 90s×N 预算语义）。
                Err(thinking_error) if thinking_channel_unavailable(&thinking_error) => {
                    tracing::warn!(
                        attempt,
                        kind = input.kind.as_str(),
                        error = %thinking_error,
                        "open judge: thinking channel unavailable — degrading to the plain json channel"
                    );
                    thinking_degraded = true;
                    match call_plain_json(completer, model_override, system, &user, timeout).await {
                        Ok(raw) => validate_judge_json(&raw, input.kind),
                        Err(json_error) => Err(format!(
                            "thinking channel failed ({thinking_error}) and the degraded json channel failed too ({json_error})"
                        )),
                    }
                }
                // 不可降级失败（超时/provider 瞬时错误等）与校验失败同一重
                // 试面：原因回灌，通道不变。
                Err(thinking_error) => Err(format!("thinking channel failed: {thinking_error}")),
            }
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match outcome {
            Ok(verdict) => {
                tracing::info!(
                    attempt,
                    elapsed_ms,
                    kind = input.kind.as_str(),
                    score = verdict.score,
                    missing = verdict.missing_points.len(),
                    thinking_degraded,
                    "open judge: verdict accepted"
                );
                return Ok(OpenJudgeOutcome {
                    verdict,
                    thinking_degraded,
                });
            }
            Err(reason) => {
                tracing::warn!(
                    attempt,
                    elapsed_ms,
                    kind = input.kind.as_str(),
                    reason = %reason,
                    "open judge: reply rejected — retrying with the reason fed back"
                );
                retry_feedback = Some(format!(
                    "上一轮输出未过校验：{reason}。请只输出符合输出契约的 JSON 对象。"
                ));
                last_error = reason;
            }
        }
    }
    Err(OpenJudgeExhausted {
        attempts: OPEN_JUDGE_ATTEMPTS,
        last: last_error,
    })
}

/// 单次思考开的结构化调用带显式超时；超时 = 可重试失败（future 被丢弃，
/// 只弃其果）。
async fn call_with_thinking(
    completer: &dyn LearningCompleter,
    model_override: Option<(&str, &str)>,
    system: &str,
    user: &str,
    timeout: Duration,
) -> Result<String, AppError> {
    let call = completer.complete_json_with_thinking(
        model_override,
        system,
        user,
        OPEN_JUDGE_MAX_TOKENS,
        Some(0.0),
    );
    with_timeout(call, timeout).await
}

/// 降级通道：普通 json 通道（思考关），同超时算术。
async fn call_plain_json(
    completer: &dyn LearningCompleter,
    model_override: Option<(&str, &str)>,
    system: &str,
    user: &str,
    timeout: Duration,
) -> Result<String, AppError> {
    let call = completer.complete_json(
        model_override,
        system,
        user,
        OPEN_JUDGE_MAX_TOKENS,
        Some(0.0),
    );
    with_timeout(call, timeout).await
}

async fn with_timeout(
    call: impl std::future::Future<Output = Result<String, AppError>>,
    timeout: Duration,
) -> Result<String, AppError> {
    tokio::time::timeout(timeout, call)
        .await
        .unwrap_or_else(|_| {
            Err(AppError::Timeout(format!(
                "open judge call exceeded {}s — treated as a retryable failure",
                timeout.as_secs()
            )))
        })
}

/// 「通道/能力不可用」可靠判别（n4 D7 降级收窄，评审裁定）：只认
/// [`LearningCompleter`] 缝约文档化的结构化输出不可用信号（crate 默认实现
/// 的 loud-failure 形态，"learning structured output unavailable"）。超时是
/// 判分自己的预算守卫（`AppError::Timeout`）；provider 错误统一
/// `BadGateway` 字符串（能力拒绝与瞬时 5xx 同形）——都不可靠判别为能力
/// 缺失，一律不降级。
fn thinking_channel_unavailable(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Conflict(message) if message.contains("structured output unavailable")
    )
}

/// 原生分数域（n4 D3：open_question 0–10、reflection 0–1）。
fn score_range(kind: ActivityKind) -> (f64, f64) {
    if kind == ActivityKind::OpenQuestion {
        (0.0, 10.0)
    } else {
        (0.0, 1.0)
    }
}

/// 及格线（与 submit_attempt 的归一化 0.6 判定同构：open 6.0/10、
/// reflection 0.6/1.0）。
fn pass_threshold(kind: ActivityKind) -> f64 {
    if kind == ActivityKind::OpenQuestion {
        6.0
    } else {
        0.6
    }
}

fn judge_system(kind: ActivityKind) -> &'static str {
    if kind == ActivityKind::OpenQuestion {
        OPEN_QUESTION_JUDGE_SYSTEM
    } else {
        REFLECTION_JUDGE_SYSTEM
    }
}

impl super::LearningService {
    /// 新评审链入口（`NOMI_LEARNING_OPEN_JUDGE` on 臂）：当前 completer 一
    /// 次评审（重试环 + 超时 + 判底进 prompt），原生分归一化到 0–1 存储
    /// 尺（open_question ÷10；0.6 = 及格线与旧尺同构）。评审失败如实上抛
    /// `AppError::Internal`——fail-loud 保留（n4 D2），绝不静默判错。
    /// 评审输入收 [`OpenJudgeInput`] 结构（Data Clumps 收口，评审裁定）。
    pub(super) async fn judge_answer(
        &self,
        input: OpenJudgeInput<'_>,
        provider_id: Option<&ProviderId>,
        model: Option<&str>,
    ) -> Result<(f64, String, Vec<String>), AppError> {
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| AppError::Conflict("AI answer grading is not configured".into()))?;
        let outcome = judge_open(
            &*completer,
            provider_id.zip(model).map(|(id, model)| (id.as_str(), model)),
            &input,
        )
        .await
        .map_err(|exhausted| AppError::Internal(exhausted.to_string()))?;
        // open_question arrives on a 0-10 scale; normalize to 0-1 so attempt
        // storage stays on the shared scale (0.6 = pass) — identical
        // arithmetic to the legacy arm.
        let normalized = if input.kind == ActivityKind::OpenQuestion {
            outcome.verdict.score / 10.0
        } else {
            outcome.verdict.score
        };
        Ok((
            normalized.clamp(0.0, 1.0),
            outcome.verdict.feedback,
            outcome.verdict.missing_points,
        ))
    }
}

/// 评审结论机械校验（形状 + 分数域 + 判对-缺失互斥双向）；失败原因回灌
/// 重试环。reply 键集从严（learnhub 同律）：多键/缺键都打回——宽松解析
/// 会把模型的结构性含糊固化成判分噪声。
fn validate_judge_json(raw: &str, kind: ActivityKind) -> Result<OpenJudgeVerdict, String> {
    let value: serde_json::Value =
        parse_json_object(raw).map_err(|error| format!("reply is not one JSON object: {error}"))?;
    let map = value.as_object().ok_or("reply is not a JSON object")?;
    let expected = ["score", "feedback", "missing_points"];
    if map.len() != expected.len() {
        return Err(format!(
            "reply must have exactly 3 keys (score/feedback/missing_points), got {} keys",
            map.len()
        ));
    }
    for key in expected {
        if !map.contains_key(key) {
            return Err(format!("reply is missing the '{key}' key"));
        }
    }
    #[derive(Deserialize)]
    struct JudgeReply {
        score: f64,
        feedback: String,
        missing_points: Vec<String>,
    }
    let reply: JudgeReply = serde_json::from_value(value)
        .map_err(|error| format!("reply fields have the wrong types: {error}"))?;
    let (min, max) = score_range(kind);
    if !reply.score.is_finite() || reply.score < min || reply.score > max {
        return Err(format!(
            "score must be a finite number between {min} and {max}, got {}",
            reply.score
        ));
    }
    if reply.feedback.trim().is_empty() {
        return Err("feedback must be a non-empty string".into());
    }
    for (index, point) in reply.missing_points.iter().enumerate() {
        if point.trim().is_empty() {
            return Err(format!("missing_points[{index}] must be a non-empty string"));
        }
    }
    // 判对-缺失互斥（n4 D3 连续分适配，双向）：score ≥ 及格 ⇔ missing 必空。
    let threshold = pass_threshold(kind);
    let passing = reply.score >= threshold;
    if passing && !reply.missing_points.is_empty() {
        return Err(format!(
            "a passing score ({} >= {threshold}) requires an empty missing_points array (judged-correct/missing mutual exclusion)",
            reply.score
        ));
    }
    if !passing && reply.missing_points.is_empty() {
        return Err(format!(
            "a failing score ({} < {threshold}) requires a non-empty missing_points array naming what the answer lacks",
            reply.score
        ));
    }
    Ok(OpenJudgeVerdict {
        score: reply.score,
        feedback: reply.feedback,
        missing_points: reply.missing_points,
    }
)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 罐头执行器：thinking/json 两通道各挂脚本逐轮返回；记录每次收到的
    /// user 文本（重试回灌断言面）。文本通道（complete/with_thinking）一
    /// 旦被触达即 panic——评审链必须走 json 族通道。
    struct Canned {
        thinking_script: Mutex<Vec<Result<String, AppError>>>,
        json_script: Mutex<Vec<Result<String, AppError>>>,
        delay: Duration,
        seen: Mutex<Vec<(String, String)>>,
    }

    impl Canned {
        fn thinking(script: Vec<Result<String, AppError>>) -> Self {
            Self {
                thinking_script: Mutex::new(script),
                json_script: Mutex::new(vec![]),
                delay: Duration::ZERO,
                seen: Mutex::new(vec![]),
            }
        }

        fn record(&self, method: &str, user: &str) {
            self.seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((method.to_owned(), user.to_owned()));
        }

        fn pop(script: &Mutex<Vec<Result<String, AppError>>>) -> Result<String, AppError> {
            let mut guard = script.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_empty() {
                Err(AppError::Internal("canned script is empty".into()))
            } else {
                guard.remove(0)
            }
        }

        fn users(&self, method: &str) -> Vec<String> {
            self.seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|(seen, _)| seen == method)
                .map(|(_, user)| user.clone())
                .collect()
        }
    }

    #[async_trait::async_trait]
    impl LearningCompleter for Canned {
        async fn complete(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<String, AppError> {
            panic!("judge must never touch the plain text channel");
        }

        async fn complete_with_thinking(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<String, AppError> {
            panic!("judge must never touch the plain thinking channel");
        }

        async fn complete_json(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            user: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, AppError> {
            self.record("json", user);
            tokio::time::sleep(self.delay).await;
            Self::pop(&self.json_script)
        }

        async fn complete_json_with_thinking(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            user: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, AppError> {
            self.record("thinking", user);
            tokio::time::sleep(self.delay).await;
            Self::pop(&self.thinking_script)
        }
    }

    fn open_input<'a>(explanation: &'a str) -> OpenJudgeInput<'a> {
        OpenJudgeInput {
            kind: ActivityKind::OpenQuestion,
            prompt: "Explain why compound interest rewards time.",
            answer: "The learner's original answer sentence.",
            lesson_title: "Compound Thinking",
            lesson_summary: "How compounding rewards patience.",
            explanation: Some(explanation),
        }
    }

    fn verdict_json(score: &str, missing: &[&str]) -> String {
        let points: Vec<String> = missing.iter().map(|p| format!("\"{p}\"")).collect();
        format!(
            r#"{{"score": {score}, "feedback": "点评", "missing_points": [{}]}}"#,
            points.join(", ")
        )
    }

    async fn judge(canned: &Canned, input: &OpenJudgeInput<'_>) -> Result<OpenJudgeOutcome, OpenJudgeExhausted> {
        judge_open_with_timeout(canned, None, input, Duration::from_secs(5)).await
    }

    #[tokio::test]
    async fn valid_verdict_passes_first_attempt_without_retry_feedback() {
        let canned = Canned::thinking(vec![Ok(verdict_json("7.5", &[]))]);
        let outcome = judge(&canned, &open_input("复利的判底原文")).await.unwrap();
        assert_eq!(outcome.verdict.score, 7.5);
        assert!(outcome.verdict.missing_points.is_empty(), "判对时缺失要点为空");
        assert!(!outcome.thinking_degraded);
        let users = canned.users("thinking");
        assert_eq!(users.len(), 1, "一次调用一档");
        assert!(
            !users[0].contains("上一轮输出未过校验"),
            "首轮不得携带回灌槽：{}",
            users[0]
        );
        assert!(users[0].contains("复利的判底原文"), "判底必须进评审 prompt");
    }

    #[tokio::test]
    async fn invalid_shape_retries_with_reason_fed_back() {
        let canned = Canned::thinking(vec![
            Ok(verdict_json("12.5", &[])), // 越界分数 → 回灌重试
            Ok(verdict_json("7.0", &[])),
        ]);
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert_eq!(outcome.verdict.score, 7.0);
        let users = canned.users("thinking");
        assert_eq!(users.len(), 2, "两次调用");
        assert!(
            users[1].contains("score must be a finite number between 0 and 10"),
            "回灌必须携带失败原因：{}",
            users[1]
        );
        assert!(
            users[1].contains("上一轮输出未过校验"),
            "回灌走固定槽位：{}",
            users[1]
        );
    }

    #[tokio::test]
    async fn passing_score_with_missing_points_is_rejected_as_contradiction() {
        let canned = Canned::thinking(vec![
            Ok(verdict_json("8.0", &["要点甲"])), // 判对-缺失互斥违约
            Ok(verdict_json("8.0", &[])),
        ]);
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert_eq!(outcome.verdict.missing_points.len(), 0);
        let users = canned.users("thinking");
        assert!(
            users[1].contains("mutual exclusion"),
            "互斥原因进回灌：{}",
            users[1]
        );
    }

    #[tokio::test]
    async fn failing_score_with_empty_missing_points_is_rejected_too() {
        // 互斥是双向的：判错必须点名缺失（n4 D3 ⇔ 适配）。
        let canned = Canned::thinking(vec![
            Ok(verdict_json("3.0", &[])),
            Ok(verdict_json("3.0", &["时间的作用"])),
        ]);
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert_eq!(outcome.verdict.score, 3.0);
        assert_eq!(outcome.verdict.missing_points, vec!["时间的作用".to_string()]);
    }

    #[tokio::test]
    async fn exhausted_after_retry_budget_with_last_reason() {
        let canned = Canned::thinking(vec![
            Ok("不是 json".into()),
            Ok("还不是 json".into()),
            Ok("依然不是 json".into()),
        ]);
        let err = judge(&canned, &open_input("判底")).await.unwrap_err();
        assert_eq!(err.attempts, OPEN_JUDGE_ATTEMPTS);
        assert!(
            err.last.contains("not one JSON object") || err.last.contains("不是"),
            "耗尽时携带最后一次原因：{}",
            err.last
        );
    }

    #[tokio::test]
    async fn timeout_is_retryable_and_exhausts_without_waiting_full_delay() {
        let mut canned = Canned::thinking(vec![]);
        canned.delay = Duration::from_millis(500);
        let started = std::time::Instant::now();
        let err = judge_open_with_timeout(&canned, None, &open_input("判底"), Duration::from_millis(40))
            .await
            .unwrap_err();
        assert!(err.last.contains("exceeded"), "超时按可重试失败处理：{}", err.last);
        assert_eq!(err.attempts, OPEN_JUDGE_ATTEMPTS);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "超时路径不等满 delay×3 的完成时长"
        );
    }

    #[tokio::test]
    async fn thinking_unavailable_degrades_to_plain_json_channel_with_marker() {
        let canned = Canned::thinking(vec![Err(AppError::Conflict(
            "learning structured output unavailable: thinking mode rejected by provider".into(),
        ))]);
        canned
            .json_script
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Ok(verdict_json("2.0", &["缺失要点"])));
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert!(outcome.thinking_degraded, "降级必须置观测标记");
        assert_eq!(outcome.verdict.score, 2.0);
        assert_eq!(canned.users("json").len(), 1, "降级在普通 json 通道完成");
    }

    /// 降级收窄（评审裁定钉，红→绿）：超时不触发降级——按可重试失败留在
    /// thinking 通道走重试环（保 90s×N 预算语义），json 通道零触达。
    #[tokio::test]
    async fn timeout_on_thinking_channel_does_not_degrade() {
        let canned = Canned::thinking(vec![
            Err(AppError::Timeout("open judge call exceeded 90s".into())),
            Ok(verdict_json("7.0", &[])),
        ]);
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert!(!outcome.thinking_degraded, "超时不得触发降级");
        assert_eq!(outcome.verdict.score, 7.0);
        assert_eq!(canned.users("thinking").len(), 2, "两试都在 thinking 通道");
        assert_eq!(canned.users("json").len(), 0, "json 通道不得被触达");
    }

    /// provider 瞬时错误（BadGateway）与能力拒绝在 LiveLearningCompleter
    /// 错误形态里同为字符串——不可靠判别为能力缺失，不降级，走重试环。
    #[tokio::test]
    async fn transient_provider_error_does_not_degrade_the_thinking_channel() {
        let canned = Canned::thinking(vec![
            Err(AppError::BadGateway(
                "LLM provider error: API error 502: bad gateway".into(),
            )),
            Ok(verdict_json("6.5", &[])),
        ]);
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert!(!outcome.thinking_degraded, "瞬时错误不是能力缺失");
        assert_eq!(canned.users("thinking").len(), 2);
        assert_eq!(canned.users("json").len(), 0);
    }

    /// 一次定臂：通道不可用判定后，后续尝试直走普通 json 通道，不再重复
    /// 探测 thinking 通道（降级臂粘滞，不中途切回）。
    #[tokio::test]
    async fn degraded_channel_is_sticky_for_remaining_attempts() {
        let canned = Canned::thinking(vec![Err(AppError::Conflict(
            "learning structured output unavailable: thinking mode rejected by provider".into(),
        ))]);
        {
            let mut json_script = canned.json_script.lock().unwrap_or_else(|e| e.into_inner());
            json_script.push(Ok("不是 json".into())); // 降级臂首轮也不合法 → 回灌重试
            json_script.push(Ok(verdict_json("5.0", &["缺失要点"])));
        }
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert!(outcome.thinking_degraded);
        assert_eq!(outcome.verdict.score, 5.0);
        assert_eq!(canned.users("thinking").len(), 1, "thinking 通道只探测一次");
        assert_eq!(canned.users("json").len(), 2, "降级后两次尝试都在 json 通道");
    }

    #[tokio::test]
    async fn reflection_scale_is_validated_on_0_1() {
        let input = OpenJudgeInput {
            kind: ActivityKind::Reflection,
            ..open_input("反思判底")
        };
        // 0–10 形态在 reflection 档越界（三轮同答复现，耗尽原因 = 越界）。
        let canned = Canned::thinking(vec![
            Ok(verdict_json("7.5", &[])),
            Ok(verdict_json("7.5", &[])),
            Ok(verdict_json("7.5", &[])),
        ]);
        let err = judge(&canned, &input).await.unwrap_err();
        assert!(err.last.contains("between 0 and 1"), "{}", err.last);

        // 0–1 形态 + 判错缺缺失要点 → 互斥打回后取合法结论。
        let canned = Canned::thinking(vec![
            Ok(verdict_json("0.4", &[])),
            Ok(verdict_json("0.4", &["缺了旧认识的对照"])),
        ]);
        let outcome = judge(&canned, &input).await.unwrap();
        assert_eq!(outcome.verdict.score, 0.4);
        assert_eq!(
            outcome.verdict.missing_points,
            vec!["缺了旧认识的对照".to_string()]
        );
    }

    #[tokio::test]
    async fn legacy_empty_explanation_assembles_the_tolerance_placeholder() {
        // 存量空 explanation 行容错（n4 D1(a)）：Some("") → 装配「以课时
        // 上下文为准」占位，评审照走。
        let canned = Canned::thinking(vec![Ok(verdict_json("7.0", &[]))]);
        judge(&canned, &open_input("   ")).await.unwrap();
        let users = canned.users("thinking");
        assert!(
            users[0].contains("(无：以课时上下文为准)"),
            "空判底必须装配容错占位：{}",
            users[0]
        );
    }

    #[tokio::test]
    async fn exact_threshold_score_counts_as_passing_for_the_contract() {
        // 6.0 恰在及格线上 → 判对侧互斥生效（缺失非空即打回）。
        let canned = Canned::thinking(vec![
            Ok(verdict_json("6.0", &["边界要点"])),
            Ok(verdict_json("6.0", &[])),
        ]);
        let outcome = judge(&canned, &open_input("判底")).await.unwrap();
        assert_eq!(outcome.verdict.score, 6.0);
        assert!(outcome.verdict.missing_points.is_empty());
    }
}
