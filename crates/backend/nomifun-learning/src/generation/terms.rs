//! 目标词条化 v1（t2）：确定性切片，无 LLM、无词典依赖、可重放。
//!
//! 目标文本是建课采样的主索引（以及 t3 课时检索的查询词源），而中文按空白
//! 切词不成立——整句会退化成一个永不命中的长词条。本模块把目标切成短词条：
//! 按标点/空白分段，中文段按**相邻二元**切片（整句子串命中蕴含其全部二元片段
//! 命中，故二元不丢召回），ASCII 词整词保留；再过滤单字与停用词、去重、限长。
//! 同一套词条同时喂给采样打分与（t3 的）检索查询，两处口径才不会漂移。

/// 词条上限：长目标截取前 [`TERM_MAX_COUNT`] 个词条（按出现顺序）。上限只
/// 服务于打分成本与查询长度——目标的开头承载意图，尾部细节进不了词条表。
pub(crate) const TERM_MAX_COUNT: usize = 32;

/// ASCII 词条的最短长度（单字母不成词条；中文段的单字在二元切片里天然消失）。
const MIN_LATIN_TERM_CHARS: usize = 2;

/// 目标停用词：功能词与目标文本的元词（「目标」「想要」……）。它们几乎必然
/// 出现在目标里、又几乎不能区分文档，进词条表只会给所有文档均匀加分。
const STOPWORDS: &[&str] = &[
    // 中文：代词/连词/助词/疑问词/量词
    "我们", "你们", "他们", "她们", "它们", "咱们", "自己", "这个", "那个", "这些", "那些",
    "哪个", "哪些", "什么", "如何", "怎么", "怎样", "以及", "或者", "并且", "因为", "所以",
    "但是", "如果", "然后", "另外", "此外", "其中", "就是", "还是", "一个", "一些", "可以",
    "能够", "应该", "必须", "通过", "关于", "对于", "进行", "有着", "这是", "那是", "首先",
    "其次", "最后", "是否", "任何", "每个", "各种", "其他", "别的",
    // 中文：目标文本的元词（意图标记，不带主题）
    "目标", "目的", "想要", "我想", "我要", "需要", "希望",
    // English function words
    "the", "and", "for", "with", "that", "this", "these", "those", "from", "about", "into",
    "your", "you", "their", "them", "they", "what", "which", "when", "where", "why", "how",
    "who", "does", "did", "can", "could", "should", "would", "will", "please", "want", "wants",
    "need", "needs", "help", "also", "then", "than",
];

/// 目标词条化（确定性、可重放）：标点/空白分段 → 中文段相邻二元切片 / ASCII
/// 词整词 → 去停用词与单字 → 去重 → 限长。无 LLM，无语言模型依赖。
pub(crate) fn term_slices(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    let mut terms: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for run in script_runs(&lowered) {
        for term in slice_run(&run) {
            if STOPWORDS.contains(&term.as_str()) {
                continue;
            }
            if !seen.insert(term.clone()) {
                continue;
            }
            terms.push(term);
            if terms.len() >= TERM_MAX_COUNT {
                return terms;
            }
        }
    }
    terms
}

/// 词条在文本里出现的次数（子串计数，大小写归一由调用方保证）。用于打分与
/// 相关段落定位——与检索侧 `score_md` 的正文词频同尺。
pub(crate) fn term_hits(text_lower: &str, term: &str) -> usize {
    if term.is_empty() {
        return 0;
    }
    text_lower.matches(term).count()
}

/// 按字形切段：非字母数字且非 CJK 的字符是边界（标点、空白、符号），
/// 同一段内再区分 CJK 与 ASCII 连续串，两类文本的切片规则不同。
fn script_runs(text: &str) -> Vec<String> {
    let mut runs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_cjk: Option<bool> = None;
    for ch in text.chars() {
        let cjk = is_cjk(ch);
        let word = ch.is_alphanumeric();
        if !word && !cjk {
            if !current.is_empty() {
                runs.push(std::mem::take(&mut current));
            }
            current_cjk = None;
            continue;
        }
        match current_cjk {
            Some(kind) if kind == cjk => current.push(ch),
            Some(_) => {
                runs.push(std::mem::take(&mut current));
                current_cjk = Some(cjk);
                current.push(ch);
            }
            None => {
                current_cjk = Some(cjk);
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

/// 单个字形段的词条切片：CJK 段取全部相邻二元（长度 n 的段产出 n-1 个二元，
/// 单字段无产出）；ASCII 段整词保留（至少 [`MIN_LATIN_TERM_CHARS`] 个字符）。
fn slice_run(run: &str) -> Vec<String> {
    let chars: Vec<char> = run.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    if is_cjk(chars[0]) {
        if chars.len() < 2 {
            return Vec::new();
        }
        return chars
            .windows(2)
            .map(|pair| pair.iter().collect::<String>())
            .collect();
    }
    if chars.len() < MIN_LATIN_TERM_CHARS {
        return Vec::new();
    }
    vec![run.to_owned()]
}

/// CJK 统一表意文字（含扩展 A）。判据只在切片规则上用，不做语言判定。
///
/// 与检索侧 `nomifun-knowledge/src/service.rs::is_cjk`（`query_terms` 的二元
/// 切片）是**有意保留的两份**（评审记录）：同一个三行判据的两个私有实现，
/// 范围逐字相同（0x3400–0x4DBF / 0x4E00–0x9FFF / 0xF900–0xFAFF），检索接缝
/// 的两端因此对"哪里没有词边界"看法一致。不提取共享函数——knowledge 侧不
/// 为它扩公共面（它只是查询词探测的私有细节），跨层依赖的代价大于三行重复；
/// **改任一侧必须同步另一侧**（knowledge 侧的同名注释已互指到这里）。
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_a_typical_goal_into_short_terms() {
        // 中文段按相邻二元切片；ASCII 词整词保留（小写化）。
        assert_eq!(
            term_slices("零基础学 Python 数据分析"),
            vec!["零基", "基础", "础学", "python", "数据", "据分", "分析"]
        );
    }

    #[test]
    fn punctuation_and_whitespace_separate_segments() {
        // 标点/空白是分段边界：不产生词条，也不把两侧黏成一个长词条；
        // 「目标」是目标文本的元词（停用词），不进词条表。
        assert_eq!(
            term_slices("目标：学会用 SQL，做用户留存分析！"),
            vec!["学会", "会用", "sql", "做用", "用户", "户留", "留存", "存分", "分析"]
        );
    }

    #[test]
    fn stopwords_and_single_chars_are_dropped() {
        let terms = term_slices("我想学习 Python，学 Python 的数据分析");
        assert!(!terms.contains(&"学".to_owned()), "{terms:?}");
        assert!(!terms.contains(&"我想".to_owned()), "{terms:?}");
        assert_eq!(
            terms.iter().filter(|term| *term == "python").count(),
            1,
            "duplicate terms collapse: {terms:?}"
        );
    }

    #[test]
    fn terms_are_capped_and_ascii_is_lowercased() {
        let goal = (0..40)
            .map(|index| format!("主题{index:02}"))
            .collect::<Vec<_>>()
            .join("，");
        assert_eq!(term_slices(&goal).len(), TERM_MAX_COUNT);
        assert_eq!(term_slices("PYTHON 数据分析"), vec!["python", "数据", "据分", "分析"]);
    }

    #[test]
    fn empty_and_symbol_only_goals_yield_no_terms() {
        assert!(term_slices("").is_empty());
        assert!(term_slices("   \n\t  ").is_empty());
        assert!(term_slices("？！、。").is_empty());
    }
}
