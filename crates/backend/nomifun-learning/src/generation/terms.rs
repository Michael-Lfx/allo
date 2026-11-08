//! 目标词条化 v1（t2）：确定性切片，无 LLM、无词典依赖、可重放。
//!
//! 目标文本是建课采样的主索引（以及 t3 课时检索的查询词源），而中文按空白
//! 切词不成立——整句会退化成一个永不命中的长词条。本模块把目标切成短词条：
//! 按标点/空白分段，中文段按**相邻二元**切片（整句子串命中蕴含其全部二元片段
//! 命中，故二元不丢召回），ASCII 词整词保留；再过滤单字与停用词、去重、限长。
//! 同一套词条同时喂给采样打分与（t3 的）检索查询，两处口径才不会漂移。
//! 另有语境词专用切片 [`context_word_slices`]（w02 最大 munch）——u04 课程
//! 语境词的装配源，只要「像词」不要全量二元，检索词条不消费它。

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

/// 语境词短语边界表（w02 最大 munch 的切分点，私有）：无歧义虚字单字——
/// 在标题/目标文本里几乎必为句法边界。是 definition 侧
/// `definition_audit::FUNCTION_CHARS` 的**从严子集**：会/用/表/取 等歧义
/// 单字不切（「学会手冲咖啡」不能在「会」处碎掉、「使用滤杯」不能在「用」
/// 处碎掉）——本表只管语境词装配，与那张定义侧表无互锁关系。
const MUNCH_BOUNDARY_CHARS: &str = "的了是在和与及或把被从向到为以对之于之其这那";

/// 语境词切片（w02 最大 munch；u04 课程语境词的装配源）：与 [`term_slices`]
/// 同底（分段 → 去停用词 → 去重保序 → 限长），但 CJK 段不再炸成全部相邻
/// 二元——**互含消解到不动点**：
/// ①CJK 段先按 [`MUNCH_BOUNDARY_CHARS`] 切成短语，ASCII 词整词；
/// ②凡短语 P（≥2 字）逐字内嵌于另一短语 Q，Q 在该处拆开：P 自成条目，
///   残段（≥2 字）另立条目、单字残段丢弃，迭代到不再互含；
/// ③停用词短语整条滤除。
/// 产物 = 标题/目标中互不包含的最长 CJK 串（手冲咖啡/萃取状态判读/调整
/// 形态）——跨词界二元垃圾（冲咖/啡的/取状/态判）不再产出。无词典依赖
/// 的确定性与「像词」不可兼得：无词典就无法把「萃取状态判读」再拆成
/// 萃取状态/判读，短语整段保留（有意义、非垃圾）——这是取舍不是缺陷。
/// [`term_slices`] 本体不动：采样打分与检索查询仍吃二元（整句子串命中
/// 蕴含全部二元命中，召回面不变）；语境词是提示词豁免面，要「像词」不要
/// 「全量二元」，同时作为点名清单已知面，词对齐了掩码残片才不产生。
pub(crate) fn context_word_slices(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    // ①初池：CJK 短语（虚字边界切段，单字残段即弃）+ ASCII 整词。
    let mut items: Vec<String> = Vec::new();
    for run in script_runs(&lowered) {
        let chars: Vec<char> = run.chars().collect();
        if !is_cjk(chars[0]) {
            if chars.len() >= MIN_LATIN_TERM_CHARS {
                items.push(run);
            }
            continue;
        }
        let mut phrase = String::new();
        for ch in chars {
            if MUNCH_BOUNDARY_CHARS.contains(ch) {
                if phrase.chars().count() >= 2 {
                    items.push(std::mem::take(&mut phrase));
                } else {
                    phrase.clear();
                }
            } else {
                phrase.push(ch);
            }
        }
        if phrase.chars().count() >= 2 {
            items.push(phrase);
        }
    }
    // ②互含消解（不动点；每次拆解使容器短 ≥2 字，必然终止）。取文本序
    // 第一对拆：容器在原位换成残段，出现序大体保留。
    loop {
        let mut splice: Option<(usize, usize, usize)> = None; // (容器, 起点, P 长度)
        'scan: for (qi, container) in items.iter().enumerate() {
            let q: Vec<char> = container.chars().collect();
            for (pi, part) in items.iter().enumerate() {
                if pi == qi {
                    continue;
                }
                let p: Vec<char> = part.chars().collect();
                if p.len() < 2 || p.len() >= q.len() {
                    continue;
                }
                if let Some(pos) = (0..=q.len() - p.len()).find(|&i| q[i..i + p.len()] == p[..]) {
                    splice = Some((qi, pos, p.len()));
                    break 'scan;
                }
            }
        }
        let Some((qi, pos, plen)) = splice else { break };
        let q: Vec<char> = items.remove(qi).chars().collect();
        // 前缀/后缀按出现序插回原位（残段 <2 字即弃）。
        let mut at = qi;
        let prefix = q[..pos].iter().collect::<String>();
        if prefix.chars().count() >= 2 {
            items.insert(at, prefix);
            at += 1;
        }
        let suffix = q[pos + plen..].iter().collect::<String>();
        if suffix.chars().count() >= 2 {
            items.insert(at, suffix);
        }
    }
    // ③停用词滤除 + 去重保序 + 限长（与 term_slices 同尺）。
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for item in items {
        if STOPWORDS.contains(&item.as_str()) || !seen.insert(item.clone()) {
            continue;
        }
        out.push(item);
        if out.len() >= TERM_MAX_COUNT {
            return out;
        }
    }
    out
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

    // ── w02 语境词最大 munch（context_word_slices）────────────────────────

    /// 真机同型（run-1793980240191）：标题内嵌于目标短语 → 消解出标题词与
    /// 残段「学会」，虚字（的/与）处切段；产物互不包含、无跨词界二元垃圾。
    #[test]
    fn context_word_slices_munch_to_longest_noncontained_strings() {
        let words = context_word_slices("手冲咖啡\n学会手冲咖啡的萃取状态判读与调整");
        assert_eq!(
            words,
            vec!["手冲咖啡", "学会", "萃取状态判读", "调整"],
            "最大 munch：标题词自成条目、残段保留、内部二元不再出现"
        );
        for garbage in ["会手", "冲咖", "啡的", "的萃", "取状", "态判", "读与", "与调"] {
            assert!(!words.contains(&garbage.to_owned()), "垃圾二元 {garbage} 不在语境词：{words:?}");
        }
    }

    /// `term_slices` 本体不动：采样/检索词条仍吃全部相邻二元（含跨词界
    /// 二元——整句子串命中蕴含二元命中，召回面不因语境词精化而变）。
    #[test]
    fn term_slices_still_emits_adjacent_bigrams_for_retrieval() {
        let bigrams = term_slices("学会手冲咖啡的萃取状态判读与调整");
        assert!(bigrams.contains(&"冲咖".to_owned()), "检索词条二元照旧：{bigrams:?}");
        assert!(bigrams.contains(&"态判".to_owned()), "检索词条二元照旧：{bigrams:?}");
    }

    /// 停用词短语整条滤除；ASCII 整词；虚字切段产生的单字残段不立条目；
    /// 无互含时短语整段保留（无词典不强拆——取舍见函数注释）。
    #[test]
    fn context_word_slices_drop_stopword_phrases_and_keep_ascii_whole() {
        let words = context_word_slices("理解闭包对象的语义与这个 SQL 优化");
        assert_eq!(
            words,
            vec!["理解闭包", "语义", "sql", "优化"],
            "「这个」是停用词短语；「象」是单字残段；ASCII 整词小写"
        );
        assert!(context_word_slices("手冲咖啡的手冲咖啡").contains(&"手冲咖啡".to_owned()));
        assert!(context_word_slices("").is_empty());
    }
}
