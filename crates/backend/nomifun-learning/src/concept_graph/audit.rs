//! Deterministic structural audit of the concept graph (zero model calls).
//! Findings are graded like a quality gate: `danger` findings (coverage,
//! excessive reference drops, residual cycles) block publishing and drive
//! the regeneration loop; `warning` findings (disconnected components,
//! fragmented sources/sinks, shallow structure, duplicates) go to a human
//! for a five-choice ruling, mirroring the R-level rules of hand-maintained
//! prerequisite graphs; `info` findings are reference only. Every finding
//! carries its evidence node ids so the UI can jump to them.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{AuditFinding, ConceptGraphData};

pub(crate) const SEV_INFO: &str = "info";
pub(crate) const SEV_WARNING: &str = "warning";
pub(crate) const SEV_DANGER: &str = "danger";

/// Non-atomic vocabulary: a title containing any of these is likely a
/// chapter/module/overview rather than a single study step.
const NON_ATOMIC_TERMS: &[&str] = &[
    "入门",
    "导论",
    "概述",
    "总览",
    "简介",
    "进阶",
    "高级",
    "第",
    "章",
    "intro",
    "introduction",
    "overview",
    "basics",
    "fundamentals",
    "advanced",
    "chapter",
    "module",
    "course",
];

/// Longest path (in edges) must reach at least this for the graph to be
/// considered a real multi-layer prerequisite structure.
const MIN_GRAPH_DEPTH: usize = 3;
/// Leaves at depth <= 1 are "only two layers deep" (root -> leaf); when more
/// than this share of leaves are shallow, the graph is probably too flat.
const SHALLOW_LEAF_RATIO: f64 = 0.5;
const SHALLOW_LEAF_DEPTH: usize = 1;
/// More than this many sinks/sources suggests the graph is fragmenting.
const MAX_SINKS: usize = 3;
const MAX_SOURCES: usize = 3;
/// Hub indicators: more than this many incoming (fan-in) or outgoing
/// (fan-out) prerequisite edges on one node.
const MAX_FAN_IN: usize = 8;
const MAX_FAN_OUT: usize = 12;
/// Findings cap their evidence node ids at this many to keep reports readable.
const MAX_EVIDENCE: usize = 12;

/// Coverage gate: a broad learning goal must decompose into at least this
/// many atomic concepts; fewer means whole sub-domains were skipped — the
/// "missing concepts" failure mode, graded danger so the model regenerates.
const MIN_CONCEPTS: usize = 60;
/// Reference-drop gate: a model output where this share (or absolute count)
/// of prerequisite entries names no defined concept is too sloppy to trust
/// — the naming is inconsistent, so the whole graph gets regenerated.
const REF_DROP_RATE_LIMIT: f64 = 0.15;
const REF_DROP_COUNT_LIMIT: usize = 12;

/// Run every deterministic structural check. The graph is assumed acyclic
/// (merge removes cycles), but a defensive cycle check guards hand-edited
/// files: on a cycle the depth-based indicators are skipped.
pub(crate) fn audit_concept_graph(graph: &ConceptGraphData) -> Vec<AuditFinding> {
    let mut findings = Vec::new();
    let n = graph.nodes.len();
    if n == 0 {
        return findings;
    }

    // Coverage gate: fewer concepts than a broad goal demands means whole
    // sub-domains were skipped. Graded danger so the generation loop
    // regenerates instead of publishing a thin graph.
    if n < MIN_CONCEPTS {
        findings.push(AuditFinding {
            kind: "coverage".into(),
            severity: SEV_DANGER.into(),
            message: format!(
                "only {n} atomic concepts — a broad learning goal needs at least {MIN_CONCEPTS}; whole sub-domains are likely missing"
            ),
            node_ids: Vec::new(),
        });
    }

    // Reference-drop gate: dropped prerequisite entries (unknown names,
    // self loops, duplicates, cycle edges) are the naming-consistency proxy.
    // A high share means the model's names do not line up, so the graph is
    // regenerated rather than published with silently missing edges.
    if graph.audit.ref_drop_count > REF_DROP_COUNT_LIMIT
        || graph.audit.ref_drop_rate > REF_DROP_RATE_LIMIT
    {
        findings.push(AuditFinding {
            kind: "excessive_reference_drops".into(),
            severity: SEV_DANGER.into(),
            message: format!(
                "{} prerequisite references were dropped ({} of all) — concept names are inconsistent",
                graph.audit.ref_drop_count,
                graph.audit.ref_drop_rate
            ),
            node_ids: graph
                .audit
                .dropped_edges
                .iter()
                .take(MAX_EVIDENCE)
                .map(|edge| edge.from.clone())
                .collect(),
        });
    }

    let index: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(position, node)| (node.id.as_str(), position))
        .collect();
    let mut prereqs: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut undirected: Vec<Vec<usize>> = vec![Vec::new(); n];
    for edge in &graph.edges {
        if let (Some(&from), Some(&to)) = (index.get(edge.from.as_str()), index.get(edge.to.as_str()))
        {
            prereqs[to].push(from);
            succs[from].push(to);
            undirected[from].push(to);
            undirected[to].push(from);
        }
    }

    // Topological order (Kahn) doubles as the cycle guard: depth-based
    // indicators need a DAG, and a cycle is itself a critical finding.
    let mut indegree: Vec<usize> = prereqs.iter().map(Vec::len).collect();
    let mut queue: VecDeque<usize> = indegree
        .iter()
        .enumerate()
        .filter(|(_, degree)| **degree == 0)
        .map(|(position, _)| position)
        .collect();
    let mut topo: Vec<usize> = Vec::with_capacity(n);
    while let Some(node) = queue.pop_front() {
        topo.push(node);
        for &next in &succs[node] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                queue.push_back(next);
            }
        }
    }
    let has_cycle = topo.len() != n;
    if has_cycle {
        let in_topo: HashSet<usize> = topo.iter().copied().collect();
        let cyclic: Vec<String> = (0..n)
            .filter(|position| !in_topo.contains(position))
            .take(MAX_EVIDENCE)
            .map(|position| graph.nodes[position].id.clone())
            .collect();
        findings.push(AuditFinding {
            kind: "cycle".into(),
            severity: SEV_DANGER.into(),
            message: "the graph still contains a cycle; depth-based checks were skipped".into(),
            node_ids: cyclic,
        });
    }

    // Depth per node (longest path from any root) via the topological order.
    let mut depth = vec![0usize; n];
    if !has_cycle {
        for &node in &topo {
            for &from in &prereqs[node] {
                depth[node] = depth[node].max(depth[from] + 1);
            }
        }
        let max_depth = depth.iter().copied().max().unwrap_or(0);
        if max_depth < MIN_GRAPH_DEPTH {
            findings.push(AuditFinding {
                kind: "shallow_depth".into(),
                severity: SEV_WARNING.into(),
                message: format!(
                    "longest prerequisite chain is only {max_depth} edges — the graph may be too flat for a broad goal"
                ),
                node_ids: Vec::new(),
            });
        }

        // Shallow leaves: leaves whose depth is at most SHALLOW_LEAF_DEPTH
        // connect to the graph only one or two layers up.
        let leaves: Vec<usize> = succs
            .iter()
            .enumerate()
            .filter(|(_, succs)| succs.is_empty())
            .map(|(position, _)| position)
            .collect();
        if leaves.len() >= 3 {
            let shallow = leaves
                .iter()
                .copied()
                .filter(|&leaf| depth[leaf] <= SHALLOW_LEAF_DEPTH)
                .count();
            if shallow as f64 / leaves.len() as f64 > SHALLOW_LEAF_RATIO {
                findings.push(AuditFinding {
                    kind: "shallow_leaves".into(),
                    severity: SEV_WARNING.into(),
                    message: format!(
                        "{shallow}/{} leaf concepts sit only {SHALLOW_LEAF_DEPTH} layer(s) above a root — they are probably not decomposed far enough",
                        leaves.len()
                    ),
                    node_ids: leaves
                        .iter()
                        .copied()
                        .filter(|&leaf| depth[leaf] <= SHALLOW_LEAF_DEPTH)
                        .take(MAX_EVIDENCE)
                        .map(|leaf| graph.nodes[leaf].id.clone())
                        .collect(),
                });
            }
        }
    }

    // Connected components (undirected): more than one means the graph is
    // fragmented — the deterministic equivalent of the old "missing
    // concepts" failure mode.
    let mut component = vec![None; n];
    let mut components: Vec<Vec<usize>> = Vec::new();
    for root in 0..n {
        if component[root].is_some() {
            continue;
        }
        let mut members = Vec::new();
        let mut stack = vec![root];
        component[root] = Some(components.len());
        while let Some(node) = stack.pop() {
            members.push(node);
            for &next in &undirected[node] {
                if component[next].is_none() {
                    component[next] = component[root];
                    stack.push(next);
                }
            }
        }
        components.push(members);
    }
    if components.len() > 1 {
        let largest = components.iter().map(Vec::len).max().unwrap_or(0);
        let detached: Vec<usize> = components
            .iter()
            .filter(|members| members.len() < largest)
            .flatten()
            .copied()
            .collect();
        findings.push(AuditFinding {
            kind: "disconnected_components".into(),
            // R-level (human-ruled), mirroring hand-maintained prerequisite
            // graphs: fragmentation is a warning for a human to fix by
            // bridging edges, not an automatic regeneration trigger.
            severity: SEV_WARNING.into(),
            message: format!(
                "{} disconnected components (largest has {largest} nodes) — missing concepts or missing edges are likely",
                components.len()
            ),
            node_ids: detached
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    // Multiple sinks/sources: a broad learning goal should converge onto one
    // goal concept and fan out from a small root set.
    let sinks: Vec<usize> = succs
        .iter()
        .enumerate()
        .filter(|(_, succs)| succs.is_empty())
        .map(|(position, _)| position)
        .collect();
    if sinks.len() > MAX_SINKS {
        findings.push(AuditFinding {
            kind: "multiple_sinks".into(),
            severity: SEV_WARNING.into(),
            message: format!(
                "{} terminal concepts — the graph does not converge onto the goal",
                sinks.len()
            ),
            node_ids: sinks
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }
    let sources: Vec<usize> = prereqs
        .iter()
        .enumerate()
        .filter(|(_, prereqs)| prereqs.is_empty())
        .map(|(position, _)| position)
        .collect();
    if sources.len() > MAX_SOURCES {
        findings.push(AuditFinding {
            kind: "multiple_sources".into(),
            severity: SEV_WARNING.into(),
            message: format!(
                "{} entry concepts — the prerequisite base may be fragmented",
                sources.len()
            ),
            node_ids: sources
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    // Degree anomalies: hubs that absorb or emit far more edges than their
    // neighbors are usually over-abstracted concepts.
    let mut anomalous: Vec<usize> = Vec::new();
    for node in 0..n {
        if prereqs[node].len() > MAX_FAN_IN || succs[node].len() > MAX_FAN_OUT {
            anomalous.push(node);
        }
    }
    if !anomalous.is_empty() {
        findings.push(AuditFinding {
            kind: "degree_anomaly".into(),
            severity: SEV_WARNING.into(),
            message: format!(
                "{} concept(s) with fan-in > {MAX_FAN_IN} or fan-out > {MAX_FAN_OUT} — likely over-abstracted",
                anomalous.len()
            ),
            node_ids: anomalous
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    // Near-duplicate titles: whitespace- and case-normalized equality.
    let mut by_normalized: HashMap<String, Vec<usize>> = HashMap::new();
    for (position, node) in graph.nodes.iter().enumerate() {
        let normalized = node
            .title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !normalized.is_empty() {
            by_normalized.entry(normalized).or_default().push(position);
        }
    }
    let duplicate_groups: Vec<&Vec<usize>> = by_normalized
        .values()
        .filter(|members| members.len() > 1)
        .collect();
    if !duplicate_groups.is_empty() {
        findings.push(AuditFinding {
            kind: "near_duplicate_titles".into(),
            severity: SEV_WARNING.into(),
            message: format!(
                "{} title(s) appear more than once after normalization — likely duplicate concepts",
                duplicate_groups.len()
            ),
            node_ids: duplicate_groups
                .iter()
                .flat_map(|group| group.iter())
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    // Non-atomic vocabulary: chapter/module-level words that do not belong in
    // an atomic concept graph (they surface as info, never as blocking).
    let non_atomic: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            let lower = node.title.to_lowercase();
            NON_ATOMIC_TERMS.iter().any(|term| lower.contains(term))
        })
        .map(|(position, _)| position)
        .collect();
    if !non_atomic.is_empty() {
        findings.push(AuditFinding {
            kind: "non_atomic_terms".into(),
            severity: SEV_INFO.into(),
            message: format!(
                "{} title(s) look like chapter/module/overview labels instead of atomic concepts",
                non_atomic.len()
            ),
            node_ids: non_atomic
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concept_graph::{
        ConceptGraphData, ConceptGraphEdge, ConceptGraphNode, DroppedEdge,
    };

    fn node(id: &str, level: Option<u8>, group: Option<&str>) -> ConceptGraphNode {
        ConceptGraphNode {
            id: id.to_owned(),
            title: id.to_owned(),
            level,
            group: group.map(str::to_owned),
            necessity: None,
            is_anchor: None,
        }
    }

    fn edge(from: &str, to: &str) -> ConceptGraphEdge {
        ConceptGraphEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            reason: None,
        }
    }

    fn graph(nodes: Vec<ConceptGraphNode>, edges: Vec<ConceptGraphEdge>) -> ConceptGraphData {
        ConceptGraphData {
            nodes,
            edges,
            audit: Default::default(),
        }
    }

    #[test]
    fn audit_flags_fragmented_graphs_and_shallow_leaves() {
        // Two disconnected chains: a (alone) and b -> c -> d -> e.
        let g = graph(
            vec![
                node("a", Some(1), Some("g1")),
                node("b", Some(1), Some("g1")),
                node("c", Some(1), Some("g1")),
                node("d", Some(1), Some("g1")),
                node("e", Some(1), Some("g1")),
            ],
            vec![edge("b", "c"), edge("c", "d"), edge("d", "e")],
        );
        let findings = audit_concept_graph(&g);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"disconnected_components"), "{kinds:?}");
        let disconnected = findings
            .iter()
            .find(|f| f.kind == "disconnected_components")
            .unwrap();
        // Fragmentation is human-ruled (R-level), not a regeneration trigger.
        assert_eq!(disconnected.severity, SEV_WARNING);
        assert_eq!(disconnected.node_ids, vec!["a".to_owned()]);
        // The b..e chain is 3 edges deep and e is the only sink: no shallow
        // leaves, no multi-sink findings.
        assert!(!kinds.contains(&"shallow_leaves"), "{kinds:?}");
        assert!(!kinds.contains(&"multiple_sinks"), "{kinds:?}");
    }

    #[test]
    fn audit_gates_coverage_and_reference_drops() {
        // A thin graph fails the coverage gate (danger -> regenerate).
        let thin = graph(
            vec![node("a", None, None), node("b", None, None)],
            vec![edge("a", "b")],
        );
        let findings = audit_concept_graph(&thin);
        let coverage = findings
            .iter()
            .find(|f| f.kind == "coverage")
            .expect("thin graph must fail the coverage gate");
        assert_eq!(coverage.severity, SEV_DANGER);

        // A graph whose naming is inconsistent fails the reference-drop gate.
        let mut sloppy = graph(
            vec![node("a", None, None), node("b", None, None)],
            vec![edge("a", "b")],
        );
        sloppy.audit.ref_drop_count = REF_DROP_COUNT_LIMIT + 1;
        sloppy.audit.ref_drop_rate = 0.5;
        sloppy.audit.dropped_edges.push(DroppedEdge {
            from: "ghost".into(),
            to: "b".into(),
            reason: "unknown reference".into(),
        });
        let findings = audit_concept_graph(&sloppy);
        let drops = findings
            .iter()
            .find(|f| f.kind == "excessive_reference_drops")
            .expect("sloppy naming must fail the reference-drop gate");
        assert_eq!(drops.severity, SEV_DANGER);
        assert!(drops.node_ids.contains(&"ghost".to_owned()));
    }

    #[test]
    fn audit_reports_cycle_defensively() {
        // b -> c, c -> b: a cycle (merge would remove it; this guards files).
        let g = graph(
            vec![node("b", Some(1), None), node("c", Some(1), None)],
            vec![edge("b", "c"), edge("c", "b")],
        );
        let findings = audit_concept_graph(&g);
        assert!(findings.iter().any(|f| f.kind == "cycle"), "{findings:?}");
        // Depth-based checks are skipped, so no shallow_depth finding.
        assert!(!findings.iter().any(|f| f.kind == "shallow_depth"));
    }

    #[test]
    fn audit_flags_duplicate_titles_and_non_atomic_terms() {
        let g = graph(
            vec![
                node("x", Some(1), None),
                node("y", Some(1), None),
                node("z", Some(1), None),
                node("w", Some(1), None),
            ],
            vec![edge("x", "y"), edge("y", "z"), edge("z", "w")],
        );
        let mut g = g;
        g.nodes[1].title = "Same Title".into();
        g.nodes[2].title = "same   title".into();
        g.nodes[3].title = "第3章 概述".into();
        let findings = audit_concept_graph(&g);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"near_duplicate_titles"), "{kinds:?}");
        assert!(kinds.contains(&"non_atomic_terms"), "{kinds:?}");
        let non_atomic = findings
            .iter()
            .find(|f| f.kind == "non_atomic_terms")
            .unwrap();
        assert_eq!(non_atomic.severity, SEV_INFO);
        assert_eq!(non_atomic.node_ids, vec!["w".to_owned()]);
    }
}
