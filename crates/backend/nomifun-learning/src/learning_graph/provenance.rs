//! 评审挂号表 `term_provenance` 的机械校验（m3；learnhub #169/ADR-0033 与
//! 修复 939f28b/604f12b 同构）：评审判据①「挂号充分」从原则陈述降为程序
//! 形态——评审结论必带挂号表，接受前由本模块机械核对**行的真伪**。
//!
//! - **表的语义**：批内每条铸名（mints canonical）与别名提名（new_aliases）
//!   的**定义**里引用或预设的领域名词逐词一行——`concept`（提名名）/`term`
//!   （名词）/`source` 四值（已教|自教|常识|缺失）/`ref`（已教/自教行的逐字
//!   出处概念名；常识/缺失行不带）。
//! - **第四值 `缺失` 是承重值**：只有三源可用时，模型只能把查无出处的缺口
//!   洗成「常识」（learnhub 三角形回放 22/24 行常识）。`缺失` 给缺口一个
//!   合法的自认通道，机械层据此强制「自认缺失却放行 = 结论不可用」。
//! - **机械核对只核「行的真伪」**：ref 逐字可解析（已教 → 已知名池；自教 →
//!   批内 teaches 键）、自教 ref 不得指向提名自身（循环自证）、已教 ref 不得
//!   指向本批提名（拿本批产出当已教出处，同循环形态）、常识行 term 逐字命中
//!   名池（事实矛盾）按三前提机械归一（rs2，learnhub #252 裁决 B 平移——
//!   详见 [`validate_term_provenance`]）、批内提名全覆盖、concept ∈ 提名集、
//!   常识/缺失行的 term 逐字落在该提名定义里（尾随括注容忍；已教/自教行写的
//!   是被预设的概念名，不做逐字要求）。**行的完备性与常识真伪不机械核**
//!   （评审员漏列名词与常识裁量归语义层，本模块不做分词点名）。
//! - **校验失败 = 结论不可用**：调用方恰一次带死因重试评审，仍败 = Degraded
//!   保守放行 + findings 留痕（评审器故障 ≠ 批缺陷）。校验域 = 门面注入的
//!   已知名池（[`super::GateOutcome`] 装配：已教账 ∪ 登记表活跃名+别名 ∪
//!   本批铸名/提名，小写折叠）+ 批文档自析，零语义裁决。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{ExternalAnchor, ProposedBatch};

/// 挂号表来源四值词表（与评审提示词逐字一致；`缺失` 承重——缺口的自认通道）。
/// 消费面 = review_schema + 互钉测试（词表-枚举-提示词三面同源）——与
/// growth_contract 的 schema 函数同款「测试钉住的文档面」，运行时无直读。
#[allow(dead_code)]
pub(crate) const TERM_SOURCES: &[&str] = &["已教", "自教", "常识", "缺失"];

/// 挂号表名词出处四值。词表与评审提示词、[`TERM_SOURCES`] 互钉，改词表须同步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum TermSource {
    /// 已教概念（登记表正名或别名，跨课程已教同算；ref 必填）。
    #[serde(rename = "已教")]
    Taught,
    /// 本批节点 teaches 的概念（本批自教；提名本身不是出处；ref 必填）。
    #[serde(rename = "自教")]
    SelfTaught,
    /// 课程外通用常识（不带 ref；term 不得命中名池）。
    #[serde(rename = "常识")]
    CommonSense,
    /// 查无出处（不在已教、本批不自教、也不属于课程外常识）——**缺口本身**。
    /// 只有三源时模型只能把缺口洗成常识；有合法自认通道，机械层才可强制
    /// 「自认缺失却放行 = 结论不可用」。
    #[serde(rename = "缺失")]
    Missing,
}

impl TermSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TermSource::Taught => "已教",
            TermSource::SelfTaught => "自教",
            TermSource::CommonSense => "常识",
            TermSource::Missing => "缺失",
        }
    }
}

/// 挂号表一行：某条提名（`concept`）定义里的领域名词（`term`）及其出处。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct TermProvenance {
    /// 所属提名的名字（mints canonical / new_aliases 名之一）。
    pub(crate) concept: String,
    /// 定义里的领域名词。
    pub(crate) term: String,
    /// 来源四值。
    pub(crate) source: TermSource,
    /// 出处概念名（已教/自教必填逐字；常识/缺失行不带）。
    #[serde(rename = "ref", default)]
    pub(crate) ref_name: Option<String>,
}

/// 机械校验读数（smoke 输出 `rows/invalid/retried/degraded/external_hits`
/// 行的数据源；读数面非门禁）。跨评审调用累加：[`ProvenanceReadout::merge`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct ProvenanceReadout {
    /// 被接受的结论携带的挂号表行数。
    pub(crate) rows: usize,
    /// 机械校验判「结论不可用」的次数（含表缺失/行伪/表键解析失败）。
    pub(crate) invalid: usize,
    /// 带死因重试评审的次数（0 或 1——恰一次）。
    pub(crate) retried: usize,
    /// 重试仍败 → Degraded 保守放行。
    pub(crate) degraded: bool,
    /// 行 concept 命中本批复用锚的行数（f1 A5 观察位：挂号行锚定 registry
    /// 概念的真机可观察信号）。
    pub(crate) external_hits: usize,
    /// 常识行撞名池机械归一的行数（rs2 读数位：归一频率真机可观察不造数）。
    pub(crate) normalized: usize,
}

impl ProvenanceReadout {
    pub(crate) fn merge(&mut self, other: &Self) {
        self.rows += other.rows;
        self.invalid += other.invalid;
        self.retried += other.retried;
        self.degraded |= other.degraded;
        self.external_hits += other.external_hits;
        self.normalized += other.normalized;
    }
}

/// 常识行撞名池的机械归一记录（rs2，learnhub #252 裁决 B 平移）：逐字命中
/// 名池、已过定义原文逐字核、ref 为空的常识行，可无歧义改判为自教/已教；
/// 改判逐条留痕（node="批"，issue 点名 term → 改判 source/ref），不挡门。
/// 声明仍被改写而不是被接受——E1 常识通道不开放；机械层不裁同形词语义，
/// 残余错配靠 findings + 人工抽查兜底。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProvenanceNormalization {
    /// 所属提名。
    pub(crate) concept: String,
    /// 定义里的名词（归一的 term）。
    pub(crate) term: String,
    /// 改判后的来源（自教/已教）。
    pub(crate) source: TermSource,
    /// 改判后的逐字出处（= term）。
    pub(crate) ref_name: String,
}

/// 挂号表机械核对结果：行数（进 `readout.rows`）+ 批外锚命中行数（进
/// `readout.external_hits`，f1 A5）+ 归一记录（rs2，进 `readout.normalized`
/// 计数并交调用方转 findings，不挡门）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TableOutcome {
    pub(crate) rows: usize,
    pub(crate) external_hits: usize,
    pub(crate) normalized: Vec<ProvenanceNormalization>,
}

/// 挂号表机械核对（**非纯函数**——rs2 修复 E 注释如实化：除返回
/// [`TableOutcome`] 外还**就地改写** `rows`——常识行撞名池的机械归一改判行
/// source/ref，归一记录随 [`TableOutcome::normalized`] 一并返回）：违规 =
/// `Err`（死因，进重试反馈），通过 =
/// `Ok`（表结果，进读数）。`pass` = 结论是否放行（缺失行 + 放行 =
/// 自相矛盾）。`external_anchors` = 本批复用命中的登记表概念锚（f1 A1）：行 concept 的
/// 合法域 = 批内提名 ∪ 锚 canonical（小写折叠，对齐 known_names 风格）；
/// 命中锚的行数进读数（A5）。
///
/// # rs2（learnhub #252 裁决 B 平移）：常识行撞名池从「整份作废重掷」改
/// 「机械归一+留痕」——归一三前提 = 常识行 ∧ 已过定义原文逐字核 ∧ ref 为空
/// （缺一不归一，维持 `Err`）；可无歧义归位的两种形态就地改写 `rows`：命中
/// 批内 teaches 键且 term ≠ 本条提名 → 自教 ref=term，命中名池且非本批提名
/// → 已教 ref=term。改判行随 [`TableOutcome::normalized`] 交调用方转
/// findings（不挡门）。其余残余形态（带 ref / 未逐字落地 / 只提名未教 /
/// 缺失行放行）维持原 `Err`，不放宽。
pub(crate) fn validate_term_provenance(
    rows: &mut [TermProvenance],
    pass: bool,
    batch: &ProposedBatch,
    known_names: &HashSet<String>,
    external_anchors: &[ExternalAnchor],
) -> Result<TableOutcome, String> {
    // 名字池一律小写折叠（allo 名字解析全口径小写；池由 gate_batch 装配）。
    // rs2 修复（评审批 A）：批内提名与 teaches 键**同口径折叠**——此前两池
    // 按字面比对而 known 池折叠，大写异形词从自教臂滑到已教臂。折叠只作用
    // 在键比较；行数据（concept/term/ref 原文）不改写，提名显示名另存一份
    // 原文供死因逐字点名。
    let known = known_names;
    let key = |value: &str| value.trim().to_lowercase();
    // 提名集（mints canonical ∪ 别名名）+ 定义对照域（canonical/提名名 → 定义）。
    let mut nominees: HashSet<String> = HashSet::new();
    let mut nominee_display: Vec<&str> = Vec::new();
    let mut definitions: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for mint in &batch.mints {
        let canonical = mint.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        nominees.insert(key(canonical));
        nominee_display.push(canonical);
        definitions.insert(key(canonical), mint.definition.trim());
    }
    for nomination in &batch.new_aliases {
        let name = nomination.name.trim();
        if name.is_empty() {
            continue;
        }
        nominees.insert(key(name));
        nominee_display.push(name);
        definitions.insert(key(name), nomination.definition.trim());
    }
    // 自教域 = 批内节点 teaches 的键，**不含提名**：提名只是命名通道，
    // 把定义里预设的构件词标成「自教 → 某条提名」是纸面自证（learnhub
    // place-value 病例同型）。键折叠同 known 池口径（rs2 修复 A）。
    let self_taught: HashSet<String> = batch
        .nodes
        .iter()
        .flat_map(|node| node.teaches.iter())
        .map(|concept| key(&concept.name))
        .filter(|name| !name.is_empty())
        .collect();
    // 本批复用命中锚的正名（小写折叠，对齐 known_names 风格；f1 A1）——
    // 行 concept 放行域 = 提名 ∪ 锚；命中集之外的 registry 存量不放行。
    let anchors: HashSet<String> = external_anchors
        .iter()
        .map(|anchor| anchor.canonical.trim().to_lowercase())
        .collect();

    // 归一后行 source 会改写，covered 取 owned 串避免与可变借用冲突（rs2）。
    let mut covered: HashSet<String> = HashSet::new();
    let mut external_hits = 0usize;
    let mut normalized: Vec<ProvenanceNormalization> = Vec::new();
    for row in rows.iter_mut() {
        let concept = row.concept.trim();
        let concept_key = key(concept);
        // f1（A1）：行 concept 合法域 = 批内提名 ∪ 本批复用命中锚——canonical
        // 落锚集 = 复用登记表存量，合法（命中行数进读数）。空 concept 与
        // 「两域皆无」（提名、锚都不在）同罪同文：凭空/存量概念不得作为行
        // concept，死因共用一份措辞（A2：指引让重试可满足，替代短路 belt）。
        // 键比对一律折叠（rs2 修复 A）。
        if concept.is_empty() || (!nominees.contains(&concept_key) && !anchors.contains(&concept_key))
        {
            return Err(format!(
                "行 concept 「{}」不在批内提名，也不在本批复用命中——登记表存量概念不得作为行 concept（本批未复用它）：删除该行，或改锚本批实际铸造/提名的概念",
                concept
            ));
        }
        if !nominees.contains(&concept_key) {
            // 走到这里的批外概念必已命中锚（两域皆无已在上方打回）。
            external_hits += 1;
        }
        // 逐字落地只对「文本词」行（常识/缺失）成立：它们声称的是定义原文里
        // 的某个词；已教/自教行写的是定义**所预设的概念名**（如「代数式」
        // 预设「四则运算」而定义原文未必出现该串），不能要求逐字包含
        // （learnhub 真机录制核验：严格版误伤 2/37 行，全为已教行）。
        // 空定义不豁免（learnhub 同构：对照域查在即逐字核对，没有「空定义
        // 跳过」臂——空定义无处逐字落地，常识/缺失行照判行伪）。
        if matches!(row.source, TermSource::CommonSense | TermSource::Missing) {
            if let Some(definition) = definitions.get(&concept_key)
                && !term_grounded(&row.term, definition)
            {
                return Err(format!(
                    "挂号行「{}」的名词未逐字出现在「{concept}」的定义里（常识/缺失行必须来自定义原文）",
                    row.term.trim()
                ));
            }
        }
        covered.insert(concept_key.clone());
        match row.source {
            TermSource::CommonSense => {
                if row.ref_name.as_deref().is_some_and(|r| !r.trim().is_empty()) {
                    return Err(format!("常识行「{}」不得带 ref", row.term.trim()));
                }
                if known.contains(&key(&row.term)) {
                    // rs2（learnhub #252 裁决 B）：逐字命中名池 = 事实矛盾行。
                    // 归一三前提 = 已过定义原文逐字核且 ref 为空（ref 空已在
                    // 上方挡住）；可无歧义归位的两种形态就地改判并留痕，不整
                    // 份作废重掷。
                    let grounded = definitions
                        .get(&concept_key)
                        .is_some_and(|definition| term_grounded(&row.term, definition));
                    if !grounded {
                        return Err(format!(
                            "常识行「{}」逐字命中名池（事实矛盾，应为已教/自教）",
                            row.term.trim()
                        ));
                    }
                    // 改判拿走 owned 串（行就地改写，行借用不得横跨写点）。
                    // term 折叠进对照键（rs2 修复 A）：大写异形的 teaches/提名
                    // 名同口径命中，ref/归一记录仍写行原文。
                    let concept = concept.to_owned();
                    let term = row.term.trim().to_owned();
                    let term_key = key(&term);
                    if self_taught.contains(&term_key) && term_key != concept_key {
                        row.source = TermSource::SelfTaught;
                        row.ref_name = Some(term.clone());
                        normalized.push(ProvenanceNormalization {
                            concept: concept.clone(),
                            term: term.clone(),
                            source: TermSource::SelfTaught,
                            ref_name: term,
                        });
                    } else if !nominees.contains(&term_key) {
                        row.source = TermSource::Taught;
                        row.ref_name = Some(term.clone());
                        normalized.push(ProvenanceNormalization {
                            concept,
                            term: term.clone(),
                            source: TermSource::Taught,
                            ref_name: term,
                        });
                    } else {
                        // 只提名未教（含 term == concept 的循环自证形态）：归位
                        // 成已教 ref=term 会拿本批提名当出处，维持原判重试。
                        return Err(format!(
                            "常识行「{}」逐字命中名池但只命中本批提名（提名本身不是出处，不能机械归位）",
                            row.term.trim()
                        ));
                    }
                }
            }
            TermSource::Taught => {
                let reference = required_ref(row)?;
                if !known.contains(&key(reference)) {
                    return Err(format!(
                        "已教行「{}」的 ref 「{reference}」不在名池（逐字对照）",
                        row.term.trim()
                    ));
                }
                // 名池含本批新铸与提名：指向本批提名 = 拿本批自己的产出当
                // 「已教」出处，与自教循环自证同型，不得过关（键折叠同口径）。
                if nominees.contains(&key(reference)) {
                    return Err(format!(
                        "已教行「{}」的 ref 「{reference}」是本批提名（不能当作已教出处）",
                        row.term.trim()
                    ));
                }
            }
            TermSource::SelfTaught => {
                let reference = required_ref(row)?;
                // 循环自证：概念自己的定义引用了某词，又说该词由本概念自教
                // ——定义预设的词不可能由定义自身供给（折叠口径比对）。
                if key(reference) == concept_key {
                    return Err(format!(
                        "自教行「{}」的 ref 指向提名概念自身（循环自证，该词仍无出处）",
                        row.term.trim()
                    ));
                }
                if !self_taught.contains(&key(reference)) {
                    return Err(format!(
                        "自教行「{}」的 ref 「{reference}」不在批内 teaches（提名本身不是出处）",
                        row.term.trim()
                    ));
                }
            }
            TermSource::Missing => {
                // 自认缺口 = 该名词无出处，批必须被打回；放行即结论自相矛盾。
                // 这是「表必须有用」的锚：没有第四值，模型只能把缺口洗成常识。
                if pass {
                    return Err(format!(
                        "挂号表自认「{}/{}」无出处，结论却放行（缺口未打回）",
                        concept,
                        row.term.trim()
                    ));
                }
                if row.ref_name.as_deref().is_some_and(|r| !r.trim().is_empty()) {
                    return Err(format!("缺失行「{}」不得带 ref", row.term.trim()));
                }
            }
        }
    }
    // 提名缺行照逐字点名原文（nominee_display 原文，键比对已折叠）。
    let uncovered: Vec<&str> = nominee_display
        .iter()
        .copied()
        .filter(|name| !covered.contains(&key(name)))
        .collect();
    if !uncovered.is_empty() {
        return Err(format!("提名缺挂号表行：{}", uncovered.join("、")));
    }
    Ok(TableOutcome { rows: rows.len(), external_hits, normalized })
}

/// 归一记录 → findings 行（rs2；调用方走既有 j09 warn 通道落库，k02 水位
/// 消费——零新面）。field 按提名归属取 `new_concepts` / `new_aliases`（名实
/// 相符，下行维护面据此定位）；node="批" = 批级意见不点名任何节点（与
/// advice 范围条款对齐）。
pub(crate) fn normalization_findings(
    normalized: &[ProvenanceNormalization],
    batch: &ProposedBatch,
) -> Vec<super::ReviewFinding> {
    let aliases: HashSet<&str> = batch
        .new_aliases
        .iter()
        .map(|nomination| nomination.name.trim())
        .collect();
    normalized
        .iter()
        .map(|row| super::ReviewFinding {
            node: "批".into(),
            field: if aliases.contains(row.concept.as_str()) {
                "new_aliases"
            } else {
                "new_concepts"
            }
            .into(),
            issue: format!(
                "常识行「{}」逐字命中名池（事实矛盾），机械改判为 {}（ref={}）",
                row.term,
                row.source.as_str(),
                row.ref_name
            ),
            advice: "留痕不挡门；人工抽查词义贴合，同形词错配时回退评审重裁".into(),
        })
        .collect()
}

/// 行名词是否逐字来自定义原文。宽容面：去掉首尾空白与**尾随括注**（模型常
/// 写「数（直线上点对应的数）」这类注释），其余逐字比对——允许改写的行会把
/// 「名词」变成综述，审计面随之失真。`pub(crate)`：碎片裁决「范围外」机械核
/// （absorb-8 z02）复用同款逐字判定（learnhub `term_grounded` 单源同构），
/// 不另立第二份逐字语义。
pub(crate) fn term_grounded(term: &str, definition: &str) -> bool {
    let t = term.trim();
    if t.is_empty() {
        return false;
    }
    if definition.contains(t) {
        return true;
    }
    for (open, close) in [('（', '）'), ('(', ')')] {
        if let Some(idx) = t.find(open) {
            let head = t[..idx].trim();
            if !head.is_empty() && t.ends_with(close) && definition.contains(head) {
                return true;
            }
        }
    }
    false
}

/// 已教/自教行的 ref 必填（非空）；缺 = 结论不可用。
fn required_ref(row: &TermProvenance) -> Result<&str, String> {
    row.ref_name
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| format!("{}行「{}」缺 ref 出处", row.source.as_str(), row.term))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::learning_graph::{ConceptMint, ConceptNewAlias, ConceptRefInput, ConceptTier, ProposedNode};

    /// 机械校验的全表测试批（learnhub BATCH_CONCEPT 同构 + 一条别名提名）：
    /// - 节点 teaches = {所有权}（自教域）；
    /// - 铸名「借用」，定义含「所有权/引用/作用域」；
    /// - 别名提名「折引用」，定义含「引用」。
    fn batch() -> ProposedBatch {
        ProposedBatch {
            nodes: vec![ProposedNode {
                title: "判读借用".into(),
                purpose: "p".into(),
                intent: String::new(),
                scope: String::new(),
                est: Some(20),
                difficulty: None,
                teaches: vec![ConceptRefInput { name: "所有权".into(), tier: ConceptTier::Know }],
                assumes: Vec::new(),
            }],
            mints: vec![ConceptMint {
                canonical: "借用".into(),
                aliases: vec!["Borrowing".into()],
                definition: "所有权不变式下多读一写：引用随作用域在编译期检查。".into(),
                source: None,
            }],
            new_aliases: vec![ConceptNewAlias {
                name: "折引用".into(),
                definition: "对引用概念的另一叫法。".into(),
                source: "s".into(),
            }],
            note: String::new(),
        }
    }

    /// 已知名池（小写折叠；gate_batch 装配口径：已教账 ∪ 登记表活跃名 ∪
    /// 本批铸名/提名）。
    fn known() -> HashSet<String> {
        ["所有权", "引用", "借用", "borrowing", "折引用"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn row(concept: &str, term: &str, source: TermSource, ref_name: Option<&str>) -> TermProvenance {
        TermProvenance {
            concept: concept.into(),
            term: term.into(),
            source,
            ref_name: ref_name.map(str::to_owned),
        }
    }

    /// 全覆盖两提名、各行合法的表。
    fn legal_rows() -> Vec<TermProvenance> {
        vec![
            row("借用", "所有权", TermSource::Taught, Some("所有权")),
            row("借用", "作用域", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ]
    }

    #[test]
    fn serde_parses_four_source_vocabulary_and_ref_key() {
        let value = serde_json::json!({
            "concept": "借用", "term": "所有权", "source": "已教", "ref": "所有权"
        });
        let parsed: TermProvenance = serde_json::from_value(value).expect("合法行可解析");
        assert_eq!(parsed.source, TermSource::Taught);
        assert_eq!(parsed.ref_name.as_deref(), Some("所有权"));
        for (word, expected) in [
            ("自教", TermSource::SelfTaught),
            ("常识", TermSource::CommonSense),
            ("缺失", TermSource::Missing),
        ] {
            let value = serde_json::json!({ "concept": "c", "term": "t", "source": word });
            let parsed: TermProvenance = serde_json::from_value(value).expect("四值词表可解析");
            assert_eq!(parsed.source, expected, "{word}");
            assert_eq!(parsed.ref_name, None, "ref 缺席 = None（serde 容旧）");
        }
        let bad = serde_json::json!({ "concept": "c", "term": "t", "source": "大概会" });
        assert!(
            serde_json::from_value::<TermProvenance>(bad).is_err(),
            "词表外 source = 形状缺陷，不可解析"
        );
    }

    #[test]
    fn legal_table_passes_with_row_count() {
        let read = validate_term_provenance(&mut legal_rows(), true, &batch(), &known(), &[]).unwrap();
        assert_eq!(read.rows, 3, "合法表过，行数进读数");
        assert_eq!(read.external_hits, 0, "无锚批零命中");
    }

    #[test]
    fn empty_batch_with_empty_table_passes() {
        // 零提名批：空表空转（legacy 形态兼容——off 臂与空批都不卡）。
        let batch = ProposedBatch::default();
        assert_eq!(
            validate_term_provenance(&mut [], true, &batch, &known(), &[]).map(|o| o.rows),
            Ok(0)
        );
    }

    #[test]
    fn taught_ref_not_in_pool_is_red() {
        let mut rows = vec![
            row("借用", "所有权", TermSource::Taught, Some("不存在的概念")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("不在名池"), "{error}");
    }

    #[test]
    fn taught_or_self_taught_row_without_ref_is_red() {
        let mut rows = vec![
            row("借用", "所有权", TermSource::Taught, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("缺 ref"), "{error}");
    }

    #[test]
    fn self_taught_ref_pointing_at_own_nomination_is_circular() {
        // 三角形折叠图案的程序形态：定义引用某词，又称该词由本提名自教
        // ——定义预设的词不可能由定义自身供给，循环自证不得过门。
        let mut rows = vec![
            row("借用", "所有权", TermSource::SelfTaught, Some("借用")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("循环自证"), "{error}");
    }

    #[test]
    fn self_taught_ref_must_be_batch_teaches() {
        // 自教域 = 批内 teaches 的键：「引用」在名池但批内无人教——提名本身
        // 不是出处，挂不出自教行。
        let mut rows = vec![
            row("借用", "引用", TermSource::SelfTaught, Some("引用")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("不在批内 teaches"), "{error}");
    }

    #[test]
    fn self_taught_ref_into_batch_teaches_passes() {
        let mut rows = vec![
            row("借用", "所有权", TermSource::SelfTaught, Some("所有权")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        assert_eq!(
            validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).map(|o| o.rows),
            Ok(2)
        );
    }

    #[test]
    fn taught_ref_pointing_at_batch_nomination_is_red() {
        // 名池含本批新铸（「借用」）；拿本批提名当「已教」出处 = 与自教循环
        // 自证同型，不得过关。
        let mut rows = vec![
            row("借用", "作用域", TermSource::Taught, Some("借用")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("本批提名"), "{error}");
    }

    // ── 常识行撞名池机械归一（rs2；learnhub #252 裁决 B 平移）──────────────

    /// 归一三前提（缺一不归一，维持 Err）齐备 + 命中批内 teaches 键且
    /// term ≠ 本条提名 → 自教 ref=term：行就地改写，归一记录随
    /// [`TableOutcome`] 交调用方转 findings。
    #[test]
    fn common_sense_row_hitting_batch_teaches_normalizes_to_self_taught() {
        let mut rows = vec![
            row("借用", "所有权", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let read = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap();
        assert_eq!(read.rows, 2);
        assert_eq!(read.normalized.len(), 1, "常识行须归一：{read:?}");
        assert_eq!(rows[0].source, TermSource::SelfTaught, "行就地改写");
        assert_eq!(rows[0].ref_name.as_deref(), Some("所有权"));
        assert_eq!(read.normalized[0].concept, "借用");
        assert_eq!(read.normalized[0].term, "所有权");
        assert_eq!(read.normalized[0].source, TermSource::SelfTaught);
        assert_eq!(read.normalized[0].ref_name, "所有权");
    }

    /// 三前提齐备 + 命中名池且非本批提名（「引用」在名池但批内无人教）
    /// → 已教 ref=term。词义同形异义与否机械层不裁，findings 留痕兜底。
    #[test]
    fn common_sense_row_hitting_taught_pool_normalizes_to_taught() {
        let mut rows = vec![
            row("借用", "引用", TermSource::CommonSense, None),
            row("折引用", "所有权", TermSource::Taught, Some("所有权")),
        ];
        let read = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap();
        assert_eq!(read.normalized.len(), 1, "常识行须归一：{read:?}");
        assert_eq!(rows[0].source, TermSource::Taught);
        assert_eq!(rows[0].ref_name.as_deref(), Some("引用"));
        assert_eq!(read.normalized[0].concept, "借用");
        assert_eq!(read.normalized[0].source, TermSource::Taught);
        assert_eq!(read.normalized[0].ref_name, "引用");
    }

    /// 归一前提 = 常识行 ref 为空；带 ref（哪怕 term 撞名池、ref 指向本批
    /// 提名）的常识行仍是行伪 → Err，不得静默归一丢 ref（上游 #252 :2178
    /// 形态平移）。
    #[test]
    fn common_sense_row_with_ref_and_pool_hit_stays_retryable() {
        let mut rows = vec![
            row("借用", "所有权", TermSource::CommonSense, Some("借用")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("不得带 ref"), "{error}");
        assert_eq!(rows[0].source, TermSource::CommonSense, "带 ref 行不得归一");
    }

    /// 归一前提 = 已过定义原文逐字核：锚概念行无定义对照域（核不了），
    /// term 撞名池也维持 Err——名池命中不得绕过落地核。
    #[test]
    fn common_sense_row_hitting_pool_without_verbatim_grounding_stays_retryable() {
        let anchors = vec![anchor("百分数改写为小数")];
        let mut rows = vec![
            row("借用", "所有权", TermSource::Taught, Some("所有权")),
            row("百分数改写为小数", "折引用", TermSource::CommonSense, None),
        ];
        let error =
            validate_term_provenance(&mut rows, true, &batch(), &known(), &anchors).unwrap_err();
        assert!(error.contains("命中名池"), "{error}");
        assert_eq!(rows[1].source, TermSource::CommonSense, "未过逐字核不得归一");
    }

    /// 只提名未教：term 撞名池且逐字落地，但只命中本批提名（批内无人教）
    /// ——机械归位成已教 ref=term 会拿本批产出当出处，维持 Err 重试
    /// （term == concept 的循环自证形态同走此臂）。
    #[test]
    fn common_sense_row_hitting_pool_but_only_nominated_stays_retryable() {
        let mut batch = batch();
        batch.new_aliases[0].definition = "对借用概念的另一叫法。".into();
        let mut rows = vec![
            row("借用", "所有权", TermSource::Taught, Some("所有权")),
            row("折引用", "借用", TermSource::CommonSense, None),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch, &known(), &[]).unwrap_err();
        assert!(error.contains("只命中本批提名"), "{error}");
    }

    /// 「次数」型同形异义（learnhub #252 驱动案）：计数义「次数」撞已教
    /// 「次数」概念——机械层不裁词义，三前提齐备即归一为已教 ref=次数，
    /// 词义出入由 findings 留痕 + 人工抽查兜底（回退条件在票面）。
    #[test]
    fn counting_homograph_pool_hit_normalizes_to_taught() {
        let mut batch = batch();
        batch.nodes.clear();
        batch.mints[0] = ConceptMint {
            canonical: "多项式".into(),
            aliases: Vec::new(),
            definition: "由变量与系数经加减乘与正整数次幂构成，次数取各项次数的最大值。".into(),
            source: None,
        };
        batch.new_aliases.clear();
        let known: HashSet<String> = ["次数"].into_iter().map(str::to_owned).collect();
        let mut rows = vec![row("多项式", "次数", TermSource::CommonSense, None)];
        let read = validate_term_provenance(&mut rows, true, &batch, &known, &[]).unwrap();
        assert_eq!(read.rows, 1);
        assert_eq!(read.normalized.len(), 1, "同形异义行机械归一：{read:?}");
        assert_eq!(rows[0].source, TermSource::Taught);
        assert_eq!(rows[0].ref_name.as_deref(), Some("次数"));
    }

    // ── 大小写折叠回归（rs2 修复 A：批内提名/teaches 键与 known 池同口径）──

    /// 大写异形的 teaches 名（「Ownership」）与折叠后的 term（「ownership」）
    /// 同口径命中自教臂：修复前 teaches 键按字面比对而 known 池折叠，异形词
    /// 从自教臂滑到已教臂（ref 指向批外概念 = 出处判定失真）。行 term/ref
    /// 写原文不改写（折叠面只限键比较）。
    #[test]
    fn case_variant_taught_name_normalizes_to_self_taught() {
        let mut batch = batch();
        batch.nodes[0].teaches[0].name = "Ownership".into();
        batch.mints[0].definition = "ownership 不变式下多读一写：引用随作用域在编译期检查。".into();
        let known: HashSet<String> = ["ownership", "引用"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut rows = vec![
            row("借用", "ownership", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let read = validate_term_provenance(&mut rows, true, &batch, &known, &[]).unwrap();
        assert_eq!(read.normalized.len(), 1, "大写异形 teaches 名须同折叠口径归一：{read:?}");
        assert_eq!(
            rows[0].source,
            TermSource::SelfTaught,
            "自教臂命中（不滑到已教臂）"
        );
        assert_eq!(rows[0].ref_name.as_deref(), Some("ownership"), "行数据写原文");
        assert_eq!(read.normalized[0].source, TermSource::SelfTaught);
    }

    /// 大写异形的提名名（mint canonical「Borrowing」）折叠进提名集：已教 ref
    /// 指向它仍判「本批提名」（拿本批产出当已教出处，不得过关）——修复前
    /// 字面比对漏判放行；行 concept 大写异形（「borrowing」）也同口径命中
    /// 提名域。
    #[test]
    fn case_variant_nominee_ref_still_counts_as_batch_nomination() {
        let mut batch = batch();
        batch.mints[0].canonical = "Borrowing".into();
        let mut rows = vec![
            row("borrowing", "作用域", TermSource::Taught, Some("borrowing")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch, &known(), &[]).unwrap_err();
        assert!(error.contains("本批提名"), "{error}");
    }

    #[test]
    fn common_sense_or_missing_row_with_ref_is_red() {
        let mut rows = vec![
            row("借用", "作用域", TermSource::CommonSense, Some("所有权")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("常识行") && error.contains("不得带 ref"), "{error}");

        let mut rows = vec![
            row("借用", "作用域", TermSource::Missing, Some("所有权")),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, false, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("缺失行") && error.contains("不得带 ref"), "{error}");
    }

    #[test]
    fn uncovered_nomination_is_red() {
        // 漏行：某条提名没有任何挂号行 = 提名定义无人核查。
        let mut rows = vec![row("借用", "所有权", TermSource::Taught, Some("所有权"))];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("缺挂号表行") && error.contains("折引用"), "{error}");
    }

    #[test]
    fn row_concept_outside_nominations_is_red() {
        let mut rows = vec![
            row("幽灵", "作用域", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("不在批内提名"), "{error}");
    }

    #[test]
    fn common_sense_term_must_be_verbatim_in_definition() {
        // 常识/缺失行声称的是定义原文里的某个词；改写成综述 = 行失真。
        let mut rows = vec![
            row("借用", "资源独占的约束", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("未逐字出现"), "{error}");
    }

    /// 空定义不豁免（learnhub 同构）：对照域查在即逐字核对，没有「空定义
    /// 跳过」臂——空定义无处逐字落地，常识/缺失行照判行伪（钉住：曾有
    /// `!definition.is_empty()` 豁免，空定义 + 常识/缺失行会静默过关）。
    #[test]
    fn empty_definition_cannot_ground_common_sense_or_missing_rows() {
        let mut batch = batch();
        batch.mints[0].definition = String::new();
        let mut rows = vec![
            row("借用", "作用域", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch, &known(), &[]).unwrap_err();
        assert!(error.contains("未逐字出现"), "{error}");

        // 缺失行同走这一臂（pass=false 避开缺失行「自认缺口却放行」先报）。
        let mut rows = vec![
            row("借用", "作用域", TermSource::Missing, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, false, &batch, &known(), &[]).unwrap_err();
        assert!(error.contains("未逐字出现"), "{error}");
    }

    #[test]
    fn grounded_term_tolerates_trailing_parenthetical() {
        // 尾随括注可去：term「作用域（编译期检查）」的括注前头逐字在定义里。
        let mut rows = vec![
            row("借用", "所有权", TermSource::Taught, Some("所有权")),
            row("借用", "作用域（编译期检查）", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        assert_eq!(
            validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).map(|o| o.rows),
            Ok(3)
        );
    }

    #[test]
    fn missing_row_with_pass_is_self_contradictory() {
        // 第四值承重：自认缺口却放行 = 表变成橡皮章，结论不可用。
        let mut rows = vec![
            row("借用", "所有权", TermSource::Missing, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error = validate_term_provenance(&mut rows, true, &batch(), &known(), &[]).unwrap_err();
        assert!(error.contains("缺口未打回"), "{error}");
        // 配套 pass=false = 自洽（缺失行合法，结论照打回）。
        assert_eq!(
            validate_term_provenance(&mut rows, false, &batch(), &known(), &[]).map(|o| o.rows),
            Ok(2)
        );
    }

    #[test]
    fn readout_merges_accumulate() {
        let mut total = ProvenanceReadout::default();
        total.merge(&ProvenanceReadout {
            rows: 3,
            invalid: 1,
            retried: 1,
            degraded: false,
            external_hits: 0,
            normalized: 2,
        });
        total.merge(&ProvenanceReadout {
            rows: 2,
            invalid: 0,
            retried: 0,
            degraded: true,
            external_hits: 2,
            normalized: 1,
        });
        assert_eq!(
            total,
            ProvenanceReadout {
                rows: 5,
                invalid: 1,
                retried: 1,
                degraded: true,
                external_hits: 2,
                normalized: 3
            }
        );
    }

    // ── 批外锚放行（f1；A1/A4/A5）──────────────────────────────────────────

    /// 锚集测试原料：登记表存量概念「百分数改写为小数」为本批复用命中
    /// （direct_host 直定复用）——canonical 不在批内提名，锚放行后合法。
    fn anchor(canonical: &str) -> ExternalAnchor {
        ExternalAnchor {
            name: "percent".into(),
            concept_id: "01JANCHORXXXXXXXXXXXXXXXXXX".into(),
            canonical: canonical.into(),
            source: crate::learning_graph::AnchorSource::DirectHost,
        }
    }

    /// f1（A1）红绿样：批外锚放行——行 concept = 锚 canonical（非提名）→
    /// 合法，external_hits 计数（A5）。
    #[test]
    fn external_anchor_canonical_releases_row_concept_and_counts_hits() {
        let anchors = vec![anchor("百分数改写为小数")];
        let mut rows = vec![
            row("借用", "所有权", TermSource::Taught, Some("所有权")),
            row("百分数改写为小数", "作用域", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let read = validate_term_provenance(&mut rows, true, &batch(), &known(), &anchors).unwrap();
        assert_eq!(read.rows, 3, "锚放行后全表过：{read:?}");
        assert_eq!(read.external_hits, 1, "命中锚的行数进读数");
    }

    /// f1（A4）红绿样：复用锚行走现行 taught+ref 路径——ref=锚正名在名池
    /// （gate 已把声明概念并网进 provenance_known），行四字段全数合法。
    #[test]
    fn anchored_row_keeps_taught_ref_semantics() {
        let anchors = vec![anchor("百分数改写为小数")];
        // 名池加锚正名（gate_batch 装配口径：登记表活跃名并入 known）。
        let mut known = known();
        known.insert("百分数改写为小数".to_owned());
        let mut rows = vec![
            row("借用", "所有权", TermSource::Taught, Some("所有权")),
            row(
                "百分数改写为小数",
                "作用域",
                TermSource::Taught,
                Some("百分数改写为小数"),
            ),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let read = validate_term_provenance(&mut rows, true, &batch(), &known, &anchors).unwrap();
        assert_eq!(read.rows, 3, "taught+ref=正名路径全绿：{read:?}");
        assert_eq!(read.external_hits, 1);
    }

    /// f1 红绿样：凭空概念（提名与锚两域皆无）仍拒——死因带存量指引
    /// （A2：指引替代短路），测试只钉「不在批内提名」前缀（既有锚）。
    #[test]
    fn fabricated_concept_outside_both_domains_stays_red_with_stock_guidance() {
        // 锚在场也救不了两域皆无的凭空概念。
        let anchors = vec![anchor("百分数改写为小数")];
        let mut rows = vec![
            row("幽灵", "作用域", TermSource::CommonSense, None),
            row("折引用", "引用", TermSource::Taught, Some("引用")),
        ];
        let error =
            validate_term_provenance(&mut rows, true, &batch(), &known(), &anchors).unwrap_err();
        assert!(error.contains("不在批内提名"), "{error}");
        assert!(error.contains("本批复用命中"), "死因点名锚域：{error}");
        assert!(
            error.contains("删除该行") && error.contains("改锚"),
            "死因带处置指引（存量概念/删除/改锚三要素）：{error}"
        );
    }

    /// f1（A1 边界）：锚 canonical 与提名重合时走现行分支——命中锚计数不
    /// 重复记（伪代码「concept ∈ 提名 → 现行分支不动」）。
    #[test]
    fn nominee_rows_take_the_current_branch_and_skip_anchor_counting() {
        let anchors = vec![anchor("借用")];
        let mut rows = legal_rows();
        let read = validate_term_provenance(&mut rows, true, &batch(), &known(), &anchors).unwrap();
        assert_eq!(read.rows, 3);
        assert_eq!(read.external_hits, 0, "提名行不算锚命中");
    }
}
