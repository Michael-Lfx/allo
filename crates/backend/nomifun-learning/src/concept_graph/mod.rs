//! Experimental concept-graph feature: decompose a broad learning goal into a
//! network of LEARNING UNITS linked by task-dependency edges — a complete DAG.
//! A unit is one human study session of at most 25 minutes: a small organic
//! combination of atomic concepts, or a single atomic concept rich enough to
//! fill a session. Unit names are action sentences ("用配方法解一元二次方程"),
//! never concept nouns or whole sub-domains ("概率基础" is meaningless as a unit).
//! Units usually fit within 30 minutes (soft cap); genuinely hard single
//! lessons may go up to 60 (hard cap).
//!
//! Generation follows the "audit-gate loop" pattern: a small SCOPE call first
//! resolves what the user's goal description actually covers — a strictly
//! complete checklist of large-block concepts (reference material only,
//! never scaffolding; no fixed count, a complex goal gets more blocks), then
//! ONE full model call enumerates the whole
//! network in a deliberately symbolic shape (unit name + direct dependency
//! names + minute budget), the program normalizes it tolerantly (dedupe, drop
//! unknown references, break cycles), then a deterministic audit grades the
//! result. Danger-grade findings are fixed by LIGHT local patch calls
//! (add/link/reverse/split/merge — never a full rewrite), for at most three
//! rounds; a graph that still fails the gate is rejected with the full report
//! so a human can repair it. The normalized graph is persisted by
//! [`crate::service::LearningService`] as JSON files so the UI can revisit it
//! without regenerating.

use std::collections::{HashMap, HashSet};

use nomifun_common::{AppError, UserId};
use serde::{Deserialize, Serialize};

use crate::completer::LearningCompleter;

mod audit;
pub mod draft;
mod log;
mod repair;

pub(crate) use audit::{SEV_DANGER, audit_concept_graph, audit_concept_graph_with_scope};
pub(crate) use log::ConceptGraphLogger;
pub(crate) use repair::{auto_repair, repair_graph};

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

/// Repair request: which finding kinds the model should address. An empty
/// list means "fix everything the audit found".
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RepairConceptGraphRequest {
    #[serde(default)]
    pub kinds: Vec<String>,
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

/// The audit gate gives the model at most this many repair rounds after the
/// first full generation call (total model calls = MAX_REPAIR_ROUNDS + 1;
/// every round after the first is a LIGHT patch call, never a full rewrite).
pub(crate) const MAX_REPAIR_ROUNDS: usize = 3;

/// One generation call writes the WHOLE network (30-60 learning units plus
/// their dependency names) in a single reply — far heavier than a
/// course-stage call. Patch calls in the repair loop are much lighter and
/// get a shorter bound. The course-generation ceiling of 180s is too tight
/// for the full call, so concept graphs get their own bound.
const CONCEPT_GRAPH_CALL_TIMEOUT_SECS: u64 = 600;

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

/// The whole generation reply: one flat concept list. No milestones, no
/// groups, no anchors — the graph itself is the deliverable. `concepts` is
/// REQUIRED (no default) so a bare array of unit objects — a shape models
/// sometimes emit instead of the wrapper — fails the object parse and falls
/// through to the array fallback instead of silently yielding an empty graph.
#[derive(Debug, Deserialize)]
pub(crate) struct RawGraph {
    pub concepts: Vec<RawConcept>,
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

// ── Generation prompt ──────────────────────────────────────────────────────

/// Generation prompt: one call, one flat list of symbolic LEARNING UNIT
/// entries — the same shape as a hand-maintained YAML prerequisite graph,
/// plus a minute budget per unit. The prompt carries the non-negotiable
/// semantic contracts (prerequisite sufficiency, real entry points only,
/// convergence between sub-domains); the audit gate loop enforces the
/// structural side (coverage, connectivity, multi-parent share, workload
/// cap).
const GENERATE_SYSTEM: &str = r#"You decompose a broad learning goal into a complete network of LEARNING UNITS linked by task-dependency edges.
A learning unit is ONE human study session, usually within 30 minutes (the soft cap): a small organic combination of simple atomic concepts, or a single atomic concept rich enough to fill a session on its own. A genuinely hard single lesson may go up to 60 minutes (the hard cap). It is NEVER a whole sub-domain — "概率基础" is a whole small field and meaningless as a unit.
Reply with ONLY one JSON object matching this shape:
{
  "concepts": [
    {"name": "单元名", "pre": ["前置单元名"], "min": 15}
  ]
}
Rules:
- "name" is an ACTION describing what the learner does in this unit — the knowledge point is the core and the sentence form serves it, never the reverse. Varied good names: "用配方法解一元二次方程", "理解导数的极限定义", "证明可导函数必连续", "比较二分法与牛顿法的收敛速度", "构造素数筛", "推导等比数列求和公式", "辨析充分条件与必要条件". Never the bare concept noun "一元二次方程求解". Start with an action verb (解/求/证明/推导/比较/判定/构造/区分/计算/应用/理解/辨析/建立/验证...). Do NOT reuse one sentence template across units (e.g. every name starting with "用"): vary the verb and the sentence structure so each name carries its knowledge point distinctly.
- "min" is the estimated study time in minutes, in 5-minute steps: keep units within 5-30 whenever possible (the soft cap); a genuinely hard single lesson may go up to 60 (the hard cap). The 60-minute budget is a HARD CAP: less is always fine, more is not.
- SUFFICIENCY CONTRACT: "pre" is the COMPLETE set of direct prerequisites. The invariant: a learner who has finished EXACTLY the units listed in "pre" (and nothing else) can fully understand this unit without any other background. For every unit ask "what must the learner already master to understand this?" and list EVERY such unit — omitting one makes the unit incomprehensible at its position in the path. Do NOT trim "pre" to shorten the reply.
- "pre": [] is allowed ONLY for units a complete beginner understands from daily intuition or school arithmetic (数数、四则运算、直观图形). Anything above that level — limits, integrals, proofs, vectors, equations — MUST list its real prerequisites. A unit about calculus with an empty "pre" is a hard error: it would strand the learner mid-path.
- CONVERGENCE IS EXPECTED: real knowledge is a DAG, not a chain. Units where two earlier threads meet legitimately depend on 2-4 prerequisites: 解析几何 depends on BOTH geometry AND equations; 微积分 depends on BOTH functions AND limits; 数列 depends on BOTH functions AND arithmetic patterns. Produce such converging units deliberately — a network where nearly every unit has exactly one prerequisite is a forest of chains, not a graph.
- SPIRAL LEARNING: the same topic legitimately appears at several depths with different viewpoints — "用配方法解一元二次方程" then "用求根公式解一元二次方程" then "用判别式判定一元二次方程根的性质". Every unit name must make its depth and viewpoint clear; never emit two units with the same name, and never emit two nearly identical units.
- Produce a COMPLETE network covering the WHOLE path from the starting point to the goal: every significant sub-domain of the topic must be decomposed. Missing sub-domains are the worst failure — decompose generously. The unit count follows the goal's true breadth: a whole field like "数学零基础到本科结业" needs well over 60 units; never stop early just to finish quickly.
- If the user message includes a reference scope analysis, treat it as the COVERAGE CHECKLIST: every listed block must be realized as one or more units (reworded into action sentences as needed).
- Every name in any "pre" list MUST also appear as a "name" in this same reply (self-contained reference space). Reuse the exact name string — no aliases, no paraphrases.
- The network must be ONE connected structure: every unit is reachable from the true entry units along dependency chains. Sub-domains MUST cross-link where they genuinely depend on each other — geometry and algebra meet in 解析几何; functions and limits meet in 微积分. Two sub-domains forming two separate disconnected trees is a hard failure.
- No star-shaped hubs: no single unit should carry more than ~12 direct dependents — spread dependencies across intermediate units instead.
- Write names in the language of the learning goal.
- Output JSON only, without Markdown fences or commentary."#;

// ── Scope analysis (pre-generation reference) ──────────────────────────────

/// Scope call: ONE light call that resolves what the goal description
/// actually covers before the heavy generation call. The output is REFERENCE
/// material only — a STRICTLY COMPLETE list of large-block concepts that a
/// complete network must cover (no fixed count: a complex goal gets more
/// blocks, a simple goal fewer); the generation stage decomposes each block
/// into learning units on its own, and the audit re-checks every block
/// against the final graph. Nothing else (unit-level naming, expected size)
/// is the scope's job — the audit owns completeness. Failure degrades to
/// no-scope generation (the pre-scope behavior), never to a hard error.
const SCOPE_SYSTEM: &str = r#"You resolve the exact coverage of a broad learning goal BEFORE it is decomposed into units.
Reply with ONLY one JSON object matching this shape:
{
  "scope": "one sentence delimiting what this goal covers and where it starts",
  "blocks": ["骨干概念一", "骨干概念二"]
}
Rules:
- "scope": a crisp one-sentence boundary of the goal — the starting point, the target level, and the subject breadth. When the learner's baseline is unclear and no explicit starting point is requested, default to absolute zero: assume the learner has NO prior knowledge or skills in the subject, and set the starting point accordingly.
- "blocks": the large-block concepts the goal genuinely covers, ordered from foundational to advanced, together spanning the WHOLE path from the starting point to the goal. This is a strictly complete checklist — every significant block a complete curriculum would include; under-listing is the worst failure, so when in doubt, split a block rather than merge two. No fixed count: a complex goal gets more blocks, a simple goal fewer.
- Write names in the language of the learning goal.
- Output JSON only, without Markdown fences or commentary."#;

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
/// instead of failing the whole analysis. `blocks` is the only field the
/// current prompt asks for; legacy two-list replies (subdomains + backbone)
/// are still accepted and merged into one checklist.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct RawScope {
    #[serde(default)]
    pub scope: String,
    #[serde(default, deserialize_with = "de_pre_list")]
    pub blocks: Vec<String>,
    #[serde(default, deserialize_with = "de_pre_list")]
    pub subdomains: Vec<String>,
    #[serde(default, deserialize_with = "de_pre_list")]
    pub backbone: Vec<String>,
}

/// Parse the scope reply; `None` means "no usable scope" and degrades to
/// scope-free generation.
fn parse_scope_reply(raw: &str) -> Option<ScopeAnalysis> {
    let parsed = crate::generation::parse_json_object::<RawScope>(raw).ok()?;
    let mut blocks = parsed.blocks;
    if blocks.is_empty() {
        // Legacy two-list replies merge into the single block checklist.
        blocks.extend(parsed.subdomains.into_iter());
        blocks.extend(parsed.backbone.into_iter());
    }
    Some(ScopeAnalysis {
        scope: parsed.scope.trim().to_owned(),
        blocks,
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

// ── Audit-gate generation loop ─────────────────────────────────────────────

/// Full first generation + light local repair rounds: ONE full call
/// enumerates the whole network (enumeration is the model's strength); if
/// danger-grade findings remain, [`auto_repair`] issues LIGHT patch calls
/// (add/link/reverse/split/merge) against the findings — never a full rewrite,
/// which is where attention decays on long outputs. At most
/// [`MAX_REPAIR_ROUNDS`] repair rounds; a graph that still fails the gate is
/// rejected with the full report so a human can repair it. `log` receives
/// the full model replies and every stage result as JSON-lines events for
/// offline diagnosis (see [`ConceptGraphLogger`]); `None` disables logging.
pub(crate) async fn generate_concept_graph(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    topic: &str,
    log: Option<&ConceptGraphLogger>,
) -> Result<ConceptGraphData, AppError> {
    if let Some(log) = log {
        log.log(
            "session_start",
            serde_json::json!({
                "topic": topic,
                "provider_id": model_override.map(|(id, _)| id.as_str()),
                "model": model_override.map(|(_, model)| model),
                "max_tokens": crate::generation::CONCEPT_GRAPH_MAX_TOKENS,
                "timeout_secs": CONCEPT_GRAPH_CALL_TIMEOUT_SECS,
            }),
        );
    }
    // Scope first: resolve what the goal covers, feed it to the generation
    // call as a coverage checklist. Best-effort — failure degrades to
    // scope-free generation.
    let scope = analyze_scope(completer, model_override, topic, log).await;
    let user = build_generate_user(topic, scope.as_ref());
    let started = std::time::Instant::now();
    let raw = crate::generation::complete_with_timeout(
        completer,
        model_override,
        GENERATE_SYSTEM,
        &user,
        crate::generation::CONCEPT_GRAPH_MAX_TOKENS,
        std::time::Duration::from_secs(CONCEPT_GRAPH_CALL_TIMEOUT_SECS),
    )
    .await?;
    if let Some(log) = log {
        log.log(
            "generate_reply",
            serde_json::json!({
                "duration_ms": started.elapsed().as_millis(),
                "reply": &raw,
                "shape": log::reply_shape(&raw),
            }),
        );
    }
    let parsed = match parse_graph_reply(&raw) {
        Ok(parsed) => parsed,
        Err(error) => {
            if let Some(log) = log {
                log.log("parse_failed", serde_json::json!({ "error": error }));
            }
            return Err(AppError::UnprocessableEntity(format!(
                "concept graph reply could not be parsed as JSON: {error}"
            )));
        }
    };
    if let Some(log) = log {
        log.log("parsed", serde_json::json!({ "concepts": parsed.concepts.len() }));
    }
    let (mut graph, fuzzy_resolved) = assemble_graph(&parsed);
    // The scope content checklist (large-block concepts) rejoins the audit
    // as a coverage contract of its own — so skipped blocks are caught and
    // repairable by name.
    graph.audit.findings = audit_concept_graph_with_scope(
        &graph,
        scope.as_ref().map(|scope| scope.blocks.as_slice()),
    );
    if let Some(log) = log {
        // Near-miss references that were resolved instead of dropped are a
        // diagnosis signal on their own: each one was a would-be orphan.
        if !fuzzy_resolved.is_empty() {
            log.log(
                "fuzzy_resolved",
                serde_json::json!({
                    "count": fuzzy_resolved.len(),
                    "pairs": &fuzzy_resolved,
                }),
            );
        }
        log.log(
            "audit",
            serde_json::json!({
                "nodes": graph.nodes.len(),
                "edges": graph.edges.len(),
                "dropped_refs": graph.audit.ref_drop_count,
                "findings": &graph.audit.findings,
            }),
        );
    }

    for _ in 0..MAX_REPAIR_ROUNDS {
        if graph
            .audit
            .findings
            .iter()
            .all(|finding| finding.severity != SEV_DANGER)
        {
            if let Some(log) = log {
                log.log(
                    "session_end",
                    serde_json::json!({
                        "ok": true,
                        "nodes": graph.nodes.len(),
                        "edges": graph.edges.len(),
                    }),
                );
            }
            return Ok(graph);
        }
        match auto_repair(topic, &graph, completer, model_override, scope.as_ref(), log).await {
            Ok(Some(fixed)) => graph = fixed,
            // Repair made no progress (or the model produced nothing usable):
            // stop patching and report the surviving failures honestly.
            Ok(None) => break,
            Err(error) => return Err(error),
        }
    }
    let report = format_audit_report(&graph);
    if let Some(log) = log {
        log.log("session_end", serde_json::json!({ "ok": false, "error": report }));
    }
    Err(AppError::UnprocessableEntity(format!(
        "concept graph still fails the audit gate after {} rounds: {}",
        MAX_REPAIR_ROUNDS + 1,
        report
    )))
}

/// Normalize one flat concept list into a graph, breaking cycles and folding
/// the drop statistics into the audit shell. Also returns the near-miss
/// references that were fuzzy-resolved instead of dropped — each one is a
/// would-be orphan, worth a diagnosis-log line.
fn assemble_graph(raw: &RawGraph) -> (ConceptGraphData, Vec<(String, String)>) {
    let batch = normalize_batch(&raw.concepts, &HashSet::new());
    let graph = finalize_graph(batch.nodes, batch.edges, batch.dropped, batch.raw_refs);
    (graph, batch.fuzzy_resolved)
}

/// Parse the generation reply: the documented `{"concepts": [...]}` object
/// first; a model that answered with the bare concepts array (with or without
/// code fences) is tolerated as a fallback. When both shapes fail, the object
/// error plus the reply diagnostic is returned so the failure is
/// self-explanatory.
fn parse_graph_reply(raw: &str) -> Result<RawGraph, String> {
    match crate::generation::parse_json_object::<RawGraph>(raw) {
        Ok(graph) => Ok(graph),
        Err(object_error) => {
            let stripped = crate::generation::strip_code_fences(raw);
            match serde_json::from_str::<Vec<RawConcept>>(&stripped) {
                Ok(concepts) => Ok(RawGraph { concepts }),
                Err(array_error) => Err(format!(
                    "{object_error}; bare array fallback failed: {array_error}"
                )),
            }
        }
    }
}

/// The generation call only ever asks for the whole network once. A resolved
/// scope (when present) is embedded as the coverage checklist: large-block
/// concepts that must all be covered — the model still produces the single
/// flat list and decomposes each block into units itself.
fn build_generate_user(topic: &str, scope: Option<&ScopeAnalysis>) -> String {
    let Some(scope) = scope else {
        return format!("Learning goal: {topic}");
    };
    let mut parts = vec![format!("Learning goal: {topic}")];
    if !scope.scope.is_empty() {
        parts.push(format!("范围界定：{}", scope.scope));
    }
    if !scope.blocks.is_empty() {
        parts.push(format!(
            "必须覆盖的大块概念（严格完备清单，每个需落实为一个或多个单元，可改写为动作句）：{}",
            scope.blocks.join("；")
        ));
    }
    parts.join("\n")
}

/// Render the audit state as a model-readable report: size, dropped
/// references with their names, and every finding with its evidence.
/// Also used by the repair stages, so it is crate-visible.
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
    fn assemble_graph_breaks_cycles_and_counts_drops() {
        // a -> b -> a is a cycle; the back edge is dropped and counted.
        let raw = RawGraph {
            concepts: vec![concept("a", &["b"]), concept("b", &["a"])],
        };
        let (graph, fuzzy) = assemble_graph(&raw);
        assert!(fuzzy.is_empty());
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1, "one of the two cycle edges is dropped");
        assert_eq!(graph.audit.ref_drop_count, 1);
        assert_eq!(graph.audit.dropped_edges[0].reason, "cycle");
    }

    #[test]
    fn merge_batch_keeps_existing_keys_and_recomputes_audit() {
        let (graph, _) = assemble_graph(&RawGraph {
            concepts: vec![concept("a", &[]), concept("b", &["a"])],
        });
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
        let (graph, _) = assemble_graph(&RawGraph {
            concepts: vec![concept("a", &[]), concept("b", &["ghost"])],
        });
        assert_eq!(graph.audit.ref_drop_count, 1);
        assert_eq!(graph.audit.raw_ref_count, 1);
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

    #[test]
    fn parse_graph_reply_accepts_the_documented_object_shape() {
        let raw = r#"{"concepts": [{"name": "A", "pre": [], "min": 10}]}"#;
        let parsed = parse_graph_reply(raw).unwrap();
        assert_eq!(parsed.concepts.len(), 1);
        assert_eq!(parsed.concepts[0].min, Some(10));
    }

    #[test]
    fn parse_graph_reply_accepts_a_bare_concept_array() {
        let raw = r#"[{"name": "A", "pre": [], "min": 10}, {"name": "B", "pre": ["A"]}]"#;
        let parsed = parse_graph_reply(raw).unwrap();
        assert_eq!(parsed.concepts.len(), 2);
        assert_eq!(parsed.concepts[1].pre, vec!["A".to_owned()]);
    }

    #[test]
    fn parse_graph_reply_accepts_a_fenced_bare_concept_array() {
        let raw = "```json\n[{\"name\": \"A\", \"min\": 5}]\n```";
        let parsed = parse_graph_reply(raw).unwrap();
        assert_eq!(parsed.concepts.len(), 1);
        assert_eq!(parsed.concepts[0].min, Some(5));
    }

    #[test]
    fn parse_graph_reply_reports_both_shapes_failing_with_diagnostics() {
        let err =
            parse_graph_reply("learning plan: 1. 学配方法 2. 学求根公式 3. 学判别式").unwrap_err();
        assert!(err.contains("no complete JSON object found"), "{err}");
        assert!(err.contains("head:"), "{err}");
        assert!(err.contains("bare array fallback failed"), "{err}");
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
    fn parse_scope_reply_tolerates_string_lists_missing_fields_and_legacy_two_lists() {
        let raw = r#"{"scope":"x","blocks":"算术"}"#;
        let scope = parse_scope_reply(raw).unwrap();
        assert_eq!(scope.blocks, vec!["算术"]);
        // Legacy two-list replies (subdomains + backbone) merge into blocks.
        let legacy =
            parse_scope_reply(r#"{"scope":"x","subdomains":["算术"],"backbone":["配方法"]}"#)
                .unwrap();
        assert_eq!(legacy.blocks, vec!["算术", "配方法"]);
    }

    #[test]
    fn build_generate_user_without_scope_stays_minimal() {
        assert_eq!(build_generate_user("数学", None), "Learning goal: 数学");
    }

    #[test]
    fn build_generate_user_embeds_scope_as_coverage_checklist() {
        let scope = ScopeAnalysis {
            scope: "零基础到本科结业".into(),
            blocks: vec!["算术".into(), "代数".into(), "配方法".into()],
        };
        let user = build_generate_user("数学-零基础到本科结业", Some(&scope));
        assert!(user.contains("范围界定：零基础到本科结业"), "{user}");
        assert!(user.contains("必须覆盖的大块概念"), "{user}");
        assert!(user.contains("算术；代数；配方法"), "{user}");
        assert!(!user.contains("预期单元规模"), "{user}");
        assert!(user.starts_with("Learning goal: 数学-零基础到本科结业"), "{user}");
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
