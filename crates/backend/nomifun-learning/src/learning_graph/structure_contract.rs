//! Structure & terrain artifact contracts (`gd.*`) — the allo port of
//! learnhub's decomposition contract v2.4 (wayfinder 票 g2/g4，地图
//! `docs/tracker/learning-absorb/`）。
//!
//! Three artifacts live here, all operating on the raw JSON value the model
//! returned through the json-mode completer seam:
//!
//! - **word lists & bounds** — the single source every consumer (validator,
//!   JSON schema, prompt self-check lists, tests) must quote;
//! - [`validate_structure`] / [`validate_terrain`] — the clause validators;
//!   every violation carries its stable `gd.*` machine code, which doubles as
//!   the retry-feedback handle (the whole-artifact retry injects the formatted
//!   report back into the next attempt);
//! - [`structure_schema`] / [`terrain_schema`] — JSON Schemas built FROM the
//!   same constants; pinning tests lock schema ↔ validator agreement.
//!
//! Division of labour (learnhub 同款): the provider's json_object mode
//! guarantees reply legality (`gd.parse` is therefore a read-side clause —
//! the validator judges object-ness, not JSON syntax); this module judges
//! shape and semantics, and the semantic clauses never retire.

use serde_json::Value;
use std::collections::HashMap;

/// 一条契约违规：`code` 是稳定机器标识（gd.* 条款编号——校验器报错、整批
/// 重试反馈、提示词自查三方的公共语言）；`warning = true` 的条目只报告、
/// 不拒收（learnhub 警告级同款）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractViolation {
    pub code: &'static str,
    pub message: String,
    pub warning: bool,
}

impl ContractViolation {
    fn red(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), warning: false }
    }

    fn warn(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), warning: true }
    }
}

/// 违规报告的全文形态（整批重试反馈注入的形状，教练/生成器按它逐条修正）。
pub fn format_violations(violations: &[ContractViolation]) -> String {
    violations
        .iter()
        .map(|violation| format!("- [{}] {}", violation.code, violation.message))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── 词表与量级档（单源：validator / schema / 提示词 / 测试都引这里）────────

/// structure 顶层键白名单。
pub const TOP_KEYS: &[&str] = &[
    "goal",
    "goal_type",
    "proficiency_statement",
    "domain_type",
    "form_reference",
    "source_tier",
    "generated_by",
    "disclaimer",
    "domain",
    "spines",
    "blocks",
    "edges",
    "gaps",
    "quality",
];

/// 结构性必填顶层键；八个标量键的缺席/为空由 gd.frontmatter 报（learnhub
/// 两类条款分工同款：gd.keys 管白名单与结构节，gd.frontmatter 管标量字段）。
pub const REQUIRED_KEYS: &[&str] = &["domain", "spines", "blocks", "gaps", "quality"];

/// 八个标量键（gd.frontmatter 逐个查非空）。
pub const SCALAR_KEYS: &[&str] = &[
    "goal",
    "goal_type",
    "proficiency_statement",
    "domain_type",
    "form_reference",
    "source_tier",
    "generated_by",
    "disclaimer",
];

/// 块条目键白名单（五键纪律：禁六件套与任何判据形字段——块不是概念）。
pub const BLOCK_KEYS: &[&str] = &["name", "slug", "spine", "stage", "note"];

/// 主干条目键白名单（thin_spine + reason = 每干块数不足的豁免声明）。
pub const SPINE_KEYS: &[&str] = &["name", "definition", "key_points", "instance", "thin_spine", "reason"];

/// 难度档词表（顺序即矩阵列序）。
pub const STAGES: &[&str] = &["入门", "进阶", "高级"];
/// 边型词表。
pub const EDGE_TYPES: &[&str] = &["assumes", "composes", "contrasts"];
/// 结构边档位词表（机面英文；注意与概念网 tier 的 know/apply/teach 是两套词）。
pub const EDGE_TIERS: &[&str] = &["know", "use", "teach"];
/// 领域适配四型词表（由模型自判）。
pub const DOMAIN_TYPES: &[&str] = &["理论", "工程", "历史", "技能"];
/// source 两档制词表。
pub const SOURCE_TIERS: &[&str] = &["名录", "章节"];
/// 目标类型词表。
pub const GOAL_TYPES: &[&str] = &["capability", "coverage"];
/// 质量清单状态词表。
pub const QUALITY_STATUS: &[&str] = &["met", "unmet"];

/// 块表量级档：下限 / 越档线（告警）/ 硬上限。
pub const BLOCKS_MIN: usize = 40;
pub const BLOCKS_SOFT_MAX: usize = 80;
pub const BLOCKS_MAX: usize = 100;
/// 主干条数档。
pub const SPINES_MIN: usize = 6;
pub const SPINES_MAX: usize = 12;
/// 每主干块数配额（薄干声明可豁免）。
pub const SPINE_BLOCK_QUOTA: usize = 5;
/// 盲区声明 / 质量清单最小条数。
pub const MIN_GAPS: usize = 3;
pub const MIN_QUALITY: usize = 5;

/// 并列连词与并列分隔符：块名一物一名的禁入字符；「」官方术语段内豁免。
/// 机械读法放宽（learnhub #53 留痕）：「和」在词首/词尾位是名词不是连词
/// （「等差前n项和」之「和」= sum），只在词中位计为并列标记；与/及无实词
/// 用法，仍在任意位计数。
const NAME_COMPOUND_MARKS: [char; 7] = ['与', '和', '及', '、', '/', ',', '，'];

fn compound_name_mark(name: &str) -> Option<char> {
    let chars: Vec<char> = name.chars().collect();
    let mut quoted = false;
    for (i, c) in chars.iter().copied().enumerate() {
        match c {
            '「' => quoted = true,
            '」' => quoted = false,
            _ if quoted => {}
            '和' if i == 0 || i + 1 == chars.len() => {}
            _ if NAME_COMPOUND_MARKS.contains(&c) => return Some(c),
            _ => {}
        }
    }
    None
}

fn slug_valid(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn door_id_valid(id: &str) -> bool {
    let mut chars = id.chars();
    if chars.next() != Some('G') {
        return false;
    }
    let digits = chars.as_str();
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// 排期语词的机械代理（gd 全文扫描；learnhub 同款放宽——裸「天/周/月」误报
/// 率高（星期/日期/九月合法），扫排期形态词表；条款意图是「不得出现顺序
/// 承诺与时间表」，非禁日历字面）。`quality` 自检清单与 `disclaimer` 出处
/// 声明豁免（元声明面——自检理由合法地谈论排期红线本身）。
const SCHEDULE_MARKERS: [&str; 12] = [
    "排期", "日程", "时间表", "进度", "计划", "小时", "每天", "每周", "每月", "个月", "半天", "整周",
];

fn collect_strings(value: &Value, skip_keys: &[&str], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if skip_keys.contains(&key.as_str()) {
                    continue;
                }
                collect_strings(child, skip_keys, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_strings(item, skip_keys, out);
            }
        }
        Value::String(text) => out.push(text.clone()),
        _ => {}
    }
}

fn trim_strings(values: &[String]) -> impl Iterator<Item = &str> {
    values.iter().map(|value| value.trim()).filter(|value| !value.is_empty())
}

fn string_array<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

// ── structure 契约 ──────────────────────────────────────────────────────────

/// structure 产物契约（gd.*）：JSON 值面逐条款执法。violations 非空 = 整批
/// 打回；warnings 只随报告透出（越档规模 / 参考质量），不触发重试。
pub fn validate_structure(value: &Value) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    let Some(map) = value.as_object() else {
        violations.push(ContractViolation::red(
            "gd.parse",
            "产物必须是单个 JSON 对象（json 模式已保证语法合法，此处查形状）",
        ));
        return violations;
    };

    // gd.keys：白名单 + 结构五件必填。
    for key in map.keys() {
        if !TOP_KEYS.contains(&key.as_str()) {
            violations.push(ContractViolation::red(
                "gd.keys",
                format!("未知顶层键「{key}」——白名单之外的键一律删除"),
            ));
        }
    }
    for key in REQUIRED_KEYS {
        if !map.contains_key(*key) {
            violations.push(ContractViolation::red(
                "gd.keys",
                format!("缺少必填顶层键「{key}」"),
            ));
        }
    }

    // gd.frontmatter：八个标量键非空 + 词表执法。
    for key in SCALAR_KEYS {
        let empty_or_missing = map
            .get(*key)
            .and_then(Value::as_str)
            .map(|text| text.trim().is_empty())
            .unwrap_or(true);
        if empty_or_missing {
            violations.push(ContractViolation::red(
                "gd.frontmatter",
                format!("标量键「{key}」缺失或为空"),
            ));
        }
    }
    if let Some(goal_type) = map.get("goal_type").and_then(Value::as_str) {
        if !GOAL_TYPES.contains(&goal_type) {
            violations.push(ContractViolation::red(
                "gd.frontmatter",
                format!("goal_type 必须是 {} 之一，得到「{goal_type}」", GOAL_TYPES.join("/")),
            ));
        }
    }
    if let Some(domain_type) = map.get("domain_type").and_then(Value::as_str) {
        if !DOMAIN_TYPES.contains(&domain_type) {
            violations.push(ContractViolation::red(
                "gd.frontmatter",
                format!("domain_type 必须是 {} 四字之一", DOMAIN_TYPES.join("/")),
            ));
        }
    }
    if let Some(source_tier) = map.get("source_tier").and_then(Value::as_str) {
        if !SOURCE_TIERS.contains(&source_tier) {
            violations.push(ContractViolation::red(
                "gd.frontmatter",
                format!("source_tier 必须是 {} 之一", SOURCE_TIERS.join("/")),
            ));
        }
    }

    // gd.domain.required：领域界定六字段 + 数量档。
    if let Some(domain) = map.get("domain") {
        violations.extend(validate_domain(domain));
    }

    // gd.spines.count / gd.spines.required。
    let Some(spines) = map.get("spines").and_then(Value::as_array) else {
        violations.push(ContractViolation::red("gd.spines.count", "spines 必须是数组"));
        return violations;
    };
    if !(SPINES_MIN..=SPINES_MAX).contains(&spines.len()) {
        violations.push(ContractViolation::red(
            "gd.spines.count",
            format!("spines 条数必须 ∈ {SPINES_MIN}..={SPINES_MAX}，得到 {} 条", spines.len()),
        ));
    }
    let mut spine_names: Vec<String> = Vec::new();
    for (index, spine) in spines.iter().enumerate() {
        let at = format!("spines[{index}]");
        let Some(spine_map) = spine.as_object() else {
            violations.push(ContractViolation::red("gd.spines.required", format!("{at} 不是对象")));
            continue;
        };
        for key in spine_map.keys() {
            if !SPINE_KEYS.contains(&key.as_str()) {
                violations.push(ContractViolation::red(
                    "gd.spines.required",
                    format!("{at} 含未知键「{key}」——主干只允许 {}", SPINE_KEYS.join("/")),
                ));
            }
        }
        for key in ["name", "definition", "instance"] {
            let empty = spine_map
                .get(key)
                .and_then(Value::as_str)
                .map(|text| text.trim().is_empty())
                .unwrap_or(true);
            if empty {
                violations.push(ContractViolation::red(
                    "gd.spines.required",
                    format!("{at} 的「{key}」缺失或为空"),
                ));
            }
        }
        let key_points = string_array(spine, "key_points");
        if key_points.iter().all(|point| point.trim().is_empty()) || spine_map.get("key_points").is_none()
        {
            violations.push(ContractViolation::red(
                "gd.spines.required",
                format!("{at} 的 key_points 至少 1 条非空要点"),
            ));
        }
        if let Some(name) = spine_map.get("name").and_then(Value::as_str) {
            let name = name.trim();
            if !name.is_empty() {
                if spine_names.iter().any(|seen| seen == name) {
                    violations.push(ContractViolation::red(
                        "gd.spines.required",
                        format!("主干名「{name}」重复"),
                    ));
                }
                spine_names.push(name.to_owned());
            }
        }
    }

    // gd.blocks.*：量级档 + 五键纪律 + 一物一名 + slug + 主干引用 + 配额。
    let Some(blocks) = map.get("blocks").and_then(Value::as_array) else {
        violations.push(ContractViolation::red("gd.blocks.count", "blocks 必须是数组"));
        return violations;
    };
    if blocks.len() > BLOCKS_MAX {
        violations.push(ContractViolation::red(
            "gd.blocks.count",
            format!("blocks {} 条超过硬上限 {BLOCKS_MAX}——拆主干或合并同质块", blocks.len()),
        ));
    } else if blocks.len() < BLOCKS_MIN {
        violations.push(ContractViolation::red(
            "gd.blocks.count",
            format!(
                "blocks {} 条不足下限 {BLOCKS_MIN}——把宽泛块拆成可独立教学的单元",
                blocks.len()
            ),
        ));
    } else if blocks.len() > BLOCKS_SOFT_MAX {
        violations.push(ContractViolation::warn(
            "gd.blocks.count",
            format!("blocks {} 条越档（>{BLOCKS_SOFT_MAX}）——检查是否可合并", blocks.len()),
        ));
    }
    let mut block_names: Vec<String> = Vec::new();
    let mut slugs: Vec<String> = Vec::new();
    let mut per_spine: HashMap<String, usize> = HashMap::new();
    for (index, block) in blocks.iter().enumerate() {
        let at = format!("blocks[{index}]");
        let Some(block_map) = block.as_object() else {
            violations.push(ContractViolation::red("gd.blocks.fields", format!("{at} 不是对象")));
            continue;
        };
        for key in block_map.keys() {
            if !BLOCK_KEYS.contains(&key.as_str()) {
                violations.push(ContractViolation::red(
                    "gd.blocks.fields",
                    format!(
                        "{at} 含禁入键「{key}」——块只有五键 {}，判据形字段一律不进块（块不是概念）",
                        BLOCK_KEYS.join("/")
                    ),
                ));
            }
        }
        let name = block_map.get("name").and_then(Value::as_str).unwrap_or_default().trim();
        if name.is_empty() {
            violations.push(ContractViolation::red("gd.blocks.slug", format!("{at} 的 name 为空")));
            continue;
        }
        if let Some(mark) = compound_name_mark(name) {
            violations.push(ContractViolation::red(
                "gd.blocks.one_name",
                format!(
                    "块名「{name}」含并列标记「{mark}」——一条 = 一个独立教学主题，多个主题须拆条；官方术语本身含并列形的以「」包裹"
                ),
            ));
        }
        if block_names.iter().any(|seen| seen == name) {
            violations.push(ContractViolation::red("gd.blocks.slug", format!("块名「{name}」重复")));
        }
        block_names.push(name.to_owned());
        let slug = block_map.get("slug").and_then(Value::as_str).unwrap_or_default().trim();
        if !slug_valid(slug) {
            violations.push(ContractViolation::red(
                "gd.blocks.slug",
                format!("块「{name}」的 slug「{slug}」必须匹配 ^[a-z0-9-]+$"),
            ));
        } else if slugs.iter().any(|seen| seen == slug) {
            violations.push(ContractViolation::red("gd.blocks.slug", format!("slug「{slug}」重复")));
        } else {
            slugs.push(slug.to_owned());
        }
        let stage = block_map.get("stage").and_then(Value::as_str).unwrap_or_default().trim();
        if !STAGES.contains(&stage) {
            violations.push(ContractViolation::red(
                "gd.blocks.fields",
                format!("块「{name}」的 stage 必须是 {} 之一，得到「{stage}」", STAGES.join("/")),
            ));
        }
        let spine_ref = block_map.get("spine").and_then(Value::as_str).unwrap_or_default().trim();
        if spine_ref.is_empty() {
            violations.push(ContractViolation::red(
                "gd.blocks.spine_ref",
                format!("块「{name}」的 spine 为空"),
            ));
        } else if !spine_names.iter().any(|seen| seen == spine_ref) {
            violations.push(ContractViolation::red(
                "gd.blocks.spine_ref",
                format!("块「{name}」的 spine「{spine_ref}」不在 spines 名集内"),
            ));
        }
        if !spine_ref.is_empty() {
            *per_spine.entry(spine_ref.to_owned()).or_default() += 1;
        }
        let note = block_map.get("note").and_then(Value::as_str).unwrap_or_default().trim();
        if note.is_empty() {
            violations.push(ContractViolation::red(
                "gd.blocks.fields",
                format!("块「{name}」的 note（一句话主题描述）为空"),
            ));
        }
    }
    for spine in &spine_names {
        let count = per_spine.get(spine).copied().unwrap_or(0);
        if count >= SPINE_BLOCK_QUOTA {
            continue;
        }
        let thin = spines
            .iter()
            .find(|spine_value| spine_value.get("name").and_then(Value::as_str) == Some(spine.as_str()));
        let exempt = thin
            .and_then(|spine_value| spine_value.get("thin_spine"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && thin
                .and_then(|spine_value| spine_value.get("reason"))
                .and_then(Value::as_str)
                .map(|reason| !reason.trim().is_empty())
                .unwrap_or(false);
        if !exempt {
            violations.push(ContractViolation::red(
                "gd.blocks.quota",
                format!(
                    "主干「{spine}」只有 {count} 块（配额 {SPINE_BLOCK_QUOTA}）——补块，或对该主干声明 thin_spine: true + 非空 reason"
                ),
            ));
        }
    }

    // gd.edges.*：端点解析域 = 块表名集；三型 + 档位词表 + 三元组唯一。
    let Some(edges) = map.get("edges").and_then(Value::as_array) else {
        violations.push(ContractViolation::red("gd.edges.type", "edges 必须是数组"));
        return violations;
    };
    let mut seen_triples: Vec<(String, String, String)> = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        let at = format!("edges[{index}]");
        let Some(edge_map) = edge.as_object() else {
            violations.push(ContractViolation::red("gd.edges.type", format!("{at} 不是对象")));
            continue;
        };
        let source = edge_map.get("source").and_then(Value::as_str).unwrap_or_default().trim();
        let target = edge_map.get("target").and_then(Value::as_str).unwrap_or_default().trim();
        for (role, endpoint) in [("source", source), ("target", target)] {
            if endpoint.is_empty() {
                violations.push(ContractViolation::red("gd.edges.endpoints", format!("{at} 的 {role} 为空")));
            } else if !block_names.iter().any(|name| name == endpoint) {
                violations.push(ContractViolation::red(
                    "gd.edges.endpoints",
                    format!(
                        "{at} 的 {role}「{endpoint}」不在块表名集内——端点解析域就是块表，悬挂 = 红"
                    ),
                ));
            }
        }
        let edge_type = edge_map.get("type").and_then(Value::as_str).unwrap_or_default().trim();
        if !EDGE_TYPES.contains(&edge_type) {
            violations.push(ContractViolation::red(
                "gd.edges.type",
                format!("{at} 的 type 必须是 {} 之一", EDGE_TYPES.join("/")),
            ));
        }
        let tier = edge_map.get("tier").and_then(Value::as_str).unwrap_or_default().trim();
        if !EDGE_TIERS.contains(&tier) {
            violations.push(ContractViolation::red(
                "gd.edges.tier",
                format!("{at} 的 tier 必须是 {} 之一（assumes 边：source = 需求方、target = 前置，别写反）", EDGE_TIERS.join("/")),
            ));
        }
        let triple = (source.to_owned(), target.to_owned(), edge_type.to_owned());
        if seen_triples.iter().any(|seen| *seen == triple) {
            violations.push(ContractViolation::red(
                "gd.edges.dup",
                format!("{at} 与既有边重复（source, target, type 三元组必须唯一）"),
            ));
        } else {
            seen_triples.push(triple);
        }
    }

    // gd.gaps：盲区声明 ≥3。
    let Some(gaps) = map.get("gaps").and_then(Value::as_array) else {
        violations.push(ContractViolation::red("gd.gaps", "gaps 必须是数组"));
        return violations;
    };
    let gap_rows: Vec<(&str, &str)> = gaps
        .iter()
        .filter_map(Value::as_object)
        .map(|gap| {
            (
                gap.get("item").and_then(Value::as_str).unwrap_or_default().trim(),
                gap.get("direction").and_then(Value::as_str).unwrap_or_default().trim(),
            )
        })
        .collect();
    let usable_gaps = gap_rows.iter().filter(|(item, direction)| !item.is_empty() && !direction.is_empty()).count();
    if usable_gaps < MIN_GAPS {
        violations.push(ContractViolation::red(
            "gd.gaps",
            format!("gaps 至少 {MIN_GAPS} 条且 item/direction 均非空，有效条目 {usable_gaps} 条"),
        ));
    }

    // gd.quality：质量清单 ≥5，unmet 必带理由。
    let Some(quality) = map.get("quality").and_then(Value::as_array) else {
        violations.push(ContractViolation::red("gd.quality", "quality 必须是数组"));
        return violations;
    };
    if quality.len() < MIN_QUALITY {
        violations.push(ContractViolation::red(
            "gd.quality",
            format!("quality 至少 {MIN_QUALITY} 条，得到 {} 条", quality.len()),
        ));
    }
    for (index, entry) in quality.iter().enumerate() {
        let at = format!("quality[{index}]");
        let Some(entry_map) = entry.as_object() else {
            violations.push(ContractViolation::red("gd.quality", format!("{at} 不是对象")));
            continue;
        };
        let status = entry_map.get("status").and_then(Value::as_str).unwrap_or_default().trim();
        if !QUALITY_STATUS.contains(&status) {
            violations.push(ContractViolation::red(
                "gd.quality",
                format!("{at} 的 status 必须是 {} 之一", QUALITY_STATUS.join("/")),
            ));
        }
        let reason = entry_map.get("reason").and_then(Value::as_str).unwrap_or_default().trim();
        if status == "unmet" && reason.is_empty() {
            violations.push(ContractViolation::red(
                "gd.quality",
                format!("{at} 标记 unmet 必须附非空 reason"),
            ));
        }
    }

    // gd.no_schedule：排期红线扫全文（quality/disclaimer 元声明面豁免）。
    let mut strings = Vec::new();
    collect_strings(value, &["quality", "disclaimer"], &mut strings);
    for text in trim_strings(&strings) {
        if let Some(marker) = SCHEDULE_MARKERS.iter().find(|marker| text.contains(**marker)) {
            violations.push(ContractViolation::red(
                "gd.no_schedule",
                format!(
                    "出现排期语词「{marker}」：「{}」——层号只表依赖深度，全文不得出现顺序承诺与时间表（想表达「没有排期语义」的正确方式是什么都不写）",
                    truncate_for_report(text, 40)
                ),
            ));
        }
    }

    violations
}

fn validate_domain(domain: &Value) -> Vec<ContractViolation> {
    let Some(domain_map) = domain.as_object() else {
        return vec![ContractViolation::red("gd.domain.required", "domain 必须是对象")];
    };
    let mut violations = Vec::new();
    for key in ["core_definition", "audience", "granularity"] {
        let empty = domain_map
            .get(key)
            .and_then(Value::as_str)
            .map(|text| text.trim().is_empty())
            .unwrap_or(true);
        if empty {
            violations.push(ContractViolation::red(
                "gd.domain.required",
                format!("domain.{key} 缺失或为空"),
            ));
        }
    }
    for key in ["includes", "excludes"] {
        let items = string_array(domain, key);
        let usable = items.iter().filter(|item| !item.trim().is_empty()).count();
        if usable < 3 {
            violations.push(ContractViolation::red(
                "gd.domain.required",
                format!("domain.{key} 至少 3 条非空，得到 {usable} 条"),
            ));
        }
    }
    let scenarios = string_array(domain, "scenarios");
    let usable_scenarios = scenarios.iter().filter(|item| !item.trim().is_empty()).count();
    if !(2..=3).contains(&usable_scenarios) {
        violations.push(ContractViolation::red(
            "gd.domain.required",
            format!("domain.scenarios 须 2–3 条非空，得到 {usable_scenarios} 条"),
        ));
    }
    let doors = string_array(domain, "prerequisite_doors");
    let usable_doors: Vec<&str> = doors.iter().map(|door| door.trim()).filter(|door| !door.is_empty()).collect();
    if usable_doors.is_empty() {
        violations.push(ContractViolation::red(
            "gd.domain.doors",
            "domain.prerequisite_doors 至少 1 条——零基础衔接由前置地形产物承担，结构只引用其门口",
        ));
    }
    for door in usable_doors {
        if !door_id_valid(door) {
            violations.push(ContractViolation::red(
                "gd.domain.doors",
                format!("门口编号「{door}」必须是 G<数字> 形——只准引用前置地形产物定义的门口，不得自定义"),
            ));
        }
    }
    violations
}

fn truncate_for_report(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let head: String = text.chars().take(max).collect();
        format!("{head}…")
    }
}

/// 门口对账（g2 裁决：structure 的 prerequisite_doors 只准引用地形门口）：
/// 逐个编号必须能在地形的 doors 定义里解析，悬挂 = 红。
pub fn reconcile_doors(structure: &Value, terrain: &Value) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    let defined: Vec<String> = terrain
        .get("doors")
        .and_then(Value::as_array)
        .map(|doors| {
            doors
                .iter()
                .filter_map(|door| door.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let referenced = structure
        .get("domain")
        .map(|domain| string_array(domain, "prerequisite_doors"))
        .unwrap_or_default();
    for door in referenced {
        let door = door.trim();
        if door.is_empty() {
            continue;
        }
        if !defined.iter().any(|defined| defined == door) {
            violations.push(ContractViolation::red(
                "gd.domain.doors",
                format!(
                    "门口「{door}」在前置地形产物里无定义（已知：{}）——重生成两产物对齐门口编号",
                    if defined.is_empty() { "无".to_owned() } else { defined.join("、") }
                ),
            ));
        }
    }
    violations
}

// ── 前置地形契约 ────────────────────────────────────────────────────────────

/// terrain 顶层键白名单（allo 裁定的减配产物：标量回执 + 门口判据；learnhub
/// 的地形分层/DAG/词汇表节在 allo 无消费方，不进契约——消费面只有门口）。
pub const TERRAIN_TOP_KEYS: &[&str] = &[
    "goal",
    "learner_basis_assumed",
    "generated_by",
    "disclaimer",
    "doors",
];

pub const TERRAIN_SCALAR_KEYS: &[&str] =
    &["goal", "learner_basis_assumed", "generated_by", "disclaimer"];

/// 门口条目键白名单。
pub const DOOR_KEYS: &[&str] = &["id", "criterion", "kind"];
/// 门语义词表：hard = 无豁免硬前置；self_check = 可自检项。
pub const DOOR_KINDS: &[&str] = &["hard", "self_check"];
/// 门口最少条数。
pub const MIN_DOORS: usize = 1;

/// 前置地形产物契约（gd.terrain.*）：门口 G1..Gn 是结构 prerequisite_doors
/// 的唯一合法引用源，也是教练首批的入口层重点参照。
pub fn validate_terrain(value: &Value) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    let Some(map) = value.as_object() else {
        violations.push(ContractViolation::red(
            "gd.terrain.keys",
            "前置地形产物必须是单个 JSON 对象",
        ));
        return violations;
    };
    for key in map.keys() {
        if !TERRAIN_TOP_KEYS.contains(&key.as_str()) {
            violations.push(ContractViolation::red(
                "gd.terrain.keys",
                format!("未知顶层键「{key}」——地形产物只允许 {}", TERRAIN_TOP_KEYS.join("/")),
            ));
        }
    }
    for key in TERRAIN_SCALAR_KEYS {
        let empty = map
            .get(*key)
            .and_then(Value::as_str)
            .map(|text| text.trim().is_empty())
            .unwrap_or(true);
        if empty {
            violations.push(ContractViolation::red(
                "gd.terrain.frontmatter",
                format!("标量键「{key}」缺失或为空"),
            ));
        }
    }
    let Some(doors) = map.get("doors").and_then(Value::as_array) else {
        violations.push(ContractViolation::red(
            "gd.terrain.doors",
            "doors 必须是数组（G1..Gn 门口判据——结构引用的唯一合法源）",
        ));
        return violations;
    };
    if doors.len() < MIN_DOORS {
        violations.push(ContractViolation::red(
            "gd.terrain.doors",
            format!("至少 {MIN_DOORS} 条门口判据"),
        ));
    }
    let mut seen_ids: Vec<String> = Vec::new();
    for (index, door) in doors.iter().enumerate() {
        let at = format!("doors[{index}]");
        let Some(door_map) = door.as_object() else {
            violations.push(ContractViolation::red("gd.terrain.doors", format!("{at} 不是对象")));
            continue;
        };
        for key in door_map.keys() {
            if !DOOR_KEYS.contains(&key.as_str()) {
                violations.push(ContractViolation::red(
                    "gd.terrain.doors",
                    format!("{at} 含未知键「{key}」——门口只允许 {}", DOOR_KEYS.join("/")),
                ));
            }
        }
        let id = door_map.get("id").and_then(Value::as_str).unwrap_or_default().trim();
        if !door_id_valid(id) {
            violations.push(ContractViolation::red(
                "gd.terrain.doors",
                format!("{at} 的 id「{id}」必须是 G<数字> 形"),
            ));
        } else if seen_ids.iter().any(|seen| seen == id) {
            violations.push(ContractViolation::red("gd.terrain.doors", format!("门口编号「{id}」重复")));
        } else {
            seen_ids.push(id.to_owned());
        }
        let criterion = door_map.get("criterion").and_then(Value::as_str).unwrap_or_default().trim();
        if criterion.is_empty() {
            violations.push(ContractViolation::red(
                "gd.terrain.doors",
                format!("{at} 的 criterion（可验证判据）为空"),
            ));
        } else if !(criterion.contains('能') || criterion.contains('可')) {
            violations.push(ContractViolation::red(
                "gd.terrain.doors",
                format!(
                    "门口「{id}」的判据须可验证（给出可观察的能力动词：能/可…）：「{}」",
                    truncate_for_report(criterion, 40)
                ),
            ));
        }
        let kind = door_map.get("kind").and_then(Value::as_str).unwrap_or_default().trim();
        if !DOOR_KINDS.contains(&kind) {
            violations.push(ContractViolation::red(
                "gd.terrain.doors",
                format!("{at} 的 kind 必须是 {} 之一（hard = 无豁免硬前置，self_check = 可自检项）", DOOR_KINDS.join("/")),
            ));
        }
    }
    violations
}

// ── JSON Schema 单源（由同一份常量装配；schema↔validator 互钉测试锁定）─────

fn string_schema() -> Value {
    serde_json::json!({ "type": "string" })
}

fn non_empty_string_schema() -> Value {
    serde_json::json!({ "type": "string", "minLength": 1 })
}

fn string_array_schema(min: usize, max: Option<usize>) -> Value {
    let mut schema = serde_json::json!({
        "type": "array",
        "items": { "type": "string", "minLength": 1 },
        "minItems": min,
    });
    if let Some(max) = max {
        schema["maxItems"] = serde_json::json!(max);
    }
    schema
}

fn enum_schema(values: &[&str]) -> Value {
    serde_json::json!({ "enum": values })
}

/// structure 产物的 JSON Schema（词表/键集/数量档全部引自本模块常量——
/// 与 validator 单源；providers 只承诺 json_object 合法性，schema 是本地
/// 结构执法的单源声明与未来 json_schema 模式的预置件）。
pub fn structure_schema() -> Value {
    let domain_properties = serde_json::json!({
        "core_definition": non_empty_string_schema(),
        "includes": string_array_schema(3, None),
        "excludes": string_array_schema(3, None),
        "prerequisite_doors": {
            "type": "array",
            "items": { "type": "string", "pattern": "^G[0-9]+$" },
            "minItems": 1,
        },
        "audience": non_empty_string_schema(),
        "scenarios": string_array_schema(2, Some(3)),
        "granularity": non_empty_string_schema(),
    });
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": TOP_KEYS,
        "properties": {
            "goal": non_empty_string_schema(),
            "goal_type": enum_schema(GOAL_TYPES),
            "proficiency_statement": non_empty_string_schema(),
            "domain_type": enum_schema(DOMAIN_TYPES),
            "form_reference": non_empty_string_schema(),
            "source_tier": enum_schema(SOURCE_TIERS),
            "generated_by": non_empty_string_schema(),
            "disclaimer": non_empty_string_schema(),
            "domain": {
                "type": "object",
                "additionalProperties": false,
                "required": domain_properties.as_object().map(|properties| {
                    properties.keys().cloned().collect::<Vec<_>>()
                }).unwrap_or_default(),
                "properties": domain_properties,
            },
            "spines": {
                "type": "array",
                "minItems": SPINES_MIN,
                "maxItems": SPINES_MAX,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "definition", "key_points", "instance"],
                    "properties": {
                        "name": non_empty_string_schema(),
                        "definition": non_empty_string_schema(),
                        "key_points": string_array_schema(1, None),
                        "instance": non_empty_string_schema(),
                        "thin_spine": { "type": "boolean" },
                        "reason": { "type": "string" },
                    },
                },
            },
            "blocks": {
                "type": "array",
                "minItems": BLOCKS_MIN,
                "maxItems": BLOCKS_MAX,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": BLOCK_KEYS,
                    "properties": {
                        "name": non_empty_string_schema(),
                        "slug": { "type": "string", "pattern": "^[a-z0-9-]+$" },
                        "spine": non_empty_string_schema(),
                        "stage": enum_schema(STAGES),
                        "note": non_empty_string_schema(),
                    },
                },
            },
            "edges": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["source", "target", "type", "tier"],
                    "properties": {
                        "source": non_empty_string_schema(),
                        "target": non_empty_string_schema(),
                        "type": enum_schema(EDGE_TYPES),
                        "tier": enum_schema(EDGE_TIERS),
                        "note": string_schema(),
                    },
                },
            },
            "gaps": {
                "type": "array",
                "minItems": MIN_GAPS,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["item", "direction"],
                    "properties": {
                        "item": non_empty_string_schema(),
                        "direction": non_empty_string_schema(),
                    },
                },
            },
            "quality": {
                "type": "array",
                "minItems": MIN_QUALITY,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["check", "status"],
                    "properties": {
                        "check": non_empty_string_schema(),
                        "status": enum_schema(QUALITY_STATUS),
                        "reason": { "type": "string" },
                    },
                },
            },
        },
    })
}

/// 前置地形产物的 JSON Schema（同款单源）。
pub fn terrain_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": TERRAIN_TOP_KEYS,
        "properties": {
            "goal": non_empty_string_schema(),
            "learner_basis_assumed": non_empty_string_schema(),
            "generated_by": non_empty_string_schema(),
            "disclaimer": non_empty_string_schema(),
            "doors": {
                "type": "array",
                "minItems": MIN_DOORS,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": DOOR_KEYS,
                    "properties": {
                        "id": { "type": "string", "pattern": "^G[0-9]+$" },
                        "criterion": non_empty_string_schema(),
                        "kind": enum_schema(DOOR_KINDS),
                    },
                },
            },
        },
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 一份恰好过门的 structure 样本（40 块 / 6 主干 / 每干 ≥5 块）。
    fn golden_structure() -> Value {
        let spine_names = ["主干一", "主干二", "主干三", "主干四", "主干五", "主干六"];
        let mut blocks = Vec::new();
        for (spine_index, spine) in spine_names.iter().enumerate() {
            for block_index in 0..7 {
                let ordinal = spine_index * 7 + block_index;
                blocks.push(json!({
                    "name": format!("块{ordinal:02}"),
                    "slug": format!("block-{ordinal:02}"),
                    "spine": spine,
                    "stage": STAGES[ordinal % STAGES.len()],
                    "note": format!("第 {ordinal} 个主题的一句话描述"),
                }));
            }
        }
        json!({
            "goal": "能独立完成一元二次方程的求解与检验",
            "goal_type": "capability",
            "proficiency_statement": "达成后能独立解任意一元二次方程并讲解每一步依据",
            "domain_type": "理论",
            "form_reference": "Flowy 学习结构模板 v1",
            "source_tier": "名录",
            "generated_by": "test-model",
            "disclaimer": "快照为生成时点参考地图，可能存在盲区",
            "domain": {
                "core_definition": "一元二次方程的代数求解体系",
                "includes": ["方程变形", "求根公式", "判别式"],
                "excludes": ["三次方程", "复数域", "数值分析"],
                "prerequisite_doors": ["G1"],
                "audience": "零基础成年学习者",
                "scenarios": ["解题应用", "讲解他人"],
                "granularity": "每块一个可独立教学的解题动作",
            },
            "spines": spine_names.iter().map(|spine| json!({
                "name": spine,
                "definition": format!("{spine}的一句话定义"),
                "key_points": ["要点一"],
                "instance": format!("达标{spine}时能独立完成一次求解"),
            })).collect::<Vec<_>>(),
            "blocks": blocks,
            "edges": [
                { "source": "块01", "target": "块00", "type": "assumes", "tier": "know", "note": "先认后用" },
                { "source": "块08", "target": "块09", "type": "composes", "tier": "use" },
                { "source": "块15", "target": "块16", "type": "contrasts", "tier": "know" },
            ],
            "gaps": [
                { "item": "应用题情境覆盖不足", "direction": "补行程/工程类情境" },
                { "item": "图像法未展开", "direction": "补抛物线图像判读" },
                { "item": "近似解法缺位", "direction": "补估算与验证策略" },
            ],
            "quality": [
                { "check": "块名一物一名", "status": "met" },
                { "check": "每干配额", "status": "met" },
                { "check": "边端点全解析", "status": "met" },
                { "check": "入门档从具体对象起步", "status": "met" },
                { "check": "覆盖全部主干方向", "status": "unmet", "reason": "应用情境块待补" },
            ],
        })
    }

    fn golden_terrain() -> Value {
        json!({
            "goal": "能独立完成一元二次方程的求解与检验",
            "learner_basis_assumed": "四则运算熟练，未接触过字母代数",
            "generated_by": "test-model",
            "disclaimer": "门口判据为生成时点快照，供自检参考",
            "doors": [
                { "id": "G1", "criterion": "能用配方法解形如 x²=p 的方程", "kind": "hard" },
                { "id": "G2", "criterion": "可辨认等式两边同乘同除的合法性", "kind": "self_check" },
            ],
        })
    }

    fn codes(violations: &[ContractViolation]) -> Vec<&'static str> {
        violations.iter().map(|violation| violation.code).collect()
    }

    #[test]
    fn golden_samples_pass_both_contracts() {
        let structure = golden_structure();
        assert!(validate_structure(&structure).is_empty(), "{:?}", validate_structure(&structure));
        let terrain = golden_terrain();
        assert!(validate_terrain(&terrain).is_empty(), "{:?}", validate_terrain(&terrain));
        assert!(reconcile_doors(&structure, &terrain).is_empty());
    }

    #[test]
    fn gd_parse_rejects_non_object() {
        let violations = validate_structure(&json!(["not", "an", "object"]));
        assert_eq!(codes(&violations), vec!["gd.parse"]);
    }

    #[test]
    fn gd_keys_enforces_whitelist_and_structural_required() {
        let mut value = golden_structure();
        value.as_object_mut().unwrap().insert("surprise".into(), json!(1));
        value.as_object_mut().unwrap().remove("gaps");
        let violations = validate_structure(&value);
        let codes = codes(&violations);
        assert!(codes.iter().filter(|code| **code == "gd.keys").count() == 2, "{violations:?}");
    }

    #[test]
    fn gd_frontmatter_enforces_scalars_and_word_lists() {
        let mut value = golden_structure();
        let map = value.as_object_mut().unwrap();
        map.insert("goal_type".into(), json!("vibes"));
        map.insert("domain_type".into(), json!("玄学"));
        map.insert("generated_by".into(), json!("  "));
        let violations = validate_structure(&value);
        let codes = codes(&violations);
        assert_eq!(codes.iter().filter(|code| **code == "gd.frontmatter").count(), 3);
    }

    #[test]
    fn gd_domain_enforces_counts_and_door_form() {
        let mut value = golden_structure();
        value["domain"]["includes"] = json!(["只剩一条"]);
        value["domain"]["scenarios"] = json!(["一条"]);
        value["domain"]["prerequisite_doors"] = json!(["门1", "Gx"]);
        let violations = validate_structure(&value);
        let codes = codes(&violations);
        assert!(codes.contains(&"gd.domain.required"), "{violations:?}");
        assert_eq!(codes.iter().filter(|code| **code == "gd.domain.doors").count(), 2);
    }

    #[test]
    fn gd_spines_enforce_count_bounds() {
        let mut value = golden_structure();
        value["spines"] = json!([]);
        let violations = validate_structure(&value);
        assert!(codes(&violations).contains(&"gd.spines.count"));
    }

    #[test]
    fn gd_blocks_reject_judgement_shaped_fields() {
        let mut value = golden_structure();
        value["blocks"][0]["definition"] = json!("判据形字段禁止进块");
        let violations = validate_structure(&value);
        assert!(codes(&violations).contains(&"gd.blocks.fields"), "{violations:?}");
    }

    #[test]
    fn gd_blocks_count_warns_over_soft_max_and_rejects_beyond_hard_max() {
        let mut value = golden_structure();
        {
            let blocks = value["blocks"].as_array_mut().unwrap();
            let extra: Vec<Value> = (42..84)
                .map(|ordinal| {
                    json!({
                        "name": format!("块{ordinal:02}"),
                        "slug": format!("block-{ordinal:02}"),
                        "spine": "主干一",
                        "stage": "入门",
                        "note": "越档补块",
                    })
                })
                .collect();
            blocks.extend(extra);
        }
        let violations = validate_structure(&value);
        assert!(
            violations.iter().any(|violation| violation.code == "gd.blocks.count" && violation.warning),
            "{violations:?}"
        );

        let mut beyond = golden_structure();
        {
            let blocks = beyond["blocks"].as_array_mut().unwrap();
            let extra: Vec<Value> = (42..104)
                .map(|ordinal| {
                    json!({
                        "name": format!("块{ordinal:03}"),
                        "slug": format!("block-{ordinal:03}"),
                        "spine": "主干一",
                        "stage": "入门",
                        "note": "超硬上限",
                    })
                })
                .collect();
            blocks.extend(extra);
        }
        let violations = validate_structure(&beyond);
        assert!(
            violations.iter().any(|violation| violation.code == "gd.blocks.count" && !violation.warning),
            "{violations:?}"
        );
    }

    #[test]
    fn gd_blocks_one_name_exempts_quoted_terms_and_word_edge_he() {
        // 改名块[2]（边不引用它——边端点解析是另一条款，不混入本判据）。
        let mut value = golden_structure();
        value["blocks"][2]["name"] = json!("等差前n项和与求和公式");
        let violations = validate_structure(&value);
        assert!(codes(&violations).contains(&"gd.blocks.one_name"), "{violations:?}");

        let mut quoted = golden_structure();
        quoted["blocks"][2]["name"] = json!("「广义表与树」的辨析");
        assert!(
            !codes(&validate_structure(&quoted)).contains(&"gd.blocks.one_name"),
            "「」段内并列豁免: {:?}",
            validate_structure(&quoted)
        );

        let mut word_edge = golden_structure();
        word_edge["blocks"][2]["name"] = json!("等差数列前n项和");
        assert!(validate_structure(&word_edge).is_empty(), "词尾「和」是名词: {:?}", validate_structure(&word_edge));
    }

    #[test]
    fn gd_blocks_slug_and_spine_ref_are_enforced() {
        let mut value = golden_structure();
        value["blocks"][0]["slug"] = json!("Bad Slug");
        value["blocks"][1]["spine"] = json!("不存在的主干");
        let violations = validate_structure(&value);
        let codes = codes(&violations);
        assert!(codes.contains(&"gd.blocks.slug"), "{violations:?}");
        assert!(codes.contains(&"gd.blocks.spine_ref"));
    }

    #[test]
    fn gd_blocks_quota_requires_thin_spine_exemption() {
        let mut value = golden_structure();
        for block in value["blocks"].as_array_mut().unwrap().iter_mut().skip(4) {
            if block["spine"] == json!("主干一") {
                *block = json!({
                    "name": "挪走块".to_owned() + block["name"].as_str().unwrap(),
                    "slug": block["slug"].as_str().unwrap().to_owned() + "-moved",
                    "spine": "主干二",
                    "stage": "入门",
                    "note": "挪走凑配额",
                });
            }
        }
        let violations = validate_structure(&value);
        assert!(codes(&violations).contains(&"gd.blocks.quota"), "{violations:?}");

        let mut exempt = value.clone();
        for spine in exempt["spines"].as_array_mut().unwrap() {
            if spine["name"] == json!("主干一") {
                spine["thin_spine"] = json!(true);
                spine["reason"] = json!("该方向只有两个独立教学主题");
            }
        }
        assert!(
            !codes(&validate_structure(&exempt)).contains(&"gd.blocks.quota"),
            "薄干声明豁免: {:?}",
            validate_structure(&exempt)
        );
    }

    #[test]
    fn gd_edges_resolve_against_block_names_only() {
        let mut value = golden_structure();
        value["edges"][0]["target"] = json!("不在块表的边端点");
        value["edges"][1]["tier"] = json!("apply");
        value["edges"][2]["type"] = json!("requires");
        let violations = validate_structure(&value);
        let codes = codes(&violations);
        assert!(codes.contains(&"gd.edges.endpoints"), "{violations:?}");
        assert!(codes.contains(&"gd.edges.tier"));
        assert!(codes.contains(&"gd.edges.type"));
    }

    #[test]
    fn gd_gaps_and_quality_have_minimums() {
        let mut value = golden_structure();
        value["gaps"] = json!([{ "item": "唯一一条", "direction": "补" }]);
        value["quality"] = json!([
            { "check": "一", "status": "met" },
            { "check": "二", "status": "met" },
            { "check": "三", "status": "unmet" },
        ]);
        let violations = validate_structure(&value);
        let codes = codes(&violations);
        assert!(codes.contains(&"gd.gaps"), "{violations:?}");
        assert!(codes.contains(&"gd.quality"));
        assert!(violations.iter().any(|violation| violation.code == "gd.quality"
            && violation.message.contains("unmet 必须附非空 reason")));
    }

    #[test]
    fn gd_no_schedule_scans_all_text_but_exempt_meta_sections() {
        let mut value = golden_structure();
        value["spines"][0]["definition"] = json!("学习计划的第一主干");
        let violations = validate_structure(&value);
        assert!(codes(&violations).contains(&"gd.no_schedule"), "{violations:?}");

        let mut meta = golden_structure();
        meta["disclaimer"] = json!("本快照无排期语义");
        meta["quality"][0]["check"] = json!("全文无排期语词");
        assert!(
            !codes(&validate_structure(&meta)).contains(&"gd.no_schedule"),
            "quality/disclaimer 元声明面豁免: {:?}",
            validate_structure(&meta)
        );
    }

    #[test]
    fn terrain_contract_enforces_doors_and_scalars() {
        assert!(validate_terrain(&golden_terrain()).is_empty());

        let mut value = golden_terrain();
        value["doors"] = json!([
            { "id": "G1", "criterion": "熟练掌握配方的深层奥秘", "kind": "hard" },
            { "id": "G1", "criterion": "能解方程", "kind": "hard" },
            { "id": "Gx", "criterion": "能解方程", "kind": "hard" },
            { "id": "G2", "criterion": "能解方程", "kind": "sometimes" },
        ]);
        value["learner_basis_assumed"] = json!("");
        let violations = validate_terrain(&value);
        let codes = codes(&violations);
        assert!(codes.iter().filter(|code| **code == "gd.terrain.doors").count() >= 4, "{violations:?}");
        assert!(codes.contains(&"gd.terrain.frontmatter"));
    }

    #[test]
    fn reconcile_doors_reports_dangling_references_with_known_ids() {
        let mut structure = golden_structure();
        structure["domain"]["prerequisite_doors"] = json!(["G1", "G9"]);
        let violations = reconcile_doors(&structure, &golden_terrain());
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert_eq!(violations[0].code, "gd.domain.doors");
        assert!(violations[0].message.contains("G9") && violations[0].message.contains("G1、G2"));
    }

    /// 互钉：schema 的词表/键集/数量档与 validator 常量同源——schema 不是
    /// 手抄的第二份事实。
    #[test]
    fn schema_is_pinned_to_validator_constants() {
        let schema = structure_schema();
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(schema["required"].as_array().unwrap().len(), TOP_KEYS.len());
        assert_eq!(properties["goal_type"]["enum"], json!(GOAL_TYPES));
        assert_eq!(properties["domain_type"]["enum"], json!(DOMAIN_TYPES));
        assert_eq!(properties["source_tier"]["enum"], json!(SOURCE_TIERS));
        assert_eq!(properties["domain"]["properties"]["prerequisite_doors"]["items"]["pattern"], json!("^G[0-9]+$"));
        assert_eq!(properties["spines"]["minItems"], json!(SPINES_MIN));
        assert_eq!(properties["spines"]["maxItems"], json!(SPINES_MAX));
        assert_eq!(properties["blocks"]["minItems"], json!(BLOCKS_MIN));
        assert_eq!(properties["blocks"]["maxItems"], json!(BLOCKS_MAX));
        assert_eq!(properties["blocks"]["items"]["required"], json!(BLOCK_KEYS));
        assert_eq!(properties["blocks"]["items"]["properties"]["stage"]["enum"], json!(STAGES));
        assert_eq!(properties["edges"]["items"]["properties"]["type"]["enum"], json!(EDGE_TYPES));
        assert_eq!(properties["edges"]["items"]["properties"]["tier"]["enum"], json!(EDGE_TIERS));
        assert_eq!(properties["gaps"]["minItems"], json!(MIN_GAPS));
        assert_eq!(properties["quality"]["minItems"], json!(MIN_QUALITY));
        assert_eq!(properties["quality"]["items"]["properties"]["status"]["enum"], json!(QUALITY_STATUS));

        let terrain = terrain_schema();
        assert_eq!(terrain["required"], json!(TERRAIN_TOP_KEYS));
        assert_eq!(terrain["properties"]["doors"]["items"]["required"], json!(DOOR_KEYS));
        assert_eq!(terrain["properties"]["doors"]["items"]["properties"]["kind"]["enum"], json!(DOOR_KINDS));
        assert_eq!(terrain["properties"]["doors"]["minItems"], json!(MIN_DOORS));
    }

    /// 金样反钉：golden 样本必须逐键落在 schema 的 required 集合内（schema
    /// 漏键时此测试先红）。
    #[test]
    fn golden_samples_satisfy_their_schemas_required_sets() {
        for (value, schema) in [(golden_structure(), structure_schema()), (golden_terrain(), terrain_schema())] {
            let required: Vec<&str> = schema["required"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            for key in required {
                assert!(value.get(key).is_some(), "schema requires「{key}」but golden sample lacks it");
            }
        }
    }
}
