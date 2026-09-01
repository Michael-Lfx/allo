//! Experimental concept-graph feature: decompose a broad learning goal into a
//! network of LEARNING UNITS linked by task-dependency edges — a complete DAG.
//! A unit is one human study session, usually within 30 minutes (soft cap); a
//! genuinely hard single lesson may go up to 60 (hard cap). Unit names are
//! action sentences ("用配方法解一元二次方程"), never concept nouns or whole
//! sub-domains ("概率基础" is meaningless as a unit).
//!
//! Generation runs EXCLUSIVELY through the agent tool loop
//! ([`ConceptGraphAgentEngine`], implemented in nomifun-ai-agent): a scope
//! call first resolves the coverage checklist, then the agent builds the
//! network step by step with the `cg_*` draft tools ([`crate::concept_graph::draft`])
//! and publishes only through the deterministic audit gate
//! ([`crate::concept_graph::audit`]). The former one-shot generation +
//! auto-repair pipeline has been retired; this module keeps the shared
//! symbolic kernel (normalization, fuzzy reference resolution, cycle
//! removal, merge), the scope analysis and the audit report renderer the
//! agent path reuses.
//!
//! Published graphs are persisted by [`crate::service::LearningService`] as
//! JSON files so the UI can revisit them without regenerating.

use std::collections::{HashMap, HashSet};

use nomifun_common::{AppError, UserId};
use serde::{Deserialize, Serialize};

use crate::completer::LearningCompleter;

mod audit;
pub mod draft;
mod log;

pub(crate) use audit::{
    common_substring_len, audit_concept_graph, audit_concept_graph_with_scope, BLOCK_MIN_SHARED,
    SEV_DANGER, SEV_INFO, SEV_WARNING,
};
pub(crate) use log::ConceptGraphLogger;

/// One node in the graph — a LEARNING UNIT: one human study session,
/// usually within 30 minutes (soft cap), at most 60 for a genuinely hard
/// single lesson. The name is an action sentence describing what the
/// learner does in the session ("用配方法解一元二次方程"), never a concept
/// noun. `min` carries the estimated workload; the audit enforces the caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptGraphNode {
    pub id: String,
    pub title: String,
    /// Estimated study time in minutes (the prompt asks for 5-minute steps
    /// up to the 30-minute soft cap; the audit warns above 30 and treats
    /// any value above 60 as a hard-cap violation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u16>,
    /// Sub-domain group label (legacy field, never set by new graphs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Why the concept is indispensable ("缺了它不行").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub necessity: Option<String>,
    /// Group entry-point concept; cross-group references may only point here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_anchor: Option<bool>,
}

/// A prerequisite edge: `from` should be mastered before `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptGraphEdge {
    pub from: String,
    pub to: String,
    /// Why `from` must precede `to` (model-provided, optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Deterministic structural audit report attached to a stored graph.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConceptGraphAudit {
    /// References the model emitted that no node satisfied — the missing
    /// concept proxy. Counts unknown references, self loops, duplicates and
    /// cycle edges dropped during normalization/merge.
    #[serde(default)]
    pub ref_drop_count: usize,
    /// Total prerequisite entries the model emitted, accumulated across the
    /// initial generation and every repair merge — the drop-rate
    /// denominator. Persisted so a merged graph keeps an honest rate
    /// instead of resetting to the last patch batch's own statistics.
    #[serde(default)]
    pub raw_ref_count: usize,
    /// `ref_drop_count` over `raw_ref_count`.
    #[serde(default)]
    pub ref_drop_rate: f64,
    /// Every dropped reference with its reason, so the report is evidence-led.
    #[serde(default)]
    pub dropped_edges: Vec<DroppedEdge>,
    /// Structural findings from deterministic scripts (Stage 3).
    #[serde(default)]
    pub findings: Vec<AuditFinding>,
}

/// One reference the pipeline dropped, with the reason (unknown reference,
/// self loop, duplicate edge, cycle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedEdge {
    pub from: String,
    pub to: String,
    pub reason: String,
}

/// One structural finding. `kind` is a stable machine-readable label the UI
/// and the repair endpoint key on; `severity` is "info" | "warning" | "danger".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditFinding {
    pub kind: String,
    pub severity: String,
    pub message: String,
    /// Node ids serving as evidence (possibly empty).
    #[serde(default)]
    pub node_ids: Vec<String>,
}

/// The validated, cycle-free DAG payload shared by storage and API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptGraphData {
    pub nodes: Vec<ConceptGraphNode>,
    pub edges: Vec<ConceptGraphEdge>,
    #[serde(default)]
    pub audit: ConceptGraphAudit,
}

/// A stored concept graph as returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptGraphRecord {
    pub id: String,
    pub user_id: String,
    pub topic: String,
    #[serde(flatten)]
    pub graph: ConceptGraphData,
    pub created_at: i64,
}

/// Agent-driven concept graph generation seam — mirrors [`LearningCompleter`]:
/// the learning crate holds only the trait; the two-loop agent engine is
/// implemented in nomifun-ai-agent. When injected, `generate_concept_graph`
/// routes through the agent tool set (draft + `cg_*` tools, audit-gated
/// publish) instead of the one-shot legacy pipeline, which stays as the
/// fallback so tests and direct calls keep working unconfigured.
#[async_trait::async_trait]
pub trait ConceptGraphAgentEngine: Send + Sync {
    /// Run the two-loop agent generation; returns the published record.
    async fn generate(
        &self,
        user_id: &UserId,
        topic: &str,
        model_override: Option<(&str, &str)>,
    ) -> Result<ConceptGraphRecord, AppError>;
}

/// List entry without the full node/edge payload.
#[derive(Debug, Clone, Serialize)]
pub struct ConceptGraphSummary {
    pub id: String,
    pub topic: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub created_at: i64,
}

impl ConceptGraphRecord {
    pub fn summary(&self) -> ConceptGraphSummary {
        ConceptGraphSummary {
            id: self.id.clone(),
            topic: self.topic.clone(),
            node_count: self.graph.nodes.len(),
            edge_count: self.graph.edges.len(),
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateConceptGraphRequest {
    pub topic: String,
    #[serde(default)]
    pub provider_id: Option<nomifun_common::ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Tolerate `"min": "15"` (or `null`/absence/non-numeric) where the shape
/// asks for a number; an unusable value becomes `None` rather than failing
/// the whole reply.
fn de_min<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(u16),
        Str(String),
    }
    Ok(match Option::<NumOrStr>::deserialize(deserializer)? {
        Some(NumOrStr::Num(n)) => Some(n),
        Some(NumOrStr::Str(s)) => s.trim().parse::<u16>().ok(),
        None => None,
    })
}

// ── Raw model output types ────────────────────────────────────────────────

/// Raw per-unit model output — deliberately symbolic and minimal, the same
/// shape as a hand-maintained YAML graph: a unit ACTION NAME plus its direct
/// dependency names plus an optional minute budget. The program derives ids,
/// edges, and all optional fields, and tolerates the usual LLM habits (a
/// single string where an array is expected, duplicate names, unknown
/// references, cycles, non-numeric minutes).
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct RawConcept {
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "de_pre_list")]
    pub pre: Vec<String>,
    /// Estimated study minutes; tolerated as a number or a digit string.
    #[serde(default, deserialize_with = "de_min")]
    pub min: Option<u16>,
}

/// Tolerate `"pre": "single name"` (or `null`/absence) where the shape asks
/// for an array.
fn de_pre_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
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

// ── Shared normalization ───────────────────────────────────────────────────

/// A normalized batch of concepts: legal nodes and edges plus drop
/// statistics. Used by the generation loop and the repair stage.
#[derive(Debug, Clone)]
pub(crate) struct NormalizedBatch {
    pub nodes: Vec<ConceptGraphNode>,
    /// Edges whose references resolved; may still contain cycles (removed
    /// by [`finalize_graph`]).
    pub edges: Vec<ConceptGraphEdge>,
    /// Total prerequisite entries the model emitted (drop-rate denominator).
    pub raw_refs: usize,
    /// References dropped during normalization (unknown/self/duplicate).
    pub dropped: Vec<DroppedEdge>,
    /// Near-miss references resolved to their unique nearest unit — kept as
    /// (emitted, resolved) pairs for the diagnosis log.
    pub fuzzy_resolved: Vec<(String, String)>,
}

/// Repair and validate a batch of raw concepts into nodes and legal edges:
/// - the concept name IS its id and title (symbolic, like a YAML graph);
/// - drops concepts with an empty name, deduplicates names (first wins);
/// - drops prerequisite references outside the batch (batch keys are always
///   allowed) plus `allowed` (repair: existing graph keys), self loops, and
///   duplicate edges, counting each drop.
pub(crate) fn normalize_batch(
    raw: &[RawConcept],
    allowed: &HashSet<String>,
) -> NormalizedBatch {
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut kept: Vec<bool> = Vec::with_capacity(raw.len());
    let mut nodes: Vec<ConceptGraphNode> = Vec::new();
    let mut raw_refs = 0usize;
    for concept in raw {
        raw_refs += concept.pre.len();
        let name = concept.name.trim();
        let is_kept = !name.is_empty() && seen_names.insert(name.to_owned());
        kept.push(is_kept);
        if !is_kept {
            continue;
        }
        nodes.push(ConceptGraphNode {
            id: name.to_owned(),
            title: name.to_owned(),
            min: concept.min,
            group: None,
            necessity: None,
            is_anchor: None,
        });
    }

    let mut seen_edges: HashSet<(String, String)> = HashSet::new();
    let mut edges: Vec<ConceptGraphEdge> = Vec::new();
    let mut dropped: Vec<DroppedEdge> = Vec::new();
    let mut fuzzy_resolved: Vec<(String, String)> = Vec::new();
    for (concept, is_kept) in raw.iter().zip(&kept) {
        let to = concept.name.trim();
        if !is_kept || !seen_names.contains(to) {
            continue;
        }
        for prereq in &concept.pre {
            let from = prereq.trim();
            if from.is_empty() {
                dropped.push(DroppedEdge {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    reason: "empty reference".into(),
                });
                continue;
            }
            // Exact reference first; a near-miss name (one insertion or
            // deletion away from exactly one defined unit) resolves instead
            // of dropping — a single dropped reference can orphan a whole
            // subtree and turn its head into a fake entry point.
            let resolved = if seen_names.contains(from) || allowed.contains(from) {
                from.to_owned()
            } else {
                match fuzzy_resolve_reference(from, &seen_names, allowed) {
                    Some(name) => {
                        fuzzy_resolved.push((from.to_owned(), name.clone()));
                        name
                    }
                    None => {
                        dropped.push(DroppedEdge {
                            from: from.to_owned(),
                            to: to.to_owned(),
                            reason: "unknown reference".into(),
                        });
                        continue;
                    }
                }
            };
            if resolved == to {
                dropped.push(DroppedEdge {
                    from: resolved,
                    to: to.to_owned(),
                    reason: "self loop".into(),
                });
                continue;
            }
            if !seen_edges.insert((resolved.clone(), to.to_owned())) {
                dropped.push(DroppedEdge {
                    from: resolved,
                    to: to.to_owned(),
                    reason: "duplicate edge".into(),
                });
                continue;
            }
            edges.push(ConceptGraphEdge {
                from: resolved,
                to: to.to_owned(),
                reason: None,
            });
        }
    }
    NormalizedBatch {
        nodes,
        edges,
        raw_refs,
        dropped,
        fuzzy_resolved,
    }
}

/// Fuzzy reference resolution ceiling: a "pre" entry that missed exact
/// lookup resolves to a defined unit when it sits within this many
/// insertions/deletions of EXACTLY ONE candidate. One, not two: a
/// substitution (一元一次 vs 一元二次) costs two indel steps and must never
/// resolve — swapped characters usually mark a genuinely different unit,
/// while a stray function word or a repeated character is a slip.
const FUZZY_REF_MAX_DISTANCE: usize = 1;

/// Resolve a near-miss "pre" name against the batch keys plus the allowlist:
/// the unique nearest candidate within [`FUZZY_REF_MAX_DISTANCE`]
/// insertions/deletions wins; a tie resolves to nothing (the model meant
/// something between the candidates, and guessing would mis-wire the edge).
/// Also used by the repair stage to resolve link/reverse/split/merge
/// endpoints — a repair model copying names out of a 100+-unit list slips
/// exactly the way the generator does.
pub(crate) fn fuzzy_resolve_reference(
    reference: &str,
    names: &HashSet<String>,
    allowed: &HashSet<String>,
) -> Option<String> {
    let reference: Vec<char> = reference.chars().collect();
    let mut best: Option<(usize, String)> = None;
    let mut tie = false;
    for candidate in names.iter().chain(allowed.iter()) {
        let distance = indel_distance(&reference, &candidate.chars().collect::<Vec<_>>());
        if distance > FUZZY_REF_MAX_DISTANCE {
            continue;
        }
        match &best {
            Some((best_distance, _)) if *best_distance < distance => {}
            Some((best_distance, best_name)) if best_distance == &distance => {
                if best_name != candidate {
                    tie = true;
                }
            }
            _ => {
                best = Some((distance, candidate.clone()));
                tie = false;
            }
        }
    }
    if tie {
        None
    } else {
        best.map(|(_, name)| name)
    }
}

/// Insert/delete edit distance (a substitution costs two): the number of
/// character insertions and deletions turning `a` into `b`.
fn indel_distance(a: &[char], b: &[char]) -> usize {
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            lcs[i][j] = if a[i - 1] == b[j - 1] {
                lcs[i - 1][j - 1] + 1
            } else {
                lcs[i - 1][j].max(lcs[i][j - 1])
            };
        }
    }
    a.len() + b.len() - 2 * lcs[a.len()][b.len()]
}

/// Keep every edge except those that close a cycle: iterative DFS coloring
/// (white/grey/black) over the prereq direction; an edge into a grey node is
/// a back edge and is dropped. Returns the kept edges and the dropped ones
/// (in original order).
pub(crate) fn remove_cycle_edges(
    order: &[String],
    edges: &[(String, String)],
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let index: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(position, key)| (key.as_str(), position))
        .collect();
    // Reverse adjacency: concept position -> its prerequisites' positions.
    let mut prereqs: Vec<Vec<usize>> = vec![Vec::new(); order.len()];
    for (from, to) in edges {
        if let (Some(&from_pos), Some(&to_pos)) = (index.get(from.as_str()), index.get(to.as_str()))
        {
            prereqs[to_pos].push(from_pos);
        }
    }

    let mut color = vec![0u8; order.len()]; // 0 white, 1 grey, 2 black
    let mut dropped_pos: HashSet<(usize, usize)> = HashSet::new();
    for root in 0..order.len() {
        if color[root] != 0 {
            continue;
        }
        color[root] = 1;
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(node, cursor)) = stack.last() {
            if cursor < prereqs[node].len() {
                let next = prereqs[node][cursor];
                stack.last_mut().unwrap().1 = cursor + 1;
                match color[next] {
                    0 => {
                        color[next] = 1;
                        stack.push((next, 0));
                    }
                    1 => {
                        // `next` is an ancestor of `node` in the DFS tree, so
                        // the original edge next -> node closes a cycle.
                        dropped_pos.insert((next, node));
                    }
                    _ => {}
                }
            } else {
                color[node] = 2;
                stack.pop();
            }
        }
    }
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for (from, to) in edges {
        match (index.get(from.as_str()), index.get(to.as_str())) {
            (Some(&from_pos), Some(&to_pos)) if !dropped_pos.contains(&(from_pos, to_pos)) => {
                kept.push((from.clone(), to.clone()));
            }
            _ => dropped.push((from.clone(), to.clone())),
        }
    }
    (kept, dropped)
}

/// Deterministic final pass shared by every merge path: drop back edges so
/// the published graph is always a DAG, and fold the drop statistics into a
/// fresh audit shell (findings are filled by the audit stage).
fn finalize_graph(
    nodes: Vec<ConceptGraphNode>,
    edges: Vec<ConceptGraphEdge>,
    mut dropped: Vec<DroppedEdge>,
    raw_refs: usize,
) -> ConceptGraphData {
    let order: Vec<String> = nodes.iter().map(|node| node.id.clone()).collect();
    let edge_pairs: Vec<(String, String)> = edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect();
    let (kept_pairs, cycle_dropped) = remove_cycle_edges(&order, &edge_pairs);
    let kept: HashSet<(String, String)> = kept_pairs.into_iter().collect();
    let final_edges = edges
        .into_iter()
        .filter(|edge| kept.contains(&(edge.from.clone(), edge.to.clone())))
        .collect();
    for (from, to) in cycle_dropped {
        dropped.push(DroppedEdge {
            from,
            to,
            reason: "cycle".into(),
        });
    }
    let audit = ConceptGraphAudit {
        ref_drop_count: dropped.len(),
        raw_ref_count: raw_refs,
        ref_drop_rate: if raw_refs == 0 {
            0.0
        } else {
            dropped.len() as f64 / raw_refs as f64
        },
        dropped_edges: dropped,
        findings: Vec::new(),
    };
    ConceptGraphData {
        nodes,
        edges: final_edges,
        audit,
    }
}

/// Incrementally merge a normalized repair batch into an existing graph:
/// existing keys win, new edges are unioned, cycles are removed globally, and
/// the audit drop statistics ACCUMULATE (previous drops stay dropped — a
/// patch cannot re-add an edge whose endpoint never existed — because the
/// orphaned-units audit keys on those entries; resetting them would blind
/// the re-audit to exactly the units the repair was supposed to reconnect).
pub(crate) fn merge_batch(graph: &ConceptGraphData, batch: &NormalizedBatch) -> ConceptGraphData {
    let mut nodes = graph.nodes.clone();
    let mut node_keys: HashSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    for node in &batch.nodes {
        if node_keys.insert(node.id.as_str()) {
            nodes.push(node.clone());
        }
    }
    let mut edges = graph.edges.clone();
    edges.extend(batch.edges.iter().cloned());
    let mut dropped = graph.audit.dropped_edges.clone();
    dropped.extend(batch.dropped.iter().cloned());
    finalize_graph(
        nodes,
        edges,
        dropped,
        graph.audit.raw_ref_count + batch.raw_refs,
    )
}

// ── Scope analysis (pre-generation reference) ──────────────────────────────

/// Scope call: ONE light call that resolves what the goal description
/// actually covers before the agent builds the network. The output is
/// REFERENCE material only — a STRICTLY COMPLETE list of large-block concepts
/// that a complete network must cover (no fixed count: a complex goal gets
/// more blocks, a simple goal fewer); the agent decomposes each block into
/// learning units on its own, and the audit re-checks every block against
/// the final graph. Nothing else (unit-level naming, expected size) is the
/// scope's job — the audit owns completeness. Failure degrades to a
/// scope-free draft, never to a hard error.
///
/// The prompt is maintained in Chinese, in lockstep with the agent-loop
/// prompts in nomifun-ai-agent (`GENERATE_AGENT_SYSTEM` / `REPAIR_AGENT_SYSTEM`)
/// — the Chinese wording is the single source of truth for the semantic
/// contracts; do not fork new English variants.
const SCOPE_SYSTEM: &str = r#"你负责在学习目标被拆解为学习单元网络之前，先厘清这个目标到底覆盖什么。
只回复一个 JSON 对象，形状如下：
{
  "scope": "一句话界定该目标覆盖什么、从哪里开始",
  "blocks": ["大块概念一", "大块概念二"]
}
规则：
- "scope"：一句话划清目标的边界——起点、要达到的水平、主题广度。当学习者基线不明且没有明确要求起点时，默认从零开始：假定学习者对该主题没有任何先备知识与技能，并据此设定起点。
- "blocks"：该目标真正覆盖的大块概念，按从基础到高级排序，合起来必须铺满从起点到目标的整条路径。这是严格完备的覆盖清单——一个完整课程该包含的大块概念都要列入；漏列是最严重的失败，拿不准时把一个大块拆成两个，也不要把两个合并成一个。数量不固定：复杂的目标多列，简单的目标少列。
- 用学习目标的语言书写。
- 只输出 JSON，不要 Markdown 代码块，不要任何解释。"#;

/// Resolved scope reference fed into the generation call. `blocks` is
/// deliberately coarse: large-block concepts the generator decomposes into
/// final unit names, never exact unit names themselves.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScopeAnalysis {
    pub scope: String,
    pub blocks: Vec<String>,
}

/// Raw scope reply — same tolerant parsing philosophy as [`RawConcept`]: a
/// bare string where a list is expected, or a missing field, degrades
/// instead of failing the whole analysis.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct RawScope {
    #[serde(default)]
    pub scope: String,
    #[serde(default, deserialize_with = "de_pre_list")]
    pub blocks: Vec<String>,
}

/// Parse the scope reply; `None` means "no usable scope" and degrades to a
/// scope-free draft.
fn parse_scope_reply(raw: &str) -> Option<ScopeAnalysis> {
    let parsed = crate::generation::parse_json_object::<RawScope>(raw).ok()?;
    Some(ScopeAnalysis {
        scope: parsed.scope.trim().to_owned(),
        blocks: parsed.blocks,
    })
}

/// One scope call, best-effort: any failure is logged and degrades to `None`
/// so the generation call still runs without a reference (pre-scope
/// behavior). Also the draft store's scope resolver (`cg_start`).
pub(crate) async fn analyze_scope(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    topic: &str,
    log: Option<&ConceptGraphLogger>,
) -> Option<ScopeAnalysis> {
    if let Some(log) = log {
        log.log("scope_start", serde_json::json!({ "topic": topic }));
    }
    let user = format!("Learning goal: {topic}");
    let started = std::time::Instant::now();
    let raw = match crate::generation::complete(
        completer,
        model_override,
        SCOPE_SYSTEM,
        &user,
        crate::generation::CONCEPT_GRAPH_SCOPE_MAX_TOKENS,
    )
    .await
    {
        Ok(raw) => raw,
        Err(error) => {
            if let Some(log) = log {
                log.log("scope_error", serde_json::json!({ "error": error.to_string() }));
            }
            return None;
        }
    };
    if let Some(log) = log {
        log.log(
            "scope_reply",
            serde_json::json!({ "duration_ms": started.elapsed().as_millis(), "reply": raw, "shape": log::reply_shape(&raw) }),
        );
    }
    match parse_scope_reply(&raw) {
        Some(scope) => {
            if let Some(log) = log {
                log.log(
                    "scope_parsed",
                    serde_json::json!({
                        "blocks": scope.blocks.len(),
                    }),
                );
            }
            Some(scope)
        }
        None => {
            if let Some(log) = log {
                log.log("scope_failed", serde_json::json!({ "reply_head": raw.chars().take(200).collect::<String>() }));
            }
            None
        }
    }
}

/// Render the audit state as a model-readable report: size, dropped
/// references with their names, and every finding with its evidence.
/// The draft kernel's `audit_report` builds on it, so it is crate-visible.
pub(crate) fn format_audit_report(graph: &ConceptGraphData) -> String {
    let mut lines = vec![format!(
        "Generated graph: {} concepts, {} edges",
        graph.nodes.len(),
        graph.edges.len()
    )];
    if graph.audit.ref_drop_count > 0 {
        lines.push(format!(
            "References dropped: {} ({} of all prerequisites). Every dropped reference is a name \
             in some \"pre\" list that matches no \"name\" in the reply — either fix the name or \
             define the concept. Dropped: {}",
            graph.audit.ref_drop_count,
            graph.audit.ref_drop_rate,
            graph
                .audit
                .dropped_edges
                .iter()
                .map(|edge| format!("{} -> {}", edge.from, edge.to))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for finding in &graph.audit.findings {
        lines.push(format!("- [{}] {}", finding.kind, finding.message));
        if !finding.node_ids.is_empty() {
            lines.push(format!("  nodes: {}", finding.node_ids.join(", ")));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(name: &str, pre: &[&str]) -> RawConcept {
        RawConcept {
            name: name.to_owned(),
            pre: pre.iter().map(|s| (*s).to_owned()).collect(),
            min: None,
        }
    }

    #[test]
    fn normalize_batch_dedupes_and_drops_invalid_references() {
        let allowed: HashSet<String> = HashSet::new();
        let batch = normalize_batch(
            &[
                concept("A", &[]),
                concept("A", &["B"]), // duplicate name, dropped entirely
                concept("B", &["A", "missing", "B", "A"]),
                concept(" ", &[]), // empty name
                concept("D", &["B"]),
            ],
            &allowed,
        );
        assert_eq!(batch.nodes.len(), 3, "A, B, D (dup A and blank dropped)");
        assert_eq!(batch.raw_refs, 6, "A emits 1, B emits 4, D emits 1");
        assert_eq!(
            batch
                .edges
                .iter()
                .map(|edge| format!("{}->{}", edge.from, edge.to))
                .collect::<Vec<_>>(),
            vec!["A->B".to_owned(), "B->D".to_owned()]
        );
        assert_eq!(
            batch.dropped.len(),
            3,
            "unknown ref + self loop + duplicate edge"
        );
        assert!(batch.dropped.iter().any(|d| d.reason == "unknown reference"));
        assert!(batch.dropped.iter().any(|d| d.reason == "self loop"));
        assert!(batch.dropped.iter().any(|d| d.reason == "duplicate edge"));
        // The concept name IS the id and title (symbolic, YAML-like).
        assert_eq!(batch.nodes[0].id, "A");
        assert_eq!(batch.nodes[0].title, "A");
        assert_eq!(batch.nodes[0].min, None);
    }

    #[test]
    fn normalize_batch_allows_external_references_from_the_allowlist() {
        let allowed: HashSet<String> = ["anchor".to_owned()].into_iter().collect();
        let batch = normalize_batch(&[concept("X", &["anchor", "missing"])], &allowed);
        assert_eq!(batch.edges.len(), 1);
        assert_eq!(batch.dropped.len(), 1);
        assert_eq!(batch.dropped[0].reason, "unknown reference");
        assert_eq!(batch.edges[0].from, "anchor");
    }

    #[test]
    fn normalize_batch_resolves_near_miss_references() {
        // One stray character ("的") against exactly one candidate resolves
        // instead of dropping — the dropped edge would have orphaned its head
        // and turned it into a fake entry point.
        let batch = normalize_batch(
            &[
                concept("用导数定义求切线斜率", &[]),
                concept("用极限理解逼近过程", &["用导数定义求切线斜率的"]),
            ],
            &HashSet::new(),
        );
        assert_eq!(batch.edges.len(), 1, "the near-miss reference resolves");
        assert_eq!(batch.edges[0].from, "用导数定义求切线斜率");
        assert_eq!(
            batch.fuzzy_resolved,
            vec![
                ("用导数定义求切线斜率的".to_owned(), "用导数定义求切线斜率".to_owned())
            ]
        );
        assert!(batch.dropped.is_empty());
    }

    #[test]
    fn normalize_batch_never_resolves_a_substitution() {
        // 解一元一次方程 vs 解一元二次方程: one swapped character, indel
        // distance 2 — a genuinely different unit, never a fuzzy match.
        let batch = normalize_batch(
            &[concept("解一元一次方程", &[]), concept("解应用题", &["解一元二次方程"])],
            &HashSet::new(),
        );
        assert!(batch.edges.is_empty());
        assert!(batch.fuzzy_resolved.is_empty());
        assert_eq!(batch.dropped.len(), 1);
        assert_eq!(batch.dropped[0].reason, "unknown reference");
    }

    #[test]
    fn normalize_batch_tolerates_single_string_pre() {
        let raw = serde_json::from_str::<RawConcept>(
            r#"{"name": "X", "pre": "A"}"#,
        )
        .unwrap();
        assert_eq!(raw.pre, vec!["A".to_owned()]);
        let raw = serde_json::from_str::<RawConcept>(r#"{"name": "Y"}"#).unwrap();
        assert!(raw.pre.is_empty());
    }

    #[test]
    fn remove_cycle_edges_keeps_diamonds_and_reports_back_edges() {
        // a -> b, a -> c, b -> d, c -> d is a diamond (kept); d -> a closes
        // the cycles d -> a -> b -> d and d -> a -> c -> d. DFS from a finds
        // both back edges into the grey ancestor a, so a -> b and a -> c are
        // dropped and the rest is kept.
        let order = vec!["a", "b", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let edges: Vec<(String, String)> = vec![
            ("a", "b"),
            ("a", "c"),
            ("b", "d"),
            ("c", "d"),
            ("d", "a"),
            ("d", "e"),
        ]
        .into_iter()
        .map(|(from, to)| (from.to_owned(), to.to_owned()))
        .collect();
        let (kept, dropped) = remove_cycle_edges(&order, &edges);
        assert_eq!(kept.len(), 4);
        assert_eq!(
            dropped,
            vec![
                ("a".to_owned(), "b".to_owned()),
                ("a".to_owned(), "c".to_owned())
            ]
        );
    }

    #[test]
    fn finalize_graph_breaks_cycles_and_counts_drops() {
        // a -> b -> a is a cycle; the back edge is dropped and counted.
        let graph = finalize_graph(
            vec![unit("a"), unit("b")],
            vec![ConceptGraphEdge { from: "a".into(), to: "b".into(), reason: None },
                 ConceptGraphEdge { from: "b".into(), to: "a".into(), reason: None }],
            Vec::new(),
            2,
        );
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1, "one of the two cycle edges is dropped");
        assert_eq!(graph.audit.ref_drop_count, 1);
        assert_eq!(graph.audit.dropped_edges[0].reason, "cycle");
    }

    fn unit(name: &str) -> ConceptGraphNode {
        ConceptGraphNode {
            id: name.to_owned(),
            title: name.to_owned(),
            min: None,
            group: None,
            necessity: None,
            is_anchor: None,
        }
    }

    fn graph_of(nodes: &[&str], edges: &[(&str, &str)]) -> ConceptGraphData {
        ConceptGraphData {
            nodes: nodes.iter().map(|name| unit(name)).collect(),
            edges: edges
                .iter()
                .map(|(from, to)| ConceptGraphEdge {
                    from: (*from).to_owned(),
                    to: (*to).to_owned(),
                    reason: None,
                })
                .collect(),
            audit: ConceptGraphAudit::default(),
        }
    }

    #[test]
    fn merge_batch_keeps_existing_keys_and_recomputes_audit() {
        let graph = graph_of(&["a", "b"], &[("a", "b")]);
        let batch = normalize_batch(&[concept("a", &[]), concept("c", &["a"])], &HashSet::new());
        let merged = merge_batch(&graph, &batch);
        assert_eq!(merged.nodes.len(), 3);
        assert!(merged.nodes.iter().any(|node| node.id == "c"));
        assert_eq!(merged.audit.ref_drop_rate, 0.0);
    }

    #[test]
    fn merge_batch_accumulates_drop_statistics() {
        // b's prerequisite "ghost" is dropped: b is an orphan candidate the
        // repair is supposed to reconnect. A patch that adds c (no drops of
        // its own) must NOT wipe that evidence — the re-audit keys on it.
        let mut graph = graph_of(&["a", "b"], &[("a", "b")]);
        graph.audit.dropped_edges = vec![DroppedEdge {
            from: "ghost".into(),
            to: "b".into(),
            reason: "unknown reference".into(),
        }];
        graph.audit.ref_drop_count = 1;
        graph.audit.raw_ref_count = 1;
        graph.audit.ref_drop_rate = 1.0;
        let allowed: HashSet<String> = ["a".to_owned(), "b".to_owned()]
            .into_iter()
            .collect();
        let batch = normalize_batch(&[concept("c", &["a"])], &allowed);
        let merged = merge_batch(&graph, &batch);
        assert_eq!(merged.audit.ref_drop_count, 1, "the old drop survives");
        assert_eq!(
            merged
                .audit
                .dropped_edges
                .iter()
                .map(|edge| (edge.from.as_str(), edge.to.as_str()))
                .collect::<Vec<_>>(),
            vec![("ghost", "b")],
            "the dropped entry keeps its names for the repair prompt"
        );
        assert_eq!(merged.audit.raw_ref_count, 2, "1 old + 1 new reference");
        assert_eq!(merged.audit.ref_drop_rate, 0.5);
    }

    #[test]
    fn raw_min_parses_numbers_and_digit_strings_tolerantly() {
        let raw = serde_json::from_str::<RawConcept>(
            r#"{"name": "用配方法解一元二次方程", "pre": [], "min": 15}"#,
        )
        .unwrap();
        assert_eq!(raw.min, Some(15));
        let raw =
            serde_json::from_str::<RawConcept>(r#"{"name": "X", "min": "20"}"#).unwrap();
        assert_eq!(raw.min, Some(20));
        let raw =
            serde_json::from_str::<RawConcept>(r#"{"name": "Y", "min": "many"}"#).unwrap();
        assert_eq!(raw.min, None, "non-numeric minutes degrade to None");
        let raw = serde_json::from_str::<RawConcept>(r#"{"name": "Z"}"#).unwrap();
        assert_eq!(raw.min, None);
    }

    // ── scope analysis ─────────────────────────────────────────────────────

    #[test]
    fn parse_scope_reply_accepts_the_documented_shape() {
        let raw = r#"{"scope":"零基础到本科","blocks":["算术","配方法"]}"#;
        let scope = parse_scope_reply(raw).unwrap();
        assert_eq!(scope.scope, "零基础到本科");
        assert_eq!(scope.blocks, vec!["算术", "配方法"]);
    }

    #[test]
    fn parse_scope_reply_returns_none_for_garbage_degrading_gracefully() {
        assert!(parse_scope_reply("sure, here is the plan...").is_none());
        assert!(parse_scope_reply("").is_none());
    }

    #[test]
    fn parse_scope_reply_tolerates_string_lists_and_missing_fields() {
        let raw = r#"{"scope":"x","blocks":"算术"}"#;
        let scope = parse_scope_reply(raw).unwrap();
        assert_eq!(scope.blocks, vec!["算术"]);
    }

    #[test]
    fn legacy_json_without_min_still_deserializes() {
        // A stored graph may lack min; the field must default so old files
        // keep loading. (Old level/group-era files are not migrated — the
        // feature is deliberately incompatible with retired semantics — but
        // deserialization stays tolerant.)
        let json = r#"{
            "id": "01J00000000000000000000000",
            "user_id": "u",
            "topic": "math",
            "nodes": [{"id": "a", "title": "A"}],
            "edges": [{"from": "a", "to": "b"}],
            "created_at": 1
        }"#;
        let record: ConceptGraphRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.graph.nodes[0].min, None);
        assert_eq!(record.graph.nodes[0].group, None);
        assert_eq!(record.graph.audit.ref_drop_count, 0);
        assert!(record.graph.audit.findings.is_empty());
    }
}
