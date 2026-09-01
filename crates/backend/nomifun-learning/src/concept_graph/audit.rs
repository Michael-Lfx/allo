//! Deterministic structural audit of the concept graph (zero model calls).
//! Findings are graded like a quality gate: `danger` findings (coverage,
//! coverage shortfall against the scope estimate, disconnected components,
//! orphaned units, tree-shaped networks, excessive reference drops, residual
//! cycles, unit overload) block publishing and drive the repair loop;
//! `warning` findings (fragmented sources/sinks, shallow structure,
//! duplicates) and `info` findings go to a human for a ruling. Every finding
//! carries its evidence node ids so the UI can jump to them.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{AuditFinding, ConceptGraphData};

pub(crate) const SEV_INFO: &str = "info";
pub(crate) const SEV_WARNING: &str = "warning";
pub(crate) const SEV_DANGER: &str = "danger";

/// Hard cap on one unit's estimated study time (minutes) — the feature's
/// workload budget: less is always fine, more is not. Reserved for
/// genuinely hard single lessons; everything else should stay within
/// [`UNIT_MINUTE_SOFT`].
pub(crate) const UNIT_MINUTE_CAP: u16 = 60;
/// Soft cap (minutes): most units should fit within it; 30-60 minute units
/// are legal (hard lessons) but flagged with a warning.
pub(crate) const UNIT_MINUTE_SOFT: u16 = 30;
/// Below this estimated minutes a unit is probably too thin to be a
/// standalone study session (info-level reference only).
const UNIT_MINUTE_FLOOR: u16 = 5;
/// Shared-title-substring threshold for the spiral check (chars): at least
/// this much overlap counts as "same topic".
const SPIRAL_TOPIC_MIN_SHARED: usize = 4;

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
/// many learning units; fewer means whole sub-domains were skipped — the
/// "missing sub-domains" failure mode, graded danger so the model extends
/// the network (via a repair round) instead of publishing a thin graph.
/// With a scope reference the bar SCALES with the block count (2 units per
/// block, floored at half this constant) — a narrow goal with a handful of
/// blocks must not be padded to a fixed size, and a very broad goal with
/// dozens of blocks must not pass at a fixed 30.
const MIN_UNITS: usize = 30;
/// Reference-drop gate: a model output where this share (or absolute count)
/// of prerequisite entries names no defined concept is too sloppy to trust
/// — the naming is inconsistent, so the whole graph gets regenerated.
const REF_DROP_RATE_LIMIT: f64 = 0.15;
const REF_DROP_COUNT_LIMIT: usize = 12;
/// Share of units expected to converge 2+ direct prerequisites; below this
/// the network is effectively a forest of chains, not a graph.
const MIN_MULTI_PARENT_SHARE: f64 = 0.1;
/// The tree check only applies to broad goals — small chains are
/// legitimately linear.
const MULTI_PARENT_MIN_NODES: usize = 30;
/// Scope blocks are large-block concepts (broad nouns / theme phrases,
/// e.g. "透视与空间", "几何概括临摹"), not exact unit names; a unit covers
/// a block when it shares at least this many consecutive characters (block
/// words run 2-6 chars, so 2 is a deliberately weak bar — the coverage
/// contract is completeness, and the repair loop adds units by block name,
/// rewritten into action sentences).
pub(crate) const BLOCK_MIN_SHARED: usize = 2;

/// Run every deterministic structural check. The graph is assumed acyclic
/// (merge removes cycles), but a defensive cycle check guards hand-edited
/// files: on a cycle the depth-based indicators are skipped.
pub(crate) fn audit_concept_graph(graph: &ConceptGraphData) -> Vec<AuditFinding> {
    audit_graph_core(graph, MIN_UNITS)
}

/// Structural audit plus the scope content checklist: the scope analysis'
/// large-block concepts become a coverage contract of their own — every
/// block must show up in at least one unit (danger, so the repair loop can
/// add units by block name). `None` skips the content check, matching the
/// pre-scope behavior. The coverage floor scales with the block count:
/// 2 units per block, floored at half the fixed [`MIN_UNITS`] constant.
pub(crate) fn audit_concept_graph_with_scope(
    graph: &ConceptGraphData,
    blocks: Option<&[String]>,
) -> Vec<AuditFinding> {
    let min_units = match blocks {
        Some(blocks) if !blocks.is_empty() => (blocks.len() * 2).max(MIN_UNITS / 2),
        _ => MIN_UNITS,
    };
    let mut findings = audit_graph_core(graph, min_units);

    // Scope content contract: the scope analysis handed the generation call
    // a block checklist, and the audit verifies every block is actually
    // covered — a model could skip half the blocks and still pass by padding
    // unrelated units. This check closes that gap deterministically (zero
    // model calls, like every check here).
    if let Some(blocks) = blocks {
        let missing: Vec<&String> = blocks
            .iter()
            .filter(|block| {
                !graph
                    .nodes
                    .iter()
                    .any(|node| common_substring_len(&node.title, block) >= BLOCK_MIN_SHARED)
            })
            .collect();
        if !missing.is_empty() {
            findings.push(AuditFinding {
                kind: "missing_block_coverage".into(),
                severity: SEV_DANGER.into(),
                message: format!(
                    "{} large-block concept(s) from the scope analysis are not covered by any unit — add a unit for each (rewrite the theme into an action sentence): {}",
                    missing.len(),
                    missing
                        .iter()
                        .map(|block| block.as_str())
                        .collect::<Vec<_>>()
                        .join("；")
                ),
                node_ids: Vec::new(),
            });
        }
    }

    findings
}

/// All structural checks (see [`audit_concept_graph`]). `min_units` is the
/// coverage floor (fixed without a scope reference, block-scaled with one).
fn audit_graph_core(graph: &ConceptGraphData, min_units: usize) -> Vec<AuditFinding> {
    let mut findings = Vec::new();
    let n = graph.nodes.len();
    // Empty graph is a hard failure, not a pass: the network was never
    // built (the model skipped construction or every patch was rejected).
    // Zero findings would let `cg_finish`/the legacy pipeline publish a
    // blank graph the UI renders as nothing — grade it danger so the
    // repair loop is handed the report and must build the units.
    if n == 0 {
        return vec![AuditFinding {
            kind: "empty_graph".into(),
            severity: SEV_DANGER.into(),
            message: "the graph has no learning units — the network was never built; add units covering the whole scope before finishing".into(),
            node_ids: Vec::new(),
        }];
    }

    // Coverage gate: fewer units than the goal demands means whole
    // sub-domains were skipped. Graded danger so the generation loop extends
    // the network instead of publishing a thin graph.
    if n < min_units {
        findings.push(AuditFinding {
            kind: "coverage".into(),
            severity: SEV_DANGER.into(),
            message: format!(
                "only {n} learning units — this goal needs at least {min_units}; whole sub-domains are likely missing"
            ),
            node_ids: Vec::new(),
        });
    }

    // Workload cap: a unit must fit at most 60 minutes — the hard budget
    // the whole feature is built on. Anything above it is a hard-cap
    // violation (danger): the model must split the unit, never quietly
    // raise the cap. 30 minutes is the SOFT cap: units up to 60 are
    // permitted for genuinely hard lessons, but anything above the soft
    // cap gets a warning so the repair loop knows a split is an option.
    let overloaded: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.min.map_or(false, |min| min > UNIT_MINUTE_CAP))
        .map(|(position, _)| position)
        .collect();
    if !overloaded.is_empty() {
        findings.push(AuditFinding {
            kind: "unit_overload".into(),
            severity: SEV_DANGER.into(),
            message: format!(
                "{} learning unit(s) exceed the {UNIT_MINUTE_CAP}-minute hard budget — split them into smaller units",
                overloaded.len()
            ),
            node_ids: overloaded
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    // Soft-cap warning: 30-60 minute units are legal (genuinely hard
    // lessons), but most units should stay within the soft cap. Warning
    // only — publishing is never blocked on it.
    let above_soft: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            node.min
                .map_or(false, |min| min > UNIT_MINUTE_SOFT && min <= UNIT_MINUTE_CAP)
        })
        .map(|(position, _)| position)
        .collect();
    if !above_soft.is_empty() {
        findings.push(AuditFinding {
            kind: "unit_above_soft_cap".into(),
            severity: SEV_WARNING.into(),
            message: format!(
                "{} learning unit(s) exceed the {UNIT_MINUTE_SOFT}-minute soft cap — fine for genuinely hard lessons, but consider splitting when possible",
                above_soft.len()
            ),
            node_ids: above_soft
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    // Fragment floor: a unit below 5 minutes is probably too thin to be a
    // study session on its own. Info-level reference: merging is a semantic
    // decision for a human (or a later repair round).
    let fragmented: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.min.map_or(false, |min| min < UNIT_MINUTE_FLOOR))
        .map(|(position, _)| position)
        .collect();
    if !fragmented.is_empty() {
        findings.push(AuditFinding {
            kind: "unit_fragment".into(),
            severity: SEV_INFO.into(),
            message: format!(
                "{} learning unit(s) estimate below {UNIT_MINUTE_FLOOR} minutes — probably too thin to be a standalone session",
                fragmented.len()
            ),
            node_ids: fragmented
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
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
                "{} prerequisite references were dropped ({} of all) — unit names are inconsistent",
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

    // Orphaned units: a dropped reference that took away a unit's ONLY
    // prerequisite turns it into a fake entry point — the reference-drop
    // rate cannot see this, because two drops out of a hundred can each
    // detach a whole subtree. Danger so the repair round reconnects them.
    let mut orphaned: Vec<usize> = graph
        .audit
        .dropped_edges
        .iter()
        .filter(|dropped| dropped.reason != "duplicate edge")
        .filter_map(|dropped| index.get(dropped.to.as_str()).copied())
        .filter(|&position| prereqs[position].is_empty())
        .collect();
    orphaned.sort_unstable();
    orphaned.dedup();
    if !orphaned.is_empty() {
        findings.push(AuditFinding {
            kind: "orphaned_units".into(),
            severity: SEV_DANGER.into(),
            message: format!(
                "{} unit(s) lost their only prerequisite to a dropped reference (name mismatch) and became fake entry points — reconnect each one to its true prerequisite",
                orphaned.len()
            ),
            node_ids: orphaned
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&position| graph.nodes[position].id.clone())
                .collect(),
        });
    }

    // Tree-shaped network: a real prerequisite graph converges — later
    // units routinely need 2+ earlier threads (conic sections need
    // coordinate geometry AND quadratic equations). When almost no unit has
    // multiple direct prerequisites the network is a forest of chains —
    // the "tree, not graph" failure mode.
    if n >= MULTI_PARENT_MIN_NODES {
        let multi_parent = prereqs.iter().filter(|parents| parents.len() >= 2).count();
        // Parenthesized: a bare `as f64 <` parses `<` as generic arguments.
        let multi_parent_share = multi_parent as f64 / n as f64;
        if multi_parent_share < MIN_MULTI_PARENT_SHARE {
            findings.push(AuditFinding {
                kind: "tree_structure".into(),
                severity: SEV_DANGER.into(),
                message: format!(
                    "only {multi_parent} of {n} units have 2+ direct prerequisites — the network is a tree/forest of chains, not a graph; add the converging dependencies (units that genuinely need two earlier threads)"
                ),
                node_ids: Vec::new(),
            });
        }
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
                        "{shallow}/{} leaf units sit only {SHALLOW_LEAF_DEPTH} layer(s) above a root — they are probably not decomposed far enough",
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
            // One connected structure is part of the generation contract, so
            // fragmentation blocks publishing and drives a repair round: the
            // model bridges the gap with link edges between existing units.
            // (The old human-ruled grading let broken graphs through the
            // automatic loop untouched — there is no human in that loop.)
            severity: SEV_DANGER.into(),
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
    // goal concept and fan out from a small root set. The caps scale with
    // the graph (one per ~15 units, floored at the constants): a 60-unit
    // network legitimately has several entry points and terminal branches,
    // and flagging them would push the model into bogus merges.
    let scale_cap = |fixed: usize| (n / 15).max(fixed);
    let max_sinks = scale_cap(MAX_SINKS);
    let max_sources = scale_cap(MAX_SOURCES);
    let sinks: Vec<usize> = succs
        .iter()
        .enumerate()
        .filter(|(_, succs)| succs.is_empty())
        .map(|(position, _)| position)
        .collect();
    if sinks.len() > max_sinks {
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
    if sources.len() > max_sources {
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
                "{} unit(s) with fan-in > {MAX_FAN_IN} or fan-out > {MAX_FAN_OUT} — likely over-abstracted",
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
                "{} title(s) appear more than once after normalization — likely duplicate units",
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

    // Spiral check: the same topic legitimately recurs at several depths
    // ("用配方法解一元二次方程" then "用求根公式解一元二次方程"), and those
    // units must sit on ONE dependency chain (one reachable from the other).
    // Same-topic units with no chain relation are same-layer duplicates —
    // info-level, since merging/renaming is a semantic human decision.
    let mut spiral: Vec<usize> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            if common_substring_len(&graph.nodes[i].title, &graph.nodes[j].title)
                < SPIRAL_TOPIC_MIN_SHARED
            {
                continue;
            }
            // Same topic; check whether they sit on one dependency chain.
            let on_chain = reachable_in(&prereqs, i, j) || reachable_in(&prereqs, j, i);
            if !on_chain {
                spiral.push(i);
                spiral.push(j);
            }
        }
    }
    spiral.sort_unstable();
    spiral.dedup();
    if !spiral.is_empty() {
        findings.push(AuditFinding {
            kind: "spiral_clash".into(),
            severity: SEV_INFO.into(),
            message: format!(
                "{} learning unit(s) revisit a topic outside any dependency chain — same-topic units should stack (one a prerequisite of the next), not sit side by side",
                spiral.len()
            ),
            node_ids: spiral
                .iter()
                .take(MAX_EVIDENCE)
                .map(|&node| graph.nodes[node].id.clone())
                .collect(),
        });
    }

    // Redundant edges: a direct prerequisite edge (u -> v) is redundant when
    // v stays reachable from u along a longer path — the direct edge shadows
    // a finer-grained staircase. Reported as info (reference only): a
    // shortcut may be semantically intentional (e.g. "逻辑 -> 离散数学"), so
    // the decision to remove it stays with a human.
    let mut redundant: Vec<(String, String)> = Vec::new();
    for edge in &graph.edges {
        let (Some(&from), Some(&to)) = (index.get(edge.from.as_str()), index.get(edge.to.as_str()))
        else {
            continue;
        };
        // BFS from `from`, ignoring the direct edge under test.
        let mut seen = vec![false; n];
        seen[from] = true;
        let mut queue = VecDeque::from(vec![from]);
        let mut reachable = false;
        while let Some(node) = queue.pop_front() {
            for &next in &succs[node] {
                if next == to {
                    if node == from {
                        continue; // the direct edge itself
                    }
                    reachable = true;
                    break;
                }
                if !seen[next] {
                    seen[next] = true;
                    queue.push_back(next);
                }
            }
            if reachable {
                break;
            }
        }
        if reachable {
            redundant.push((edge.from.clone(), edge.to.clone()));
        }
    }
    if !redundant.is_empty() {
        findings.push(AuditFinding {
            kind: "redundant_edges".into(),
            severity: SEV_INFO.into(),
            message: format!(
                "{} direct prerequisite edge(s) shadow a longer existing path — review whether the shortcut is intentional",
                redundant.len()
            ),
            node_ids: redundant
                .iter()
                .take(MAX_EVIDENCE)
                .map(|(from, _)| from.clone())
                .collect(),
        });
    }

    findings
}

/// Longest common contiguous substring length (char-based).
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

/// BFS along the prerequisite direction: is `to` an ancestor of `from`?
fn reachable_in(prereqs: &[Vec<usize>], from: usize, to: usize) -> bool {
    let mut seen = vec![false; prereqs.len()];
    let mut stack = vec![from];
    seen[from] = true;
    while let Some(node) = stack.pop() {
        for &prev in &prereqs[node] {
            if prev == to {
                return true;
            }
            if !seen[prev] {
                seen[prev] = true;
                stack.push(prev);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concept_graph::{
        ConceptGraphData, ConceptGraphEdge, ConceptGraphNode, DroppedEdge,
    };

    fn node(id: &str, min: Option<u16>) -> ConceptGraphNode {
        ConceptGraphNode {
            id: id.to_owned(),
            title: id.to_owned(),
            min,
            group: None,
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
                node("a", None),
                node("b", None),
                node("c", None),
                node("d", None),
                node("e", None),
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
        // Fragmentation blocks publishing and drives a repair round —
        // the automatic loop has no human to rule on warnings.
        assert_eq!(disconnected.severity, SEV_DANGER);
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
            vec![node("a", None), node("b", None)],
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
            vec![node("a", None), node("b", None)],
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
    fn audit_scales_the_coverage_gate_with_the_scope_checklist() {
        // Without a scope reference the floor is the fixed 30-unit gate.
        let chain = |count: usize| {
            let nodes: Vec<ConceptGraphNode> =
                (0..count).map(|i| node(&format!("u{i}"), Some(10))).collect();
            let edges: Vec<ConceptGraphEdge> = (0..count.saturating_sub(1))
                .map(|i| edge(&format!("u{i}"), &format!("u{}", i + 1)))
                .collect();
            graph(nodes, edges)
        };
        assert!(
            audit_concept_graph(&chain(15)).iter().any(|f| f.kind == "coverage"),
            "15 units fail the fixed 30-unit gate"
        );

        // A narrow goal with 7 scope blocks lowers the bar to 14 — the 15
        // floor wins only on ties, so 15 units clear the scaled gate where
        // the fixed 30-unit gate would have forced 15 more units of padding.
        let blocks: Vec<String> = (0..7).map(|i| format!("块{i}")).collect();
        let findings = audit_concept_graph_with_scope(&chain(15), Some(&blocks));
        assert!(
            !findings.iter().any(|f| f.kind == "coverage"),
            "15 units clear the 14-unit scaled gate: {findings:?}"
        );

        // A broad goal with 25 blocks RAISES the bar to 50.
        let blocks: Vec<String> = (0..25).map(|i| format!("块{i}")).collect();
        let findings = audit_concept_graph_with_scope(&chain(30), Some(&blocks));
        let coverage = findings
            .iter()
            .find(|f| f.kind == "coverage")
            .expect("30 units must fail the 50-unit scaled gate");
        assert!(coverage.message.contains("50"), "{}", coverage.message);
    }

    #[test]
    fn audit_relaxes_the_sink_and_source_caps_for_large_graphs() {
        // A 60-unit graph shaped as one long spine with `terminals` separate
        // branches hanging off the root: the scaled cap is 60/15 = 4, so 4
        // terminals sit exactly at the cap while 5 exceed it — neither count
        // would pass the fixed cap of 3.
        let branchy = |terminals: usize| {
            let nodes: Vec<ConceptGraphNode> =
                (0..60).map(|i| node(&format!("u{i}"), Some(10))).collect();
            // Spine u0 -> u1 -> ... -> u(59-terminals); every node on it has
            // an outgoing edge.
            let mut edges: Vec<ConceptGraphEdge> = (0..(60 - terminals))
                .map(|i| edge(&format!("u{i}"), &format!("u{}", i + 1)))
                .collect();
            // The last `terminals` nodes hang directly off the root and are
            // the only sinks.
            for terminal in 0..terminals {
                edges.push(edge("u0", &format!("u{}", 59 - terminal)));
            }
            graph(nodes, edges)
        };
        // 4 terminals on 60 units: exactly at the scaled cap — no finding.
        let findings = audit_concept_graph(&branchy(4));
        assert!(
            !findings.iter().any(|f| f.kind == "multiple_sinks"),
            "{findings:?}"
        );
        // 5 terminals on 60 units: one over the scaled cap — flagged.
        let findings = audit_concept_graph(&branchy(5));
        assert!(
            findings.iter().any(|f| f.kind == "multiple_sinks"),
            "{findings:?}"
        );
    }

    #[test]
    fn audit_reports_cycle_defensively() {
        // b -> c, c -> b: a cycle (merge would remove it; this guards files).
        let g = graph(
            vec![node("b", None), node("c", None)],
            vec![edge("b", "c"), edge("c", "b")],
        );
        let findings = audit_concept_graph(&g);
        assert!(findings.iter().any(|f| f.kind == "cycle"), "{findings:?}");
        // Depth-based checks are skipped, so no shallow_depth finding.
        assert!(!findings.iter().any(|f| f.kind == "shallow_depth"));
    }

    #[test]
    fn audit_flags_duplicate_titles() {
        let g = graph(
            vec![
                node("x", None),
                node("y", None),
                node("z", None),
                node("w", None),
            ],
            vec![edge("x", "y"), edge("y", "z"), edge("z", "w")],
        );
        let mut g = g;
        g.nodes[1].title = "Same Title".into();
        g.nodes[2].title = "same   title".into();
        let findings = audit_concept_graph(&g);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"near_duplicate_titles"), "{kinds:?}");
        let duplicates = findings
            .iter()
            .find(|f| f.kind == "near_duplicate_titles")
            .unwrap();
        assert_eq!(duplicates.severity, SEV_WARNING);
        assert_eq!(duplicates.node_ids, vec!["y".to_owned(), "z".to_owned()]);
    }

    #[test]
    fn audit_flags_unit_overload_as_danger() {
        // A unit above the 60-minute hard cap is a hard-budget violation.
        let g = graph(
            vec![
                node("用配方法解一元二次方程", Some(15)),
                node("用判别式判定一元二次方程根的性质", Some(70)),
            ],
            vec![],
        );
        let findings = audit_concept_graph(&g);
        let overload = findings
            .iter()
            .find(|f| f.kind == "unit_overload")
            .expect("70-minute unit must be flagged");
        assert_eq!(overload.severity, SEV_DANGER);
        assert_eq!(
            overload.node_ids,
            vec!["用判别式判定一元二次方程根的性质".to_owned()]
        );
        // The 15-minute unit stays clean.
        assert!(!findings.iter().any(|f| f.kind == "unit_fragment"));
    }

    #[test]
    fn audit_flags_units_above_soft_cap_as_warning_only() {
        // 30-60 minute units are legal (hard lessons): warning, never a gate.
        let g = graph(
            vec![
                node("用配方法解一元二次方程", Some(15)),
                node("构造三元组求不定方程整数解", Some(45)),
            ],
            vec![],
        );
        let findings = audit_concept_graph(&g);
        let soft = findings
            .iter()
            .find(|f| f.kind == "unit_above_soft_cap")
            .expect("45-minute unit must hit the soft-cap warning");
        assert_eq!(soft.severity, SEV_WARNING);
        assert_eq!(soft.node_ids, vec!["构造三元组求不定方程整数解".to_owned()]);
        assert!(
            !findings.iter().any(|f| f.kind == "unit_overload"),
            "45 minutes is within the 60-minute hard cap"
        );
    }

    #[test]
    fn audit_flags_unit_fragment_as_info() {
        // Below 5 minutes a unit is probably too thin; 5 itself is fine.
        let g = graph(
            vec![
                node("比较两个分数的大小", Some(3)),
                node("通分", Some(5)),
            ],
            vec![edge("通分", "比较两个分数的大小")],
        );
        let findings = audit_concept_graph(&g);
        let fragment = findings
            .iter()
            .find(|f| f.kind == "unit_fragment")
            .expect("3-minute unit must be flagged");
        assert_eq!(fragment.severity, SEV_INFO);
        assert_eq!(fragment.node_ids, vec!["比较两个分数的大小".to_owned()]);
    }

    #[test]
    fn audit_flags_spiral_units_outside_a_chain() {
        // Same topic, no dependency chain: same-layer duplicates.
        let clash = graph(
            vec![
                node("用配方法解一元二次方程", Some(15)),
                node("用求根公式解一元二次方程", Some(15)),
            ],
            vec![],
        );
        let findings = audit_concept_graph(&clash);
        let spiral = findings
            .iter()
            .find(|f| f.kind == "spiral_clash")
            .expect("side-by-side same-topic units must be flagged");
        assert_eq!(spiral.severity, SEV_INFO);
        assert_eq!(spiral.node_ids.len(), 2);

        // Same topic stacked on one chain: legitimate spiral, no finding.
        let chain = graph(
            vec![
                node("用配方法解一元二次方程", Some(15)),
                node("用求根公式解一元二次方程", Some(15)),
            ],
            vec![edge("用配方法解一元二次方程", "用求根公式解一元二次方程")],
        );
        let findings = audit_concept_graph(&chain);
        assert!(
            !findings.iter().any(|f| f.kind == "spiral_clash"),
            "{findings:?}"
        );
    }

    #[test]
    fn audit_flags_redundant_edges() {
        // a -> d shadows the longer staircase a -> b -> c -> d.
        let g = graph(
            vec![
                node("a", None),
                node("b", None),
                node("c", None),
                node("d", None),
            ],
            vec![
                edge("a", "b"),
                edge("b", "c"),
                edge("c", "d"),
                edge("a", "d"),
            ],
        );
        let findings = audit_concept_graph(&g);
        let redundant = findings
            .iter()
            .find(|f| f.kind == "redundant_edges")
            .expect("a->d must be flagged as redundant");
        // Reference-level finding, human decides whether the shortcut is kept.
        assert_eq!(redundant.severity, SEV_INFO);
        // Only the shadowing edge endpoint is evidence; staircase edges are fine.
        assert_eq!(redundant.node_ids, vec!["a".to_owned()]);
    }

    #[test]
    fn audit_flags_tree_structured_networks() {
        // 30 chained units, every unit single-parent: the tree gate fires.
        let nodes: Vec<ConceptGraphNode> =
            (0..30).map(|i| node(&format!("u{i}"), Some(10))).collect();
        let edges: Vec<ConceptGraphEdge> = (0..29)
            .map(|i| edge(&format!("u{i}"), &format!("u{}", i + 1)))
            .collect();
        let g = graph(nodes, edges);
        let findings = audit_concept_graph(&g);
        let tree = findings
            .iter()
            .find(|f| f.kind == "tree_structure")
            .expect("a pure chain of 30 units is a tree, not a graph");
        assert_eq!(tree.severity, SEV_DANGER);

        // Three converging units (3/30 = 10%) clear the gate.
        let nodes: Vec<ConceptGraphNode> =
            (0..30).map(|i| node(&format!("w{i}"), Some(10))).collect();
        let mut edges: Vec<ConceptGraphEdge> = (0..29)
            .map(|i| edge(&format!("w{i}"), &format!("w{}", i + 1)))
            .collect();
        for target in [5, 10, 15] {
            edges.push(edge("w0", &format!("w{target}")));
        }
        let g = graph(nodes, edges);
        let findings = audit_concept_graph(&g);
        assert!(!findings.iter().any(|f| f.kind == "tree_structure"), "{findings:?}");
    }

    #[test]
    fn audit_flags_orphans_created_by_dropped_references() {
        // "a" emitted a prerequisite that no defined unit matched, so it
        // became a fake entry point; the b -> c edge is intact, so only
        // "a" is flagged.
        let mut g = graph(
            vec![node("a", None), node("b", None), node("c", None)],
            vec![edge("b", "c")],
        );
        g.audit.dropped_edges.push(DroppedEdge {
            from: "ghost".into(),
            to: "a".into(),
            reason: "unknown reference".into(),
        });
        g.audit.ref_drop_count = 1;
        g.audit.ref_drop_rate = 0.05;
        let findings = audit_concept_graph(&g);
        let orphaned = findings
            .iter()
            .find(|f| f.kind == "orphaned_units")
            .expect("a unit whose only prerequisite was dropped must be flagged");
        assert_eq!(orphaned.severity, SEV_DANGER);
        assert_eq!(orphaned.node_ids, vec!["a".to_owned()]);
    }

    #[test]
    fn audit_gates_missing_blocks_as_danger() {
        let g = graph(
            vec![
                node("用配方法解一元二次方程", Some(15)),
                node("用求根公式解一元二次方程", Some(15)),
            ],
            vec![edge("用配方法解一元二次方程", "用求根公式解一元二次方程")],
        );
        // Scope blocks are large-block concepts, not unit names: a block
        // that appears in no unit title is flagged (the repair loop adds a
        // unit by block name, rewritten into an action sentence). Sharing
        // only a theme word — e.g. "配方法" — counts as covered by design;
        // the fuzzy overlap bar is 2 chars.
        let blocks = vec!["拉格朗日乘数法".to_owned(), "可导与连续".to_owned()];
        let findings = audit_concept_graph_with_scope(&g, Some(&blocks));
        let missing = findings
            .iter()
            .find(|f| f.kind == "missing_block_coverage")
            .expect("uncovered blocks must be flagged");
        assert_eq!(missing.severity, SEV_DANGER);
        assert!(missing.message.contains("拉格朗日乘数法"), "{}", missing.message);
        assert!(missing.message.contains("可导与连续"), "{}", missing.message);

        // A block with a near-miss unit name (reworded by the generator)
        // clears the gate: theme-word overlap counts as covered.
        let blocks = vec!["配方法".to_owned()];
        let findings = audit_concept_graph_with_scope(&g, Some(&blocks));
        assert!(
            !findings.iter().any(|f| f.kind == "missing_block_coverage"),
            "{findings:?}"
        );

        // The plain audit entry point carries no content checklist.
        let findings = audit_concept_graph(&g);
        assert!(!findings.iter().any(|f| f.kind == "missing_block_coverage"), "{findings:?}");
    }
}
