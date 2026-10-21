//! 结构发现的确定性后处理(纯函数,t2):DFS 去环、Kahn 拓扑排序、
//! Jaccard 覆盖补齐、Overview 兜底。全部无 IO、无 LLM,可独立重放。

use super::*;
use std::collections::{HashMap, HashSet, VecDeque};

/// DFS 去环:只在 `depends_on` 边(顺序约束)里找环;每找到一个环,丢掉
/// 环上 `rationale` 最短的 `depends_on` 边(置信度代理),记一条 warning;
/// 上限 [`STRUCTURE_CYCLE_BREAK_MAX_EDGES`] 次。返回是否仍有残留环。
pub(crate) fn break_cycles(graph: &mut ConceptGraph, warnings: &mut Vec<String>) -> bool {
    for _ in 0..STRUCTURE_CYCLE_BREAK_MAX_EDGES {
        let Some(cycle) = find_cycle(graph) else {
            return false;
        };
        // 环上 rationale 最短的边(平局取先出现的)。
        let mut victim: Option<usize> = None;
        for edge in &cycle {
            let position = graph
                .edges
                .iter()
                .position(|candidate| candidate.src == edge.src && candidate.dst == edge.dst)
                .unwrap_or(usize::MAX);
            let better = match victim {
                None => true,
                Some(current) => {
                    let current_len = graph.edges[current].rationale.chars().count();
                    current_len > edge.rationale.chars().count()
                }
            };
            if better && position != usize::MAX {
                victim = Some(position);
            }
        }
        match victim {
            Some(position) => {
                let removed = graph.edges.remove(position);
                warnings.push(format!(
                    "概念依赖存在环,已丢弃置信度最低(rationale 最短)的边 {} → {}: {}",
                    removed.src, removed.dst, removed.rationale
                ));
            }
            None => return !find_cycle(graph).is_none(),
        }
    }
    let residual = find_cycle(graph).is_some();
    if residual {
        warnings.push(format!(
            "概念依赖去环超过 {} 次上限仍有残留环;拓扑排序将回退原序",
            STRUCTURE_CYCLE_BREAK_MAX_EDGES
        ));
    }
    residual
}

/// 找一条 depends_on 环(三色 DFS;返回环上的边序列,无环返回 None)。
fn find_cycle(graph: &ConceptGraph) -> Option<Vec<ConceptGraphEdge>> {
    let mut adjacency: HashMap<&str, Vec<&ConceptGraphEdge>> = HashMap::new();
    for edge in &graph.edges {
        if edge.relation == ConceptEdgeRelation::DependsOn {
            adjacency.entry(edge.src.as_str()).or_default().push(edge);
        }
    }
    // color: 0=白 1=灰 2=黑;栈记录当前路径边。
    let mut color: HashMap<&str, u8> = HashMap::new();
    let mut stack: Vec<&ConceptGraphEdge> = Vec::new();

    fn dfs<'a>(
        node: &'a str,
        adjacency: &HashMap<&'a str, Vec<&'a ConceptGraphEdge>>,
        color: &mut HashMap<&'a str, u8>,
        stack: &mut Vec<&'a ConceptGraphEdge>,
    ) -> Option<Vec<ConceptGraphEdge>> {
        color.insert(node, 1);
        for edge in adjacency.get(node).map(Vec::as_slice).unwrap_or(&[]) {
            match color.get(edge.dst.as_str()).copied().unwrap_or(0) {
                1 => {
                    // 回边:环起点是灰色祖先(edge.dst),从栈里截出环上的边。
                    let start = stack
                        .iter()
                        .position(|candidate| candidate.src == edge.dst)
                        .unwrap_or(0);
                    let mut cycle: Vec<ConceptGraphEdge> =
                        stack[start..].iter().map(|e| (*e).clone()).collect();
                    cycle.push((*edge).clone());
                    return Some(cycle);
                }
                0 => {
                    stack.push(edge);
                    if let Some(cycle) = dfs(&edge.dst, adjacency, color, stack) {
                        return Some(cycle);
                    }
                    stack.pop();
                }
                _ => {}
            }
        }
        color.insert(node, 2);
        None
    }

    let nodes: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    for node in nodes {
        if color.get(node).copied().unwrap_or(0) == 0 {
            stack.clear();
            if let Some(cycle) = dfs(node, &adjacency, &mut color, &mut stack) {
                return Some(cycle);
            }
        }
    }
    None
}

/// 章际依赖图:概念 `depends_on` 提升(每概念归到首个覆盖它的章)+ 章节声明
/// 的 `prerequisites`(按标题)。返回 (依赖边 [章索引依赖章索引], 未知前置
/// 的「章 → 前置」披露清单)。
fn chapter_dependencies(
    chapters: &[PlannedChapter],
    graph: &ConceptGraph,
) -> (Vec<(usize, usize)>, Vec<String>) {
    // 概念 → 首个覆盖它的章(确定性:按章节序)。
    let mut concept_owner: HashMap<&str, usize> = HashMap::new();
    for (index, chapter) in chapters.iter().enumerate() {
        for concept in &chapter.covers {
            concept_owner.entry(concept.as_str()).or_insert(index);
        }
    }
    let mut unresolved: Vec<String> = Vec::new();
    let mut deps: HashSet<(usize, usize)> = HashSet::new();
    for edge in &graph.edges {
        if edge.relation != ConceptEdgeRelation::DependsOn {
            continue;
        }
        if let (Some(&src_chapter), Some(&dst_chapter)) =
            (concept_owner.get(edge.src.as_str()), concept_owner.get(edge.dst.as_str()))
        {
            if src_chapter != dst_chapter {
                deps.insert((src_chapter, dst_chapter));
            }
        }
    }
    // 标题 → 索引(重名取首个;章标题在规范化阶段已去重)。
    let mut title_index: HashMap<&str, usize> = HashMap::new();
    for (index, chapter) in chapters.iter().enumerate() {
        title_index.entry(chapter.title.trim()).or_insert(index);
    }
    for (index, chapter) in chapters.iter().enumerate() {
        for prerequisite in &chapter.prerequisites {
            match title_index.get(prerequisite.trim()) {
                Some(&pre) if pre != index => {
                    deps.insert((index, pre));
                }
                Some(_) => {}
                None => unresolved.push(format!("{} → {}", chapter.title, prerequisite)),
            }
        }
    }
    (deps.into_iter().collect(), unresolved)
}

/// Kahn 拓扑排序章节:依赖 = 概念 depends_on 提升 + 章节 prerequisites;
/// 平局按原序(反复取「依赖已满足的最前章」);残留环回退原序 + warning。
/// 返回 (新序章节列表, 未知前置披露)。
pub(crate) fn topo_sort_chapters(
    chapters: &[PlannedChapter],
    graph: &ConceptGraph,
    warnings: &mut Vec<String>,
) -> (Vec<PlannedChapter>, Vec<String>) {
    let (deps, unresolved) = chapter_dependencies(chapters, graph);
    for reference in &unresolved {
        warnings.push(format!("章节前置引用了不存在的章「{reference}」;已忽略该前置"));
    }
    let depends_on: Vec<HashSet<usize>> = {
        let mut lists = vec![HashSet::new(); chapters.len()];
        for (src, dst) in &deps {
            lists[*src].insert(*dst);
        }
        lists
    };

    let mut ordered: Vec<usize> = Vec::with_capacity(chapters.len());
    let mut placed = vec![false; chapters.len()];
    let mut remaining: VecDeque<usize> = (0..chapters.len()).collect();
    while let Some(position) = remaining
        .iter()
        .position(|&index| depends_on[index].iter().all(|dep| placed[*dep]))
    {
        let index = remaining.remove(position).expect("position 刚刚还在");
        placed[index] = true;
        ordered.push(index);
        if remaining.is_empty() {
            break;
        }
    }
    if !remaining.is_empty() {
        let stuck: Vec<String> = remaining
            .iter()
            .map(|&index| chapters[index].title.clone())
            .collect();
        warnings.push(format!(
            "章节依赖存在环({});拓扑排序回退原序",
            stuck.join("、")
        ));
        ordered.extend(remaining);
    }
    (ordered.into_iter().map(|index| chapters[index].clone()).collect(), unresolved)
}

/// 字符二元组集合(中文友好的廉价 token 化;单字符退化为自身)。
fn bigrams(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut set = HashSet::with_capacity(chars.len());
    if chars.len() < 2 {
        if let Some(only) = chars.first() {
            set.insert(only.to_string());
        }
        return set;
    }
    for window in chars.windows(2) {
        set.insert(window.iter().collect());
    }
    set
}

fn jaccard(left: &str, right: &str) -> f32 {
    let left = bigrams(left);
    let right = bigrams(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count();
    let union = left.union(&right).count();
    intersection as f32 / union as f32
}

/// Jaccard 覆盖补齐:未被任何章 `covers` 的概念,按 `label+description` 与
/// `title+summary+objectives` 的二元组 Jaccard 挂到最相关章(平局取最前)。
pub(crate) fn complete_uncovered_concepts(chapters: &mut [PlannedChapter], graph: &ConceptGraph) {
    if chapters.is_empty() {
        return;
    }
    let covered: HashSet<String> = chapters
        .iter()
        .flat_map(|chapter| chapter.covers.iter().cloned())
        .collect();
    for node in &graph.nodes {
        if covered.contains(&node.id) {
            continue;
        }
        let probe = format!("{}{}", node.label, node.description);
        let mut best: Option<(usize, f32)> = None;
        for (index, chapter) in chapters.iter().enumerate() {
            let target = format!(
                "{}{}{}",
                chapter.title,
                chapter.summary,
                chapter.learning_objectives.join("")
            );
            let score = jaccard(&probe, &target);
            if best.is_none_or(|(_, current)| score > current) {
                best = Some((index, score));
            }
        }
        if let Some((index, _)) = best {
            chapters[index].covers.push(node.id.clone());
        }
    }
}

/// 无章节兜底:证据在手但模型未产出任何章 → 合成单一 Overview 章(summary
/// 取证据摘要、covers 全部概念、path 级锚点标 `anchor_uncertain`)。
pub(crate) fn overview_fallback(
    evidence_summary: &str,
    graph: &ConceptGraph,
    anchors: Vec<SourceAnchor>,
) -> PlannedChapter {
    PlannedChapter {
        title: "Overview".to_owned(),
        summary: evidence_summary.to_owned(),
        learning_objectives: Vec::new(),
        covers: graph.nodes.iter().map(|node| node.id.clone()).collect(),
        prerequisites: Vec::new(),
        source_anchors: anchors,
        content_type: ChapterContentType::Reference,
    }
}
