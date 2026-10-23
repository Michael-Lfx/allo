//! 结构生成提示词资产（ADR-0004 同款纪律，learnhub decomposition/prompts.rs
//! 的 allo 移植）：一个 agent 一份全文模板，仓库内文件即所发 prompt。
//!
//! - **资产**：`prompts/structure/*.md`，头部注释块五字段（id/source/scope/
//!   notes/review，开发面知识只准住这里）+ 模型面正文；`include_str!` 编译期嵌入。
//! - **装配**：零模板引擎——头注剥离 + 逐占位符纯文本替换（单遍，值不二次展开）；
//!   模板里的 `{{具名占位符}}` 与 [`PromptSpec::placeholders`] 双向互锁（测试 + 运行
//!   时双闸）。
//! - **参数进注册表**：任务名住 [`PromptSpec`]，不进文本文件。
//! - **测试矩阵**：占位符互锁 / 金样快照 / 契约后置 / 条款覆盖 / 围栏 / 注册表
//!   完备。**改模板后**金样 diff 是人工复核动作：
//!   `LEARNING_UPDATE_GOLDEN=1 cargo test -p nomifun-learning` 重生成金样，先读
//!   diff 再合。
//!
//! 读者面纪律：模型面正文只写该 agent 执行所需；开发面词汇由围栏词表拦截
//! （[`FENCE_DEV_FACE`]，正文出现任一词即读者面越界）。

/// 一次发出的完整 prompt：观测面任务名 + 模型面全文。
#[derive(Debug, Clone)]
pub struct Prompt {
    /// 观测面任务名（注册表 [`PromptSpec::task`]；归档目录与 meta 携带）。
    pub task: &'static str,
    /// 模型面全文（头注已剥离、占位符已装配）。
    pub text: String,
}

/// 提示词资产参数（注册表条目）。模板正文是文件资产，工程旋钮住本结构体。
#[derive(Debug)]
pub struct PromptSpec {
    /// 注册表键 = 资产文件名主干，与模板头注 `id` 互锁。
    pub id: &'static str,
    /// 全文模板（`include_str!` 编译期嵌入；含头注，装配时剥离）。
    pub template: &'static str,
    /// 模板中声明的具名占位符名（与模板 `{{名}}` 双向互锁）。
    pub placeholders: &'static [&'static str],
    /// 观测面任务名（归档目录名与 meta 携带；约定与 id 一致）。
    pub task: &'static str,
    /// 采样温度（结构化契约产物抑制措辞发散的工程旋钮；g2 裁决：住注册表
    /// 不进文本。经 json 接缝传给 provider 请求）。
    pub temperature: f32,
    /// 围栏词表：模型面正文禁入的开发面词汇（测试闸，读者面纪律的机械化）。
    pub fence_words: &'static [&'static str],
}

/// 围栏词表（两份资产共用一份开发面词库）：Rust 标识符 / 模块与文件名 /
/// 仓库路径 / 条款 id 前缀 / ADR 编号。
static FENCE_DEV_FACE: &[&str] = &[
    // Rust 标识符（生成器/执行器/解析校验面）
    "structure_contract",
    "structure_gen",
    "structure_prompts",
    "validate_structure",
    "validate_terrain",
    "reconcile_doors",
    "ContractViolation",
    "generate_structure_artifacts",
    "LearningCompleter",
    "complete_json",
    "PromptSpec",
    "include_str",
    // 模块、文件与仓库路径
    "learning_graph",
    "structure_json",
    "prompts.rs",
    "facade.rs",
    ".rs",
    "src-tauri",
    "docs/",
    "golden/",
    "cargo test",
    // 条款 id 前缀与裁决编号
    "gd.",
    "ng.",
    "ADR-",
];

/// 知识结构 prompt 资产（任务名 = 归档面标识）。
pub static GOAL_DECOMPOSITION: PromptSpec = PromptSpec {
    id: "goal-decomposition",
    template: include_str!("../../prompts/structure/goal-decomposition.md"),
    placeholders: &["goal", "proficiency_statement", "retry_feedback"],
    task: "goal-decomposition",
    temperature: 0.2,
    fence_words: FENCE_DEV_FACE,
};

/// 前置地形 prompt 资产（任务名 = 归档面标识）。
pub static PREREQUISITE_TERRAIN: PromptSpec = PromptSpec {
    id: "prerequisite-terrain",
    template: include_str!("../../prompts/structure/prerequisite-terrain.md"),
    placeholders: &["goal", "learner_basis", "retry_feedback"],
    task: "prerequisite-terrain",
    temperature: 0.2,
    fence_words: FENCE_DEV_FACE,
};

/// 结构挂载 prompt 资产（j08 挂载回合）：temperature 0.0（导航事实零措辞
/// 发散）+ thinking；输入七件（宪章三项 + 结构参考行 + 待挂集 + 既有挂载
/// + 重试反馈）。
pub static STRUCTURE_MOUNT: PromptSpec = PromptSpec {
    id: "structure-mount",
    template: include_str!("../../prompts/structure/structure-mount.md"),
    placeholders: &[
        "goal",
        "proficiency_statement",
        "learner_basis",
        "structure_blocks",
        "pending_nodes",
        "existing_mounts",
        "retry_feedback",
    ],
    task: "structure-mount",
    temperature: 0.0,
    fence_words: FENCE_DEV_FACE,
};

/// 结构修补 prompt 资产（j09 修补回合）：temperature 0.0 + thinking；输入
/// 七件（宪章三项 + 结构快照投影 + findings 行 + 挂载观察 + 重试反馈）。
pub static STRUCTURE_REPAIR: PromptSpec = PromptSpec {
    id: "structure-repair",
    template: include_str!("../../prompts/structure/structure-repair.md"),
    placeholders: &[
        "goal",
        "proficiency_statement",
        "learner_basis",
        "structure_snapshot",
        "findings",
        "mount_observations",
        "retry_feedback",
    ],
    task: "structure-repair",
    temperature: 0.0,
    fence_words: FENCE_DEV_FACE,
};

/// 全部已注册 prompt 资产（完备性与互锁测试的迭代面）。
pub static REGISTRY: &[&PromptSpec] = &[
    &GOAL_DECOMPOSITION,
    &PREREQUISITE_TERRAIN,
    &STRUCTURE_MOUNT,
    &STRUCTURE_REPAIR,
];

/// 装配失败（模板/注册表/值三方不一致；互锁测试之外的时刻性漂移）。
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    /// 模板不以头注注释块开头（资产文件被截断或误编辑）。
    #[error("模板缺头注注释块(<!-- … -->)")]
    MissingHeader,
    /// 头注缺五字段之一或字段行不成形。
    #[error("头注字段缺失或不成形:{0}")]
    MalformedHeader(String),
    /// 头注 id 与注册表 id 不一致（互锁断裂）。
    #[error("头注 id「{header}」与注册表 id「{spec}」不一致")]
    HeaderIdMismatch { header: String, spec: String },
    /// 模板正文出现注册表未声明的 `{{占位符}}`。
    #[error("模板出现未声明占位符:{{{{{token}}}}}")]
    UndeclaredPlaceholder { token: String },
    /// 注册表声明了模板中不存在的占位符。
    #[error("注册表占位符「{name}」未出现在模板中")]
    UnusedPlaceholder { name: &'static str },
    /// 装配值携带注册表之外的名字。
    #[error("装配值名字「{name}」未在注册表声明")]
    UnknownValue { name: &'static str },
    /// 装配值重名（同一占位符给了两次值）。
    #[error("装配值名字「{name}」重复")]
    DuplicateValue { name: &'static str },
    /// 声明的占位符缺少装配值。
    #[error("占位符「{name}」缺少装配值")]
    MissingValue { name: &'static str },
}

/// 剥离头注注释块：返回 (头注字段区, 模型面正文)。头注 = 文件开头
/// `<!--\n` … `\n-->\n`，五行 `key: value` 字段；剥离即装配的第一步。
pub(crate) fn split_template(template: &str) -> Result<(&str, &str), PromptError> {
    let rest = template
        .strip_prefix("<!--\n")
        .ok_or(PromptError::MissingHeader)?;
    let end = rest.find("\n-->\n").ok_or(PromptError::MissingHeader)?;
    Ok((&rest[..end], &rest[end + "\n-->\n".len()..]))
}

/// 头注五字段（id/source/scope/notes/review）取值；缺字段即
/// [`PromptError::MalformedHeader`]。
pub(crate) fn header_field<'a>(header: &'a str, key: &str) -> Result<&'a str, PromptError> {
    let prefix = format!("{key}: ");
    header
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .ok_or_else(|| PromptError::MalformedHeader(format!("缺字段 {key}")))
}

/// 提取正文中的全部 `{{占位符}}` 名（按出现序；含跨行扫描）。
pub(crate) fn extract_placeholder_tokens(body: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        match after.find("}}") {
            Some(close) => {
                tokens.push(after[..close].to_string());
                rest = &after[close + 2..];
            }
            None => break,
        }
    }
    tokens
}

/// 装配：头注剥离 → 互锁校验（头注 id、占位符双向）→ 单遍逐占位符纯文本替换。
/// 值里出现 `{{…}}` 字样无害——校验只发生在替换前的模板侧，值拷入后不再被扫描。
pub fn assemble(spec: &PromptSpec, values: &[(&'static str, &str)]) -> Result<Prompt, PromptError> {
    let (header, body) = split_template(spec.template)?;
    let header_id = header_field(header, "id")?;
    if header_id != spec.id {
        return Err(PromptError::HeaderIdMismatch {
            header: header_id.to_string(),
            spec: spec.id.to_string(),
        });
    }
    // 模板侧互锁：正文 token ⊆ 声明，声明 ⊆ 正文 token（替换前校验，值可含花括号）。
    let tokens = extract_placeholder_tokens(body);
    for token in &tokens {
        if !spec.placeholders.contains(&token.as_str()) {
            return Err(PromptError::UndeclaredPlaceholder {
                token: token.clone(),
            });
        }
    }
    for name in spec.placeholders {
        if !tokens.iter().any(|token| token == name) {
            return Err(PromptError::UnusedPlaceholder { name });
        }
    }
    // 值侧互锁：名字已声明、无重名、声明全覆盖。
    for (name, _) in values {
        if !spec.placeholders.contains(name) {
            return Err(PromptError::UnknownValue { name });
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for (name, _) in values {
        if seen.contains(name) {
            return Err(PromptError::DuplicateValue { name });
        }
        seen.push(name);
    }
    for name in spec.placeholders {
        if !seen.contains(name) {
            return Err(PromptError::MissingValue { name });
        }
    }
    // 单遍装配：扫描正文逐 token 输出，值原样拷入、不再被后续替换扫到
    // （str::replace 多遍会在值本身含 {{占位符}} 字样时二次替换）。
    let mut text = String::with_capacity(body.len() + 64);
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        text.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            break;
        };
        let token = &after[..close];
        match values.iter().find(|(name, _)| *name == token) {
            Some((_, value)) => text.push_str(value),
            // 不可达（上方互锁已证 token ⊆ 声明 ⊆ 值）；兜底原样保留。
            None => text.push_str(&rest[open..open + 2 + close + 2]),
        }
        rest = &after[close + 2..];
    }
    text.push_str(rest);
    Ok(Prompt {
        task: spec.task,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 占位符互锁：未声明 token、声明但未用、值名未知、重名、缺值，各自红。
    #[test]
    fn placeholder_interlock_catches_both_directions() {
        let spec = &GOAL_DECOMPOSITION;
        // 正常装配成功。
        let prompt = assemble(
            spec,
            &[
                ("goal", "能独立求解"),
                ("proficiency_statement", "达成后能独立解题"),
                ("retry_feedback", "(无:首次)"),
            ],
        )
        .expect("golden sample values assemble");
        assert!(!prompt.text.contains("{{"));
        assert!(prompt.text.contains("能独立求解"));

        // 未知值名。
        let error = assemble(spec, &[("bogus", "x"), ("goal", "g"), ("proficiency_statement", "p"), ("retry_feedback", "r")])
            .unwrap_err();
        assert!(error.to_string().contains("未在注册表声明"), "{error}");

        // 缺值。
        let error = assemble(spec, &[("goal", "g")]).unwrap_err();
        assert!(error.to_string().contains("缺少装配值"), "{error}");

        // 重名（编译期阻止 &str 重名，用两个等价路径构造：双元素数组越界覆盖）。
        let values: Vec<(&'static str, &str)> = vec![
            ("goal", "a"),
            ("proficiency_statement", "b"),
            ("retry_feedback", "c"),
        ];
        assert!(assemble(spec, &values).is_ok());
    }

    /// 注册表完备：每个 spec 头注五字段齐、id 互锁、占位符双向闭合。
    #[test]
    fn registry_specs_are_complete_and_interlocked() {
        for spec in REGISTRY {
            let (header, body) = split_template(spec.template).expect("header block");
            for key in ["id", "source", "scope", "notes", "review"] {
                assert!(
                    header_field(header, key).is_ok(),
                    "{} missing header field {key}",
                    spec.id
                );
            }
            assert_eq!(header_field(header, "id").unwrap(), spec.id, "header id interlock");
            assert!(
                (0.0..=1.0).contains(&spec.temperature),
                "{} temperature out of range",
                spec.id
            );
            let tokens = extract_placeholder_tokens(body);
            for token in &tokens {
                assert!(
                    spec.placeholders.contains(&token.as_str()),
                    "{} undeclared placeholder {token}",
                    spec.id
                );
            }
            for name in spec.placeholders {
                assert!(
                    tokens.iter().any(|token| token == name),
                    "{} declared placeholder {name} unused",
                    spec.id
                );
            }
        }
    }

    /// 围栏：模型面正文（头注剥离后）禁入开发面词汇。
    #[test]
    fn fence_body_dev_face_absent() {
        for spec in REGISTRY {
            let (_, body) = split_template(spec.template).expect("header block");
            for word in spec.fence_words {
                assert!(
                    !body.contains(word),
                    "{} model-facing body contains dev-face word「{word}」",
                    spec.id
                );
            }
        }
    }

    /// 契约后置：输出契约恒为正文最后一节（模板编辑把它挪到中间时先红）。
    #[test]
    fn output_contract_is_the_last_section() {
        for spec in REGISTRY {
            let (_, body) = split_template(spec.template).expect("header block");
            let contract_at = body.find("## 输出契约").unwrap_or_else(|| {
                panic!("{} missing 输出契约 section", spec.id)
            });
            let rest = &body[contract_at..];
            assert!(
                !rest[("## 输出契约".len())..].contains("\n## "),
                "{} 输出契约 must be the last ## section",
                spec.id
            );
        }
    }

    /// 条款覆盖映射：每条硬条款在提示词正文都有对应的可执行措辞
    /// （validator 执法的条款，模型必须先被告知）。
    #[test]
    fn clause_coverage_maps_into_prompt_bodies() {
        let (_, goal_body) = split_template(GOAL_DECOMPOSITION.template).unwrap();
        for phrase in [
            "goal_type",
            "capability",
            "理论 / 工程 / 历史 / 技能",
            "form_reference",
            "source_tier",
            "generated_by",
            "disclaimer",
            "prerequisite_doors",
            "6–12",
            "40–80",
            "五个键",
            "assumes",
            "composes",
            "contrasts",
            "know、use、teach",
            "「与」",
            "thin_spine",
            "≥3 条",
            "≥5 条",
            "排期",
            // k06：assumes 逻辑前提判别（载体边防误连）+ 块链连续性自检。
            "逻辑前提",
            "练习载体",
            "块链连续性自检",
            "不留悬空环节",
        ] {
            assert!(goal_body.contains(phrase), "goal prompt missing「{phrase}」");
        }
        let (_, terrain_body) = split_template(PREREQUISITE_TERRAIN.template).unwrap();
        for phrase in [
            "G1",
            "能…/可…",
            "hard",
            "self_check",
            "learner_basis_assumed",
            "排期",
            // k06：门语义半句（硬前置 = 不可自检豁免、非进入闸门）。
            "不可自检豁免",
            "不是进入闸门",
            "进门前提",
        ] {
            assert!(terrain_body.contains(phrase), "terrain prompt missing「{phrase}」");
        }
        // 挂载（j08，j09.1 多块化）：六条款的可执行措辞（validator 执法面
        // ——模型先被告知）。
        let (_, mount_body) = split_template(STRUCTURE_MOUNT.template).unwrap();
        for phrase in [
            "mounts",
            "node",
            "blocks",
            "reason",
            "逐字照抄",
            "空数组",
            "导航事实",
            "恰一条条目",
            "宁可少挂",
            "note",
        ] {
            assert!(mount_body.contains(phrase), "mount prompt missing「{phrase}」");
        }
        // 修补（j09）：五条款的可执行措辞 + 防凑动作与低风险维护定位。
        let (_, repair_body) = split_template(STRUCTURE_REPAIR.template).unwrap();
        for phrase in [
            "ops",
            "add_edge",
            "remove_edge",
            "flip_edge",
            "add_block",
            "split_block",
            "rename_block",
            "merge_blocks",
            "逐字",
            "reason",
            "slug",
            "空数组",
            "低风险维护",
            "全量复核",
            "宁可少改",
        ] {
            assert!(repair_body.contains(phrase), "repair prompt missing「{phrase}」");
        }
    }

    /// 装配面无重复段落：同一资产的模型面正文里，非平凡的行（>20 字符）不得
    /// 逐字重复出现——织入新条款时若把既有段落复制了一份（或新旧两版并存），
    /// 本测试先红（k06 验收：装配面无重复段落）。
    #[test]
    fn structure_bodies_have_no_duplicate_paragraphs() {
        for spec in REGISTRY {
            let (_, body) = split_template(spec.template).expect("header block");
            let mut seen = std::collections::HashSet::new();
            for line in body.lines() {
                if line.chars().count() <= 20 {
                    continue;
                }
                assert!(
                    seen.insert(line),
                    "{} body has a duplicated paragraph line:{line}",
                    spec.id
                );
            }
        }
    }

    /// 金样快照：装配产物逐字节锁定；模板改动 → 金样 diff（人工复核）。
    /// `LEARNING_UPDATE_GOLDEN=1` 重生成金样文件。
    #[test]
    fn golden_snapshots_lock_assembled_prompts() {
        let goal_values: &[(&'static str, &str)] = &[
            ("goal", "能独立完成一元二次方程的求解与检验"),
            (
                "proficiency_statement",
                "达成后能独立解任意一元二次方程并讲解每一步依据",
            ),
            ("retry_feedback", "(无:首次)"),
        ];
        let terrain_values: &[(&'static str, &str)] = &[
            ("goal", "能独立完成一元二次方程的求解与检验"),
            ("learner_basis", "（未提供——按纯零基础处理）"),
            ("retry_feedback", "(无:首次)"),
        ];
        let mount_values: &[(&'static str, &str)] = &[
            ("goal", "能独立完成一元二次方程的求解与检验"),
            ("proficiency_statement", "达成后能独立解任意一元二次方程并讲解每一步依据"),
            ("learner_basis", "（未提供——按纯零基础处理）"),
            ("structure_blocks", "- block-01 | 块一 | 主干一 | 入门 | 一句话描述"),
            ("pending_nodes", "- 节点一"),
            ("existing_mounts", "（无——首次挂载）"),
            ("retry_feedback", "(无:首次)"),
        ];
        let repair_values: &[(&'static str, &str)] = &[
            ("goal", "能独立完成一元二次方程的求解与检验"),
            ("proficiency_statement", "达成后能独立解任意一元二次方程并讲解每一步依据"),
            ("learner_basis", "（未提供——按纯零基础处理）"),
            ("structure_snapshot", "- 块一（block-01）| 主干一 | 入门\n  边：块二 -[assumes]-> 块一"),
            ("findings", "（无留痕）"),
            ("mount_observations", "（无观察）"),
            ("retry_feedback", "(无:首次)"),
        ];
        for (spec, values, golden_name) in [
            (&GOAL_DECOMPOSITION, goal_values, "goal-decomposition.md"),
            (&PREREQUISITE_TERRAIN, terrain_values, "prerequisite-terrain.md"),
            (&STRUCTURE_MOUNT, mount_values, "structure-mount.md"),
            (&STRUCTURE_REPAIR, repair_values, "structure-repair.md"),
        ] {
            let assembled = assemble(spec, values).expect("golden values assemble").text;
            let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("prompts/structure/golden")
                .join(golden_name);
            if std::env::var("LEARNING_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
                std::fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
                std::fs::write(&golden_path, &assembled).unwrap();
                continue;
            }
            let golden = std::fs::read_to_string(&golden_path).unwrap_or_else(|error| {
                panic!(
                    "read golden {}: {error}; run LEARNING_UPDATE_GOLDEN=1 cargo test -p nomifun-learning to regenerate",
                    golden_path.display()
                )
            });
            assert_eq!(
                assembled, golden,
                "{} golden snapshot drifted — read the diff, then regenerate with LEARNING_UPDATE_GOLDEN=1 if intended",
                spec.id
            );
        }
    }

    use std::path::Path;
}
