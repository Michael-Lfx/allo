//! 范围越界词面核对（absorb-8 z03，learnhub #205 裁决 Q1=b 域中立移植；
//! sq2 双面收口升级）：「教学范围」声明里「不教/不展开/不单独/留待/归后续」
//! 类边界表述后的名词串，机械提取后按**双粒度**（词干集/整词集，见
//! [`ExcludedTerms`]）扫三个面：
//!
//! - **正文面**（节标题与正文文本）：命中 = `scope_boundary_hit`，sq2 起
//!   升 **danger**（未豁免的功能性提及正是超纲题的判死弹药——票面倾向
//!   落地；否定性提及豁免保留，合法边界句不受伤）。
//! - **题面面**（题干 + 全部选项，[`activity_boundary_hits`]）：命中 =
//!   `activity_scope_boundary_hit` **danger**——scope 声明点名的概念不得
//!   成为任何题的考点轴（「正文提及 ≠ 可考」）；题面**零豁免**（R-c：
//!   题面无「写边界」的合法场景）。
//! - **改写面重扫**（[`rescan_body_messages`]）：落库后对被改节重跑一次
//!   （零 LLM），命中 → warning + tracing 留痕，不阻断用户操作。
//!
//! 另带正文半禁令 1 的跨节点承诺词表（[`cross_node_promise_hits`]，warning
//! 报告面）。「不该无痕地生成」由修复轮与人判读 findings 兜底。
//!
//! - **零 LLM 零新调用**：纯词面机械核对；深层语义越界不设机制面——换说法
//!   的语义越界归真机走查（learnhub Q1=b 裁决同律 + sq2 R-d 同律）。
//! - **范围外点名 ≠ 引用**（正文面）：正文对范围外名词的**否定性提及**
//!   （「本课不教积分」类边界说明，小结/引入处诚实写边界）豁免不报——
//!   骨架行「教学范围」行尾注（范围外内容不教，越界点名不构成引用）是
//!   写作面配对条款。题面面不适用（R-c 收窄）。
//! - **匹配粒度适用界（sq2 R-b）**：整词子串拦不住同根变体（「回收细节」
//!   拦不住「回收路径/回收机制」）；词干匹配须有适用界——**头名词域整体
//!   未立名**（已知概念名单零命中，如「回收」）才词干匹配；头名词已立名/
//!   已教（如「引用」合法使用、只排除「语法」形态）退回整词匹配，防
//!   「引用语法→引用」误伤正文合法句「返回指向该对象的引用」。名单由
//!   调用方喂（audit 喂 graph.known_names，改写面喂 course_known_names）。
//! - **旧档零扫描**：scope 缺席/空白（DB 默认空串）= 无排除面，零扫描零
//!   findings（调用方折叠，本模块对空 scope 恒返回空表）。

/// 范围声明的边界表述词表（票面口径「不教/不展开/不单独/留待/归后续」类；
/// 域中立，无单一领域示例）。提取面：词表命中处之后至子句界为排除段。
const BOUNDARY_MARKERS: &[&str] = &[
    "不教",
    "不展开",
    "不单独",
    "不讲",
    "不引入",
    "不涉及",
    "不碰",
    "不深究",
    "留待",
    "归后续",
    "留给后续",
    "后续学习点",
];

/// 正文提及的否定性豁免词表（范围外点名 ≠ 引用）= 边界表述词表全量 +
/// 正文侧补充措辞（「超出/不在/不属于」类，小结与引入处写边界的常见句式）。
/// 词表单源派生自 [`BOUNDARY_MARKERS`]（learnhub code-review S2 同款：加词
/// 单点改）。
const MENTION_NEGATIONS_EXTRA: &[&str] = &[
    "超出本课",
    "超出本点",
    "超出范围",
    "不在本课",
    "不在本点",
    "不在范围",
    "不属于本课",
    "不属于本点",
    "不属本课",
    "不属本点",
];

/// 否定性豁免判定用全量词表（迭代器，零分配）。
fn mention_negations() -> impl Iterator<Item = &'static str> {
    BOUNDARY_MARKERS
        .iter()
        .copied()
        .chain(MENTION_NEGATIONS_EXTRA.iter().copied())
}

/// 子句界（排除段截取与豁免子句判定共用；learnhub 词表移植补全角形——
/// 源文件仅剩半角逗/叹/问号的重复条目（全角形丢失的痕迹），allo 正文与
/// scope 是全角中文标点，两种形都收、语义为集合）。
const CLAUSE_DELIMS: &[char] = &['。', '；', ';', '，', ',', '！', '!', '？', '?', '\n'];

/// 词串内并列分隔（顿号、斜杠与常用连词切出名词串——sq2 补「/」：真实
/// scope 原文「不教指针/引用语法」用斜杠并列，不切则整串成词永不命中）。
/// 单字连词会把「参与式设计」类真词切碎——「宁滥勿缺」容忍面照 learnhub
/// 点名清单口径：切词碎片由 findings 读者判读，零拦截下不设真词豁免词表
///（豁免表穷举不可能，与词表教义同源）。
const TERM_DELIMS: &[char] = &['、', '与', '和', '及', '或', '/'];

/// 泛指停用词（无鉴别力的泛指词不入排除词串——命中面全是噪声）。「后续」
/// sq2 补入：真实 scope「留待后续」的排除段只剩「后续」二字，是时间泛指
/// 不是领域名词；sq2 升 danger 后它会把「后续课程再展开」类合法边界句
/// 打成阻断项，必须拦在提取层。
const STOPWORDS: &[&str] = &[
    "内容",
    "知识点",
    "知识",
    "概念",
    "方法",
    "问题",
    "部分",
    "主题",
    "话题",
    "细节",
    "东西",
    "内容点",
    "后续",
];

/// 排除词串长度上限（learnhub v1.18 片段口径同源；超长 = 整句散文不是名词
/// 串，整段跳过不截断——截断会造出假词）。
const TERM_MAX_CHARS: usize = 40;

/// 从范围声明文本机械提取排除词串（去重排序稳定输出）。
///
/// 算法：每个边界表述命中处 → 其后至下一子句界为排除段 → 段内按并列分隔
/// 切名词串 → 剥引词（「例如/包括」类）与「等/之类」收尾 → 过滤（≥2 字、
/// ≤40 字、非停用词、不含边界表述）。零 LLM，纯词面。
pub fn excluded_terms(scope: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for marker in BOUNDARY_MARKERS {
        let mut from = 0usize;
        while let Some(pos) = scope[from..].find(marker) {
            let seg_start = from + pos + marker.len();
            let seg_end = scope[seg_start..]
                .find(|c| CLAUSE_DELIMS.contains(&c))
                .map_or(scope.len(), |p| seg_start + p);
            terms.extend(candidate_terms(&scope[seg_start..seg_end]));
            from = seg_end.max(seg_start);
            if from >= scope.len() {
                break;
            }
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

/// 复合排除词的泛指头名词后缀（「回收细节/回收方式」类的尾半——剥掉后
/// 剩下的前半是词干）。保守清单：每收一个后缀，词干匹配面就宽一档，宁窄
/// 勿滥；剥后余 ≥2 字才算剥出（剥剩单字不立词）。
const GENERIC_HEAD_SUFFIXES: &[&str] = &[
    "细节", "方式", "方法", "机制", "路径", "语法", "技巧", "流程", "原理", "规则", "过程",
    "手段", "体系", "形态",
];

/// sq2（R-b）双粒度排除词表：**词干集**（复合词剥泛指头名词后缀得的词干，
/// 头名词域在已知概念名单零命中——词干匹配拦「回收路径/回收方式」类同根
/// 变体）与**整词集**（头名词已立名/已教、或无泛指头名词可剥——整词匹配，
/// 防「引用语法→引用」误伤合法句）。两类都是「待匹配串」，正文面共用同一
/// 否定性豁免判定；题面面共用零豁免判定。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExcludedTerms {
    /// 词干集：头名词域未立名的复合词剥出的词干。
    pub stems: Vec<String>,
    /// 整词集：头名词已立名的复合词原形 + 无泛指头名词可剥的裸词。
    pub whole: Vec<String>,
}

impl ExcludedTerms {
    /// 全部待匹配串（词干在前整词在后，去重稳定）。
    fn all_terms(&self) -> Vec<String> {
        let mut all = self.stems.clone();
        all.extend(self.whole.iter().cloned());
        all.sort();
        all.dedup();
        all
    }

    /// 全空 = 零排除面（空 scope / 无边界表述），调用方可短路零扫描。
    pub fn is_empty(&self) -> bool {
        self.stems.is_empty() && self.whole.is_empty()
    }
}

/// 双粒度提取：[`excluded_terms`] 原文提取后逐词分类——剥得出泛指头名词
/// 且词干在名单零命中 → 词干集；其余（头名词已立名，含包含命中如
/// 「垃圾回收」含「回收」；或剥无可剥）→ 整词集。名单由调用方喂（audit
/// 喂 graph.known_names，改写面喂 course_known_names）；空名单 = 词干面
/// 激进（名单零命中即词干匹配，票面适用界原文）。
pub fn excluded_terms_dual(scope: &str, known_names: &[String]) -> ExcludedTerms {
    let mut dual = ExcludedTerms::default();
    for term in excluded_terms(scope) {
        match head_stem(&term) {
            Some(stem) if !named_domain(&stem, known_names) => dual.stems.push(stem),
            _ => dual.whole.push(term),
        }
    }
    dual.stems.sort();
    dual.stems.dedup();
    dual.whole.sort();
    dual.whole.dedup();
    dual
}

/// 复合词剥泛指头名词后缀得词干（剥到稳定）；剥无可剥或剥剩 <2 字返回
/// `None`（原词整词面保留）。
fn head_stem(term: &str) -> Option<String> {
    let mut current = term;
    while let Some(suffix) = GENERIC_HEAD_SUFFIXES
        .iter()
        .find(|suffix| current.ends_with(*suffix))
    {
        let stem = &current[..current.len() - suffix.len()];
        if stem.chars().count() < 2 {
            return None;
        }
        current = stem;
    }
    (current != term).then(|| current.to_owned())
}

/// 词干是否已立名：任一已知概念名**包含**该词干（「垃圾回收」含「回收」
/// → 同域已立名，词面粒度退回整词防误伤；票面取证口径 LIKE '%回收%'
/// 同形）。空名单恒 `false`（零命中 → 词干面）。
fn named_domain(stem: &str, known_names: &[String]) -> bool {
    known_names.iter().any(|name| name.contains(stem))
}

/// sq2（R-b/R-c）题面扫描：题干 + 全部选项，**零豁免**（R-c 题面收窄：
/// 题面无「写边界」的合法场景——否定性提及在题面同样把排除概念立成判定
/// 轴）。命中串带粒度标注（「回收（词干）」）供 finding 点名。explanation
/// 不入扫面（裁决落地：判底揭示面允许点名边界排除理由）。reflection/
/// open_question 题干同扫（契约口径「不得成为**任何**题的考点轴」）。
pub fn activity_boundary_hits(
    terms: &ExcludedTerms,
    prompt: &str,
    options: &[&str],
) -> Vec<String> {
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<String> = Vec::new();
    for term in &terms.stems {
        if prompt.contains(term.as_str()) || options.iter().any(|option| option.contains(term)) {
            hits.push(format!("{term}（词干）"));
        }
    }
    for term in &terms.whole {
        if prompt.contains(term.as_str()) || options.iter().any(|option| option.contains(term)) {
            hits.push(term.clone());
        }
    }
    hits
}

/// sq2（R-a 正文半禁令 1）跨节点承诺词表：正文预告**其它节点/课时**教什么
/// /何时教的指点形态——后续节点由教练独立生成，承诺不可验证。同节点内的
/// 下一节预告合法（「下一节」不在表：「下一节」与「下一节点」是子串包含
/// 关系的反向——匹配「下一节点」不伤「下一节」）；模糊惯语「后续课程/
/// 留待后续」类边界收尾不收（否定性豁免条款的保留面）。换说法的语义越界
/// 不设机制面（R-d 归真机走查）。
pub const CROSS_NODE_PROMISE_TERMS: &[&str] = &[
    "下一节点",
    "下个节点",
    "后续节点",
    "之后的节点",
    "接下来的节点",
    "下一课",
    "下一章",
    "后续课时",
    "后面会讲",
    "后面再讲",
    "后面再谈",
    "后面细讲",
    "以后会讲",
    "以后再讲",
];

/// 一行正文里的跨节点承诺命中词（保序；零豁免——承诺措辞即违规形态，
/// 否定性边界说明「本节点不展开 X」不含这些指点词）。
pub fn cross_node_promise_hits(line: &str) -> Vec<&'static str> {
    CROSS_NODE_PROMISE_TERMS
        .iter()
        .copied()
        .filter(|term| line.contains(term))
        .collect()
}

/// sq2（R-b 改写面）落库重扫入口：scope 排除词（双粒度，正文面豁免保留）
/// + 跨节点承诺词表，一次扫被改节正文。命中串为 tracing 留痕文案（调用方
/// warning + tracing，不阻断）。空 scope = scope 词面零扫描（承诺词表不依
/// 赖 scope，照扫——调用方 service 侧对无声明面提前返回，见
/// `rescan_section_boundary`）。
pub fn rescan_body_messages(scope: &str, known_names: &[String], body: &str) -> Vec<String> {
    let mut messages = Vec::new();
    let terms = excluded_terms_dual(scope, known_names);
    if !terms.is_empty() {
        for term in terms.all_terms() {
            for line in body.lines() {
                if has_unexempted_mention(line, &term) {
                    messages.push(format!("教学范围排除词「{term}」"));
                }
            }
        }
    }
    for line in body.lines() {
        for term in cross_node_promise_hits(line) {
            messages.push(format!("跨节点承诺措辞「{term}」"));
        }
    }
    messages.sort();
    messages.dedup();
    messages
}

/// 扫一节（allo 分节契约，ADR-0002）：节标题与正文各为扫描面——排除词串
/// 出现且非否定性提及 → 逐条一条 findings。文案指名来源字段（教学范围）
/// 与命中位置（节 N 标题/正文）。空 scope = 零扫描（旧档零开销）。
pub fn scan_section(
    scope: &str,
    known_names: &[String],
    section_key: &str,
    title: &str,
    body: &str,
) -> Vec<String> {
    scan_section_with_terms(&excluded_terms_dual(scope, known_names), section_key, title, body)
}

/// 同 [`scan_section`]，双粒度词表由调用方预计算传入——lesson_draft 审计
/// 循环逐节复用同一份词表（每课时一次 [`excluded_terms_dual`]，不再每节
/// 重算）。
pub fn scan_section_with_terms(
    terms: &ExcludedTerms,
    section_key: &str,
    title: &str,
    body: &str,
) -> Vec<String> {
    let mut findings: Vec<String> = Vec::new();
    if terms.is_empty() {
        return findings;
    }
    for term in terms.all_terms() {
        if has_unexempted_mention(title, &term) {
            findings.push(format!(
                "范围声明排除「{term}」（来源：教学范围），但节 {section_key} 标题「{title}」出现该词"
            ));
        }
    }
    for line in body.lines() {
        for term in terms.all_terms() {
            if has_unexempted_mention(line, &term) {
                findings.push(format!(
                    "范围声明排除「{term}」（来源：教学范围），但节 {section_key} 正文出现该词"
                ));
            }
        }
    }
    findings.sort();
    findings.dedup();
    findings
}

/// 扫整篇正文（旧单篇文档契约，learnhub `scan_body` 同款）：`## ` 行为节
/// 标题扫描面，其余为正文；命中位置报「节标题」或「文首/节「标题」正文」。
/// sq2 起收双粒度词表（[`excluded_terms_dual`] 预计算，正文面豁免保留）。
pub fn scan_body(terms: &ExcludedTerms, body: &str) -> Vec<String> {
    if terms.is_empty() {
        return Vec::new();
    }
    let all_terms = terms.all_terms();
    let mut findings: Vec<String> = Vec::new();
    let mut section = String::new();
    for line in body.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            section = header.to_string();
            for term in &all_terms {
                if has_unexempted_mention(line, term) {
                    findings.push(format!(
                        "范围声明排除「{term}」（来源：教学范围），但节标题「{section}」出现该词"
                    ));
                }
            }
            continue;
        }
        for term in &all_terms {
            if has_unexempted_mention(line, term) {
                let where_ = if section.is_empty() {
                    "文首".to_string()
                } else {
                    format!("节「{section}」")
                };
                findings.push(format!(
                    "范围声明排除「{term}」（来源：教学范围），但{where_}正文出现该词"
                ));
            }
        }
    }
    findings.sort();
    findings.dedup();
    findings
}

/// 一行文本里 term 的出现是否至少有一处不在否定性子句中（命中判定：
/// 任一非豁免出现 = 命中；全部出现都落在否定子句 = 点名边界，豁免）。
fn has_unexempted_mention(line: &str, term: &str) -> bool {
    let mut from = 0usize;
    while let Some(pos) = line[from..].find(term) {
        let hit = from + pos;
        let clause_start = line[..hit]
            .char_indices()
            .rev()
            .find(|(_, c)| CLAUSE_DELIMS.contains(c))
            .map(|(i, _)| i + char_len_at(line, i))
            .unwrap_or(0);
        let clause_end = line[hit..]
            .find(|c| CLAUSE_DELIMS.contains(&c))
            .map_or(line.len(), |p| hit + p);
        let clause = &line[clause_start..clause_end];
        if !mention_negations().any(|m| clause.contains(m)) {
            return true;
        }
        from = hit + term.len();
    }
    false
}

/// `char_indices` 闭包内的字符字节长（行内按字节回退定位用）。
fn char_len_at(line: &str, byte_at: usize) -> usize {
    line[byte_at..].chars().next().map_or(1, |c| c.len_utf8())
}

/// 排除段 → 候选词串（切分 + 剥引词 + 过滤）。
fn candidate_terms(segment: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in segment.split(|c| TERM_DELIMS.contains(&c)) {
        let mut run = raw.trim();
        // 引词剥离（「例如/比如/包括/诸如/如/像」类，多轮剥到稳定）
        loop {
            let stripped = ["例如", "诸如", "比如", "包括", "如", "像"]
                .iter()
                .find_map(|lead| run.strip_prefix(lead));
            match stripped {
                Some(rest) => run = rest.trim_start(),
                None => break,
            }
        }
        // 「等/之类」收尾截断（仅当其后还有 ≥2 字的描述时才截——「等级」
        // 一类含「等」的真词不受伤）
        if let Some(p) = run.find("等") {
            let suffix_len = run[p + "等".len()..].chars().count();
            if suffix_len >= 2 {
                run = run[..p].trim_end();
            }
        }
        if let Some(p) = run.find("之类") {
            let suffix_len = run[p + "之类".len()..].chars().count();
            if suffix_len >= 2 {
                run = run[..p].trim_end();
            }
        }
        let chars = run.chars().count();
        if !(2..=TERM_MAX_CHARS).contains(&chars)
            || STOPWORDS.contains(&run)
            || BOUNDARY_MARKERS.iter().any(|m| run.contains(m))
        {
            continue;
        }
        out.push(run.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 票面样例 scope（播种负样本）：边界表述「不教」+「留待后续学习点」，
    /// 排除段含并列名词串（与/、切分）。
    const SCOPE: &str = "教正整数幂的记号与基本运算；不教对数、积分与无穷级数，留待后续学习点";

    // —— 判据一：命中（排除词出现在节标题/正文 → findings 指名来源与位置）

    /// 播种负样本（票面场景）：scope「不引入方程」+ 节标题含「方程」→
    /// 命中 1 条，指名来源字段（教学范围）与命中位置（节 N 标题）。
    #[test]
    fn seeded_scope_hit_in_section_title_produces_one_finding() {
        let findings = scan_section("教四则运算；不引入方程", &[], "s2", "概念：方程的初步认识", "正文。");
        assert_eq!(findings.len(), 1, "标题命中一条：{findings:?}");
        assert!(
            findings[0].contains("方程")
                && findings[0].contains("教学范围")
                && findings[0].contains("节 s2 标题")
                && findings[0].contains("概念：方程的初步认识"),
            "finding 指名来源字段（教学范围）与命中位置（节 N 标题）：{findings:?}"
        );
        // 正文行命中：位置报「节 N 正文」。
        let findings = scan_section(SCOPE, &[], "s1", "概念：幂的记号", "顺带一提积分的几何意义。");
        assert_eq!(findings.len(), 1, "正文命中一条：{findings:?}");
        assert!(
            findings[0].contains("积分") && findings[0].contains("节 s1 正文"),
            "正文命中位置报节 N 正文：{findings:?}"
        );
    }

    /// learnhub 扫描面同款（整篇 `## ` 文档）：节标题与正文各产一条。
    #[test]
    fn body_scan_hits_section_title_and_body_each_once() {
        let terms = excluded_terms_dual(SCOPE, &[]);
        let body = "## 节1·引入·为什么要有幂\n记号第一课。\n\n## 节2·练习·对数初体验\n按规则推演，完整解答附后。\n";
        let findings = scan_body(&terms, body);
        assert_eq!(findings.len(), 1, "标题命中一条：{findings:?}");
        assert!(
            findings[0].contains("对数")
                && findings[0].contains("教学范围")
                && findings[0].contains("节标题")
                && findings[0].contains("节2·练习·对数初体验"),
            "finding 指名来源字段与命中位置：{findings:?}"
        );
        let body = "## 节1·讲解·幂的规则\n顺带一提积分的几何意义。\n";
        let findings = scan_body(&terms, body);
        assert_eq!(findings.len(), 1, "正文命中一条：{findings:?}");
        assert!(
            findings[0].contains("积分") && findings[0].contains("节「节1·讲解·幂的规则」"),
            "正文命中位置含所在节标题：{findings:?}"
        );
    }

    // —— 判据二：不命中（正文干净 / scope 无边界表述 / 空 scope 零扫描）

    #[test]
    fn clean_body_markerless_scope_and_empty_scope_produce_no_findings() {
        let terms = excluded_terms_dual(SCOPE, &[]);
        let body = "## 节1·引入·为什么要有幂\n记号第一课。\n\n## 节2·练习·判定底数\n按规则推演。\n";
        assert!(
            scan_body(&terms, body).is_empty(),
            "正文不撞排除词 = 零 findings"
        );
        // scope 无任何边界表述（正向声明教什么）= 零排除词，扫了也白扫。
        let positive = excluded_terms_dual("教正整数幂的记号与基本运算，覆盖计算与化简", &[]);
        assert!(
            scan_body(&positive, "## 节1·讲解·对数\n积分也讲。\n").is_empty(),
            "无边界表述 = 无排除词 = 零 findings（正向声明不构排除）"
        );
        // 空 scope（旧档 DB 默认空串）= 零扫描零 findings。
        assert!(
            scan_section("", &[], "s1", "概念：方程", "讲方程。").is_empty(),
            "空 scope = 零扫描"
        );
        assert!(
            scan_body(&excluded_terms_dual("", &[]), "## 节1·方程\n讲方程。\n").is_empty(),
            "空 scope = 零扫描"
        );
    }

    // —— 判据三：否定词豁免（范围外点名 ≠ 引用）

    #[test]
    fn negated_mention_of_excluded_term_is_exempt() {
        // 正文只在否定子句里点名边界（小结写「不教积分」）= 诚实写边界，不报。
        let terms = excluded_terms_dual(SCOPE, &[]);
        let body = "## 节5·小结·要点回收\n回收三条规则；本课不教积分，后续课程再展开。\n";
        assert!(
            scan_body(&terms, body).is_empty(),
            "否定性提及 = 点名边界，豁免不报：{:?}",
            scan_body(&terms, body)
        );
        // 分节面同款豁免（小结节正文否定性提及）。
        assert!(
            scan_section(
                SCOPE,
                &[],
                "s5",
                "小结：要点回收",
                "回收三条规则；本课不教积分，后续课程再展开。"
            )
            .is_empty(),
            "分节扫描面豁免同形"
        );
        // 混合行：同一行既有否定点名又有正向展开 = 命中不豁免。
        let mixed = "## 节2·讲解·越界讲解\n本课不教积分，但下面我们完整推导积分的计算公式。\n";
        let findings = scan_body(&terms, mixed);
        assert_eq!(findings.len(), 1, "同行正向展开仍命中：{findings:?}");
    }

    // —— sq2（R-b）双粒度提取：词干集/整词集分类（头名词域适用界）

    /// sq2 验收底线 scope（真实声明原文片段，map.md 取证 2026-11-17
    /// nomicore.log）：「不教指针/引用语法，不教回收细节，留待后续」。
    const REAL_SCOPE: &str = "教栈上临时量与堆上长存数据的分界；不教指针/引用语法，不教回收细节，留待后续";

    /// sq2 验收底线已知概念名单（本课合法使用面：「引用」在 assumes 链内
    /// 已立名——票面实证；「回收」在概念注册表零命中——map.md 取证）。
    fn real_known_names() -> Vec<String> {
        ["引用", "局部变量", "栈区", "堆区", "返回值"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    /// sq2 验收底线三件（票面 Acceptance 逐字）：真实 scope 原文复扫——
    /// ①命中题面「回收路径」（词干拦同根变体）、②命中正文 mermaid Note
    /// 「回收机制」、③**不命中**正文「返回指向该对象的引用」（头名词
    /// 「引用」已立名 → 退回整词，防词干误伤合法句）。
    #[test]
    fn real_scope_dual_granularity_hits_the_three_acceptance_cases() {
        let dual = excluded_terms_dual(REAL_SCOPE, &real_known_names());
        // 词表形态：词干集收「回收」（头名词域注册表零命中）；整词集收
        // 「引用语法」（头名词已立名，只拦「语法」形态）与「指针」。
        assert!(
            dual.stems.iter().any(|t| t == "回收"),
            "「回收细节」剥泛指头名词得词干「回收」（名单零命中 → 词干面）：{:?}",
            dual
        );
        assert!(
            dual.whole.iter().any(|t| t == "引用语法"),
            "「引用语法」头名词「引用」已立名 → 退回整词：{:?}",
            dual
        );
        // ①题面「回收路径」命中（裸 contains，零豁免——R-c 题面收窄）。
        assert!(
            activity_boundary_hits(&dual, "下列哪种写法的回收路径与持有者判断正确？", &[])
                .iter()
                .any(|hit| hit.contains("回收")),
            "题面「回收路径」被词干「回收」命中：{:?}",
            activity_boundary_hits(&dual, "下列哪种写法的回收路径与持有者判断正确？", &[])
        );
        // ②正文 mermaid Note「回收机制」命中（正文面豁免保留：该子句无
        // 否定性表述，不豁免）。
        let findings = scan_body(
            &dual,
            "## 演示\n```mermaid\nflowchart TD\nA[\"注: 这一块归回收机制处理\"]\n```\n",
        );
        assert!(
            findings.iter().any(|m| m.contains("回收")),
            "正文 mermaid Note「回收机制」被命中：{findings:?}"
        );
        // ③正文合法句「返回指向该对象的引用」零命中（整词「引用语法」不
        // 在场、词干面无「引用」）。
        let findings = scan_body(&dual, "## 讲解\n函数返回指向该对象的引用，调用方据此持有它。\n");
        assert!(
            findings.is_empty(),
            "合法句「返回指向该对象的引用」不得被词干误伤：{findings:?}"
        );
    }

    /// 适用界反向臂：头名词在名单内（含包含命中——「垃圾回收」含「回收」
    /// 同域已立名）→ 词干不立，整词面拦。
    #[test]
    fn named_head_domain_falls_back_to_whole_word() {
        let known = vec!["垃圾回收".to_owned()];
        let dual = excluded_terms_dual("不教回收细节", &known);
        assert!(
            dual.stems.is_empty() && dual.whole.iter().any(|t| t == "回收细节"),
            "「回收」被「垃圾回收」包含命中 → 整词面：{:?}",
            dual
        );
        assert!(
            activity_boundary_hits(&dual, "谈谈回收路径与回收机制。", &[]).is_empty(),
            "整词「回收细节」拦不住「回收路径」——已立名域的窄面是适用界代价"
        );
    }

    /// 空 scope / 无边界表述 scope = 零提取零扫描（旧档零开销不变）。
    #[test]
    fn dual_extraction_on_empty_and_markerless_scope_is_empty() {
        assert!(excluded_terms_dual("", &real_known_names()).is_empty(), "空 scope");
        assert!(
            excluded_terms_dual("教栈与堆的分界", &real_known_names()).is_empty(),
            "无边界表述 = 无排除词"
        );
    }

    /// sq2（R-b 改写面）落库重扫入口：scope 排除词（词干粒度）与跨节点
    /// 承诺措辞一次扫出；否定性边界句豁免保留；空 scope 零扫描。
    #[test]
    fn rescan_body_messages_combines_scope_terms_and_promise_terms() {
        // 命中臂：词干「回收」拦正文变体 + 承诺措辞，两条齐发。
        let hits = rescan_body_messages(
            REAL_SCOPE,
            &real_known_names(),
            "这一块归回收机制处理，下一节点起再谈。\n",
        );
        assert!(
            hits.iter().any(|m| m.contains("回收")) && hits.iter().any(|m| m.contains("下一节点")),
            "scope 词与承诺词一次扫出：{hits:?}"
        );
        // 豁免臂：否定性边界句零命中（改写面与生成面同一豁免条款）。
        let hits = rescan_body_messages(
            REAL_SCOPE,
            &real_known_names(),
            "本节点不展开回收细节，也不预告后续安排。\n",
        );
        assert!(hits.is_empty(), "否定性边界句豁免：{hits:?}");
        // 空 scope：scope 词面零扫描，承诺词表照扫（不依赖 scope 声明面）。
        let hits = rescan_body_messages("", &[], "这一块归回收机制处理，下一节点起再谈。\n");
        assert!(
            !hits.iter().any(|m| m.contains("回收")) && hits.iter().any(|m| m.contains("下一节点")),
            "空 scope = scope 词面零扫描、承诺词面照扫：{hits:?}"
        );
    }

    #[test]
    fn extraction_splits_conjunctions_and_filters_noise() {
        let terms = excluded_terms(SCOPE);
        assert_eq!(
            terms,
            vec!["对数".to_string(), "无穷级数".to_string(), "积分".to_string()],
            "「不教」段按 、与 切分；「留待后续学习点」无后续名词不产词"
        );
        // 引词与「等」收尾：剥离后得真词。
        let terms = excluded_terms("不教例如微积分等内容；不涉及矩阵等进阶工具");
        assert!(
            terms.iter().any(|t| t == "微积分") && terms.iter().any(|t| t == "矩阵"),
            "「例如」剥引词、「等进阶工具」收尾截断：{terms:?}"
        );
        // <2 字、停用词、含边界表述的段不入词。
        let terms = excluded_terms("不讲力与热；不涉及相关知识；不教留待后续再说的内容");
        assert!(
            terms.iter().all(|t| t != "力" && t != "知识" && t != "留待后续"),
            "短词/停用词/边界自指过滤：{terms:?}"
        );
    }
}
