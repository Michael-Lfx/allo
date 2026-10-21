//! 定义近邻与名字近形（二轮 g3 复用通道的机械面；learnhub defnear.rs /
//! nearname.rs 同构移植）：把「提名」装配到登记表的近邻候选集，供概念评审
//! 裁决「复用 | 新铸」。纯函数、零 IO。
//!
//! - 定义归一化：全角折叠 + 剥空白标点 + 小写——归一化后逐字相等 =
//!   **直定宿主**（不送裁决，learnhub 同律）；
//! - 字符 trigram Jaccard：定义近邻的相似度（≥ [`DEFINITION_NEAR_THRESHOLD`]
//!   入候选）；
//! - 名字近形：归一化后互包含（双向、最短 2 字符）；
//! - 词面补位：二元组共享计数 top-[`WORD_FACE_TOP_K`]，**零分亦入**——
//!   候选集永远非空替评审兜底可见面。

/// 定义近邻阈值（trigram Jaccard；learnhub defnear 同量级）。
pub const DEFINITION_NEAR_THRESHOLD: f64 = 0.55;

/// 词面补位候选数（零分亦入）。
pub const WORD_FACE_TOP_K: usize = 3;

/// 归一化登记表条目（候选装配的输入面）。
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub concept_id: String,
    pub canonical: String,
    pub definition: String,
    /// 活跃别名（退役名不参与候选装配）。
    pub active_aliases: Vec<String>,
}

/// 近邻候选（评审裁决的指向面；`reason` 供评审消息与人读报告）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborCandidate {
    pub concept_id: String,
    pub canonical: String,
    pub reason: String,
}

/// 归一化：全角折叠（ASCII 区）+ 剥空白与标点 + 小写。CJK 保持原字。
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        // 全角 ASCII 区（U+FF01..U+FF5E）折叠回半角。
        let folded = match u32::from(ch) {
            code @ 0xFF01..=0xFF5E => char::from_u32(code - 0xFEE0).unwrap_or(ch),
            _ => ch,
        };
        for part in folded.to_lowercase() {
            if part.is_whitespace() || part.is_ascii_punctuation() {
                continue;
            }
            // 常见 CJK 标点（中点/顿号/句读/引号/括号等）按标点剥除。
            if matches!(
                part,
                '·' | '、' | '。' | '，' | '：' | '；' | '！' | '？' | '「' | '」' | '『' | '』'
                    | '（' | '）' | '《' | '》' | '—' | '…' | '－' | '～'
            ) {
                continue;
            }
            out.push(part);
        }
    }
    out
}

/// 字符 trigram 集合（归一化后；不足 3 字取全串为唯一 gram）。
fn trigrams(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 3 {
        return vec![text.to_owned()];
    }
    chars
        .windows(3)
        .map(|window| window.iter().collect())
        .collect()
}

/// 字符 trigram Jaccard 相似度（归一化输入）。
pub fn trigram_jaccard(a: &str, b: &str) -> f64 {
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (left, right) = (trigrams(&a), trigrams(&b));
    let left_set: std::collections::HashSet<&String> = left.iter().collect();
    let right_set: std::collections::HashSet<&String> = right.iter().collect();
    let intersection = left_set.intersection(&right_set).count();
    let union = left_set.union(&right_set).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

/// 名字近形（learnhub nearname 同构：归一化后互包含，双向任一、最短段
/// ≥2 字符——「因式分解」近形「分解」「因式」，不近形「极限」）。
pub fn name_near_form(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    if a.len() < 2 || b.len() < 2 {
        return false;
    }
    a.contains(&b) || b.contains(&a)
}

/// 归一化后的字符 Levenshtein 距离（逐 char DP；双方都过 [`normalize`]）。
pub fn char_edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (normalize(a), normalize(b));
    let (a, b) = (a.chars().collect::<Vec<_>>(), b.chars().collect::<Vec<_>>());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitute = prev[j] + usize::from(ca != cb);
            let insert = cur[j] + 1;
            let delete = prev[j + 1] + 1;
            cur.push(substitute.min(insert).min(delete));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// 受限编辑近形（生长引用归一的候选谓词，比 [`name_near_form`] 宽一档）：
/// 距离 ≤1 且短边 ≥3 字符，或距离 2 且短边 ≥5 字符——「十进位置值」/「十进制
/// 位值」（距离 2）入，两字短名（「数位」/「数值」）不入：短名一字差常常是
/// 两个真概念，归一权必须留给评审门。
pub fn bounded_edit_close(a: &str, b: &str) -> bool {
    let distance = char_edit_distance(a, b);
    if distance == 0 {
        return false;
    }
    let (a, b) = (normalize(a), normalize(b));
    let shortest = a.chars().count().min(b.chars().count());
    match distance {
        1 => shortest >= 3,
        2 => shortest >= 5,
        _ => false,
    }
}

/// 词面共享分（二元组交集计数；词面补位的排序键）。
fn word_face_score(a: &str, b: &str) -> usize {
    let bigrams = |text: &str| -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() < 2 {
            return vec![text.to_owned()];
        }
        chars.windows(2).map(|window| window.iter().collect()).collect()
    };
    let (left, right) = (bigrams(&normalize(a)), bigrams(&normalize(b)));
    let left_set: std::collections::HashSet<String> = left.into_iter().collect();
    let right_set: std::collections::HashSet<String> = right.into_iter().collect();
    left_set.intersection(&right_set).count()
}

/// 一次提名（铸名 canonical 或 new_alias 名字）。
#[derive(Debug, Clone)]
pub struct Nomination {
    /// 提名名字。
    pub name: String,
    /// 提名定义（铸名的 definition / new_alias 的 definition）。
    pub definition: String,
}

/// 装配一份提名的近邻候选集（learnhub defnear 机械面）：精确占用 → 定义
/// 归一化直定 → 定义近邻 → 名字近形 → 词面 top-k 补位（去重保序，强度
/// 降序）。
pub fn assemble_neighbors(nomination: &Nomination, registry: &[RegistryEntry]) -> Vec<NeighborCandidate> {
    let mut candidates: Vec<NeighborCandidate> = Vec::new();
    let mut push = |candidates: &mut Vec<NeighborCandidate>, candidate: NeighborCandidate| {
        if !candidates
            .iter()
            .any(|existing| existing.concept_id == candidate.concept_id)
        {
            candidates.push(candidate);
        }
    };

    let normalized_definition = normalize(&nomination.definition);
    let mut word_face_scored: Vec<(usize, &RegistryEntry)> = registry
        .iter()
        .map(|entry| {
            let score = word_face_score(&nomination.name, &entry.canonical)
                + entry
                    .active_aliases
                    .iter()
                    .map(|alias| word_face_score(&nomination.name, alias))
                    .max()
                    .unwrap_or(0);
            (score, entry)
        })
        .collect();
    word_face_scored.sort_by(|a, b| b.0.cmp(&a.0));

    for entry in registry {
        let names: Vec<&str> = std::iter::once(entry.canonical.as_str())
            .chain(entry.active_aliases.iter().map(String::as_str))
            .collect();
        // 精确占用（名字照抄即撞）。
        if names
            .iter()
            .any(|name| normalize(name) == normalize(&nomination.name))
        {
            push(
                &mut candidates,
                NeighborCandidate {
                    concept_id: entry.concept_id.clone(),
                    canonical: entry.canonical.clone(),
                    reason: "名字精确占用（该名字已是登记表活跃名）".into(),
                },
            );
            continue;
        }
        // 定义归一化直定（逐字相等——最强信号）。
        if !normalized_definition.is_empty()
            && normalize(&entry.definition) == normalized_definition
        {
            push(
                &mut candidates,
                NeighborCandidate {
                    concept_id: entry.concept_id.clone(),
                    canonical: entry.canonical.clone(),
                    reason: "定义归一化后与该概念逐字相同".into(),
                },
            );
            continue;
        }
        // 定义近邻（trigram Jaccard）。
        let similarity = trigram_jaccard(&nomination.definition, &entry.definition);
        if similarity >= DEFINITION_NEAR_THRESHOLD && !entry.definition.trim().is_empty() {
            push(
                &mut candidates,
                NeighborCandidate {
                    concept_id: entry.concept_id.clone(),
                    canonical: entry.canonical.clone(),
                    reason: format!("定义近邻（相似度 {similarity:.2}）"),
                },
            );
            continue;
        }
        // 名字近形（互包含）。
        if names
            .iter()
            .any(|name| name_near_form(name, &nomination.name))
        {
            push(
                &mut candidates,
                NeighborCandidate {
                    concept_id: entry.concept_id.clone(),
                    canonical: entry.canonical.clone(),
                    reason: "名字近形（互包含）".into(),
                },
            );
        }
    }

    // 词面 top-k 补位（零分亦入）：候选集替评审保底可见面。
    for (score, entry) in word_face_scored.into_iter().take(WORD_FACE_TOP_K) {
        push(
            &mut candidates,
            NeighborCandidate {
                concept_id: entry.concept_id.clone(),
                canonical: entry.canonical.clone(),
                reason: format!("词面补位（共享二元组 {score}）"),
            },
        );
    }
    candidates
}

/// 归一化直定判定：提名定义归一化后与某登记表条目逐字相同 → 返回该条目
/// （直定宿主，不送裁决——learnhub new_aliases 处理链第一步）。
pub fn definition_exact_host<'a>(
    nomination: &Nomination,
    registry: &'a [RegistryEntry],
) -> Option<&'a RegistryEntry> {
    let normalized = normalize(&nomination.definition);
    if normalized.is_empty() {
        return None;
    }
    registry.iter().find(|entry| normalize(&entry.definition) == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(concept_id: &str, canonical: &str, definition: &str, aliases: &[&str]) -> RegistryEntry {
        RegistryEntry {
            concept_id: concept_id.to_owned(),
            canonical: canonical.to_owned(),
            definition: definition.to_owned(),
            active_aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
        }
    }

    #[test]
    fn normalize_folds_width_punct_and_case() {
        assert_eq!(normalize("Ｌｉｍｉｔｓ！"), "limits");
        assert_eq!(normalize("极限 （数学） "), "极限数学");
        assert_eq!(normalize("a，、b"), "ab");
    }

    #[test]
    fn exact_definition_direct_host() {
        let registry = vec![entry("c1", "极限", "无限逼近的值", &[])];
        let nomination = Nomination {
            name: "Limit".into(),
            definition: "无限逼近的值！".into(),
        };
        let host = definition_exact_host(&nomination, &registry).expect("direct host");
        assert_eq!(host.concept_id, "c1");
    }

    #[test]
    fn near_definition_and_name_shapes_enter_candidates() {
        let registry = vec![
            entry("c1", "极限", "无限逼近的值，描述函数趋近行为", &["Limits"]),
            entry("c2", "导数", "函数瞬时变化率", &[]),
        ];
        let nomination = Nomination {
            name: "极限概念".into(),
            definition: "无限逼近的值，描述函数趋近的极限行为".into(),
        };
        let candidates = assemble_neighbors(&nomination, &registry);
        let ids: Vec<&str> = candidates.iter().map(|candidate| candidate.concept_id.as_str()).collect();
        assert!(ids.contains(&"c1"), "near host must be a candidate: {candidates:?}");
        assert!(
            candidates
                .iter()
                .find(|candidate| candidate.concept_id == "c2")
                .is_some(),
            "word-face fallback keeps every entry visible (top-k)"
        );
    }

    #[test]
    fn name_near_form_requires_mutual_containment() {
        assert!(name_near_form("因式分解", "分解"));
        assert!(name_near_form("十字相乘法", "十字相乘"));
        assert!(!name_near_form("极限", "导数"));
    }

    #[test]
    fn exact_name_occupation_leads_the_candidates() {
        let registry = vec![entry("c1", "极限", "无限逼近的值", &["Limits"])];
        let nomination = Nomination {
            name: "limits".into(),
            definition: "别的东西".into(),
        };
        let candidates = assemble_neighbors(&nomination, &registry);
        assert_eq!(candidates[0].concept_id, "c1");
        assert!(candidates[0].reason.contains("精确占用"));
    }
}
