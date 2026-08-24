//! Experimental concept-graph feature: decompose a broad learning goal (e.g.
//! "from zero to university mathematics") into atomic concepts linked by
//! prerequisite edges — a complete DAG — in one model call. The normalized
//! graph is persisted by [`crate::service::LearningService`] as JSON files so
//! the UI can revisit it without regenerating.

use std::collections::HashMap;

use nomifun_common::AppError;
use nomifun_knowledge::KnowledgeCompleter;
use serde::{Deserialize, Serialize};

use crate::generation::{complete, parse_json_object};
/// One atomic concept node in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptGraphNode {
    pub id: String,
    pub title: String,
}

/// A prerequisite edge: `from` should be mastered before `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptGraphEdge {
    pub from: String,
    pub to: String,
}

/// The validated, cycle-free DAG payload shared by storage and API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptGraphData {
    pub nodes: Vec<ConceptGraphNode>,
    pub edges: Vec<ConceptGraphEdge>,
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

/// The model must decompose until concepts are atomic and the prerequisite
/// relation is complete; validation then normalizes whatever arrives.
const CONCEPT_GRAPH_SYSTEM: &str = r#"You decompose a learning goal into a complete prerequisite DAG of atomic concepts.
Reply with ONLY one JSON object matching this shape:
{
  "concepts": [
    {"key": "lowercase-stable-key", "title": "concise concept name", "prerequisites": ["earlier-key"]}
  ]
}
Rules:
- Decompose until every concept is atomic: one skill or idea a learner masters in a single study step (e.g. "polynomial factoring", "indefinite integral", "natural numbers"), never a chapter, module, or course.
- Be complete: cover the full span from the most foundational prerequisites up to the stated goal; the goal itself must appear as the top concept.
- Produce 40-120 concepts for broad goals; fewer only for narrow goals.
- "prerequisites" lists the DIRECT prerequisites of the concept; every entry must reference a key defined in the same reply and the relations must form no cycles.
- Write titles in the language of the learning goal.
- Output JSON only, without Markdown fences or commentary."#;

/// Raw per-concept model output; normalization repairs the usual LLM habits
/// (duplicate keys, unknown references, self loops, cycles).
#[derive(Debug, Deserialize)]
struct RawConcept {
    #[serde(default)]
    key: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    prerequisites: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawConceptGraph {
    #[serde(default)]
    concepts: Vec<RawConcept>,
}

pub(crate) const MIN_CONCEPTS: usize = 3;
pub(crate) const MAX_CONCEPTS: usize = 200;

/// One model call with at most one targeted retry, mirroring the blueprint
/// stage: a rejected decomposition is returned to the model with the concrete
/// validation error so the second attempt fixes structure.
pub(crate) async fn generate_concept_graph(
    completer: &dyn KnowledgeCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    topic: &str,
) -> Result<ConceptGraphData, AppError> {
    let mut last_error = String::new();
    for attempt in 0..2 {
        let user = if attempt == 0 {
            format!("Learning goal: {topic}")
        } else {
            format!(
                "Learning goal: {topic}\n\nThe previous decomposition was rejected: {last_error}\n\
                 Return a corrected JSON now, keeping the decomposition complete and atomic."
            )
        };
        let raw = complete(completer, model_override, CONCEPT_GRAPH_SYSTEM, &user).await?;
        match parse_json_object::<RawConceptGraph>(&raw) {
            Ok(parsed) => match normalize_concept_graph(parsed) {
                Ok(graph) => return Ok(graph),
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error,
        }
    }
    Err(AppError::UnprocessableEntity(format!(
        "model did not return a valid concept graph: {last_error}"
    )))
}

/// Repair and validate the raw model output into a DAG:
/// - drops concepts with an empty key or title, deduplicates keys (first wins);
/// - drops prerequisite references to unknown keys, self loops, and duplicates;
/// - drops back edges so the result is always acyclic;
/// - rejects graphs that are too small or too large to be useful.
fn normalize_concept_graph(raw: RawConceptGraph) -> Result<ConceptGraphData, String> {
    use std::collections::{HashMap, HashSet};

    // First occurrence wins on duplicate keys; empty keys/titles are dropped.
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for concept in &raw.concepts {
        let key = concept.key.trim();
        let title = concept.title.trim();
        if key.is_empty() || title.is_empty() || titles.contains_key(key) {
            continue;
        }
        titles.insert(key.to_owned(), title.to_owned());
        order.push(key.to_owned());
    }
    if order.len() < MIN_CONCEPTS {
        return Err(format!(
            "expected at least {MIN_CONCEPTS} concepts with key and title, got {}",
            order.len()
        ));
    }
    if order.len() > MAX_CONCEPTS {
        return Err(format!(
            "expected at most {MAX_CONCEPTS} concepts, got {}; decompose less granularly",
            order.len()
        ));
    }

    // Known references only, no self loops, no duplicate edges.
    let mut seen_edges: HashSet<(String, String)> = HashSet::new();
    let mut edges: Vec<(String, String)> = Vec::new();
    for concept in &raw.concepts {
        let to = concept.key.trim();
        if !titles.contains_key(to) {
            continue;
        }
        for prerequisite in &concept.prerequisites {
            let from = prerequisite.trim();
            let edge = (from.to_owned(), to.to_owned());
            if from == to || !titles.contains_key(from) || !seen_edges.insert(edge.clone()) {
                continue;
            }
            edges.push(edge);
        }
    }

    // Drop back edges (DFS) so the published graph is always a DAG even when
    // the model sneaks in a cycle.
    let acyclic_edges = remove_cycle_edges(&order, &edges);

    let nodes = order
        .into_iter()
        .map(|id| ConceptGraphNode {
            id: id.clone(),
            title: titles.remove(&id).unwrap_or_default(),
        })
        .collect();
    let final_edges = acyclic_edges
        .into_iter()
        .map(|(from, to)| ConceptGraphEdge { from, to })
        .collect();
    Ok(ConceptGraphData {
        nodes,
        edges: final_edges,
    })
}

/// Keep every edge except those that close a cycle: iterative DFS coloring
/// (white/grey/black) over the prereq direction; an edge into a grey node is
/// a back edge and is dropped.
fn remove_cycle_edges(order: &[String], edges: &[(String, String)]) -> Vec<(String, String)> {
    use std::collections::HashSet;

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
    let mut dropped: HashSet<(usize, usize)> = HashSet::new();
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
                        dropped.insert((next, node));
                    }
                    _ => {}
                }
            } else {
                color[node] = 2;
                stack.pop();
            }
        }
    }
    edges
        .iter()
        .filter(|(from, to)| match (index.get(from.as_str()), index.get(to.as_str())) {
            (Some(&from_pos), Some(&to_pos)) => !dropped.contains(&(from_pos, to_pos)),
            _ => false,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(key: &str, title: &str, prerequisites: &[&str]) -> RawConcept {
        RawConcept {
            key: key.to_owned(),
            title: title.to_owned(),
            prerequisites: prerequisites.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn normalization_dedupes_and_drops_invalid_references() {
        let raw = RawConceptGraph {
            concepts: vec![
                concept("a", "A", &[]),
                concept("a", "Duplicate", &[]),
                concept("b", "B", &["a", "missing", "b", "a"]),
                concept(" ", "No key", &[]),
                concept("c", "  ", &[]),
                concept("d", "D", &["b"]),
            ],
        };
        let graph = normalize_concept_graph(raw).unwrap();
        assert_eq!(
            graph.nodes,
            vec![
                ConceptGraphNode { id: "a".into(), title: "A".into() },
                ConceptGraphNode { id: "b".into(), title: "B".into() },
                ConceptGraphNode { id: "d".into(), title: "D".into() },
            ]
        );
        assert_eq!(
            graph.edges,
            vec![
                ConceptGraphEdge { from: "a".into(), to: "b".into() },
                ConceptGraphEdge { from: "b".into(), to: "d".into() },
            ]
        );
    }

    #[test]
    fn normalization_removes_cycles_but_keeps_diamonds() {
        // a -> b, a -> c, b -> d, c -> d is a diamond (kept); d -> a closes a
        // cycle and must be the only dropped edge.
        let raw = RawConceptGraph {
            concepts: vec![
                concept("a", "A", &[]),
                concept("b", "B", &["a"]),
                concept("c", "C", &["a"]),
                concept("d", "D", &["b", "c", "a"]),
                concept("e", "E", &["d"]),
            ],
        };
        let graph = normalize_concept_graph(raw).unwrap();
        assert_eq!(graph.nodes.len(), 5);
        let edges: Vec<(String, String)> = graph
            .edges
            .iter()
            .map(|edge| (edge.from.clone(), edge.to.clone()))
            .collect();
        assert!(!edges.contains(&("d".to_owned(), "a".to_owned())), "cycle edge kept");
        assert!(edges.contains(&("a".to_owned(), "b".to_owned())));
        assert!(edges.contains(&("a".to_owned(), "c".to_owned())));
        assert!(edges.contains(&("b".to_owned(), "d".to_owned())));
        assert!(edges.contains(&("c".to_owned(), "d".to_owned())));
        assert!(edges.contains(&("d".to_owned(), "e".to_owned())));
    }

    #[test]
    fn normalization_rejects_too_small_and_too_large() {
        let small = RawConceptGraph {
            concepts: vec![concept("a", "A", &[]), concept("b", "B", &["a"])],
        };
        assert!(normalize_concept_graph(small).is_err());

        let large = RawConceptGraph {
            concepts: (0..(MAX_CONCEPTS + 1))
                .map(|i| concept(&format!("k{i}"), &format!("T{i}"), &[]))
                .collect(),
        };
        assert!(normalize_concept_graph(large).is_err());
    }
}
