//! 概念缺口回灌教练（m3，learning-maintenance-loop 波）的纯机械面：从评审
//! 留痕（`learning_structure_findings` 的 concept-review 行）里认出**概念
//! 缺口**，去重、去掉已处置的（点名的概念已全部在已教账里）、封顶后渲染成
//! 教练上下文【未处置概念缺口（评审留痕）】块的行。纯函数、零 IO：读表与
//! 开关装配在 service，块头与框架语在 `CoachContext::render_on`。
//!
//! 概念缺口 = 账内缺席 × 教学依赖 × 评审未放行（三元合取，见
//! [`is_concept_gap`]）。缺口清单是给「能长节点的消费者」看的信号：评审
//! 留痕里绝大多数是登记面观察与判定注记，光「没出现」不构成缺口，必须是
//! 教学卡在它上面、且评审没有挥手放行——拿不准的宁可不进，误灌噪声会污染
//! 教练对下一批的判断。

use super::defnear::{STEM_SIMILARITY_THRESHOLD, normalize, stem_similarity};
use crate::service::truncate_text;

/// 教练面关心的留痕行。字段名 = 读表查询的列名，行映射由 `sqlx::FromRow`
/// 派生直接接上（查询见 `LearningService::coach_concept_gaps`）；取列一律
/// 走访问器，调用点不写下标。
#[derive(Clone, sqlx::FromRow)]
pub(crate) struct FindingRow {
    seq: i64,
    source: String,
    field: String,
    issue: String,
    advice: String,
}

impl FindingRow {
    fn seq(&self) -> i64 {
        self.seq
    }

    fn source(&self) -> &str {
        &self.source
    }

    fn field(&self) -> &str {
        &self.field
    }

    fn issue(&self) -> &str {
        &self.issue
    }

    fn advice(&self) -> &str {
        &self.advice
    }
}

/// 教练上下文里概念缺口行的上限（去重、去已处置后，按最近一次被指出取前
/// 这么多条）。
pub(crate) const COACH_CONCEPT_GAP_LINES: usize = 5;

/// 缺席措辞——评审原文里「账内没有」的各种说法（field 是自由文本、措辞无枚举，
/// 标记词取自本机真实留痕）：命中其一 = 账内缺席。
const ABSENCE_MARKERS: &[&str] = &[
    "未见",
    "没出现",
    "未出现",
    "没在账",
    "未在账",
    "不在账",
    "无对应条目",
    "没有对应条目",
    "没有独立",
    "未单独入账",
    "没单独入账",
    "未入账",
    "无出处",
    "未教",
    "未挂号",
    "没挂号",
    "存在缺口",
];

/// 教学依赖措辞——缺席之所以构成缺口：它挡着教学（本批或后续节点依赖它、
/// 它是前置底座、要先教/先挂号才承载得了）。issue 或 advice 命中其一即可。
const TEACHING_DEPENDENCY_MARKERS: &[&str] = &[
    "依赖",
    "前置",
    "底座",
    "缺口",
    "先教",
    "补挂号",
    "先挂号",
    "需先挂号",
    "内容依赖",
    "承载",
];

/// 放行词——评审在 issue 里已经挥手放行（按噪声/常识/描述性用语处理、本批
/// 不动）：那是判定注记，不是待处置缺口。只看 issue：advice 常以「否则/
/// 如果」给条件性建议，措辞里带这些词不代表放行。
const DISMISSAL_MARKERS: &[&str] = &["噪声", "不作", "放行", "常识", "描述性用语", "本批不改"];

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
}

/// 这一行是不是概念缺口。
pub(crate) fn is_concept_gap(row: &FindingRow) -> bool {
    let issue = row.issue();
    // (a) 只认评审 warn 通道的留痕：预算留痕（growth-budget）与挂载观察
    // （mount-observation）说的是别的事。
    row.source() == "concept-review"
        // (b) review 字段 = 评审缺席留痕（评审器故障 ≠ 批缺陷）；fragments
        // 字段 = 未处置片段裁决——与本块同用「未处置」一词，不得靠词面串进来。
        && !matches!(row.field().trim(), "review" | "fragments")
        // (c) 账内缺席措辞。
        && contains_any(issue, ABSENCE_MARKERS)
        // (d) 教学依赖措辞。
        && (contains_any(issue, TEACHING_DEPENDENCY_MARKERS)
            || contains_any(row.advice(), TEACHING_DEPENDENCY_MARKERS))
        // (e) 评审没有放行。
        && !contains_any(issue, DISMISSAL_MARKERS)
}

/// 去重后的一个缺口组：评审对同一个缺口的全部留痕。
struct GapGroup<'a> {
    /// 代表行：组内最低 seq 那一行——第一次被指出时的措辞。
    representative: &'a FindingRow,
    /// 组内最高 seq：最后一次被指出。输出序的排序键。
    last_reported: i64,
    /// 组内各行点名的概念名之并（已归一）。
    names: Vec<String>,
}

/// [`concept_gap_lines`] 的产出。
pub(crate) struct ConceptGapLines {
    /// 出的行。
    pub(crate) lines: Vec<String>,
    /// 去重后因点名的概念已全部在已教账里而不再出的组数。
    pub(crate) resolved: usize,
}

/// 从留痕行里取概念缺口，渲染为教练上下文的行。输入按 seq DESC（读表查询
/// 的序）。同一个缺口会被评审反复留痕：并成一组，组内代表行取最低 seq——
/// 第一次被指出的那一条（措辞以第一次为准）。「取最近 K 条」按**最后一次被
/// 指出**排，不按第一次：排序键是组内最高 seq，所以仍在被反复指出的老缺口
/// 不会被后来才出现、此后没人再提的新缺口挤掉。输出至多
/// [`COACH_CONCEPT_GAP_LINES`] 行。非缺口行不出行；已处置的缺口
/// （[`gap_resolved`]，对照 `taught_summary` = 已教概念账文本）也不出行。
pub(crate) fn concept_gap_lines(rows: &[FindingRow], taught_summary: &str) -> ConceptGapLines {
    let mut groups: Vec<GapGroup<'_>> = Vec::new();
    for row in rows.iter().filter(|row| is_concept_gap(row)) {
        let names = concept_names(row);
        match groups
            .iter_mut()
            .find(|group| same_gap(group.representative, row))
        {
            // 同一缺口更早的留痕：接替代表行；last_reported 不动。
            Some(group) => {
                group.representative = row;
                merge_names(&mut group.names, names);
            }
            None => groups.push(GapGroup {
                representative: row,
                last_reported: row.seq(),
                names,
            }),
        }
    }
    groups.sort_by(|a, b| b.last_reported.cmp(&a.last_reported));
    let (open, resolved): (Vec<&GapGroup<'_>>, Vec<&GapGroup<'_>>) = groups
        .iter()
        .partition(|group| !gap_resolved(&group.names, taught_summary));
    ConceptGapLines {
        lines: open
            .iter()
            .take(COACH_CONCEPT_GAP_LINES)
            .map(|group| render_line(group.representative))
            .collect(),
        resolved: resolved.len(),
    }
}

/// 已处置判定：一组缺口点名的概念（已归一）是否已**全部**在已教概念账里
/// （`canonical@档位、…` 一串）。名字按子串找；账面同样归一后再比，所以
/// 不分大小写、名字里的标点与空白不碍事。保守：抽不出名字（或名字归一后为
/// 空）的组无从判断，不算已处置；有一个名字不在账里也不算。
pub(crate) fn gap_resolved(names: &[String], taught_summary: &str) -> bool {
    let taught = normalize(taught_summary);
    !names.is_empty()
        && names
            .iter()
            .all(|name| !name.is_empty() && taught.contains(name.as_str()))
}

/// 名字之并（保序去重）。
fn merge_names(into: &mut Vec<String>, names: Vec<String>) {
    for name in names {
        if !into.contains(&name) {
            into.push(name);
        }
    }
}

/// 两行是不是同一个缺口：issue 措辞近形（评审反复留痕时字句略有出入），或
/// 共享至少一个概念名（同一缺口换说法点名同一批概念）。
fn same_gap(a: &FindingRow, b: &FindingRow) -> bool {
    if stem_similarity(a.issue(), b.issue()) >= STEM_SIMILARITY_THRESHOLD {
        return true;
    }
    let (names_a, names_b) = (concept_names(a), concept_names(b));
    names_a.iter().any(|name| names_b.contains(name))
}

/// 行里的概念名（已归一）：引号段 ∪ 缺席词紧跟的枚举。
fn concept_names(row: &FindingRow) -> Vec<String> {
    let mut names = quoted_segments(row.issue());
    names.extend(enumerated_concepts(row.issue()));
    names.into_iter().map(|name| normalize(&name)).collect()
}

/// 引号对：引号里的文字整体算一个概念名。
const QUOTE_PAIRS: [(char, char); 3] = [('「', '」'), ('『', '』'), ('“', '”')];

/// 引号段：「…」『…』“…” 里的文字整体算一个概念名。
fn quoted_segments(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let Some((_, close)) = QUOTE_PAIRS.iter().find(|(open, _)| *open == ch) else {
            continue;
        };
        let mut name = String::new();
        for inner in chars.by_ref() {
            if inner == *close {
                break;
            }
            name.push(inner);
        }
        if !name.trim().is_empty() {
            names.push(name.trim().to_owned());
        }
    }
    names
}

/// 枚举段的字符上限：更长的段不是概念名，是没被顿号切开的整句。
const CONCEPT_NAME_MAX_CHARS: usize = 12;

/// issue 里首个缺席词紧跟的枚举概念名（「未见容器、接口、元素、返回类型等
/// 基础概念」→ 容器 / 接口 / 元素 / 返回类型）：缺席词之后、首个
/// 等/，/；/。/（ 之前的文字，按 、/和/与 切开，留 1..=12 字的段。
/// 只有成清单才算（至少两段，或以「等」收尾）——缺席词后面不一定是枚举：
/// 「没在账内出现」紧跟的是续词「内出现」，「无出处：「栈区」是本批的前置」
/// 紧跟的是半句话；单段收进来，措辞同款、概念不同的两条缺口就会被误并。
fn enumerated_concepts(issue: &str) -> Vec<String> {
    let Some(marker) = ABSENCE_MARKERS
        .iter()
        .filter_map(|marker| issue.find(marker).map(|at| (at, *marker)))
        .min_by_key(|(at, _)| *at)
        .map(|(_, marker)| marker)
    else {
        return Vec::new();
    };
    let rest = &issue[issue.find(marker).unwrap() + marker.len()..];
    let end = rest
        .find(['等', '，', '；', '。', '（'])
        .unwrap_or(rest.len());
    let parts: Vec<&str> = rest[..end]
        .split(['、', '和', '与'])
        .map(str::trim)
        .collect();
    if parts.len() < 2 && !rest[end..].starts_with('等') {
        return Vec::new();
    }
    parts
        .into_iter()
        .filter(|part| (1..=CONCEPT_NAME_MAX_CHARS).contains(&part.chars().count()))
        .map(str::to_owned)
        .collect()
}

/// 每段（issue 首句 / advice 首句）的字符上限——与挂载观察池行同一截断口径
/// （[`truncate_text`]：按字符截、截动补省略号）。
const LINE_PART_MAX_CHARS: usize = 120;

/// 首句：首个 。！？；换行之前的文字（分隔符不留、两侧空白不留）。文本以句
/// 读开头时取第一句有内容的——行不能没有内容，也不能把句读堆在行首。
fn first_sentence(text: &str) -> &str {
    text.split(['。', '！', '？', '；', '\n'])
        .map(str::trim)
        .find(|sentence| !sentence.is_empty())
        .unwrap_or("")
}

/// 一行缺口：`- {issue 首句}（建议：{advice 首句}）`；advice 为空就不带
/// 建议尾（沿用评审问题清单 `- [{field}] {node}: {issue}（建议：{advice}）`
/// 的尾形）。两段各截到 [`LINE_PART_MAX_CHARS`]。
fn render_line(row: &FindingRow) -> String {
    let issue_head = truncate_text(first_sentence(row.issue()), LINE_PART_MAX_CHARS);
    if row.advice().trim().is_empty() {
        format!("- {issue_head}")
    } else {
        format!(
            "- {issue_head}（建议：{}）",
            truncate_text(first_sentence(row.advice()), LINE_PART_MAX_CHARS)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一行留痕夹具（列同读表查询）。
    fn finding(seq: i64, source: &str, field: &str, issue: &str, advice: &str) -> FindingRow {
        FindingRow {
            seq,
            source: source.into(),
            field: field.into(),
            issue: issue.into(),
            advice: advice.into(),
        }
    }

    fn concept_review(seq: i64, field: &str, issue: &str, advice: &str) -> FindingRow {
        finding(seq, "concept-review", field, issue, advice)
    }

    /// DB seq 13（`field = ""`）——旗舰缺口行。
    const SEQ_13_ISSUE: &str = "已教概念账中未见容器、接口、元素、返回类型等基础概念，本批迭代器协议与结果类型又直接依赖它们，疑似课程在抽象与类型系统线的前置底座存在缺口。";
    const SEQ_13_ADVICE: &str =
        "后续批次可先补铸容器、接口、返回类型等基础概念，再承载迭代器协议与结果类型等前沿节点。";

    /// 分类口径钉在本机真实留痕上（`learning_structure_findings` 的库行与
    /// 归档的评审原文，逐字）：真缺口只有旗舰行（seq 13）与两条 intent 行；
    /// 登记面观察、判定注记、评审缺席留痕、预算留痕一律不进。归档行不是
    /// 库行，seq 只是夹具键（101 起）。
    #[test]
    fn concept_gap_classification_admits_only_real_gap_rows() {
        let table = [
            (
                "DB seq 13",
                concept_review(13, "", SEQ_13_ISSUE, SEQ_13_ADVICE),
                true,
                "账内未见 + 本批依赖 + 疑似前置底座缺口，评审没有放行",
            ),
            (
                "DB seq 3",
                concept_review(
                    3,
                    "review",
                    "评审缺席——Degraded 保守放行（评审器故障 ≠ 批缺陷）；死因：评审调用失败：Conflict: 生成已被用户取消",
                    "本批未获语义评审背书，留痕供修补回合关注；连续评审缺席时先查评审模型可用性",
                ),
                false,
                "评审缺席留痕住 review 字段——说的是评审器故障，不是概念缺口",
            ),
            (
                "DB seq 11",
                concept_review(
                    11,
                    "mints",
                    "「运行期类型信息」定义中的「元数据」为通用计算背景词，未在登记表出现；本批按常识处理。",
                    "如后续元编程线要反复使用该词，建议在课程用语表增列「元数据」以免逐批复述。",
                ),
                false,
                "登记面观察且已按常识处理——「未在登记表出现」不是账内缺席措辞，教学也不依赖它",
            ),
            (
                "归档 intent（死锁）",
                concept_review(
                    101,
                    "intent",
                    "intent 以「锁有开销与死锁风险」为由引入动机时提到死锁，但死锁在已教账、本批 teaches/assumes 中都没出现",
                    "若死锁要作为内容依赖就补挂号（或先教），否则把措辞弱化为「锁的额外风险」一类背景表述",
                ),
                true,
                "死锁在已教账与本批 teaches/assumes 中都没出现，advice 点名它可能要作为内容依赖",
            ),
            (
                "归档 intent（垃圾回收）",
                concept_review(
                    102,
                    "intent",
                    "intent 用「垃圾回收家族」「追踪式 GC」定位本节点，作为上位概念的垃圾回收没在账内出现（「回收」本身已由已教 堆分配 覆盖）",
                    "如果只是描述内存管理线的谱系位置，可保留为背景；如果要求学习者辨认垃圾回收家族，需先挂号",
                ),
                true,
                "上位概念垃圾回收没在账内出现，advice 给出「需先挂号」的教学依赖条件",
            ),
            (
                "DB seq 14",
                concept_review(
                    14,
                    "title",
                    "标题与 intent 中的「接口」「容器」是编程领域常见用语，概念登记表与已教账均无对应条目；本批把它们作为描述性用语放行（指涉的正是本点所教的统一取用约定）",
                    "若后续要把「接口」「容器」作为独立教学点，走课程用语表增词提案正式立项，或在改写标题时统一措辞",
                ),
                false,
                "登记表与已教账均无对应条目，但评审按描述性用语放行、issue 与 advice 都没说教学依赖它",
            ),
            (
                "DB seq 1",
                concept_review(
                    1,
                    "teaches",
                    "整数、浮点、布尔、字符串四类基础值在已教账与登记表中没有独立词条，仅作为 值分类 的下位内容存在",
                    "后续节点若需单独引用这些值类别词面，走课程用语表增词提案，或统一引用 值分类 的别名与定义词面",
                ),
                false,
                "没有独立词条，但只是 值分类 的下位内容——没有教学依赖",
            ),
            (
                "DB seq 16",
                concept_review(
                    16,
                    "",
                    "登记表没有独立的「内存释放/回收」概念名，而「堆分配」「引用计数」等既有定义及本节点标题都使用「释放/回收／回收机制」这类词",
                    "考虑在课程用语表增补「内存释放（回收）」词条，供后续内存管理节点统一挂号，避免同类词反复以描述性短语出现",
                ),
                false,
                "没有独立概念名，advice 只是建议增补词条——没有教学依赖",
            ),
            (
                "归档 mints（堆对象）",
                concept_review(
                    103,
                    "mints",
                    "「堆对象」作为引用计数定义的构成词未单独入账；当前按常识处理，但若后续 GC 节点需严格区分堆对象与堆分配，建议将「堆对象」纳入课程用语表或随节点 teaches 挂号。",
                    "在后续引入 GC 或内存模型对比节点时，将「堆对象」作为 teaches 或课程用语表词条显式挂牌。",
                ),
                false,
                "未单独入账但当前按常识处理，advice 只是条件性建议——没有教学依赖",
            ),
            (
                "后续真实回合（登记表）",
                concept_review(
                    104,
                    "登记表",
                    "「编译器」「解释器」未在已教账与登记表内，本批定义以常识引用；后续执行模型线若深入可考虑登记",
                    "",
                ),
                false,
                "已按常识引用放行，没有教学依赖，也没有账内缺席措辞",
            ),
            (
                "后续真实回合（intent）",
                concept_review(
                    105,
                    "intent",
                    "「垃圾回收」「所有权转移」作为内存管理线后续学习点被提及，登记表内暂无对应概念",
                    "",
                ),
                false,
                "「暂无对应概念」不是账内缺席措辞，也没有教学依赖",
            ),
            (
                "DB seq 12",
                finding(
                    12,
                    "growth-budget",
                    "",
                    "自动补批超时（墙钟 10 分钟）：本批尚未过评审门，不再发起评审（3 个节点在途作废）",
                    "",
                ),
                false,
                "预算留痕不是评审留痕（source = growth-budget）",
            ),
            (
                "DB seq 9",
                concept_review(
                    9,
                    "teaches",
                    "提名定义与节点 scope 中的「栈区／堆区」未在概念登记表单列（结构材料中「栈帧分配」「堆分配」是其后续块）；本批按「定义句内就地给出划分判据、属本节点教学内容」处理为噪声，不作登记缺口。",
                    "建议走课程用语表增词提案，把「栈区」「堆区」列为用语，供后续 栈帧分配／堆分配／垃圾回收 节点直接复用，避免各批重复就地定义。",
                ),
                false,
                "评审自己已处理为噪声、不作登记缺口——没有账内缺席措辞，issue 还带放行词",
            ),
        ];
        for (label, row, expected, reason) in table {
            assert_eq!(is_concept_gap(&row), expected, "{label}：{reason}");
        }
    }

    /// (e) 放行词：评审已经挥手放行的观察不是待处置缺口——哪怕它同时带缺席
    /// 与教学依赖措辞。标记词逐个钉（测试侧硬编码，不读实现常量）。
    #[test]
    fn concept_gap_classification_rejects_rows_the_reviewer_waved_through() {
        for dismissal in ["噪声", "不作", "放行", "常识", "描述性用语", "本批不改"]
        {
            let row = concept_review(
                20,
                "",
                &format!(
                    "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们；评审意见：{dismissal}。"
                ),
                "后续批次先补教。",
            );
            assert!(
                !is_concept_gap(&row),
                "issue 带放行词「{dismissal}」：评审已放行，不是待处置缺口"
            );
        }
        // 放行词只看 issue：advice 惯用条件句给建议（「若只是背景可保留…否则
        // …」），措辞里带这些词不代表评审放行。
        let advice_only = concept_review(
            21,
            "",
            "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们。",
            "若只是背景表述可按常识保留；要作为教学点则先教。",
        );
        assert!(
            is_concept_gap(&advice_only),
            "放行词只出现在 advice：issue 仍是未放行的缺口"
        );
    }

    /// (a)(b) 来源与字段边界：措辞再像缺口，不是评审留痕、或住 review /
    /// fragments 字段的行一律不进——两类行各有自己的消费面（评审缺席供修补
    /// 回合关注；未处置片段走碎片裁决），且它们与本块同用「未处置」一词。
    #[test]
    fn concept_gap_classification_ignores_other_sources_and_reserved_fields() {
        const GAP_ISSUE: &str = "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们。";
        const GAP_ADVICE: &str = "后续批次先补教。";
        for source in ["growth-budget", "mount-observation"] {
            let row = finding(30, source, "", GAP_ISSUE, GAP_ADVICE);
            assert!(!is_concept_gap(&row), "source = {source}：不是评审留痕");
        }
        for field in ["review", "fragments"] {
            let row = concept_review(31, field, GAP_ISSUE, GAP_ADVICE);
            assert!(!is_concept_gap(&row), "field = {field}：保留字段不进缺口块");
        }
    }

    /// (c) 缺席措辞标记逐个钉（测试侧硬编码标记词，不读实现常量——漏一个词即红）。
    #[test]
    fn concept_gap_classification_recognises_every_absence_marker() {
        for marker in [
            "未见",
            "没出现",
            "未出现",
            "没在账",
            "未在账",
            "不在账",
            "无对应条目",
            "没有对应条目",
            "没有独立",
            "未单独入账",
            "没单独入账",
            "未入账",
            "无出处",
            "未教",
            "未挂号",
            "没挂号",
            "存在缺口",
        ] {
            let row = concept_review(
                40,
                "",
                &format!("已教概念账中{marker}「栈区」，本批节点直接依赖它。"),
                "后续批次先补教。",
            );
            assert!(is_concept_gap(&row), "缺席措辞「{marker}」应被认出");
        }
        // 不带任何缺席措辞的观察不是缺口——哪怕 issue 自称是缺口。
        let no_absence = concept_review(
            41,
            "",
            "「元数据」为通用计算背景词，登记表里可以增列；本批节点依赖它。",
            "后续批次先补教。",
        );
        assert!(
            !is_concept_gap(&no_absence),
            "没有账内缺席措辞：不是概念缺口"
        );
    }

    /// (d) 教学依赖措辞标记逐个钉——写进 issue 或写进 advice 都算（测试侧硬编码
    /// 标记词，不读实现常量）。
    #[test]
    fn concept_gap_classification_requires_a_teaching_dependency_marker() {
        for marker in [
            "依赖",
            "前置",
            "底座",
            "缺口",
            "先教",
            "补挂号",
            "先挂号",
            "需先挂号",
            "内容依赖",
            "承载",
        ] {
            let in_issue = concept_review(
                50,
                "",
                &format!("已教概念账中未见「栈区」这一概念；备注：{marker}。"),
                "后续批次处理。",
            );
            assert!(
                is_concept_gap(&in_issue),
                "issue 带教学依赖措辞「{marker}」"
            );
            let in_advice = concept_review(
                51,
                "",
                "已教概念账中未见「栈区」这一概念。",
                &format!("后续批次处理：{marker}。"),
            );
            assert!(
                is_concept_gap(&in_advice),
                "advice 带教学依赖措辞「{marker}」"
            );
        }
        let neither = concept_review(
            52,
            "",
            "已教概念账中未见「栈区」这一概念。",
            "后续批次可考虑增补词条。",
        );
        assert!(
            !is_concept_gap(&neither),
            "缺席但与教学无涉：不是缺口（拿不准的宁可不进）"
        );
    }

    /// 行格式：`- {issue 首句}（建议：{advice 首句}）`。首句 = 首个 。！？；
    /// 换行之前的文字（分隔符不留）；advice 为空 = 不带建议尾。非缺口行不出行。
    #[test]
    fn concept_gap_lines_carry_the_first_sentence_of_issue_and_advice() {
        // 旗舰行：issue / advice 各到句号为止。
        let lines = concept_gap_lines(&[concept_review(13, "", SEQ_13_ISSUE, SEQ_13_ADVICE)], "").lines;
        assert_eq!(
            lines,
            vec![
                "- 已教概念账中未见容器、接口、元素、返回类型等基础概念，本批迭代器协议与结果类型又直接依赖它们，疑似课程在抽象与类型系统线的前置底座存在缺口（建议：后续批次可先补铸容器、接口、返回类型等基础概念，再承载迭代器协议与结果类型等前沿节点）".to_owned()
            ],
            "issue 与 advice 各取首句、句号不留"
        );
        // advice 的首句到分号为止；issue 没有句末标点就取全文。
        let lines = concept_gap_lines(
            &[concept_review(
                15,
                "intent",
                "intent 用「垃圾回收家族」「追踪式 GC」定位本节点，作为上位概念的垃圾回收没在账内出现（「回收」本身已由已教 堆分配 覆盖）",
                "如果只是描述内存管理线的谱系位置，可保留为背景；如果要求学习者辨认垃圾回收家族，需先挂号",
            )],
            "",
        )
        .lines;
        assert_eq!(
            lines,
            vec![
                "- intent 用「垃圾回收家族」「追踪式 GC」定位本节点，作为上位概念的垃圾回收没在账内出现（「回收」本身已由已教 堆分配 覆盖）（建议：如果只是描述内存管理线的谱系位置，可保留为背景）".to_owned()
            ],
            "advice 首句到「；」为止"
        );
        // advice 为空：只有 issue 首句。
        let lines = concept_gap_lines(
            &[concept_review(
                16,
                "",
                "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们。",
                "",
            )],
            "",
        )
        .lines;
        assert_eq!(
            lines,
            vec!["- 已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们".to_owned()],
            "advice 为空 = 不带建议尾"
        );
        // 非缺口行不出行。
        let dismissed = concept_review(
            17,
            "",
            "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们；按常识处理。",
            "后续批次先补教。",
        );
        assert!(
            concept_gap_lines(&[dismissed], "").lines.is_empty(),
            "被放行的行不进缺口块"
        );
    }

    /// 首句切分：。！？；与换行各自结束首句。
    #[test]
    fn concept_gap_lines_cut_each_part_at_its_first_sentence_end() {
        for delimiter in ['。', '！', '？', '；', '\n'] {
            let row = concept_review(
                18,
                "",
                &format!("已教概念账中未见「栈区」{delimiter}后半句不该出现，本批依赖它。"),
                &format!("后续批次先教{delimiter}后半句不该出现。"),
            );
            assert_eq!(
                concept_gap_lines(std::slice::from_ref(&row), "").lines,
                vec!["- 已教概念账中未见「栈区」（建议：后续批次先教）".to_owned()],
                "首句到「{delimiter:?}」为止"
            );
        }
        // 行尾的回车不留。
        let row = concept_review(
            19,
            "",
            "已教概念账中未见「栈区」\r\n后半句，本批依赖它。",
            "后续批次先教。",
        );
        assert_eq!(
            concept_gap_lines(std::slice::from_ref(&row), "").lines,
            vec!["- 已教概念账中未见「栈区」（建议：后续批次先教）".to_owned()],
            "首句两侧的空白与回车不留"
        );
        // 以句读开头：跳过空首句取第一句有内容的——行不能没有内容，也不能
        // 把句读堆在行首。
        let row = concept_review(
            20,
            "",
            "；。已教概念账中未见「栈区」，本批依赖它",
            "后续批次先教。",
        );
        assert_eq!(
            concept_gap_lines(std::slice::from_ref(&row), "").lines,
            vec!["- 已教概念账中未见「栈区」，本批依赖它（建议：后续批次先教）".to_owned()],
            "首句为空：取第一句有内容的"
        );
        // advice 同理。
        let row = concept_review(
            23,
            "",
            "已教概念账中未见「栈区」，本批依赖它。",
            "；后续批次先教。",
        );
        assert_eq!(
            concept_gap_lines(std::slice::from_ref(&row), "").lines,
            vec!["- 已教概念账中未见「栈区」，本批依赖它（建议：后续批次先教）".to_owned()],
            "advice 首句为空：同样取第一句有内容的"
        );
    }

    /// 每段（issue 首句 / advice 首句）各截到 120 字符，截动处补省略号；恰 120
    /// 字符不动。
    #[test]
    fn concept_gap_lines_truncate_each_part_to_120_chars() {
        let long = "甲".repeat(130);
        let row = concept_review(
            21,
            "",
            &format!("已教账未见{long}，本批依赖它"),
            &format!("后续批次先教{long}。"),
        );
        assert_eq!(
            concept_gap_lines(std::slice::from_ref(&row), "").lines,
            vec![format!(
                "- 已教账未见{}…（建议：后续批次先教{}…）",
                "甲".repeat(115),
                "甲".repeat(114)
            )],
            "issue 首句 135 字符、advice 首句 136 字符：各截到 120 字符"
        );
        // 恰 120 字符：不截、不带省略号。
        let exact = format!("已教账未见{}", "甲".repeat(115));
        let row = concept_review(22, "", &format!("{exact}。本批依赖它。"), "");
        assert_eq!(
            concept_gap_lines(std::slice::from_ref(&row), "").lines,
            vec![format!("- {exact}")],
            "120 字符是上限本身：不动"
        );
    }

    const FLAGSHIP_LINE: &str = "- 已教概念账中未见容器、接口、元素、返回类型等基础概念，本批迭代器协议与结果类型又直接依赖它们，疑似课程在抽象与类型系统线的前置底座存在缺口（建议：后续批次可先补铸容器、接口、返回类型等基础概念，再承载迭代器协议与结果类型等前沿节点）";

    /// 去重（措辞近形）：同一轮的多次评审调用、跨轮的重复观察会把同一缺口
    /// 反复留痕；措辞近形的只出一行，代表行取最低 seq——第一次被指出的那一
    /// 条。夹具两行都没有引号段、缺席词后也不是清单——去重只能走措辞通道。
    #[test]
    fn concept_gap_lines_merge_near_duplicate_wording_keeping_the_oldest_row() {
        let older = concept_review(
            3,
            "",
            "栈区在已教账中未见身影，本批节点直接依赖它。",
            "后续批次先补教。",
        );
        let newer = concept_review(
            9,
            "",
            "栈区在已教账中未见踪影，本批节点直接依赖它们。",
            "后续批次要先补教。",
        );
        assert!(
            stem_similarity(older.issue(), newer.issue()) >= STEM_SIMILARITY_THRESHOLD,
            "夹具前提：两条措辞近形（相似度 {}）",
            stem_similarity(older.issue(), newer.issue())
        );
        assert_eq!(
            concept_gap_lines(&[newer, older], "").lines,
            vec!["- 栈区在已教账中未见身影，本批节点直接依赖它（建议：后续批次先补教）".to_owned()],
            "近形措辞只出一行，且是最低 seq 那一行（输入按 seq DESC）"
        );
    }

    /// 去重（共享概念名）：措辞不近形，但引号段点到旗舰行枚举里的同名概念
    /// ——同一个缺口。夹具前提先钉两条措辞不近形，去重只能走概念名通道。
    #[test]
    fn concept_gap_lines_merge_rows_that_name_the_same_concept() {
        let variant = concept_review(
            31,
            "title",
            "标题与 intent 中的「接口」「容器」在已教概念账中均无对应条目，本批节点直接依赖它们才成立",
            "后续批次先把它们补教。",
        );
        assert!(
            stem_similarity(variant.issue(), SEQ_13_ISSUE) < STEM_SIMILARITY_THRESHOLD,
            "夹具前提：两条措辞不近形（相似度 {}）",
            stem_similarity(variant.issue(), SEQ_13_ISSUE)
        );
        assert!(
            is_concept_gap(&variant),
            "夹具前提：变体行本身是缺口（未放行、带教学依赖）"
        );
        assert_eq!(
            concept_gap_lines(&[variant, concept_review(13, "", SEQ_13_ISSUE, SEQ_13_ADVICE)], "").lines,
            vec![FLAGSHIP_LINE.to_owned()],
            "共享「接口」「容器」：同一个缺口，代表行取最低 seq"
        );
    }

    /// 封顶：去重后取最近的 5 行（newest first），更早的让位。夹具前提先钉
    /// 七行两两不近形——被截掉的是封顶，不是去重。
    #[test]
    fn concept_gap_lines_keep_the_five_newest_gaps() {
        let issues: [&str; 7] = [
            "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们。",
            "「死锁」在已教账中没出现，本批 intent 以它为内容依赖。",
            "作为上位概念的「垃圾回收」没在账内出现，学习者要先学会它才能承载本节点。",
            "已教账无出处：「栈区」是本批的前置。",
            "「堆对象」未入账，后续 GC 节点依赖它。",
            "「所有权转移」未挂号，本批内容依赖它。",
            "抽象线的前置底座存在缺口：「函数签名」未被教过。",
        ];
        for (index, issue) in issues.iter().enumerate() {
            for other in issues.iter().skip(index + 1) {
                assert!(
                    stem_similarity(issue, other) < STEM_SIMILARITY_THRESHOLD,
                    "夹具前提：第 {index} 行与「{other}」不近形（相似度 {}）",
                    stem_similarity(issue, other)
                );
            }
        }
        // 输入按 seq DESC：7 … 1。
        let rows: Vec<FindingRow> = issues
            .iter()
            .enumerate()
            .map(|(index, issue)| concept_review(7 - index as i64, "", issue, ""))
            .collect();
        let expected: Vec<String> = [
            "- 已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们",
            "- 「死锁」在已教账中没出现，本批 intent 以它为内容依赖",
            "- 作为上位概念的「垃圾回收」没在账内出现，学习者要先学会它才能承载本节点",
            "- 已教账无出处：「栈区」是本批的前置",
            "- 「堆对象」未入账，后续 GC 节点依赖它",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(
            concept_gap_lines(&rows, "").lines,
            expected,
            "只留最近的 5 行，newest first"
        );
        // 去重在封顶之前：更高 seq 的重复行（seq 10，与 seq 7 同文——同一轮
        // 的多次评审调用会把同一缺口反复留痕）并进原组、不占名额——不去重的
        // 话它会自己占一个名额、挤掉 seq 3 那一行。复述的是最近那个缺口，
        // 不改变任何一组的最近序（复述老缺口会把它提到最前，见下一条用例）。
        let mut with_duplicate = vec![concept_review(10, "", issues[0], "")];
        with_duplicate.extend(rows);
        assert_eq!(
            concept_gap_lines(&with_duplicate, "").lines,
            expected,
            "重复行先并掉：结果与没有它时逐行相同"
        );
    }

    /// 「取最近 K 条」按**最后一次被指出**的 seq 排，不按第一次：六个互不近形
    /// 的缺口里，最低 seq 的那个在最高 seq 上又被近形复述了一遍——它仍在输
    /// 出里（且排最前），让位的是最近没人再指出的那一个（seq 2）。出的行仍
    /// 是第一次被指出时的措辞（seq 1），不是复述的措辞（seq 7）。
    #[test]
    fn concept_gap_lines_rank_a_gap_by_when_it_was_last_reported() {
        // 六个缺口按 seq 6 … 1 依序给出；末行（最低 seq 1）就是被复述的那一个。
        let issues: [&str; 6] = [
            "「所有权转移」未挂号，本批内容依赖它。",
            "「堆对象」未入账，后续 GC 节点依赖它。",
            "已教账无出处：「栈区」是本批的前置。",
            "作为上位概念的「垃圾回收」没在账内出现，学习者要先学会它才能承载本节点。",
            "「死锁」在已教账中没出现，本批 intent 以它为内容依赖。",
            "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们。",
        ];
        const RESTATED: &str =
            "已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们才成立。";
        assert!(
            stem_similarity(issues[5], RESTATED) >= STEM_SIMILARITY_THRESHOLD,
            "夹具前提：seq 7 是 seq 1 那条缺口的近形复述（相似度 {}）",
            stem_similarity(issues[5], RESTATED)
        );
        assert_ne!(
            first_sentence(RESTATED),
            first_sentence(issues[5]),
            "夹具前提：复述的措辞与第一次的不同——出的行取哪一条措辞可辨"
        );
        // 六条互不近形：seq 1 那条缺口以复述行代表（它与 seq 1 近形，见上）。
        let mut distinct: Vec<&str> = issues[..5].to_vec();
        distinct.push(RESTATED);
        for (index, issue) in distinct.iter().enumerate() {
            for other in distinct.iter().skip(index + 1) {
                assert!(
                    stem_similarity(issue, other) < STEM_SIMILARITY_THRESHOLD,
                    "夹具前提：第 {index} 行与「{other}」不近形（相似度 {}）",
                    stem_similarity(issue, other)
                );
            }
        }

        // 输入按 seq DESC：复述行 7 在前，其后 6 … 1。
        let mut rows: Vec<FindingRow> = vec![concept_review(7, "", RESTATED, "")];
        rows.extend(
            issues
                .iter()
                .enumerate()
                .map(|(index, issue)| concept_review(6 - index as i64, "", issue, "")),
        );
        let expected: Vec<String> = [
            "- 已教概念账中未见容器、接口等基础概念，本批节点直接依赖它们",
            "- 「所有权转移」未挂号，本批内容依赖它",
            "- 「堆对象」未入账，后续 GC 节点依赖它",
            "- 已教账无出处：「栈区」是本批的前置",
            "- 作为上位概念的「垃圾回收」没在账内出现，学习者要先学会它才能承载本节点",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(
            concept_gap_lines(&rows, "").lines,
            expected,
            "按最后一次被指出排：seq 1 那条缺口最近还在被指出，留；seq 2 那条最久没人提，让位"
        );
    }

    /// 夹具：一组缺口点名的概念名（与 [`concept_names`] 同样归一）。
    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| normalize(value)).collect()
    }

    /// 已处置 = 点名的概念**全部**已在已教概念账里。账面是 `canonical@档位、…`
    /// 一串：名字按子串找——不分大小写，标点与空白不碍事（名字是归一过的，
    /// 账面同样归一后再比）。
    #[test]
    fn gap_is_resolved_when_every_concept_it_names_is_taught() {
        let taught = "死锁@teach、GC@apply、内存释放/回收@teach";
        assert!(gap_resolved(&names(&["死锁"]), taught), "一个名字，在账");
        assert!(gap_resolved(&names(&["死锁", "GC"]), taught), "两个名字都在账");
        assert!(gap_resolved(&names(&["内存释放/回收"]), taught), "账面与名字里的标点不碍事");
    }

    /// 保守：有一个名字不在账里就不算已处置——缺口可能只补了一半。
    #[test]
    fn gap_is_kept_when_only_some_of_its_concepts_are_taught() {
        let taught = "死锁@teach、向量基础@apply";
        assert!(!gap_resolved(&names(&["死锁", "栈区"]), taught), "栈区不在账");
        assert!(!gap_resolved(&names(&["栈区", "堆区"]), taught), "一个都不在账");
    }

    /// 抽不出概念名的组无从判断——留。归一后为空的名字同理（不算在账）。
    #[test]
    fn gap_is_kept_when_it_names_no_concept() {
        let taught = "死锁@teach";
        assert!(!gap_resolved(&names(&[]), taught), "没有名字：不能判，留");
        assert!(!gap_resolved(&names(&["——"]), taught), "名字归一后为空：等于没有名字");
    }

    /// 块名是「未处置」：点名的概念已全部进了已教账的缺口不再出行——否则一
    /// 条补上了的缺口会一直重放到它掉出读表窗口。判的是**整组**名字之并：组
    /// 里任一行点到的概念没教，整组照留；别的缺口各判各的。
    #[test]
    fn concept_gap_lines_leave_out_gaps_whose_concepts_have_all_been_taught() {
        // 输入按 seq DESC。seq 9 与 seq 3 是同一个缺口（都点名「接口」），而
        // seq 9 那行还点到「元素」。
        let rows = [
            concept_review(9, "", "「接口」「元素」在已教账中没出现，本批节点直接依赖它们。", ""),
            concept_review(5, "intent", "「死锁」在已教账中没出现，本批 intent 以它为内容依赖。", ""),
            concept_review(3, "", "「接口」在已教账中没出现，本批节点直接依赖它。", ""),
        ];
        let deadlock = "- 「死锁」在已教账中没出现，本批 intent 以它为内容依赖";
        let interface = "- 「接口」在已教账中没出现，本批节点直接依赖它";
        // 「死锁」已教 → 那一组丢；「接口」已教但「元素」没教 → 整组照留，出
        // 的仍是第一次被指出的 seq 3 那一行。
        let partly = concept_gap_lines(&rows, "接口@teach、死锁@teach");
        assert_eq!(
            partly.lines,
            vec![interface.to_owned()],
            "全教了的丢；教了一半的留"
        );
        assert_eq!(partly.resolved, 1, "读数：丢了一组");
        // 全教了：两组都丢。
        let all = concept_gap_lines(&rows, "接口@teach、元素@teach、死锁@teach");
        assert!(all.lines.is_empty());
        assert_eq!(all.resolved, 2);
        // 已教账为空：照常出行，零丢弃（接口组最近一次被指出是 seq 9，排在前）。
        let none = concept_gap_lines(&rows, "");
        assert_eq!(
            none.lines,
            vec![interface.to_owned(), deadlock.to_owned()],
            "没有已教概念：缺口照常出行"
        );
        assert_eq!(none.resolved, 0);
    }

    /// 缺席词后的枚举必须是清单：措辞同款、概念不同的两条缺口（都是「没在账
    /// 内出现」）不得被并掉——「没在账」后面紧跟的「内出现」是续词，不是概念名。
    #[test]
    fn concept_gap_lines_keep_gaps_that_share_only_their_phrasing_tail() {
        let garbage_collection = concept_review(
            102,
            "intent",
            "intent 用「垃圾回收家族」「追踪式 GC」定位本节点，作为上位概念的垃圾回收没在账内出现（「回收」本身已由已教 堆分配 覆盖）",
            "如果只是描述内存管理线的谱系位置，可保留为背景；如果要求学习者辨认垃圾回收家族，需先挂号",
        );
        let ownership = concept_review(
            103,
            "intent",
            "intent 用「所有权转移」定位本节点，作为上位概念的所有权转移没在账内出现",
            "如果要求学习者辨认所有权转移，需先挂号",
        );
        assert!(
            is_concept_gap(&garbage_collection) && is_concept_gap(&ownership),
            "夹具前提：两行都是缺口"
        );
        assert!(
            stem_similarity(garbage_collection.issue(), ownership.issue()) < STEM_SIMILARITY_THRESHOLD,
            "夹具前提：两条措辞不近形（相似度 {}）",
            stem_similarity(garbage_collection.issue(), ownership.issue())
        );
        assert_eq!(
            concept_gap_lines(&[ownership, garbage_collection], "").lines,
            vec![
                "- intent 用「所有权转移」定位本节点，作为上位概念的所有权转移没在账内出现（建议：如果要求学习者辨认所有权转移，需先挂号）",
                "- intent 用「垃圾回收家族」「追踪式 GC」定位本节点，作为上位概念的垃圾回收没在账内出现（「回收」本身已由已教 堆分配 覆盖）（建议：如果只是描述内存管理线的谱系位置，可保留为背景）",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
            "概念不同（所有权转移 ≠ 垃圾回收家族）：两条缺口各出一行"
        );
    }

    /// 缺席词后的枚举：成清单（顿号分隔）或以「等」收尾才算概念名；光秃秃的
    /// 一个段不算——那多半是续词或半句话。概念名引号内外归一后同名。
    #[test]
    fn concept_gap_lines_read_an_enumeration_only_when_it_is_a_list() {
        // 输入按 seq DESC：引号行 60 在前，枚举行 / 光杆行 59 在后。
        let quoted = concept_review(
            60,
            "intent",
            "「垃圾回收」在已教账中没出现，本批 intent 以它为内容依赖。",
            "",
        );
        let listed = concept_review(
            59,
            "",
            "已教概念账中未见垃圾回收、所有权转移等上位概念，本批节点直接依赖它们。",
            "",
        );
        let alone = concept_review(59, "", "已教概念账中未见垃圾回收，本批节点直接依赖它。", "");
        for other in [&listed, &alone] {
            assert!(
                stem_similarity(other.issue(), quoted.issue()) < STEM_SIMILARITY_THRESHOLD,
                "夹具前提：与引号行措辞不近形（相似度 {}）",
                stem_similarity(other.issue(), quoted.issue())
            );
        }
        let quoted_line = "- 「垃圾回收」在已教账中没出现，本批 intent 以它为内容依赖".to_owned();
        assert_eq!(
            concept_gap_lines(&[quoted.clone(), listed], "").lines,
            vec![
                "- 已教概念账中未见垃圾回收、所有权转移等上位概念，本批节点直接依赖它们".to_owned()
            ],
            "清单里点到「垃圾回收」：与引号行是同一个缺口，代表行取最低 seq（枚举行）"
        );
        assert_eq!(
            concept_gap_lines(&[quoted, alone], "").lines,
            vec![
                quoted_line,
                "- 已教概念账中未见垃圾回收，本批节点直接依赖它".to_owned(),
            ],
            "缺席词后光秃秃一个名字：不算概念名，两行各留"
        );
    }
}
