//! LLM seam for the learning generation pipeline — the learning module's own
//! completer trait, mirroring `KnowledgeCompleter` (nomifun-knowledge) and
//! `CompanionCompleter` (nomifun-companion). The learning crate holds only the
//! trait; provider/model resolution and the actual completion call are the
//! implementor's concern (production: `LiveLearningCompleter` in
//! nomifun-ai-agent).
//!
//! Unlike `KnowledgeCompleter`, the signature carries an explicit output
//! budget (`max_tokens`): the learning pipeline's calls differ wildly in
//! output size — a tiny grading JSON vs. a whole concept graph in one reply —
//! so every stage passes the budget it actually needs instead of a
//! one-size-fits-all cap.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use nomifun_common::AppError;

/// 一次 LLM 调用的用量增量（e1 相位耗时读数面）：任务体的计数包装（学习侧
/// [`crate::service::CountingCompleter`] / 课时侧 agent 的 CountingProvider）
/// 在每次调用后上报；长任务注册表在 finish 行聚合落档。字符量口径 =
/// `chars().count()` 全字符（含空白）——读数是体量代理，不是计费口径。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LlmUsageDelta {
    pub llm_calls: u64,
    pub input_chars: u64,
    pub output_chars: u64,
}

/// 长任务 LLM 用量累计句柄（e1 相位耗时读数）：任务 begin 时铸造、guard 持
/// 有一份，任务体的计数包装经 [`Self::add`] 上报，注册表 finish 时取
/// [`Self::snapshot`]。Clone 廉价（Arc 内核）；原子面 = 多调用并发安全
/// （e4 段写并行的前置）。
#[derive(Clone, Default)]
pub struct LlmUsageMeter {
    totals: Arc<LlmUsageTotals>,
}

#[derive(Default)]
struct LlmUsageTotals {
    llm_calls: AtomicU64,
    input_chars: AtomicU64,
    output_chars: AtomicU64,
}

impl LlmUsageMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记一批用量增量（并发安全；零增量无开销差异）。
    pub fn add(&self, delta: LlmUsageDelta) {
        self.totals
            .llm_calls
            .fetch_add(delta.llm_calls, AtomicOrdering::Relaxed);
        self.totals
            .input_chars
            .fetch_add(delta.input_chars, AtomicOrdering::Relaxed);
        self.totals
            .output_chars
            .fetch_add(delta.output_chars, AtomicOrdering::Relaxed);
    }

    /// 当前累计快照。
    pub fn snapshot(&self) -> LlmUsageDelta {
        LlmUsageDelta {
            llm_calls: self.totals.llm_calls.load(AtomicOrdering::Relaxed),
            input_chars: self.totals.input_chars.load(AtomicOrdering::Relaxed),
            output_chars: self.totals.output_chars.load(AtomicOrdering::Relaxed),
        }
    }
}

impl std::fmt::Debug for LlmUsageMeter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let usage = self.snapshot();
        formatter
            .debug_struct("LlmUsageMeter")
            .field("llm_calls", &usage.llm_calls)
            .field("input_chars", &usage.input_chars)
            .field("output_chars", &usage.output_chars)
            .finish()
    }
}

#[async_trait::async_trait]
pub trait LearningCompleter: Send + Sync {
    /// One completion call. `model_override` is the caller's explicit
    /// `(provider_id, model)` pick when the user chose one (the two are
    /// always provided together, enforced at the HTTP layer); `None` leaves
    /// the model choice to the implementor's default.
    async fn complete(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError>;

    /// Judgment-heavy planning call (learning-graph coach): thinking stays ON
    /// so the model can actually deliberate (survey entry domains, descend the
    /// prerequisite ladder) before emitting the JSON contract; the drain still
    /// prefers `content` and only falls back to the reasoning channel when the
    /// provider leaves it empty. The default delegates to [`complete`] so test
    /// doubles and conservative implementors keep working.
    async fn complete_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        self.complete(model_override, system, user, max_tokens).await
    }

    /// JSON-mode call: the provider is asked to guarantee a syntactically
    /// valid JSON object reply (`response_format: json_object`), which is the
    /// legality floor of every structured-output contract in this crate — the
    /// local schema validator judges shape, never legality. There is no
    /// text-mode fallback: the default implementation fails loudly, and so
    /// does every implementor whose provider lacks the mode. Structure and
    /// terrain generation (the artifact contracts) build on this seam.
    /// `temperature` 来自提示词资产注册表（`PromptSpec`；None = provider 默认）。
    async fn complete_json(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        let _ = (model_override, system, user, max_tokens, temperature);
        Err(AppError::Conflict(
            "learning structured output unavailable: this completer does not support json mode"
                .into(),
        ))
    }

    /// JSON-mode sibling of [`complete_with_thinking`]: artifact generation
    /// (structure + terrain) deliberates before emitting its contract JSON,
    /// so thinking stays ON and the budget covers the reasoning channel too.
    /// The default delegates to [`complete_json`].
    async fn complete_json_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        self.complete_json(model_override, system, user, max_tokens, temperature)
            .await
    }
}
