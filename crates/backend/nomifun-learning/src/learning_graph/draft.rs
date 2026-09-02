//! Draft-graph kernel: pure in-memory graph manipulation powering the agent
//! tool set (`lg_patch` and the query tools live in `nomifun-ai-agent`).
//! Zero IO, zero model calls: every operation reuses the crate's
//! deterministic normalization, merge and audit logic, so the agent edits
//! through exactly the gates the legacy pipeline enforces.
//!
//! A draft starts EMPTY (topic + optional scope reference from the scope
//! analysis); the agent builds the whole graph with batched [`GraphOp`]s.
//! Ops apply strictly in order — a later op may reference nodes an earlier
//! op in the same batch created — and each op is validated before it
//! touches the graph: unknown references, duplicate names, self loops and
//! over-budget minutes are rejected with a reason the model can act on;
//! operations that would close a cycle are rolled back whole (the graph
//! must stay a DAG at every step, the same invariant `finalize_graph`
//! enforces on the legacy paths). After a batch the deterministic audit
//! re-runs, so every [`PatchReport`] carries a fresh gate snapshot.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::audit::{
    BLOCK_MIN_SHARED, UNIT_MINUTE_CAP, audit_learning_graph_with_scope,
    common_substring_len,
};
use super::{
    AuditFinding, LearningGraphData, LearningGraphEdge, LearningGraphNode, RawConcept, ScopeAnalysis,
    format_audit_report, fuzzy_resolve_reference, merge_batch, normalize_batch,
    remove_cycle_edges,
};

// ── GraphOp: the model-facing operation vocabulary ─────────────────────────

/// One batched edit on the draft graph. Serialized as a tagged JSON object
/// (`{"op": "add", ...}`), tolerant of a bare string where a list is
/// expected and a digit string for `min` — same tolerance the legacy raw
/// parse applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GraphOp {
    /// Insert a brand-new unit. Every `pre` name must already exist in the
    /// graph (or be created by an earlier op of the same batch).
    Add {
        name: String,
        #[serde(default, deserialize_with = "de_string_list")]
        pre: Vec<String>,
        #[serde(default, deserialize_with = "de_opt_min")]
        min: Option<u16>,
    },
    /// Draw a prerequisite edge `from -> to` between two existing units.
    Link {
        from: String,
        to: String,
    },
    /// Remove a single prerequisite edge `from -> to` between two existing
    /// units — the edge-level counterpart of [`GraphOp::Delete`]. Whether
    /// the change orphans a unit is decided by the audit, not by the
    /// unlink.
    Unlink {
        from: String,
        to: String,
    },
    /// Flip an existing prerequisite edge: `from -> to` becomes
    /// `to -> from`.
    Reverse {
        from: String,
        to: String,
    },
    /// Replace the full prerequisite set of an existing unit. Every `pre`
    /// name must already exist in the graph (or be created by an earlier op
    /// of the same batch); duplicates are tolerated (deduped). Whether the
    /// new set is sufficient — or leaves the unit without prerequisites —
    /// is the audit's call.
    SetPre {
        target: String,
        #[serde(default, deserialize_with = "de_string_list")]
        pre: Vec<String>,
    },
    /// Replace one unit by several finer units. A replacement may name its
    /// own prerequisites (existing units or other replacements); a
    /// replacement with an empty `pre` inherits the split target's
    /// prerequisites instead. Units that depended on the target now depend
    /// on the ENTRY replacements — those no replacement depends on.
    Split {
        target: String,
        into: Vec<SplitUnit>,
    },
    /// Fold several units into one: every edge that touched a target is
    /// redirected onto `into`, then the targets are removed.
    Merge {
        into: String,
        targets: Vec<String>,
    },
    /// Rename a unit and/or change its minute budget. Renaming rewrites
    /// every edge that referenced the old name.
    Update {
        target: String,
        name: Option<String>,
        #[serde(default, deserialize_with = "de_opt_min")]
        min: Option<u16>,
    },
    /// Remove a unit and every edge touching it. Units that listed it as a
    /// prerequisite lose that entry — whether that orphans them is decided
    /// by the audit, not by the delete.
    Delete {
        target: String,
    },
}

/// One replacement unit inside a `split`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitUnit {
    pub name: String,
    #[serde(default, deserialize_with = "de_string_list")]
    pub pre: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_min")]
    pub min: Option<u16>,
}

/// Tolerate `"pre": "single name"` (or `null`/absence) where the shape asks
/// for an array — the same leniency `RawConcept` applies.
fn de_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
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

/// Tolerate minutes as a number or a digit string (same as `RawConcept`).
fn de_opt_min<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
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

// ── The draft ──────────────────────────────────────────────────────────────

/// A draft graph under construction. Held in memory by the service layer
/// (generation is a short-lived operation; drafts do not survive restarts).
#[derive(Debug, Clone)]
pub(crate) struct DraftGraph {
    pub topic: String,
    pub scope: Option<ScopeAnalysis>,
    pub graph: LearningGraphData,
    /// Increments once per accepted op, so tool callers can detect
    /// concurrent edits.
    pub revision: u64,
}

impl DraftGraph {
    pub(crate) fn new(topic: String, scope: Option<ScopeAnalysis>) -> Self {
        let mut draft = Self {
            topic,
            scope,
            graph: LearningGraphData {
                nodes: Vec::new(),
                edges: Vec::new(),
                audit: Default::default(),
            },
            revision: 0,
        };
        // A fresh draft is ALREADY audited: an empty graph carries its
        // empty_graph danger from birth, so a premature `lg_finish` on a
        // never-patched draft is rejected instead of publishing blank.
        draft.refresh_audit();
        draft
    }

    /// Apply a batch of operations in order and re-run the audit gate.
    /// Accepted ops mutate the draft and bump `revision`; rejected ops are
    /// reported with a reason and leave the graph untouched.
    pub(crate) fn apply_ops(&mut self, ops: Vec<GraphOp>) -> PatchReport {
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for op in ops {
            match self.apply_one(&op) {
                Ok(outcome) => accepted.push(outcome),
                Err(reason) => rejected.push(RejectedOp { op, reason }),
            }
        }
        self.refresh_audit();
        PatchReport {
            revision: self.revision,
            accepted,
            rejected,
            audit_summary: summarize_findings(&self.graph.audit.findings),
        }
    }

    fn apply_one(&mut self, op: &GraphOp) -> Result<OpOutcome, String> {
        match op {
            GraphOp::Add { name, pre, min } => self.op_add(name, pre, *min),
            GraphOp::Link { from, to } => self.op_link(from, to),
            GraphOp::Unlink { from, to } => self.op_unlink(from, to),
            GraphOp::Reverse { from, to } => self.op_reverse(from, to),
            GraphOp::SetPre { target, pre } => self.op_set_pre(target, pre),
            GraphOp::Split { target, into } => self.op_split(target, into),
            GraphOp::Merge { into, targets } => self.op_merge(into, targets),
            GraphOp::Update { target, name, min } => self.op_update(target, name.as_deref(), *min),
            GraphOp::Delete { target } => self.op_delete(target),
        }
    }

    fn op_add(&mut self, name: &str, pre: &[String], min: Option<u16>) -> Result<OpOutcome, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("add: unit name must not be empty".into());
        }
        if self.node_exists(name) {
            return Err(format!("add: unit '{name}' already exists"));
        }
        if let Some(min) = min {
            if min > UNIT_MINUTE_CAP {
                return Err(format!(
                    "add: min {min} exceeds the {UNIT_MINUTE_CAP}-minute hard cap"
                ));
            }
        }
        let mut pre_refs: Vec<String> = Vec::with_capacity(pre.len());
        for raw in pre {
            let reference = raw.trim();
            if reference.is_empty() {
                return Err(format!("add: empty prerequisite on '{name}'"));
            }
            if reference == name {
                return Err(format!("add: '{name}' cannot depend on itself"));
            }
            if !self.node_exists(reference) {
                let hint = self
                    .closest(reference)
                    .map(|c| format!("; closest existing unit: '{c}'"))
                    .unwrap_or_default();
                return Err(format!("add: unknown prerequisite '{reference}'{hint}"));
            }
            pre_refs.push(reference.to_owned());
        }
        let nodes_before = self.graph.nodes.len();
        let edges_before = self.graph.edges.len();
        // Reuse the crate's normalization + merge so the drop statistics
        // accumulate exactly like every other path (all pre-checks passed,
        // so nothing is actually dropped).
        let raw = RawConcept {
            name: name.to_owned(),
            pre: pre_refs,
            min,
        };
        let keys: HashSet<String> = self.graph.nodes.iter().map(|node| node.id.clone()).collect();
        let batch = normalize_batch(std::slice::from_ref(&raw), &keys);
        self.graph = merge_batch(&self.graph, &batch);
        self.revision += 1;
        let minutes = min.map(|m| m.to_string()).unwrap_or_else(|| "未标注".into());
        // Near-miss references that fuzzy-resolved are reported back so the
        // model knows which node its reference actually attached to.
        let fuzzy_note = if batch.fuzzy_resolved.is_empty() {
            String::new()
        } else {
            let pairs: Vec<String> = batch
                .fuzzy_resolved
                .iter()
                .map(|(emitted, resolved)| format!("'{emitted}' → '{resolved}'"))
                .collect();
            format!("（模糊引用解析：{}——已按解析后的节点建边）", pairs.join("、"))
        };
        Ok(OpOutcome {
            op: "add".into(),
            summary: format!(
                "已添加单元 '{name}'（前置 {} 个，预算 {minutes} 分钟）{fuzzy_note}",
                pre.len()
            ),
            node_delta: self.graph.nodes.len() as i64 - nodes_before as i64,
            edge_delta: self.graph.edges.len() as i64 - edges_before as i64,
        })
    }

    fn op_link(&mut self, from: &str, to: &str) -> Result<OpOutcome, String> {
        let from = from.trim();
        let to = to.trim();
        if !self.node_exists(from) {
            return Err(format!("link: unknown unit '{from}'"));
        }
        if !self.node_exists(to) {
            return Err(format!("link: unknown unit '{to}'"));
        }
        if from == to {
            return Err(format!("link: '{from}' cannot depend on itself"));
        }
        if self.edge_exists(from, to) {
            return Err(format!("link: edge '{from} -> {to}' already exists"));
        }
        let snapshot = self.graph.clone();
        let nodes_before = self.graph.nodes.len();
        let edges_before = self.graph.edges.len();
        self.push_edge(from, to);
        if let Err(cycle) = self.reject_if_cycle() {
            self.graph = snapshot;
            return Err(cycle);
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "link".into(),
            summary: format!("已建立依赖 {from} -> {to}"),
            node_delta: self.graph.nodes.len() as i64 - nodes_before as i64,
            edge_delta: self.graph.edges.len() as i64 - edges_before as i64,
        })
    }

    fn op_unlink(&mut self, from: &str, to: &str) -> Result<OpOutcome, String> {
        let from = from.trim();
        let to = to.trim();
        if !self.node_exists(from) {
            return Err(format!("unlink: unknown unit '{from}'"));
        }
        if !self.node_exists(to) {
            return Err(format!("unlink: unknown unit '{to}'"));
        }
        if !self.edge_exists(from, to) {
            return Err(format!("unlink: no edge '{from} -> {to}' to remove"));
        }
        let edges_before = self.graph.edges.len();
        self.graph
            .edges
            .retain(|edge| !(edge.from == from && edge.to == to));
        self.revision += 1;
        Ok(OpOutcome {
            op: "unlink".into(),
            summary: format!(
                "已移除依赖 {from} -> {to}；{to} 是否因此失去前置由审计判定"
            ),
            node_delta: 0,
            edge_delta: self.graph.edges.len() as i64 - edges_before as i64,
        })
    }

    fn op_reverse(&mut self, from: &str, to: &str) -> Result<OpOutcome, String> {
        let from = from.trim();
        let to = to.trim();
        if !self.node_exists(from) {
            return Err(format!("reverse: unknown unit '{from}'"));
        }
        if !self.node_exists(to) {
            return Err(format!("reverse: unknown unit '{to}'"));
        }
        if from == to {
            return Err(format!("reverse: '{from}' cannot depend on itself"));
        }
        if !self.edge_exists(from, to) {
            return Err(format!("reverse: no edge '{from} -> {to}' to reverse"));
        }
        if self.edge_exists(to, from) {
            return Err(format!(
                "reverse: edge '{to} -> {from}' already exists; reversing would duplicate it"
            ));
        }
        let snapshot = self.graph.clone();
        let nodes_before = self.graph.nodes.len();
        let edges_before = self.graph.edges.len();
        if let Some(edge) = self.graph.edges.iter_mut().find(|edge| {
            edge.from == from && edge.to == to
        }) {
            edge.from = to.to_owned();
            edge.to = from.to_owned();
        }
        if let Err(cycle) = self.reject_if_cycle() {
            self.graph = snapshot;
            return Err(cycle);
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "reverse".into(),
            summary: format!("已反转依赖 {from} -> {to} 为 {to} -> {from}"),
            node_delta: self.graph.nodes.len() as i64 - nodes_before as i64,
            edge_delta: self.graph.edges.len() as i64 - edges_before as i64,
        })
    }

    fn op_set_pre(&mut self, target: &str, pre: &[String]) -> Result<OpOutcome, String> {
        let target = target.trim();
        if !self.node_exists(target) {
            return Err(format!("set_pre: unknown unit '{target}'"));
        }
        let mut pre_refs: Vec<String> = Vec::with_capacity(pre.len());
        let mut seen: HashSet<String> = HashSet::new();
        for raw in pre {
            let reference = raw.trim();
            if reference.is_empty() {
                return Err(format!("set_pre: empty prerequisite on '{target}'"));
            }
            if reference == target {
                return Err(format!("set_pre: '{target}' cannot depend on itself"));
            }
            if !self.node_exists(reference) {
                let hint = self
                    .closest(reference)
                    .map(|c| format!("; closest existing unit: '{c}'"))
                    .unwrap_or_default();
                return Err(format!("set_pre: unknown prerequisite '{reference}'{hint}"));
            }
            if seen.insert(reference.to_owned()) {
                pre_refs.push(reference.to_owned());
            }
        }
        // A full-set replacement can close a cycle (pointing the target at
        // one of its own descendants), so the whole op rolls back on one.
        let snapshot = self.graph.clone();
        let edges_before = self.graph.edges.len();
        let replaced = self
            .graph
            .edges
            .iter()
            .filter(|edge| edge.to == target)
            .count();
        self.graph.edges.retain(|edge| edge.to != target);
        for reference in &pre_refs {
            self.push_edge(reference, target);
        }
        if let Err(cycle) = self.reject_if_cycle() {
            self.graph = snapshot;
            return Err(cycle);
        }
        self.revision += 1;
        let new_set = if pre_refs.is_empty() {
            "空（成为入门单元）".to_owned()
        } else {
            pre_refs.join("、")
        };
        Ok(OpOutcome {
            op: "set_pre".into(),
            summary: format!(
                "已将 '{target}' 的前置整体替换为 [{}]（移除 {replaced} 条旧前置边）；\
                 新集合是否充分由审计判定",
                new_set
            ),
            node_delta: 0,
            edge_delta: self.graph.edges.len() as i64 - edges_before as i64,
        })
    }

    fn op_split(&mut self, target: &str, into: &[SplitUnit]) -> Result<OpOutcome, String> {
        let target = target.trim();
        if !self.node_exists(target) {
            return Err(format!("split: unknown unit '{target}'"));
        }
        if into.is_empty() {
            return Err("split: needs at least one replacement unit".into());
        }
        // Validate the whole replacement list before touching the graph.
        let mut seen: HashSet<&str> = HashSet::new();
        for unit in into {
            let name = unit.name.trim();
            if name.is_empty() {
                return Err("split: replacement unit name must not be empty".into());
            }
            if !seen.insert(name) {
                return Err(format!("split: replacement unit '{name}' appears twice"));
            }
            if name == target {
                return Err(format!(
                    "split: replacement unit must not reuse the target name '{target}'"
                ));
            }
            if self.node_exists(name) {
                return Err(format!(
                    "split: replacement unit '{name}' already exists in the graph"
                ));
            }
            if let Some(min) = unit.min {
                if min > UNIT_MINUTE_CAP {
                    return Err(format!(
                        "split: min {min} on '{name}' exceeds the {UNIT_MINUTE_CAP}-minute hard cap"
                    ));
                }
            }
            for raw in &unit.pre {
                let reference = raw.trim();
                if reference.is_empty() {
                    return Err(format!(
                        "split: empty prerequisite on replacement '{name}'"
                    ));
                }
                if reference == name {
                    return Err(format!(
                        "split: replacement unit '{name}' cannot depend on itself"
                    ));
                }
                if reference == target {
                    return Err(format!(
                        "split: replacement unit '{name}' must not depend on the split target \
                         '{target}'; name its actual prerequisites instead"
                    ));
                }
                let is_sibling = into.iter().any(|u| u.name.trim() == reference);
                if !is_sibling && !self.node_exists(reference) {
                    let hint = self
                        .closest(reference)
                        .map(|c| format!("; closest existing unit: '{c}'"))
                        .unwrap_or_default();
                    return Err(format!(
                        "split: unknown prerequisite '{reference}' on replacement '{name}'{hint}"
                    ));
                }
            }
        }
        // Replacement-internal references must be acyclic and leave at least
        // one ENTRY unit (nothing else in the list depends on it) so the
        // target's dependents can be redirected onto it.
        let mut internal: Vec<(String, String)> = Vec::new();
        for unit in into {
            let to = unit.name.trim();
            for raw in &unit.pre {
                let reference = raw.trim();
                if into.iter().any(|u| u.name.trim() == reference) {
                    internal.push((reference.to_owned(), to.to_owned()));
                }
            }
        }
        let order: Vec<String> = into.iter().map(|u| u.name.trim().to_owned()).collect();
        let cycle_dropped = remove_cycle_edges(&order, &internal).1;
        if !cycle_dropped.is_empty() {
            let list = cycle_dropped
                .iter()
                .map(|(f, t)| format!("{f} -> {t}"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "split: replacement units form a dependency cycle ({list}); leave at least one \
                 entry unit that no replacement depends on"
            ));
        }
        // ENTRY replacements are the chain heads: units that depend on no
        // other replacement (b1 in `into: [b1, b2(pre: [b1])]`). The split
        // target's dependents must hang off those heads — a dependent that
        // needs the whole `b` must study its decomposed chain in order, so
        // it links to the chain's start, not its end.
        let entries: Vec<&str> = into
            .iter()
            .filter(|unit| {
                !unit.pre.iter().any(|raw| {
                    into.iter().any(|other| other.name.trim() == raw.trim())
                })
            })
            .map(|unit| unit.name.trim())
            .collect();
        if entries.is_empty() {
            // Unreachable after the internal cycle check (a DAG of
            // replacements always has a source), kept as a hard guard.
            return Err(
                "split: replacement units form a closed dependency cycle with no entry point \
                 for the target's dependents"
                    .into(),
            );
        }
        // Apply: drop the target, insert the replacements, inherit and
        // redirect edges, then roll back the whole op if a cycle appears.
        let snapshot = self.graph.clone();
        let nodes_before = self.graph.nodes.len();
        let edges_before = self.graph.edges.len();
        let target_pre: Vec<String> = self
            .graph
            .edges
            .iter()
            .filter(|edge| edge.to == target)
            .map(|edge| edge.from.clone())
            .collect();
        let dependents: Vec<String> = self
            .graph
            .edges
            .iter()
            .filter(|edge| edge.from == target)
            .map(|edge| edge.to.clone())
            .collect();
        self.graph.nodes.retain(|node| node.id != target);
        self.graph
            .edges
            .retain(|edge| edge.from != target && edge.to != target);
        for unit in into {
            let name = unit.name.trim();
            self.graph.nodes.push(LearningGraphNode {
                id: name.to_owned(),
                title: name.to_owned(),
                min: unit.min,
                group: None,
                necessity: None,
                is_anchor: None,
            });
            if unit.pre.is_empty() {
                for prereq in &target_pre {
                    self.push_edge(prereq, name);
                }
            } else {
                for raw in &unit.pre {
                    self.push_edge(raw.trim(), name);
                }
            }
        }
        for dependent in &dependents {
            for entry in &entries {
                self.push_edge(entry, dependent);
            }
        }
        if let Err(cycle) = self.reject_if_cycle() {
            self.graph = snapshot;
            return Err(cycle);
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "split".into(),
            summary: format!(
                "已将 '{target}' 拆分为 {} 个单元：{} 个前置由未指定 pre 的新单元继承，\
                 {} 个后继重定向至入口单元 {}",
                into.len(),
                target_pre.len(),
                dependents.len(),
                entries.join("、")
            ),
            node_delta: self.graph.nodes.len() as i64 - nodes_before as i64,
            edge_delta: self.graph.edges.len() as i64 - edges_before as i64,
        })
    }

    fn op_merge(&mut self, into: &str, targets: &[String]) -> Result<OpOutcome, String> {
        let into = into.trim();
        if !self.node_exists(into) {
            return Err(format!("merge: unknown unit '{into}'"));
        }
        if targets.is_empty() {
            return Err("merge: needs at least one target unit".into());
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for raw in targets {
            let target = raw.trim();
            if !self.node_exists(target) {
                return Err(format!("merge: unknown unit '{target}'"));
            }
            if target == into {
                return Err(format!("merge: '{into}' cannot merge into itself"));
            }
            if !seen.insert(target) {
                return Err(format!("merge: target '{target}' appears twice"));
            }
        }
        let snapshot = self.graph.clone();
        let nodes_before = self.graph.nodes.len();
        let edges_before = self.graph.edges.len();
        let is_target = |id: &str| targets.iter().any(|raw| raw.trim() == id);
        let mut folded: Vec<LearningGraphEdge> = Vec::with_capacity(self.graph.edges.len());
        for edge in &self.graph.edges {
            let from_is_target = is_target(&edge.from);
            let to_is_target = is_target(&edge.to);
            if !from_is_target && !to_is_target {
                folded.push(edge.clone());
                continue;
            }
            if from_is_target && to_is_target {
                // target -> target within the merged group collapses into
                // a self loop on `into` and disappears.
                continue;
            }
            let from = if from_is_target { into.to_owned() } else { edge.from.clone() };
            let to = if to_is_target { into.to_owned() } else { edge.to.clone() };
            if from == to {
                continue;
            }
            folded.push(LearningGraphEdge {
                from,
                to,
                reason: edge.reason.clone(),
            });
        }
        // Dedup: a prerequisite shared by several targets would otherwise
        // reappear once per target.
        let mut seen_edges: HashSet<(String, String)> = HashSet::new();
        let mut deduped: Vec<LearningGraphEdge> = Vec::with_capacity(folded.len());
        for edge in folded {
            if seen_edges.insert((edge.from.clone(), edge.to.clone())) {
                deduped.push(edge);
            }
        }
        self.graph.edges = deduped;
        self.graph.nodes.retain(|node| !is_target(&node.id));
        if let Err(cycle) = self.reject_if_cycle() {
            self.graph = snapshot;
            return Err(cycle);
        }
        self.revision += 1;
        let folded_count = targets.len();
        Ok(OpOutcome {
            op: "merge".into(),
            summary: format!(
                "已将 {} 个单元（{}）合并进 '{into}'，所有触及它们的依赖已重定向",
                folded_count,
                targets.iter().map(|t| t.trim()).collect::<Vec<_>>().join("、")
            ),
            node_delta: self.graph.nodes.len() as i64 - nodes_before as i64,
            edge_delta: self.graph.edges.len() as i64 - edges_before as i64,
        })
    }

    fn op_update(
        &mut self,
        target: &str,
        new_name: Option<&str>,
        min: Option<u16>,
    ) -> Result<OpOutcome, String> {
        let target = target.trim();
        if !self.node_exists(target) {
            return Err(format!("update: unknown unit '{target}'"));
        }
        let rename_to = match new_name.map(str::trim) {
            Some(name) if name.is_empty() => return Err("update: new name must not be empty".into()),
            Some(name) if name != target => {
                if self.node_exists(name) {
                    return Err(format!("update: unit '{name}' already exists"));
                }
                Some(name.to_owned())
            }
            _ => None,
        };
        if let Some(min) = min {
            if min > UNIT_MINUTE_CAP {
                return Err(format!(
                    "update: min {min} exceeds the {UNIT_MINUTE_CAP}-minute hard cap"
                ));
            }
        }
        if rename_to.is_none() && min.is_none() {
            return Err(format!(
                "update: nothing to change on '{target}' (name unchanged and no min given)"
            ));
        }
        let mut actions: Vec<String> = Vec::new();
        if let Some(name) = &rename_to {
            for node in &mut self.graph.nodes {
                if node.id == target {
                    node.id = name.clone();
                    node.title = name.clone();
                }
            }
            for edge in &mut self.graph.edges {
                if edge.from == target {
                    edge.from = name.clone();
                }
                if edge.to == target {
                    edge.to = name.clone();
                }
            }
            actions.push(format!("改名 → '{name}'"));
        }
        if let Some(min) = min {
            let id = rename_to.as_deref().unwrap_or(target);
            if let Some(node) = self.graph.nodes.iter_mut().find(|node| node.id == id) {
                node.min = Some(min);
            }
            actions.push(format!("预算 → {min} 分钟"));
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "update".into(),
            summary: format!("已更新 '{target}'（{}）", actions.join("，")),
            node_delta: 0,
            edge_delta: 0,
        })
    }

    fn op_delete(&mut self, target: &str) -> Result<OpOutcome, String> {
        let target = target.trim();
        if !self.node_exists(target) {
            return Err(format!("delete: unknown unit '{target}'"));
        }
        let incoming = self.graph.edges.iter().filter(|edge| edge.to == target).count();
        let outgoing = self.graph.edges.iter().filter(|edge| edge.from == target).count();
        self.graph.nodes.retain(|node| node.id != target);
        self.graph
            .edges
            .retain(|edge| edge.from != target && edge.to != target);
        self.revision += 1;
        Ok(OpOutcome {
            op: "delete".into(),
            summary: format!(
                "已删除 '{target}'：移除 {incoming} 条前置边与 {outgoing} 条后继边；\
                 依赖它的单元已失去该前置，是否成为孤儿由审计判定"
            ),
            node_delta: -1,
            edge_delta: -(incoming as i64 + outgoing as i64),
        })
    }

    /// Add a deduplicated, self-loop-free edge (used by split/merge
    /// redirections; callers validate existence beforehand).
    fn push_edge(&mut self, from: &str, to: &str) {
        if from == to || self.edge_exists(from, to) {
            return;
        }
        self.graph.edges.push(LearningGraphEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            reason: None,
        });
    }

    /// Roll-back helper: `Err` when the graph currently contains a cycle,
    /// naming the back edges `remove_cycle_edges` would drop — the caller
    /// restores its snapshot on this error.
    fn reject_if_cycle(&self) -> Result<(), String> {
        let order: Vec<String> = self.graph.nodes.iter().map(|node| node.id.clone()).collect();
        let pairs: Vec<(String, String)> = self
            .graph
            .edges
            .iter()
            .map(|edge| (edge.from.clone(), edge.to.clone()))
            .collect();
        let dropped = remove_cycle_edges(&order, &pairs).1;
        if dropped.is_empty() {
            return Ok(());
        }
        let list = dropped
            .iter()
            .map(|(f, t)| format!("{f} -> {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!("operation would create a cycle (DAG check drops: {list})"))
    }

    /// Re-run the deterministic audit (with the scope block checklist when a
    /// scope reference exists) so findings always reflect the latest state.
    pub(crate) fn refresh_audit(&mut self) {
        let findings = audit_learning_graph_with_scope(
            &self.graph,
            self.scope.as_ref().map(|scope| scope.blocks.as_slice()),
        );
        self.graph.audit.findings = findings;
    }

    // ── Lookups the agent queries with ────────────────────────────────────

    /// 全图紧凑清单：每行一个单元 `标题 [分钟] <- 前置1; 前置2`（无前置
    /// 省略箭头段）。供 agent 一次性通读全图——续建恢复认知、上交前的语
    /// 义自查都用它；行式文本比 JSON 紧凑一个量级（600 节点约 10k token）。
    pub(crate) fn compact_dump(&self) -> String {
        let mut lines = Vec::with_capacity(self.graph.nodes.len());
        for node in &self.graph.nodes {
            let mut line = match node.min {
                Some(min) => format!("{} [{min}]", node.title),
                None => node.title.clone(),
            };
            let pres: Vec<&str> = self
                .graph
                .edges
                .iter()
                .filter(|edge| edge.to == node.title)
                .map(|edge| edge.from.as_str())
                .collect();
            if !pres.is_empty() {
                line.push_str(" <- ");
                line.push_str(&pres.join("; "));
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    /// One-line overview: sizes, workload, sub-domain distribution,
    /// entry/terminal units and the audit summary.
    pub(crate) fn inspect(&self) -> InspectView {
        let mut total_minutes = 0u64;
        for node in &self.graph.nodes {
            if let Some(min) = node.min {
                total_minutes += min as u64;
            }
        }
        let (entry_total, entry_nodes) = capped(self.sources(), 12);
        let (terminal_total, terminal_nodes) = capped(self.sinks(), 12);
        let block_units = self.scope.as_ref().map(|scope| {
            scope
                .blocks
                .iter()
                .map(|block| BlockUnitCount {
                    name: block.clone(),
                    units: self
                        .graph
                        .nodes
                        .iter()
                        .filter(|node| common_substring_len(&node.title, block) >= BLOCK_MIN_SHARED)
                        .count(),
                })
                .collect()
        });
        let scope_coverage = self.scope.as_ref().map(|scope| ScopeCoverage {
            blocks_covered: scope
                .blocks
                .iter()
                .filter(|block| {
                    self.graph
                        .nodes
                        .iter()
                        .any(|node| common_substring_len(&node.title, block) >= BLOCK_MIN_SHARED)
                })
                .count(),
            blocks_total: scope.blocks.len(),
        });
        InspectView {
            topic: self.topic.clone(),
            revision: self.revision,
            node_count: self.graph.nodes.len(),
            edge_count: self.graph.edges.len(),
            total_minutes,
            entry_total,
            entry_nodes,
            terminal_total,
            terminal_nodes,
            block_units,
            scope_coverage,
            audit_summary: summarize_findings(&self.graph.audit.findings),
        }
    }

    /// Name-substring and/or overlap-word filtered unit list. The overlap
    /// filter uses the SAME title-overlap bar as the audit, so what the
    /// agent sees as "covered" matches what the gate will grade.
    pub(crate) fn query(&self, filter: &NodeQuery) -> NodeListView {
        let limit = filter.limit.clamp(1, 200);
        let mut matched = 0usize;
        let mut nodes = Vec::new();
        for node in &self.graph.nodes {
            if let Some(matcher) = &filter.matcher {
                let matcher = matcher.trim();
                if !matcher.is_empty() && !node.title.contains(matcher) {
                    continue;
                }
            }
            if let Some(keyword) = &filter.keyword {
                let keyword = keyword.trim();
                if !keyword.is_empty()
                    && common_substring_len(&node.title, keyword) < BLOCK_MIN_SHARED
                {
                    continue;
                }
            }
            matched += 1;
            if nodes.len() >= limit {
                continue;
            }
            nodes.push(NodeInfo {
                id: node.id.clone(),
                min: node.min,
                pre: self
                    .graph
                    .edges
                    .iter()
                    .filter(|edge| edge.to == node.id)
                    .map(|edge| edge.from.clone())
                    .collect(),
                dependents: self
                    .graph
                    .edges
                    .iter()
                    .filter(|edge| edge.from == node.id)
                    .map(|edge| edge.to.clone())
                    .collect(),
            });
        }
        NodeListView { matched, nodes }
    }

    /// The ancestor/descendant closure of the given units (both directions
    /// combined with `Both`). `depth: None` walks the FULL closure; a depth
    /// of 0 returns the roots only, 1 the roots plus their direct
    /// neighbours, and so on.
    pub(crate) fn subgraph(
        &self,
        roots: &[String],
        direction: SubgraphDirection,
        depth: Option<usize>,
    ) -> SubgraphView {
        let roots: Vec<String> = roots
            .iter()
            .map(|root| root.trim().to_owned())
            .filter(|root| self.node_exists(root))
            .collect();
        let mut included: HashSet<String> = roots.iter().cloned().collect();
        let mut frontier: Vec<String> = roots.clone();
        let mut hops = 0usize;
        while !frontier.is_empty() && depth.is_none_or(|limit| hops < limit) {
            let mut next: Vec<String> = Vec::new();
            for id in &frontier {
                for edge in &self.graph.edges {
                    let candidate = match direction {
                        SubgraphDirection::Ancestors if edge.to == *id => Some(edge.from.clone()),
                        SubgraphDirection::Descendants if edge.from == *id => Some(edge.to.clone()),
                        SubgraphDirection::Both if edge.to == *id => Some(edge.from.clone()),
                        SubgraphDirection::Both if edge.from == *id => Some(edge.to.clone()),
                        _ => None,
                    };
                    if let Some(candidate) = candidate {
                        if included.insert(candidate.clone()) {
                            next.push(candidate);
                        }
                    }
                }
            }
            frontier = next;
            hops += 1;
        }
        SubgraphView {
            roots,
            direction: direction.label().to_owned(),
            depth,
            nodes: self
                .graph
                .nodes
                .iter()
                .filter(|node| included.contains(&node.id))
                .map(|node| SubgraphNode {
                    id: node.id.clone(),
                    min: node.min,
                })
                .collect(),
            edges: self
                .graph
                .edges
                .iter()
                .filter(|edge| included.contains(&edge.from) && included.contains(&edge.to))
                .map(|edge| SubgraphEdge {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                })
                .collect(),
        }
    }

    /// Full findings text (same renderer the legacy gate uses) prefixed with
    /// the scope block checklist and its live coverage state — the repair
    /// loop's primary input.
    pub(crate) fn audit_report(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if let Some(scope) = &self.scope {
            lines.push("==== Scope reference ====".into());
            if !scope.scope.is_empty() {
                lines.push(format!("范围界定：{}", scope.scope));
            }
            if !scope.blocks.is_empty() {
                let missing: Vec<&String> = scope
                    .blocks
                    .iter()
                    .filter(|block| {
                        !self.graph.nodes.iter().any(|node| {
                            common_substring_len(&node.title, block) >= BLOCK_MIN_SHARED
                        })
                    })
                    .collect();
                lines.push(format!(
                    "大块概念（{} 个，{} 个已覆盖）",
                    scope.blocks.len(),
                    scope.blocks.len() - missing.len()
                ));
                if !missing.is_empty() {
                    lines.push(format!(
                        "  未覆盖（需按此落实为单元，改写为动作句）：{}",
                        missing
                            .iter()
                            .map(|block| block.as_str())
                            .collect::<Vec<_>>()
                            .join("；")
                    ));
                }
            }
            lines.push(String::new());
        }
        lines.push("==== Audit report ====".into());
        lines.push(format_audit_report(&self.graph));
        lines.join("\n")
    }

    /// The scope reference as a standalone text — the generation loop's
    /// coverage checklist (`None` when no scope analysis ran).
    pub(crate) fn scope_reference(&self) -> Option<String> {
        let scope = self.scope.as_ref()?;
        let mut parts: Vec<String> = Vec::new();
        if !scope.goal.is_empty() {
            parts.push(format!("最终目标：{}", scope.goal));
        }
        if !scope.baseline.is_empty() {
            parts.push(format!("用户起点：{}", scope.baseline));
        }
        if !scope.scope.is_empty() {
            parts.push(format!("范围界定：{}", scope.scope));
        }
        if !scope.blocks.is_empty() {
            parts.push(format!(
                "大块概念（严格完备覆盖清单，每个需落实为一个或多个单元，可改写为动作句）：{}",
                scope.blocks.join("；")
            ));
        }
        Some(parts.join("\n"))
    }

    /// Lightweight creation view handed back by `lg_start`.
    pub(crate) fn view(&self, draft_id: &str) -> DraftView {
        DraftView {
            draft_id: draft_id.to_owned(),
            topic: self.topic.clone(),
            revision: self.revision,
            node_count: self.graph.nodes.len(),
            edge_count: self.graph.edges.len(),
            scope: self.scope.as_ref().map(|scope| ScopeView {
                goal: scope.goal.clone(),
                baseline: scope.baseline.clone(),
                scope: scope.scope.clone(),
                blocks: scope.blocks.clone(),
            }),
        }
    }

    fn node_exists(&self, id: &str) -> bool {
        self.graph.nodes.iter().any(|node| node.id == id)
    }

    fn edge_exists(&self, from: &str, to: &str) -> bool {
        self.graph
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to)
    }

    /// Entry units: no incoming prerequisite edge.
    fn sources(&self) -> Vec<String> {
        let incoming: HashSet<&str> = self.graph.edges.iter().map(|edge| edge.to.as_str()).collect();
        self.graph
            .nodes
            .iter()
            .filter(|node| !incoming.contains(node.id.as_str()))
            .map(|node| node.id.clone())
            .collect()
    }

    /// Terminal units: no outgoing edge.
    fn sinks(&self) -> Vec<String> {
        let outgoing: HashSet<&str> = self
            .graph
            .edges
            .iter()
            .map(|edge| edge.from.as_str())
            .collect();
        self.graph
            .nodes
            .iter()
            .filter(|node| !outgoing.contains(node.id.as_str()))
            .map(|node| node.id.clone())
            .collect()
    }

    /// Nearest existing unit name for an unknown reference (same near-miss
    /// matcher the normalizer uses), so rejection messages tell the model
    /// what to fix.
    fn closest(&self, reference: &str) -> Option<String> {
        let names: HashSet<String> = self.graph.nodes.iter().map(|node| node.id.clone()).collect();
        fuzzy_resolve_reference(reference, &names, &HashSet::new())
    }
}

/// Cap a list for compact views, returning (total, first `cap` entries).
fn capped(ids: Vec<String>, cap: usize) -> (usize, Vec<String>) {
    let total = ids.len();
    let shown = ids.into_iter().take(cap).collect();
    (total, shown)
}

// ── Views returned to the agent tools ──────────────────────────────────────

/// Creation view of a draft (returned by `lg_start`).
#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    pub draft_id: String,
    pub topic: String,
    pub revision: u64,
    pub node_count: usize,
    pub edge_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeView>,
}

/// The scope reference in a serializable shape (the internal
/// [`ScopeAnalysis`] stays crate-private). `goal`/`baseline` are omitted
/// when empty (scope-free or old-shape replies).
#[derive(Debug, Clone, Serialize)]
pub struct ScopeView {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub baseline: String,
    pub scope: String,
    pub blocks: Vec<String>,
}

/// Result of a `lg_patch` batch.
#[derive(Debug, Clone, Serialize)]
pub struct PatchReport {
    /// Draft revision after the batch (one per accepted op).
    pub revision: u64,
    pub accepted: Vec<OpOutcome>,
    pub rejected: Vec<RejectedOp>,
    /// Fresh gate snapshot: every finding grouped by (severity, kind) with
    /// the evidence node ids of danger findings.
    pub audit_summary: Vec<FindingSummary>,
}

/// One accepted op with a human-readable effect summary.
#[derive(Debug, Clone, Serialize)]
pub struct OpOutcome {
    pub op: String,
    pub summary: String,
    pub node_delta: i64,
    pub edge_delta: i64,
}

/// One rejected op with the exact reason (and, for unknown references, the
/// closest existing unit name).
#[derive(Debug, Clone, Serialize)]
pub struct RejectedOp {
    pub op: GraphOp,
    pub reason: String,
}

/// Audit findings grouped by severity + kind, with evidence node ids.
#[derive(Debug, Clone, Serialize)]
pub struct FindingSummary {
    pub severity: String,
    pub kind: String,
    pub count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub node_ids: Vec<String>,
}

fn summarize_findings(findings: &[AuditFinding]) -> Vec<FindingSummary> {
    let mut order: Vec<(&str, &str)> = Vec::new();
    let mut counts: HashMap<(&str, &str), usize> = HashMap::new();
    let mut ids: HashMap<(&str, &str), Vec<String>> = HashMap::new();
    for finding in findings {
        let key = (finding.severity.as_str(), finding.kind.as_str());
        if !counts.contains_key(&key) {
            order.push(key);
        }
        *counts.entry(key).or_insert(0) += 1;
        ids.entry(key).or_default().extend(finding.node_ids.iter().cloned());
    }
    order
        .into_iter()
        .map(|(severity, kind)| FindingSummary {
            severity: severity.to_owned(),
            kind: kind.to_owned(),
            count: counts[&(severity, kind)],
            node_ids: ids.remove(&(severity, kind)).unwrap_or_default(),
        })
        .collect()
}

/// `lg_inspect` overview.
#[derive(Debug, Clone, Serialize)]
pub struct InspectView {
    pub topic: String,
    pub revision: u64,
    pub node_count: usize,
    pub edge_count: usize,
    /// Sum of every stated minute budget (0 when none are set).
    pub total_minutes: u64,
    pub entry_total: usize,
    /// First 12 entry units (see `entry_total` for the full count).
    pub entry_nodes: Vec<String>,
    pub terminal_total: usize,
    /// First 12 terminal units (see `terminal_total` for the full count).
    pub terminal_nodes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_units: Option<Vec<BlockUnitCount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_coverage: Option<ScopeCoverage>,
    pub audit_summary: Vec<FindingSummary>,
}

/// How many units currently overlap one scope block name (same overlap bar
/// as the audit).
#[derive(Debug, Clone, Serialize)]
pub struct BlockUnitCount {
    pub name: String,
    pub units: usize,
}

/// Scope checklist coverage: how many large-block concepts already have
/// matching units.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeCoverage {
    pub blocks_covered: usize,
    pub blocks_total: usize,
}

/// Filter for `lg_query`.
#[derive(Debug, Clone, Default)]
pub struct NodeQuery {
    /// Substring matched against unit titles (empty/`None` = no filter).
    pub matcher: Option<String>,
    /// Overlap word (e.g. a scope block name); only units sharing >= 2
    /// consecutive chars with it are kept (the audit's own bar).
    pub keyword: Option<String>,
    /// Default 50, clamped to 200 — protects the context window.
    pub limit: usize,
}

/// `lg_query` result.
#[derive(Debug, Clone, Serialize)]
pub struct NodeListView {
    /// How many units matched the filter (may exceed `nodes.len()` when the
    /// limit cut the list).
    pub matched: usize,
    pub nodes: Vec<NodeInfo>,
}

/// One unit with its direct prerequisites and dependents — everything the
/// agent needs before editing it.
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub min: Option<u16>,
    pub pre: Vec<String>,
    pub dependents: Vec<String>,
}

/// Which neighbourhood a `lg_subgraph` call walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubgraphDirection {
    /// Everything the roots transitively depend on.
    Ancestors,
    /// Everything that transitively depends on the roots.
    Descendants,
    /// Both directions.
    Both,
}

impl SubgraphDirection {
    fn label(self) -> &'static str {
        match self {
            SubgraphDirection::Ancestors => "ancestors",
            SubgraphDirection::Descendants => "descendants",
            SubgraphDirection::Both => "both",
        }
    }
}

/// `lg_subgraph` result: the local DAG around the roots.
#[derive(Debug, Clone, Serialize)]
pub struct SubgraphView {
    pub roots: Vec<String>,
    pub direction: String,
    pub depth: Option<usize>,
    pub nodes: Vec<SubgraphNode>,
    pub edges: Vec<SubgraphEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubgraphNode {
    pub id: String,
    pub min: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubgraphEdge {
    pub from: String,
    pub to: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> DraftGraph {
        DraftGraph::new("数学".into(), None)
    }

    fn op_add(name: &str, pre: &[&str], min: u16) -> GraphOp {
        GraphOp::Add {
            name: name.into(),
            pre: pre.iter().map(|s| (*s).to_owned()).collect(),
            min: Some(min),
        }
    }

    /// Build a chain a -> b -> c plus a converging unit d(b, c).
    fn seeded() -> DraftGraph {
        let mut graph = draft();
        let report = graph.apply_ops(vec![
            op_add("a", &[], 10),
            op_add("b", &["a"], 15),
            op_add("c", &["b"], 20),
            op_add("d", &["b", "c"], 25),
        ]);
        assert!(report.accepted.len() == 4 && report.rejected.is_empty());
        graph
    }

    #[test]
    fn compact_dump_lists_units_with_minutes_and_pres() {
        let graph = seeded();
        let dump = graph.compact_dump();
        assert!(dump.contains("a [10]"), "{dump}");
        assert!(dump.contains("b [15] <- a"), "{dump}");
        assert!(dump.contains("d [25] <- b; c"), "{dump}");
        assert_eq!(dump.lines().count(), 4, "one line per unit");
    }

    #[test]
    fn graph_op_deserializes_tagged_shapes_tolerantly() {
        let ops: Vec<GraphOp> = serde_json::from_str(
            r#"[
                {"op": "add", "name": "A", "pre": "B", "min": "15"},
                {"op": "link", "from": "A", "to": "B"},
                {"op": "unlink", "from": "A", "to": "B"},
                {"op": "set_pre", "target": "B", "pre": "A"},
                {"op": "split", "target": "A", "into": [{"name": "A1", "min": 5}]},
                {"op": "merge", "into": "A", "targets": ["B"]},
                {"op": "update", "target": "A", "name": "A2"},
                {"op": "delete", "target": "A2"}
            ]"#,
        )
        .unwrap();
        assert_eq!(ops.len(), 8);
        match &ops[0] {
            GraphOp::Add { pre, min, .. } => {
                assert_eq!(pre, &vec!["B".to_owned()]);
                assert_eq!(*min, Some(15));
            }
            _ => panic!("expected add"),
        }
        assert!(matches!(&ops[2], GraphOp::Unlink { .. }));
        assert!(matches!(&ops[3], GraphOp::SetPre { .. }));
        // Missing optional fields default.
        let ops: Vec<GraphOp> =
            serde_json::from_str(r#"[{"op": "add", "name": "A"}, {"op": "update", "target": "A"}]"#)
                .unwrap();
        match &ops[0] {
            GraphOp::Add { pre, min, .. } => {
                assert!(pre.is_empty());
                assert_eq!(*min, None);
            }
            _ => panic!("expected add"),
        }
        let ops: Vec<GraphOp> =
            serde_json::from_str(r#"[{"op": "set_pre", "target": "A"}]"#).unwrap();
        assert!(
            matches!(&ops[0], GraphOp::SetPre { pre, .. } if pre.is_empty()),
            "set_pre tolerates a missing pre list"
        );
    }

    #[test]
    fn add_builds_nodes_and_edges_in_batch_order() {
        let mut graph = draft();
        let report = graph.apply_ops(vec![
            op_add("a", &[], 5),
            op_add("b", &["a"], 10),
            op_add("c", &["a", "b"], 15),
        ]);
        assert!(report.rejected.is_empty());
        assert_eq!(report.accepted.len(), 3);
        assert_eq!(graph.graph.nodes.len(), 3);
        assert_eq!(graph.graph.edges.len(), 3);
        assert_eq!(graph.revision, 3);
        // Drop statistics accumulate like every other path.
        assert_eq!(graph.graph.audit.raw_ref_count, 3);
        assert_eq!(graph.graph.audit.ref_drop_rate, 0.0);
    }

    #[test]
    fn add_rejects_empty_duplicate_unknown_self_loop_and_over_budget() {
        let mut graph = seeded();
        let report = graph.apply_ops(vec![
            GraphOp::Add { name: "  ".into(), pre: vec![], min: None },
            GraphOp::Add { name: "a".into(), pre: vec![], min: None },
            GraphOp::Add { name: "e".into(), pre: vec!["ghost".into()], min: None },
            GraphOp::Add { name: "f".into(), pre: vec!["f".into()], min: None },
            GraphOp::Add { name: "g".into(), pre: vec![], min: Some(70) },
        ]);
        assert!(report.accepted.is_empty());
        assert_eq!(report.rejected.len(), 5);
        assert!(report.rejected[0].reason.contains("must not be empty"));
        assert!(report.rejected[1].reason.contains("already exists"));
        assert!(report.rejected[2].reason.contains("unknown prerequisite"));
        assert!(report.rejected[3].reason.contains("depend on itself"));
        assert!(report.rejected[4].reason.contains("hard cap"));
        assert_eq!(graph.graph.nodes.len(), 4, "nothing was applied");
    }

    #[test]
    fn add_unknown_reference_reports_the_closest_unit() {
        let mut graph = seeded();
        let report = graph.apply_ops(vec![op_add("e", &["a的"], 10)]);
        assert_eq!(report.rejected.len(), 1);
        assert!(
            report.rejected[0].reason.contains("closest existing unit: 'a'"),
            "{}",
            report.rejected[0].reason
        );
    }

    #[test]
    fn link_and_reverse_validate_and_roll_back_cycles() {
        let mut graph = seeded();
        // link: unknown unit, self loop, duplicate edge.
        let report = graph.apply_ops(vec![
            GraphOp::Link { from: "ghost".into(), to: "a".into() },
            GraphOp::Link { from: "a".into(), to: "a".into() },
            GraphOp::Link { from: "a".into(), to: "b".into() },
        ]);
        assert!(report.accepted.is_empty());
        assert_eq!(report.rejected.len(), 3);

        // link closing a cycle is rolled back whole.
        let report = graph.apply_ops(vec![GraphOp::Link {
            from: "d".into(),
            to: "a".into(),
        }]);
        assert_eq!(report.accepted.len(), 0);
        assert!(report.rejected[0].reason.contains("cycle"));
        assert_eq!(graph.graph.edges.len(), 4, "rolled back");

        // reverse is a legal way to flip a leaf into a root: b -> c
        // becomes c -> b (both are prerequisites of d either way).
        let report = graph.apply_ops(vec![GraphOp::Reverse {
            from: "b".into(),
            to: "c".into(),
        }]);
        assert_eq!(report.rejected.len(), 0, "{:?}", report.rejected);
        assert!(graph.edge_exists("c", "b"));

        // reversing an edge that does not exist is rejected.
        let report = graph.apply_ops(vec![GraphOp::Reverse {
            from: "d".into(),
            to: "b".into(),
        }]);
        assert_eq!(report.accepted.len(), 0);
        assert!(report.rejected[0].reason.contains("no edge"));
    }

    #[test]
    fn unlink_removes_one_edge_and_keeps_nodes() {
        let mut graph = seeded();
        // Unknown units and a missing edge are rejected.
        let report = graph.apply_ops(vec![
            GraphOp::Unlink { from: "ghost".into(), to: "a".into() },
            GraphOp::Unlink { from: "a".into(), to: "d".into() },
        ]);
        assert_eq!(report.accepted.len(), 0);
        assert_eq!(report.rejected.len(), 2);
        assert!(report.rejected[0].reason.contains("unknown unit"));
        assert!(report.rejected[1].reason.contains("no edge"));

        // Only the one edge goes; every node and the other edges stay.
        let report = graph.apply_ops(vec![GraphOp::Unlink {
            from: "b".into(),
            to: "c".into(),
        }]);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        assert_eq!(graph.graph.nodes.len(), 4, "nodes are untouched");
        assert!(!graph.edge_exists("b", "c"));
        assert!(graph.edge_exists("a", "b"));
        assert!(graph.edge_exists("b", "d"));
        assert!(graph.edge_exists("c", "d"));
        // Removing an edge can never close a cycle.
        assert!(graph.reject_if_cycle().is_ok());
        // A legit unlink drops no reference, so the audit never flags
        // orphaned_units here — c just became an entry unit the agent must
        // reconnect when the audit (or its own review) asks for it.
        let findings = graph
            .graph
            .audit
            .findings
            .iter()
            .filter(|finding| finding.kind == "orphaned_units")
            .count();
        assert_eq!(findings, 0, "findings: {:?}", graph.graph.audit.findings);
        let inspect = graph.inspect();
        assert!(
            inspect.entry_nodes.contains(&"c".to_owned()),
            "c without prerequisites is now an entry unit: {:?}",
            inspect.entry_nodes
        );
    }

    #[test]
    fn set_pre_replaces_the_full_prerequisite_set() {
        let mut graph = seeded();
        // Unknown target, unknown/self/empty references are rejected.
        let report = graph.apply_ops(vec![
            GraphOp::SetPre { target: "ghost".into(), pre: vec!["a".into()] },
            GraphOp::SetPre { target: "c".into(), pre: vec!["c的".into()] },
            GraphOp::SetPre { target: "c".into(), pre: vec!["c".into()] },
            GraphOp::SetPre { target: "c".into(), pre: vec!["  ".into()] },
        ]);
        assert_eq!(report.accepted.len(), 0);
        assert_eq!(report.rejected.len(), 4);
        assert!(report.rejected[0].reason.contains("unknown unit"));
        assert!(
            report.rejected[1].reason.contains("closest existing unit"),
            "unknown prerequisite gets a near-miss hint: {}",
            report.rejected[1].reason
        );
        assert!(report.rejected[2].reason.contains("depend on itself"));
        assert!(report.rejected[3].reason.contains("empty prerequisite"));
        assert_eq!(graph.graph.edges.len(), 4, "nothing was applied");

        // c's prerequisites were [b]; the full set is now [a]: the old edge
        // b -> c disappears in the same op, and a duplicated reference is
        // deduped.
        let report = graph.apply_ops(vec![GraphOp::SetPre {
            target: "c".into(),
            pre: vec!["a".into(), "a".into()],
        }]);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        assert!(!graph.edge_exists("b", "c"));
        assert!(graph.edge_exists("a", "c"));
        assert_eq!(graph.graph.edges.len(), 4, "a -> c replaced b -> c once");
        assert!(report.accepted[0].summary.contains("整体替换"));

        // A set that would close a cycle rolls back whole: a -> b -> c -> d,
        // so pointing a at d closes the loop a -> b -> c -> d -> a.
        let report = graph.apply_ops(vec![GraphOp::SetPre {
            target: "a".into(),
            pre: vec!["d".into()],
        }]);
        assert_eq!(report.accepted.len(), 0);
        assert!(report.rejected[0].reason.contains("cycle"));
        assert!(graph.edge_exists("a", "b"), "rolled back");

        // Clearing the set is legal — the audit grades the empty pre.
        let report = graph.apply_ops(vec![GraphOp::SetPre {
            target: "d".into(),
            pre: vec![],
        }]);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        assert!(graph.graph.edges.iter().all(|edge| edge.to != "d"));
    }

    #[test]
    fn split_replaces_target_inherits_prerequisites_and_redirects_dependents() {
        let mut graph = seeded();
        let report = graph.apply_ops(vec![GraphOp::Split {
            target: "b".into(),
            into: vec![
                SplitUnit { name: "b1".into(), pre: vec![], min: Some(10) },
                SplitUnit { name: "b2".into(), pre: vec!["b1".into()], min: Some(10) },
            ],
        }]);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        assert_eq!(report.accepted.len(), 1);
        assert!(!graph.node_exists("b"));
        assert!(graph.node_exists("b1") && graph.node_exists("b2"));
        // b1 inherits b's prerequisite a; b2 explicitly depends on b1.
        assert!(graph.edge_exists("a", "b1"));
        assert!(graph.edge_exists("b1", "b2"));
        // b's dependents (c, d) hang off the ENTRY replacement b1 (the
        // chain head), never off the chain tail b2.
        assert!(graph.edge_exists("b1", "c") && graph.edge_exists("b1", "d"));
        assert!(!graph.edge_exists("b2", "c") && !graph.edge_exists("b2", "d"));
        // No cycle survived.
        assert!(graph.reject_if_cycle().is_ok());
    }

    #[test]
    fn split_rejects_bad_replacement_lists() {
        let mut graph = seeded();
        let report = graph.apply_ops(vec![
            GraphOp::Split { target: "ghost".into(), into: vec![SplitUnit { name: "x".into(), pre: vec![], min: None }] },
            GraphOp::Split { target: "a".into(), into: vec![] },
            GraphOp::Split { target: "a".into(), into: vec![SplitUnit { name: "a".into(), pre: vec![], min: None }] },
            GraphOp::Split { target: "a".into(), into: vec![SplitUnit { name: "b".into(), pre: vec![], min: None }] },
            GraphOp::Split { target: "a".into(), into: vec![SplitUnit { name: "x".into(), pre: vec!["a".into()], min: None }] },
            GraphOp::Split { target: "a".into(), into: vec![SplitUnit { name: "x".into(), pre: vec![], min: Some(70) }] },
            GraphOp::Split { target: "a".into(), into: vec![SplitUnit { name: "x".into(), pre: vec!["ghost".into()], min: None }] },
            // closed replacement cycle: no entry unit for dependents.
            GraphOp::Split { target: "a".into(), into: vec![
                SplitUnit { name: "x".into(), pre: vec!["y".into()], min: None },
                SplitUnit { name: "y".into(), pre: vec!["x".into()], min: None },
            ] },
        ]);
        assert!(report.accepted.is_empty());
        assert_eq!(report.rejected.len(), 8);
        assert!(report.rejected[1].reason.contains("at least one replacement"));
        assert!(report.rejected[2].reason.contains("must not reuse the target name"));
        assert!(report.rejected[3].reason.contains("already exists"));
        assert!(report.rejected[4].reason.contains("must not depend on the split target"));
        assert!(report.rejected[5].reason.contains("hard cap"));
        assert!(report.rejected[6].reason.contains("unknown prerequisite"));
        assert!(report.rejected[7].reason.contains("dependency cycle"));
        assert_eq!(graph.graph.nodes.len(), 4, "nothing was applied");
    }

    #[test]
    fn merge_folds_targets_and_dedups_shared_prerequisites() {
        let mut graph = draft();
        graph.apply_ops(vec![
            op_add("x", &[], 5),
            op_add("p", &[], 5),
            op_add("a", &["p", "x"], 10),
            op_add("b", &["p"], 10),
        ]);
        let report = graph.apply_ops(vec![GraphOp::Merge {
            into: "x".into(),
            targets: vec!["a".into(), "b".into()],
        }]);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        assert!(!graph.node_exists("a") && !graph.node_exists("b"));
        // p -> a, p -> b both redirect to p -> x (deduped to a single edge).
        assert_eq!(graph.graph.edges.iter().filter(|e| e.from == "p").count(), 1);
        assert!(graph.edge_exists("p", "x"));
    }

    #[test]
    fn merge_rejects_unknown_self_and_duplicate_targets() {
        let mut graph = seeded();
        let report = graph.apply_ops(vec![
            GraphOp::Merge { into: "ghost".into(), targets: vec!["a".into()] },
            GraphOp::Merge { into: "a".into(), targets: vec![] },
            GraphOp::Merge { into: "a".into(), targets: vec!["a".into()] },
            GraphOp::Merge { into: "a".into(), targets: vec!["ghost".into()] },
            GraphOp::Merge { into: "a".into(), targets: vec!["b".into(), "b".into()] },
        ]);
        assert!(report.accepted.is_empty());
        assert_eq!(report.rejected.len(), 5);
        assert!(report.rejected[1].reason.contains("at least one target"));
        assert!(report.rejected[2].reason.contains("merge into itself"));
        assert!(report.rejected[4].reason.contains("appears twice"));
    }

    #[test]
    fn merge_that_would_close_a_cycle_is_rolled_back() {
        // x -> b, b -> a. Merging a into x redirects b -> a onto b -> x,
        // closing the cycle b -> x -> b.
        let mut graph = draft();
        graph.apply_ops(vec![
            op_add("x", &[], 5),
            op_add("b", &["x"], 5),
            op_add("a", &["b"], 5),
        ]);
        let report = graph.apply_ops(vec![GraphOp::Merge {
            into: "x".into(),
            targets: vec!["a".into()],
        }]);
        assert_eq!(report.accepted.len(), 0, "{:?}", report.accepted);
        assert!(report.rejected[0].reason.contains("cycle"));
        assert!(graph.node_exists("a"), "rolled back");
    }

    #[test]
    fn update_renames_rewrites_edges_and_checks_budget() {
        let mut graph = seeded();
        let report = graph.apply_ops(vec![
            GraphOp::Update { target: "b".into(), name: Some("b'".into()), min: Some(10) },
            GraphOp::Update { target: "ghost".into(), name: Some("x".into()), min: None },
            GraphOp::Update { target: "a".into(), name: Some("c".into()), min: None },
            GraphOp::Update { target: "a".into(), name: None, min: Some(70) },
        ]);
        assert_eq!(report.accepted.len(), 1);
        assert_eq!(report.rejected.len(), 3);
        assert!(!graph.node_exists("b"));
        assert!(graph.node_exists("b'"));
        // c's prerequisite entry was rewritten.
        assert!(graph.edge_exists("b'", "c"));
        assert!(graph.edge_exists("a", "b'"));
        let renamed = graph.graph.nodes.iter().find(|n| n.id == "b'").unwrap();
        assert_eq!(renamed.min, Some(10));
        assert!(report.rejected[1].reason.contains("already exists"));
        assert!(report.rejected[2].reason.contains("hard cap"));
    }

    #[test]
    fn delete_removes_touching_edges_and_leaves_orphans_to_the_audit() {
        let mut graph = seeded();
        let report = graph.apply_ops(vec![
            GraphOp::Delete { target: "b".into() },
            GraphOp::Delete { target: "ghost".into() },
        ]);
        assert_eq!(report.accepted.len(), 1);
        assert_eq!(report.rejected.len(), 1);
        assert!(!graph.node_exists("b"));
        // Incoming (a -> b) and outgoing (b -> c, b -> d) edges all gone.
        assert!(!graph.edge_exists("a", "b"));
        assert!(!graph.edge_exists("b", "c"));
        // c and d lost their only prerequisite: the audit must flag the
        // split graph as disconnected (the structural orphan signal).
        let disconnected = graph
            .graph
            .audit
            .findings
            .iter()
            .find(|finding| finding.kind == "disconnected_components");
        assert!(disconnected.is_some(), "findings: {:?}", graph.graph.audit.findings);
        assert_eq!(disconnected.unwrap().severity, "danger");
    }

    #[test]
    fn inspect_reports_sources_sinks_and_audit_summary() {
        let graph = seeded();
        let view = graph.inspect();
        assert_eq!(view.node_count, 4);
        assert_eq!(view.edge_count, 4);
        assert_eq!(view.total_minutes, 70);
        assert_eq!(view.entry_nodes, vec!["a".to_owned()]);
        assert_eq!(view.terminal_nodes, vec!["d".to_owned()]);
        // 30-unit coverage gate is far away on a 4-unit test graph.
        let coverage = view
            .audit_summary
            .iter()
            .find(|summary| summary.kind == "coverage");
        assert!(coverage.is_some());
        assert_eq!(coverage.unwrap().severity, "danger");
    }

    #[test]
    fn query_filters_by_substring_and_overlap_keyword() {
        let mut graph = draft();
        graph.scope = Some(ScopeAnalysis {
            scope: String::new(),
            blocks: vec!["一元二次方程".into()],
            ..Default::default()
        });
        graph.apply_ops(vec![
            op_add("用配方法解一元二次方程", &[], 15),
            op_add("用求根公式解一元二次方程", &["用配方法解一元二次方程"], 15),
            op_add("画函数图像", &[], 10),
        ]);
        let all = graph.query(&NodeQuery { matcher: None, keyword: None, limit: 10 });
        assert_eq!(all.matched, 3);
        let filtered = graph.query(&NodeQuery {
            matcher: Some("配方法".into()),
            keyword: None,
            limit: 10,
        });
        assert_eq!(filtered.matched, 1);
        assert_eq!(filtered.nodes[0].id, "用配方法解一元二次方程");
        let by_keyword = graph.query(&NodeQuery {
            matcher: None,
            keyword: Some("一元二次方程".into()),
            limit: 10,
        });
        assert_eq!(by_keyword.matched, 2);
        // Limit truncates the list but reports the true match count.
        let limited = graph.query(&NodeQuery { matcher: None, keyword: None, limit: 2 });
        assert_eq!(limited.matched, 3);
        assert_eq!(limited.nodes.len(), 2);
        let node = &limited.nodes[0];
        assert_eq!(node.dependents, vec!["用求根公式解一元二次方程".to_owned()]);
    }

    #[test]
    fn subgraph_walks_ancestors_descendants_and_both() {
        let graph = seeded();
        let roots = vec!["c".to_owned()];
        let ancestors = graph.subgraph(&roots, SubgraphDirection::Ancestors, None);
        assert_eq!(ancestors.nodes.len(), 3, "a, b, c");
        assert_eq!(ancestors.edges.len(), 2);
        let descendants = graph.subgraph(&roots, SubgraphDirection::Descendants, None);
        assert_eq!(descendants.nodes.len(), 2, "c, d");
        let both = graph.subgraph(&roots, SubgraphDirection::Both, None);
        assert_eq!(both.nodes.len(), 4, "whole graph");
        // Depth 1 from c: only b (ancestor) — a stays out.
        let shallow = graph.subgraph(&roots, SubgraphDirection::Ancestors, Some(1));
        assert_eq!(shallow.nodes.len(), 2);
        assert!(shallow.nodes.iter().all(|n| n.id != "a"));
        // Unknown roots are dropped silently.
        let missing = graph.subgraph(
            &vec!["ghost".to_owned(), "c".to_owned()],
            SubgraphDirection::Descendants,
            None,
        );
        assert_eq!(missing.roots, vec!["c".to_owned()]);
    }

    #[test]
    fn audit_report_and_scope_reference_render_the_checklists() {
        let mut graph = draft();
        graph.scope = Some(ScopeAnalysis {
            goal: "能独立解一元二次方程并解释判别式的含义".into(),
            baseline: "对代数一无所知，只会四则运算".into(),
            scope: "零基础到一元二次方程".into(),
            blocks: vec!["配方法".into()],
        });
        graph.apply_ops(vec![op_add("用配方法解一元二次方程", &[], 15)]);
        let reference = graph.scope_reference().unwrap();
        assert!(reference.contains("零基础到一元二次方程"));
        assert!(reference.contains("最终目标：能独立解一元二次方程"));
        assert!(reference.contains("用户起点：对代数一无所知"));
        assert!(reference.contains("大块概念"));
        assert!(!reference.contains("预期单元规模"), "{reference}");
        let report = graph.audit_report();
        assert!(report.contains("==== Scope reference ===="));
        assert!(report.contains("已覆盖"), "{report}");
        assert!(report.contains("==== Audit report ===="));
        assert!(report.contains("1 concepts, 0 edges"), "{report}");
    }

    #[test]
    fn view_reports_creation_state() {
        let graph = seeded();
        let view = graph.view("draft-1");
        assert_eq!(view.draft_id, "draft-1");
        assert_eq!(view.node_count, 4);
        assert_eq!(view.revision, 4);
        assert!(view.scope.is_none());
    }
}
