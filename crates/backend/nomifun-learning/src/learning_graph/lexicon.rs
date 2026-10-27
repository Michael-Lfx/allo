//! 定义用语表（定义接地的已知面；absorb-5 m9，learnhub #173 同源移植）。
//!
//! 三层已知面 = **登记表活跃名 ∪ 仓库 base（本文件嵌入）∪ 课程增词**：
//! - [`BASE_YAML`]：仓库级定义语言（跨课程复用；`lexicon/base.yaml`，人工
//!   维护、版本化；增词走仓库提交）——原样移植自 learnhub；
//! - 课程层：v1 存储从简，走 `learning_courses.structure_json` 的 `lexicon`
//!   旁键（`CourseLexicon`，缺席 = 空表合法）——只有**增词**（`words`）与
//!   **豁免**（`ignore`：片段含豁免词的点名结果视而不见）两个 section；
//!
//! 铁律：课程层**只能豁免与增词，改不了仓库 base**——base 是共享资产，
//! 课程旁键里写什么都污染不到它（结构上不可能：装配是单向并集）。
//!
//! 互钉：base 的 `function_single` 与 [`definition_audit::FUNCTION_CHARS`]
//! 是两处常量、无单一来源——由测试逐字互锁，改词表必须同步。

use serde::{Deserialize, Serialize};

/// 仓库级基础词表（编译期嵌入；人工维护，提交即生效）。
pub const BASE_YAML: &str = include_str!("../../lexicon/base.yaml");

/// 课程层在 `structure_json` 快照信封里的旁键名。
pub const COURSE_LEXICON_KEY: &str = "lexicon";

/// 仓库级基础词表（解析形态）。
#[derive(Debug, Clone, Deserialize)]
pub struct BaseLexicon {
    #[serde(default)]
    pub version: u32,
    /// 无歧义虚字单字（接地掩码；与 [`definition_audit::FUNCTION_CHARS`]
    /// 互钉——两处常量无单一来源，互锁由测试守护，改词表须同步）。
    #[serde(default)]
    pub function_single: String,
    #[serde(default)]
    pub words: Vec<String>,
}

/// 课程用语表（两 section；全部缺省合法）。铁律：只有豁免与增词，
/// 没有删除/改写仓库 base 的通道。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseLexicon {
    /// 课程增词（本课定义语言，旁键人工维护）。
    #[serde(default)]
    pub words: Vec<String>,
    /// 豁免：点名结果中含这些词的片段被视而不见（只影响读数，不动 base）。
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// 解析仓库 base（审计读数前调用；损坏 = fail loud——资产坏了不能悄悄放行）。
pub fn parse_base() -> Result<BaseLexicon, String> {
    serde_yaml::from_str(BASE_YAML).map_err(|e| format!("lexicon/base.yaml 损坏:{e}"))
}

/// 从 `structure_json` 快照信封读课程层（缺席/NULL/空白 = 空表合法；
/// 信封非法 JSON 或 `lexicon` 旁键形状不对 = fail loud）。
pub fn course_lexicon_from_snapshot(snapshot: Option<&str>) -> Result<CourseLexicon, String> {
    let Some(raw) = snapshot.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(CourseLexicon::default());
    };
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("structure_json 不是合法 JSON:{e}"))?;
    let Some(side) = value.get(COURSE_LEXICON_KEY) else {
        return Ok(CourseLexicon::default());
    };
    serde_json::from_value(side.clone())
        .map_err(|e| format!("structure_json 的 {COURSE_LEXICON_KEY} 旁键损坏:{e}"))
}

/// 旁键搬运原语：把已解析的课程层值贴到新信封上（缺席/Null 不贴——
/// 不伪造空表）。
pub fn attach_lexicon_side_key(envelope: &mut serde_json::Value, side: Option<&serde_json::Value>) {
    if let Some(side) = side.filter(|side| !side.is_null()) {
        envelope[COURSE_LEXICON_KEY] = side.clone();
    }
}

/// 快照重写时把课程层旁键带过去（`run_structure_generation` 重跑覆盖、
/// `run_structure_repair` 修补落盘都会整信封重建——旁键是课程层资产，
/// 结构产物换代不改变课程用语表）。
pub fn carry_lexicon_side_key(envelope: &mut serde_json::Value, previous_snapshot: Option<&str>) {
    let Ok(previous) = serde_json::from_str::<serde_json::Value>(
        previous_snapshot.unwrap_or_default().trim(),
    ) else {
        return;
    };
    attach_lexicon_side_key(envelope, previous.get(COURSE_LEXICON_KEY));
}

/// 接地检查的已知面 = base.words ∪ 课程 words（登记表活跃名由服务面另并；
/// 去重交由调用方）。课程层只增不删——这里没有从已知面移除任何词的通道。
pub fn grounded_words(base: &BaseLexicon, course: &CourseLexicon) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.extend(base.words.iter().cloned());
    out.extend(course.words.iter().cloned());
    out
}

/// 豁免过滤：片段含任一 ignore 词即丢弃（「视而不见」语义；空豁免词忽略）。
pub fn apply_ignores(fragments: &[String], course: &CourseLexicon) -> Vec<String> {
    fragments
        .iter()
        .filter(|f| {
            !course
                .ignore
                .iter()
                .any(|w| !w.is_empty() && f.contains(w.as_str()))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// base 资产可解析,且虚字表与 definition_audit 的掩码表逐字一致(互钉)。
    #[test]
    fn base_parses_and_function_single_matches_audit() {
        let base = parse_base().expect("base.yaml 应可解析");
        let _ = base.version;
        assert!(
            base.words.len() >= 100,
            "v0 应不少于 100 词:{}",
            base.words.len()
        );
        assert!(!base.function_single.is_empty());
        assert_eq!(
            base.function_single,
            crate::learning_graph::definition_audit::FUNCTION_CHARS,
            "base 虚字表与审计掩码表漂移"
        );
    }

    /// base 不含明显领域名词(领域词应挂号,不应进定义语言表)。
    #[test]
    fn base_excludes_domain_nouns() {
        let base = parse_base().expect("base.yaml 应可解析");
        for domain in [
            "线段",
            "内角",
            "三角形",
            "平均分",
            "分数",
            "有理数",
            "论点",
            "实体",
            "所有权",
            "借用",
        ] {
            assert!(
                !base.words.iter().any(|w| w == domain),
                "领域名词「{domain}」不得进基础词表(应挂号由课程教)"
            );
        }
    }

    /// 旧快照兼容:NULL/空白/无旁键的旧信封 = 空课程层;旁键三形读回。
    #[test]
    fn course_lexicon_reads_legacy_snapshots_and_side_key() {
        assert_eq!(
            course_lexicon_from_snapshot(None).unwrap(),
            CourseLexicon::default(),
            "NULL = 空表(旧课程)"
        );
        assert_eq!(
            course_lexicon_from_snapshot(Some("  ")).unwrap(),
            CourseLexicon::default()
        );
        // 无旁键的旧信封(既有结构产物) = 空表。
        let legacy = serde_json::json!({
            "version": 1, "generated_at": 1, "structure": {}, "terrain": {},
        })
        .to_string();
        assert_eq!(
            course_lexicon_from_snapshot(Some(&legacy)).unwrap(),
            CourseLexicon::default(),
            "旧快照无旁键 = 空表"
        );
        // 带旁键的信封读回两 section。
        let with_side = serde_json::json!({
            "version": 1, "structure": {}, "terrain": {},
            "lexicon": { "words": ["竖式"], "ignore": ["确定"] },
        })
        .to_string();
        let course = course_lexicon_from_snapshot(Some(&with_side)).unwrap();
        assert_eq!(course.words, vec!["竖式".to_string()]);
        assert_eq!(course.ignore, vec!["确定".to_string()]);
        // 损坏 = fail loud(信封非法 / 旁键形状不对)。
        assert!(course_lexicon_from_snapshot(Some("not json")).is_err());
        let bad_side = serde_json::json!({ "lexicon": { "words": "不是数组" } }).to_string();
        assert!(course_lexicon_from_snapshot(Some(&bad_side)).is_err());
    }

    /// 快照换代带旁键:重跑/修补整信封重建时课程层不丢;旧快照无旁键则不加。
    #[test]
    fn carry_lexicon_side_key_survives_snapshot_rewrite() {
        let previous = serde_json::json!({
            "version": 1, "structure": {}, "terrain": {},
            "lexicon": { "words": ["竖式"], "ignore": [] },
        })
        .to_string();
        let mut envelope = serde_json::json!({ "version": 1, "structure": {}, "terrain": {} });
        carry_lexicon_side_key(&mut envelope, Some(&previous));
        assert_eq!(
            envelope.get(COURSE_LEXICON_KEY).unwrap(),
            &serde_json::json!({ "words": ["竖式"], "ignore": [] }),
            "换代后旁键保留"
        );
        // 旧快照无旁键:新信封不加键(不伪造空表)。
        let legacy = serde_json::json!({ "version": 1, "structure": {} }).to_string();
        let mut envelope = serde_json::json!({ "version": 2, "structure": {} });
        carry_lexicon_side_key(&mut envelope, Some(&legacy));
        assert!(envelope.get(COURSE_LEXICON_KEY).is_none());
        // NULL 快照:同上。
        let mut envelope = serde_json::json!({ "version": 2, "structure": {} });
        carry_lexicon_side_key(&mut envelope, None);
        assert!(envelope.get(COURSE_LEXICON_KEY).is_none());
    }

    /// 已知面组装:base ∪ 课程词并集(课程层只增;无删除通道——铁律的
    /// 结构面:任何课程旁键都造不出「从已知面减词」)。
    #[test]
    fn grounded_words_unions_base_and_course_layers() {
        let base = BaseLexicon {
            version: 0,
            function_single: String::new(),
            words: vec!["去掉".into()],
        };
        let course = CourseLexicon {
            words: vec!["去掉".into(), "竖式".into()],
            ignore: vec!["确定".into()],
        };
        let mut known = grounded_words(&base, &course);
        known.sort();
        known.dedup();
        assert_eq!(
            known,
            vec!["去掉".to_string(), "竖式".to_string()],
            "base ∪ 课程增词(去重);ignore 不进已知面"
        );
    }

    /// 课程豁免语义:含豁免词的片段被视而不见;不含的照常点名。
    #[test]
    fn apply_ignores_hides_only_matching_fragments() {
        let course = CourseLexicon {
            words: vec![],
            ignore: vec!["确定".into(), "".into()],
        };
        let frags = vec!["线段".to_string(), "确定的说明".to_string()];
        let filtered = apply_ignores(&frags, &course);
        assert_eq!(
            filtered,
            vec!["线段".to_string()],
            "含豁免词的片段被视而不见,其余照常;空豁免词忽略"
        );
    }

    /// 豁免铁律:课程层豁免/增词都不反噬仓库 base——base 原样解析结果
    /// 与课程旁键内容完全无关(结构上单向并集,这里钉死读数口径)。
    #[test]
    fn course_layer_never_mutates_base() {
        let base_before = parse_base().unwrap();
        let course = CourseLexicon {
            words: vec!["线段".into()],
            ignore: vec!["去掉".into()],
        };
        let _ = grounded_words(&base_before, &course);
        let base_after = parse_base().unwrap();
        assert_eq!(
            base_before.words, base_after.words,
            "base 词表与课程层无关"
        );
    }
}
