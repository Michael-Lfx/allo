//! 已会声明管线（m2，learnhub ADR-0032 / #164 的域中立移植）：学习者勾选
//! 满足的地形门口判据，系统按声明铸概念（走既有铸名纪律），**只铸概念不落
//! 节点**——不排课、不计 XP、不建任何学习行为。已知集 = 已教 ∪ 已会声明。
//!
//! 流程（learnhub basis_declaration/facade.rs 同构）：勾选校验（⊆ 课程门口）
//! → 声明差集（幂等增量，空 = noop 零 LLM 零写）→ 命中块（m1 `door_refs`
//! AND 口径：块的 door_refs 全部落在勾选集内才算命中）→ 一次结构化 LLM 调用
//! 产概念清单（最小可教单元条款 + 1..60）→ 既有命名门（defnear：名字精确
//! 占用 / 定义归一化直定宿主机械复用；歧义近邻走 j04 复用裁决通道——评审
//! reuse 语义，落池必经 [`alias_records::register_alias_tx`] 登记单点，不得
//! 绕过）→ 单事务写锚行（锚表即铸造出处）。
//!
//! 撤销语义（grill 定案「声明可撤、概念不灭」）：撤销 = 删本课锚行、收缩本
//! 课已知投影；概念留池（登记表全局有效），来源标记保留不抹。
//!
//! 用户面正名：本机制统一叫**「已会声明」（Declared Known）**——「基础声明」
//! 已被建课向导的自由文本输入（`learner_basis`）占用；路由 slug 与 smoke 档
//! 名可留 `basis`（代码面命名），用户可见措辞一律「已会声明」。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::learning_graph::structure_prompts::PromptSpec;
use crate::models::BlockView;

/// 已会声明的运维开关：`NOMI_LEARNING_BASIS_DECLARATION` 置 off/0/false 时
/// 端点如实拒绝（409），机制其余部分无行为差异。
pub const BASIS_DECLARATION_SWITCH_ENV: &str = "NOMI_LEARNING_BASIS_DECLARATION";

/// 开关取值判定（off/0/false；大小写与首尾空白宽容——公共判定单源）。
pub fn is_basis_declaration_disabled_value(value: &str) -> bool {
    crate::is_off_switch_value(value)
}

/// 机制是否开启（未设/其它值 = 开启）。
pub fn basis_declaration_enabled() -> bool {
    std::env::var(BASIS_DECLARATION_SWITCH_ENV)
        .map(|value| !is_basis_declaration_disabled_value(&value))
        .unwrap_or(true)
}

/// 围栏词表（开发面词汇，模型面正文禁入）：Rust 标识符 / 模块与文件名 /
/// 仓库路径 / 表名 / 开关名。
static FENCE_DEV_FACE: &[&str] = &[
    "BasisDeclarer",
    "DeclarationNomination",
    "DeclarationBatch",
    "LearningCompleter",
    "register_alias_tx",
    "AliasSpec",
    "defnear",
    "PromptSpec",
    "include_str",
    "basis_declaration",
    "learning_graph",
    "structure_prompts",
    "learning_basis_declarations",
    "learning_concept_registry",
    "prompts.rs",
    "facade.rs",
    ".rs",
    "src-tauri",
    "docs/",
    "golden/",
    "cargo test",
    "NOMI_LEARNING_",
    "gd.",
    "ng.",
    "ADR-",
];

/// 已会声明 prompt 资产（任务名 = 归档面标识）：temperature 0.2（概念清单
/// 抑制措辞发散）+ thinking；输入四件（目标 / 程度声明 / 已具备块行 / 前置
/// 地形）。**只铸概念、不落节点**。
pub static BASIS_DECLARATION: PromptSpec = PromptSpec {
    id: "basis-declaration",
    template: include_str!("../../prompts/basis-declaration/basis-declaration.md"),
    placeholders: &["goal", "proficiency_statement", "target_blocks", "terrain"],
    task: "basis-declaration",
    temperature: 0.2,
    fence_words: FENCE_DEV_FACE,
};

/// 概念清单的条数上限（learnhub 12→60 同一条教训：上限压出主题级铸造，
/// 污染后续生成——上限 60 与最小可教单元条款是同一条教训的两半）。
pub const MAX_DECLARED_CONCEPTS: usize = 60;

/// 命名门复用裁决的评审系统提示词（j04 reuse 通道，声明面的窄化版）：仅当
/// 机械面（名字精确占用 / 定义归一化直定宿主）无法定夺、且近邻候选非空时
/// 才送裁；裁决落池必经登记单点。失败 = 如实失败（零部分写入），不绕门。
pub const REUSE_RULING_SYSTEM: &str = r#"你是概念登记表的命名裁决员。学习者声明已会的内容正在铸概念入册，对每条提名，依据它的近邻候选（登记表现有概念 + 入选理由），逐条裁决「复用 | 新铸」：
- 复用 = 该提名的名字或定义其实是候选中某个既有概念的另一投影——该概念已在册，不重铸新行；提名名会落为该概念的活跃别名。
- 新铸 = 提名确实是登记表里没有的新概念。
判据：定义近邻/名字近形只是机械线索；拿不准时按「同一概念用了两个不同名字应复用既有名」与候选定义比对裁决——宁可复用也不重铸同义概念，但确实不同的对象不要硬并成一条。
只输出 JSON（除它之外不输出任何字符）：{"rulings":[{"name":"<提名名逐字照抄>","verdict":"reuse","concept_id":"<该提名候选清单里真实存在的 concept_id>"} 或 {"name":"<提名名逐字照抄>","verdict":"mint"}]}。每条提名恰一条裁决，不得遗漏、不得发明候选之外的 concept_id。"#;

/// 概念铸造调用的系统提示词（装配模板走 user 消息——structure_gen 同款双段
/// 形制；这里只钉立场：只铸概念、严格按契约输出）。
pub const DECLARATION_SYSTEM: &str = "你是学习概念登记表的铸造执行者。严格按用户消息里的任务与输出契约产出 JSON：只铸概念（名字 + 一句话定义 + 出处说明），不产出学习节点、不写教学步骤、不设计例题与练习；模板未要求的内容一律不输出。";

/// 登记表来源标记（m2 定案）：已会声明铸入的概念在
/// `learning_concept_registry.source` 上的恒值——概念行来源徽标与
/// 「来自哪次声明」反查（锚表）的锚点。
pub const DECLARATION_SOURCE: &str = "basis-declaration";

/// 一次声明铸出/复用的概念（返回面）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclaredConceptView {
    pub concept_id: String,
    pub name: String,
    /// 新铸 | 复用（复用 = 命名门撞既有概念，入池不重铸）。
    pub minted: bool,
}

/// 一条模型提名的概念（结构化调用的四字段契约，learnhub 同形）。
#[derive(Debug, Clone, Deserialize)]
pub struct DeclarationNomination {
    pub name: String,
    /// 机器标识（契约四字段之一；allo 登记表不消费 slug——名字唯一性由
    /// 命名门的归一化查重承担，此字段仅保形不落库）。
    #[serde(default)]
    #[allow(dead_code)]
    pub slug: String,
    pub definition: String,
    #[serde(default)]
    pub source: String,
}

/// 结构化调用的出参形制（顶层键仅 concepts）。
#[derive(Debug, Deserialize)]
pub struct DeclarationBatch {
    pub concepts: Vec<DeclarationNomination>,
}

/// 命中块行（AND 口径的产物）：块视图 + 本次勾选集中实际命中的门口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetBlock {
    pub name: String,
    pub slug: String,
    pub spine: String,
    pub stage: String,
    pub note: String,
    pub doors: Vec<String>,
}

/// 命中块（AND 口径，learnhub target_rows 同构）：`door_refs` 非空且**全部**
/// ⊆ 勾选集；呈现序随块表序（m1 的读侧同源）。部分 door_refs 命中不算。
pub fn target_rows(blocks: &[BlockView], checked: &BTreeSet<String>) -> Vec<TargetBlock> {
    blocks
        .iter()
        .filter(|block| !block.door_refs.is_empty())
        .filter(|block| block.door_refs.iter().all(|door| checked.contains(door)))
        .map(|block| TargetBlock {
            name: block.name.clone(),
            slug: block.slug.clone(),
            spine: block.spine.clone(),
            stage: block.stage.clone(),
            note: block.note.clone(),
            doors: block.door_refs.clone(),
        })
        .collect()
}

/// 已具备块行（装配输入格式，learnhub target_lines 同构）：
/// `- 名字(slug)|主干|阶段|一句话|门口:…`。
pub fn target_lines(rows: &[TargetBlock]) -> String {
    rows.iter()
        .map(|block| {
            format!(
                "- {}({})|{}|{}|{}|门口:{}",
                block.name,
                block.slug,
                block.spine,
                block.stage,
                block.note,
                block.doors.join("、")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 概念清单的机械归一（结构门形状面，命名门之前）：trim、剔除空名/空定义、
/// 批内按归一化名字与归一化定义双键去重（同物两名/同义重铸批内先折叠），
/// 超上限截断到 [`MAX_DECLARED_CONCEPTS`]（learnhub 按门口分组；allo 锚行
/// 一次性，如实截断并出警告）。返回 (归一清单, 警告)。
pub fn normalize_nominations(
    nominations: Vec<DeclarationNomination>,
) -> (Vec<DeclarationNomination>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();
    let mut seen_definitions: Vec<String> = Vec::new();
    let mut kept: Vec<DeclarationNomination> = Vec::new();
    for mut nomination in nominations {
        nomination.name = nomination.name.trim().to_owned();
        nomination.definition = nomination.definition.trim().to_owned();
        nomination.source = nomination.source.trim().to_owned();
        if nomination.name.is_empty() || nomination.definition.is_empty() {
            continue;
        }
        let name_key = nomination.name.to_lowercase();
        if seen_names.contains(&name_key) {
            continue;
        }
        let definition_key = super::defnear::normalize(&nomination.definition);
        if !definition_key.is_empty() && seen_definitions.contains(&definition_key) {
            continue;
        }
        seen_names.push(name_key);
        if !definition_key.is_empty() {
            seen_definitions.push(definition_key);
        }
        kept.push(nomination);
    }
    if kept.len() > MAX_DECLARED_CONCEPTS {
        warnings.push(format!(
            "声明概念清单超过上限 {MAX_DECLARED_CONCEPTS} 条，已按序截断（超出部分未入锚）"
        ));
        kept.truncate(MAX_DECLARED_CONCEPTS);
    }
    (kept, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(name: &str, slug: &str, door_refs: &[&str]) -> BlockView {
        BlockView {
            name: name.to_owned(),
            slug: slug.to_owned(),
            spine: "主干一".to_owned(),
            stage: "入门".to_owned(),
            note: format!("{name} 的一句话"),
            layer: 0,
            no_prereq: true,
            door_refs: door_refs.iter().map(|door| door.to_string()).collect(),
        }
    }

    #[test]
    fn switch_values_match_the_common_off_semantics() {
        for value in ["off", "OFF", "0", "false", " off "] {
            assert!(
                is_basis_declaration_disabled_value(value),
                "{value:?} 应判关闭"
            );
        }
        for value in ["on", "1", "true", ""] {
            assert!(!is_basis_declaration_disabled_value(value), "{value:?} 不关闭");
        }
    }

    /// AND 口径：door_refs 全落勾选集才算命中；部分命中与零标注不入。
    #[test]
    fn target_rows_use_and_semantics() {
        let blocks = vec![
            block("块A", "block-a", &["G1"]),
            block("块B", "block-b", &["G1", "G2"]),
            block("块C", "block-c", &["G2"]),
            block("块D", "block-d", &[]),
        ];
        let checked: BTreeSet<String> = ["G1".to_owned()].into();
        let rows = target_rows(&blocks, &checked);
        assert_eq!(
            rows.iter().map(|row| row.slug.as_str()).collect::<Vec<_>>(),
            vec!["block-a"],
            "部分 door_refs 命中不算（块B 缺 G2）、零标注不算（块D）"
        );
        let both: BTreeSet<String> = ["G1".to_owned(), "G2".to_owned()].into();
        let rows = target_rows(&blocks, &both);
        assert_eq!(
            rows.iter().map(|row| row.slug.as_str()).collect::<Vec<_>>(),
            vec!["block-a", "block-b", "block-c"],
            "全勾后三块全中，块序随块表序"
        );
    }

    #[test]
    fn target_lines_render_doors_annotation() {
        let rows = vec![TargetBlock {
            name: "所有权".to_owned(),
            slug: "ownership".to_owned(),
            spine: "语言核心".to_owned(),
            stage: "入门".to_owned(),
            note: "值唯一属主".to_owned(),
            doors: vec!["G1".to_owned()],
        }];
        let lines = target_lines(&rows);
        assert!(lines.contains("- 所有权(ownership)|语言核心|入门|值唯一属主|门口:G1"), "{lines}");
    }

    fn nomination(name: &str, definition: &str) -> DeclarationNomination {
        DeclarationNomination {
            name: name.to_owned(),
            slug: "slug".to_owned(),
            definition: definition.to_owned(),
            source: "声明自述".to_owned(),
        }
    }

    /// 归一：空名剔除、批内同名/同义折叠、上限截断出警告。
    #[test]
    fn normalize_nominations_dedupes_and_caps() {
        let (kept, warnings) = normalize_nominations(vec![
            nomination("加法", "求和的运算"),
            nomination(" 加法 ", "另一个定义"),
            nomination("求和", "求和的运算！"),
            nomination("", "无名"),
            nomination("除法", ""),
        ]);
        assert_eq!(kept.len(), 1, "同名折叠、定义归一化同义折叠、空名空定义剔除");
        assert_eq!(kept[0].name, "加法");
        assert!(warnings.is_empty());

        let flood: Vec<DeclarationNomination> = (0..MAX_DECLARED_CONCEPTS + 5)
            .map(|index| nomination(&format!("概念{index}"), &format!("第 {index} 条定义")))
            .collect();
        let (kept, warnings) = normalize_nominations(flood);
        assert_eq!(kept.len(), MAX_DECLARED_CONCEPTS);
        assert_eq!(warnings.len(), 1, "截断必须出警告");
    }

    /// 提示词资产：占位符双向互锁 + 围栏 + 注册表自洽（structure_prompts 同款三闸）。
    mod prompt_asset {
        use super::*;
        use crate::learning_graph::structure_prompts::{
            assemble, extract_placeholder_tokens, header_field, split_template,
        };

        #[test]
        fn placeholders_interlock_both_ways() {
            let spec = &BASIS_DECLARATION;
            let tokens = extract_placeholder_tokens(spec.template);
            for token in &tokens {
                assert!(
                    spec.placeholders.contains(&token.as_str()),
                    "模板出现未声明占位符 {{{token}}}"
                );
            }
            for name in spec.placeholders {
                assert!(
                    tokens.iter().any(|token| token == name),
                    "注册表占位符 {name} 未出现在模板中"
                );
            }
        }

        #[test]
        fn fence_body_dev_face_absent() {
            let (_, body) = split_template(BASIS_DECLARATION.template).expect("头注可剥离");
            for word in BASIS_DECLARATION.fence_words {
                assert!(
                    !body.contains(word),
                    "正文出现开发面词汇 {word}"
                );
            }
        }

        #[test]
        fn header_is_wellformed_and_id_interlocked() {
            let (header, _) = split_template(BASIS_DECLARATION.template).expect("头注可剥离");
            for field in ["id", "source", "scope", "notes", "review"] {
                assert!(header_field(header, field).is_ok(), "头注缺 {field}");
            }
        }

        #[test]
        fn minimum_teachable_unit_clause_present() {
            let (_, body) = split_template(BASIS_DECLARATION.template).expect("头注可剥离");
            assert!(
                body.contains("最小可教单元") && body.contains("禁止把块名整体铸成一条概念"),
                "最小可教单元条款必须在场（learnhub #164 粒度教训）"
            );
            assert!(body.contains("1–60"), "条数契约 1–60 必须在场");
        }

        #[test]
        fn assembly_produces_the_contract() {
            let prompt = assemble(
                &BASIS_DECLARATION,
                &[
                    ("goal", "目标"),
                    ("proficiency_statement", "程度"),
                    ("target_blocks", "- 块(slug)|门口:G1"),
                    ("terrain", "门口定义"),
                ],
            )
            .expect("装配应成功");
            assert_eq!(prompt.task, "basis-declaration");
            assert!(prompt.text.contains("- 块(slug)|门口:G1"));
            assert!(!prompt.text.contains("{{goal}}"), "占位符应全部替换");
        }
    }
}
