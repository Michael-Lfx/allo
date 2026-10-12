//! 知识结构读侧投影（wayfinder 票 t1 design facts）：`structure_json` 快照
//! 是唯一落盘真相，本模块只是其读侧投影——纯函数、零 LLM、零时钟；IO 只在
//! service 边界读一次快照行。**课程边界铁律**：只重算本课快照，不 join 概念
//! 登记表、不做块→概念任何解析或名字匹配；跨课程概念事实不进本接口。
//! v1 无缓存（一次行读 + 纯函数重算；profiling 有据再加）。
//!
//! 预计算面（前端三图 + 矩阵的数据源）：主干分组、块名集、主干×阶段矩阵格、
//! 依赖分层（层号 = assumes 前置链深度，L0 = 无前置的块；层号不是排期）、
//! 枢纽出入度（assumes + composes 的端点计数，方向反转展示归前端）。

use serde::Serialize;
use serde_json::Value;

use crate::models::{
    BlockView, EdgeView, GapView, HubView, LearningStructureView, MatrixRowView, QualityView,
    SpineGroupView, SpineView, StructureDomainView, StructureScalarsView, StructureView,
    TerrainDoorView, TerrainView,
};

/// 从快照信封构建结构视图。`envelope` 为 None = 结构未生成（Missing 合法
/// 空状态）：200 恒答，`structure`/`terrain` 均为 None。
pub fn build_structure_view(
    course_id: nomifun_common::LearningCourseId,
    envelope: Option<&Value>,
) -> LearningStructureView {
    let Some(envelope) = envelope else {
        return LearningStructureView {
            course_id,
            structure: None,
            terrain: None,
            generated_at: None,
        };
    };
    let structure = envelope.get("structure").cloned().unwrap_or(Value::Null);
    let terrain = envelope.get("terrain").cloned().unwrap_or(Value::Null);
    let generated_at = envelope.get("generated_at").and_then(Value::as_i64);
    LearningStructureView {
        course_id,
        structure: Some(build_structure(&structure)),
        terrain: build_terrain(&terrain),
        generated_at,
    }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn build_structure(structure: &Value) -> StructureView {
    let spines: Vec<SpineView> = structure
        .get("spines")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()

                .map(|spine| SpineView {
                    name: text(spine, "name"),
                    definition: text(spine, "definition"),
                    key_points: strings(spine, "key_points"),
                    instance: text(spine, "instance"),
                    thin_spine: spine.get("thin_spine").and_then(Value::as_bool),
                    reason: text(spine, "reason"),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut blocks: Vec<BlockView> = structure
        .get("blocks")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()

                .map(|block| BlockView {
                    name: text(block, "name"),
                    slug: text(block, "slug"),
                    spine: text(block, "spine"),
                    stage: text(block, "stage"),
                    note: text(block, "note"),
                    layer: 0,
                    no_prereq: true,
                })
                .collect()
        })
        .unwrap_or_default();
    let edges: Vec<EdgeView> = structure
        .get("edges")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()

                .map(|edge| EdgeView {
                    source: text(edge, "source"),
                    target: text(edge, "target"),
                    edge_type: text(edge, "type"),
                    tier: text(edge, "tier"),
                    note: text(edge, "note"),
                })
                .collect()
        })
        .unwrap_or_default();
    let gaps: Vec<GapView> = structure
        .get("gaps")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()

                .map(|gap| GapView {
                    item: text(gap, "item"),
                    direction: text(gap, "direction"),
                })
                .collect()
        })
        .unwrap_or_default();
    let quality: Vec<QualityView> = structure
        .get("quality")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()

                .map(|entry| QualityView {
                    check: text(entry, "check"),
                    status: text(entry, "status"),
                    reason: text(entry, "reason"),
                })
                .collect()
        })
        .unwrap_or_default();

    // 依赖分层：层号 = assumes 前置链深度——L0 = 不做任何 assumes 边之源
    // 的块（不依赖任何其它块），其后逐层加深；层号不是排期。Kahn 拓扑松弛：
    // 环上块（合同不拒环，投影必须稳）永不出队，冻结在可达深度（纯环 = 0）。
    let index_by_name: std::collections::HashMap<String, usize> = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.name.clone(), index))
        .collect();
    let mut last_layer: Vec<usize> = vec![0; blocks.len()];
    let mut pending: Vec<usize> = vec![0; blocks.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); blocks.len()];
    for edge in &edges {
        if edge.edge_type != "assumes" {
            continue;
        }
        if let (Some(&source), Some(&target)) =
            (index_by_name.get(&edge.source), index_by_name.get(&edge.target))
        {
            pending[source] += 1;
            dependents[target].push(source);
        }
    }
    let mut queue: std::collections::VecDeque<usize> = (0..blocks.len())
        .filter(|index| pending[*index] == 0)
        .collect();
    let no_prereq: Vec<bool> = (0..blocks.len())
        .map(|index| pending[index] == 0)
        .collect();
    while let Some(node) = queue.pop_front() {
        for &dependent in &dependents[node] {
            last_layer[dependent] = last_layer[dependent].max(last_layer[node] + 1);
            pending[dependent] -= 1;
            if pending[dependent] == 0 {
                queue.push_back(dependent);
            }
        }
    }
    for (index, block) in blocks.iter_mut().enumerate() {
        block.layer = last_layer[index];
        block.no_prereq = no_prereq[index];
    }

    // 主干分组（块表序保持）+ 块名集 + 主干×阶段矩阵。
    let groups: Vec<SpineGroupView> = spines
        .iter()
        .map(|spine| SpineGroupView {
            spine: spine.name.clone(),
            block_names: blocks
                .iter()
                .filter(|block| block.spine == spine.name)
                .map(|block| block.name.clone())
                .collect(),
        })
        .collect();
    let mut block_names: Vec<String> = blocks.iter().map(|block| block.name.clone()).collect();
    block_names.sort();
    let mut matrix: Vec<MatrixRowView> = spines
        .iter()
        .map(|spine| MatrixRowView {
            spine: spine.name.clone(),
            entryway: Vec::new(),
            intermediate: Vec::new(),
            advanced: Vec::new(),
        })
        .collect();
    for block in &blocks {
        if let Some(row) = matrix.iter_mut().find(|row| row.spine == block.spine) {
            match block.stage.as_str() {
                "入门" => row.entryway.push(block.name.clone()),
                "进阶" => row.intermediate.push(block.name.clone()),
                _ => row.advanced.push(block.name.clone()),
            }
        }
    }

    // 枢纽出入度：assumes（方向反转前）与 composes 的端点计数，出入度和
    // 降序取前十（并列按名升序）。
    let mut degree: std::collections::HashMap<&str, (usize, usize)> =
        std::collections::HashMap::new();
    for edge in &edges {
        if edge.edge_type == "contrasts" {
            continue;
        }
        if let Some(&source) = index_by_name.get(&edge.source) {
            let entry = degree.entry(blocks[source].name.as_str()).or_default();
            entry.1 += 1;
        }
        if let Some(&target) = index_by_name.get(&edge.target) {
            let entry = degree.entry(blocks[target].name.as_str()).or_default();
            entry.0 += 1;
        }
    }
    let mut hubs: Vec<HubView> = degree
        .into_iter()
        .map(|(name, (in_degree, out_degree))| HubView {
            name: name.to_owned(),
            in_degree,
            out_degree,
        })
        .collect();
    hubs.sort_by(|a, b| {
        (b.in_degree + b.out_degree)
            .cmp(&(a.in_degree + a.out_degree))
            .then(a.name.cmp(&b.name))
    });
    hubs.truncate(10);

    let domain = structure.get("domain");
    StructureView {
        scalars: StructureScalarsView {
            goal: text(structure, "goal"),
            goal_type: text(structure, "goal_type"),
            proficiency_statement: text(structure, "proficiency_statement"),
            domain_type: text(structure, "domain_type"),
            form_reference: text(structure, "form_reference"),
            source_tier: text(structure, "source_tier"),
            generated_by: text(structure, "generated_by"),
            disclaimer: text(structure, "disclaimer"),
        },
        domain: StructureDomainView {
            core_definition: domain.map(|domain| text(domain, "core_definition")).unwrap_or_default(),
            includes: domain.map(|domain| strings(domain, "includes")).unwrap_or_default(),
            excludes: domain.map(|domain| strings(domain, "excludes")).unwrap_or_default(),
            prerequisite_doors: domain
                .map(|domain| strings(domain, "prerequisite_doors"))
                .unwrap_or_default(),
            audience: domain.map(|domain| text(domain, "audience")).unwrap_or_default(),
            scenarios: domain.map(|domain| strings(domain, "scenarios")).unwrap_or_default(),
            granularity: domain.map(|domain| text(domain, "granularity")).unwrap_or_default(),
        },
        spines,
        blocks,
        edges,
        gaps,
        quality,
        groups,
        block_names,
        matrix,
        hubs,
    }
}

fn build_terrain(terrain: &Value) -> Option<TerrainView> {
    if !terrain.is_object() {
        return None;
    }
    let doors: Vec<TerrainDoorView> = terrain
        .get("doors")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()

                .map(|door| TerrainDoorView {
                    id: text(door, "id"),
                    criterion: text(door, "criterion"),
                    kind: text(door, "kind"),
                })
                .collect()
        })
        .unwrap_or_default();
    if doors.is_empty() {
        return None;
    }
    Some(TerrainView {
        learner_basis_assumed: text(terrain, "learner_basis_assumed"),
        disclaimer: text(terrain, "disclaimer"),
        doors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_envelope() -> Value {
        json!({
            "version": 1,
            "generated_at": 1000,
            "structure": {
                "goal": "能独立求解",
                "goal_type": "capability",
                "proficiency_statement": "达成后能独立解题",
                "domain_type": "理论",
                "form_reference": "Flowy 学习结构模板 v1",
                "source_tier": "名录",
                "generated_by": "model-x",
                "disclaimer": "参考地图",
                "domain": {
                    "core_definition": "求解体系",
                    "includes": ["变形", "公式", "判别式"],
                    "excludes": ["三次方程", "复数域", "数值分析"],
                    "prerequisite_doors": ["G1"],
                    "audience": "零基础",
                    "scenarios": ["解题", "讲解"],
                    "granularity": "一题一步"
                },
                "spines": [
                    { "name": "主干一", "definition": "d1", "key_points": ["k"], "instance": "i" },
                    { "name": "主干二", "definition": "d2", "key_points": ["k"], "instance": "i" }
                ],
                "blocks": [
                    { "name": "块A", "slug": "block-a", "spine": "主干一", "stage": "入门", "note": "n" },
                    { "name": "块B", "slug": "block-b", "spine": "主干一", "stage": "进阶", "note": "n" },
                    { "name": "块C", "slug": "block-c", "spine": "主干二", "stage": "高级", "note": "n" },
                    { "name": "块D", "slug": "block-d", "spine": "主干二", "stage": "入门", "note": "n" }
                ],
                "edges": [
                    { "source": "块B", "target": "块A", "type": "assumes", "tier": "know", "note": "" },
                    { "source": "块C", "target": "块B", "type": "assumes", "tier": "use", "note": "" },
                    { "source": "块C", "target": "块D", "type": "composes", "tier": "know", "note": "" }
                ],
                "gaps": [{ "item": "g", "direction": "d" }],
                "quality": [{ "check": "c", "status": "met" }]
            },
            "terrain": {
                "goal": "能独立求解",
                "learner_basis_assumed": "纯零基础（未声明）",
                "generated_by": "model-x",
                "disclaimer": "判据快照",
                "doors": [
                    { "id": "G1", "criterion": "能解 x²=p", "kind": "hard" }
                ]
            }
        })
    }

    fn view() -> StructureView {
        build_structure_view(
            nomifun_common::LearningCourseId::new(),
            Some(&sample_envelope()),
        )
        .structure
        .expect("sample has structure")
    }

    /// 依赖分层：L0 = 无前置（不做任何 assumes 边之源），层号 = 前置链深。
    #[test]
    fn layers_follow_assumes_chain_depth() {
        let view = view();
        let layer = |name: &str| {
            view.blocks
                .iter()
                .find(|block| block.name == name)
                .map(|block| (block.layer, block.no_prereq))
                .unwrap()
        };
        assert_eq!(layer("块A"), (0, true), "L0 = 不做任何 assumes 边之源");
        assert_eq!(layer("块B"), (1, false));
        assert_eq!(layer("块C"), (2, false));
        assert_eq!(layer("块D"), (0, true), "composes 不参与依赖分层");
    }

    /// 矩阵与主干分组：按主干×阶段归格，块表序保持。
    #[test]
    fn matrix_and_groups_fold_by_spine_and_stage() {
        let view = view();
        assert_eq!(view.groups[0].spine, "主干一");
        assert_eq!(view.groups[0].block_names, vec!["块A", "块B"]);
        let row = view.matrix.iter().find(|row| row.spine == "主干一").unwrap();
        assert_eq!(row.entryway, vec!["块A"]);
        assert_eq!(row.intermediate, vec!["块B"]);
        assert!(view.block_names.iter().eq(["块A", "块B", "块C", "块D"].iter()), "块名集升序");
    }

    /// 枢纽：出入度按 assumes + composes 计（contrasts 不计），和降序前十
    /// （并列按名升序）。
    #[test]
    fn hubs_rank_by_total_degree() {
        let view = view();
        // 块B（in 1、out 1）与块C（in 0、out 2）同为 2 度，块B 名序在前。
        let top = view.hubs.first().expect("hub exists");
        assert_eq!(top.name, "块B");
        assert_eq!((top.in_degree, top.out_degree), (1, 1));
        assert!(
            view.hubs.iter().any(|hub| hub.name == "块C" && hub.out_degree == 2),
            "{:?}",
            view.hubs
        );
        assert!(view.hubs.iter().any(|hub| hub.name == "块A" && hub.in_degree == 1));
        assert!(
            view.hubs.iter().any(|hub| hub.name == "块D" && hub.in_degree == 1 && hub.out_degree == 0),
            "块D 只被 composes 引用为目标——计入入度而非出度"
        );
    }

    /// None 信封 = Missing：全 None、200 恒答形状。
    #[test]
    fn missing_envelope_is_a_legal_empty_view() {
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), None);
        assert!(view.structure.is_none());
        assert!(view.terrain.is_none());
        assert!(view.generated_at.is_none());
    }

    /// 地形投影：门口列表 + 基础声明回执；空门口 = None（未生成地形）。
    #[test]
    fn terrain_view_carries_doors() {
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&sample_envelope()));
        let terrain = view.terrain.expect("sample has terrain");
        assert_eq!(terrain.doors.len(), 1);
        assert_eq!(terrain.doors[0].id, "G1");
        assert_eq!(terrain.learner_basis_assumed, "纯零基础（未声明）");

        let doorless = json!({ "version": 1, "structure": {}, "terrain": { "doors": [] } });
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&doorless));
        assert!(view.terrain.is_none());
    }

    /// 环安全：assumes 成环时投影不挂、层号冻结在环可达深度。
    #[test]
    fn cyclic_assumes_freeze_instead_of_hanging() {
        let envelope = json!({
            "structure": {
                "spines": [{ "name": "s", "definition": "d", "key_points": ["k"], "instance": "i" }],
                "blocks": [
                    { "name": "X", "slug": "x", "spine": "s", "stage": "入门", "note": "" },
                    { "name": "Y", "slug": "y", "spine": "s", "stage": "入门", "note": "" }
                ],
                "edges": [
                    { "source": "X", "target": "Y", "type": "assumes", "tier": "know" },
                    { "source": "Y", "target": "X", "type": "assumes", "tier": "know" }
                ]
            }
        });
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&envelope));
        let structure = view.structure.expect("structure present");
        assert!(structure.blocks.iter().all(|block| block.layer == 0), "环冻结在 0 层");
    }
}
