//! 知识结构读侧投影（wayfinder 票 t1 design facts）：`structure_json` 快照
//! 是唯一落盘真相，本模块只是其读侧投影——纯函数、零 LLM、零时钟；IO 只在
//! service 边界读一次快照行。**课程边界铁律**：只重算本课快照，不 join 概念
//! 登记表、不做块→概念任何解析或名字匹配；跨课程概念事实不进本接口。
//! v1 无缓存（一次行读 + 纯函数重算；profiling 有据再加）。
//!
//! 预计算面（前端三图 + 矩阵的数据源）：主干分组、块名集、主干×阶段矩阵格、
//! 依赖分层（层号 = assumes 前置链深度，L0 = 无前置的块；层号不是排期）、
//! 枢纽出入度（assumes + composes 的端点计数，方向反转展示归前端）。

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::Value;

use crate::models::{
    BlockView, EdgeView, GapView, HubView, LearningStructureView, MatrixRowView, MountBlockRefView,
    MountEntryView, MountIndexEntryView, MountObservationView, NodeMountStateView, QualityView,
    SpineGroupView, SpineView, StructureDomainView, StructureMountsView, StructureScalarsView,
    StructureView, TerrainDoorView, TerrainView,
};

/// 从快照信封构建结构视图。`envelope` 为 None = 结构未生成（Missing 合法
/// 空状态）：200 恒答，`structure`/`terrain` 均为 None。`mount_row` =
/// (mounts_json, stored_hash, hash_version)；`stale` = 调用方按存量
/// hash_version 选算法比较后的结论（结构变过，挂载显影）。`node_titles` =
/// 现存节点标题（三态分母）。
pub fn build_structure_view(
    course_id: nomifun_common::LearningCourseId,
    envelope: Option<&Value>,
    node_titles: &[String],
    mount_row: Option<(&str, i64, i64)>,
    stale: bool,
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
    // 读侧归一（x08）：legacy v1 中文字面量只在视图层转码显示，不落盘；
    // v2 数据零改写。
    let mut normalized = envelope.clone();
    super::structure_contract::normalize_structure_vocab(&mut normalized);
    let structure = normalized.get("structure").cloned().unwrap_or(Value::Null);
    let terrain = normalized.get("terrain").cloned().unwrap_or(Value::Null);
    let generated_at = normalized.get("generated_at").and_then(Value::as_i64);
    let mounts = mount_row.map(|(mounts_json, _stored_hash, _stored_version)| {
        let stored = serde_json::from_str::<Value>(mounts_json)
            .map(|value| super::mounting::parse_mount_entries(&value))
            .unwrap_or_default();
        build_mounts_view(&structure, &stored, node_titles, stale)
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

// ── 地形清单（sb1，ADR-0018 决定 2）─────────────────────────────────────────

/// 地形清单的来源说明行（置于清单主体之前）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerrainListNote {
    /// 尚无挂载行：清单即完整地形。
    NoMounts,
    /// 挂载行已过时（结构变过、待重新检查）：整表按未覆盖处理，清单回到
    /// 完整地形。
    StaleMounts,
}

impl TerrainListNote {
    fn line(self) -> &'static str {
        match self {
            Self::NoMounts => "尚无节点挂载记录，以下为完整地形",
            Self::StaleMounts => "结构已更新，节点与结构的对应待重新检查，以下为完整地形",
        }
    }
}

/// 一行地形：块 slug（覆盖判定键）+ 块名（折叠行用）+ 呈现行。
struct TerrainRow {
    slug: String,
    name: String,
    line: String,
}

/// 地形清单的覆盖读数：`covered` = 已覆盖块的 slug → 挂在该块的活节点数；
/// `note` = 清单前的说明行（`None` = 挂载行 fresh）。
pub(crate) struct TerrainCoverage {
    pub covered: HashMap<String, usize>,
    pub note: Option<TerrainListNote>,
}

/// 挂载行 → 地形清单的覆盖读数（ADR-0018 决定 2：已覆盖的唯一依据是挂载
/// 表——一个块挂了至少 1 个未删除节点才算覆盖）。宁可多展开：错误折叠的
/// 代价是单向的，拿过期地图告诉教练「这块不用长了」比多展开几行坏得多
/// （多展开的代价只是教练可能重复提名，由花名册与已教账拦住）。纯函数、
/// 读路径不写库。
///
/// - `structure_raw` = 整列 `structure_json` 的原始字符串（stale 判定按它
///   算 hash，不是内层 structure）。
/// - `node_titles` = 现存节点标题（节点是硬删，孤儿条目要到下一轮挂载才
///   被清出）。
/// - `mount_row` = (mounts_json, structure_hash, hash_version)，同
///   [`build_structure_view`]。
///
/// 无挂载行 → 空覆盖集 + 尚无记录说明；行 stale（结构变过）→ 空覆盖集 +
/// stale 说明；行 fresh → 只数「活节点 × 结构内 slug」的条目（条目里的
/// 悬挂 slug 不计，但不牵连同条目其余仍有效的 slug）。
pub(crate) fn terrain_coverage(
    structure: &Value,
    structure_raw: &str,
    node_titles: &HashSet<String>,
    mount_row: Option<(&str, i64, i64)>,
) -> TerrainCoverage {
    let Some((mounts_json, stored_hash, stored_version)) = mount_row else {
        return TerrainCoverage {
            covered: HashMap::new(),
            note: Some(TerrainListNote::NoMounts),
        };
    };
    if super::mounting::mounts_stale(stored_hash, stored_version, structure_raw) {
        return TerrainCoverage {
            covered: HashMap::new(),
            note: Some(TerrainListNote::StaleMounts),
        };
    }
    // 挂载是导航辅助面：个别坏行不使整体失效（与读侧视图同款宽松解析）。
    let entries = serde_json::from_str::<Value>(mounts_json)
        .map(|value| super::mounting::parse_mount_entries(&value))
        .unwrap_or_default();
    let block_slugs: HashSet<&str> = structure
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block.get("slug").and_then(Value::as_str))
                .filter(|slug| !slug.trim().is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut covered: HashMap<String, usize> = HashMap::new();
    for entry in entries
        .iter()
        .filter(|entry| node_titles.contains(&entry.node))
    {
        // 条目内去重：同一 slug 写两遍仍只算一个节点挂了这块。
        let slugs: HashSet<&str> = entry.blocks.iter().map(String::as_str).collect();
        for slug in slugs.into_iter().filter(|slug| block_slugs.contains(slug)) {
            *covered.entry(slug.to_owned()).or_insert(0) += 1;
        }
    }
    TerrainCoverage {
        covered,
        note: None,
    }
}

/// 结构快照 → 呈现行（纯解析，单源）：`- 块名(slug)|主题一句|难度档|主干|无前置`
/// 或 `…|前置:A、B`；呈现序 = 难度档 / 前置链深 / 主干序 / 块序（层号来自
/// [`build_structure`]，环冻结不挂）。
fn terrain_rows(structure: &Value) -> Vec<TerrainRow> {
    let view = build_structure(structure);
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
    let mut rows: Vec<(usize, usize, usize, usize, TerrainRow)> = view
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
                TerrainRow {
                    slug: block.slug.clone(),
                    name: block.name.clone(),
                    line,
                },
            )
        })
        .collect();
    rows.sort_by_key(|(stage, layer, spine, index, _)| (*stage, *layer, *spine, *index));
    rows.into_iter()
        .map(|(_, _, _, _, row)| row)
        .collect()
}

/// 地形清单渲染（教练上下文块 2，ADR-0018 决定 2）：`covered` 之外的块逐块
/// 展开，行格式与呈现序见 [`terrain_rows`]；`covered` 里的块折叠成一行
/// 「已覆盖 {k} 块：名称、名称…」（名称同呈现序，置于清单之前）。折叠行
/// **不报节点数**：`covered` 的值是各块的挂载数，一个节点挂多块时相加会
/// 重复计，报出去容易被读成「有多少个节点已就位」；教练只需要知道哪些块
/// 已覆盖。`note` = 清单前的说明行。无块 = 空串（整块省略）。纯结构事实的
/// 读侧投影：不做块名↔概念名匹配，不参与任何判定。
/// 测试兼容包装（仅测试构建）：空声明集、非首批 = 旧全量输出。生产走
/// [`render_terrain_list_with_declared`]。
#[cfg(test)]
pub(crate) fn render_terrain_list(
    structure: &Value,
    covered: &HashMap<String, usize>,
    note: Option<TerrainListNote>,
) -> String {
    render_terrain_list_with_declared(structure, covered, note, &BTreeSet::new(), false)
}

/// Stage 1c：首批（课程尚无节点）参考面减去本课已会声明块（slug 精确匹配）。
/// **只减自报事实**：未声明块全量保留，即使其前置块被声明剔除也不级联；
/// 无 slug 的块永不被命中。剔除面只用于注记/可读性，不参与判定。
pub(crate) fn render_terrain_list_with_declared(
    structure: &Value,
    covered: &HashMap<String, usize>,
    note: Option<TerrainListNote>,
    declared_blocks: &BTreeSet<String>,
    first_batch: bool,
) -> String {
    let rows = terrain_rows(structure);
    if rows.is_empty() {
        return String::new();
    }
    let (declared_rows, rows): (Vec<TerrainRow>, Vec<TerrainRow>) = if first_batch {
        rows.into_iter()
            .partition(|row| !row.slug.is_empty() && declared_blocks.contains(&row.slug))
    } else {
        (Vec::new(), rows)
    };
    let mut lines: Vec<String> = note.map(|note| note.line().to_owned()).into_iter().collect();
    if !declared_rows.is_empty() {
        let names = declared_rows
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>()
            .join("、");
        lines.push(format!(
            "已会声明块（本批不装）：{names}——其概念已在已教概念账，不要为它们生成学习节点"
        ));
    }
    // 已覆盖 = 挂了至少 1 个节点：计数为 0 或 slug 已不在结构里的条目不算。
    let (folded, expanded): (Vec<&TerrainRow>, Vec<&TerrainRow>) = rows
        .iter()
        .partition(|row| covered.get(&row.slug).is_some_and(|count| *count > 0));
    if !folded.is_empty() {
        let names = folded
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>()
            .join("、");
        lines.push(format!("已覆盖 {} 块：{names}", folded.len()));
    }
    lines.extend(expanded.into_iter().map(|row| row.line.to_owned()));
    lines.join("\n")
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
                "domain_type": "theory",
                "form_reference": "Flowy 学习结构模板 v1",
                "source_tier": "catalog",
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
                    { "name": "块A", "slug": "block-a", "spine": "主干一", "stage": "basic", "note": "n", "door_refs": ["G1"] },
                    { "name": "块B", "slug": "block-b", "spine": "主干一", "stage": "advanced", "note": "n" },
                    { "name": "块C", "slug": "block-c", "spine": "主干二", "stage": "expert", "note": "n" },
                    { "name": "块D", "slug": "block-d", "spine": "主干二", "stage": "basic", "note": "n", "door_refs": ["G1", "G2"] }
                ],
                "edges": [
                    { "source": "块B", "target": "块A", "type": "assumes", "tier": "know", "note": "" },
                    { "source": "块C", "target": "块B", "type": "assumes", "tier": "apply", "note": "" },
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
            false,
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
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), None, &[], None, false);
        assert!(view.structure.is_none());
        assert!(view.terrain.is_none());
        assert!(view.generated_at.is_none());
    }

    /// 地形投影：门口列表 + 基础声明回执；空门口 = None（未生成地形）。
    #[test]
    fn terrain_view_carries_doors() {
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&sample_envelope()), &[], None, false);
        let terrain = view.terrain.expect("sample has terrain");
        assert_eq!(terrain.doors.len(), 2);
        assert_eq!(terrain.doors[0].id, "G1");
        assert_eq!(terrain.learner_basis_assumed, "纯零基础（未声明）");

        let doorless = json!({ "version": 1, "structure": {}, "terrain": { "doors": [] } });
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&doorless), &[], None, false);
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
            false,
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
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&legacy), &[], None, false);
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
                    { "name": "X", "slug": "x", "spine": "s", "stage": "basic", "note": "" },
                    { "name": "Y", "slug": "y", "spine": "s", "stage": "basic", "note": "" }
                ],
                "edges": [
                    { "source": "X", "target": "Y", "type": "assumes", "tier": "know" },
                    { "source": "Y", "target": "X", "type": "assumes", "tier": "know" }
                ]
            }
        });
        let view = build_structure_view(nomifun_common::LearningCourseId::new(), Some(&envelope), &[], None, false);
        let structure = view.structure.expect("structure present");
        assert!(structure.blocks.iter().all(|block| block.layer == 0), "环冻结在 0 层");
    }

    // ── sb1（ADR-0018 决定 2）：地形清单 ────────────────────────────────────

    /// 样例结构的全量行清单（呈现序：档位 / 前置链深 / 主干序 / 块序）。
    const FULL_TERRAIN: &str = "- 块A(block-a)|n|basic|主干一|无前置\n\
         - 块D(block-d)|n|basic|主干二|无前置\n\
         - 块B(block-b)|n|advanced|主干一|前置:块A\n\
         - 块C(block-c)|n|expert|主干二|前置:块B";
    const NO_MOUNTS_LINE: &str = "尚无节点挂载记录，以下为完整地形";
    const STALE_MOUNTS_LINE: &str = "结构已更新，节点与结构的对应待重新检查，以下为完整地形";

    fn sample_structure() -> Value {
        sample_envelope()["structure"].clone()
    }

    fn coverage(pairs: &[(&str, usize)]) -> HashMap<String, usize> {
        pairs
            .iter()
            .map(|(slug, count)| ((*slug).to_owned(), *count))
            .collect()
    }

    /// 验收②：覆盖集为空 → 清单主体 == 全量行清单，逐字节等于「旧输出」
    /// （这串字面量在施工前先对全量摊开的旧渲染 `render_reference_lines`
    /// 跑绿，是它的留证），前面另加说明行；挂载行 fresh 而覆盖为零时没有
    /// 说明行；无块 = 空串（整块省略）。
    #[test]
    fn terrain_list_without_coverage_is_the_full_terrain() {
        let structure = sample_structure();
        let empty = HashMap::new();
        assert_eq!(
            render_terrain_list(&structure, &empty, Some(TerrainListNote::NoMounts)),
            format!("{NO_MOUNTS_LINE}\n{FULL_TERRAIN}")
        );
        assert_eq!(
            render_terrain_list(&structure, &empty, Some(TerrainListNote::StaleMounts)),
            format!("{STALE_MOUNTS_LINE}\n{FULL_TERRAIN}")
        );
        assert_eq!(render_terrain_list(&structure, &empty, None), FULL_TERRAIN);
        for note in [None, Some(TerrainListNote::NoMounts), Some(TerrainListNote::StaleMounts)] {
            assert_eq!(render_terrain_list(&json!({}), &empty, note), "", "无块 = 空串");
        }
    }

    /// 行格式与呈现序的特性测试（地形清单第一次把这套行格式钉住）：
    /// `- 名称(slug)|主题一句|档位|主干|前置`；主题空白取 `—`；前置按边序
    /// 顿号连接；未知档位排最后。
    #[test]
    fn terrain_rows_pin_the_format_and_presentation_order() {
        let multi = json!({
            "spines": [{ "name": "s", "definition": "d", "key_points": [], "instance": "i" }],
            "blocks": [
                { "name": "X", "slug": "x", "spine": "s", "stage": "", "note": "  " },
                { "name": "Y", "slug": "y", "spine": "s", "stage": "basic", "note": "y 的主题" },
                { "name": "Z", "slug": "z", "spine": "s", "stage": "advanced", "note": "" }
            ],
            "edges": [
                { "source": "X", "target": "Y", "type": "assumes" },
                { "source": "X", "target": "Z", "type": "assumes" }
            ]
        });
        assert_eq!(
            render_terrain_list(&multi, &HashMap::new(), None),
            "- Y(y)|y 的主题|basic|s|无前置\n\
             - Z(z)|—|advanced|s|无前置\n\
             - X(x)|—||s|前置:Y、Z",
            "主题空白取破折号、多前置按边序顿号连接、未知档位排最后"
        );
    }

    /// 验收③：`covered` 含 k 块 → 展开行数 = 总块数 − k、呈现序不变（展开
    /// 行是全量序列的子序列）、折叠行的名称与节点数正确、名称顺序确定
    /// （同呈现序，不随 `covered` 的插入序走）。计数为 0 或 slug 已不在
    /// 结构里（悬挂）的条目不算已覆盖。
    #[test]
    fn terrain_list_folds_covered_blocks_into_one_line() {
        let structure = sample_structure();
        let full_rows: Vec<&str> = FULL_TERRAIN.split('\n').collect();
        // (覆盖集, 期望折叠行, 期望仍展开的块在 FULL_TERRAIN 里的下标)
        let cases: Vec<(HashMap<String, usize>, &str, &[usize])> = vec![
            (
                // 插入序故意打乱——折叠行仍按呈现序列名（A、D、C）。
                coverage(&[("block-c", 1), ("block-a", 2), ("block-d", 4)]),
                "已覆盖 3 块：块A、块D、块C",
                &[2],
            ),
            (
                coverage(&[("block-c", 1), ("block-a", 2)]),
                "已覆盖 2 块：块A、块C",
                &[1, 2],
            ),
            (
                coverage(&[("block-b", 3)]),
                "已覆盖 1 块：块B",
                &[0, 1, 3],
            ),
            (
                coverage(&[("block-a", 1), ("block-b", 1), ("block-c", 1), ("block-d", 1)]),
                "已覆盖 4 块：块A、块D、块B、块C",
                &[],
            ),
        ];
        for (covered, fold_line, expanded_at) in cases {
            let list = render_terrain_list(&structure, &covered, None);
            let lines: Vec<&str> = list.split('\n').collect();
            assert_eq!(lines[0], fold_line, "{list}");
            let expanded = &lines[1..];
            let k = covered.len();
            assert_eq!(
                expanded.len(),
                full_rows.len() - k,
                "展开行数 = 总块数 − {k}：{list}"
            );
            let expected: Vec<&str> = expanded_at.iter().map(|&at| full_rows[at]).collect();
            assert_eq!(expanded, expected.as_slice(), "呈现序不变：{list}");
        }
        // 计数为 0、slug 已不在结构里：都不算已覆盖——清单 = 完整地形，无折叠行。
        let bogus = coverage(&[("block-a", 0), ("block-gone", 5)]);
        assert_eq!(render_terrain_list(&structure, &bogus, None), FULL_TERRAIN);
    }

    // ── 覆盖读数（裁决 2026-11-24：两种失效都对齐 ADR，宁可多展开）──────────

    fn nodes(titles: &[&str]) -> HashSet<String> {
        titles.iter().map(|title| (*title).to_owned()).collect()
    }

    /// 挂载行按现行 v2 口径写入时的 (mounts_json, hash, version)：与挂载回合
    /// 落盘同一算法（[`super::super::mounting::hash_structure_column`]）。
    fn mount_row<'a>(raw: &str, mounts_json: &'a str) -> (&'a str, i64, i64) {
        let (hash, version) = super::super::mounting::hash_structure_column(raw);
        (mounts_json, hash as i64, version)
    }

    /// 无挂载行：空覆盖集 + 「尚无节点挂载记录」说明。
    #[test]
    fn coverage_without_a_mount_row_is_empty_and_said_so() {
        let raw = serde_json::to_string(&sample_envelope()).unwrap();
        let read = terrain_coverage(&sample_structure(), &raw, &nodes(&[]), None);
        assert!(read.covered.is_empty(), "{:?}", read.covered);
        assert_eq!(read.note, Some(TerrainListNote::NoMounts));
    }

    /// 挂载行 stale（结构变过、块 slug 都还在）：整表按未覆盖 + stale 说明
    /// ——结构刚被修补过，被改的块最该重新出现在教练面前。同一行在结构
    /// 未变时是 fresh 且照常折叠；v1 行（对整列原样字节求 hash）同样判
    /// fresh——stale 判定的入参必须是整列 `structure_json` 的原始字符串。
    #[test]
    fn coverage_treats_a_stale_mount_row_as_uncovered() {
        let before = sample_envelope();
        let raw_before = serde_json::to_string(&before).unwrap();
        let nodes = nodes(&["节点一"]);
        let json = r#"[{"node":"节点一","blocks":["block-a"],"reason":"r"}]"#;
        let row = mount_row(&raw_before, json);

        let fresh = terrain_coverage(&before["structure"], &raw_before, &nodes, Some(row));
        assert_eq!(fresh.note, None);
        assert_eq!(fresh.covered, coverage(&[("block-a", 1)]));

        let v1 = (
            json,
            super::super::mounting::fnv1a_64(raw_before.as_bytes()) as i64,
            super::super::mounting::STRUCTURE_HASH_VERSION_V1,
        );
        let fresh_v1 = terrain_coverage(&before["structure"], &raw_before, &nodes, Some(v1));
        assert_eq!(fresh_v1.note, None, "v1 行：结构未变 = fresh");
        assert_eq!(fresh_v1.covered, coverage(&[("block-a", 1)]));

        // 只改一个块的主题——slug 全部仍在，行照样 stale → 空覆盖集。
        let mut after = before.clone();
        after["structure"]["blocks"][1]["note"] = json!("修补过的主题");
        let raw_after = serde_json::to_string(&after).unwrap();
        let stale = terrain_coverage(&after["structure"], &raw_after, &nodes, Some(row));
        assert!(stale.covered.is_empty(), "{:?}", stale.covered);
        assert_eq!(stale.note, Some(TerrainListNote::StaleMounts));
    }

    /// 行 fresh：只数「活节点 × 结构内 slug」。孤儿条目（节点已删，下一轮
    /// 挂载才清出）不计；条目里的悬挂 slug 不计，但不牵连同条目其余仍有效
    /// 的 slug——与 `merge_mounts` 的「任一 slug 失效即孤儿」不同，地形清单
    /// 只关心「这个块有没有活着的节点」。
    #[test]
    fn coverage_counts_live_nodes_on_slugs_still_in_the_structure() {
        let envelope = sample_envelope();
        let raw = serde_json::to_string(&envelope).unwrap();
        let mounts_json = serde_json::json!([
            { "node": "节点一", "blocks": ["block-a"], "reason": "r" },
            { "node": "节点二", "blocks": ["block-a", "block-gone"], "reason": "跨块" },
            { "node": "已删节点", "blocks": ["block-b"], "reason": "孤儿" },
            { "node": "节点三", "blocks": [], "reason": "结构外" },
            { "node": "节点四", "blocks": ["block-d", "block-d"], "reason": "同一 slug 重复只计一次" }
        ])
        .to_string();
        let row = mount_row(&raw, &mounts_json);
        let nodes = nodes(&["节点一", "节点二", "节点三", "节点四"]);

        let read = terrain_coverage(&envelope["structure"], &raw, &nodes, Some(row));
        assert_eq!(read.note, None);
        assert_eq!(
            read.covered,
            coverage(&[("block-a", 2), ("block-d", 1)]),
            "块A 两个活节点；块D 一个；块B 只有孤儿条目 = 未覆盖；悬挂 slug block-gone 不计"
        );
    }

    // ── Stage 1c：首批参考面减去已会声明块 ────────────────────────────────

    #[test]
    fn terrain_list_first_batch_subtracts_declared_blocks_without_cascade() {
        let structure = sample_structure();
        let empty = HashMap::new();
        let declared: BTreeSet<String> = ["block-a".to_string()].into_iter().collect();
        let list = render_terrain_list_with_declared(&structure, &empty, None, &declared, true);
        assert!(list.contains("已会声明块（本批不装）：块A"), "{list}");
        assert!(!list.contains("- 块A(block-a)"), "声明块行不出现：{list}");
        assert!(
            list.contains("- 块B(block-b)|n|advanced|主干一|前置:块A"),
            "未声明块保留、结构标注不因前置被声明而级联裁剪：{list}"
        );
        assert!(list.contains("- 块C(block-c)"), "{list}");
        assert!(list.contains("- 块D(block-d)"), "{list}");
    }

    #[test]
    fn terrain_list_declared_filter_is_first_batch_only_and_empty_set_is_byte_identical() {
        let structure = sample_structure();
        let empty = HashMap::new();
        let declared: BTreeSet<String> =
            ["block-a".to_string(), "block-c".to_string()].into_iter().collect();
        assert_eq!(
            render_terrain_list_with_declared(&structure, &empty, None, &declared, false),
            FULL_TERRAIN,
            "非首批不筛"
        );
        let none: BTreeSet<String> = BTreeSet::new();
        assert_eq!(
            render_terrain_list_with_declared(&structure, &empty, None, &none, true),
            FULL_TERRAIN,
            "首批空声明集逐字节同旧"
        );
        let list = render_terrain_list_with_declared(&structure, &empty, None, &declared, true);
        assert!(list.contains("已会声明块（本批不装）：块A、块C"), "{list}");
        assert!(!list.contains("- 块A(block-a)"), "{list}");
        assert!(!list.contains("- 块C(block-c)"), "{list}");
        assert!(list.contains("- 块B(block-b)"), "{list}");
        assert!(list.contains("- 块D(block-d)"), "{list}");
    }

    #[test]
    fn terrain_list_declared_filter_never_removes_slugless_blocks() {
        let structure = json!({
            "spines": [{ "name": "s", "definition": "d", "key_points": [], "instance": "i" }],
            "blocks": [
                { "name": "无slug块", "spine": "s", "stage": "basic", "note": "n" },
                { "name": "声明块X", "slug": "x", "spine": "s", "stage": "basic", "note": "n" }
            ]
        });
        let declared: BTreeSet<String> =
            ["x".to_string(), String::new()].into_iter().collect();
        let list =
            render_terrain_list_with_declared(&structure, &HashMap::new(), None, &declared, true);
        assert!(list.contains("无slug块"), "{list}");
        assert!(!list.contains("- 声明块X("), "{list}");
    }
}
