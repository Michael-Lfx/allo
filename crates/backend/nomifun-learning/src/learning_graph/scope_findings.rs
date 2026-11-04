//! 范围越界词面核对（absorb-8 z03，learnhub #205 裁决 Q1=b 域中立移植，
//! 报告-only 零拦截）：「教学范围」声明里「不教/不展开/不单独/留待/归后续」
//! 类边界表述后的名词串，机械提取后扫正文**节标题与正文文本**——命中即逐
//! 条一条 findings（指名来源字段「教学范围」与命中位置「节 N 标题/正文」），
//! 进 lesson_draft 审计观察面（warning 通道）；findings 只进修复轮报告与
//! 观测面，非阻断。「不该无痕地生成」由人判读 findings，不由机械打回。
//!
//! - **零 LLM 零新调用**：纯词面机械核对；深层语义越界不设机制面——换说法
//!   的语义越界归真机走查（learnhub Q1=b 裁决同律，「写作取向零机械门」
//!   不变量保持）。
//! - **范围外点名 ≠ 引用**：正文对范围外名词的**否定性提及**（「本课不教
//!   积分」类边界说明，小结/引入处诚实写边界）豁免不报——骨架行「教学范围」
//!   行尾注（范围外内容不教，越界点名不构成引用）是写作面配对条款。
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

/// 词串内并列分隔（顿号与常用连词切出名词串）。单字连词会把「参与式设计」
/// 类真词切碎——「宁滥勿缺」容忍面照 learnhub 点名清单口径：切词碎片由
/// findings 读者判读，零拦截下不设真词豁免词表（豁免表穷举不可能，与词表
/// 教义同源）。
const TERM_DELIMS: &[char] = &['、', '与', '和', '及', '或'];

/// 泛指停用词（无鉴别力的泛指词不入排除词串——命中面全是噪声）。
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

/// 扫一节（allo 分节契约，ADR-0002）：节标题与正文各为扫描面——排除词串
/// 出现且非否定性提及 → 逐条一条 findings。文案指名来源字段（教学范围）
/// 与命中位置（节 N 标题/正文）。空 scope = 零扫描（旧档零开销）。
pub fn scan_section(scope: &str, section_key: &str, title: &str, body: &str) -> Vec<String> {
    scan_section_with_terms(&excluded_terms(scope), section_key, title, body)
}

/// 同 [`scan_section`]，排除词串由调用方预计算传入——lesson_draft 审计
/// 循环逐节复用同一份词表（每课时一次 [`excluded_terms`]，不再每节重算）。
pub fn scan_section_with_terms(
    terms: &[String],
    section_key: &str,
    title: &str,
    body: &str,
) -> Vec<String> {
    let mut findings: Vec<String> = Vec::new();
    for term in terms {
        if has_unexempted_mention(title, term) {
            findings.push(format!(
                "范围声明排除「{term}」（来源：教学范围），但节 {section_key} 标题「{title}」出现该词"
            ));
        }
    }
    for line in body.lines() {
        for term in terms {
            if has_unexempted_mention(line, term) {
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
pub fn scan_body(scope: &str, body: &str) -> Vec<String> {
    let terms = excluded_terms(scope);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut findings: Vec<String> = Vec::new();
    let mut section = String::new();
    for line in body.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            section = header.to_string();
            for term in &terms {
                if has_unexempted_mention(line, term) {
                    findings.push(format!(
                        "范围声明排除「{term}」（来源：教学范围），但节标题「{section}」出现该词"
                    ));
                }
            }
            continue;
        }
        for term in &terms {
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
        let findings = scan_section("教四则运算；不引入方程", "s2", "概念：方程的初步认识", "正文。");
        assert_eq!(findings.len(), 1, "标题命中一条：{findings:?}");
        assert!(
            findings[0].contains("方程")
                && findings[0].contains("教学范围")
                && findings[0].contains("节 s2 标题")
                && findings[0].contains("概念：方程的初步认识"),
            "finding 指名来源字段（教学范围）与命中位置（节 N 标题）：{findings:?}"
        );
        // 正文行命中：位置报「节 N 正文」。
        let findings = scan_section(SCOPE, "s1", "概念：幂的记号", "顺带一提积分的几何意义。");
        assert_eq!(findings.len(), 1, "正文命中一条：{findings:?}");
        assert!(
            findings[0].contains("积分") && findings[0].contains("节 s1 正文"),
            "正文命中位置报节 N 正文：{findings:?}"
        );
    }

    /// learnhub 扫描面同款（整篇 `## ` 文档）：节标题与正文各产一条。
    #[test]
    fn body_scan_hits_section_title_and_body_each_once() {
        let body = "## 节1·引入·为什么要有幂\n记号第一课。\n\n## 节2·练习·对数初体验\n按规则推演，完整解答附后。\n";
        let findings = scan_body(SCOPE, body);
        assert_eq!(findings.len(), 1, "标题命中一条：{findings:?}");
        assert!(
            findings[0].contains("对数")
                && findings[0].contains("教学范围")
                && findings[0].contains("节标题")
                && findings[0].contains("节2·练习·对数初体验"),
            "finding 指名来源字段与命中位置：{findings:?}"
        );
        let body = "## 节1·讲解·幂的规则\n顺带一提积分的几何意义。\n";
        let findings = scan_body(SCOPE, body);
        assert_eq!(findings.len(), 1, "正文命中一条：{findings:?}");
        assert!(
            findings[0].contains("积分") && findings[0].contains("节「节1·讲解·幂的规则」"),
            "正文命中位置含所在节标题：{findings:?}"
        );
    }

    // —— 判据二：不命中（正文干净 / scope 无边界表述 / 空 scope 零扫描）

    #[test]
    fn clean_body_markerless_scope_and_empty_scope_produce_no_findings() {
        let body = "## 节1·引入·为什么要有幂\n记号第一课。\n\n## 节2·练习·判定底数\n按规则推演。\n";
        assert!(
            scan_body(SCOPE, body).is_empty(),
            "正文不撞排除词 = 零 findings"
        );
        // scope 无任何边界表述（正向声明教什么）= 零排除词，扫了也白扫。
        let positive = "教正整数幂的记号与基本运算，覆盖计算与化简";
        assert!(
            scan_body(positive, "## 节1·讲解·对数\n积分也讲。\n").is_empty(),
            "无边界表述 = 无排除词 = 零 findings（正向声明不构排除）"
        );
        // 空 scope（旧档 DB 默认空串）= 零扫描零 findings。
        assert!(
            scan_section("", "s1", "概念：方程", "讲方程。").is_empty(),
            "空 scope = 零扫描"
        );
        assert!(scan_body("", "## 节1·方程\n讲方程。\n").is_empty(), "空 scope = 零扫描");
    }

    // —— 判据三：否定词豁免（范围外点名 ≠ 引用）

    #[test]
    fn negated_mention_of_excluded_term_is_exempt() {
        // 正文只在否定子句里点名边界（小结写「不教积分」）= 诚实写边界，不报。
        let body = "## 节5·小结·要点回收\n回收三条规则；本课不教积分，后续课程再展开。\n";
        assert!(
            scan_body(SCOPE, body).is_empty(),
            "否定性提及 = 点名边界，豁免不报：{:?}",
            scan_body(SCOPE, body)
        );
        // 分节面同款豁免（小结节正文否定性提及）。
        assert!(
            scan_section(SCOPE, "s5", "小结：要点回收", "回收三条规则；本课不教积分，后续课程再展开。")
                .is_empty(),
            "分节扫描面豁免同形"
        );
        // 混合行：同一行既有否定点名又有正向展开 = 命中不豁免。
        let mixed = "## 节2·讲解·越界讲解\n本课不教积分，但下面我们完整推导积分的计算公式。\n";
        let findings = scan_body(SCOPE, mixed);
        assert_eq!(findings.len(), 1, "同行正向展开仍命中：{findings:?}");
    }

    // —— 提取面：切分/引词/停用词/长度边界

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
