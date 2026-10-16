//! 挂载回合（j08，三轮 g1「结构转活文档·上」）：把课程节点挂到结构块的导
//! 航事实层。挂载是导航事实——无判定身份（不进调度、不进门禁、不承载概
//! 念字段）；挂载与修补合起来才是「结构从快照变活文档」的最小闭环，本模
//! 块是第一步（修补随 j09）。
//!
//! 口径（learnhub engine/maintenance/mounting.rs 同构）：
//! - **待挂集** = 现存节点 ∧（未评估 ∪ 失效 slug）；hash 失配（结构变过）
//!   = stale，全量重评。**待挂集空 = skipped（零 LLM 零写）**。
//! - **失败整回合重试不降级**：RoundRetry ≤ 3，同指纹早停（j03 骨架复
//!   用），逐条款违规报告注入重试装配。
//! - **落盘 = 单行原子覆盖**（merge upsert：本轮替换、未评估保留、孤儿清
//!   出）。
//! - **结构 hash = 手写 FNV-1a 64**——跨语言跨版本稳定，勿用语言默认
//!   hash（StdHasher 种子不稳定）。

use std::collections::HashSet;

use serde_json::Value;

use nomifun_common::AppError;

use super::structure_contract::{self, ContractViolation};
use crate::completer::LearningCompleter;

// ── st.mount.* 六条款（单源：validator / 契约文档新章 / 提示词互钉）──────

pub const ST_MOUNT_TOP_KEYS: &str = "st.mount.top.keys";
pub const ST_MOUNT_ENTRIES_REQUIRED: &str = "st.mount.entries.required";
pub const ST_MOUNT_ENTRIES_ALIGN: &str = "st.mount.entries.align";
pub const ST_MOUNT_BLOCKS_RESOLVE: &str = "st.mount.blocks.resolve";
pub const ST_MOUNT_BLOCKS_DEDUP: &str = "st.mount.blocks.dedup";
pub const ST_MOUNT_NO_CONCEPT_FIELDS: &str = "st.mount.no_concept_fields";

/// 条目顶层键白名单（含模型可选的备注键）。
pub const MOUNT_TOP_KEYS: &[&str] = &["mounts", "note"];
/// 挂载条目键白名单：node（节点标题逐字）+ blocks（块 slug 数组，空 =
/// 结构外）+ reason（一句话）；概念字段（teaches/assumes/tier…）一回来即
/// 违反 no_concept_fields。
pub const MOUNT_ENTRY_KEYS: &[&str] = &["node", "blocks", "reason"];
/// 超载块阈值（挂载观察：≥5 节点的块建议拆分——观察不构成编辑义务）。
pub const MOUNT_OVERLOAD_THRESHOLD: usize = 5;

// ── 结构 hash ─────────────────────────────────────────────────────────────

/// FNV-1a 64（手写，跨版本稳定）：对 `structure_json` 列的原样字节做锚。
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ── 挂载条目 ──────────────────────────────────────────────────────────────

/// 一条挂载事实：`blocks` 为空 = 已评估、声明在结构外（Outside）；非空 =
/// 挂入的块 slug 清单（一个节点可挂多块——内容跨块；一块可被多个节点挂，
/// 都没有唯一性限制；单条内 slug 不重复）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub node: String,
    pub blocks: Vec<String>,
    pub reason: String,
}

impl MountEntry {
    pub fn mounted(node: &str, blocks: &[&str], reason: &str) -> Self {
        Self {
            node: node.to_owned(),
            blocks: blocks.iter().map(|slug| (*slug).to_owned()).collect(),
            reason: reason.to_owned(),
        }
    }

    pub fn outside(node: &str, reason: &str) -> Self {
        Self {
            node: node.to_owned(),
            blocks: Vec::new(),
            reason: reason.to_owned(),
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        serde_json::json!({
            "node": self.node,
            "blocks": self.blocks,
            "reason": self.reason,
        })
    }
}

/// 从模型回包/存储列解析条目（宽松形状：坏条目跳过不 Broken——挂载是导
/// 航辅助面，个别坏行不使整体失效；生成面的严格形态走 validate_mounts）。
/// 三读：模型回包 = `{mounts: [...]}` 对象（blocks 数组），存储列 = 裸数组，
/// 兼容位 = j08 早版单块条目（`block: <slug|null>` → 单元素数组/空）。
pub fn parse_mount_entries(value: &Value) -> Vec<MountEntry> {
    let items = match value {
        Value::Array(items) => Some(items),
        Value::Object(_) => value.get("mounts").and_then(Value::as_array),
        _ => None,
    };
    let Some(items) = items else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let node = item.get("node")?.as_str()?.trim().to_owned();
            if node.is_empty() {
                return None;
            }
            let blocks = match item.get("blocks").and_then(Value::as_array) {
                Some(slugs) => slugs
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|slug| !slug.is_empty())
                    .map(str::to_owned)
                    .collect(),
                None => match item.get("block") {
                    Some(Value::String(slug)) if !slug.trim().is_empty() => {
                        vec![slug.trim().to_owned()]
                    }
                    _ => Vec::new(),
                },
            };
            let reason = item
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned();
            Some(MountEntry { node, blocks, reason })
        })
        .collect()
}

/// 挂载条目数组 → 存储列 JSON（单行原子覆盖的载荷；列 CHECK 是裸数组）。
pub fn entries_json(entries: &[MountEntry]) -> String {
    let items: Vec<Value> = entries.iter().map(MountEntry::to_json).collect();
    Value::Array(items).to_string()
}

// ── st.mount.* 验证器（生成面执法；与契约文档新章互钉）────────────────────

/// 验证一份挂载产物：`block_slugs` = 当前结构 blocks[].slug 全集（逐字），
/// `node_titles` = 待挂集（本轮允许评估的节点标题，逐字）。六条款全红——
/// 违规报告原文注入重试。
pub fn validate_mounts(
    artifact: &Value,
    block_slugs: &HashSet<String>,
    node_titles: &HashSet<String>,
) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    let red = |code: &'static str, message: String| ContractViolation {
        code,
        message,
        warning: false,
    };
    if !artifact.is_object() {
        return vec![red(
            ST_MOUNT_TOP_KEYS,
            "挂载产物必须是单个 JSON 对象（顶层键只允许 mounts、note）".into(),
        )];
    }
    // st.mount.top.keys：顶层键白名单 + mounts 必为数组。
    let keys: Vec<&str> = artifact
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect())
        .unwrap_or_default();
    for key in &keys {
        if !MOUNT_TOP_KEYS.contains(key) {
            violations.push(red(
                ST_MOUNT_TOP_KEYS,
                format!("未知顶层键「{key}」——顶层只允许 mounts、note"),
            ));
        }
    }
    let Some(items) = artifact.get("mounts").and_then(Value::as_array) else {
        violations.push(red(
            ST_MOUNT_TOP_KEYS,
            "缺少 mounts 数组（0 条也是合法答案，但键必须在）".into(),
        ));
        return violations;
    };
    let mut seen_nodes: HashSet<String> = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let at = format!("mounts[{index}]");
        let Some(object) = item.as_object() else {
            violations.push(red(
                ST_MOUNT_ENTRIES_REQUIRED,
                format!("{at} 必须是对象（键：node/blocks/reason）"),
            ));
            continue;
        };
        // st.mount.entries.required：条目键白名单 + 三键齐备非空。
        for key in object.keys() {
            if !MOUNT_ENTRY_KEYS.contains(&key.as_str()) {
                violations.push(red(
                    ST_MOUNT_ENTRIES_REQUIRED,
                    format!("{at} 携带未知键「{key}」——条目只允许 node/blocks/reason"),
                ));
            }
        }
        let Some(node) = object.get("node").and_then(Value::as_str).map(str::trim) else {
            violations.push(red(
                ST_MOUNT_ENTRIES_REQUIRED,
                format!("{at} 缺 node（节点标题逐字）"),
            ));
            continue;
        };
        if node.is_empty() {
            violations.push(red(
                ST_MOUNT_ENTRIES_REQUIRED,
                format!("{at} 的 node 是空串"),
            ));
            continue;
        }
        // st.mount.entries.required：blocks 必为 slug 字符串数组（空数组 =
        // 结构外，合法）；reason 一句话非空。
        let blocks: Vec<String> = match object.get("blocks").and_then(Value::as_array) {
            Some(slugs) => slugs
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|slug| !slug.is_empty())
                .map(str::to_owned)
                .collect(),
            None => {
                violations.push(red(
                    ST_MOUNT_ENTRIES_REQUIRED,
                    format!("{at} 缺 blocks（slug 数组；结构外给空数组）"),
                ));
                continue;
            }
        };
        if blocks.len() != object.get("blocks").and_then(Value::as_array).unwrap_or(&Vec::new()).len()
        {
            violations.push(red(
                ST_MOUNT_BLOCKS_RESOLVE,
                format!("{at} 的 blocks 含空串或非字符串项——slug 逐字照抄结构参考行"),
            ));
        }
        let reason_ok = object
            .get("reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|text| !text.is_empty());
        if !reason_ok {
            violations.push(red(
                ST_MOUNT_ENTRIES_REQUIRED,
                format!("{at} 缺 reason（一句话：为什么挂这里/为什么在结构外）"),
            ));
        }
        // st.mount.entries.align：node 必须逐字命中待挂集。
        if !node_titles.contains(node) {
            violations.push(red(
                ST_MOUNT_ENTRIES_ALIGN,
                format!(
                    "{at} 的节点「{node}」不在待挂集里——只评估待挂集节点，标题逐字照抄输入"
                ),
            ));
        }
        // st.mount.blocks.resolve：每个 slug 逐字解析到结构块。
        for slug in &blocks {
            if !block_slugs.contains(slug) {
                violations.push(red(
                    ST_MOUNT_BLOCKS_RESOLVE,
                    format!(
                        "{at} 的块 slug「{slug}」在当前结构里不存在——slug 逐字照抄结构参考行"
                    ),
                ));
            }
        }
        // st.mount.blocks.dedup：一个节点只一条条目（重复条目整条点名）；
        // 单条内 slug 不重复。
        if !seen_nodes.insert(node.to_owned()) {
            violations.push(red(
                ST_MOUNT_BLOCKS_DEDUP,
                format!("节点「{node}」出现多条目——待挂集每个节点恰一条条目"),
            ));
        }
        let mut entry_slugs: HashSet<&str> = HashSet::new();
        for slug in &blocks {
            if !entry_slugs.insert(slug.as_str()) {
                violations.push(red(
                    ST_MOUNT_BLOCKS_DEDUP,
                    format!("{at} 的 blocks 内 slug「{slug}」重复"),
                ));
            }
        }
        // st.mount.no_concept_fields：条目内嵌对象携带概念字段即违规
        // （挂载是导航事实，无判定身份）。
        for key in object.keys() {
            if matches!(key.as_str(), "teaches" | "assumes" | "tier" | "concepts") {
                violations.push(red(
                    ST_MOUNT_NO_CONCEPT_FIELDS,
                    format!("{at} 携带概念字段「{key}」——挂载无判定身份，概念事实只住节点挂接表"),
                ));
            }
        }
    }
    // st.mount.entries.align（覆盖面）：待挂集每个节点恰一条——缺失同样点名。
    for title in node_titles {
        if !seen_nodes.contains(title.as_str()) {
            violations.push(red(
                ST_MOUNT_ENTRIES_ALIGN,
                format!("待挂节点「{title}」没有条目——待挂集每个节点都必须给出决定"),
            ));
        }
    }
    violations
}

// ── 待挂集与合并 ──────────────────────────────────────────────────────────

/// 待挂集（现存节点全集的纯函数面）：未评估（无条目）∪ 失效 slug（条目
/// 任一挂块在当前结构里不存在——其 node 重新评估）。stale = 结构 hash 失配
/// 时调用方直接传全量。
pub fn pending_nodes(
    node_titles: &[String],
    existing: &[MountEntry],
    block_slugs: &HashSet<String>,
) -> Vec<String> {
    let evaluated: HashSet<&str> = existing.iter().map(|entry| entry.node.as_str()).collect();
    let dangling: HashSet<&str> = existing
        .iter()
        .filter(|entry| {
            entry
                .blocks
                .iter()
                .any(|slug| !block_slugs.contains(slug))
        })
        .map(|entry| entry.node.as_str())
        .collect();
    node_titles
        .iter()
        .filter(|title| !evaluated.contains(title.as_str()) || dangling.contains(title.as_str()))
        .cloned()
        .collect()
}

/// merge upsert（单行原子覆盖的合成面）：本轮替换（pending 节点以本轮结
/// 果为准）、未评估保留、孤儿清出（节点不存在或块 slug 失效的既有条目丢
/// 弃）。本轮条目按出现顺序去重（同节点首条为准）。
pub fn merge_mounts(
    existing: &[MountEntry],
    evaluated: &[MountEntry],
    pending: &[String],
    node_titles: &HashSet<String>,
    block_slugs: &HashSet<String>,
) -> Vec<MountEntry> {
    let pending: HashSet<&str> = pending.iter().map(String::as_str).collect();
    let mut merged: Vec<MountEntry> = existing
        .iter()
        .filter(|entry| {
            if pending.contains(entry.node.as_str()) {
                return false; // 本轮替换
            }
            if !node_titles.contains(entry.node.as_str()) {
                return false; // 孤儿：节点已不存在
            }
            if entry
                .blocks
                .iter()
                .any(|slug| !block_slugs.contains(slug))
            {
                return false; // 孤儿：块 slug 已失效
            }
            true
        })
        .cloned()
        .collect();
    let mut seen: HashSet<String> = merged.iter().map(|entry| entry.node.clone()).collect();
    for entry in evaluated {
        if seen.insert(entry.node.clone()) {
            merged.push(entry.clone());
        }
    }
    merged
}

/// 挂载观察派生（修补输入源之一，纯读函数不打点）：结构外单元 + 空块 +
/// 超载块（≥ [`MOUNT_OVERLOAD_THRESHOLD`] 节点）。仅观察，不构成编辑义务。
pub fn mount_observations(
    blocks: &[(String, String)], // (slug, name)
    entries: &[MountEntry],
) -> Vec<(String, String)> {
    let mut observations = Vec::new();
    for entry in entries.iter().filter(|entry| entry.blocks.is_empty()) {
        observations.push((
            "outside-node".to_owned(),
            format!("节点「{}」声明在结构外：{}", entry.node, entry.reason),
        ));
    }
    for (slug, name) in blocks {
        let mounted: Vec<&str> = entries
            .iter()
            .filter_map(|entry| {
                (entry.blocks.iter().any(|s| s == slug)).then_some(entry.node.as_str())
            })
            .collect();
        if mounted.is_empty() {
            observations.push((
                "empty-block".to_owned(),
                format!("块「{name}」（{slug}）没有任何节点挂载"),
            ));
        } else if mounted.len() >= MOUNT_OVERLOAD_THRESHOLD {
            observations.push((
                "overloaded-block".to_owned(),
                format!(
                    "块「{name}」（{slug}）挂了 {} 个节点（≥{}，建议后续修补拆分）：{}",
                    mounted.len(),
                    MOUNT_OVERLOAD_THRESHOLD,
                    mounted.join("、")
                ),
            ));
        }
    }
    observations
}

// ── 挂载回合（单 LLM 调用 + 契约门 + RoundRetry）────────────────────────

/// 会话级系统行（与结构生成同基调；任务纪律住提示词资产）。
const MOUNT_SYSTEM: &str =
    "你是学习课程的结构挂载引擎：严格按任务说明输出一个 JSON 对象，不输出任何其他字符。";

/// 挂载回合输入（七件）：宪章三项 + 结构参考行 + 待挂集 + 既有挂载 + 重试
/// 反馈。
pub(crate) struct MountRoundInput<'a> {
    pub goal: &'a str,
    pub proficiency_statement: &'a str,
    pub learner_basis: &'a str,
    pub structure_blocks: &'a str,
    pub pending_nodes: &'a str,
    pub existing_mounts: &'a str,
}

/// 逐条挂载回合：装配 STRUCTURE_MOUNT 提示词（temperature 0.0 + thinking）
/// → json 接缝 → 契约门；违规报告原文注入重试，≤ [`MAX_ATTEMPTS`] 轮，同
/// 指纹早停（j03 骨架复用）。真取消：令牌循环头检查 + `select!` 竞速。
pub(crate) async fn run_mount_round(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    input: &MountRoundInput<'_>,
    block_slugs: &HashSet<String>,
    node_titles: &HashSet<String>,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<Vec<MountEntry>, AppError> {
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
    for attempt in 1..=structure_gen_max_attempts() {
        if cancel.is_some_and(|token| token.is_cancelled()) {
            return Err(AppError::Conflict(
                crate::service::CANCELLED_MESSAGE.to_owned(),
            ));
        }
        let prompt = super::structure_prompts::assemble(
            &super::structure_prompts::STRUCTURE_MOUNT,
            &[
                ("goal", input.goal),
                ("proficiency_statement", input.proficiency_statement),
                ("learner_basis", input.learner_basis),
                ("structure_blocks", input.structure_blocks),
                ("pending_nodes", input.pending_nodes),
                ("existing_mounts", input.existing_mounts),
                ("retry_feedback", feedback.as_str()),
            ],
        )
        .map_err(|error| AppError::Internal(error.to_string()))?;
        let raw = crate::generation::complete_json_coach(
            completer,
            model_override,
            MOUNT_SYSTEM,
            &prompt.text,
            crate::generation::STRUCTURE_MAX_TOKENS,
            Some(super::structure_prompts::STRUCTURE_MOUNT.temperature),
        )
        .await;
        let raw = match raw {
            Ok(raw) => raw,
            Err(error) => {
                // LLM 错误（无凭据/网络）不与条款违约混报：如实上抛最后
                // 一次的错误——配置问题不会因重试收敛。
                last = error.to_string();
                last_was_llm = true;
                tracing::warn!(attempt, "mount round LLM call failed");
                continue;
            }
        };
        last_was_llm = false;
        let artifact = crate::generation::parse_json_object::<Value>(&raw)
            .map_err(|error| format!("挂载产物解析失败（非合法 JSON 对象）：{error}"))
            .and_then(|value| {
                if value.is_object() {
                    Ok(value)
                } else {
                    Err("挂载产物必须是单个 JSON 对象".to_owned())
                }
            });
        let artifact = match artifact {
            Ok(artifact) => artifact,
            Err(report) => {
                last = report;
                feedback = format!("上一轮产物违反契约,逐条修正后重新生成:\n{last}");
                continue;
            }
        };
        let violations = validate_mounts(&artifact, block_slugs, node_titles);
        let red: Vec<&ContractViolation> =
            violations.iter().filter(|item| !item.warning).collect();
        if red.is_empty() {
            return Ok(parse_mount_entries(&artifact));
        }
        let owned: Vec<ContractViolation> = red.into_iter().cloned().collect();
        last = structure_contract::format_violations(&owned);
        // 同指纹早停（j03 骨架）：连败两轮同一份逐条款报告 → 反馈回灌已
        // 无法改变结果，立即耗尽。
        let fingerprint = super::growth_retry::normalize_report(&last);
        if attempt > 1 && prev_fingerprint.as_deref() == Some(fingerprint.as_str()) {
            tracing::warn!(attempt, "mount round produced the same violations twice — stopping early");
            break;
        }
        prev_fingerprint = Some(fingerprint);
        feedback = format!("上一轮产物违反契约,逐条修正后重新生成:\n{last}");
    }
    if last_was_llm {
        return Err(AppError::Conflict(format!(
            "挂载回合未执行（生成通道不可用，挂载表未改动）：{last}"
        )));
    }
    Err(AppError::UnprocessableEntity(format!(
        "挂载回合未通过契约门（逐条款报告见下）；挂载表未改动，可重试：\n{last}"
    )))
}

fn structure_gen_max_attempts() -> usize {
    super::structure_gen::MAX_ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn slugs(items: &[&str]) -> HashSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn titles(items: &[&str]) -> HashSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn fnv1a_is_hand_written_and_stable() {
        // FNV-1a 64 的公开测试向量（"a" 与空串）。
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn well_formed_mounts_pass_all_clauses() {
        let artifact = json!({
            "mounts": [
                { "node": "节点一", "blocks": ["block-01"], "reason": "同讲一个主题" },
                { "node": "节点二", "blocks": [], "reason": "纯练习节点，不属于任何块" }
            ],
            "note": "两条结构外判断见 reason"
        });
        let violations = validate_mounts(&artifact, &slugs(&["block-01"]), &titles(&["节点一", "节点二"]));
        assert!(
            violations.iter().all(|item| item.warning),
            "well-formed artifact must pass: {violations:?}"
        );
    }

    #[test]
    fn clause_violations_are_red_and_coded() {
        let artifact = json!({
            "mounts": [
                { "node": "节点一", "blocks": ["block-99"], "reason": "" },
                { "node": "节点一", "blocks": ["block-01"], "reason": "重复节点" },
                { "node": "不在待挂集", "blocks": [], "reason": "r", "teaches": [] }
            ],
            "bogus": 1
        });
        let violations = validate_mounts(&artifact, &slugs(&["block-01"]), &titles(&["节点一", "节点二"]));
        let codes: HashSet<&str> = violations.iter().map(|item| item.code).collect();
        assert!(codes.contains(&ST_MOUNT_TOP_KEYS), "{violations:?}");
        assert!(codes.contains(&ST_MOUNT_ENTRIES_REQUIRED), "{violations:?}");
        assert!(codes.contains(&ST_MOUNT_ENTRIES_ALIGN), "{violations:?}");
        assert!(codes.contains(&ST_MOUNT_BLOCKS_RESOLVE), "{violations:?}");
        assert!(codes.contains(&ST_MOUNT_BLOCKS_DEDUP), "{violations:?}");
        assert!(codes.contains(&ST_MOUNT_NO_CONCEPT_FIELDS), "{violations:?}");
        assert!(violations.iter().any(|item| !item.warning));
    }

    /// 覆盖面（entries.align 的缺失半边）：待挂集里有节点没条目 = 红条。
    #[test]
    fn missing_pending_node_is_red() {
        let artifact = json!({
            "mounts": [
                { "node": "节点一", "blocks": ["block-01"], "reason": "r" }
            ]
        });
        let violations =
            validate_mounts(&artifact, &slugs(&["block-01"]), &titles(&["节点一", "节点二"]));
        assert!(
            violations
                .iter()
                .any(|item| item.code == ST_MOUNT_ENTRIES_ALIGN && !item.warning),
            "{violations:?}"
        );
    }

    #[test]
    fn pending_set_is_unevaluated_plus_dangling() {
        let nodes = vec!["甲".to_owned(), "乙".to_owned(), "丙".to_owned()];
        let existing = vec![
            MountEntry::mounted("甲", &["block-01"], "r"),
            MountEntry::mounted("乙", &["block-gone"], "r"),
        ];
        let pending = pending_nodes(&nodes, &existing, &slugs(&["block-01"]));
        // 甲已评估且块仍解析 → 不在待挂；乙的块失效 → 重评；丙未评估 → 待挂。
        assert_eq!(pending, vec!["乙".to_owned(), "丙".to_owned()]);
    }

    #[test]
    fn merge_replaces_pending_keeps_evaluated_and_drops_orphans() {
        let existing = vec![
            MountEntry::mounted("甲", &["block-01"], "旧理由"),
            MountEntry::mounted("乙", &["block-gone"], "失效块"),
            MountEntry::mounted("孤儿", &["block-01"], "节点已删"),
            MountEntry::outside("丙", "声明结构外"),
        ];
        let evaluated = vec![MountEntry::mounted("甲", &["block-02"], "本轮改挂")];
        let merged = merge_mounts(
            &existing,
            &evaluated,
            &["甲".to_owned()],
            &titles(&["甲", "丙"]),
            &slugs(&["block-01", "block-02"]),
        );
        // 甲被本轮替换；乙（失效块孤儿）与「孤儿」（节点不存在）清出；丙保留。
        assert_eq!(
            merged,
            vec![
                MountEntry::outside("丙", "声明结构外"),
                MountEntry::mounted("甲", &["block-02"], "本轮改挂"),
            ]
        );
    }

    #[test]
    fn observations_cover_outside_empty_and_overloaded() {
        let blocks = vec![
            ("block-01".to_owned(), "块一".to_owned()),
            ("block-02".to_owned(), "块二".to_owned()),
        ];
        let entries = vec![
            MountEntry::mounted("甲", &["block-01"], "r"),
            MountEntry::mounted("乙", &["block-01"], "r"),
            MountEntry::mounted("丙", &["block-01"], "r"),
            MountEntry::mounted("丁", &["block-01"], "r"),
            MountEntry::mounted("戊", &["block-01"], "r"),
            MountEntry::outside("己", "纯练习"),
        ];
        let observations = mount_observations(&blocks, &entries);
        let kinds: Vec<&str> = observations.iter().map(|(kind, _)| kind.as_str()).collect();
        assert!(kinds.contains(&"outside-node"));
        assert!(kinds.contains(&"empty-block"));
        assert!(kinds.contains(&"overloaded-block"));
    }

    #[test]
    fn parse_tolerates_malformed_entries_and_dual_reads_old_shape() {
        let value = json!({
            "mounts": [
                { "node": "甲", "blocks": ["block-01"], "reason": "r" },
                { "node": "旧", "block": "block-02", "reason": "旧形状单块" },
                { "node": "外", "block": null, "reason": "旧形状结构外" },
                { "block": "block-02" },
                { "node": "", "blocks": [], "reason": "r" },
                "garbage"
            ]
        });
        let entries = parse_mount_entries(&value);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].blocks, vec!["block-01".to_owned()]);
        assert_eq!(entries[1].blocks, vec!["block-02".to_owned()], "单块兼容位");
        assert!(entries[2].blocks.is_empty(), "旧 null = 结构外");
        assert_eq!(entries[0].node, "甲");
    }
    /// validator ↔ 契约文档互钉（learnhub 形态）：六条款码必须在契约新章
    /// 出现——改码或改文档先红一方。
    #[test]
    fn clause_codes_are_interlocked_with_the_contract_doc() {
        let doc = include_str!("../../../../../docs/contracts/learning-structure-artifacts.md");
        for code in [
            ST_MOUNT_TOP_KEYS,
            ST_MOUNT_ENTRIES_REQUIRED,
            ST_MOUNT_ENTRIES_ALIGN,
            ST_MOUNT_BLOCKS_RESOLVE,
            ST_MOUNT_BLOCKS_DEDUP,
            ST_MOUNT_NO_CONCEPT_FIELDS,
        ] {
            assert!(doc.contains(code), "contract doc missing {code}");
        }
    }
}
