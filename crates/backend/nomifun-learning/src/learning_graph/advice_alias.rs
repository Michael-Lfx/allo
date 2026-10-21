//! 评审建议的机械转化面（t4）：advice 指向「裁为别名/复用」的问题由代码
//! 直接执行，不折返模型。真机实录（run-1792403482606 call 3）：评审对近义
//! 铸名的诊断准确、修法完整（「把该铸名裁为复用『进位制』，写入顶层
//! reuse（concept_id: …）」），却写进 problems 打回——遵循是概率性的，每次
//! 打回烧一轮重裁预算（点修越权高发段）。本模块把这类问题解析成合成
//! reuse 裁决，复用 j04 落池通道（铸名不铸新行、落为宿主活跃别名、节点引
//! 用随 rulings 改写），问题从报告剔除。
//!
//! 解析面**从窄**（宁漏勿错——解析失败的问题照常打回）：
//! - field ∈ {teaches, mints}；
//! - issue+advice 含复用信号词（「复用」或「别名」）；
//! - 恰好一个本批提名名（「X」/『X』引号内的名字，mints canonical 或
//!   new_aliases 名）；
//! - 恰好一个宿主：登记表可唯一解析的引号名（canonical ∪ 活跃别名），或
//!   文本中的 concept_id 字面量（须登记表在册）。

use std::collections::{HashMap, HashSet};

use super::ReviewProblem;

/// 机械转化结果：问题下标 + （提名名小写， 宿主 concept_id）。
pub(crate) type AdviceConversion = (usize, String, String);

/// 解析可机械转化的问题。`nomination_names` = 本批提名名（小写）；
/// `registry_hosts` = 登记表活跃名（小写）→ (concept_id, canonical)，歧义名
/// 不在册；`valid_concept_ids` = 登记表在册 concept_id（小写）。
pub(crate) fn advice_reuse_conversions(
    problems: &[ReviewProblem],
    nomination_names: &HashSet<String>,
    registry_hosts: &HashMap<String, (String, String)>,
    valid_concept_ids: &HashSet<String>,
) -> Vec<AdviceConversion> {
    let mut conversions = Vec::new();
    for (index, problem) in problems.iter().enumerate() {
        if !matches!(problem.field.as_str(), "teaches" | "mints") {
            continue;
        }
        let text = format!("{}{}", problem.issue, problem.advice);
        if !text.contains("复用") && !text.contains("别名") {
            continue;
        }
        // 提名名与宿主候选都从引号段里找；issue/advice 常各自重复点名同一
        // 对象，按去重后的不同名计；提名名撞登记表名的段不算宿主（那是撞
        // 池面，走 gate 的机械复用，不是 advice 面）。
        let mut nominated: Vec<String> = Vec::new();
        let mut hosts: Vec<String> = Vec::new();
        for span in quoted_spans(&text) {
            let key = span.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            if nomination_names.contains(&key) {
                if !nominated.contains(&key) {
                    nominated.push(key);
                }
                continue;
            }
            if let Some((concept_id, _)) = registry_hosts.get(&key) {
                hosts.push(concept_id.clone());
            }
        }
        for literal in concept_id_literals(&text) {
            if valid_concept_ids.contains(&literal) {
                hosts.push(literal);
            }
        }
        let mut distinct_hosts: Vec<String> = hosts;
        distinct_hosts.sort();
        distinct_hosts.dedup();
        // 恰好一个提名名 + 恰好一个宿主才转化；多名/多宿主/无名都是歧义，
        // 照常打回（宁漏勿错）。
        if nominated.len() != 1 || distinct_hosts.len() != 1 {
            continue;
        }
        conversions.push((index, nominated[0].clone(), distinct_hosts[0].clone()));
    }
    conversions
}

/// 「X」与『X』引号段（评审中英混排的两种书名习惯；不做嵌套）。
fn quoted_spans(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, open)) = chars.next() {
        let close = match open {
            '「' => '」',
            '『' => '』',
            _ => continue,
        };
        let mut end = None;
        while let Some(&(offset, ch)) = chars.peek() {
            chars.next();
            if ch == close {
                end = Some(offset + ch.len_utf8());
                break;
            }
            if ch == open {
                // 嵌套开引号：段作废（宁漏勿错）。
                end = None;
                break;
            }
        }
        if let Some(end) = end {
            spans.push(&text[start + open.len_utf8()..end - close.len_utf8()]);
        }
    }
    spans
}

/// 文本中的 concept_id 字面量（8-4-4-4-12 小写十六进制；大小写不敏感收，
/// 归一小写后比对）。
fn concept_id_literals(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, found: &mut Vec<String>| {
        if is_concept_id_shape(current) {
            found.push(current.to_lowercase());
        }
        current.clear();
    };
    for ch in text.chars() {
        if ch.is_ascii_hexdigit() || ch == '-' {
            current.push(ch);
        } else {
            flush(&mut current, &mut found);
        }
    }
    flush(&mut current, &mut found);
    found
}

fn is_concept_id_shape(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(field: &str, issue: &str, advice: &str) -> ReviewProblem {
        ReviewProblem {
            node: "节点".into(),
            field: field.into(),
            issue: issue.into(),
            advice: advice.into(),
        }
    }

    fn hosts(entries: &[(&str, &str, &str)]) -> HashMap<String, (String, String)> {
        entries
            .iter()
            .map(|&(name, id, canonical)| (name.to_owned(), (id.to_owned(), canonical.to_owned())))
            .collect()
    }

    /// 真机实录形态（run-1792403482606 call 3）：issue/advice 都用『』点名，
    /// advice 内带 concept_id 字面量。
    #[test]
    fn converts_real_machine_advice_with_concept_id_literal() {
        let problems = vec![problem(
            "mints",
            "提名『二进制编码』定义与登记表既有概念『进位制』内涵重叠——应复用，不新铸。",
            "把该铸名裁为复用『进位制』，写入顶层 reuse（concept_id: 01a15212-0462-7182-9234-a6b32ecd99a4）。",
        )];
        let nominations: HashSet<String> = ["二进制编码".to_owned()].into();
        let registry = hosts(&[(
            "进位制",
            "01a15212-0462-7182-9234-a6b32ecd99a4",
            "进位制",
        )]);
        let valid: HashSet<String> = ["01a15212-0462-7182-9234-a6b32ecd99a4".to_owned()].into();
        let parsed = advice_reuse_conversions(&problems, &nominations, &registry, &valid);
        assert_eq!(
            parsed,
            vec![(0, "二进制编码".to_owned(), "01a15212-0462-7182-9234-a6b32ecd99a4".to_owned())],
            "{parsed:?}"
        );
    }

    /// 宿主只以名字出现（无 concept_id）且登记表可唯一解析 → 转化。
    #[test]
    fn converts_with_registry_resolvable_host_name() {
        let problems = vec![problem(
            "teaches",
            "节点 teaches「霍纳法」与既有概念『二进制转十进制』是同一概念的两个名字。",
            "方案二：按复用裁决将该铸名裁为既有『二进制转十进制』的别名，不另铸新行。",
        )];
        let nominations: HashSet<String> = ["霍纳法".to_owned(), "分段转法".to_owned()].into();
        let registry = hosts(&[
            ("二进制转十进制", "01a1442e-a230-7650-a30a-80246ee6bb5d", "二进制转十进制"),
            ("分段转法", "01a99999-9999-7999-8999-999999999999", "别的东西"),
        ]);
        // 「分段转法」是提名名：不算宿主候选，即便登记表撞池。
        let valid: HashSet<String> = HashSet::new();
        let parsed = advice_reuse_conversions(&problems, &nominations, &registry, &valid);
        assert_eq!(
            parsed,
            vec![(0, "霍纳法".to_owned(), "01a1442e-a230-7650-a30a-80246ee6bb5d".to_owned())]
        );
    }

    /// 无复用信号 / 宿主解析不出 / 多个宿主 / 无提名名 → 照常打回。
    #[test]
    fn skips_without_signal_host_or_nominee() {
        let nominations: HashSet<String> = ["向量代数".to_owned()].into();
        let registry = hosts(&[("线性组合", "01a00000-0000-7000-8000-000000000001", "线性组合")]);
        let valid: HashSet<String> = HashSet::new();
        // 无信号词。
        let parsed = advice_reuse_conversions(
            &[problem("mints", "铸名「向量代数」重复。", "改个名字。")],
            &nominations, &registry, &valid,
        );
        assert!(parsed.is_empty());
        // 信号在但宿主不在登记表。
        let parsed = advice_reuse_conversions(
            &[problem("mints", "铸名「向量代数」重叠。", "应复用『不存在的概念』。")],
            &nominations, &registry, &valid,
        );
        assert!(parsed.is_empty());
        // 两个宿主候选 → 歧义不裁。
        let rich = hosts(&[
            ("线性组合", "01a00000-0000-7000-8000-000000000001", "线性组合"),
            ("矩阵", "01a00000-0000-7000-8000-000000000002", "矩阵"),
        ]);
        let parsed = advice_reuse_conversions(
            &[problem("mints", "「向量代数」介于『线性组合』与『矩阵』之间。", "应复用。")],
            &nominations, &rich, &valid,
        );
        assert!(parsed.is_empty());
        // 引号里没有本批提名名。
        let parsed = advice_reuse_conversions(
            &[problem("teaches", "『线性组合』重复教。", "应复用别名。")],
            &nominations, &registry, &valid,
        );
        assert!(parsed.is_empty());
    }

    /// field 不在面内（assumes/difficulty）→ 不碰。
    #[test]
    fn skips_non_mint_fields() {
        let nominations: HashSet<String> = ["向量代数".to_owned()].into();
        let registry = hosts(&[("线性组合", "01a00000-0000-7000-8000-000000000001", "线性组合")]);
        let parsed = advice_reuse_conversions(
            &[problem("assumes", "「向量代数」应复用『线性组合』。", "同左。")],
            &nominations, &registry, &HashSet::new(),
        );
        assert!(parsed.is_empty());
    }

    /// 嵌套引号段作废、concept_id 形状错位不收。
    #[test]
    fn nested_spans_and_malformed_ids_are_rejected() {
        assert_eq!(quoted_spans("「外层『内层』收尾」"), vec!["外层『内层』收尾"]);
        assert!(!is_concept_id_shape("01a15212-0462-7182-9234-a6b32ecd99a"));
        assert!(!is_concept_id_shape("01a15212_0462_7182_9234_a6b32ecd99a4"));
        assert!(is_concept_id_shape("01A15212-0462-7182-9234-A6B32ECDA4A4"));
        assert_eq!(
            concept_id_literals("前置 id=01a15212-0462-7182-9234-a6b32ecd99a4 后置"),
            vec!["01a15212-0462-7182-9234-a6b32ecd99a4".to_owned()]
        );
    }
}
