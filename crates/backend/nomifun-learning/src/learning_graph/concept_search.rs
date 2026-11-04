//! 概念登记表只读检索（y03：learnhub `contentgen/search_concepts.rs` 的
//! 四通道移植·零写侧）。
//!
//! - **四通道分级**（等级由高到低）：`exact`（canonical 大小写不敏感精确）
//!   → `alias`（活跃别名精确）→ `nearname`（[`defnear::name_near_form`]
//!   ∪ [`defnear::stem_similarity`] ≥ [`defnear::STEM_SIMILARITY_THRESHOLD`]，
//!   近名机械单源，不写第二套相似度）→ `fulltext`（definition 包含 query，
//!   大小写不敏感）。
//! - **去重**：同 concept_id 取最高等级，同级取最高 score；按等级 → score
//!   降序 → concept_id 升序，截 [`SEARCH_TOP_K`]。
//! - **零命中**：`hits: []` + `not_found: true`——不抛错，模型拿结构化裁定
//!   自行研判（自查赋权不是新门）。
//!
//! 纯函数、零 IO：DB 只负责取行（别名池的 doc_version 过滤在服务层），
//! 排序/去重/通道判定全在本模块，可脱离数据库单测。

use crate::models::{ConceptSearchHit, ConceptSearchResults};

use super::defnear::{RegistryEntry, name_near_form, stem_similarity, STEM_SIMILARITY_THRESHOLD};

/// 检索结果条数上限（票面 top-5）。
pub const SEARCH_TOP_K: usize = 5;

/// 命中通道名（与 [`ConceptSearchHit::matched_by`] 的序列化值一致）。
pub const MATCHED_BY_EXACT: &str = "exact";
pub const MATCHED_BY_ALIAS: &str = "alias";
pub const MATCHED_BY_NEARNAME: &str = "nearname";
pub const MATCHED_BY_FULLTEXT: &str = "fulltext";

/// 通道分级序（枚举序 = 优先级序）：exact > alias > nearname > fulltext。
pub fn grade_rank(matched_by: &str) -> u8 {
    match matched_by {
        MATCHED_BY_EXACT => 0,
        MATCHED_BY_ALIAS => 1,
        MATCHED_BY_NEARNAME => 2,
        _ => 3,
    }
}

/// 检索（search 模式）：四通道全量扫描一趟，按 concept_id 去重取最高等级，
/// 排序后截 top-5。空 query 由服务层挡（400）；本函数对空 query 返回零命中。
pub fn search_hits(registry: &[RegistryEntry], query: &str) -> ConceptSearchResults {
    let hits = rank_hits(registry, query);
    let not_found = hits.is_empty();
    ConceptSearchResults { hits, not_found }
}

fn rank_hits(registry: &[RegistryEntry], query: &str) -> Vec<ConceptSearchHit> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    let mut hits: Vec<ConceptSearchHit> = registry
        .iter()
        .filter_map(|entry| match_grade(entry, query, &query_lower))
        .collect();
    hits.sort_by(|left, right| {
        grade_rank(&left.matched_by)
            .cmp(&grade_rank(&right.matched_by))
            .then_with(|| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.concept_id.cmp(&right.concept_id))
    });
    hits.truncate(SEARCH_TOP_K);
    hits
}

/// 单概念的最高命中通道（exact > alias > nearname > fulltext；首个命中即
/// 返回——等级已按优先级检查，去重天然成立）。
fn match_grade(
    entry: &RegistryEntry,
    query: &str,
    query_lower: &str,
) -> Option<ConceptSearchHit> {
    // 1 canonical 精确（大小写不敏感）。
    if entry.canonical.trim().to_lowercase() == query_lower {
        return Some(hit(entry, MATCHED_BY_EXACT, 1.0));
    }
    // 2 活跃别名精确。
    if entry
        .active_aliases
        .iter()
        .any(|alias| alias.trim().to_lowercase() == query_lower)
    {
        return Some(hit(entry, MATCHED_BY_ALIAS, 1.0));
    }
    // 3 近名：名字近形（互包含）∪ 名字相似分过阈值——机械单源在 defnear。
    let names: Vec<&str> = std::iter::once(entry.canonical.as_str())
        .chain(entry.active_aliases.iter().map(String::as_str))
        .filter(|name| !name.trim().is_empty())
        .collect();
    let near_score = names
        .iter()
        .map(|name| stem_similarity(query, name))
        .fold(f64::NEG_INFINITY, f64::max);
    if names.iter().any(|name| name_near_form(name, query))
        || near_score >= STEM_SIMILARITY_THRESHOLD
    {
        return Some(hit(entry, MATCHED_BY_NEARNAME, near_score.max(0.0)));
    }
    // 4 全文兜底：definition 包含 query（大小写不敏感）。
    let definition_lower = entry.definition.to_lowercase();
    if !query_lower.is_empty() && definition_lower.contains(query_lower) {
        let score = (query.chars().count() as f64
            / entry.definition.chars().count().max(1) as f64)
            .min(1.0);
        return Some(hit(entry, MATCHED_BY_FULLTEXT, score));
    }
    None
}

fn hit(entry: &RegistryEntry, matched_by: &str, score: f64) -> ConceptSearchHit {
    ConceptSearchHit {
        concept_id: entry.concept_id.clone(),
        canonical: entry.canonical.clone(),
        aliases: entry.active_aliases.clone(),
        definition: entry.definition.clone(),
        matched_by: matched_by.to_owned(),
        score,
    }
}

/// detail 模式命中：按 concept_id 取到的行 → 完整定义 + 全部活跃别名
/// （不受 top-5 限制；matched_by 记 exact——按 id 精确取行）。
pub fn detail_hit(entry: &RegistryEntry) -> ConceptSearchHit {
    hit(entry, MATCHED_BY_EXACT, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0190f5fe-7c00-7a00-8abc-111111111111";
    const B: &str = "0190f5fe-7c00-7a00-8abc-222222222222";

    fn entry(concept_id: &str, canonical: &str, definition: &str, aliases: &[&str]) -> RegistryEntry {
        RegistryEntry {
            concept_id: concept_id.to_owned(),
            canonical: canonical.to_owned(),
            definition: definition.to_owned(),
            active_aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
        }
    }

    fn concept_ids(results: &ConceptSearchResults) -> Vec<&str> {
        results.hits.iter().map(|hit| hit.concept_id.as_str()).collect()
    }

    #[test]
    fn exact_canonical_hit_is_case_insensitive_and_carries_aliases() {
        let registry = vec![entry(A, "Limits", "函数趋近的值", &["Limit"])];
        let results = search_hits(&registry, "limits");
        assert!(!results.not_found);
        assert_eq!(results.hits.len(), 1);
        let hit = &results.hits[0];
        assert_eq!(hit.concept_id, A);
        assert_eq!(hit.canonical, "Limits");
        assert_eq!(hit.aliases, vec!["Limit".to_owned()], "活跃别名随命中返回");
        assert_eq!(hit.matched_by, MATCHED_BY_EXACT);
        assert_eq!(hit.score, 1.0);
    }

    #[test]
    fn active_alias_is_transparent_and_aliases_rank_below_exact() {
        let registry = vec![
            entry(A, "极限", "无限逼近的值", &["Limits"]),
            entry(B, "导数", "瞬时变化率", &[]),
        ];
        let alias_hit = search_hits(&registry, "LIMITS");
        assert_eq!(alias_hit.hits.len(), 1);
        assert_eq!(alias_hit.hits[0].concept_id, A);
        assert_eq!(alias_hit.hits[0].matched_by, MATCHED_BY_ALIAS);
        assert_eq!(alias_hit.hits[0].score, 1.0);

        // 跨概念排序：canonical 精确在别名命中之前。
        let mut borrowed = entry(B, "借用", "值的临时访问", &[]);
        borrowed.active_aliases = vec!["极限".into()];
        let results = search_hits(&[entry(A, "极限", "无限逼近的值", &[]), borrowed], "极限");
        assert_eq!(results.hits.len(), 2);
        assert_eq!(results.hits[0].concept_id, A);
        assert_eq!(results.hits[0].matched_by, MATCHED_BY_EXACT);
        assert_eq!(results.hits[1].concept_id, B);
        assert_eq!(results.hits[1].matched_by, MATCHED_BY_ALIAS);
    }

    #[test]
    fn name_near_form_is_a_nearname_hit() {
        let registry = vec![entry(A, "因式分解", "把多项式拆成因子之积", &[])];
        let results = search_hits(&registry, "分解");
        assert_eq!(results.hits.len(), 1, "互包含近名必须命中：{results:?}");
        assert_eq!(results.hits[0].matched_by, MATCHED_BY_NEARNAME);
        assert!(results.hits[0].score > 0.0);
    }

    #[test]
    fn stem_similarity_at_threshold_is_a_nearname_hit() {
        // 「因式分解法」对「因式分解」的 trigram 包含度 = 1.0（≥ 阈值），
        // 近名机械来自 defnear 单源。
        let registry = vec![entry(A, "因式分解", "把多项式拆成因子之积", &[])];
        let results = search_hits(&registry, "因式分解法");
        assert_eq!(results.hits.len(), 1, "{results:?}");
        assert_eq!(results.hits[0].matched_by, MATCHED_BY_NEARNAME);
        assert!(
            results.hits[0].score >= STEM_SIMILARITY_THRESHOLD,
            "阈值分支的分数应过线：{}",
            results.hits[0].score
        );
    }

    #[test]
    fn nearname_hits_sort_by_score_desc() {
        let registry = vec![
            entry(A, "因式分解", "把多项式拆成因子之积", &[]),
            entry(B, "分解", "拆开", &[]),
        ];
        let results = search_hits(&registry, "因式分解法");
        assert_eq!(results.hits.len(), 2, "{results:?}");
        assert_eq!(results.hits[0].concept_id, A);
        assert_eq!(results.hits[0].matched_by, MATCHED_BY_NEARNAME);
        assert_eq!(results.hits[1].matched_by, MATCHED_BY_NEARNAME);
        assert!(
            results.hits[0].score > results.hits[1].score,
            "同级按 score 降序：{:?}",
            results.hits
        );
    }

    #[test]
    fn definition_substring_is_the_fulltext_fallback() {
        let registry = vec![entry(A, "所有权", "悬垂指针是所有权误用的代价", &[])];
        let results = search_hits(&registry, "悬垂");
        assert_eq!(results.hits.len(), 1);
        assert_eq!(results.hits[0].matched_by, MATCHED_BY_FULLTEXT);
        assert!(
            results.hits[0].score > 0.0 && results.hits[0].score <= 1.0,
            "包含度封顶 1：{}",
            results.hits[0].score
        );
    }

    #[test]
    fn definition_substring_is_case_insensitive() {
        let registry = vec![entry(A, "所有权", "Rust 的所有权规则", &[])];
        let results = search_hits(&registry, "rust");
        assert_eq!(results.hits.len(), 1);
        assert_eq!(results.hits[0].matched_by, MATCHED_BY_FULLTEXT);
    }

    #[test]
    fn dedup_keeps_the_highest_grade_per_concept() {
        // 「所有权」同时命中 exact（canonical）与 fulltext（定义子串）——
        // 同概念只出一条，取最高级。
        let registry = vec![entry(A, "所有权", "悬垂指针是所有权误用的代价", &[])];
        let results = search_hits(&registry, "所有权");
        assert_eq!(results.hits.len(), 1, "{results:?}");
        assert_eq!(results.hits[0].matched_by, MATCHED_BY_EXACT);
    }

    #[test]
    fn ranking_caps_at_top5() {
        let six: Vec<RegistryEntry> = (0..6)
            .map(|i| {
                entry(
                    &format!("0190f5fe-7c00-7a00-8abc-00000000000{i}"),
                    &format!("概念{i}"),
                    "矩阵是核心对象",
                    &[],
                )
            })
            .collect();
        let results = search_hits(&six, "矩阵");
        assert_eq!(results.hits.len(), SEARCH_TOP_K, "六条全文命中截 top-5");
        assert!(results.hits.iter().all(|hit| hit.matched_by == MATCHED_BY_FULLTEXT));
    }

    #[test]
    fn zero_hit_reports_not_found_without_error() {
        let registry = vec![entry(A, "所有权", "值的所有权规则", &[])];
        let results = search_hits(&registry, "量子纠缠");
        assert!(results.hits.is_empty());
        assert!(results.not_found);
    }

    #[test]
    fn detail_hit_returns_full_definition_and_all_active_aliases() {
        let mut record = entry(A, "极限", &"长定义".repeat(100), &[]);
        record.active_aliases = (1..=6).map(|i| format!("别名{i}")).collect();
        let hit = detail_hit(&record);
        assert_eq!(hit.concept_id, A);
        assert_eq!(hit.canonical, "极限");
        assert_eq!(hit.matched_by, MATCHED_BY_EXACT);
        assert_eq!(hit.aliases.len(), 6, "detail 不受 top-5 限制");
        assert_eq!(hit.definition, "长定义".repeat(100), "全定义不截断");
    }
}
