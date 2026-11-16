//! 建课时点的来源计划快照信封（t4）：t1 清点/分诊 + t2 计划 + t3 展开覆盖
//! 装进**一个版本化 JSON 对象**落 `learning_courses.course_source_plan_json`
//! （迁移 070）。字段演进靠信封 `version`——读侧对未知版本降级
//! `available=false`，不崩溃、不伪造空计划；NULL = 旧课程/描述流/未走新
//! 管线（回退采样路径建的课程不伪造快照）。
//!
//! 体积控制在序列化入口统一截断（[`CourseSourcePlanSnapshot::capture`]），
//! **不改 t1–t3 内存类型的语义**：内存 preview ≤512 字（t1 清点上限），
//! 快照再压到 ≤200 字；headings 内存 ≤32 条，快照压到 ≤16 条。截断事件
//! 聚合成信封级 `warnings`（不静默）。`plan`/`coverage` 本就不带 lesson
//! 正文（只有结构化骨架 + ≤120 字锚点片段），审计需要全文时按 `rel_path`
//! 回 KB 读。

use super::structure::SourcePlan;
use super::expand::SourceCoverage;
use super::triage::{KbInventory, KbTriageReport};

/// 快照信封版本（v1 = t1–t3 冻结形状）。加字段/改语义时递增，读侧对未知
/// 版本降级（见模块注释）。
pub const COURSE_SOURCE_PLAN_SNAPSHOT_VERSION: u32 = 1;

/// 快照里单文件正文预览上限（票面：preview ≤200 字；内存口径是 512）。
pub const SNAPSHOT_PREVIEW_MAX_CHARS: usize = 200;
/// 快照里单文件 headings 条数上限（票面：数量设上限；内存口径是 32）。
pub const SNAPSHOT_HEADINGS_MAX: usize = 16;

/// 建课时点的来源计划快照（`course_source_plan_json` 列的信封形状）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CourseSourcePlanSnapshot {
    pub version: u32,
    pub inventory: KbInventory,
    pub triage: KbTriageReport,
    pub plan: SourcePlan,
    pub coverage: SourceCoverage,
    /// 快照级披露（体积控制的截断事件等）；t1–t3 各自的 warnings 保留在
    /// 对应段落里。
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl CourseSourcePlanSnapshot {
    /// 组装 v1 信封：克隆 t1–t3 产物并对 inventory 做体积控制截断（超限
    /// 聚合进 `warnings`，不静默）。调用方只在建课确实走了「计划 → 展开」
    /// 管线（四件套齐全）时调用；描述流/回退路径写 NULL，不伪造快照。
    pub fn capture(
        inventory: &KbInventory,
        triage: &KbTriageReport,
        plan: &SourcePlan,
        coverage: &SourceCoverage,
    ) -> Self {
        let mut warnings = Vec::new();
        let mut inventory = inventory.clone();
        let mut preview_truncated = 0usize;
        let mut headings_truncated = 0usize;
        for file in &mut inventory.files {
            let preview_len = file.preview.chars().count();
            if preview_len > SNAPSHOT_PREVIEW_MAX_CHARS {
                file.preview = file.preview.chars().take(SNAPSHOT_PREVIEW_MAX_CHARS).collect();
                preview_truncated += 1;
            }
            if file.headings.len() > SNAPSHOT_HEADINGS_MAX {
                file.headings.truncate(SNAPSHOT_HEADINGS_MAX);
                headings_truncated += 1;
            }
        }
        if preview_truncated > 0 {
            warnings.push(format!(
                "快照体积控制:{preview_truncated} 个文件的 preview 截断至 {SNAPSHOT_PREVIEW_MAX_CHARS} 字(内存口径 512 字)"
            ));
        }
        if headings_truncated > 0 {
            warnings.push(format!(
                "快照体积控制:{headings_truncated} 个文件的 headings 截断至 {SNAPSHOT_HEADINGS_MAX} 条(内存口径 32 条)"
            ));
        }
        Self {
            version: COURSE_SOURCE_PLAN_SNAPSHOT_VERSION,
            inventory,
            triage: triage.clone(),
            plan: plan.clone(),
            coverage: coverage.clone(),
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::{
        KbFileKind, KbInventoryBase, KbInventoryFile, KbScanScope, KbTriageCounts,
    };

    fn inventory_file(rel_path: &str, preview: String, headings: Vec<String>) -> KbInventoryFile {
        KbInventoryFile {
            rel_path: rel_path.to_owned(),
            kb_id: "kb-1".to_owned(),
            size: 1024,
            modified_at: Some(1),
            content_hash: "h".repeat(64),
            headings,
            heading_levels: Vec::new(),
            preview,
            kind: KbFileKind::Markdown,
        }
    }

    fn empty_triage() -> KbTriageReport {
        KbTriageReport {
            files: Vec::new(),
            counts: KbTriageCounts::default(),
            excluded: Vec::new(),
            duplicates: Vec::new(),
            degraded: false,
            warnings: Vec::new(),
            batch_repair_count: 0,
            still_missing_count: 0,
        }
    }

    /// 体积控制（票面验收）：超限 preview 截到 ≤200 字、headings 截到
    /// ≤16 条，截断事件进信封 warnings（不静默）；**内存类型语义不改**——
    /// 传入的 inventory 原样（截断只发生在快照副本上）。
    #[test]
    fn capture_truncates_oversized_inventory_fields_and_records_warnings() {
        let long_preview = "长".repeat(600);
        let many_headings: Vec<String> = (0..40).map(|index| format!("标题{index}")).collect();
        let inventory = KbInventory {
            bases: vec![KbInventoryBase {
                kb_id: "kb-1".to_owned(),
                name: "教材".to_owned(),
                namespace: String::new(),
                total_files: 1,
            }],
            files: vec![inventory_file("ch01.md", long_preview.clone(), many_headings.clone())],
            total_files: 1,
            total_bytes: 1024,
            skipped_hidden: 0,
            scan_scope: KbScanScope::Markdown,
        };
        let snapshot = CourseSourcePlanSnapshot::capture(
            &inventory,
            &empty_triage(),
            &SourcePlan::default(),
            &SourceCoverage::default(),
        );

        let stored = &snapshot.inventory.files[0];
        assert!(
            stored.preview.chars().count() <= SNAPSHOT_PREVIEW_MAX_CHARS,
            "快照 preview 截到 ≤{} 字,实际 {} 字",
            SNAPSHOT_PREVIEW_MAX_CHARS,
            stored.preview.chars().count()
        );
        assert!(
            stored.headings.len() <= SNAPSHOT_HEADINGS_MAX,
            "快照 headings 截到 ≤{} 条,实际 {} 条",
            SNAPSHOT_HEADINGS_MAX,
            stored.headings.len()
        );
        assert_eq!(stored.headings, &many_headings[..SNAPSHOT_HEADINGS_MAX]);
        assert!(
            !snapshot.warnings.is_empty(),
            "截断必须记进信封 warnings,不静默"
        );

        // 内存类型语义不改:t1 清点的原值原样保留(512 字/32 条口径)。
        assert_eq!(inventory.files[0].preview, long_preview);
        assert_eq!(inventory.files[0].headings.len(), 40);
    }

    /// h1 计数字段的信封兼容：零值不序列化（快照字节形状与 v1 完全一致），
    /// 缺字段的旧快照读回默认 0——读侧不感知、不报错。
    #[test]
    fn triage_counter_fields_stay_backward_compatible() {
        let empty_inventory = KbInventory {
            bases: Vec::new(),
            files: Vec::new(),
            total_files: 0,
            total_bytes: 0,
            skipped_hidden: 0,
            scan_scope: KbScanScope::Markdown,
        };
        let snapshot = CourseSourcePlanSnapshot::capture(
            &empty_inventory,
            &empty_triage(),
            &SourcePlan::default(),
            &SourceCoverage::default(),
        );
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(
            json["triage"].get("batch_repair_count").is_none(),
            "零值不序列化，信封字节形状与 v1 一致：{json}"
        );
        assert!(
            json["triage"].get("still_missing_count").is_none(),
            "零值不序列化：{json}"
        );
        // 旧快照（缺新字段）原样读回：默认 0，不报错不伪造。
        let parsed: CourseSourcePlanSnapshot =
            serde_json::from_value(json).expect("缺字段的快照可读回");
        assert_eq!(parsed.triage.batch_repair_count, 0);
        assert_eq!(parsed.triage.still_missing_count, 0);
    }

    /// 信封契约(票面验收):序列化恒为 JSON 对象(迁移 070 的 CHECK 只放行
    /// 对象)、`version:1`;1000 文件库的快照体积可控(截断生效,单文件
    /// ≈ 路径 + hash + 200 字 preview + 16 条标题 ≈ 1KB → 总量 < 2MB);
    /// 可完整往返。正文不落快照:全文只以 ≤200 字 preview 的形式出现。
    #[test]
    fn envelope_serializes_as_a_bounded_version_one_object() {
        let files: Vec<KbInventoryFile> = (0..1000)
            .map(|index| {
                inventory_file(
                    &format!("docs/chapter-{index:04}.md"),
                    "预".repeat(400),
                    (0..32).map(|h| format!("第{h}节 要点")).collect(),
                )
            })
            .collect();
        let inventory = KbInventory {
            bases: vec![KbInventoryBase {
                kb_id: "kb-1".to_owned(),
                name: "教材".to_owned(),
                namespace: String::new(),
                total_files: files.len(),
            }],
            files,
            total_files: 1000,
            total_bytes: 1024 * 1000,
            skipped_hidden: 0,
            scan_scope: KbScanScope::Markdown,
        };
        let snapshot = CourseSourcePlanSnapshot::capture(
            &inventory,
            &empty_triage(),
            &SourcePlan::default(),
            &SourceCoverage::default(),
        );

        let json = serde_json::to_string(&snapshot).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.is_object(), "迁移 070 CHECK 只放行 JSON 对象");
        assert_eq!(value["version"], serde_json::json!(1));
        assert!(
            json.len() < 2_000_000,
            "1000 文件库快照体积需可控,实际 {} 字节",
            json.len()
        );
        // 正文只以 ≤200 字 preview 出现(内存 400 字被截)。
        let stored_preview = value["inventory"]["files"][0]["preview"]
            .as_str()
            .unwrap();
        assert_eq!(stored_preview.chars().count(), SNAPSHOT_PREVIEW_MAX_CHARS);
        let stored_headings = value["inventory"]["files"][0]["headings"]
            .as_array()
            .unwrap();
        assert_eq!(stored_headings.len(), SNAPSHOT_HEADINGS_MAX);

        // 完整往返:信封可原样读回(读侧解析的前提)。
        let parsed: CourseSourcePlanSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, COURSE_SOURCE_PLAN_SNAPSHOT_VERSION);
        assert_eq!(parsed.inventory.total_files, 1000);
        assert!(!parsed.warnings.is_empty(), "截断事件随快照落库");
    }
}
