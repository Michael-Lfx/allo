//! 定义审计（机械层，learnhub #171/#173 一阶移植；absorb-5 m9）：候选抽取、
//! 接地点名、自指、互指环。纯本地零 LLM、零写入——审计是缺口标记不是拦截门
//! （「读侧先看见」）。
//!
//! **消费面**：`concept_audit`（服务面只读报告）消费全部四个纯函数；
//! candidate_terms / grounding_runs 的进一步消费（生长评审点名清单、词汇
//! 裁决）在后续票接入。不改登记表、不做语义裁决。
//!
//! - [`candidate_terms`]:对定义枚举 CJK 1–4 字 n-gram 作为**候选术语**;只做两类
//!   过滤(恰为已知词、纯虚字组合),**不做切分、不做单字分类**。设计立场是
//!   「宁滥勿缺」:单字歧义(所/有/用/内…)在中文里无字面解,任何按单字预先
//!   分类的方案都会在某门课的复合术语上碎掉(learnhub Spike 3 实证:
//!   所有权→所+有+权、内角→内+角);候选集允许噪声。
//! - [`grounding_runs`]:定义中未被已知词覆盖的最大 CJK 连续段(最长优先
//!   掩码;单字只作边界 token)。
//! - [`self_references`]:定义里出现自身正名(ISO 704 禁止自指);
//! - [`mutual_cycles`]:两两互指(A 定义用 B、B 定义用 A;循环定义的最短形态)。
//!
//! 自指与互指环是纯图/字符串检查,跨域零失败(数学/哲学/Rust 多域夹具)。

/// 虚字表:纯虚字组合(所有字都在此表内)不进候选;**单字是否虚字不在此判定**,
/// 见模块注——歧义单字一律放行为候选,由下游裁决分类。
pub const FUNCTION_CHARS: &str = "的了是在和与及或把被从向到为以对之于之其这那有无不可能会要需称记作表所示取等若则即按照由此每两三一二零几个份种也又而并将已未很最更再还都只就才者地得着过们它他她我你使让用自至而且还是就是只是已经正在没有不是不会不必不用";

/// 候选术语:CJK 1–4 字 n-gram,排除「恰为已知词」与「纯虚字组合」,去重保序。
/// `known` = 已注册名 ∪ 定义用语表（仓库级基础用语）∪ 课程增词(有着落的词,不再是缺口)。
/// 已知词的**超串保留**(如 已知「整数」而「整数加法」未在册——它正是候选)。
pub fn candidate_terms(definition: &str, known: &[String]) -> Vec<String> {
    let known: std::collections::BTreeSet<&str> = known.iter().map(String::as_str).collect();
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    // 最大 CJK 连续段内枚举,不跨标点/空白/拉丁字符。
    let mut run: Vec<char> = Vec::new();
    fn emit(run: &[char], out: &mut Vec<String>, seen: &mut std::collections::BTreeSet<String>) {
        let n = run.len();
        for len in 1..=4usize.min(n) {
            for start in 0..=(n - len) {
                let gram: String = run[start..start + len].iter().collect();
                if seen.insert(gram.clone()) {
                    out.push(gram);
                }
            }
        }
    }
    for ch in definition.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            run.push(ch);
        } else {
            emit(&run, &mut out, &mut seen);
            run.clear();
        }
    }
    emit(&run, &mut out, &mut seen);
    out.retain(|gram| {
        !known.contains(gram.as_str()) && !gram.chars().all(|c| FUNCTION_CHARS.contains(c))
    });
    out
}

/// 接地点名片段:定义中**未被已知词覆盖**的最大 CJK 连续段(已知词最长优先、
/// 不重叠地掩码;**歧义单字不作掩码**——否则「内角」被「内」吞掉、「所有权」
/// 被「所/有」拆碎;无歧义虚字单字掩码)。片段经边界归一化(剥尾随/前导无
/// 歧义虚字)后去重保序,超过 [`MAX_NAMING_FRAGMENTS`] 时按长度降序截断。
/// 「宁滥勿缺」:片段必含切词噪声,处置权在下游(碎片裁决面 m3 后接)。
pub fn grounding_runs(definition: &str, known: &[String]) -> Vec<String> {
    let known: std::collections::BTreeSet<&str> = known.iter().map(String::as_str).collect();
    let chars: Vec<char> = definition.chars().collect();
    let mut used = vec![false; chars.len()];
    let mut words: Vec<&str> = known.iter().copied().collect();
    // 单字只作边界 token:单字在册词不作掩码(歧义单字吞复合词缺陷纪律)。
    words.retain(|w| w.chars().count() >= 2);
    words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    for word in words {
        let w: Vec<char> = word.chars().collect();
        let mut i = 0usize;
        while i + w.len() <= chars.len() {
            if !used[i] && chars[i..i + w.len()] == w[..] {
                for u in used.iter_mut().skip(i).take(w.len()) {
                    *u = true;
                }
                i += w.len();
            } else {
                i += 1;
            }
        }
    }
    let mut runs: Vec<String> = Vec::new();
    let mut cur: String = String::new();
    for (i, ch) in chars.iter().enumerate() {
        let cjk = ('\u{4e00}'..='\u{9fff}').contains(ch);
        let masked = used[i] || (cjk && FUNCTION_CHARS.contains(*ch));
        if masked || !cjk {
            if !cur.is_empty() {
                runs.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(*ch);
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for run in &runs {
        let norm = normalize_run(run);
        if !norm.is_empty() && seen.insert(norm.clone()) {
            out.push(norm);
        }
    }
    if out.len() > MAX_NAMING_FRAGMENTS {
        // 截断保序:先按长度降序选前 40,再按原出现序放回。
        let mut indexed: Vec<(usize, String)> = out.into_iter().enumerate().collect();
        indexed.sort_by(|a, b| {
            b.1.chars()
                .count()
                .cmp(&a.1.chars().count())
                .then(a.0.cmp(&b.0))
        });
        indexed.truncate(MAX_NAMING_FRAGMENTS);
        indexed.sort_by_key(|(i, _)| *i);
        out = indexed.into_iter().map(|(_, s)| s).collect();
    }
    out
}

/// 点名片段上限(每条定义;超限按长度优先截断)。
pub const MAX_NAMING_FRAGMENTS: usize = 40;

/// 片段边界归一化:剥掉尾随/前导的无歧义虚字(只剥到还剩 1 字为止);
/// **歧义单字不剥**(所有用为以对于中上下内外后前时向——它们可能是术语语素)。
fn normalize_run(run: &str) -> String {
    const TRAIL: &str = "的了是在着过和与及或把被从向到为以对之于之其这那无不可会要需每两三一二零几个份种也又而并将已未很最更再还都只就才者地得们它他她我你使让处里等条";
    const LEAD: &str =
        "来去使让把被在从向对由于其这那和与及或每两三四五六七八九十百千万几每条个只不";
    let mut chars: Vec<char> = run.chars().collect();
    while chars.len() > 1 && TRAIL.contains(chars[chars.len() - 1]) {
        chars.pop();
    }
    while chars.len() > 1 && LEAD.contains(chars[0]) {
        chars.remove(0);
    }
    chars.into_iter().collect()
}

/// 自指:定义里出现自身正名(ISO 704 禁止;这里只做精确包含,调用方传正名即可)。
pub fn self_references(concepts: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, def) in concepts {
        if !name.is_empty() && def.contains(name.as_str()) {
            out.push(name.clone());
        }
    }
    out
}

/// 两两互指(A 定义含 B、B 定义含 A);返回 (name_a, name_b) 且 a < b 去重。
pub fn mutual_cycles(concepts: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (i, (a, def_a)) in concepts.iter().enumerate() {
        for (j, (b, def_b)) in concepts.iter().enumerate() {
            if j <= i || a == b {
                continue;
            }
            let a_hits_b = def_a.contains(b.as_str());
            let b_hits_a = def_b.contains(a.as_str());
            if a_hits_b && b_hits_a {
                let (x, y) = if a < b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                out.push((x, y));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 夹具（learnhub 原样移植）：编译期嵌入，测试零运行时文件依赖。
    const PHILOSOPHY_FIXTURE: &str =
        include_str!("../../../../../docs/research/fixtures/learning/philosophy-intro/case.json");
    const RUST_FIXTURE: &str =
        include_str!("../../../../../docs/research/fixtures/learning/rust-ownership/case.json");

    fn contains(cands: &[String], want: &str) -> bool {
        cands.iter().any(|c| c == want)
    }

    fn fixture_concepts(raw: &str) -> Vec<(String, String)> {
        let doc: serde_json::Value = serde_json::from_str(raw).expect("夹具可解析");
        doc["concepts"]
            .as_array()
            .expect("concepts 数组")
            .iter()
            .map(|c| {
                (
                    c["name"].as_str().unwrap_or_default().to_string(),
                    c["definition"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    fn def_of<'a>(concepts: &'a [(String, String)], name: &str) -> &'a str {
        concepts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.as_str())
            .unwrap_or_else(|| panic!("夹具缺概念:{name}"))
    }

    fn allowed(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    /// 数学回归:三角形定义的构件词必须全部成为候选(1–4 字全覆盖)。
    #[test]
    fn triangle_candidates_include_constituent_nouns() {
        let def = "由三条线段首尾顺次相接围成的图形；三条线段是它的边，相邻两边在顶点处夹出的角是它的内角，每个三角形有三条边和三个内角。";
        let cands = candidate_terms(def, &["三角形".to_string()]);
        for want in ["线段", "内角", "顶点", "图形", "边", "角", "相邻"] {
            assert!(
                contains(&cands, want),
                "{want} 须是候选:{:?}",
                &cands[..cands.len().min(30)]
            );
        }
        // 纯虚字组合与虚字单字不进候选
        assert!(!contains(&cands, "的了"));
        assert!(!contains(&cands, "的"));
    }

    /// 已知词不再候选;其超串保留(整数在册,整数加法未在册 → 候选)。
    #[test]
    fn known_words_are_not_candidates_but_their_expansions_are() {
        let def = "整数加法是把两个整数合并成一个数的运算。";
        let cands = candidate_terms(def, &["整数".to_string(), "加法".to_string()]);
        assert!(!contains(&cands, "整数"), "在册词不是候选");
        assert!(!contains(&cands, "加法"), "在册词不是候选");
        assert!(contains(&cands, "整数加法"), "已知词的超串是候选");
    }

    /// Rust 夹具:自指(所有权)与互指环(借用↔所有权)被抓住;借用定义的候选
    /// 含 所有权/引用(单字歧义语素不再拆碎它们)。
    #[test]
    fn rust_fixture_checks() {
        let concepts = fixture_concepts(RUST_FIXTURE);
        assert_eq!(
            self_references(&concepts),
            vec!["所有权".to_string()],
            "自指须点名"
        );
        assert_eq!(
            mutual_cycles(&concepts),
            vec![("借用".to_string(), "所有权".to_string())],
            "互指环须点名"
        );
        let def = def_of(&concepts, "借用");
        let cands = candidate_terms(def, &[]);
        assert!(contains(&cands, "所有权"), "所有权须是候选:{cands:?}");
        assert!(contains(&cands, "引用"), "引用须是候选");
        assert!(contains(&cands, "临时"), "临时须是候选");
    }

    /// 哲学夹具:自指(怀疑论)与互指环(实体↔属性);属性定义的候选含 实体/述说。
    #[test]
    fn philosophy_fixture_checks() {
        let concepts = fixture_concepts(PHILOSOPHY_FIXTURE);
        assert_eq!(
            self_references(&concepts),
            vec!["怀疑论".to_string()],
            "自指须点名"
        );
        assert_eq!(
            mutual_cycles(&concepts),
            vec![("实体".to_string(), "属性".to_string())],
            "互指环须点名"
        );
        let def = def_of(&concepts, "属性");
        let cands = candidate_terms(def, &[]);
        assert!(contains(&cands, "实体"), "实体须是候选:{cands:?}");
        assert!(contains(&cands, "述说"), "述说须是候选");
    }

    /// 接地点名:三角形的构件词逐个露出为片段——单字歧义不吞并、
    /// 尾随虚词被剥(顶点处→顶点)。
    #[test]
    fn grounding_runs_expose_uncovered_constituent_nouns() {
        let def = "由三条线段首尾顺次相接围成的图形；三条线段是它的边，相邻两边在顶点处夹出的角是它的内角，每个三角形有三条边和三个内角。";
        let known = allowed(&[
            "三角形",
            "由",
            "三",
            "条",
            "首尾",
            "顺次",
            "相接",
            "围成",
            "的",
            "是",
            "它",
            "相邻",
            "两边",
            "在",
            "处",
            "夹出",
            "每个",
            "和",
            "有",
            "个",
        ]);
        let runs = grounding_runs(def, &known);
        for want in ["线段", "内角", "顶点", "图形", "边", "角"] {
            assert!(
                runs.iter()
                    .any(|r| r.contains(want) || want.contains(r.as_str())),
                "{want} 须出现在点名片段(或其子串):{runs:?}"
            );
        }
    }

    /// 已知词覆盖处不点名:全部覆盖 = 零片段。
    #[test]
    fn fully_grounded_definition_yields_no_fragments() {
        let def = "整数加法是把两个整数合并成一个数的运算。";
        let known = allowed(&[
            "整数",
            "加法",
            "是把",
            "两个",
            "合并成",
            "一个",
            "数的",
            "运算",
        ]);
        assert!(grounding_runs(def, &known).is_empty());
    }

    /// 歧义单字不作掩码:已知「所有」时,「所有权」仍整段露出(不被 所/有 拆碎)。
    #[test]
    fn ambiguous_single_chars_do_not_mask() {
        let def = "对所有权的排他支配。";
        let known = allowed(&["所有", "对", "的", "支配"]);
        let runs = grounding_runs(def, &known);
        assert!(
            runs.iter().any(|r| r.contains("权")),
            "权 须在片段中:{runs:?}"
        );
        assert!(
            !runs.iter().any(|r| r == "所有"),
            "已知词「所有」覆盖处不点名:{runs:?}"
        );
    }

    /// 单字只作边界 token(acceptance:内角不被 内+角 吃掉):在册单字
    /// 「内」「角」不掩码——「内角」仍以完整片段露出。
    #[test]
    fn known_single_chars_do_not_swallow_compound_terms() {
        let def = "三角形的内角和是 180 度。";
        let known = allowed(&["内", "角", "三"]);
        let runs = grounding_runs(def, &known);
        assert!(
            runs.iter().any(|r| r.contains("内角")),
            "内角 不被单字 内/角 吞掉:{runs:?}"
        );
    }

    /// 最长优先:同一位置同时命中长短两词时,长词整体掩码、零残留
    /// (短词优先会把长词拆出尾巴)。
    #[test]
    fn longest_known_word_masks_first() {
        let def = "整数加法满足交换律。";
        let known = allowed(&["整数", "整数加法", "交换"]);
        let runs = grounding_runs(def, &known);
        assert!(
            !runs.iter().any(|r| r.contains("整数加法")),
            "长词整体覆盖,不再留尾巴:{runs:?}"
        );
        assert!(
            !runs.iter().any(|r| r.contains("加法")),
            "长词优先掩盖后,其子串不二次露出:{runs:?}"
        );
    }
}
