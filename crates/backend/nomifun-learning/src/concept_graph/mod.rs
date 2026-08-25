//! Experimental concept-graph feature: decompose a broad learning goal (e.g.
//! "from zero to university mathematics") into atomic concepts linked by
//! prerequisite edges — a complete DAG.
//!
//! Generation follows the "audit-gate loop" pattern: ONE model call produces
//! the whole graph in a deliberately symbolic shape (concept names + their
//! direct prerequisite names — the same minimal schema as hand-maintained
//! YAML graphs), the program normalizes it tolerantly (dedupe, drop unknown
//! references, break cycles), then a deterministic audit grades the result.
//! Danger-grade findings are fed back to the model as a report and the model
//! regenerates the COMPLETE graph, for at most three rounds; a graph that
//! still fails the gate is rejected instead of published half-broken. The
//! normalized graph is persisted by [`crate::service::LearningService`] as
//! JSON files so the UI can revisit it without regenerating.

use std::collections::{HashMap, HashSet};

use nomifun_common::AppError;
use serde::{Deserialize, Serialize};

use crate::completer::LearningCompleter;

mod audit;
mod repair;

pub(crate) use audit::{SEV_DANGER, audit_concept_graph};
pub(crate) use repair::repair_graph;

/// One node in the graph. `level` splits the two generation layers of the
/// retired multi-stage pipeline; it stays in the model for file
/// compatibility (old stored graphs carry it) but new single-call graphs
/// never set it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptGraphNode {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// Sub-domain group label (atomic concepts only).
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
    /// `ref_drop_count` over the total prerequisite entries the model emitted.
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

/// The audit gate gives the model at most this many regeneration rounds
/// after the first attempt (total attempts = MAX_REPAIR_ROUNDS + 1).
pub(crate) const MAX_REPAIR_ROUNDS: usize = 3;

/// One generation call writes the WHOLE graph (60-120 concepts plus their
/// prerequisite names) in a single reply — far heavier than a course-stage
/// call, and a re-generation round must rewrite everything again. The
/// course-generation ceiling of 180s is too tight for that, so concept
/// graphs get their own bound.
const CONCEPT_GRAPH_CALL_TIMEOUT_SECS: u64 = 600;

// ── Raw model output types ────────────────────────────────────────────────

/// Raw per-concept model output — deliberately symbolic and minimal, the
/// same shape as a hand-maintained YAML graph: a concept NAME plus its direct
/// prerequisite names. The program derives ids, edges, and all optional
/// fields, and tolerates the usual LLM habits (a single string where an
/// array is expected, duplicate names, unknown references, cycles).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawConcept {
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "de_pre_list")]
    pub pre: Vec<String>,
}

/// The whole generation reply: one flat concept list. No milestones, no
/// groups, no anchors — the graph itself is the deliverable.
#[derive(Debug, Deserialize)]
pub(crate) struct RawGraph {
    #[serde(default)]
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
            level: None,
            group: None,
            necessity: None,
            is_anchor: None,
        });
    }

    let mut seen_edges: HashSet<(String, String)> = HashSet::new();
    let mut edges: Vec<ConceptGraphEdge> = Vec::new();
    let mut dropped: Vec<DroppedEdge> = Vec::new();
    for (concept, is_kept) in raw.iter().zip(&kept) {
        let to = concept.name.trim();
        if !is_kept || !seen_names.contains(to) {
            continue;
        }
        for prereq in &concept.pre {
            let from = prereq.trim();
            let pair = (from.to_owned(), to.to_owned());
            if from.is_empty() {
                dropped.push(DroppedEdge {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    reason: "empty reference".into(),
                });
                continue;
            }
            if from == to {
                dropped.push(DroppedEdge {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    reason: "self loop".into(),
                });
                continue;
            }
            if !seen_names.contains(from) && !allowed.contains(from) {
                dropped.push(DroppedEdge {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    reason: "unknown reference".into(),
                });
                continue;
            }
            if !seen_edges.insert(pair) {
                dropped.push(DroppedEdge {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    reason: "duplicate edge".into(),
                });
                continue;
            }
            edges.push(ConceptGraphEdge {
                from: from.to_owned(),
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
    }
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
/// the audit counters are recomputed from the batch's own statistics.
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
    finalize_graph(nodes, edges, batch.dropped.clone(), batch.raw_refs)
}

// ── Generation prompt ──────────────────────────────────────────────────────

/// Generation prompt: one call, one flat list of symbolic concept entries —
/// the same shape as a hand-maintained YAML prerequisite graph. Structural
/// quality (coverage, connectivity, chains) is enforced by the audit gate
/// loop, not by asking the model for more scaffolding.
const GENERATE_SYSTEM: &str = r#"You decompose a broad learning goal into a complete graph of atomic concepts linked by prerequisite edges.
Reply with ONLY one JSON object matching this shape:
{
  "concepts": [
    {"name": "概念名", "pre": ["前置概念名"]}
  ]
}
Rules:
- "name" is the standard textbook name of an atomic concept — one skill or idea a learner masters in a single study step. Never a chapter, module, course, or overview label.
- "pre" lists the DIRECT prerequisites: concepts that must be mastered before this one. List at most 3 — only what is strictly needed. Use "pre": [] for foundational concepts.
- Produce 60-120 concepts covering the WHOLE path from the starting point to the goal: every significant sub-domain of the topic must be decomposed. Missing concepts are the worst failure — decompose generously.
- Every name in any "pre" list MUST also appear as a "name" in this same reply (self-contained reference space). Reuse the exact name string — no aliases, no paraphrases.
- The graph must be ONE connected structure: every concept is reachable from the foundational concepts along prerequisite chains.
- Concepts must form learning chains: most concepts depend on one or two earlier ones; avoid star-shaped hubs where everything depends directly on a single concept.
- Write names in the language of the learning goal.
- Output JSON only, without Markdown fences or commentary."#;

// ── Audit-gate generation loop ─────────────────────────────────────────────

/// Single-call generation with an audit gate: generate the whole graph in
/// one model call, normalize it tolerantly, audit it deterministically; if
/// any danger-grade finding remains, the audit report is fed back to the
/// model and it regenerates the COMPLETE graph — for at most
/// [`MAX_REPAIR_ROUNDS`] rounds. A graph that still fails the gate is
/// rejected: a half-broken graph is never published.
pub(crate) async fn generate_concept_graph(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    topic: &str,
) -> Result<ConceptGraphData, AppError> {
    let mut last_report = String::new();
    let mut last_error = String::new();
    for round in 0..=MAX_REPAIR_ROUNDS {
        let user = build_generate_user(topic, round, &last_report, &last_error);
        let raw = crate::generation::complete_with_timeout(
            completer,
            model_override,
            GENERATE_SYSTEM,
            &user,
            crate::generation::CONCEPT_GRAPH_MAX_TOKENS,
            std::time::Duration::from_secs(CONCEPT_GRAPH_CALL_TIMEOUT_SECS),
        )
        .await?;
        match crate::generation::parse_json_object::<RawGraph>(&raw) {
            Ok(parsed) => {
                let mut graph = assemble_graph(&parsed);
                graph.audit.findings = audit_concept_graph(&graph);
                if graph
                    .audit
                    .findings
                    .iter()
                    .all(|finding| finding.severity != SEV_DANGER)
                {
                    return Ok(graph);
                }
                last_report = format_audit_report(&graph);
                last_error.clear();
            }
            Err(error) => {
                last_error = format!("the previous reply could not be parsed as JSON: {error}");
            }
        }
    }
    Err(AppError::UnprocessableEntity(format!(
        "concept graph still fails the audit gate after {} rounds: {}",
        MAX_REPAIR_ROUNDS + 1,
        if last_report.is_empty() {
            last_error
        } else {
            last_report
        }
    )))
}

/// Normalize one flat concept list into a graph, breaking cycles and folding
/// the drop statistics into the audit shell.
fn assemble_graph(raw: &RawGraph) -> ConceptGraphData {
    let batch = normalize_batch(&raw.concepts, &HashSet::new());
    finalize_graph(batch.nodes, batch.edges, batch.dropped, batch.raw_refs)
}

/// The first round asks for the graph; later rounds attach the previous
/// attempt's audit report (or parse error) and demand a COMPLETE rewrite.
fn build_generate_user(
    topic: &str,
    round: usize,
    last_report: &str,
    last_error: &str,
) -> String {
    let mut lines = vec![format!("Learning goal: {topic}")];
    if round > 0 {
        lines.push(String::new());
        lines.push(
            "The previous attempt failed the structural audit gate. Fix EVERY issue listed below \
             and return the COMPLETE revised concept graph — every concept again, not just the fixes."
                .into(),
        );
        if !last_report.is_empty() {
            lines.push(last_report.to_owned());
        }
        if !last_error.is_empty() {
            lines.push(format!("JSON problem: {last_error}"));
        }
    }
    lines.join("\n")
}

/// Render the audit state as a model-readable report: size, dropped
/// references with their names, and every finding with its evidence.
fn format_audit_report(graph: &ConceptGraphData) -> String {
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
        assert_eq!(batch.nodes[0].level, None);
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
        let graph = assemble_graph(&raw);
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1, "one of the two cycle edges is dropped");
        assert_eq!(graph.audit.ref_drop_count, 1);
        assert_eq!(graph.audit.dropped_edges[0].reason, "cycle");
    }

    #[test]
    fn merge_batch_keeps_existing_keys_and_recomputes_audit() {
        let graph = assemble_graph(&RawGraph {
            concepts: vec![concept("a", &[]), concept("b", &["a"])],
        });
        let batch = normalize_batch(&[concept("a", &[]), concept("c", &["a"])], &HashSet::new());
        let merged = merge_batch(&graph, &batch);
        assert_eq!(merged.nodes.len(), 3);
        assert!(merged.nodes.iter().any(|node| node.id == "c"));
        assert_eq!(merged.audit.ref_drop_rate, 0.0);
    }

    #[test]
    fn old_single_call_json_still_deserializes() {
        // A pre-pipeline stored graph has no level/group/necessity/reason/audit;
        // every new field must default so old files keep loading.
        let json = r#"{
            "id": "01J00000000000000000000000",
            "user_id": "u",
            "topic": "math",
            "nodes": [{"id": "a", "title": "A"}],
            "edges": [{"from": "a", "to": "b"}],
            "created_at": 1
        }"#;
        let record: ConceptGraphRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.graph.nodes[0].level, None);
        assert_eq!(record.graph.nodes[0].group, None);
        assert_eq!(record.graph.audit.ref_drop_count, 0);
        assert!(record.graph.audit.findings.is_empty());
    }
}
