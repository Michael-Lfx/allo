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
    pub(crate) fn red(code: &'static str, message: impl Into<String>) -> Self {
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

/// 难度档词表（顺序即矩阵列序；落库码，中文只住展示面——CONTEXT.md
/// Stored Code）。
pub const STAGES: &[&str] = &["basic", "advanced", "expert"];

/// 阶段排序权重（词表单源：未知值排最后）——矩阵分格与参考材料呈现序共用。
pub fn stage_rank(stage: &str) -> usize {
    STAGES
        .iter()
        .position(|known| *known == stage)
        .unwrap_or(STAGES.len())
}
/// 边型词表。
pub const EDGE_TYPES: &[&str] = &["assumes", "composes", "contrasts"];
/// 结构边档位词表（与概念网 tier 同集，tier 合一取 apply）。
pub const EDGE_TIERS: &[&str] = &["know", "apply", "teach"];
/// 领域适配四型词表（由模型自判；落库码）。
pub const DOMAIN_TYPES: &[&str] = &["theory", "engineering", "history", "skill"];
/// source 两档制词表（落库码）。
pub const SOURCE_TIERS: &[&str] = &["catalog", "chapter"];
/// 目标类型词表。
pub const GOAL_TYPES: &[&str] = &["capability", "coverage"];
/// 质量清单状态词表。
pub const QUALITY_STATUS: &[&str] = &["met", "unmet"];

/// 中文/旧词 → 落库码的机械映射（只收编已知同义词，不造值）。返回 None =
/// 不在表内，留给契约门照常打回。
fn normalize_vocab_code(field: &str, raw: &str) -> Option<&'static str> {
    let key = raw.trim();
    let lower = key.to_lowercase();
    match field {
        "domain_type" => match key {
            "理论" => Some("theory"),
            "工程" => Some("engineering"),
            "历史" => Some("history"),
            "技能" => Some("skill"),
            _ => match lower.as_str() {
                "theory" => Some("theory"),
                "engineering" => Some("engineering"),
                "history" => Some("history"),
                "skill" => Some("skill"),
                _ => None,
            },
        },
        "source_tier" => match key {
            "名录" => Some("catalog"),
            "章节" => Some("chapter"),
            _ => match lower.as_str() {
                "catalog" => Some("catalog"),
                "chapter" => Some("chapter"),
                _ => None,
            },
        },
        "stage" => match key {
            "入门" => Some("basic"),
            "进阶" => Some("advanced"),
            "高级" => Some("expert"),
            _ => match lower.as_str() {
                "basic" => Some("basic"),
                "advanced" => Some("advanced"),
                "expert" => Some("expert"),
                _ => None,
            },
        },
        "tier" => match key {
            "知道" => Some("know"),
            "会用" => Some("apply"),
            "能教" => Some("teach"),
            // 旧词 `use` 与概念网 tier 合一取 apply（裁决 Q5）。
            "use" => Some("apply"),
            _ => match lower.as_str() {
                "know" => Some("know"),
                "apply" => Some("apply"),
                "teach" => Some("teach"),
                _ => None,
            },
        },
        _ => None,
    }
}

/// 结构产物的机械归一面：模型仍可吐中文（或旧词 `use`/大小写变体），契约
/// 门前统一成落库码。作用域 = 顶层 `domain_type`/`source_tier`、
/// `blocks[].stage`、`edges[].tier`，以及修补产物的 `ops[].stage`/
/// `ops[].tier`（同一函数覆盖两种产物）。只收编已知同义词，表外值原样留给
/// 契约门打回。返回改写条数（调用方 warn 留痕）。
pub fn normalize_structure_vocab(value: &mut Value) -> usize {
    let Some(map) = value.as_object_mut() else {
        return 0;
    };
    let mut rewrites = 0usize;
    for field in ["domain_type", "source_tier"] {
        let Some(current) = map.get(field).and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let Some(code) = normalize_vocab_code(field, &current) else {
            continue;
        };
        if current != code
            && map
                .insert(field.to_owned(), Value::String(code.to_owned()))
                .is_some()
        {
            rewrites += 1;
        }
    }
    for (array_key, field) in [
        ("blocks", "stage"),
        ("edges", "tier"),
        ("ops", "stage"),
        ("ops", "tier"),
    ] {
        let Some(items) = map.get_mut(array_key).and_then(Value::as_array_mut) else {
            continue;
        };
        for item in items {
            let Some(item_map) = item.as_object_mut() else {
                continue;
            };
            let Some(current) = item_map.get(field).and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            let Some(code) = normalize_vocab_code(field, &current) else {
                continue;
            };
            if current != code
                && item_map
                    .insert(field.to_owned(), Value::String(code.to_owned()))
                    .is_some()
            {
                rewrites += 1;
            }
        }
    }
    rewrites
}

/// 结构产物门前删键（p2，病历 2026-11-09 attempt1）：模型多吐白名单之外的
/// 顶层键时机械删除放行 + 每键一行 warn 留痕——不再整份两产物打回重烧，
/// 与条款自述「白名单之外的键一律删除」对齐（现状自述删键、行为整份拒绝
/// 的自相矛盾就此消除）。缺必填键/类型错等硬伤不在此列，仍由
/// [`validate_structure`] 红条打回走原重生成路径。返回删键留痕行（全
/// warn 级，code = `gd.keys`；调用方并入违约表即随 warnings 透出）。
pub fn drop_unknown_top_keys(value: &mut Value) -> Vec<ContractViolation> {
    let Some(map) = value.as_object_mut() else {
        return Vec::new();
    };
    let unknown: Vec<String> = map
        .keys()
        .filter(|key| !TOP_KEYS.contains(&key.as_str()))
        .cloned()
        .collect();
    for key in &unknown {
        map.remove(key);
    }
    unknown
        .into_iter()
        .map(|key| {
            ContractViolation::warn(
                "gd.keys",
                format!("未知顶层键「{key}」已删除放行（白名单之外的键一律删除）"),
            )
        })
        .collect()
}

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

/// L0 硬门字符：纯标点并列符——无词形生命、无记号生命，名字里出现即必然
/// 捆绑（`、`/`，`）。命中 = 红条 `gd.blocks.one_name`（y01 三层分工的
/// 无条件面）。
pub const HARD_NAME_MARKS: [char; 2] = ['、', '，'];

/// L1 点名字符：歧义字与半角标点（`与/和/及/,//`）。命中只记
/// `gd.blocks.name_hint` 警告、不终审——「求和公式」「调和级数」与
/// 「双进程与窗口」双向反例下，任何词表/位置启发式都不可先验封闭；
/// **`和` 不设词首/词尾豁免**（词边未必是名词，是否捆绑交 L2 裁决）。
pub const SUSPECT_NAME_MARKS: [char; 5] = ['与', '和', '及', ',', '/'];

/// 「」官方术语段内豁免的扫描：`visit` 只看段外字符。
fn scan_name_marks(name: &str, mut visit: impl FnMut(char)) {
    let mut quoted = false;
    for c in name.chars() {
        match c {
            '「' => quoted = true,
            '」' => quoted = false,
            _ if quoted => {}
            _ => visit(c),
        }
    }
}

/// L0 硬门标记（首个命中；「」段内豁免）。
pub fn hard_name_mark(name: &str) -> Option<char> {
    let mut hit = None;
    scan_name_marks(name, |c| {
        if hit.is_none() && HARD_NAME_MARKS.contains(&c) {
            hit = Some(c);
        }
    });
    hit
}

/// L1 点名标记（去重保序；「」段内豁免；`和` 无词边豁免）。
pub fn soft_name_marks(name: &str) -> Vec<char> {
    let mut marks: Vec<char> = Vec::new();
    scan_name_marks(name, |c| {
        if SUSPECT_NAME_MARKS.contains(&c) && !marks.contains(&c) {
            marks.push(c);
        }
    });
    marks
}

/// L1 点名事实（y01）：L2 批量裁决的输入行——名字 + 首个被点字符 +
/// 产物内位置。位置只用于报告回灌，不进裁决输入（中立面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameSuspect {
    pub name: String,
    pub mark: char,
    /// `blocks[<下标>]`（报告面用；裁决输入不带位置）。
    pub location: String,
}

/// 点名清单派生（纯函数）：逐块扫描 L1 形态。L0 命中归红条、不进清单；
/// 「」段内不进；同名只留第一条（L2 输出契约按名字去重）。
pub fn collect_name_suspects(structure: &Value) -> Vec<NameSuspect> {
    let mut suspects: Vec<NameSuspect> = Vec::new();
    let Some(blocks) = structure.get("blocks").and_then(Value::as_array) else {
        return suspects;
    };
    for (index, block) in blocks.iter().enumerate() {
        let name = block.get("name").and_then(Value::as_str).unwrap_or_default().trim();
        if name.is_empty() || hard_name_mark(name).is_some() {
            continue;
        }
        let Some(mark) = soft_name_marks(name).into_iter().next() else {
            continue;
        };
        if suspects.iter().any(|seen| seen.name == name) {
            continue;
        }
        suspects.push(NameSuspect {
            name: name.to_owned(),
            mark,
            location: format!("blocks[{index}]"),
        });
    }
    suspects
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

    // gd.keys：白名单 + 结构五件必填。未知键的放行走门前机械删键（p2：
    // `drop_unknown_top_keys`，删除 + warn 留痕）——这里降为 warn 留痕，只兜
    // 未经门前的直接调用（修补候选/外部落库面）；缺必填键仍是红条硬伤。
    for key in map.keys() {
        if !TOP_KEYS.contains(&key.as_str()) {
            violations.push(ContractViolation::warn(
                "gd.keys",
                format!("未知顶层键「{key}」在白名单之外——生成门前已机械删除放行，此处仅留痕"),
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
        if let Some(mark) = hard_name_mark(name) {
            violations.push(ContractViolation::red(
                "gd.blocks.one_name",
                // f2（A3）：出路指引补「改用无并列的规范名」半句（learnhub 同
                // 款先例）——报全逻辑不动，打地鼠走提示词交卷自检句压制。
                // y01：硬门只认纯标点（L0）——歧义字/半角标点降 L1 提示。
                format!(
                    "块名「{name}」含并列标记「{mark}」——一条 = 一个独立教学主题，多个主题须拆条；官方术语本身含并列形的以「」包裹，或改用无并列的规范名"
                ),
            ));
        } else if let Some(mark) = soft_name_marks(name).into_iter().next() {
            // y01 L1：歧义标记只点名提示（不单独阻断）——「求和公式」类术
            // 语本字不再被机械门误杀；是否构成捆绑交 L2 批量裁决。warn 不
            // 进 format_violations 的生成反馈（调用方只取 !warning）。
            violations.push(ContractViolation::warn(
                "gd.blocks.name_hint",
                format!(
                    "块名「{name}」含并列字/分隔符「{mark}」——若是多个主题须拆成多条，或改用无并列的规范名；术语本字含该字可保留（提示，不阻断）"
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

    // gd.no_schedule：排期红线扫全文（quality/disclaimer 元声明面豁免；
    // u09 再豁免 goal——目标原句回执必须逐字，目标含排期类词不是产物的
    // 排期语义，其余字段照扫；x01 再收半步——marker 命中处若该词本身出自
    // 学习目标原句（用户点名的课程主题词，如进度管理课的「进度」），同样
    // 不构成产物的排期语义——课程主题与排期承诺是两回事；其余 marker
    // （时间表/每天/每周/小时…）照扫，真正的排期承诺形态仍被拦）。
    let goal_text = value
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut strings = Vec::new();
    collect_strings(value, &["quality", "disclaimer", "goal"], &mut strings);
    for text in trim_strings(&strings) {
        if let Some(marker) = SCHEDULE_MARKERS
            .iter()
            .find(|marker| text.contains(**marker) && !goal_text.contains(**marker))
        {
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

    /// 归一面红绿样（x08）：中文/旧词/大小写 → 落库码；表外值不动（留给
    /// 契约门打回，归一不造值）。
    #[test]
    fn structure_vocab_normalizes_known_synonyms_only() {
        let mut structure = json!({
            "domain_type": "理论",
            "source_tier": "章节",
            "blocks": [
                { "name": "块一", "stage": "入门" },
                { "name": "块二", "stage": "Advanced" },
                { "name": "块三", "stage": "高级" }
            ],
            "edges": [
                { "tier": "use" },
                { "tier": "知道" },
                { "tier": "会用" },
                { "tier": "TEACH" }
            ]
        });
        assert_eq!(normalize_structure_vocab(&mut structure), 9);
        assert_eq!(structure["domain_type"], json!("theory"));
        assert_eq!(structure["source_tier"], json!("chapter"));
        assert_eq!(structure["blocks"][0]["stage"], json!("basic"));
        assert_eq!(structure["blocks"][1]["stage"], json!("advanced"));
        assert_eq!(structure["blocks"][2]["stage"], json!("expert"));
        assert_eq!(structure["edges"][0]["tier"], json!("apply"));
        assert_eq!(structure["edges"][1]["tier"], json!("know"));
        assert_eq!(structure["edges"][2]["tier"], json!("apply"));
        assert_eq!(structure["edges"][3]["tier"], json!("teach"));

        // 已是码 → 零改写（幂等）。
        assert_eq!(normalize_structure_vocab(&mut structure), 0);

        // 表外值不造值：原样保留，由 gd.* 打回。
        let mut unknown = json!({
            "domain_type": "玄学",
            "blocks": [{ "name": "块", "stage": "宗师" }],
            "edges": [{ "tier": "bogus" }]
        });
        assert_eq!(normalize_structure_vocab(&mut unknown), 0);
        assert_eq!(unknown["domain_type"], json!("玄学"));
        assert_eq!(unknown["blocks"][0]["stage"], json!("宗师"));
        assert_eq!(unknown["edges"][0]["tier"], json!("bogus"));
    }

    /// 修补产物同函数覆盖：ops[].stage/tier 也归一。
    #[test]
    fn structure_vocab_normalizes_repair_ops() {
        let mut artifact = json!({
            "ops": [
                { "op": "add_edge", "tier": "use" },
                { "op": "add_block", "stage": "入门" }
            ]
        });
        assert_eq!(normalize_structure_vocab(&mut artifact), 2);
        assert_eq!(artifact["ops"][0]["tier"], json!("apply"));
        assert_eq!(artifact["ops"][1]["stage"], json!("basic"));
    }


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
            "domain_type": "theory",
            "form_reference": "Flowy 学习结构模板 v1",
            "source_tier": "catalog",
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
                { "source": "块08", "target": "块09", "type": "composes", "tier": "apply" },
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
            blocks[0]["stage"] = json!("basic");
            blocks[1]["stage"] = json!("advanced");
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
            blocks[0]["stage"] = json!("basic");
            blocks[1]["stage"] = json!("advanced");
            let edges = aligned.get_mut("edges").unwrap().as_array_mut().unwrap();
            edges[0] = json!({ "source": "块01", "target": "块00", "type": "assumes", "tier": "know", "note": "同向" });
        }
        assert!(validate_structure(&aligned).is_empty(), "{:?}", validate_structure(&aligned));
        let mut same_stage = golden_structure();
        {
            let blocks = same_stage.get_mut("blocks").unwrap().as_array_mut().unwrap();
            blocks[0]["stage"] = json!("basic");
            blocks[1]["stage"] = json!("basic");
            let edges = same_stage.get_mut("edges").unwrap().as_array_mut().unwrap();
            edges[0] = json!({ "source": "块00", "target": "块01", "type": "assumes", "tier": "know", "note": "同档" });
        }
        assert!(validate_structure(&same_stage).is_empty(), "{:?}", validate_structure(&same_stage));
        let mut composes = golden_structure();
        {
            let blocks = composes.get_mut("blocks").unwrap().as_array_mut().unwrap();
            blocks[0]["stage"] = json!("basic");
            blocks[1]["stage"] = json!("expert");
            let edges = composes.get_mut("edges").unwrap().as_array_mut().unwrap();
            edges[0] = json!({ "source": "块00", "target": "块01", "type": "composes", "tier": "apply", "note": "组装不限档" });
        }
        assert!(validate_structure(&composes).is_empty(), "{:?}", validate_structure(&composes));
    }

    #[test]
    fn gd_parse_rejects_non_object() {
        let violations = validate_structure(&json!(["not", "an", "object"]));
        assert_eq!(codes(&violations), vec!["gd.parse"]);
    }

    /// p2：缺必填键仍是硬伤（红条 gd.keys）——宽容只覆盖未知键，缺键/类型
    /// 错等照旧整份打回走 Rejected 重生成路径。
    #[test]
    fn gd_keys_missing_required_still_red() {
        let mut value = golden_structure();
        value.as_object_mut().unwrap().remove("gaps");
        let violations = validate_structure(&value);
        let keys_red: Vec<&ContractViolation> = violations
            .iter()
            .filter(|violation| !violation.warning && violation.code == "gd.keys")
            .collect();
        // 缺键行（gd.keys）仍红；下游 gd.gaps 级联红不在本判据内（缺键本就
        // 整份打回，p2 未动）。
        assert_eq!(keys_red.len(), 1, "{violations:?}");
        assert!(
            keys_red[0].message.contains("缺少必填顶层键「gaps」"),
            "{}",
            keys_red[0].message
        );
    }

    /// p2（病历 2026-11-09 attempt1）：模型多吐一个顶层键 `type` → 门前机械
    /// 删键放行 + warn 留痕，不再整份两产物打回重烧。删键后产物无该键、
    /// 契约门零违约（金样本就全绿）；未经门前的直接校验未知键降为 warn
    /// 留痕（兑现「白名单之外的键一律删除」的自述语义：删键是机械修复动作，
    /// 不再是拒收条款）。
    #[test]
    fn gd_keys_unknown_top_key_deleted_at_the_door_with_warn_trail() {
        let mut structure = golden_structure();
        structure.as_object_mut().unwrap().insert("type".into(), json!("quiz"));
        // 门前删键：键消失 + 每键一行 warn 留痕（措辞点名键与「删除」）。
        let dropped = drop_unknown_top_keys(&mut structure);
        assert_eq!(dropped.len(), 1, "{dropped:?}");
        assert!(
            dropped
                .iter()
                .all(|violation| violation.code == "gd.keys" && violation.warning),
            "删键留痕必须全为 warn 级: {dropped:?}"
        );
        assert!(
            dropped[0].message.contains("type") && dropped[0].message.contains("删除"),
            "留痕行点名键与处置: {}",
            dropped[0].message
        );
        assert!(structure.get("type").is_none(), "放行产物不得残留未知键");
        assert!(
            validate_structure(&structure).is_empty(),
            "删键后契约门零违约: {:?}",
            validate_structure(&structure)
        );
        // 干净产物零触碰（幂等）；非对象输入防御回退。
        let mut clean = golden_structure();
        assert!(drop_unknown_top_keys(&mut clean).is_empty());
        assert!(drop_unknown_top_keys(&mut json!(["not", "an", "object"])).is_empty());

        // 未经门前的直接校验（修补候选/外部落库面）：未知键 warn 留痕在场、
        // 不再进红条。
        let mut raw = golden_structure();
        raw.as_object_mut().unwrap().insert("surprise".into(), json!(1));
        let violations = validate_structure(&raw);
        assert!(
            violations.iter().all(|violation| violation.warning),
            "未知键不得再进红条: {violations:?}"
        );
        assert!(
            violations.iter().any(|violation| violation.code == "gd.keys"
                && violation.warning
                && violation.message.contains("surprise")),
            "warn 留痕必须在场: {violations:?}"
        );
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
                        "stage": "basic",
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
                        "stage": "basic",
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

    /// f2（② 打地鼠防回归，A3）：多个块名同含**纯标点**并列标记时校验器
    /// **一次报全**——报告逐块一行、各提一名（被点名的与别处新冒的同在这
    /// 一份报告里，模型无需逐轮试探）；违约消息带出路指引（拆条 / 「」包
    /// 裹 / 改规范名）。y01：硬门只认 `、`/`，`（L0）。
    #[test]
    fn gd_blocks_one_name_hard_marks_report_all_offenders_with_guidance_in_one_pass() {
        let mut value = golden_structure();
        // 边端点不引用块02/块05——改名不牵动 gd.edges.endpoints，判据不混。
        value["blocks"][2]["name"] = json!("百分数、利率的对应");
        value["blocks"][5]["name"] = json!("底数，指数");
        let violations = validate_structure(&value);
        let one_name: Vec<&ContractViolation> = violations
            .iter()
            .filter(|violation| violation.code == "gd.blocks.one_name")
            .collect();
        assert_eq!(one_name.len(), 2, "一次报全: {violations:?}");
        assert!(
            one_name.iter().any(|violation| violation.message.contains("百分数、利率的对应"))
                && one_name.iter().any(|violation| violation.message.contains("底数，指数")),
            "每行各提一名: {:?}",
            one_name
        );
        for violation in &one_name {
            assert!(!violation.warning, "L0 硬门是红条");
            assert!(
                violation.message.contains("多个主题须拆条")
                    && violation.message.contains("或改用无并列的规范名"),
                "出路指引在每行: {}",
                violation.message
            );
        }
        // L0 命中不重复发 hint（红条已点名，不再提示同名）。
        assert!(
            !violations.iter().any(|violation| violation.code == "gd.blocks.name_hint"),
            "L0 命中不进 L1 提示: {violations:?}"
        );
    }

    /// y01 L0/L1 分工：`与/和/及/,//` 只出 `gd.blocks.name_hint` warn，
    /// 不单独阻断；「」段内整段豁免；词边 `和` 豁免取消（词首/词尾同样是
    /// 筛查面，是否捆绑交 L2 裁决）——提示词承诺的「求和公式」不再被误杀。
    #[test]
    fn gd_blocks_name_hint_flags_soft_marks_without_rejecting() {
        let cases = [
            ("求和公式", '和'),
            ("等差前n项和", '和'),
            ("和差角公式", '和'),
            ("双进程与窗口", '与'),
            ("甲,乙", ','),
            ("甲/乙", '/'),
            ("加法及减法", '及'),
        ];
        for (name, mark) in cases {
            // 改名块[2]（边不引用它——边端点解析是另一条款，不混入本判据）。
            let mut value = golden_structure();
            value["blocks"][2]["name"] = json!(name);
            let violations = validate_structure(&value);
            assert!(
                !codes(&violations).contains(&"gd.blocks.one_name"),
                "「{name}」不得被 L0 硬门误杀: {violations:?}"
            );
            assert!(
                violations.iter().all(|violation| violation.warning),
                "「{name}」不得产生任何红条: {violations:?}"
            );
            let hint = violations
                .iter()
                .find(|violation| violation.code == "gd.blocks.name_hint")
                .unwrap_or_else(|| panic!("「{name}」应有 name_hint: {violations:?}"));
            assert!(hint.warning, "hint 是警告级");
            assert!(
                hint.message.contains(name) && hint.message.contains(&mark.to_string()),
                "hint 点名名字与被点字符: {}",
                hint.message
            );
        }

        // 「」段内整段豁免（L0/L1 同门）：无红、无 hint。
        let mut quoted = golden_structure();
        quoted["blocks"][2]["name"] = json!("「广义表与树」的辨析");
        let violations = validate_structure(&quoted);
        assert!(!codes(&violations).contains(&"gd.blocks.one_name"), "{violations:?}");
        assert!(
            !violations.iter().any(|violation| violation.code == "gd.blocks.name_hint"),
            "「」段内不进点名: {violations:?}"
        );
        // 段内豁免不掩护段外：`「甲、乙」与丙` 的硬标点在段内、`与` 在段外。
        let mut mixed = golden_structure();
        mixed["blocks"][2]["name"] = json!("「甲、乙」与丙");
        let violations = validate_structure(&mixed);
        assert!(!codes(&violations).contains(&"gd.blocks.one_name"), "{violations:?}");
        assert!(
            violations
                .iter()
                .any(|violation| violation.code == "gd.blocks.name_hint"
                    && violation.message.contains('与')),
            "段外「与」仍点名: {violations:?}"
        );
    }

    /// y01 `collect_name_suspects`：L2 裁决的输入清单——只收 L1 形态
    /// （名字 + 首个被点字符 + 产物内位置），L0 命中与「」段内不进清单；
    /// 同名只留一条（输出契约按名字去重）。
    #[test]
    fn collect_name_suspects_reports_soft_marks_only() {
        let mut value = golden_structure();
        value["blocks"][2]["name"] = json!("求和公式");
        value["blocks"][5]["name"] = json!("双进程与窗口");
        value["blocks"][7]["name"] = json!("百分数、利率");
        value["blocks"][9]["name"] = json!("「广义表与树」的辨析");
        let suspects = collect_name_suspects(&value);
        assert_eq!(suspects.len(), 2, "{suspects:?}");
        assert_eq!(suspects[0].name, "求和公式");
        assert_eq!(suspects[0].mark, '和');
        assert_eq!(suspects[0].location, "blocks[2]");
        assert_eq!(suspects[1].name, "双进程与窗口");
        assert_eq!(suspects[1].mark, '与');
        assert_eq!(suspects[1].location, "blocks[5]");

        // 同名（异常产物也会重复）只留第一条。
        let duplicated = json!({
            "blocks": [
                { "name": "求和公式" },
                { "name": "求和公式" }
            ]
        });
        let suspects = collect_name_suspects(&duplicated);
        assert_eq!(suspects.len(), 1, "{suspects:?}");
        assert_eq!(suspects[0].location, "blocks[0]");

        assert!(collect_name_suspects(&golden_structure()).is_empty());
        assert!(collect_name_suspects(&json!({})).is_empty());
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
                    "stage": "basic",
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
        value["edges"][1]["tier"] = json!("bogus");
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

    /// u09 红绿样：goal 是目标原句的逐字回执——目标含「进度/计划/小时」
    /// 类词时合规回执不得触发 gd.no_schedule（原句不许改写）；其余字段含
    /// 排期词照扫照报（豁免面只有 goal）。
    #[test]
    fn gd_no_schedule_exempts_the_goal_receipt_but_not_other_fields() {
        let mut receipt = golden_structure();
        receipt["goal"] = json!("掌握项目进度管理：制定计划并按小时推进");
        assert!(
            !codes(&validate_structure(&receipt)).contains(&"gd.no_schedule"),
            "goal 原句回执豁免：{:?}",
            validate_structure(&receipt)
        );

        // x01 主题词豁免后，此处「照扫」改用 goal 不含的 marker 验证：
        // goal 含「计划」——含「计划」的定义被豁免；含「每周」的仍照报。
        let mut elsewhere = golden_structure();
        elsewhere["goal"] = json!("掌握项目进度管理：制定计划并按小时推进");
        elsewhere["spines"][0]["definition"] = json!("每周固定推进两讲，配套练习");
        assert!(
            codes(&validate_structure(&elsewhere)).contains(&"gd.no_schedule"),
            "非主题 marker（goal 外的「每周」）照扫：{:?}",
            validate_structure(&elsewhere)
        );
        let mut subject = golden_structure();
        subject["goal"] = json!("掌握项目进度管理：制定计划并按小时推进");
        subject["spines"][0]["definition"] = json!("制定计划并拆解任务清单");
        assert!(
            !codes(&validate_structure(&subject)).contains(&"gd.no_schedule"),
            "goal 内主题词（计划）在其它字段落位 = 领域语义豁免：{:?}",
            validate_structure(&subject)
        );
    }

    /// x01 真机病历同形（run：进度管理课三轮级联失败 EXIT=2）：排期
    /// marker 命中处，若该词本身出自课程目标原句（用户点名的课程主题词），
    /// 不构成产物的排期语义——进度管理课的定义必然谈论「进度」；非主题
    /// marker（时间表/每天/每周…）照扫照报，真正的排期承诺形态仍被拦。
    #[test]
    fn gd_no_schedule_exempt_subject_words_from_the_goal_but_keep_other_markers() {
        let mut value = golden_structure();
        value["goal"] = json!("掌握项目进度管理的关键路径方法");
        value["spines"][0]["definition"] = json!("量化各活动的进度弹性，判断延误影响");
        assert!(
            !codes(&validate_structure(&value)).contains(&"gd.no_schedule"),
            "主题词豁免：{:?}",
            validate_structure(&value)
        );

        let mut hard = golden_structure();
        hard["goal"] = json!("掌握项目进度管理的关键路径方法");
        hard["spines"][0]["definition"] = json!("按时间表每周推进三章");
        assert!(
            codes(&validate_structure(&hard)).contains(&"gd.no_schedule"),
            "非主题 marker（goal 外的「时间表/每周」）照扫：{:?}",
            validate_structure(&hard)
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
