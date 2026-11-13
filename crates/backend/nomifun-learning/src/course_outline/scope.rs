//! 传统课程大纲的范围分析（原 `learning_graph::analyze_scope` 整体迁移）：
//! 大块概念清单 + 目标/基线/范围界定，供 outline draft 的覆盖率账使用。
//!
//! 迁移出处：wayfinder 学习模块吸收 effort 的 g3/g4 裁决退役了**学习图侧**
//! 的范围参考（教练上下文由结构参考材料取代），但传统课程的 agent 大纲管线
//! （`course_outline`）仍在消费同一分析（scope blocks 对模块/课时标题的覆盖
//! 检查）——裁决的两个消费方清单漏列了它。按「不回归既有功能」处理：语义
//! 原样搬到本模块，学习图侧不再引用。

use serde::Deserialize;

use crate::completer::LearningCompleter;

/// Scope call: ONE light call that resolves what the goal description
/// actually covers. The output is REFERENCE material for the outline audit;
/// failure degrades to a scope-free start, never to a hard error.
const SCOPE_SYSTEM: &str = r#"你负责在学习目标开始生长之前，先厘清这个目标到底覆盖什么。
只回复一个 JSON 对象，形状如下：
{
  "goal": "完成整个学习后应达到的最终状态——可检验的能力描述",
  "baseline": "学习者被假定的起点状态",
  "scope": "一句话界定该目标覆盖什么、从哪里开始",
  "blocks": ["大块概念一", "大块概念二"]
}
规则：
- "goal"：明确的学习目标。从学习目标描述中提炼学习者完成后能做到什么、理解到什么程度——写成可检验的能力陈述，而不是重复用户的原话。
- "baseline"：用户起点。当学习者基线不明且没有明确要求起点时，一律视作用户对目标相关领域彻底的一无所知；只有用户明确说出自己已具备的知识或技能时，才照实记录。
- "scope"：一句话划清目标的边界——起点、要达到的水平、主题广度，须与 goal 和 baseline 保持一致。
- "blocks"：该目标真正覆盖的大块概念，按从基础到高级排序，合起来必须铺满从 baseline 到 goal 的整条路径。数量不固定：复杂的目标多列，简单的目标少列。
- 用学习目标的语言书写。
- 只输出 JSON，不要 Markdown 代码块，不要任何解释。"#;

/// Resolved scope reference consumed by the outline draft audit.
#[derive(Debug, Clone, Default)]
pub struct ScopeAnalysis {
    pub goal: String,
    pub baseline: String,
    pub scope: String,
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct RawScope {
    #[serde(default)]
    goal: String,
    #[serde(default)]
    baseline: String,
    #[serde(default)]
    scope: String,
    #[serde(default, deserialize_with = "de_string_list")]
    blocks: Vec<String>,
}

fn de_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<OneOrMany>::deserialize(deserializer)? {
        Some(OneOrMany::One(one)) => vec![one],
        Some(OneOrMany::Many(many)) => many,
        None => Vec::new(),
    })
}
fn parse_scope_reply(raw: &str) -> Option<ScopeAnalysis> {
    let parsed = crate::generation::parse_json_object::<RawScope>(raw).ok()?;
    Some(ScopeAnalysis {
        goal: parsed.goal.trim().to_owned(),
        baseline: parsed.baseline.trim().to_owned(),
        scope: parsed.scope.trim().to_owned(),
        blocks: parsed.blocks,
    })
}

/// One scope call, best-effort: any failure degrades to `None`.
pub(crate) async fn analyze_scope(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    topic: &str,
) -> Option<ScopeAnalysis> {
    let user = format!("Learning goal: {topic}");
    let raw = crate::generation::complete(
        completer,
        model_override,
        SCOPE_SYSTEM,
        &user,
        crate::generation::OUTLINE_SCOPE_MAX_TOKENS,
    )
    .await
    .ok()?;
    parse_scope_reply(&raw)
}
