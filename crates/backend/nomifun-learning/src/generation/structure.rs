//! 证据驱动的结构发现(t2):在 t1 清点/分诊之上,从目标生成多角度检索
//! 查询,对全量候选库检索并均衡汇总证据,再以「草稿 → 评审 → 修订」三段
//! LLM 产出 `concept_graph + chapters` 的 [`SourcePlan`],最后用确定性后处理
//! (DFS 去环、Kahn 拓扑、Jaccard 覆盖补齐、Overview 兜底)与覆盖对账
//! (heading/paragraph/path 级 topic)收口。它是后续分批展开(t3)的权威
//! 章节骨架,不是最终课程大纲。
//!
//! 失败策略:无 completer / 草稿连续失败 → `Ok(None)`(调用方回退现行
//! 采样路径);零证据 → 降级空章节 plan(gaps 非空,不伪造章节);只有
//! 零候选/零 core 硬失败。取消沿用 `CancellationToken`(`CancellationCompleter`
//! 包装 + 阶段间检查点)。

use super::*;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// 检索查询条数区间(票面:4–8 条多角度)。
pub(crate) const STRUCTURE_QUERY_MIN: usize = 4;
pub(crate) const STRUCTURE_QUERY_MAX: usize = 8;
/// 检索并发上限(票面建议 ≤6)。
pub(crate) const STRUCTURE_SEARCH_CONCURRENCY: usize = 6;
/// 检索总调用上限(票面建议 ≤48,query × kb 计数)。
pub(crate) const STRUCTURE_SEARCH_MAX_CALLS: usize = 48;
/// 单次 `search_bases` 调用的命中条数上限。
pub(crate) const STRUCTURE_SEARCH_PER_CALL_LIMIT: usize = 8;
/// 均衡后的证据 chunk 总量上限(轮询取样预算)。
pub(crate) const STRUCTURE_EVIDENCE_MAX_CHUNKS: usize = 24;
/// 草稿阶段的尝试上限(解析失败反馈重试,同指纹早停)。
pub(crate) const STRUCTURE_PLAN_MAX_ATTEMPTS: usize = 3;
/// 评审轮上限(草稿后 critique→revise→复评,复评同指纹即停)。
pub(crate) const STRUCTURE_CRITIQUE_MAX_ROUNDS: usize = 2;
/// DFS 去环的丢边上限(票面 ≤20 次)。
pub(crate) const STRUCTURE_CYCLE_BREAK_MAX_EDGES: usize = 20;
/// 章节数量上限(输出面有界;超出丢尾部 + warning)。
pub(crate) const STRUCTURE_MAX_CHAPTERS: usize = 48;

// ── 计划类型(serde 友好;t4 落库保持字段语义稳定)──────────────────────

/// 章节来源锚点:定位顺序 heading → paragraph → path 级(标
/// `anchor_uncertain` + warning,不编造标题)。`rel_path` 与采样/清点同口径
/// (多库 `{库名}/rel` 引用路径)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceAnchor {
    pub kb_id: String,
    pub rel_path: String,
    /// 文件内标题(H1-H3 原文);headingless 文件为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_or_anchor: Option<String>,
    /// 段落序号(1 起,headingless 文件的定位锚)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph: Option<usize>,
    /// 证据片段(≤120 字,可回原文)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// 无法精确定位(path 级兜底或模型给的标题对不上原文)。
    #[serde(default)]
    pub anchor_uncertain: bool,
}

/// 概念图边关系:`depends_on` 的 src 依赖 dst(dst 是前置,应先教)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptEdgeRelation {
    DependsOn,
    Extends,
    Related,
}

/// 概念图节点(候选概念原子化、可复用)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptGraphNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub weight: f32,
}

/// 概念图边(带 rationale 作置信度代理)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptGraphEdge {
    pub src: String,
    pub dst: String,
    pub relation: ConceptEdgeRelation,
    #[serde(default)]
    pub rationale: String,
}

/// 概念图(节点 + 边;只约束 `depends_on` 的顺序)。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptGraph {
    #[serde(default)]
    pub nodes: Vec<ConceptGraphNode>,
    #[serde(default)]
    pub edges: Vec<ConceptGraphEdge>,
}

/// 章节内容类型(concept 正文 / practice 练习 / reference 参考)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChapterContentType {
    #[default]
    Concept,
    Practice,
    Reference,
}

/// 计划章节:权威骨架的一章(t3 在章节内展开 lesson 粒度)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlannedChapter {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub learning_objectives: Vec<String>,
    /// 覆盖的概念 id(后处理保证每个概念至少被一章覆盖)。
    #[serde(default)]
    pub covers: Vec<String>,
    /// 前置章标题(概念依赖提升 + 模型声明,拓扑排序校验)。
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub source_anchors: Vec<SourceAnchor>,
    #[serde(default)]
    pub content_type: ChapterContentType,
}

/// topic 对账状态:covered(被某章锚点覆盖)/ skipped(显式跳过 + 理由)/
/// uncertain(保留可见)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    Covered,
    Skipped,
    Uncertain,
}

/// 文件级对账状态(由 topic 状态聚合;文件只是锚载体,不是覆盖单位)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCoverageStatus {
    Covered,
    Partial,
    Skipped,
    Uncertain,
}

/// 单个 topic(heading/paragraph/path 级)的对账条目。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageTopic {
    pub topic_id: String,
    pub label: String,
    #[serde(default)]
    pub source_anchors: Vec<SourceAnchor>,
    #[serde(default)]
    pub assigned_chapters: Vec<String>,
    pub status: TopicStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 单文件对账(角色来自 t1,章节归属由锚点聚合)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageFile {
    pub rel_path: String,
    pub role: crate::generation::KbTriageRole,
    #[serde(default)]
    pub assigned_chapters: Vec<String>,
    pub status: FileCoverageStatus,
}

/// 被排除出计划的文件(noise/duplicate,必须能回答「为什么没进课程」)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageExclusion {
    pub rel_path: String,
    pub role: crate::generation::KbTriageRole,
    pub reason: String,
}

/// 覆盖对账台账。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoverageLedger {
    #[serde(default)]
    pub topics: Vec<CoverageTopic>,
    #[serde(default)]
    pub files: Vec<CoverageFile>,
    #[serde(default)]
    pub excluded: Vec<CoverageExclusion>,
    #[serde(default)]
    pub unresolved: Vec<String>,
}

/// 逐来源检索贡献读数(0 贡献显式 warning)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceCount {
    pub kb_id: String,
    pub kb_name: String,
    pub chunks: usize,
}

/// 证据摘要(检索均衡后的压缩;查询语言跟源材料走)。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceEvidence {
    #[serde(default)]
    pub queries: Vec<String>,
    #[serde(default)]
    pub source_counts: Vec<SourceCount>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub gaps: Vec<String>,
    /// 候选概念(8-20,原子化)。
    #[serde(default)]
    pub candidate_concepts: Vec<String>,
    /// 补充观察(≤5)。
    #[serde(default)]
    pub notes: Vec<String>,
    /// 成本读数:候选文件数与检索调用的计划/实际数(t5 估算门消费)。
    #[serde(default)]
    pub candidate_files: usize,
    #[serde(default)]
    pub planned_search_calls: usize,
    #[serde(default)]
    pub executed_search_calls: usize,
    /// 查询指纹(同库同目标重跑可对比)。
    #[serde(default)]
    pub queries_hash: String,
    /// 证据指纹(查询 + 来源计数 + 摘要)。
    #[serde(default)]
    pub evidence_hash: String,
    /// 候选文件内容指纹聚合(inventory content_hash,按引用路径序)。
    #[serde(default)]
    pub source_fingerprint: String,
}

/// 证据驱动的权威章节骨架(t2 交付;t3 按章节展开 lesson,t4 落库)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourcePlan {
    pub goal: String,
    #[serde(default)]
    pub evidence: SourceEvidence,
    #[serde(default)]
    pub concept_graph: ConceptGraph,
    #[serde(default)]
    pub chapters: Vec<PlannedChapter>,
    #[serde(default)]
    pub coverage: CoverageLedger,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// 任一阶段从 LLM 降级到确定性规则(仍可用;空章节 = 不可用,调用方回退)。
    #[serde(default)]
    pub degraded: bool,
    /// 计划指纹(goal + evidence_hash + 章节/概念骨架)。
    #[serde(default)]
    pub plan_hash: String,
}

impl SourcePlan {
    /// 计划可用性:有章节即可用(降级计划带着 warning 照常建课;零章节 =
    /// 零证据或草稿失败,调用方回退现行采样路径)。
    pub fn is_usable(&self) -> bool {
        !self.chapters.is_empty()
    }

    /// 简报/`co_start` 的一行披露(章节数 + 章节清单 + 覆盖/降级读数)。
    pub fn brief_line(&self) -> String {
        let titles: Vec<&str> = self.chapters.iter().map(|c| c.title.as_str()).collect();
        let covered = self
            .coverage
            .topics
            .iter()
            .filter(|t| t.status == TopicStatus::Covered)
            .count();
        format!(
            "共 {} 章{}: {}; 概念 {} 个, topic 覆盖 {}/{}, 来源查询 {} 条, 检索调用 {}/{}{}",
            self.chapters.len(),
            if self.degraded { "(降级)" } else { "" },
            titles.join(" → "),
            self.concept_graph.nodes.len(),
            covered,
            self.coverage.topics.len(),
            self.evidence.queries.len(),
            self.evidence.executed_search_calls,
            self.evidence.planned_search_calls,
            if self.warnings.is_empty() {
                String::new()
            } else {
                format!(", warnings {}", self.warnings.len())
            },
        )
    }
}

// ── 确定性后处理(纯函数,单测直测)────────────────────────────────────

pub(crate) mod postprocess;
pub(crate) mod topics;

#[cfg(test)]
mod tests;
