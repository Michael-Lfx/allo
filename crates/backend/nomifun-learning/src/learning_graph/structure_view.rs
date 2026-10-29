//! 知识结构读侧投影（wayfinder 票 t1 design facts）：`structure_json` 快照
//! 是唯一落盘真相，本模块只是其读侧投影——纯函数、零 LLM、零时钟；IO 只在
//! service 边界读一次快照行。**课程边界铁律**：只重算本课快照，不 join 概念
//! 登记表、不做块→概念任何解析或名字匹配；跨课程概念事实不进本接口。
//! v1 无缓存（一次行读 + 纯函数重算；profiling 有据再加）。
//!
//! 预计算面（前端三图 + 矩阵的数据源）：主干分组、块名集、主干×阶段矩阵格、
//! 依赖分层（层号 = assumes 前置链深度，L0 = 无前置的块；层号不是排期）、
//! 枢纽出入度（assumes + composes 的端点计数，方向反转展示归前端）。

use serde_json::Value;

use crate::models::{
    BlockView, EdgeView, GapView, HubView, LearningStructureView, MatrixRowView, MountBlockRefView,
    MountEntryView, MountIndexEntryView, MountObservationView, NodeMountStateView, QualityView,
    SpineGroupView, SpineView, StructureDomainView, StructureMountsView, StructureScalarsView,
    StructureView, TerrainDoorView, TerrainView,
};

/// 从快照信封构建结构视图。`envelope` 为 None = 结构未生成（Missing 合法
/// 空状态）：200 恒答，`structure`/`terrain` 均为 None。`mount_row` =
/// (mounts_json, stored_hash)；`current_hash` = 当前 structure_json 字节的
/// FNV-1a 64——两者比对即 `stale`（结构变过，挂载显影）。`node_titles` =
/// 现存节点标题（三态分母）。
pub fn build_structure_view(
    course_id: nomifun_common::LearningCourseId,
    envelope: Option<&Value>,
    node_titles: &[String],
    mount_row: Option<(&str, i64)>,
    current_hash: u64,
) -> LearningStructureView {
    let Some(envelope) = envelope else {
        return LearningStructureView {
            course_id,
            structure: None,
            terrain: None,
            generated_at: None,
            mounts: None,
        };
    };
    let structure = envelope.get("structure").cloned().unwrap_or(Value::Null);
    let terrain = envelope.get("terrain").cloned().unwrap_or(Value::Null);
    let generated_at = envelope.get("generated_at").and_then(Value::as_i64);
    let mounts = mount_row.map(|(mounts_json, stored_hash)| {
        let stored = serde_json::from_str::<Value>(mounts_json)
            .map(|value| super::mounting::parse_mount_entries(&value))
            .unwrap_or_default();
        build_mounts_view(
            &structure,
            &stored,
            node_titles,
            (stored_hash as u64) != current_hash,
        )
    });
    let structure_view = build_structure(&structure);
    let terrain_view = build_terrain(&terrain, &structure_view.blocks);
    LearningStructureView {
        course_id,
        structure: Some(structure_view),
        terrain: terrain_view,
        generated_at,
        mounts,
    }
}

/// 挂载读侧三态投影（纯函数）：三态单源（Mounted/Outside/NotEvaluated，
/// 悬挂 slug 逐条降级为 NotEvaluated 不 Broken）+ mount_index（块 slug →
/// 已挂节点清单，只含可解析块）+ 挂载观察派生（结构外/空块/超载）。
pub fn build_mounts_view(
    structure: &Value,
    entries: &[super::mounting::MountEntry],
    node_titles: &[String],
    stale: bool,
) -> StructureMountsView {
    let block_names: std::collections::HashMap<String, String> = structure
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    let slug = block.get("slug")?.as_str()?;
                    let name = block.get("name")?.as_str()?;
                    Some((slug.to_owned(), name.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    let entry_views: Vec<MountEntryView> = entries
        .iter()
        .map(|entry| {
            let refs: Vec<MountBlockRefView> = entry
                .blocks
                .iter()
                .map(|slug| {
                    let block_name = block_names.get(slug).cloned();
                    MountBlockRefView {
                        slug: slug.clone(),
                        dangling: block_name.is_none(),
                        block_name,
                    }
                })
                .collect();
            MountEntryView {
                node: entry.node.clone(),
                blocks: refs,
                reason: entry.reason.clone(),
            }
        })
        .collect();
    // 三态单源：Mounted = 条目且至少一个块可解析（悬挂块逐条降级标注，不
    // Broken）；Outside = 条目 blocks 空（已评估、声明结构外）；
    // NotEvaluated = 无条目。
    let by_node: std::collections::HashMap<&str, &super::mounting::MountEntry> =
        entries.iter().map(|entry| (entry.node.as_str(), entry)).collect();
    let mut states = Vec::new();
    let mut mounted_count = 0usize;
    let mut outside_count = 0usize;
    let mut not_evaluated_count = 0usize;
    for title in node_titles {
        let state = match by_node.get(title.as_str()) {
            Some(entry) if !entry.blocks.is_empty() => {
                let resolves = entry
                    .blocks
                    .iter()
                    .any(|slug| block_names.contains_key(slug));
                if resolves {
                    mounted_count += 1;
                    "mounted"
                } else {
                    not_evaluated_count += 1;
                    "not_evaluated"
                }
            }
            Some(_) => {
                outside_count += 1;
                "outside"
            }
            None => {
                not_evaluated_count += 1;
                "not_evaluated"
            }
        };
        states.push(NodeMountStateView {
            node: title.clone(),
            state: state.to_owned(),
        });
    }
    // mount_index：可解析块 → 已挂节点清单（结构顺序；多块节点进多个清单）。
    let mut index = Vec::new();
    for (slug, name) in &block_names {
        let nodes: Vec<String> = entries
            .iter()
            .filter(|entry| entry.blocks.iter().any(|s| s == slug))
            .map(|entry| entry.node.clone())
            .collect();
        if !nodes.is_empty() {
            index.push(MountIndexEntryView {
                block: slug.clone(),
                block_name: Some(name.clone()),
                nodes,
            });
        }
    }
    let observations = super::mounting::mount_observations(
        &block_names.iter().map(|(s, n)| (s.clone(), n.clone())).collect::<Vec<_>>(),
        entries,
    )
    .into_iter()
    .map(|(kind, detail)| MountObservationView { kind, detail })
    .collect();
    StructureMountsView {
        stale,
        entries: entry_views,
        index,
        states,
        observations,
        mounted_count,
        outside_count,
        not_evaluated_count,
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

/// 结构快照 → 结构视图（纯解析，单源读侧）：已会声明管线复用同一解析路径
/// 取块 door_refs（m1 AND 命中口径与读侧同源，零第二真相）。
pub(crate) fn build_structure(structure: &Value) -> StructureView {
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
                    // 门位标签（m1）：旧快照无键 = 空数组（serde default 同义）。
                    door_refs: strings(block, "door_refs"),
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
            match super::structure_contract::stage_rank(&block.stage) {
                0 => row.entryway.push(block.name.clone()),
                1 => row.intermediate.push(block.name.clone()),
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

/// 地形快照 → 地形视图（纯解析）：已会声明管线复用它取课程门口清单
/// （勾选 ⊆ 课程门口的校验面与过门自检读侧同源）。
pub(crate) fn build_terrain(terrain: &Value, blocks: &[BlockView]) -> Option<TerrainView> {
    if !terrain.is_object() {
        return None;
    }
    let doors: Vec<TerrainDoorView> = terrain
        .get("doors")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|door| {
                    let id = text(door, "id");
                    // 块覆盖统计（m1）：door_refs 引用本门口的块数（读侧机械
                    // 读数，消费面 = 过门自检；引用不存在门口的块不计入）。
                    let covered_blocks = blocks
                        .iter()
                        .filter(|block| block.door_refs.iter().any(|referenced| referenced == &id))
                        .count();
                    TerrainDoorView {
                        id,
                        criterion: text(door, "criterion"),
                        kind: text(door, "kind"),
                        covered_blocks,
                    }
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

/// 结构参考材料行渲染（教练上下文块 2，g3 裁决的行格式与呈现序）：
/// `- 块名(slug)|主题一句|难度档|主干|无前置` 或 `…|前置:A、B`；
/// 呈现序 = 难度档 / 前置链深 / 主干序 / 块序。纯结构事实，不裁剪候选面。
pub(crate) fn render_reference_lines(structure: &Value) -> String {
    let view = build_structure(structure);
    if view.blocks.is_empty() {
        return String::new();
    }
    let mut prereqs: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for edge in &view.edges {
        if edge.edge_type != "assumes" {
            continue;
        }
        prereqs
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
    }
    let spine_order: std::collections::HashMap<&str, usize> = view
        .groups
        .iter()
        .enumerate()
        .map(|(index, group)| (group.spine.as_str(), index))
        .collect();
    let stage_rank = super::structure_contract::stage_rank;
    let mut rows: Vec<(usize, usize, usize, usize, String)> = view
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let note = if block.note.trim().is_empty() { "—" } else { block.note.trim() };
            let tail = match prereqs.get(block.name.as_str()) {
                Some(targets) if !targets.is_empty() => {
                    format!("前置:{}", targets.join("、"))
                }
                _ => "无前置".to_owned(),
            };
            let line = format!(
                "- {}({})|{}|{}|{}|{}",
                block.name, block.slug, note, block.stage, block.spine, tail
            );
            (
                stage_rank(&block.stage),
                block.layer,
                spine_order.get(block.spine.as_str()).copied().unwrap_or(usize::MAX),
                index,
                line,
            )
        })
        .collect();
    rows.sort_by_key(|(stage, layer, spine, index, _)| (*stage, *layer, *spine, *index));
    rows.into_iter()
        .map(|(_, _, _, _, line)| line)
        .collect::<Vec<_>>()
        .join("\n")
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
                    { "name": "块A", "slug": "block-a", "spine": "主干一", "stage": "入门", "note": "n", "door_refs": ["G1"] },
                    { "name": "块B", "slug": "block-b", "spine": "主干一", "stage": "进阶", "note": "n" },
                    { "name": "块C", "slug": "block-c", "spine": "主干二", "stage": "高级", "note": "n" },
                    { "name": "块D", "slug": "block-d", "spine": "主干二", "stage": "入门", "note": "n", "door_refs": ["G1", "G2"] }
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
                    { "id": "G1", "criterion": "能解 x²=p", "kind": "hard" },
                    { "id": "G2", "criterion": "可辨认等式变形的合法性", "kind": "self_check" }
                ]
            }
        })
    }

    fn view() -> StructureView {
        build_structure_view(
            nomifun_common::LearningCourseId::new(),
            Some(&sample_envelope()),
            &[],
            None,
            0,
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
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), None, &[], None, 0);
        assert!(view.structure.is_none());
        assert!(view.terrain.is_none());
        assert!(view.generated_at.is_none());
    }

    /// 地形投影：门口列表 + 基础声明回执；空门口 = None（未生成地形）。
    #[test]
    fn terrain_view_carries_doors() {
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&sample_envelope()), &[], None, 0);
        let terrain = view.terrain.expect("sample has terrain");
        assert_eq!(terrain.doors.len(), 2);
        assert_eq!(terrain.doors[0].id, "G1");
        assert_eq!(terrain.learner_basis_assumed, "纯零基础（未声明）");

        let doorless = json!({ "version": 1, "structure": {}, "terrain": { "doors": [] } });
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&doorless), &[], None, 0);
        assert!(view.terrain.is_none());
    }

    // ── m1（learnhub #165，契约 v2.14）：门位标签读侧 ────────────────────────

    /// 块视图携带 door_refs；无键旧快照 = 空 Vec（serde default 容旧快照）。
    #[test]
    fn block_views_carry_door_refs_with_legacy_snapshot_default() {
        let view = view();
        let refs = |name: &str| {
            view.blocks
                .iter()
                .find(|block| block.name == name)
                .map(|block| block.door_refs.clone())
                .unwrap()
        };
        assert_eq!(refs("块A"), vec!["G1".to_owned()]);
        assert_eq!(refs("块D"), vec!["G1".to_owned(), "G2".to_owned()]);
        assert_eq!(refs("块B"), Vec::<String>::new(), "无键旧快照 = 空数组");
        assert_eq!(refs("块C"), Vec::<String>::new());
    }

    /// TerrainView 块覆盖统计：每条门口覆盖块数（块 door_refs 引用计一次；
    /// 引用不存在门口的块不计入任何门口）。
    #[test]
    fn terrain_view_counts_covered_blocks_per_door() {
        let full = build_structure_view(
            nomifun_common::LearningCourseId::new(),
            Some(&sample_envelope()),
            &[],
            None,
            0,
        );
        let terrain = full.terrain.expect("sample has terrain");
        let count = |id: &str| {
            terrain
                .doors
                .iter()
                .find(|door| door.id == id)
                .map(|door| door.covered_blocks)
                .unwrap()
        };
        assert_eq!(count("G1"), 2, "块A + 块D 引用 G1");
        assert_eq!(count("G2"), 1, "仅块D 引用 G2");

        // 旧快照：块无 door_refs 键 = 全部计数 0，投影不炸。
        let mut legacy = sample_envelope();
        for block in legacy["structure"]["blocks"].as_array_mut().unwrap() {
            block.as_object_mut().unwrap().remove("door_refs");
        }
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&legacy), &[], None, 0);
        let terrain = view.terrain.expect("legacy has terrain");
        assert!(terrain.doors.iter().all(|door| door.covered_blocks == 0));
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
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&envelope), &[], None, 0);
        let structure = view.structure.expect("structure present");
        assert!(structure.blocks.iter().all(|block| block.layer == 0), "环冻结在 0 层");
    }
}
