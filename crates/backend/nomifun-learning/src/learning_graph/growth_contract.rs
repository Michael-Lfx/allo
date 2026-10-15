//! 生长批产物契约（`ng.*`，g4 裁决：教练批全迁新基建，与 structure_contract
//! 同族）——schema 单源 + 条款化形状 validator + 评审输出契约。
//!
//! 分工（learnhub 同款）：provider 的 json_object 保证**合法 JSON**（ng.parse
//! 是读侧条款）；本模块执法**形状**（键白名单/词表/量级/批内唯一）；DB 语义门
//! （登记表撞名、引用可解析、就绪即发布、课程内标题唯一）留
//! [`super::validate_batch`]，同用 ng.* 编号——重试装配按编号逐条修正。
//! **容错 deserializer 已退役**（g4）：`"est": "15"`、中文档位、单对象数组
//! 等旧习惯一律按违约打回，由条款报告引导重试。

use serde_json::Value;

use super::{MAX_BATCH_NODES, structure_contract::ContractViolation};

/// 概念档位词表（机面英文；容错退役后中文档位 = 违约）。
pub const TIERS: &[&str] = &["know", "apply", "teach"];
/// 节点批次顶层键白名单。
pub const BATCH_KEYS: &[&str] = &["nodes", "mints", "new_aliases", "note"];
/// 节点键白名单（缺省可选键见各条款）。
pub const NODE_KEYS: &[&str] = &["title", "purpose", "est", "difficulty", "teaches", "assumes"];
/// 铸名键白名单。
pub const MINT_KEYS: &[&str] = &["canonical", "aliases", "definition"];
/// new_aliases 提名键白名单（三轮 g2：三字段全必填）。
pub const NEW_ALIAS_KEYS: &[&str] = &["name", "definition", "source"];
/// 节点标题长度上限（字符）。
pub const TITLE_MAX_CHARS: usize = 80;
/// est 区间（门放行 1–60）。est = 专注工作量尺寸锚（1 est 分钟 = 1 分钟
/// 专注学习）；键名统一 est（j01），DB 列保留旧名 `estimated_minutes`。
pub const EST_RANGE: (u64, u64) = (1, 60);
/// 难度区间。
pub const DIFFICULTY_RANGE: (u64, u64) = (1, 5);
/// 评审问题条数上限。
pub const REVIEW_PROBLEMS_MAX: usize = 8;
/// 评审 verdict 词表。
pub const REVIEW_VERDICTS: &[&str] = &["pass", "reject"];
/// 评审问题 field 词表（与评审提示词逐字一致）。
pub const REVIEW_FIELDS: &[&str] = &["teaches", "assumes", "mints", "title", "difficulty"];

fn red(code: &'static str, message: impl Into<String>) -> ContractViolation {
    ContractViolation {
        code,
        message: message.into(),
        warning: false,
    }
}

fn as_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value.as_object()
}

fn array_at<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    value.get(key).and_then(Value::as_array)
}

/// 生长批**形状契约**（ng.*，无 DB 事实）：顶层键、节点形状/词表/量级、批内
/// 标题与铸名唯一。违规非空 = 打回重试（报告注入）。
pub fn validate_proposal(value: &Value) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    let Some(map) = as_object(value) else {
        violations.push(red("ng.parse", "生长批必须是单个 JSON 对象"));
        return violations;
    };
    for key in map.keys() {
        if !BATCH_KEYS.contains(&key.as_str()) {
            violations.push(red(
                "ng.keys",
                format!("未知顶层键「{key}」——只允许 {}", BATCH_KEYS.join("/")),
            ));
        }
    }

    // ── 节点 ────────────────────────────────────────────────────────────
    let nodes = array_at(value, "nodes");
    let Some(nodes) = nodes else {
        violations.push(red("ng.nodes.required", "nodes 必须是数组（缺口为 0 时给空数组）"));
        return violations;
    };
    if nodes.len() > MAX_BATCH_NODES {
        violations.push(red(
            "ng.batch.size",
            format!("一批最多 {MAX_BATCH_NODES} 个新节点，本批 {} 个——拆批重交", nodes.len()),
        ));
    }
    let mut seen_titles: Vec<String> = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let at = format!("nodes[{index}]");
        let Some(node_map) = as_object(node) else {
            violations.push(red("ng.nodes.required", format!("{at} 不是对象")));
            continue;
        };
        for key in node_map.keys() {
            if !NODE_KEYS.contains(&key.as_str()) {
                violations.push(red(
                    "ng.keys",
                    format!("{at} 含未知键「{key}」——节点只允许 {}", NODE_KEYS.join("/")),
                ));
            }
        }
        let title = node_map.get("title").and_then(Value::as_str).unwrap_or_default().trim();
        if title.is_empty() {
            violations.push(red("ng.nodes.required", format!("{at} 的 title 缺失或为空")));
        } else {
            if title.chars().count() > TITLE_MAX_CHARS {
                violations.push(red(
                    "ng.nodes.required",
                    format!("节点标题过长（>{TITLE_MAX_CHARS} 字）：{title}"),
                ));
            }
            let key = title.to_lowercase();
            if seen_titles.iter().any(|seen| *seen == key) {
                violations.push(red("ng.title.batch_unique", format!("批内节点标题重复：{title}")));
            }
            seen_titles.push(key);
        }
        if node_map.get("purpose").and_then(Value::as_str).unwrap_or_default().trim().is_empty() {
            violations.push(red(
                "ng.nodes.required",
                format!("{at}（{title}）的 purpose 缺失或为空——每个节点必须带日常经验切口"),
            ));
        }
        if let Some(est) = node_map.get("est") {
            let valid = est
                .as_u64()
                .is_some_and(|value| (EST_RANGE.0..=EST_RANGE.1).contains(&value));
            if !valid {
                violations.push(red(
                    "ng.nodes.est",
                    format!(
                        "节点 {title} 的 est 必须是 {}-{} 的整数（1 est 分钟 = 1 分钟专注；字符串等旧形状已退役）：{est}",
                        EST_RANGE.0, EST_RANGE.1
                    ),
                ));
            }
        }
        if let Some(difficulty) = node_map.get("difficulty") {
            let valid = difficulty
                .as_u64()
                .is_some_and(|value| (DIFFICULTY_RANGE.0..=DIFFICULTY_RANGE.1).contains(&value));
            if !valid {
                violations.push(red(
                    "ng.nodes.difficulty",
                    format!(
                        "节点 {title} 的 difficulty 必须是 {}-{} 的整数：{difficulty}",
                        DIFFICULTY_RANGE.0, DIFFICULTY_RANGE.1
                    ),
                ));
            }
        }
        for field in ["teaches", "assumes"] {
            let Some(references) = node_map.get(field) else {
                continue;
            };
            let Some(references) = references.as_array() else {
                violations.push(red(
                    "ng.refs.shape",
                    format!("{at} 的 {field} 必须是数组（单对象形状已退役）"),
                ));
                continue;
            };
            for (reference_index, reference) in references.iter().enumerate() {
                let reference_at = format!("{at}.{field}[{reference_index}]");
                let Some(reference_map) = as_object(reference) else {
                    violations.push(red("ng.refs.shape", format!("{reference_at} 不是对象")));
                    continue;
                };
                let name = reference_map.get("name").and_then(Value::as_str).unwrap_or_default().trim();
                if name.is_empty() {
                    violations.push(red(
                        "ng.refs.name",
                        format!("{reference_at} 的 name 为空——引用必须带概念名"),
                    ));
                }
                let tier = reference_map.get("tier").and_then(Value::as_str).unwrap_or_default();
                if !TIERS.contains(&tier) {
                    violations.push(red(
                        "ng.refs.tier",
                        format!(
                            "{reference_at}（{name}）的 tier 必须是 {} 之一（中文档位等旧形状已退役）：「{tier}」",
                            TIERS.join("/")
                        ),
                    ));
                }
            }
        }
    }

    // ── 铸名（批内唯一；与登记表的撞名属语义门）──────────────────────────
    let Some(mints) = array_at(value, "mints") else {
        violations.push(red("ng.mints.shape", "mints 必须是数组"));
        return violations;
    };
    let mut used_names: Vec<String> = Vec::new();
    // 批内共享名池：节点标题 + 铸名 canonical/别名 + new_aliases 提名——
    // 一物一名（ng.aliases.atomic_name 的比对域）。
    if let Some(nodes) = array_at(value, "nodes") {
        for node in nodes {
            if let Some(title) = node.get("title").and_then(Value::as_str) {
                used_names.push(title.trim().to_lowercase());
            }
        }
    }
    for (index, mint) in mints.iter().enumerate() {
        let at = format!("mints[{index}]");
        let Some(mint_map) = as_object(mint) else {
            violations.push(red("ng.mints.shape", format!("{at} 不是对象")));
            continue;
        };
        for key in mint_map.keys() {
            if !MINT_KEYS.contains(&key.as_str()) {
                violations.push(red(
                    "ng.keys",
                    format!("{at} 含未知键「{key}」——铸名只允许 {}", MINT_KEYS.join("/")),
                ));
            }
        }
        let canonical = mint_map.get("canonical").and_then(Value::as_str).unwrap_or_default().trim();
        if canonical.is_empty() {
            violations.push(red("ng.mints.fields", format!("{at} 的 canonical 为空")));
            continue;
        }
        let canonical_key = canonical.to_lowercase();
        if used_names.iter().any(|used| *used == canonical_key) {
            violations.push(red("ng.mints.batch_unique", format!("批内铸名重复：「{canonical}」")));
        }
        used_names.push(canonical_key);
        if let Some(aliases) = mint_map.get("aliases") {
            let Some(aliases) = aliases.as_array() else {
                violations.push(red(
                    "ng.mints.fields",
                    format!("{at}（{canonical}）的 aliases 必须是字符串数组"),
                ));
                continue;
            };
            for alias in aliases {
                let Some(alias) = alias.as_str() else {
                    violations.push(red(
                        "ng.mints.fields",
                        format!("{at}（{canonical}）的别名不是字符串"),
                    ));
                    continue;
                };
                let alias = alias.trim();
                if alias.is_empty() {
                    continue;
                }
                let alias_key = alias.to_lowercase();
                if used_names.iter().any(|used| *used == alias_key) {
                    violations.push(red(
                        "ng.mints.batch_unique",
                        format!("铸名「{alias}」（{canonical} 的别名）与批内既有名字冲突"),
                    ));
                }
                used_names.push(alias_key);
            }
        }
    }

    // ── new_aliases 提名（三轮 g2）：三字段全必填 + 批内同池唯一 ──────────
    let new_aliases = array_at(value, "new_aliases");
    if let Some(new_aliases) = new_aliases {
        for (index, nomination) in new_aliases.iter().enumerate() {
            let at = format!("new_aliases[{index}]");
            let Some(nomination_map) = as_object(nomination) else {
                violations.push(red("ng.aliases.required", format!("{at} 不是对象")));
                continue;
            };
            for key in nomination_map.keys() {
                if !NEW_ALIAS_KEYS.contains(&key.as_str()) {
                    violations.push(red(
                        "ng.keys",
                        format!("{at} 含未知键「{key}」——别名提名只允许 {}", NEW_ALIAS_KEYS.join("/")),
                    ));
                }
            }
            for field in NEW_ALIAS_KEYS {
                let value_text = nomination_map
                    .get(*field)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if value_text.is_empty() {
                    violations.push(red(
                        "ng.aliases.required",
                        format!("{at} 的 {field} 缺失或为空——别名提名三字段全必填"),
                    ));
                }
            }
            if let Some(name) = nomination_map.get("name").and_then(Value::as_str) {
                let name_key = name.trim().to_lowercase();
                if !name_key.is_empty() {
                    if used_names.iter().any(|used| *used == name_key) {
                        violations.push(red(
                            "ng.aliases.atomic_name",
                            format!(
                                "别名提名「{name}」与批内既有名字（节点/铸名/别名）互撞——一物一名"
                            ),
                        ));
                    }
                    used_names.push(name_key);
                }
            }
        }
    }
    violations
}

/// 概念评审**输出契约**（ng.review.*）：verdict 词表 + problems 形状/词表。
/// 评审不可用（调用失败或本节违约）= 保守放行的判定在调用方（语义不变）。
pub fn validate_review(value: &Value) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    let Some(map) = as_object(value) else {
        violations.push(red("ng.review.parse", "评审回包必须是单个 JSON 对象"));
        return violations;
    };
    let verdict = map.get("verdict").and_then(Value::as_str).unwrap_or_default();
    if !REVIEW_VERDICTS.contains(&verdict) {
        violations.push(red(
            "ng.review.verdict",
            format!("verdict 必须是 {} 之一：「{verdict}」", REVIEW_VERDICTS.join("/")),
        ));
        return violations;
    }
    // 复用裁决面（j04）：`reuse: [{name, concept_id}]`——提名为复用的裁决
    // 输出；concept_id 是否真在候选集内由调用方机械核验（禁编造），这里只
    // 执形状。verdict=pass 与 reject 都可携带。
    if let Some(reuse) = map.get("reuse") {
        let Some(entries) = reuse.as_array() else {
            violations.push(red("ng.review.reuse", "reuse 必须是数组"));
            return violations;
        };
        for (index, entry) in entries.iter().enumerate() {
            let at = format!("reuse[{index}]");
            let Some(entry_map) = as_object(entry) else {
                violations.push(red("ng.review.reuse", format!("{at} 不是对象")));
                continue;
            };
            for field in ["name", "concept_id"] {
                if entry_map
                    .get(field)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                {
                    violations.push(red(
                        "ng.review.reuse",
                        format!("{at} 的 {field} 缺失或为空"),
                    ));
                }
            }
        }
    }
    // findings warn 通道（j09，可选键）：`findings: [{node?, field?, issue,
    // advice?}]`——数据缺陷观察，不打回只留痕；issue 非空是唯一硬形状。
    if let Some(findings) = map.get("findings") {
        let Some(entries) = findings.as_array() else {
            violations.push(red("ng.review.findings", "findings 必须是数组"));
            return violations;
        };
        for (index, entry) in entries.iter().enumerate() {
            let at = format!("findings[{index}]");
            let Some(entry_map) = as_object(entry) else {
                violations.push(red("ng.review.findings", format!("{at} 不是对象")));
                continue;
            };
            if entry_map
                .get("issue")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                violations.push(red(
                    "ng.review.findings",
                    format!("{at} 的 issue 缺失或为空——留痕是线索，issue 必须说清缺什么"),
                ));
            }
        }
    }
    if verdict == "pass" {
        return violations;
    }
    let Some(problems) = map.get("problems").and_then(Value::as_array) else {
        violations.push(red("ng.review.problems", "reject 必须带 problems 数组"));
        return violations;
    };
    if problems.is_empty() || problems.len() > REVIEW_PROBLEMS_MAX {
        violations.push(red(
            "ng.review.problems",
            format!("problems 条数须在 1..={REVIEW_PROBLEMS_MAX} 之间：{} 条", problems.len()),
        ));
    }
    for (index, problem) in problems.iter().enumerate() {
        let at = format!("problems[{index}]");
        let Some(problem_map) = as_object(problem) else {
            violations.push(red("ng.review.problems", format!("{at} 不是对象")));
            continue;
        };
        for key in ["node", "field", "issue"] {
            if problem_map.get(key).and_then(Value::as_str).unwrap_or_default().trim().is_empty() {
                violations.push(red(
                    "ng.review.problems",
                    format!("{at} 的 {key} 缺失或为空"),
                ));
            }
        }
        let field = problem_map.get("field").and_then(Value::as_str).unwrap_or_default();
        if !REVIEW_FIELDS.contains(&field) {
            violations.push(red(
                "ng.review.problems",
                format!("{at} 的 field 必须是 {} 之一：「{field}」", REVIEW_FIELDS.join("/")),
            ));
        }
    }
    violations
}

// ── JSON Schema 单源（由同一份常量装配；schema↔validator 互钉测试锁定）───────

/// 生长批产物的 JSON Schema（词表/键集/量级全部引自本模块常量）。
pub fn proposal_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "nodes": {
                "type": "array",
                "maxItems": MAX_BATCH_NODES,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "purpose"],
                    "properties": {
                        "title": { "type": "string", "minLength": 1, "maxLength": TITLE_MAX_CHARS },
                        "purpose": { "type": "string", "minLength": 1 },
                        "est": {
                            "type": "integer",
                            "minimum": EST_RANGE.0,
                            "maximum": EST_RANGE.1,
                        },
                        "difficulty": {
                            "type": "integer",
                            "minimum": DIFFICULTY_RANGE.0,
                            "maximum": DIFFICULTY_RANGE.1,
                        },
                        "teaches": { "$ref": "#/$defs/refs" },
                        "assumes": { "$ref": "#/$defs/refs" },
                    },
                },
            },
            "mints": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["canonical"],
                    "properties": {
                        "canonical": { "type": "string", "minLength": 1 },
                        "aliases": { "type": "array", "items": { "type": "string", "minLength": 1 } },
                        "definition": { "type": "string" },
                    },
                },
            },
            "new_aliases": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "definition", "source"],
                    "properties": {
                        "name": { "type": "string", "minLength": 1 },
                        "definition": { "type": "string", "minLength": 1 },
                        "source": { "type": "string", "minLength": 1 },
                    },
                },
            },
            "note": { "type": "string" },
        },
        "$defs": {
            "refs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "tier"],
                    "properties": {
                        "name": { "type": "string", "minLength": 1 },
                        "tier": { "enum": TIERS },
                    },
                },
            },
        },
    })
}

/// 概念评审输出的 JSON Schema。
pub fn review_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["verdict"],
        "properties": {
            "verdict": { "enum": REVIEW_VERDICTS },
            "problems": {
                "type": "array",
                "maxItems": REVIEW_PROBLEMS_MAX,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["node", "field", "issue"],
                    "properties": {
                        "node": { "type": "string", "minLength": 1 },
                        "field": { "enum": REVIEW_FIELDS },
                        "issue": { "type": "string", "minLength": 1 },
                        "advice": { "type": "string" },
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

    fn golden_batch() -> Value {
        json!({
            "nodes": [
                { "title": "用定义计算极限", "purpose": "从数格子接近某数的经验切入",
                  "est": 15, "difficulty": 2,
                  "teaches": [{ "name": "极限", "tier": "know" }], "assumes": [] },
                { "title": "用极限法则求切线", "purpose": "从坡道倾斜度的日常经验切入",
                  "est": 20, "difficulty": 3,
                  "teaches": [{ "name": "求导法则", "tier": "apply" }],
                  "assumes": [{ "name": "极限", "tier": "know" }] }
            ],
            "mints": [
                { "canonical": "极限", "aliases": ["Limits"], "definition": "无限逼近的值" },
                { "canonical": "求导法则", "aliases": [], "definition": "" }
            ],
            "note": "首批两节点"
        })
    }

    #[test]
    fn golden_batch_passes_shape_contract() {
        assert!(validate_proposal(&golden_batch()).is_empty());
    }

    #[test]
    fn legacy_tolerant_shapes_are_now_violations() {
        // 容错 deserializer 退役：字符串 est + 退役键名 minutes + 中文档位 +
        // 单对象引用形状。
        let value = json!({
            "nodes": [{
                "title": "用配方法解方程", "purpose": "p",
                "est": "15",
                "minutes": 15,
                "teaches": { "name": "配方法", "tier": "会用" },
                "assumes": []
            }],
            "mints": []
        });
        let codes: Vec<&str> = validate_proposal(&value).iter().map(|v| v.code).collect();
        assert!(codes.contains(&"ng.nodes.est"), "{codes:?}");
        assert!(codes.contains(&"ng.keys"), "{codes:?}");
        assert!(codes.contains(&"ng.refs.shape"), "{codes:?}");
    }

    #[test]
    fn chinese_tier_within_array_is_a_violation() {
        let value = json!({
            "nodes": [{
                "title": "n", "purpose": "p",
                "teaches": [{ "name": "配方法", "tier": "知道" }]
            }],
            "mints": []
        });
        let violations = validate_proposal(&value);
        assert!(violations.iter().any(|v| v.code == "ng.refs.tier"), "{violations:?}");
    }

    #[test]
    fn batch_size_and_title_uniqueness_are_enforced() {
        let nodes: Vec<Value> = (0..8)
            .map(|index| json!({ "title": format!("节点{index}"), "purpose": "p" }))
            .collect();
        let value = json!({ "nodes": nodes, "mints": [] });
        assert!(validate_proposal(&value).iter().any(|v| v.code == "ng.batch.size"));

        let value = json!({
            "nodes": [
                { "title": "重复标题", "purpose": "p" },
                { "title": "重复标题", "purpose": "p" }
            ],
            "mints": []
        });
        assert!(validate_proposal(&value).iter().any(|v| v.code == "ng.title.batch_unique"));
    }

    #[test]
    fn batch_internal_mint_collisions_are_shape_violations() {
        let value = json!({
            "nodes": [],
            "mints": [
                { "canonical": "极限", "aliases": ["Limits"] },
                { "canonical": "LIMITS" }
            ]
        });
        assert!(validate_proposal(&value).iter().any(|v| v.code == "ng.mints.batch_unique"));
    }

    #[test]
    fn unknown_keys_are_violations() {
        let mut value = golden_batch();
        value["surprise"] = json!(1);
        value["nodes"][0]["definition"] = json!("判据形字段不进节点");
        let codes: Vec<&str> = validate_proposal(&value).iter().map(|v| v.code).collect();
        assert!(codes.contains(&"ng.keys"), "{codes:?}");
    }

    #[test]
    fn empty_batch_is_legal() {
        let value = json!({ "nodes": [], "mints": [], "note": "缺口 0" });
        assert!(validate_proposal(&value).is_empty());
    }

    #[test]
    fn new_alias_nominations_are_validated() {
        // 三字段齐全 = 合法。
        let value = json!({
            "nodes": [],
            "mints": [],
            "new_aliases": [
                { "name": "十字相乘", "definition": "把二次三项式拆成两因式乘积的技巧", "source": "教练提名：本批换一个操作角度" }
            ],
            "note": "n"
        });
        assert!(validate_proposal(&value).is_empty(), "{:?}", validate_proposal(&value));
        // 缺 source = ng.aliases.required。
        let bad = json!({
            "nodes": [], "mints": [],
            "new_aliases": [{ "name": "十字相乘", "definition": "d" }],
            "note": "n"
        });
        let codes: Vec<&str> = validate_proposal(&bad).iter().map(|v| v.code).collect();
        assert!(codes.contains(&"ng.aliases.required"), "{codes:?}");
        // 与批内铸名互撞 = ng.aliases.atomic_name。
        let clash = json!({
            "nodes": [], "mints": [{ "canonical": "十字相乘", "definition": "d" }],
            "new_aliases": [{ "name": "十字相乘", "definition": "另一个定义", "source": "s" }],
            "note": "n"
        });
        let codes: Vec<&str> = validate_proposal(&clash).iter().map(|v| v.code).collect();
        assert!(codes.contains(&"ng.aliases.atomic_name"), "{codes:?}");
    }

    #[test]
    fn review_contract_accepts_pass_and_valid_reject() {
        assert!(validate_review(&json!({ "verdict": "pass" })).is_empty());
        let reject = json!({
            "verdict": "reject",
            "problems": [
                { "node": "节点A", "field": "assumes", "issue": "缺「比例」", "advice": "补铸名" }
            ]
        });
        assert!(validate_review(&reject).is_empty());
    }

    #[test]
    fn review_contract_rejects_bad_verdict_and_fields() {
        let violations = validate_review(&json!({ "verdict": "maybe" }));
        assert!(violations.iter().any(|v| v.code == "ng.review.verdict"), "{violations:?}");

        let reject = json!({
            "verdict": "reject",
            "problems": [{ "node": "n", "field": "mood", "issue": "i" }]
        });
        assert!(validate_review(&reject).iter().any(|v| v.code == "ng.review.problems"));
    }

    /// 互钉：schema 的词表/键集/量级与 validator 常量同源。
    #[test]
    fn schema_is_pinned_to_validator_constants() {
        let schema = proposal_schema();
        let node_items = &schema["properties"]["nodes"]["items"];
        assert_eq!(node_items["required"], json!(["title", "purpose"]));
        assert_eq!(node_items["properties"]["est"]["maximum"], json!(EST_RANGE.1));
        assert_eq!(node_items["properties"]["difficulty"]["maximum"], json!(DIFFICULTY_RANGE.1));
        assert_eq!(schema["properties"]["nodes"]["maxItems"], json!(MAX_BATCH_NODES));
        assert_eq!(schema["$defs"]["refs"]["items"]["properties"]["tier"]["enum"], json!(TIERS));
        assert_eq!(schema["properties"]["mints"]["items"]["required"], json!(["canonical"]));

        let review = review_schema();
        assert_eq!(review["properties"]["verdict"]["enum"], json!(REVIEW_VERDICTS));
        assert_eq!(
            review["properties"]["problems"]["items"]["properties"]["field"]["enum"],
            json!(REVIEW_FIELDS)
        );
        assert_eq!(review["properties"]["problems"]["maxItems"], json!(REVIEW_PROBLEMS_MAX));
    }

    /// 金样反钉：golden 批必须逐键落在 schema 的白名单内。
    #[test]
    fn golden_batch_satisfies_schema_whitelist() {
        let schema = proposal_schema();
        let value = golden_batch();
        let top_keys: Vec<&str> = schema["properties"].as_object().unwrap().keys().map(String::as_str).collect();
        for key in value.as_object().unwrap().keys() {
            assert!(top_keys.contains(&key.as_str()), "top key「{key}」not in schema");
        }
        let node_keys: Vec<&str> = schema["properties"]["nodes"]["items"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        for node in value["nodes"].as_array().unwrap() {
            for key in node.as_object().unwrap().keys() {
                assert!(node_keys.contains(&key.as_str()), "node key「{key}」not in schema");
            }
        }
    }
}
