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
/// 两值同源）。消费面 = 评审 Verdict 解析 + 互钉测试。
#[allow(dead_code)]
pub(crate) const FRAGMENT_VERDICTS: &[&str] = &["噪声", "名词"];

/// 一条点名片段的裁决（learnhub `FragmentVerdict` 同形）：term 与点名清单
/// 逐字对照（子串互含亦算处置），verdict 两值，note 一句话依据（进观测面
/// 审计档案）。缺席 = 空数组（serde default 容旧包；必填性 = 提示词 +
/// 重试执法，不走 serde 必填）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct FragmentVerdict {
    /// 片段原文（与点名清单逐字对照）。
    pub(crate) term: String,
    /// 噪声 = 放行；名词 = 必须打回并指名。
    pub(crate) verdict: FragmentRuling,
    /// 一句话依据。
    #[serde(default)]
    pub(crate) note: String,
}

/// 裁决两值。词表与评审提示词、[`FRAGMENT_VERDICTS`] 互钉，改词表须同步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum FragmentRuling {
    /// 切词碎片或元语言组合（停用词/虚字残留、跨词边界噪声）——放行。
    #[serde(rename = "噪声")]
    Noise,
    /// 像样的领域名词却无出处——必须 pass=false 且 problems 指名该片段。
    #[serde(rename = "名词")]
    Noun,
}

/// 碎片裁决机械读数（smoke 输出 `fragments: total/rows/unhandled/degraded`
/// 行的数据源；读数面非门禁）。跨评审调用累加：[`FragmentReadout::merge`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct FragmentReadout {
    /// 点名清单片段数（机械派生，评审输入）。
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

/// 点名处置机械核对（纯函数）：**每个片段必须有显式决定**——
/// - `噪声` 裁决行（note 依据）→ 放行；
/// - `名词` 裁决行 → 必须 pass=false 且 problems 指名该片段（子串命中），
///   否则 = 判名词却放行，自相矛盾 → 结论不可用（`Err`，恰一次带死因重试；
///   enforce 与 report-only 两档都不容）；
/// - 无裁决行但 problems 指名 → 打回处置，放行；
/// - 全无 = 未处置，返回给调用方按 enforcement 分档（enforce = 合成打回
///   问题；report-only = 降 findings）。
pub(crate) fn validate_fragments(
    verdicts: &[FragmentVerdict],
    pass: bool,
    problems: &[ReviewProblem],
    naming: &[String],
) -> Result<Vec<String>, String> {
    let mut unhandled: Vec<String> = Vec::new();
    for frag in naming {
        let verdict = verdicts.iter().find(|fv| relates(&fv.term, frag));
        let problem_named = problems.iter().any(|p| p.issue.contains(frag.as_str()));
        match verdict {
            Some(fv) if fv.verdict == FragmentRuling::Noun => {
                if pass || !problems.iter().any(|p| relates(&p.issue, frag)) {
                    return Err(format!(
                        "片段「{frag}」判「名词」却放行或未在 problems 指名（自相矛盾）"
                    ));
                }
            }
            Some(_) => {}
            None => {
                if pass || !problem_named {
                    if pass && problem_named {
                        // problems 指名但 pass=true：同属自相矛盾，恒打回。
                        return Err(format!(
                            "片段「{frag}」在 problems 中指名却 pass=true（自相矛盾）"
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
/// 消费，不打回）。
pub(crate) fn unhandled_findings(unhandled: &[String]) -> Vec<ReviewFinding> {
    unhandled
        .iter()
        .map(|frag| ReviewFinding {
            node: String::new(),
            field: "fragments".into(),
            issue: format!("机械点名片段「{frag}」未处置（report-only，留痕不打回）"),
            advice: "挂号自教/改写绕开/课程增词提案，三选一".into(),
        })
        .collect()
}

/// 未处置片段的 enforce 打回问题（L1 级门失败；批级意见不点名节点——
/// advice 范围条款同口径：修法只动随批提名自身）。
pub(crate) fn unhandled_problems(unhandled: &[String]) -> Vec<ReviewProblem> {
    unhandled
        .iter()
        .map(|frag| ReviewProblem {
            node: String::new(),
            field: "mints".into(),
            issue: format!("机械点名片段「{frag}」未有着落（判噪声须给依据，判名词须打回指名）"),
            advice: "挂号自教/改写绕开/课程增词提案，三选一".into(),
        })
        .collect()
}

/// 点名清单的模型面渲染：空 = 明示「全部有着落」，非空 = 逐行编号
/// （learnhub `naming_text` 同款）。
pub(crate) fn naming_list_block(naming: &[String]) -> String {
    if naming.is_empty() {
        return "\n\n【机械点名清单（缺口碎片；逐条裁决，输出硬约束）】\n(无:本批定义全部有着落)".into();
    }
    let list = naming
        .iter()
        .enumerate()
        .map(|(i, term)| format!("{}. {term}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n\n【机械点名清单（缺口碎片；逐条裁决，输出硬约束）】\n{list}")
}

/// 点名清单派生（纯函数；learnhub facade 同构）：本批提名定义（mints ∪
/// new_aliases）逐条 [`definition_audit::grounding_runs`]，跨条去重保序，
/// 课程豁免过滤。`known` = 已知面（已教账 ∪ 登记表 ∪ 本批 ∪ base ∪ 课程
/// 增词，调用方拼装；小写折叠与 CJK 片段互不干扰）。
pub(crate) fn derive_naming_list(
    batch: &ProposedBatch,
    known: &[String],
    course: &CourseLexicon,
) -> Vec<String> {
    let mut naming: Vec<String> = Vec::new();
    let defs = batch
        .mints
        .iter()
        .map(|mint| mint.definition.as_str())
        .chain(batch.new_aliases.iter().map(|a| a.definition.as_str()));
    for definition in defs {
        for frag in super::definition_audit::grounding_runs(definition, known) {
            if !naming.contains(&frag) {
                naming.push(frag);
            }
        }
    }
    lexicon::apply_ignores(&naming, course)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_graph::{ConceptMint, ConceptNewAlias, ProposedBatch};

    fn verdict(term: &str, word: &str, note: &str) -> FragmentVerdict {
        FragmentVerdict {
            term: term.into(),
            verdict: serde_json::from_value(serde_json::json!(word)).expect("词表内裁决"),
            note: note.into(),
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
    fn serde_parses_two_word_vocabulary_and_tolerates_missing_note() {
        let parsed: FragmentVerdict =
            serde_json::from_value(serde_json::json!({ "term": "线段", "verdict": "噪声" }))
                .expect("缺 note 合法（serde default 容旧）");
        assert_eq!(parsed.term, "线段");
        assert_eq!(parsed.verdict, FragmentRuling::Noise);
        assert_eq!(parsed.note, "");
        let noun: FragmentVerdict = serde_json::from_value(serde_json::json!(
            { "term": "内角", "verdict": "名词", "note": "构件名词无出处" }
        ))
        .expect("全形状可解析");
        assert_eq!(noun.verdict, FragmentRuling::Noun);
        let bad = serde_json::json!({ "term": "t", "verdict": "大概会" });
        assert!(
            serde_json::from_value::<FragmentVerdict>(bad).is_err(),
            "词表外 verdict = 形状缺陷，不可解析"
        );
    }

    // ── 全覆盖核对 ───────────────────────────────────────────────────────

    #[test]
    fn every_fragment_disposed_as_noise_passes() {
        let naming = ["线段".to_string(), "内角".to_string()];
        let verdicts = vec![
            verdict("线段", "噪声", "切词碎片"),
            verdict("内角", "噪声", "元语言组合"),
        ];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming), Ok(vec![]));
    }

    #[test]
    fn noun_verdict_with_reject_and_problem_mention_passes() {
        let naming = ["内角".to_string()];
        let verdicts = vec![verdict("内角", "名词", "构件名词无出处")];
        let problems = vec![problem("缺口名词「内角」无人教，打回")];
        assert_eq!(
            validate_fragments(&verdicts, false, &problems, &naming),
            Ok(vec![])
        );
    }

    #[test]
    fn noun_verdict_with_pass_is_self_contradictory() {
        let naming = ["内角".to_string()];
        let verdicts = vec![verdict("内角", "名词", "构件名词无出处")];
        let error = validate_fragments(&verdicts, true, &[], &naming).unwrap_err();
        assert!(error.contains("判「名词」却放行"), "{error}");
    }

    #[test]
    fn noun_verdict_without_problem_mention_is_self_contradictory() {
        // pass=false 但 problems 没指名该片段：名词裁决落空，同样矛盾。
        let naming = ["内角".to_string()];
        let verdicts = vec![verdict("内角", "名词", "构件名词无出处")];
        let problems = vec![problem("别的节点难度不实")];
        let error = validate_fragments(&verdicts, false, &problems, &naming).unwrap_err();
        assert!(error.contains("未在 problems 指名"), "{error}");
    }

    #[test]
    fn problem_mention_without_verdict_row_is_a_disposition_when_rejected() {
        // 无裁决行但 problems 指名（pass=false）= 打回处置；指名按片段逐个
        // 对照——被指名的「线段」算处置，未被提及的「内角」仍算未处置。
        let naming = ["线段".to_string(), "内角".to_string()];
        let problems = vec![problem("缺口名词「线段」无人教，打回")];
        assert_eq!(
            validate_fragments(&[], false, &problems, &naming),
            Ok(vec!["内角".to_string()])
        );
    }

    #[test]
    fn problem_mention_with_pass_is_self_contradictory() {
        let naming = ["线段".to_string()];
        let problems = vec![problem("缺口名词「线段」无人教，打回")];
        let error = validate_fragments(&[], true, &problems, &naming).unwrap_err();
        assert!(error.contains("指名却 pass=true"), "{error}");
    }

    #[test]
    fn unhandled_fragments_are_returned_for_the_caller_to_divide() {
        let naming = ["线段".to_string(), "内角".to_string(), "顶点".to_string()];
        let verdicts = vec![verdict("线段", "噪声", "切词碎片")];
        let unhandled = validate_fragments(&verdicts, true, &[], &naming).unwrap();
        assert_eq!(unhandled, vec!["内角".to_string(), "顶点".to_string()]);
    }

    #[test]
    fn substring_containment_counts_as_disposition() {
        // 片段「线段」被裁决行「线段首尾」覆盖（子串互含 = 最小辨认）。
        let naming = ["线段".to_string()];
        let verdicts = vec![verdict("线段首尾", "噪声", "顺次相接的切词噪声")];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming), Ok(vec![]));
        // 反向：裁决行「线」被片段「线段」包含，同样算处置。
        let verdicts = vec![verdict("线", "噪声", "单字残片")];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming), Ok(vec![]));
    }

    #[test]
    fn empty_naming_list_always_passes() {
        assert_eq!(validate_fragments(&[], true, &[], &[]), Ok(vec![]));
        let naming: Vec<String> = vec![];
        let verdicts = vec![verdict("多余裁决", "噪声", "清单外行不核")];
        assert_eq!(validate_fragments(&verdicts, true, &[], &naming), Ok(vec![]));
    }

    // ── 未处置的两档分流 ─────────────────────────────────────────────────

    #[test]
    fn unhandled_become_report_only_findings_by_default() {
        let rows = unhandled_findings(&["内角".to_string(), "顶点".to_string()]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].field, "fragments");
        assert!(rows[0].issue.contains("内角") && rows[0].issue.contains("未处置"));
        assert!(rows[0].node.is_empty(), "批级意见不点名节点");
    }

    #[test]
    fn unhandled_become_synthesized_problems_under_enforce() {
        let rows = unhandled_problems(&["内角".to_string()]);
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
            naming.iter().any(|f| f.contains("线段")),
            "线段须在点名清单:{naming:?}"
        );
        assert!(
            !naming.iter().any(|f| f.contains("三角形")),
            "已知词覆盖处不点名:{naming:?}"
        );
        // 课程豁免：含豁免词的片段视而不见。
        let course = CourseLexicon { words: vec![], ignore: vec!["线段".into()] };
        assert!(
            !derive_naming_list(&batch, &known, &course).iter().any(|f| f.contains("线段")),
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
            naming.iter().any(|f| f.contains("线段")),
            "铸名定义的缺口须点名:{naming:?}"
        );
        assert!(
            naming.iter().any(|f| f == "别名"),
            "别名提名的定义同样入点名域:{naming:?}"
        );
        assert_eq!(
            naming.iter().filter(|f| f.as_str() == "线段").count(),
            1,
            "跨条同名片段去重保序:{naming:?}"
        );
    }

    #[test]
    fn naming_list_block_renders_numbered_list_or_empty_note() {
        let empty = naming_list_block(&[]);
        assert!(empty.contains("(无:本批定义全部有着落)"), "{empty}");
        let block = naming_list_block(&["线段".to_string(), "内角".to_string()]);
        assert!(block.contains("【机械点名清单"), "{block}");
        assert!(block.contains("1. 线段") && block.contains("2. 内角"), "{block}");
    }
}
