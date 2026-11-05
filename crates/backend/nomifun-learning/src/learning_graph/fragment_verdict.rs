//! 评审碎片裁决面（m9 补接；learnhub #173 `FragmentVerdict`/`validate_naming`
//! 同构移植）：机械铺砖（[`definition_audit::grounding_runs`]，m9 审计面同源）
//! 报出的未着落片段注入评审，评审逐片段给 `噪声 | 名词` 裁决，本模块机械核
//! **每个片段都有显式处置**。
//!
//! - **独立清单，不并表**：碎片 = 定义里未被已知词覆盖的命名残片（缺口本身），
//!   provenance（m3）= 定义名词的出处四值（出处面）——两物。清单走输出顶层
//!   独立 `fragments` 键、独立校验，与 `term_provenance` 并行不合并。
//! - **裁决词表**：`噪声` = 切词碎片或元语言组合（放行，note 写依据）；
//!   `名词` = 像样的领域名词却无出处——必须 pass=false 且 problems 指名该
//!   片段，判名词却放行 = 自相矛盾（结论不可用，恰一次带死因重试）。
//! - **未处置片段 = miss**，交调用方按开关分档：`NOMI_LEARNING_LEXICON_ENFORCE`
//!   on = 合成打回问题（L1 级门失败，走既有重裁回路）；off（v1 缺省）=
//!   report-only 降 findings（j09 通道 → k02 水位消费），不打回。
//! - **点名的已知面** = 已教账 ∪ 登记表活跃名+别名 ∪ 本批铸名/提名 ∪
//!   [`lexicon::BASE_YAML`] ∪ 课程增词；课程豁免（ignore）的片段视而不见
//!   （[`lexicon::apply_ignores`]）。「宁滥勿缺」：片段必含切词噪声，处置权
//!   在评审（模型辨误报），机械层只核全覆盖。

use serde::{Deserialize, Serialize};

use super::lexicon::{self, CourseLexicon};
use super::{ProposedBatch, ReviewFinding, ReviewProblem};

/// 碎片裁决词表（与评审提示词逐字一致；learnhub `FragmentVerdict.verdict`
/// 三值同源，absorb-8 z02 增「范围外」——intent/scope 里的否定性边界提及，
/// 诚实写边界不得惩罚性打回）。消费面 = 评审 Verdict 解析 + 互钉测试。
#[allow(dead_code)]
pub(crate) const FRAGMENT_VERDICTS: &[&str] = &["噪声", "名词", "范围外"];

/// 点名清单来源四值词表（learnhub `FRAGMENT_SOURCES` 同源，#205）。消费面 =
/// 互钉测试（词表-枚举-提示词三面同源），运行时无直读。
#[allow(dead_code)]
pub(crate) const FRAGMENT_SOURCES: &[&str] = &["definition", "node_title", "intent", "scope"];

/// 点名片段来源（absorb-8 z02 四源扩面，learnhub #205 `FragmentSource`
/// 同构）：机械认定片段出自批内哪一串文本。评审行报 `source` 据此填，
/// 「范围外」的合法性据此核——边界声明只住 intent/scope 两处
/// （[`FragmentSource::is_boundary`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum FragmentSource {
    /// 提名（mints ∪ new_aliases）的定义。
    #[serde(rename = "definition")]
    Definition,
    /// 节点显示名。
    #[serde(rename = "node_title")]
    NodeTitle,
    /// 节点意图（intent 字段）。
    #[serde(rename = "intent")]
    Intent,
    /// 节点教学范围（scope 字段）。
    #[serde(rename = "scope")]
    Scope,
}

impl FragmentSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FragmentSource::Definition => "definition",
            FragmentSource::NodeTitle => "node_title",
            FragmentSource::Intent => "intent",
            FragmentSource::Scope => "scope",
        }
    }

    /// 「范围外」裁决的合法来源（learnhub 同款）：边界声明只住意图与范围——
    /// 显示名/定义里的正向用词不构成边界声明。
    pub(crate) fn is_boundary(self) -> bool {
        matches!(self, FragmentSource::Intent | FragmentSource::Scope)
    }
}

/// 机械点名清单的一行（learnhub `NameFragment` 同构）：未被已知词汇覆盖的
/// 片段 + 出自批内哪串文本。同词跨源保留多行——「范围外」机械核按词判
/// 任一边界来源在场（跨源多行合法；learnhub 20f1dcf 病历修正后的最终
/// 语义，「行报与机械首行一致」单行硬比不吸）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameFragment {
    /// 片段原文（与批文本逐字对照；子串互含亦算处置）。
    pub(crate) term: String,
    /// 机械认定的来源（片段派生自哪串文本）。
    pub(crate) source: FragmentSource,
}

/// 一条点名片段的裁决（learnhub `FragmentVerdict` 同形）：term 与点名清单
/// 逐字对照（子串互含亦算处置），verdict 三值，note 一句话依据（进观测面
/// 审计档案）。缺席 = 空数组（serde default 容旧包；必填性 = 提示词 +
/// 重试执法，不走 serde 必填）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct FragmentVerdict {
    /// 片段原文（与点名清单逐字对照）。
    pub(crate) term: String,
    /// 噪声 = 放行；名词 = 必须打回并指名；范围外 = 过机械核放行。
    pub(crate) verdict: FragmentRuling,
    /// 片段来源四值（definition/node_title/intent/scope）。缺席 = None
    /// （serde default 容旧行）；范围外行 None = 机械核拒——解析面容缺席、
    /// 必填性由提示词 + 重试执法（learnhub v1.16 先例同构）。
    #[serde(default)]
    pub(crate) source: Option<FragmentSource>,
    /// 一句话依据。
    #[serde(default)]
    pub(crate) note: String,
}

/// 裁决三值。词表与评审提示词、[`FRAGMENT_VERDICTS`] 互钉，改词表须同步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum FragmentRuling {
    /// 切词碎片或元语言组合（停用词/虚字残留、跨词边界噪声）——放行。
    #[serde(rename = "噪声")]
    Noise,
    /// 像样的领域名词却无出处——必须 pass=false 且 problems 指名该片段。
    #[serde(rename = "名词")]
    Noun,
    /// intent/scope 里的否定性边界提及（「不教 X」「X 留待后续」）——诚实
    /// 写边界不打回，过 [`check_out_of_scope_row`] 三件机械核放行。
    #[serde(rename = "范围外")]
    OutOfScope,
}

/// 碎片裁决机械读数（smoke 输出 `fragments: total/rows/unhandled/degraded`
/// 行的数据源；读数面非门禁）。跨评审调用累加：[`FragmentReadout::merge`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct FragmentReadout {
    /// 机械派生待裁决词数 = 点名清单合并渲染后行数（p01 清单瘦身：同词跨源
    /// 一行；核验面仍核跨源全行，此处只改读数口径——smoke `total` 随之）。
    pub(crate) fragments: usize,
    /// 被接受的结论携带的裁决行数。
    pub(crate) rows: usize,
    /// 未处置片段数（report-only 降 findings / enforce 合成打回）。
    pub(crate) unhandled: usize,
    /// 「名词却放行」类自相矛盾两连败 → Degraded 保守放行。
    pub(crate) degraded: bool,
}

impl FragmentReadout {
    pub(crate) fn merge(&mut self, other: &Self) {
        self.fragments += other.fragments;
        self.rows += other.rows;
        self.unhandled += other.unhandled;
        self.degraded |= other.degraded;
    }
}

/// 片段与裁决行/问题的匹配：子串互含（片段「线段」可被裁决行「线段首尾」
/// 覆盖，反之亦然——评审允许对片段做最小辨认；learnhub `relates` 同款）。
fn relates(a: &str, b: &str) -> bool {
    a.contains(b) || b.contains(a)
}

/// 「范围外」行的机械核（absorb-8 z02；learnhub #205 `20f1dcf` 最终语义逐条
/// 移植——2026-11-04 真机病历修正后的形态）：合法性 =
/// ①该词在机械清单里存在 intent/scope 来源的在场行（按词判断、跨源多行
/// 合法；词仅出现在显示名/定义的正向用词时「范围外」无从谈起——谎报 scope
/// 来源借逐字命中逃逸的堵法）；
/// ②行报 `source` 必须在场且为边界来源（intent/scope 二选一）；
/// ③term 逐字命中本批某节点对应字段（intent→intent、scope→scope）原文
/// （边界声明必须真实在册）。
/// ~~行报来源与机械首行一致的单行硬比~~不吸：同词双源（intent「为后续方程
/// 建模」+ scope「不引入方程」）裁决引任一边界来源合法——单行硬比把合法
/// 裁决核伪 → 两连重试 → Degraded 保守放行整场评审被跳过（learnhub 病历）。
/// 违者 = 结论不可用走评审重试（与「名词却放行」同缝，调用方恰一次带死因）。
fn check_out_of_scope_row(
    fv: &FragmentVerdict,
    naming: &[NameFragment],
    batch: &ProposedBatch,
) -> Result<(), String> {
    if !naming
        .iter()
        .any(|f| f.source.is_boundary() && relates(&fv.term, &f.term))
    {
        return Err(format!(
            "片段「{}」判「范围外」被拒——该词没有 intent/scope 来源的机械在场行（仅出现在显示名/定义里的正向用词不构成边界声明）",
            fv.term
        ));
    }
    let Some(row_source) = fv.source else {
        return Err(format!(
            "片段「{}」判「范围外」但缺 source 来源标记（intent/scope 二选一）",
            fv.term
        ));
    };
    if !row_source.is_boundary() {
        return Err(format!(
            "片段「{}」判「范围外」的行报来源「{}」非 intent/scope（边界声明只住这两处）",
            fv.term,
            row_source.as_str()
        ));
    }
    let hit = batch.nodes.iter().any(|node| {
        let text = match row_source {
            FragmentSource::Intent => &node.intent,
            FragmentSource::Scope => &node.scope,
            // 已被 is_boundary 拦截；防御式短路（无对应字段可核）。
            FragmentSource::Definition | FragmentSource::NodeTitle => return false,
        };
        super::provenance::term_grounded(&fv.term, text)
    });
    if !hit {
        return Err(format!(
            "范围外行「{}」的 term 未逐字命中本批任何节点的 {} 文本（边界声明必须真实在册）",
            fv.term,
            row_source.as_str()
        ));
    }
    Ok(())
}

/// 点名处置机械核对（纯函数）：**每个片段必须有显式决定**——
/// - `噪声` 裁决行（note 依据）→ 放行；
/// - `名词` 裁决行 → 必须 pass=false 且 problems 指名该片段（子串命中），
///   否则 = 判名词却放行，自相矛盾 → 结论不可用（`Err`，恰一次带死因重试；
///   enforce 与 report-only 两档都不容）；
/// - `范围外` 裁决行 → [`check_out_of_scope_row`] 三件机械核，违者 =
///   结论不可用（同路重试）；
/// - 无裁决行但 problems 指名 → 打回处置，放行；
/// - 全无 = 未处置，返回给调用方按 enforcement 分档（enforce = 合成打回
///   问题；report-only = 降 findings）。词表外裁决在解析面已拒
///   （serde 枚举词表），不存在第四态。
pub(crate) fn validate_fragments(
    verdicts: &[FragmentVerdict],
    pass: bool,
    problems: &[ReviewProblem],
    naming: &[NameFragment],
    batch: &ProposedBatch,
) -> Result<Vec<NameFragment>, String> {
    let mut unhandled: Vec<NameFragment> = Vec::new();
    for frag in naming {
        let verdict = verdicts.iter().find(|fv| relates(&fv.term, &frag.term));
        let problem_named = problems.iter().any(|p| p.issue.contains(&frag.term));
        match verdict {
            Some(fv) if fv.verdict == FragmentRuling::OutOfScope => {
                check_out_of_scope_row(fv, naming, batch)?;
            }
            Some(fv) if fv.verdict == FragmentRuling::Noun => {
                if pass || !problems.iter().any(|p| relates(&p.issue, &frag.term)) {
                    return Err(format!(
                        "片段「{}」判「名词」却放行或未在 problems 指名（自相矛盾）",
                        frag.term
                    ));
                }
            }
            // 噪声 = 放行。
            Some(_) => {}
            None => {
                if pass || !problem_named {
                    if pass && problem_named {
                        // problems 指名但 pass=true：同属自相矛盾，恒打回。
                        return Err(format!(
                            "片段「{}」在 problems 中指名却 pass=true（自相矛盾）",
                            frag.term
                        ));
                    }
                    unhandled.push(frag.clone());
                }
            }
        }
    }
    Ok(unhandled)
}

/// 未处置片段的 report-only findings 行（v1 缺省档；j09 通道 → k02 水位
/// 消费，不打回）。行文带来源标记（观测面审计可定位片段出自哪串文本）。
pub(crate) fn unhandled_findings(unhandled: &[NameFragment]) -> Vec<ReviewFinding> {
    unhandled
        .iter()
        .map(|frag| ReviewFinding {
            node: String::new(),
            field: "fragments".into(),
            issue: format!(
                "机械点名片段「{}」（来源 {}）未处置（report-only，留痕不打回）",
                frag.term,
                frag.source.as_str()
            ),
            advice: "挂号自教/改写绕开/课程增词提案，三选一".into(),
        })
        .collect()
}

/// 未处置片段的 enforce 打回问题（L1 级门失败；批级意见不点名节点——
/// advice 范围条款同口径：修法只动随批提名自身）。
pub(crate) fn unhandled_problems(unhandled: &[NameFragment]) -> Vec<ReviewProblem> {
    unhandled
        .iter()
        .map(|frag| ReviewProblem {
            node: String::new(),
            field: "mints".into(),
            issue: format!(
                "机械点名片段「{}」（来源 {}）未有着落（判噪声须给依据，判名词须打回）",
                frag.term,
                frag.source.as_str()
            ),
            advice: "挂号自教/改写绕开/课程增词提案，三选一".into(),
        })
        .collect()
}

/// 点名清单的模型面渲染：空 = 明示「全部有着落」，非空 = 逐行编号并标注
/// 来源（absorb-8 z02，learnhub `naming_text` 扩面同款）；p01 清单瘦身：
/// 同词跨源合并为一行多标签（[`merged_naming_rows`]，保序按首现行）——
/// 评审输出每词一行、term 相同的一行裁决覆盖它的全部来源行（prompt 配套
/// 条款同语义）。核验面（[`validate_fragments`]）保留跨源全行不动：合并
/// 只发生在渲染与读数口径，机械核零改动。
pub(crate) fn naming_list_block(naming: &[NameFragment]) -> String {
    if naming.is_empty() {
        return "\n\n【机械点名清单（缺口碎片；逐条裁决，输出硬约束）】\n(无:本批全部有着落)".into();
    }
    let list = merged_naming_rows(naming)
        .iter()
        .enumerate()
        .map(|(i, (term, sources))| {
            let tags = sources
                .iter()
                .map(|source| format!("[{}]", source.as_str()))
                .collect::<String>();
            format!("{}. {} {tags}", i + 1, term)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n\n【机械点名清单（缺口碎片；逐条裁决，输出硬约束）】\n{list}")
}

/// 合并渲染的一行：同 term 的跨源行折叠为「词 + 来源标签列」（标签按首现
/// 行序；同源重复不存在——派生面已同源同词去重）。纯展示形状，核验面
/// 不消费。
fn merged_naming_rows(naming: &[NameFragment]) -> Vec<(&str, Vec<FragmentSource>)> {
    let mut merged: Vec<(&str, Vec<FragmentSource>)> = Vec::new();
    for frag in naming {
        match merged.iter_mut().find(|(term, _)| *term == frag.term) {
            Some((_, sources)) => {
                if !sources.contains(&frag.source) {
                    sources.push(frag.source);
                }
            }
            None => merged.push((frag.term.as_str(), vec![frag.source])),
        }
    }
    merged
}

/// 读数口径（p01）：合并渲染后行数 = 机械派生待裁决词数（真机病历：127
/// 行清单对 92 unique terms——合并行数才是模型的实际裁决负担）。
pub(crate) fn merged_row_count(naming: &[NameFragment]) -> usize {
    merged_naming_rows(naming).len()
}

/// 点名清单派生（纯函数；absorb-8 z02 四源扩面，learnhub #205
/// `collect_naming_fragments` 同构）：输入面 = 本批提名定义（mints ∪
/// new_aliases）∪ 各节点显示名 ∪ 节点 intent ∪ 节点 scope，逐条
/// [`definition_audit::grounding_runs`]（known 掩码 + 宁滥勿缺语义不变）。
/// 同源同词去重保序；**跨源同词保留多行**（范围外核的跨源数据基础）。
/// p01 清单瘦身：**非 definition 源的单字碎片不进清单**——声明面
/// （node_title/intent/scope）的单字几乎全是切词残渣（真机 run-1793828585963
/// 127 行清单 38 行单字）；definition 源保持宁滥勿缺不动（单字照收）。
/// 课程豁免 [`lexicon::apply_ignores`] 照旧对全源生效（按词过滤——同词多行
/// 同滤；位置不动）。`known` = 已知面（已教账 ∪ 登记表 ∪ 本批 ∪ base ∪
/// 课程增词，调用方拼装；小写折叠与 CJK 片段互不干扰）。
pub(crate) fn derive_naming_list(
    batch: &ProposedBatch,
    known: &[String],
    course: &CourseLexicon,
) -> Vec<NameFragment> {
    let mut naming: Vec<NameFragment> = Vec::new();
    let mut collect = |text: &str, source: FragmentSource, naming: &mut Vec<NameFragment>| {
        for frag in super::definition_audit::grounding_runs(text, known) {
            if source != FragmentSource::Definition && frag.chars().count() == 1 {
                continue;
            }
            if !naming.iter().any(|f| f.source == source && f.term == frag) {
                naming.push(NameFragment { term: frag, source });
            }
        }
    };
    let defs = batch
        .mints
        .iter()
        .map(|mint| mint.definition.as_str())
        .chain(batch.new_aliases.iter().map(|a| a.definition.as_str()));
    for definition in defs {
        collect(definition, FragmentSource::Definition, &mut naming);
    }
    for node in &batch.nodes {
        collect(&node.title, FragmentSource::NodeTitle, &mut naming);
        collect(&node.intent, FragmentSource::Intent, &mut naming);
        collect(&node.scope, FragmentSource::Scope, &mut naming);
    }
    // 豁免按词过滤（apply_ignores 单源），同词多行一致同滤；保序重建。
    let kept: Vec<String> =
        lexicon::apply_ignores(&naming.iter().map(|f| f.term.clone()).collect::<Vec<_>>(), course);
    naming.into_iter().filter(|f| kept.contains(&f.term)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_graph::{ConceptMint, ConceptNewAlias, ProposedBatch, ProposedNode};

    fn verdict(term: &str, word: &str, note: &str) -> FragmentVerdict {
        FragmentVerdict {
            term: term.into(),
            verdict: serde_json::from_value(serde_json::json!(word)).expect("词表内裁决"),
            source: None,
            note: note.into(),
        }
    }

    fn verdict_with_source(term: &str, word: &str, source: &str, note: &str) -> FragmentVerdict {
        FragmentVerdict {
            source: serde_json::from_value(serde_json::json!(source)).expect("词表内来源"),
            ..verdict(term, word, note)
        }
    }

    /// 点名清单一行（absorb-8 z02 四源扩面后清单携带来源标记）。
    fn frag(term: &str, source: FragmentSource) -> NameFragment {
        NameFragment { term: term.into(), source }
    }

    /// 带声明面两字段的节点（范围外核的对照域）。
    fn node_with_brief(title: &str, intent: &str, scope: &str) -> ProposedNode {
        ProposedNode {
            title: title.to_owned(),
            purpose: String::new(),
            intent: intent.to_owned(),
            scope: scope.to_owned(),
            est: Some(20),
            difficulty: None,
            teaches: Vec::new(),
            assumes: Vec::new(),
        }
    }

    fn problem(issue: &str) -> ReviewProblem {
        ReviewProblem {
            node: String::new(),
            field: "mints".into(),
            issue: issue.into(),
            advice: String::new(),
        }
    }

    fn batch_with(defs: &[&str]) -> ProposedBatch {
        ProposedBatch {
            mints: defs
                .iter()
                .map(|definition| ConceptMint {
                    canonical: format!("名{definition}"),
                    aliases: Vec::new(),
                    definition: definition.to_string(),
                    source: None,
                })
                .collect(),
            new_aliases: Vec::new(),
            ..ProposedBatch::default()
        }
    }

    // ── serde 形状（容旧 + 词表） ────────────────────────────────────────

    #[test]
    fn serde_parses_three_word_vocabulary_and_tolerates_missing_note_and_source() {
        let parsed: FragmentVerdict =
            serde_json::from_value(serde_json::json!({ "term": "线段", "verdict": "噪声" }))
                .expect("缺 note/source 合法（serde default 容旧行）");
        assert_eq!(parsed.term, "线段");
        assert_eq!(parsed.verdict, FragmentRuling::Noise);
        assert_eq!(parsed.note, "");
        assert_eq!(parsed.source, None, "缺 source = None（容旧，范围外行另核）");
        let noun: FragmentVerdict = serde_json::from_value(serde_json::json!(
            { "term": "内角", "verdict": "名词", "note": "构件名词无出处" }
        ))
        .expect("全形状可解析");
        assert_eq!(noun.verdict, FragmentRuling::Noun);
        let oos: FragmentVerdict = serde_json::from_value(serde_json::json!(
            { "term": "方程", "verdict": "范围外", "source": "scope", "note": "scope 原文「不引入方程」" }
        ))
        .expect("范围外行带 source 可解析");
        assert_eq!(oos.verdict, FragmentRuling::OutOfScope);
        assert_eq!(oos.source, Some(FragmentSource::Scope));
        let bad_verdict = serde_json::json!({ "term": "t", "verdict": "大概会" });
        assert!(
            serde_json::from_value::<FragmentVerdict>(bad_verdict).is_err(),
            "词表外 verdict = 形状缺陷，不可解析"
        );
        let bad_source =
            serde_json::json!({ "term": "t", "verdict": "范围外", "source": "teaches" });
        assert!(
            serde_json::from_value::<FragmentVerdict>(bad_source).is_err(),
            "来源词表外 source = 形状缺陷，不可解析"
        );
    }

    // ── 全覆盖核对 ───────────────────────────────────────────────────────

    #[test]
    fn every_fragment_disposed_as_noise_passes() {
        let naming = vec![frag("线段", FragmentSource::Definition), frag("内角", FragmentSource::Definition)];
        let verdicts = vec![
            verdict("线段", "噪声", "切词碎片"),
            verdict("内角", "噪声", "元语言组合"),
        ];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming, &ProposedBatch::default()), Ok(vec![]));
    }

    #[test]
    fn noun_verdict_with_reject_and_problem_mention_passes() {
        let naming = vec![frag("内角", FragmentSource::Definition)];
        let verdicts = vec![verdict("内角", "名词", "构件名词无出处")];
        let problems = vec![problem("缺口名词「内角」无人教，打回")];
        assert_eq!(
            validate_fragments(&verdicts, false, &problems, &naming, &ProposedBatch::default()),
            Ok(vec![])
        );
    }

    #[test]
    fn noun_verdict_with_pass_is_self_contradictory() {
        let naming = vec![frag("内角", FragmentSource::Definition)];
        let verdicts = vec![verdict("内角", "名词", "构件名词无出处")];
        let error = validate_fragments(&verdicts, true, &[], &naming, &ProposedBatch::default()).unwrap_err();
        assert!(error.contains("判「名词」却放行"), "{error}");
    }

    #[test]
    fn noun_verdict_without_problem_mention_is_self_contradictory() {
        // pass=false 但 problems 没指名该片段：名词裁决落空，同样矛盾。
        let naming = vec![frag("内角", FragmentSource::Definition)];
        let verdicts = vec![verdict("内角", "名词", "构件名词无出处")];
        let problems = vec![problem("别的节点难度不实")];
        let error = validate_fragments(&verdicts, false, &problems, &naming, &ProposedBatch::default()).unwrap_err();
        assert!(error.contains("未在 problems 指名"), "{error}");
    }

    #[test]
    fn problem_mention_without_verdict_row_is_a_disposition_when_rejected() {
        // 无裁决行但 problems 指名（pass=false）= 打回处置；指名按片段逐个
        // 对照——被指名的「线段」算处置，未被提及的「内角」仍算未处置。
        let naming = vec![frag("线段", FragmentSource::Definition), frag("内角", FragmentSource::Definition)];
        let problems = vec![problem("缺口名词「线段」无人教，打回")];
        assert_eq!(
            validate_fragments(&[], false, &problems, &naming, &ProposedBatch::default()),
            Ok(vec![frag("内角", FragmentSource::Definition)])
        );
    }

    #[test]
    fn problem_mention_with_pass_is_self_contradictory() {
        let naming = vec![frag("线段", FragmentSource::Definition)];
        let problems = vec![problem("缺口名词「线段」无人教，打回")];
        let error = validate_fragments(&[], true, &problems, &naming, &ProposedBatch::default()).unwrap_err();
        assert!(error.contains("指名却 pass=true"), "{error}");
    }

    #[test]
    fn unhandled_fragments_are_returned_for_the_caller_to_divide() {
        let naming = vec![
            frag("线段", FragmentSource::Definition),
            frag("内角", FragmentSource::Definition),
            frag("顶点", FragmentSource::NodeTitle),
        ];
        let verdicts = vec![verdict("线段", "噪声", "切词碎片")];
        let unhandled = validate_fragments(&verdicts, true, &[], &naming, &ProposedBatch::default()).unwrap();
        assert_eq!(unhandled, vec![frag("内角", FragmentSource::Definition), frag("顶点", FragmentSource::NodeTitle)]);
    }

    #[test]
    fn substring_containment_counts_as_disposition() {
        // 片段「线段」被裁决行「线段首尾」覆盖（子串互含 = 最小辨认）。
        let naming = vec![frag("线段", FragmentSource::Definition)];
        let verdicts = vec![verdict("线段首尾", "噪声", "顺次相接的切词噪声")];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming, &ProposedBatch::default()), Ok(vec![]));
        // 反向：裁决行「线」被片段「线段」包含，同样算处置。
        let verdicts = vec![verdict("线", "噪声", "单字残片")];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming, &ProposedBatch::default()), Ok(vec![]));
    }

    #[test]
    fn empty_naming_list_always_passes() {
        assert_eq!(validate_fragments(&[], true, &[], &[], &ProposedBatch::default()), Ok(vec![]));
        let naming: Vec<NameFragment> = vec![];
        let verdicts = vec![verdict("多余裁决", "噪声", "清单外行不核")];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming, &ProposedBatch::default()), Ok(vec![]));
    }

    // ── 未处置的两档分流 ─────────────────────────────────────────────────

    #[test]
    fn unhandled_become_report_only_findings_by_default() {
        let rows = unhandled_findings(&[frag("内角", FragmentSource::Definition), frag("顶点", FragmentSource::NodeTitle)]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].field, "fragments");
        assert!(rows[0].issue.contains("内角") && rows[0].issue.contains("未处置"));
        assert!(rows[0].node.is_empty(), "批级意见不点名节点");
    }

    #[test]
    fn unhandled_become_synthesized_problems_under_enforce() {
        let rows = unhandled_problems(&[frag("内角", FragmentSource::Definition)]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].issue.contains("内角") && rows[0].issue.contains("未有着落"));
        assert!(rows[0].node.is_empty(), "批级意见不点名节点（advice 范围条款）");
        assert!(!rows[0].advice.contains("移出本批"), "不触发摘节点机械面");
    }

    // ── 读数 ─────────────────────────────────────────────────────────────

    #[test]
    fn readout_merges_accumulate() {
        let mut total = FragmentReadout::default();
        total.merge(&FragmentReadout { fragments: 3, rows: 3, unhandled: 0, degraded: false });
        total.merge(&FragmentReadout { fragments: 2, rows: 0, unhandled: 2, degraded: true });
        assert_eq!(
            total,
            FragmentReadout { fragments: 5, rows: 3, unhandled: 2, degraded: true }
        );
    }

    // ── 点名清单派生 ─────────────────────────────────────────────────────

    #[test]
    fn derivation_exposes_uncovered_runs_and_applies_ignores() {
        // 已知面给了「三角形/由/三/条/的」等；「线段/内角」无着落 → 点名。
        let batch = batch_with(&["由三条线段围成的图形，内角和是 180 度。"]);
        let known: Vec<String> = ["三角形", "由", "三", "条", "的", "围成", "图形", "内角和", "是", "度"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let course = CourseLexicon::default();
        let naming = derive_naming_list(&batch, &known, &course);
        assert!(
            naming.iter().any(|f| f.term.contains("线段")),
            "线段须在点名清单:{naming:?}"
        );
        assert!(
            !naming.iter().any(|f| f.term.contains("三角形")),
            "已知词覆盖处不点名:{naming:?}"
        );
        // 课程豁免：含豁免词的片段视而不见。
        let course = CourseLexicon { words: vec![], ignore: vec!["线段".into()] };
        assert!(
            !derive_naming_list(&batch, &known, &course).iter().any(|f| f.term.contains("线段")),
            "豁免片段不进清单"
        );
    }

    #[test]
    fn derivation_covers_mints_and_aliases_and_dedups() {
        let mut batch = batch_with(&["线段首尾相接的封闭图形。"]);
        batch.new_aliases.push(ConceptNewAlias {
            name: "折引用".into(),
            definition: "线段的另一叫法，借用别名。".into(),
            source: "s".into(),
        });
        let known: Vec<String> = ["首尾", "相接", "的", "封闭", "图形", "另一", "叫法", "借用"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let naming = derive_naming_list(&batch, &known, &CourseLexicon::default());
        assert!(
            naming.iter().any(|f| f.source == FragmentSource::Definition && f.term.contains("线段")),
            "铸名定义的缺口须点名(definition 源):{naming:?}"
        );
        assert!(
            naming.iter().any(|f| f.source == FragmentSource::Definition && f.term == "别名"),
            "别名提名的定义同样入点名域:{naming:?}"
        );
        assert_eq!(
            naming.iter().filter(|f| f.term.contains("线段")).count(),
            1,
            "同源同名片段去重保序:{naming:?}"
        );
    }

    /// 四源扩面（absorb-8 z02，learnhub #205 `collect_naming_fragments` 同构
    /// 钉）：定义 ∪ 显示名 ∪ intent ∪ scope，片段带来源标记；同源同词去重、
    /// **跨源同词保留多行**（范围外核「按词判任一边界在场行」的数据基础）；
    /// 课程豁免对全源生效。
    #[test]
    fn derivation_collects_four_sources_with_source_tags() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief(
                "把相同因数的连乘写成幂并指出底数与指数",
                "把连乘经验升格为记号，承接乘法",
                "只立记号与读法；不引入方程",
            )],
            mints: vec![ConceptMint {
                canonical: "正整数幂".into(),
                aliases: Vec::new(),
                definition: "一个数自乘若干次所得的积。".into(),
                source: None,
            }],
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let known: Vec<String> = ["乘法", "记号", "读法", "形式", "学习点", "后续", "正整数", "自乘", "若干次", "所得", "一个数", "只立", "与", "承接", "升格", "把", "的", "；", "。", "并指出"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let naming = derive_naming_list(&batch, &known, &CourseLexicon::default());
        let of = |source: FragmentSource| naming.iter().filter(move |f| f.source == source);
        assert!(
            of(FragmentSource::Definition).any(|f| f.term.contains("自乘") || f.term.contains("积")),
            "提名定义源照旧收集:{naming:?}"
        );
        assert!(
            of(FragmentSource::NodeTitle).any(|f| f.term.contains("底数") || f.term.contains("指数")),
            "标题领域名词进 node_title 源(此前 allo 一直缺):{naming:?}"
        );
        assert!(
            of(FragmentSource::Intent).any(|f| f.term.contains("连乘")),
            "intent 名词进 intent 源:{naming:?}"
        );
        assert!(
            of(FragmentSource::Scope).any(|f| f.term.contains("方程")),
            "scope 排除段名词进 scope 源:{naming:?}"
        );
        assert!(
            naming.iter().all(|f| !f.term.contains("乘法")),
            "已知词豁免生效(乘法不露面):{naming:?}"
        );
    }

    /// 跨源同词保留多行：「方程」同现于 intent 与 scope 时清单两行都在——
    /// 范围外核按词判任一边界来源在场（跨源多行合法）的数据基础。
    #[test]
    fn same_term_across_sources_keeps_multiple_rows() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief("列出数量关系式", "为后续方程建模备料", "不做展开与化简，不引入方程")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let known: Vec<String> = ["列出", "数量关系", "式", "为", "后续", "建模", "备料", "不做", "展开", "化简", "不引入"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let naming = derive_naming_list(&batch, &known, &CourseLexicon::default());
        let equation_rows: Vec<&NameFragment> =
            naming.iter().filter(|f| f.term.contains("方程")).collect();
        assert_eq!(
            equation_rows.len(),
            2,
            "同词双源(int+scope)保留两行:{naming:?}"
        );
        assert!(
            equation_rows.iter().any(|f| f.source == FragmentSource::Intent)
                && equation_rows.iter().any(|f| f.source == FragmentSource::Scope),
            "两行各带自己的来源:{equation_rows:?}"
        );
        // 课程豁免对全源生效：豁免「方程」→ 双行一起消失。
        let course = CourseLexicon { words: vec![], ignore: vec!["方程".into()] };
        assert!(
            !derive_naming_list(&batch, &known, &course).iter().any(|f| f.term.contains("方程")),
            "豁免片段跨源同滤"
        );
    }

    #[test]
    fn naming_list_block_renders_numbered_list_with_source_tags_or_empty_note() {
        let empty = naming_list_block(&[]);
        assert!(empty.contains("(无:本批全部有着落)"), "{empty}");
        let block = naming_list_block(&[
            frag("方程", FragmentSource::Scope),
            frag("线段", FragmentSource::NodeTitle),
        ]);
        assert!(block.contains("【机械点名清单"), "{block}");
        assert!(block.contains("1. 方程 [scope]"), "{block}");
        assert!(block.contains("2. 线段 [node_title]"), "{block}");
    }

    // ── p01 清单瘦身（单字过滤/合并渲染/一行覆盖多源） ────────────────────

    /// 单字过滤三源 × 三态（p01 清单瘦身；真机 run-1793828585963：127 行
    /// 清单 38 行单字占 30%，全部出自声明面切片）：非 definition 源
    /// （node_title/intent/scope）的单字碎片不进清单——声明面单字几乎全是
    /// 切词残渣；definition 源保持宁滥勿缺不动（单字照收）。课程豁免
    /// （apply_ignores）位置不动，另有既有测试钉。
    #[test]
    fn single_char_fragments_filtered_from_declaration_sources_only() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief("途", "萃", "教")],
            mints: vec![ConceptMint {
                canonical: "研磨粗细".into(),
                aliases: Vec::new(),
                definition: "粗".into(),
                source: None,
            }],
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let naming = derive_naming_list(&batch, &[], &CourseLexicon::default());
        assert!(
            naming.iter().any(|f| f.source == FragmentSource::Definition && f.term == "粗"),
            "definition 源单字照收（宁滥勿缺不动）：{naming:?}"
        );
        for (term, source) in [
            ("途", FragmentSource::NodeTitle),
            ("萃", FragmentSource::Intent),
            ("教", FragmentSource::Scope),
        ] {
            assert!(
                !naming.iter().any(|f| f.source == source && f.term == term),
                "声明面源单字不进清单（{}）：{naming:?}",
                source.as_str()
            );
        }
    }

    /// 合并渲染一行多标签（p01 清单瘦身；真机病历：滤纸×3/温度计×3/途×3
    /// ——跨源重复 35 行）：同 term 跨源合并为一行、来源标签按首现行序并
    /// 列；清单行数 = unique 词数（读数 fragments 口径同步改合并后行数）。
    #[test]
    fn naming_list_block_merges_cross_source_rows_into_one_tagged_line() {
        let naming = vec![
            frag("滤纸", FragmentSource::Definition),
            frag("滤杯", FragmentSource::Definition),
            frag("滤纸", FragmentSource::Intent),
            frag("滤纸", FragmentSource::Scope),
        ];
        let block = naming_list_block(&naming);
        assert!(block.contains("1. 滤纸 [definition][intent][scope]"), "{block}");
        assert!(block.contains("2. 滤杯 [definition]"), "{block}");
        assert_eq!(block.matches("滤纸").count(), 1, "同词只渲染一行：{block}");
        assert_eq!(merged_row_count(&naming), 2, "合并后行数 = unique 词数");
    }

    /// 一行覆盖多源（p01 清单瘦身；真机：127 行响应对 92 unique terms 全过
    /// 核——`relates` 多对一匹配本就合法）：核验面保留跨源全行不动，同词
    /// 一条裁决行覆盖它的全部来源行——零 unhandled。
    /// `validate_fragments` / `check_out_of_scope_row` 零改动即得此语义。
    #[test]
    fn one_noise_verdict_row_covers_all_cross_source_rows() {
        let naming = vec![
            frag("滤纸", FragmentSource::Definition),
            frag("滤纸", FragmentSource::Intent),
            frag("滤纸", FragmentSource::Scope),
            frag("温度计", FragmentSource::NodeTitle),
            frag("温度计", FragmentSource::Scope),
        ];
        let verdicts = vec![
            verdict("滤纸", "噪声", "本节点 teaches 手冲器具功能覆盖"),
            verdict("温度计", "噪声", "本节点 teaches 水温测量覆盖"),
        ];
        assert_eq!(
            validate_fragments(&verdicts, true, &[], &naming, &ProposedBatch::default()),
            Ok(vec![]),
            "同词一行裁决覆盖全部来源行"
        );
    }

    // ── 「范围外」三件机械核（learnhub 20f1dcf 最终语义移植） ────────────

    /// 病历形态钉（learnhub `out_of_scope_multi_source_term_accepts_any_
    /// boundary_citation` 移植）：同词双源（intent「为后续方程建模备料」+
    /// scope「不引入方程」），裁决行报 scope 判范围外 = 合法放行——钉死
    /// 「行报与机械首行一致」单行硬比回归；引 intent 同样合法（任一边界
    /// 来源即可）。
    #[test]
    fn out_of_scope_multi_source_term_accepts_any_boundary_citation() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief("列出数量关系式", "为后续方程建模备料", "不做展开与化简，不引入方程")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let naming = vec![
            frag("方程", FragmentSource::Intent),
            frag("方程", FragmentSource::Scope),
        ];
        // 引 scope（机械首行是 intent——单行硬比会核伪的形态）。
        let verdicts = vec![verdict_with_source("方程", "范围外", "scope", "scope 原文「不引入方程」")];
        assert_eq!(
            validate_fragments(&verdicts, true, &[], &naming, &batch),
            Ok(vec![]),
            "跨源引用 scope 合法放行"
        );
        // 引 intent 同样合法。
        let verdicts = vec![verdict_with_source("方程", "范围外", "intent", "intent 原文「为后续方程建模备料」")];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming, &batch), Ok(vec![]));
    }

    /// 词仅在 title/definition、机械清单无边界在场行时谎报 scope 判范围外
    /// = 拒（learnhub `title_fragment_claiming_scope_out_of_scope_is_rejected_
    /// by_machine_check` 移植）：即便 term 逐字命中 scope 原文也拒——正向
    /// 用词不构成边界声明，堵借逐字命中逃逸的洞。
    #[test]
    fn title_fragment_claiming_scope_out_of_scope_is_rejected() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief("指出底数与指数", "把乘方记号接到连乘经验上", "不教底数与指数的推广")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        // 机械来源 = node_title（无 intent/scope 在场行）；行谎报 scope 且
        // term 逐字命中 scope 文本——仍拒。
        let naming = vec![frag("底数与指数", FragmentSource::NodeTitle)];
        let verdicts = vec![verdict_with_source("底数与指数", "范围外", "scope", "范围里有这句")];
        let error = validate_fragments(&verdicts, true, &[], &naming, &batch).unwrap_err();
        assert!(error.contains("没有 intent/scope 来源的机械在场行"), "{error}");
    }

    /// term 未逐字命中对应字段原文 = 拒（有边界在场行、行报边界来源，但
    /// scope 原文里没写过这个词——「没写过的边界不成立」）。
    #[test]
    fn out_of_scope_term_not_verbatim_in_field_text_is_rejected() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief("学代数式", "为后续方程建模备料", "不做展开与化简")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let naming = vec![frag("积分", FragmentSource::Scope)];
        let verdicts = vec![verdict_with_source("积分", "范围外", "scope", "scope 里说不教积分")];
        let error = validate_fragments(&verdicts, true, &[], &naming, &batch).unwrap_err();
        assert!(error.contains("未逐字命中"), "{error}");
    }

    /// 行报非边界来源（definition）判范围外 = 拒（learnhub
    /// `out_of_scope_non_boundary_citation_is_rejected` 移植；机械清单有
    /// 边界在场行也救不了——边界声明只住 intent/scope）。
    #[test]
    fn out_of_scope_non_boundary_citation_is_rejected() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief("列出数量关系式", "为后续方程建模备料", "不引入方程")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let naming = vec![
            frag("方程", FragmentSource::Intent),
            frag("方程", FragmentSource::Scope),
            frag("方程", FragmentSource::Definition),
        ];
        let verdicts = vec![verdict_with_source("方程", "范围外", "definition", "定义里提过方程")];
        let error = validate_fragments(&verdicts, true, &[], &naming, &batch).unwrap_err();
        assert!(error.contains("非 intent/scope"), "{error}");
    }

    /// 范围外行缺 source 来源标记 = 拒（serde default 容旧行 ≠ 核验放行）。
    #[test]
    fn out_of_scope_row_without_source_is_rejected() {
        let batch = ProposedBatch {
            nodes: vec![node_with_brief("列出数量关系式", "为后续方程建模备料", "不引入方程")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let naming = vec![frag("方程", FragmentSource::Scope)];
        let verdicts = vec![verdict("方程", "范围外", "没带来源标记")];
        let error = validate_fragments(&verdicts, true, &[], &naming, &batch).unwrap_err();
        assert!(error.contains("缺 source"), "{error}");
    }
}
