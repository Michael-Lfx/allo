//! 七型结构修补 ops（j09，三轮 g1「结构转活文档·下」）：模型只表达意图，
//! 纯函数引擎管自洽——「ops = 意图，引擎管自洽」（learnhub
//! engine/maintenance/ops.rs 口径）。零 LLM 零 IO 零时钟；依序应用，任一
//! op 无法应用 = 整体失败（不降级不跳过）；拆/并/改名的引用面级联引擎代
//! 管（改名跟改、拆块边退场、并块去重去自环 + 级联注记记数）。
//!
//! 块引用 = 块名逐字（编辑面看现状）；slug 只出现在新块定义且必须新发
//! （no_slug_churn：不得复用当前或已退场 slug）。应用后 gd.* 全量重过零
//! 放宽（st.ops.apply 执法在门面）。

use std::collections::HashSet;

use serde_json::Value;

use nomifun_common::AppError;

use crate::completer::LearningCompleter;

use super::structure_contract::{self, ContractViolation, EDGE_TIERS, EDGE_TYPES};

// ── st.ops.* 五条款（单源：validator / 契约文档新章 / 提示词互钉）──────────

pub const ST_OPS_SHAPE: &str = "st.ops.shape";
pub const ST_OPS_ENDPOINTS: &str = "st.ops.endpoints";
pub const ST_OPS_REASON: &str = "st.ops.reason";
pub const ST_OPS_APPLY: &str = "st.ops.apply";
pub const ST_OPS_NO_SLUG_CHURN: &str = "st.ops.no_slug_churn";

pub const OP_KINDS: &[&str] = &[
    "add_edge",
    "remove_edge",
    "flip_edge",
    "add_block",
    "split_block",
    "rename_block",
    "merge_blocks",
];

// ── op 模型 ───────────────────────────────────────────────────────────────

/// 一条结构修补意图（引擎依序应用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructureOp {
    AddEdge {
        source: String,
        target: String,
        edge_type: String,
        tier: String,
        note: String,
        reason: String,
    },
    /// 三元组唯一命中即删，不命中 = 引擎整体失败。
    RemoveEdge {
        source: String,
        target: String,
        edge_type: String,
        reason: String,
    },
    /// 方向纠正：端点对调，type/tier/note 保持。
    FlipEdge {
        source: String,
        target: String,
        edge_type: String,
        reason: String,
    },
    AddBlock {
        name: String,
        slug: String,
        spine: String,
        stage: String,
        note: String,
        reason: String,
    },
    /// 拆块：旧块退场（随块边一并退场，需要的关系由加边补回），新块进场。
    SplitBlock {
        target: String,
        name: String,
        slug: String,
        stage: String,
        note: String,
        reason: String,
    },
    /// 只动块名（slug 闭禁——rename 不携带 slug 键）；引用旧名的边跟改。
    RenameBlock {
        from: String,
        to: String,
        reason: String,
    },
    /// 合并：源块退场，端点改指新块 + 同三元组去重 + 自环退场。
    MergeBlocks {
        sources: Vec<String>,
        name: String,
        slug: String,
        stage: String,
        note: String,
        reason: String,
    },
}

impl StructureOp {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::AddEdge { .. } => "add_edge",
            Self::RemoveEdge { .. } => "remove_edge",
            Self::FlipEdge { .. } => "flip_edge",
            Self::AddBlock { .. } => "add_block",
            Self::SplitBlock { .. } => "split_block",
            Self::RenameBlock { .. } => "rename_block",
            Self::MergeBlocks { .. } => "merge_blocks",
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            Self::AddEdge { reason, .. }
            | Self::RemoveEdge { reason, .. }
            | Self::FlipEdge { reason, .. }
            | Self::AddBlock { reason, .. }
            | Self::SplitBlock { reason, .. }
            | Self::RenameBlock { reason, .. }
            | Self::MergeBlocks { reason, .. } => reason,
        }
    }

    /// 人面标签（审计行与摘要的 applied[] 用）。
    pub fn label(&self) -> String {
        match self {
            Self::AddEdge { source, target, edge_type, .. } => {
                format!("add_edge {source} -[{edge_type}]-> {target}")
            }
            Self::RemoveEdge { source, target, edge_type, .. } => {
                format!("remove_edge {source} -[{edge_type}]-> {target}")
            }
            Self::FlipEdge { source, target, edge_type, .. } => {
                format!("flip_edge {source} -[{edge_type}]-> {target}")
            }
            Self::AddBlock { name, slug, .. } => format!("add_block {name} ({slug})"),
            Self::SplitBlock { target, name, .. } => {
                format!("split_block {target} -> {name}")
            }
            Self::RenameBlock { from, to, .. } => format!("rename_block {from} -> {to}"),
            Self::MergeBlocks { sources, name, .. } => {
                format!("merge_blocks {} -> {name}", sources.join(" + "))
            }
        }
    }

    fn from_json(item: &Value) -> Result<Self, String> {
        let at = "ops[]";
        let kind = item
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{at} 缺 op 类型"))?;
        let reason = item
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let text = |key: &str| {
            item.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{at}（{kind}）缺 {key} 或为空"))
        };
        match kind {
            "add_edge" => Ok(Self::AddEdge {
                source: text("source")?,
                target: text("target")?,
                edge_type: text("type")?,
                tier: text("tier")?,
                note: item
                    .get("note")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                reason,
            }),
            "remove_edge" => Ok(Self::RemoveEdge {
                source: text("source")?,
                target: text("target")?,
                edge_type: text("type")?,
                reason,
            }),
            "flip_edge" => Ok(Self::FlipEdge {
                source: text("source")?,
                target: text("target")?,
                edge_type: text("type")?,
                reason,
            }),
            "add_block" => Ok(Self::AddBlock {
                name: text("name")?,
                slug: text("slug")?,
                spine: text("spine")?,
                stage: text("stage")?,
                note: text("note")?,
                reason,
            }),
            "split_block" => Ok(Self::SplitBlock {
                target: text("target")?,
                name: text("name")?,
                slug: text("slug")?,
                stage: text("stage")?,
                note: text("note")?,
                reason,
            }),
            "rename_block" => Ok(Self::RenameBlock {
                from: text("from")?,
                to: text("to")?,
                reason,
            }),
            "merge_blocks" => {
                let sources: Vec<String> = item
                    .get("sources")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("{at}（merge_blocks）缺 sources 数组"))?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty())
                    .collect();
                if sources.is_empty() {
                    return Err(format!("{at}（merge_blocks）sources 为空"));
                }
                Ok(Self::MergeBlocks {
                    sources,
                    name: text("name")?,
                    slug: text("slug")?,
                    stage: text("stage")?,
                    note: text("note")?,
                    reason,
                })
            }
            other => Err(format!(
                "{at} 未知 op 类型「{other}」——七型：{}",
                OP_KINDS.join("/")
            )),
        }
    }
}

/// 解析修补产物（严格形态：解析失败返回错误清单——生成面红条）。
pub fn parse_ops(artifact: &Value) -> Result<Vec<StructureOp>, Vec<String>> {
    let Some(items) = artifact.get("ops").and_then(Value::as_array) else {
        return Err(vec!["修补产物缺 ops 数组（ops 可空但键必须在）".to_owned()]);
    };
    let mut ops = Vec::new();
    let mut errors = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match StructureOp::from_json(item) {
            Ok(op) => ops.push(op),
            Err(error) => errors.push(format!("ops[{index}]: {error}")),
        }
    }
    if errors.is_empty() {
        Ok(ops)
    } else {
        Err(errors)
    }
}

// ── st.ops.* 验证器（应用序模拟；st.ops.apply 执法在门面）─────────────────

/// 验证一份修补产物（应用前的静态面）：shape / reason / endpoints（应用序
/// 模拟——前序块 ops 即时进端点域）/ no_slug_churn。全红。
pub fn validate_ops(artifact: &Value, structure: &Value) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    let red = |code: &'static str, message: String| ContractViolation {
        code,
        message,
        warning: false,
    };
    if !artifact.is_object() || artifact.get("ops").is_none() {
        return vec![red(
            ST_OPS_SHAPE,
            "修补产物必须是单个 JSON 对象且带 ops 键（ops 可为空数组）".into(),
        )];
    }
    let ops = match parse_ops(artifact) {
        Ok(ops) => ops,
        Err(errors) => {
            return errors
                .into_iter()
                .map(|message| red(ST_OPS_SHAPE, message))
                .collect();
        }
    };
    let raw_items = artifact
        .get("ops")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // 应用序模拟：块名集与 slug 集（含退场）随 op 推进。
    let mut names: HashSet<String> = structure
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block.get("name").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut slugs: HashSet<String> = structure
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block.get("slug").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut retired_slugs: HashSet<String> = HashSet::new();
    for (index, op) in ops.iter().enumerate() {
        let at = format!("ops[{index}]({})", op.kind());
        if op.reason().trim().is_empty() {
            violations.push(red(
                ST_OPS_REASON,
                format!("{at} 缺 reason——每个操作一条依据，输入里没有依据的编辑不做"),
            ));
        }
        // 端点域：边 ops 的两端必须在该 op 时刻的块名集里。
        let (endpoints, requires_target_fresh, new_slug, new_name) = match op {
            StructureOp::AddEdge { source, target, .. }
            | StructureOp::RemoveEdge { source, target, .. }
            | StructureOp::FlipEdge { source, target, .. } => {
                (vec![source.clone(), target.clone()], false, None, None)
            }
            StructureOp::AddBlock { name, slug, .. } => {
                (Vec::new(), true, Some(slug.clone()), Some(name.clone()))
            }
            StructureOp::SplitBlock { target, name, slug, .. } => {
                (vec![target.clone()], true, Some(slug.clone()), Some(name.clone()))
            }
            StructureOp::RenameBlock { from, to, .. } => {
                // to 是本 op 引入的新名，不算端点引用（应用后进名集）。
                (vec![from.clone()], false, None, Some(to.clone()))
            }
            StructureOp::MergeBlocks { sources, name, slug, .. } => {
                (sources.clone(), true, Some(slug.clone()), Some(name.clone()))
            }
        };
        for endpoint in &endpoints {
            if !names.contains(endpoint) {
                violations.push(red(
                    ST_OPS_ENDPOINTS,
                    format!(
                        "{at} 引用块「{endpoint}」不在该 op 时刻的块表里——块名逐字，且只能引用前序 op 之后真实存在的块"
                    ),
                ));
            }
        }
        match op {
            StructureOp::RenameBlock { .. } => {
                // no_slug_churn：rename 键闭禁 slug（编辑面只动名字；解析面
                // 忽略多余键，这里回查 raw 键点名）。
                if let Some(raw) = raw_items.get(index) {
                    if raw.get("slug").is_some() {
                        violations.push(red(
                            ST_OPS_NO_SLUG_CHURN,
                            format!("{at} 携带 slug 键——rename 只动块名，slug 终身不变"),
                        ));
                    }
                }
            }
            StructureOp::AddEdge { edge_type, tier, .. } => {
                if !EDGE_TYPES.contains(&edge_type.as_str()) {
                    violations.push(red(
                        ST_OPS_SHAPE,
                        format!("{at} 的 type 必须是 {} 之一", EDGE_TYPES.join("/")),
                    ));
                }
                if !EDGE_TIERS.contains(&tier.as_str()) {
                    violations.push(red(
                        ST_OPS_SHAPE,
                        format!("{at} 的 tier 必须是 {} 之一", EDGE_TIERS.join("/")),
                    ));
                }
            }
            StructureOp::RemoveEdge { edge_type, .. } | StructureOp::FlipEdge { edge_type, .. } => {
                if !EDGE_TYPES.contains(&edge_type.as_str()) {
                    violations.push(red(
                        ST_OPS_SHAPE,
                        format!("{at} 的 type 必须是 {} 之一", EDGE_TYPES.join("/")),
                    ));
                }
            }
            _ => {}
        }
        if let Some(slug) = &new_slug {
            if requires_target_fresh && (slugs.contains(slug) || retired_slugs.contains(slug)) {
                violations.push(red(
                    ST_OPS_NO_SLUG_CHURN,
                    format!("{at} 的新 slug「{slug}」复用了当前或已退场的 slug——新块必须新发 slug"),
                ));
            }
            slugs.insert(slug.clone());
        }
        if let Some(name) = &new_name {
            names.insert(name.clone());
        }
        // 推进模拟面：rename 换名、split/merge 退场旧名。
        match op {
            StructureOp::RenameBlock { from, to, .. } => {
                names.remove(from);
                retired_slugs.extend(retire_slug_of(structure, from));
            }
            StructureOp::SplitBlock { target, .. } => {
                names.remove(target);
                retired_slugs.extend(retire_slug_of(structure, target));
            }
            StructureOp::MergeBlocks { sources, .. } => {
                for source in sources {
                    names.remove(source);
                    retired_slugs.extend(retire_slug_of(structure, source));
                }
            }
            _ => {}
        }
    }
    violations
}

/// 退场块的 slug 回查（模拟面用；查不到 = 引用面已不精确，退场集不扩充）。
fn retire_slug_of(structure: &Value, name: &str) -> Option<String> {
    structure
        .get("blocks")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|block| {
            let block_name = block.get("name")?.as_str()?;
            let slug = block.get("slug")?.as_str()?;
            (block_name == name).then(|| slug.to_owned())
        })
}

// ── 应用引擎（纯函数；任一 op 无法应用 = 整体失败）────────────────────────

/// 依序应用 ops 到结构对象（原地变形 blocks/edges），返回 (新结构, 级联注
/// 记)。应用后调用方必须跑 gd.* 全量重过（st.ops.apply）。
pub fn apply_ops(structure: &mut Value, ops: &[StructureOp]) -> Result<Vec<String>, String> {
    let mut cascade: Vec<String> = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        let at = format!("ops[{index}]({})", op.kind());
        match op {
            StructureOp::AddEdge { source, target, edge_type, tier, note, .. } => {
                let blocks = structure_blocks_mut(structure)?;
                if !blocks.iter().any(|block| block["name"] == **source)
                    || !blocks.iter().any(|block| block["name"] == **target)
                {
                    return Err(format!("{at} 端点块不存在"));
                }
                let edges = structure_edges_mut(structure)?;
                if edges.iter().any(|edge| {
                    edge["source"] == **source
                        && edge["target"] == **target
                        && edge["type"] == **edge_type
                }) {
                    return Err(format!("{at} 同三元组边已存在"));
                }
                edges.push(serde_json::json!({
                    "source": source, "target": target, "type": edge_type,
                    "tier": tier, "note": note,
                }));
            }
            StructureOp::RemoveEdge { source, target, edge_type, .. } => {
                let edges = structure_edges_mut(structure)?;
                let mut hits = edges.iter().enumerate().filter(|(_, edge)| {
                    edge["source"] == **source
                        && edge["target"] == **target
                        && edge["type"] == **edge_type
                });
                let Some((position, _)) = hits.next() else {
                    return Err(format!(
                        "{at} 三元组 ({source}, {target}, {edge_type}) 无命中——remove_edge 只删唯一命中"
                    ));
                };
                if hits.next().is_some() {
                    return Err(format!("{at} 三元组命中多条——数据不自洽，拒绝盲删"));
                }
                edges.remove(position);
            }
            StructureOp::FlipEdge { source, target, edge_type, .. } => {
                let edges = structure_edges_mut(structure)?;
                let Some(edge) = edges.iter_mut().find(|edge| {
                    edge["source"] == **source
                        && edge["target"] == **target
                        && edge["type"] == **edge_type
                }) else {
                    return Err(format!("{at} 三元组无命中"));
                };
                let from = edge["source"].clone();
                let to = edge["target"].clone();
                edge["source"] = to;
                edge["target"] = from;
            }
            StructureOp::AddBlock { name, slug, spine, stage, note, .. } => {
                let blocks = structure_blocks_mut(structure)?;
                if blocks.iter().any(|block| block["name"] == **name) {
                    return Err(format!("{at} 块名「{name}」已存在"));
                }
                blocks.push(serde_json::json!({
                    "name": name, "slug": slug, "spine": spine,
                    "stage": stage, "note": note,
                }));
            }
            StructureOp::SplitBlock { target, name, slug, stage, note, .. } => {
                let dropped = count_edges_touching(structure, target);
                // 随块边退场先于块删（两者借用面不相交），再补新块。
                {
                    let edges = structure_edges_mut(structure)?;
                    edges.retain(|edge| edge["source"] != **target && edge["target"] != **target);
                }
                let blocks = structure_blocks_mut(structure)?;
                let Some(position) = blocks
                    .iter()
                    .position(|block| block["name"] == **target)
                else {
                    return Err(format!("{at} 目标块「{target}」不存在"));
                };
                if blocks.iter().any(|block| block["name"] == **name) {
                    return Err(format!("{at} 新块名「{name}」已存在"));
                }
                let spine = blocks[position]["spine"].clone();
                blocks.remove(position);
                cascade.push(format!(
                    "拆块「{target}」退场，随块边 {dropped} 条退场（需要的关系由加边补回）"
                ));
                blocks.push(serde_json::json!({
                    "name": name, "slug": slug, "spine": spine,
                    "stage": stage, "note": note,
                }));
            }
            StructureOp::RenameBlock { from, to, .. } => {
                let blocks = structure_blocks_mut(structure)?;
                if !blocks.iter().any(|block| block["name"] == **from) {
                    return Err(format!("{at} 块「{from}」不存在"));
                }
                if blocks.iter().any(|block| block["name"] == **to) {
                    return Err(format!("{at} 新名「{to}」已存在"));
                }
                if let Some(block) = blocks
                    .iter_mut()
                    .find(|block| block["name"] == **from)
                {
                    block["name"] = (**to).into();
                }
                let edges = structure_edges_mut(structure)?;
                let mut followed = 0usize;
                for edge in edges.iter_mut() {
                    if edge["source"] == **from {
                        edge["source"] = (**to).into();
                        followed += 1;
                    }
                    if edge["target"] == **from {
                        edge["target"] = (**to).into();
                        followed += 1;
                    }
                }
                cascade.push(format!("改名「{from}」→「{to}」，{followed} 条边端点跟改"));
            }
            StructureOp::MergeBlocks { sources, name, slug, stage, note, .. } => {
                // 校验（immutable 面）。
                {
                    let blocks = structure_blocks(structure)?;
                    for source in sources {
                        if !blocks.iter().any(|block| block["name"] == **source) {
                            return Err(format!("{at} 源块「{source}」不存在"));
                        }
                    }
                    if blocks.iter().any(|block| block["name"] == **name) {
                        return Err(format!("{at} 新块名「{name}」已存在"));
                    }
                    if sources.iter().any(|source| source == name) {
                        return Err(format!("{at} 新块名不得与源块同名"));
                    }
                }
                let spine = structure_blocks(structure)?
                    .iter()
                    .find(|block| block["name"] == sources[0].as_str())
                    .map(|block| block["spine"].clone())
                    .unwrap_or_else(|| "未分组".into());
                // 边重写先于块删（两者借用面不相交）。
                {
                    let edges = structure_edges_mut(structure)?;
                    let mut kept: Vec<Value> = Vec::new();
                    let mut deduped = 0usize;
                    let mut self_loops = 0usize;
                    for mut edge in edges.drain(..) {
                        let mut source = edge["source"].as_str().unwrap_or_default().to_owned();
                        let mut target = edge["target"].as_str().unwrap_or_default().to_owned();
                        if sources.iter().any(|s| s == &source) {
                            source = name.clone();
                        }
                        if sources.iter().any(|s| s == &target) {
                            target = name.clone();
                        }
                        if source == target {
                            self_loops += 1;
                            continue;
                        }
                        let triple = (
                            source.clone(),
                            target.clone(),
                            edge["type"].as_str().unwrap_or_default().to_owned(),
                        );
                        if kept.iter().any(|kept_edge| {
                            (
                                kept_edge["source"].as_str().unwrap_or_default(),
                                kept_edge["target"].as_str().unwrap_or_default(),
                                kept_edge["type"].as_str().unwrap_or_default(),
                            ) == (triple.0.as_str(), triple.1.as_str(), triple.2.as_str())
                        }) {
                            deduped += 1;
                            continue;
                        }
                        edge["source"] = source.into();
                        edge["target"] = target.into();
                        kept.push(edge);
                    }
                    cascade.push(format!(
                        "合并 {} → 「{name}」：端点改指新块、同三元组去重 {deduped} 条、自环退场 {self_loops} 条",
                        sources.join(" + ")
                    ));
                    *structure_edges_mut(structure)? = kept;
                }
                let blocks = structure_blocks_mut(structure)?;
                for source in sources {
                    let position = blocks
                        .iter()
                        .position(|block| block["name"] == **source)
                        .expect("source existence checked above");
                    blocks.remove(position);
                }
                blocks.push(serde_json::json!({
                    "name": name, "slug": slug, "spine": spine,
                    "stage": stage, "note": note,
                }));
            }
        }
    }
    Ok(cascade)
}

fn structure_blocks_mut(structure: &mut Value) -> Result<&mut Vec<Value>, String> {
    structure
        .get_mut("blocks")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "结构快照缺 blocks 数组".to_owned())
}

fn structure_blocks(structure: &Value) -> Result<&Vec<Value>, String> {
    structure
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or_else(|| "结构快照缺 blocks 数组".to_owned())
}

// ── 修补回合（单 LLM 调用 + st.ops 门 + RoundRetry）──────────────────────

/// 会话级系统行（与结构生成同基调；任务纪律住提示词资产）。
const REPAIR_SYSTEM: &str =
    "你是学习课程的结构修补引擎：严格按任务说明输出一个 JSON 对象，不输出任何其他字符。";

/// 修补回合输入（七件）。
pub(crate) struct RepairRoundInput<'a> {
    pub goal: &'a str,
    pub proficiency_statement: &'a str,
    pub learner_basis: &'a str,
    pub structure_snapshot: &'a str,
    pub findings: &'a str,
    pub mount_observations: &'a str,
}

/// 修补回合：装配 STRUCTURE_REPAIR 提示词（temperature 0.0 + thinking）→
/// json 接缝 → st.ops 静态门；违规报告原文注入重试，≤ MAX_ATTEMPTS 轮，同
/// 指纹早停（j03 骨架复用）；**ops 空 = 合法结论**（返回空 Vec，调用方按
/// noop 处理——LLM 已跑、不写盘）。真取消：令牌循环头检查 + select! 竞速。
pub(crate) async fn run_repair_round(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    input: &RepairRoundInput<'_>,
    structure: &Value,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<Vec<StructureOp>, AppError> {
    let cancellation =
        cancel.map(|token| crate::service::CancellationCompleter::new(completer, token.clone()));
    let completer: &dyn LearningCompleter = match &cancellation {
        Some(wrapper) => wrapper,
        None => completer,
    };
    let mut feedback = "(无:首次)".to_owned();
    let mut last = String::from("(未执行)");
    let mut last_was_llm = false;
    let mut prev_fingerprint: Option<String> = None;
    let max_attempts = super::structure_gen::MAX_ATTEMPTS;
    for attempt in 1..=max_attempts {
        if cancel.is_some_and(|token| token.is_cancelled()) {
            return Err(AppError::Conflict(
                crate::service::CANCELLED_MESSAGE.to_owned(),
            ));
        }
        let prompt = super::structure_prompts::assemble(
            &super::structure_prompts::STRUCTURE_REPAIR,
            &[
                ("goal", input.goal),
                ("proficiency_statement", input.proficiency_statement),
                ("learner_basis", input.learner_basis),
                ("structure_snapshot", input.structure_snapshot),
                ("findings", input.findings),
                ("mount_observations", input.mount_observations),
                ("retry_feedback", feedback.as_str()),
            ],
        )
        .map_err(|error| AppError::Internal(error.to_string()))?;
        let raw = crate::generation::complete_json_coach(
            completer,
            model_override,
            REPAIR_SYSTEM,
            &prompt.text,
            crate::generation::STRUCTURE_MAX_TOKENS,
            Some(super::structure_prompts::STRUCTURE_REPAIR.temperature),
        )
        .await;
        let raw = match raw {
            Ok(raw) => raw,
            Err(error) => {
                last = error.to_string();
                last_was_llm = true;
                tracing::warn!(attempt, "repair round LLM call failed");
                continue;
            }
        };
        last_was_llm = false;
        let artifact = match crate::generation::parse_json_object::<Value>(&raw) {
            Ok(value) if value.is_object() => value,
            Ok(_) => {
                last = "修补产物必须是单个 JSON 对象".to_owned();
                feedback = format!("上一轮产物违反契约,逐条修正后重新生成:\n{last}");
                continue;
            }
            Err(error) => {
                last = format!("修补产物解析失败（非合法 JSON 对象）：{error}");
                feedback = format!("上一轮产物违反契约,逐条修正后重新生成:\n{last}");
                continue;
            }
        };
        let violations = validate_ops(&artifact, structure);
        let red: Vec<&ContractViolation> =
            violations.iter().filter(|item| !item.warning).collect();
        if red.is_empty() {
            return parse_ops(&artifact).map_err(|errors| {
                AppError::Internal(format!("修补产物解析失败：{}", errors.join("；")))
            });
        }
        let owned: Vec<ContractViolation> = red.into_iter().cloned().collect();
        last = structure_contract::format_violations(&owned);
        let fingerprint = super::growth_retry::normalize_report(&last);
        if attempt > 1 && prev_fingerprint.as_deref() == Some(fingerprint.as_str()) {
            tracing::warn!(
                attempt,
                "repair round produced the same violations twice — stopping early"
            );
            break;
        }
        prev_fingerprint = Some(fingerprint);
        feedback = format!("上一轮产物违反契约,逐条修正后重新生成:\n{last}");
    }
    if last_was_llm {
        return Err(AppError::Conflict(format!(
            "修补回合未执行（生成通道不可用，结构与审计均未改动）：{last}"
        )));
    }
    Err(AppError::UnprocessableEntity(format!(
        "修补回合未通过条款门（逐条款报告见下）；结构与审计均未改动，可重试：\n{last}"
    )))
}

fn structure_edges_mut(structure: &mut Value) -> Result<&mut Vec<Value>, String> {
    let edges = structure
        .get_mut("edges")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "结构快照缺 edges 数组".to_owned())?;
    Ok(edges)
}

fn count_edges_touching(structure: &Value, name: &str) -> usize {
    structure
        .get("edges")
        .and_then(Value::as_array)
        .map(|edges| {
            edges
                .iter()
                .filter(|edge| edge["source"] == *name || edge["target"] == *name)
                .count()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 最小过门结构（两块一边）。
    fn sample_structure() -> Value {
        json!({
            "goal": "g",
            "spines": [{ "name": "主干一", "definition": "d", "key_points": [], "instance": "i" }],
            "blocks": [
                { "name": "块一", "slug": "block-01", "spine": "主干一", "stage": "入门", "note": "n" },
                { "name": "块二", "slug": "block-02", "spine": "主干一", "stage": "进阶", "note": "n" }
            ],
            "edges": [
                { "source": "块二", "target": "块一", "type": "assumes", "tier": "know", "note": "" }
            ]
        })
    }

    fn ops_artifact(ops: Value) -> Value {
        json!({ "ops": ops })
    }

    fn apply_all(ops: Vec<StructureOp>) -> (Value, Vec<String>) {
        let mut structure = sample_structure();
        let cascade = apply_ops(&mut structure, &ops).expect("ops apply");
        (structure, cascade)
    }

    #[test]
    fn add_and_remove_and_flip_edge_roundtrip() {
        let (structure, _) = apply_all(vec![
            StructureOp::AddEdge {
                source: "块一".into(),
                target: "块二".into(),
                edge_type: "contrasts".into(),
                tier: "use".into(),
                note: "对照".into(),
                reason: "真对照".into(),
            },
            StructureOp::RemoveEdge {
                source: "块二".into(),
                target: "块一".into(),
                edge_type: "assumes".into(),
                reason: "方向写反".into(),
            },
        ]);
        let edges = structure["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["type"], json!("contrasts"));

        // flip：端点对调、type/tier 保持。
        let (structure, _) = apply_all(vec![StructureOp::FlipEdge {
            source: "块二".into(),
            target: "块一".into(),
            edge_type: "assumes".into(),
            reason: "方向纠正".into(),
        }]);
        let edge = &structure["edges"][0];
        assert_eq!(edge["source"], json!("块一"));
        assert_eq!(edge["target"], json!("块二"));
        assert_eq!(edge["tier"], json!("know"));
    }

    #[test]
    fn remove_edge_fails_on_no_hit() {
        let mut structure = sample_structure();
        let error = apply_ops(
            &mut structure,
            &[StructureOp::RemoveEdge {
                source: "块一".into(),
                target: "块二".into(),
                edge_type: "assumes".into(),
                reason: "不存在的三元组".into(),
            }],
        )
        .unwrap_err();
        assert!(error.contains("无命中"), "{error}");
    }

    #[test]
    fn rename_follows_edges_and_keeps_slug() {
        let (structure, cascade) = apply_all(vec![StructureOp::RenameBlock {
            from: "块一".into(),
            to: "块壹".into(),
            reason: "一物一名纠偏".into(),
        }]);
        let blocks = structure["blocks"].as_array().unwrap();
        assert_eq!(blocks[0]["name"], json!("块壹"));
        assert_eq!(blocks[0]["slug"], json!("block-01"), "rename 不动 slug");
        assert_eq!(structure["edges"][0]["target"], json!("块壹"));
        assert!(cascade.iter().any(|note| note.contains("1 条边端点跟改")));
    }

    #[test]
    fn split_block_retires_old_and_drops_its_edges() {
        let (structure, cascade) = apply_all(vec![StructureOp::SplitBlock {
            target: "块一".into(),
            name: "块一分".into(),
            slug: "block-new".into(),
            stage: "入门".into(),
            note: "拆出的原子主题".into(),
            reason: "原块承载两个主题".into(),
        }]);
        let names: Vec<&str> = structure["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|block| block["name"].as_str().unwrap())
            .collect();
        assert!(!names.contains(&"块一"), "旧块退场");
        assert!(names.contains(&"块一分"));
        // 随块边退场（指向块一的 assumes 边没了）。
        assert_eq!(structure["edges"].as_array().unwrap().len(), 0);
        assert!(cascade.iter().any(|note| note.contains("随块边 1 条退场")));
    }

    #[test]
    fn merge_blocks_redirects_dedups_and_drops_self_loops() {
        // 三块两边的结构：块三 assumes 块一 + 块二 assumes 块三；合并一二后
        // 块三的 assumes 自环应退场。
        let mut structure = json!({
            "goal": "g",
            "spines": [{ "name": "主干一", "definition": "d", "key_points": [], "instance": "i" }],
            "blocks": [
                { "name": "块一", "slug": "block-01", "spine": "主干一", "stage": "入门", "note": "n" },
                { "name": "块二", "slug": "block-02", "spine": "主干一", "stage": "入门", "note": "n" },
                { "name": "块三", "slug": "block-03", "spine": "主干一", "stage": "进阶", "note": "n" }
            ],
            "edges": [
                { "source": "块二", "target": "块一", "type": "assumes", "tier": "know", "note": "" },
                { "source": "块三", "target": "块一", "type": "assumes", "tier": "know", "note": "" },
                { "source": "块三", "target": "块二", "type": "assumes", "tier": "know", "note": "" }
            ]
        });
        let cascade = apply_ops(
            &mut structure,
            &[StructureOp::MergeBlocks {
                sources: vec!["块一".into(), "块二".into()],
                name: "块甲".into(),
                slug: "block-new".into(),
                stage: "入门".into(),
                note: "合并主题".into(),
                reason: "两块同主题".into(),
            }],
        )
        .unwrap();
        let edges = structure["edges"].as_array().unwrap();
        // 块三→块一 与 块三→块二 都指向新块且同三元组 → 去重剩 1；块二→块一
        // 变自环 → 退场。
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["target"], json!("块甲"));
        assert!(cascade.iter().any(|note| note.contains("自环退场 1 条")));
        assert!(cascade.iter().any(|note| note.contains("去重 1 条")));
    }

    #[test]
    fn st_ops_red_green_samples() {
        // 绿：合法产物零红。
        let green = ops_artifact(json!([
            { "op": "add_edge", "source": "块一", "target": "块二", "type": "contrasts",
              "tier": "use", "note": "", "reason": "真对照" },
            { "op": "rename_block", "from": "块二", "to": "块贰", "reason": "纠偏" }
        ]));
        assert!(validate_ops(&green, &sample_structure()).is_empty());

        // 红：逐条款各有命中。
        let bad_ops_artifact = ops_artifact(json!([
            { "op": "add_edge", "source": "不存在", "target": "块二", "type": "bogus",
              "tier": "bogus", "reason": "" },
            { "op": "rename_block", "from": "块二", "to": "块贰", "slug": "block-x", "reason": "r" },
            { "op": "add_block", "name": "新块", "slug": "block-01", "spine": "主干一",
              "stage": "入门", "note": "n", "reason": "r" }
        ]));
        let violations = validate_ops(&bad_ops_artifact, &sample_structure());
        let codes: HashSet<&str> = violations.iter().map(|item| item.code).collect();
        assert!(codes.contains(&ST_OPS_SHAPE), "{violations:?}");
        assert!(codes.contains(&ST_OPS_REASON), "{violations:?}");
        assert!(codes.contains(&ST_OPS_ENDPOINTS), "{violations:?}");
        assert!(codes.contains(&ST_OPS_NO_SLUG_CHURN), "{violations:?}");
        assert!(violations.iter().any(|item| !item.warning));
    }

    #[test]
    fn endpoints_follow_application_order() {
        // add_block 之后引用新块 = 合法（应用序模拟：前序块 ops 即时进端点域）。
        let fresh = ops_artifact(json!([
            { "op": "add_block", "name": "块三", "slug": "block-03", "spine": "主干一",
              "stage": "高级", "note": "n", "reason": "补块" },
            { "op": "add_edge", "source": "块三", "target": "块一", "type": "composes",
              "tier": "use", "note": "", "reason": "组装" }
        ]));
        assert!(validate_ops(&fresh, &sample_structure()).is_empty());

        // 不加块直接引用 = 端点越界。
        let dangling = ops_artifact(json!([
            { "op": "add_edge", "source": "块三", "target": "块一", "type": "composes",
              "tier": "use", "note": "", "reason": "r" }
        ]));
        let violations = validate_ops(&dangling, &sample_structure());
        assert!(
            violations
                .iter()
                .any(|item| item.code == ST_OPS_ENDPOINTS && !item.warning)
        );
    }

    #[test]
    fn no_slug_churn_tracks_retired_slugs() {
        // split 退场 block-01 后，后续 add_block 复用 block-01 = 红条。
        let ops_artifact = ops_artifact(json!([
            { "op": "split_block", "target": "块一", "name": "块一分", "slug": "block-new",
              "stage": "入门", "note": "n", "reason": "r" },
            { "op": "add_block", "name": "块新", "slug": "block-01", "spine": "主干一",
              "stage": "入门", "note": "n", "reason": "r" }
        ]));
        let violations = validate_ops(&ops_artifact, &sample_structure());
        assert!(
            violations
                .iter()
                .any(|item| item.code == ST_OPS_NO_SLUG_CHURN && !item.warning),
            "{violations:?}"
        );
    }

    #[test]
    fn ops_empty_is_a_legal_conclusion() {
        let ops_artifact = ops_artifact(json!([]));
        assert!(validate_ops(&ops_artifact, &sample_structure()).is_empty());
        let ops = parse_ops(&ops_artifact).unwrap();
        assert!(ops.is_empty());
    }

    /// validator ↔ 契约文档互钉：五条款码必须在契约新章出现。
    #[test]
    fn clause_codes_are_interlocked_with_the_contract_doc() {
        let doc = include_str!("../../../../../docs/contracts/learning-structure-artifacts.md");
        for code in [
            ST_OPS_SHAPE,
            ST_OPS_ENDPOINTS,
            ST_OPS_REASON,
            ST_OPS_APPLY,
            ST_OPS_NO_SLUG_CHURN,
        ] {
            assert!(doc.contains(code), "contract doc missing {code}");
        }
    }
}
