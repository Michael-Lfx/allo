//! Course outline draft + deterministic audit + agent engine seam. The
//! learning crate owns the draft vocabulary the `co_*` agent tools edit, the
//! deterministic audit gate that has the last word on publishing, and the
//! engine trait; the two-loop agent engine itself lives in nomifun-ai-agent.
//! The audit vocabulary below is shared with the lesson draft's audit.

pub mod draft;
pub(crate) mod scope;

use serde::{Deserialize, Serialize};

use crate::generation::Blueprint;

// ── Shared audit vocabulary ────────────────────────────────────────────────

pub(crate) const SEV_INFO: &str = "info";
pub(crate) const SEV_WARNING: &str = "warning";
pub(crate) const SEV_DANGER: &str = "danger";

/// Scope blocks are large-block themes (broad nouns / theme phrases); a
/// title covers a block when it shares at least this many consecutive
/// characters (block words run 2-6 chars, so 2 is a deliberately weak bar).
pub(crate) const BLOCK_MIN_SHARED: usize = 2;

/// One deterministic audit finding. `severity` is one of [`SEV_INFO`] /
/// [`SEV_WARNING`] / [`SEV_DANGER`]; DANGER findings block publishing and
/// drive the repair loop. `node_ids` carries the evidence keys so reports
/// can point at the offending entities.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AuditFinding {
    pub kind: String,
    pub severity: String,
    pub message: String,
    pub node_ids: Vec<String>,
}

/// Length (in chars) of the longest common substring of `a` and `b` — the
/// weak substring-overlap measure behind the scope-block coverage check and
/// the closest-candidate hints.
pub(crate) fn common_substring_len(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut best = 0usize;
    for i in 0..a.len() {
        for j in 0..b.len() {
            let mut len = 0;
            while i + len < a.len() && j + len < b.len() && a[i + len] == b[j + len] {
                len += 1;
            }
            best = best.max(len);
        }
    }
    best
}

/// Knowledge-base context resolved before the engine starts: the base's
/// name and description are fetched up front, so the engine never touches
/// the knowledge service itself — the sampled files ride on the draft and
/// the agent reads them through the `co_read` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBaseBrief {
    pub kb_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// The generation brief handed to [`CourseOutlineAgentEngine`]: exactly one
/// source — a free-text course description or a resolved knowledge base.
/// Sized like `GenerateCourseRequest` so both flows share one engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineBrief {
    /// Description flow: the whole grounding. Mutually exclusive with
    /// `knowledge_base`.
    #[serde(default)]
    pub description: Option<String>,
    /// kb flow: the resolved base context.
    #[serde(default)]
    pub knowledge_base: Option<KnowledgeBaseBrief>,
    /// 多知识库（t4）：全量来源库上下文（首个 = 主来源 = `knowledge_base`）。
    /// 单库时为空（与 `knowledge_base` 同值语义，避免同一信息两处漂移）。
    #[serde(default)]
    pub knowledge_bases: Vec<KnowledgeBaseBrief>,
    /// kb flow: the sampled `(path, excerpt)` corpus, resolved by the
    /// synchronous pipeline BEFORE the engine starts (sampling is local
    /// file IO, milliseconds). The draft reads them via `co_read`; the
    /// description flow leaves this empty. Carried on the brief so the
    /// engine trait needs exactly one context argument.
    #[serde(default)]
    pub samples: Vec<(String, String)>,
    #[serde(default)]
    pub domain: Option<String>,
    /// 用户目标（t1）：知识库流的整体目标文本——简单时即课程名，复杂时是
    /// 目标描述/长说明。旧客户端/描述流缺省为 `None`（主题回退库名与描述，
    /// 行为与旧路径完全一致）。
    #[serde(default)]
    pub goal: Option<String>,
    /// 目标感知采样的披露快照（t2）：完整文件树/已采未采计数/低分未采清单。
    /// 无目标路径（字母序回退）为 `None`。
    #[serde(default)]
    pub sampling: Option<crate::generation::SampleDisclosure>,
}

impl OutlineBrief {
    /// Which source this brief carries — the event payloads' `kind` field.
    pub fn kind(&self) -> &'static str {
        if self.description.is_some() {
            "description"
        } else {
            "knowledge_base"
        }
    }

    /// 目标派生的课程标题（t1 裁决：课程名默认即目标文本，不引入 LLM 派名）：
    /// 归一单行 + 限长；无目标或空白目标返回 `None`（调用方沿用模型标题）。
    pub fn goal_title(&self) -> Option<String> {
        let title = crate::models::goal_course_title(self.goal.as_deref()?);
        (!title.is_empty()).then_some(title)
    }

    /// 全量来源库（t4）：多库走 `knowledge_bases`，单库回退
    /// `knowledge_base`——调用方不必区分两者（顺序 = 用户选择顺序）。
    pub fn source_knowledge_bases(&self) -> Vec<&KnowledgeBaseBrief> {
        if self.knowledge_bases.is_empty() {
            self.knowledge_base.iter().collect()
        } else {
            self.knowledge_bases.iter().collect()
        }
    }
}

/// Agent-driven course outline generation seam — mirrors
/// [`crate::learning_graph::LearningGraphAgentEngine`]: the learning crate
/// holds only the trait; the two-loop agent engine is implemented in
/// nomifun-ai-agent. When injected, `generate_course` routes through it
/// (draft + `co_*` tools, audit-gated publish) instead of the legacy
/// one-shot pipeline, which stays as the fallback for tests and direct
/// calls.
#[async_trait::async_trait]
pub trait CourseOutlineAgentEngine: Send + Sync {
    /// Run the outline agent loop; returns the audit-gated blueprint.
    async fn generate(
        &self,
        brief: &OutlineBrief,
        model_override: Option<(&str, &str)>,
    ) -> Result<Blueprint, nomifun_common::AppError>;
}
