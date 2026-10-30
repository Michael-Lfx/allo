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

/// 块条目必填键（五键纪律：禁六件套与任何判据形字段——块不是概念）。
pub const BLOCK_KEYS: &[&str] = &["name", "slug", "spine", "stage", "note"];

/// 块条目唯一可选键（m1，learnhub #165 契约 v2.14 门位标签）：覆盖本块主题
/// 的门口编号（G-id 数组，逐字取自 domain.prerequisite_doors 声明的门口）；
/// 语义 =「学习者能满足这些门口判据，就必然已会本块」——宁缺勿滥。
pub const BLOCK_OPTIONAL_KEYS: &[&str] = &["door_refs"];

/// 块条目键白名单 = 必填五键 + 唯一可选键。
pub fn block_key_allowed(key: &str) -> bool {
    BLOCK_KEYS.contains(&key) || BLOCK_OPTIONAL_KEYS.contains(&key)
}

/// 主干条目键白名单（thin_spine + reason = 每干块数不足的豁免声明）。
pub const SPINE_KEYS: &[&str] = &["name", "definition", "key_points", "instance", "thin_spine", "reason"];

/// 难度档词表（顺序即矩阵列序）。
pub const STAGES: &[&str] = &["入门", "进阶", "高级"];

/// 阶段排序权重（词表单源：未知值排最后）——矩阵分格与参考材料呈现序共用。
pub fn stage_rank(stage: &str) -> usize {
    STAGES
        .iter()
        .position(|known| *known == stage)
        .unwrap_or(STAGES.len())
}
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

/// 块表量级档：硬下限 / 密度参考档（低于即告警）/ 越档线（告警）/ 硬上限。
/// f2 三档制（A1）：25 以下是结构非法（红）；25–39「低于 40 参考档」只警告
/// ——小目标课如实精简，密度抽验兜底（E2E 两轮对照 31 条与 51 条内容密度
/// 无差，硬凑 40 产出「底数与指数」类边缘块）；80–100 越档警告不变。
pub const BLOCKS_MIN: usize = 25;
pub const BLOCKS_DENSITY_FLOOR: usize = 40;
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
    } else if blocks.len() < BLOCKS_DENSITY_FLOOR {
        // f2（A1）：25–39 只警告不拒收——小目标课如实精简，密度抽验兜底；
        // 警告不进重试反馈（A2：不触发重试，预算让给真实违约）。
        violations.push(ContractViolation::warn(
            "gd.blocks.count",
            format!(
                "blocks {} 条低于 {BLOCKS_DENSITY_FLOOR} 参考档——小目标课如实精简，密度抽验兜底",
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
    // 块名 → stage 档位序（t6 gd.stages.ordering 的判读面；stage 无效的红条
    // 另发，这里只收合法档）。
    let mut block_stage: HashMap<String, usize> = HashMap::new();
    for (index, block) in blocks.iter().enumerate() {
        let at = format!("blocks[{index}]");
        let Some(block_map) = block.as_object() else {
            violations.push(ContractViolation::red("gd.blocks.fields", format!("{at} 不是对象")));
            continue;
        };
        for key in block_map.keys() {
            if !block_key_allowed(key) {
                violations.push(ContractViolation::red(
                    "gd.blocks.fields",
                    format!(
                        "{at} 含禁入键「{key}」——块只有五键 {} 加唯一可选键 door_refs，判据形字段一律不进块（块不是概念）",
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
                // f2（A3）：出路指引补「改用无并列的规范名」半句（learnhub 同
                // 款先例）——报全逻辑不动，打地鼠走提示词交卷自检句压制。
                format!(
                    "块名「{name}」含并列标记「{mark}」——一条 = 一个独立教学主题，多个主题须拆条；官方术语本身含并列形的以「」包裹，或改用无并列的规范名"
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
        } else {
            block_stage.insert(name.to_owned(), STAGES.iter().position(|candidate| candidate == &stage).unwrap_or(0));
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
        // gd.blocks.door_refs（m1，learnhub #165 契约 v2.14 门位标签）：形态
        // G<数字> + 块内去重；跨产物解析（引用 ⊆ 地形门口产出 且 ⊆ 本课
        // 声明课程门）归对账面 reconcile_doors。
        let door_refs = string_array(block, "door_refs");
        let mut seen_block_doors: Vec<&str> = Vec::new();
        for (door_index, door) in door_refs.iter().enumerate() {
            let door = door.trim();
            if !door_id_valid(door) {
                violations.push(ContractViolation::red(
                    "gd.blocks.door_refs",
                    format!(
                        "{at} 的 door_refs[{door_index}]「{door}」必须是 G<数字> 形——门位标签只准引用前置地形产物定义的门口"
                    ),
                ));
                continue;
            }
            if seen_block_doors.contains(&door) {
                violations.push(ContractViolation::red(
                    "gd.blocks.door_refs",
                    format!("{at} 的 door_refs[{door_index}]「{door}」块内重复引用"),
                ));
            } else {
                seen_block_doors.push(door);
            }
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
        // gd.stages.ordering（t6，warn 级）：assumes 边的 target（前置）块
        // stage 不得高于 source（需求方）块——入门块依赖进阶/高级块与「入门
        // 档贴地起排」的门位口径冲突（2026-10-19 生长节点专家审计：位权概
        // 念案）。档位错位是质量问题不是结构非法：warn 只留痕（修补回合/
        // 评审的输入面），不触发重试。
        if edge_type == "assumes"
            && let (Some(source_stage), Some(target_stage)) =
                (block_stage.get(source), block_stage.get(target))
            && target_stage > source_stage
        {
            violations.push(ContractViolation::warn(
                "gd.stages.ordering",
                format!(
                    "assumes 边「{source}」({}) 的前置「{target}」({}) 档位更高——入门档不贴门口起排；升 source 档或把前置拆成低档块",
                    STAGES[*source_stage], STAGES[*target_stage],
                ),
            ));
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

/// 门口对账（g2 裁决：structure 的 prerequisite_doors 只准引用地形门口；
/// m1 扩叉积——learnhub #165 `check_door_references` 同款）：①课程门逐个
/// 编号必须能在地形的 doors 定义里解析；②块级 door_refs 门位标签逐条解析
/// 进地形门口集（悬空 = 红）**且** ⊆ 本课声明课程门（越出 = 永不满足的
/// 死标签，红）。悬挂/越出消息都指名（编号 + 已知清单），走既有反馈重试
/// 回路，零新机制。
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
    let known = if defined.is_empty() { "无".to_owned() } else { defined.join("、") };
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
                    "门口「{door}」在前置地形产物里无定义（已知：{known}）——重生成两产物对齐门口编号"
                ),
            ));
        }
    }
    // 块级门位标签叉积（m1）：引用 ⊆ 地形门口产出 且 ⊆ 课程门。
    let gate: Vec<String> = structure
        .get("domain")
        .map(|domain| {
            string_array(domain, "prerequisite_doors")
                .into_iter()
                .map(|door| door.trim().to_owned())
                .filter(|door| !door.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let block_name = |block: &Value| -> String {
        block
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("(未命名块)")
            .to_owned()
    };
    for block in structure
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| blocks.as_slice())
        .unwrap_or_default()
    {
        for door in string_array(block, "door_refs") {
            let door = door.trim();
            if door.is_empty() {
                continue;
            }
            if !defined.iter().any(|defined| defined == door) {
                violations.push(ContractViolation::red(
                    "gd.blocks.door_refs",
                    format!(
                        "块「{}」的门位标签「{door}」在前置地形产物里无定义（已知：{known}）——重生成两产物对齐门口编号",
                        block_name(block)
                    ),
                ));
            } else if !gate.iter().any(|gate| gate == door) {
                violations.push(ContractViolation::red(
                    "gd.blocks.door_refs",
                    format!(
                        "块「{}」的门位标签「{door}」越出本课声明的课程门（{}）——门位标签只能引用 prerequisite_doors 里声明的门口，越出即永不满足的死标签",
                        block_name(block),
                        if gate.is_empty() { "空".to_owned() } else { gate.join("、") }
                    ),
                ));
            }
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
                        // 唯一可选键（m1 门位标签）：G-id 数组，形态同课程门。
                        "door_refs": {
                            "type": "array",
                            "items": { "type": "string", "pattern": "^G[0-9]+$" },
                        },
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

    /// 一份恰好过门的 structure 样本（42 块 / 6 主干 / 每干 7 块）。
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

    /// t6：入门块 assumes 进阶块 = 档位倒挂，warn 级 gd.stages.ordering 留
    /// 痕（不红——档位错位是质量问题不是结构非法；2026-10-19 审计位权概
    /// 念案）。反向（进阶依赖入门）与升档后同向不告警。
    #[test]
    fn gd_stages_ordering_warns_on_inverted_assumes_only() {
        let mut inverted = golden_structure();
        {
            let blocks = inverted.get_mut("blocks").unwrap().as_array_mut().unwrap();
            blocks[0]["stage"] = json!("入门");
            blocks[1]["stage"] = json!("进阶");
            let edges = inverted.get_mut("edges").unwrap().as_array_mut().unwrap();
            edges[0] = json!({ "source": "块00", "target": "块01", "type": "assumes", "tier": "know", "note": "倒挂" });
        }
        let violations = validate_structure(&inverted);
        assert!(
            violations.iter().any(|violation| violation.code == "gd.stages.ordering" && violation.warning),
            "{violations:?}"
        );
        assert!(
            violations.iter().all(|violation| violation.warning),
            "stage 倒挂只 warn 不红：{violations:?}"
        );
        // 反向（进阶依赖入门）合法；同档不告警；composes/contrasts 不受档位约束。
        let mut aligned = golden_structure();
        {
            let blocks = aligned.get_mut("blocks").unwrap().as_array_mut().unwrap();
            blocks[0]["stage"] = json!("入门");
            blocks[1]["stage"] = json!("进阶");
            let edges = aligned.get_mut("edges").unwrap().as_array_mut().unwrap();
            edges[0] = json!({ "source": "块01", "target": "块00", "type": "assumes", "tier": "know", "note": "同向" });
        }
        assert!(validate_structure(&aligned).is_empty(), "{:?}", validate_structure(&aligned));
        let mut same_stage = golden_structure();
        {
            let blocks = same_stage.get_mut("blocks").unwrap().as_array_mut().unwrap();
            blocks[0]["stage"] = json!("入门");
            blocks[1]["stage"] = json!("入门");
            let edges = same_stage.get_mut("edges").unwrap().as_array_mut().unwrap();
            edges[0] = json!({ "source": "块00", "target": "块01", "type": "assumes", "tier": "know", "note": "同档" });
        }
        assert!(validate_structure(&same_stage).is_empty(), "{:?}", validate_structure(&same_stage));
        let mut composes = golden_structure();
        {
            let blocks = composes.get_mut("blocks").unwrap().as_array_mut().unwrap();
            blocks[0]["stage"] = json!("入门");
            blocks[1]["stage"] = json!("高级");
            let edges = composes.get_mut("edges").unwrap().as_array_mut().unwrap();
            edges[0] = json!({ "source": "块00", "target": "块01", "type": "composes", "tier": "use", "note": "组装不限档" });
        }
        assert!(validate_structure(&composes).is_empty(), "{:?}", validate_structure(&composes));
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

    /// f2（① 防措辞回归红样，蓝图 4.2/A4）：注入行是 `- [G1]（硬前置）判据`
    /// 的人读形状——措辞回归（模型把整行照抄进门口字段）必须仍被拒。整行作
    /// door 字符串：形态臂（非 G<数字> 形）与对账臂（地形门口集解析）双红。
    #[test]
    fn whole_injection_line_as_door_string_stays_red() {
        let injected = "- [G1]（硬前置）能用配方法解形如 x²=p 的方程";

        // 课程门臂：gd.domain.doors 形态红，逐字点名整行。
        let mut gates = golden_structure();
        gates["domain"]["prerequisite_doors"] = json!([injected]);
        let violations = validate_structure(&gates);
        let doors: Vec<&ContractViolation> = violations
            .iter()
            .filter(|violation| violation.code == "gd.domain.doors" && !violation.warning)
            .collect();
        assert_eq!(doors.len(), 1, "整行照抄恰红一条: {violations:?}");
        assert!(
            doors[0].message.contains("G<数字>") && doors[0].message.contains(injected),
            "形态臂点名整行: {}",
            doors[0].message
        );

        // 对账臂：整行解析不进地形门口集——悬空红。
        let violations = reconcile_doors(&gates, &golden_terrain());
        assert!(
            violations.iter().any(|violation| violation.code == "gd.domain.doors"
                && !violation.warning
                && violation.message.contains(injected)),
            "对账臂悬空红: {violations:?}"
        );

        // 块级门位标签臂（m1）：整行进 door_refs——形态红 + 对账悬空红。
        let mut refs = golden_structure();
        refs["blocks"][0]["door_refs"] = json!([injected]);
        let violations = validate_structure(&refs);
        assert!(
            violations.iter().any(|violation| violation.code == "gd.blocks.door_refs"
                && !violation.warning),
            "door_refs 形态红: {violations:?}"
        );
        let violations = reconcile_doors(&refs, &golden_terrain());
        assert!(
            violations.iter().any(|violation| violation.code == "gd.blocks.door_refs"
                && !violation.warning
                && violation.message.contains(injected)),
            "door_refs 对账悬空红: {violations:?}"
        );
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

    /// 把金样块表替换为恰 n 条（按主干轮转分布），不足配额的主干声明薄干
    /// 豁免——除 gd.blocks.count 外零杂音，专供下限/参考档边界考察。
    fn structure_with_block_count(n: usize) -> Value {
        let mut value = golden_structure();
        let spine_names = ["主干一", "主干二", "主干三", "主干四", "主干五", "主干六"];
        let blocks: Vec<Value> = (0..n)
            .map(|ordinal| {
                json!({
                    "name": format!("块{ordinal:02}"),
                    "slug": format!("block-{ordinal:02}"),
                    "spine": spine_names[ordinal % spine_names.len()],
                    "stage": STAGES[ordinal % STAGES.len()],
                    "note": format!("第 {ordinal} 个主题的一句话描述"),
                })
            })
            .collect();
        value["blocks"] = json!(blocks);
        let mut per_spine = [0usize; 6];
        for ordinal in 0..n {
            per_spine[ordinal % spine_names.len()] += 1;
        }
        for (index, spine) in value["spines"].as_array_mut().unwrap().iter_mut().enumerate() {
            if per_spine[index] < SPINE_BLOCK_QUOTA {
                spine["thin_spine"] = json!(true);
                spine["reason"] = json!("该方向只分出这些独立教学主题");
            }
        }
        value
    }

    /// f2（③ 下限三档，A1）：25 红 / 40 参考档 warn / 100 红三档——下限边界
    /// 24 红、25–39「低于 40 参考档」警告过闸不拒收（31 块小目标课复验零红
    /// 条）；84 越档警告与 104 硬上限红照旧。schema minItems 与常量互钉
    /// （schema_is_pinned_to_validator_constants）自动跟随，零改。
    #[test]
    fn gd_blocks_count_three_tiers_boundary() {
        // 24：不足下限 25，红。
        let under = validate_structure(&structure_with_block_count(24));
        let violation = under
            .iter()
            .find(|violation| violation.code == "gd.blocks.count")
            .expect("24 blocks must be flagged");
        assert!(!violation.warning, "{under:?}");
        assert!(violation.message.contains("不足下限 25"), "{}", violation.message);

        // 25 / 31 / 39：参考档下带 warn 过闸（不拒收），且除该警告外零违约
        // （31 块小目标课红条消失，smoke structure 档放行的单测级判据）。
        for n in [25, 31, 39] {
            let violations = validate_structure(&structure_with_block_count(n));
            let violation = violations
                .iter()
                .find(|violation| violation.code == "gd.blocks.count")
                .unwrap_or_else(|| panic!("{n} blocks must hit the density floor"));
            assert!(violation.warning, "{n}: {violations:?}");
            assert!(
                violation.message.contains("低于 40 参考档"),
                "{n}: {}",
                violation.message
            );
            assert!(
                violations.iter().all(|violation| violation.warning),
                "{n} 块零红条: {violations:?}"
            );
        }

        // 84：越档警告（既有档不变）；104：硬上限红。
        let soft = validate_structure(&structure_with_block_count(84));
        assert!(
            soft.iter().any(|violation| violation.code == "gd.blocks.count" && violation.warning),
            "{soft:?}"
        );
        let beyond = validate_structure(&structure_with_block_count(104));
        assert!(
            beyond
                .iter()
                .any(|violation| violation.code == "gd.blocks.count" && !violation.warning),
            "{beyond:?}"
        );
    }

    /// f2（② 打地鼠防回归，A3）：多个块名同含并列标记时校验器**一次报全**
    /// ——报告逐块一行、各提一名（被点名的与别处新冒的同在这一份报告里，
    /// 模型无需逐轮试探）；违约消息带出路指引（拆条 / 「」包裹 / 改规范名）。
    #[test]
    fn gd_blocks_one_name_reports_all_offenders_with_guidance_in_one_pass() {
        let mut value = golden_structure();
        // 边端点不引用块02/块05——改名不牵动 gd.edges.endpoints，判据不混。
        value["blocks"][2]["name"] = json!("百分数与利率的对应");
        value["blocks"][5]["name"] = json!("底数与指数");
        let violations = validate_structure(&value);
        let one_name: Vec<&ContractViolation> = violations
            .iter()
            .filter(|violation| violation.code == "gd.blocks.one_name")
            .collect();
        assert_eq!(one_name.len(), 2, "一次报全: {violations:?}");
        assert!(
            one_name.iter().any(|violation| violation.message.contains("百分数与利率的对应"))
                && one_name.iter().any(|violation| violation.message.contains("底数与指数")),
            "每行各提一名: {:?}",
            one_name
        );
        for violation in &one_name {
            assert!(
                violation.message.contains("多个主题须拆条")
                    && violation.message.contains("或改用无并列的规范名"),
                "出路指引在每行: {}",
                violation.message
            );
        }
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

    // ── m1（learnhub #165，契约 v2.14）：块级 door_refs 门位标签 ────────────

    /// 合法 door_refs：G-id 形态、块内不重复、引用地形门口且 ⊆ 课程门——
    /// validate_structure 与 reconcile_doors 双闸全过。
    #[test]
    fn block_door_refs_legal_references_pass_both_gates() {
        let mut structure = golden_structure();
        structure["blocks"][0]["door_refs"] = json!(["G1"]);
        structure["blocks"][7]["door_refs"] = json!(["G1"]);
        assert!(
            validate_structure(&structure).is_empty(),
            "{:?}",
            validate_structure(&structure)
        );
        assert!(reconcile_doors(&structure, &golden_terrain()).is_empty());
    }

    /// 旧快照无键兼容：door_refs 是唯一可选键——五键块照常过门。
    #[test]
    fn block_door_refs_absent_is_legal_for_old_snapshots() {
        assert!(validate_structure(&golden_structure()).is_empty());
        assert!(reconcile_doors(&golden_structure(), &golden_terrain()).is_empty());
    }

    /// 形态与去重（gd.blocks.door_refs，红）：非 G<数字> 形、块内重复引用
    /// 各自打回。
    #[test]
    fn block_door_refs_enforces_shape_and_within_block_dedup() {
        let mut malformed = golden_structure();
        malformed["blocks"][0]["door_refs"] = json!(["门1"]);
        let violations = validate_structure(&malformed);
        assert!(violations.iter().any(|violation| violation.code == "gd.blocks.door_refs"
            && !violation.warning), "{violations:?}");

        let mut duplicated = golden_structure();
        duplicated["blocks"][0]["door_refs"] = json!(["G1", "G1"]);
        let violations = validate_structure(&duplicated);
        assert!(violations.iter().any(|violation| violation.code == "gd.blocks.door_refs"
            && violation.message.contains("重复")), "{violations:?}");
    }

    /// 叉积核验（对账门面，悬空 = 红）：door_refs 引用的 G-id 必须在
    /// 地形门口集内——悬空打回且消息指名（悬空编号 + 已知门口清单）。
    #[test]
    fn reconcile_doors_rejects_dangling_block_refs_by_name() {
        let mut structure = golden_structure();
        structure["blocks"][0]["door_refs"] = json!(["G1", "G9"]);
        let violations = reconcile_doors(&structure, &golden_terrain());
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert_eq!(violations[0].code, "gd.blocks.door_refs");
        assert!(violations[0].message.contains("G9"), "{:?}", violations[0].message);
        assert!(violations[0].message.contains("G1、G2"), "{:?}", violations[0].message);
        assert!(violations[0].message.contains("块00"), "{:?}", violations[0].message);
    }

    /// 叉积核验（死标签臂，learnhub check_door_references 同款）：door_refs
    /// 解析进地形门口但越出本课声明课程门（prerequisite_doors）= 永不满足
    /// 的死标签，红。
    #[test]
    fn reconcile_doors_rejects_refs_outside_declared_course_gate() {
        let mut structure = golden_structure();
        structure["domain"]["prerequisite_doors"] = json!(["G1"]);
        structure["blocks"][0]["door_refs"] = json!(["G2"]);
        let violations = reconcile_doors(&structure, &golden_terrain());
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert_eq!(violations[0].code, "gd.blocks.door_refs");
        assert!(violations[0].message.contains("G2"), "{:?}", violations[0].message);
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
        // door_refs 唯一可选键：进 properties 不进 required（可选性钉死）。
        assert_eq!(
            properties["blocks"]["items"]["properties"]["door_refs"]["items"]["pattern"],
            json!("^G[0-9]+$")
        );
        assert!(
            !properties["blocks"]["items"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|key| key == "door_refs"),
            "door_refs must stay optional in schema required"
        );
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
