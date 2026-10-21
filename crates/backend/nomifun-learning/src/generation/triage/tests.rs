use super::*;
use crate::completer::LearningCompleter;
use nomifun_common::AppError;
use nomifun_realtime::UserEventSink;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct NoopBroadcaster;

impl UserEventSink for NoopBroadcaster {
    fn send_to_user(&self, _user_id: &str, _event: nomifun_api_types::WebSocketMessage<serde_json::Value>) {
    }
}

/// 内存库 + 真实 SQLite 仓储的 KnowledgeService（照 `service/tests.rs` 的
/// `job_test_service` 装配，只保留清点/分诊需要的 knowledge 侧）。
async fn triage_knowledge_service() -> (Arc<KnowledgeService>, tempfile::TempDir) {
    let data_dir = tempfile::tempdir().unwrap();
    let database = nomifun_db::init_database_memory().await.unwrap();
    let owner_id = nomifun_db::installation_owner_id(database.pool())
        .await
        .unwrap();
    let knowledge = Arc::new(KnowledgeService::new(
        Arc::new(nomifun_db::SqliteKnowledgeRepository::new(
            database.pool().clone(),
        )),
        data_dir.path(),
        nomifun_knowledge::KnowledgeEventEmitter::new(Arc::new(NoopBroadcaster), Arc::from(owner_id)),
    ));
    (knowledge, data_dir)
}

/// 建一个受管空库并返回 (kb_id, root_path)——测试直接往 root 写 markdown。
async fn create_blank_base(knowledge: &KnowledgeService, name: &str) -> (KnowledgeBaseId, String) {
    let outcome = knowledge
        .quick_create_base(Some(name), None, "blank", None, None, None, None)
        .await
        .unwrap();
    (
        outcome.base.knowledge_base_id.parse().unwrap(),
        outcome.base.root_path.clone(),
    )
}

/// 往受管库根写一个 markdown 文件（建目录、写内容）。
fn write_kb_file(root: &str, rel: &str, content: &str) {
    let path = std::path::PathBuf::from(root).join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// 占位 completer：调用即失败（分诊调用失败 → 规则降级的夹具）。
struct FailingCompleter;

#[async_trait::async_trait]
impl LearningCompleter for FailingCompleter {
    async fn complete(
        &self,
        _model_override: Option<(&str, &str)>,
        _system: &str,
        _user: &str,
        _max_tokens: u32,
    ) -> Result<String, AppError> {
        Err(AppError::Internal("completer unavailable in triage tests".into())
        )
    }
}

#[tokio::test]
async fn triage_inventories_every_markdown_file_and_degrades_to_core_without_a_completer() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "代数笔记").await;
    write_kb_file(
        &root,
        "notes/equations.md",
        "# 一元二次方程\n\n## 配方法\n\n把一般形式配成完全平方。\n\n## 判别式\n\n用根的判别式判断根的情况。\n",
    );
    write_kb_file(&root, "notes/geometry.md", "# 圆的性质\n\n垂径定理与圆周角。\n");
    write_kb_file(&root, "ignored.txt", "not markdown");

    let (inventory, report) =
        run_kb_triage(&knowledge, &[kb_id.clone()], "初中代数：一元二次方程", None)
            .await
            .unwrap();

    // 清点面：v1 只覆盖 Markdown 文档面，且如实披露。
    assert_eq!(inventory.total_files, 2, "非 markdown 不入清点");
    assert_eq!(inventory.files.len(), 2);
    assert!(inventory.total_bytes > 0);
    assert!(
        report.warnings.iter().any(|warning| warning.contains("Markdown")),
        "仅扫描 Markdown 必须披露，不能静默忽略：{:?}",
        report.warnings
    );
    let equations = inventory
        .files
        .iter()
        .find(|file| file.rel_path == "notes/equations.md")
        .expect("single-KB rel_path 是库内相对路径");
    assert_eq!(equations.kb_id, kb_id.as_str());
    assert!(equations.size > 0);
    assert!(equations.modified_at.is_some());
    assert_eq!(equations.content_hash.len(), 64, "sha256 十六进制");
    assert_eq!(
        equations.headings,
        ["一元二次方程", "配方法", "判别式"],
        "H1-H3 标题（跳过围栏外噪声）"
    );
    assert!(equations.preview.contains("完全平方"), "preview: {}", equations.preview);
    assert_eq!(equations.kind, KbFileKind::Markdown);

    // 分诊面：无 completer → 规则降级，非噪声全部 core，不阻断。
    assert!(report.degraded);
    assert_eq!(report.files.len(), inventory.total_files, "报告覆盖清点的每个文件（总数守恒）");
    assert_eq!(report.counts.core, 2);
    for file in &report.files {
        assert_eq!(file.role, KbTriageRole::Core);
        assert!(!file.reason.is_empty(), "降级 core 也要有理由");
    }
    assert_eq!(report.candidates().len(), 2, "降级时非噪声文件全部进候选");
}

#[tokio::test]
async fn triage_degrades_to_core_when_the_completer_call_fails() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "物理笔记").await;
    write_kb_file(&root, "optics.md", "# 几何光学\n\n光的折射定律。\n");
    let completer = FailingCompleter;
    let (inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "几何光学入门", Some(&completer)).await.unwrap();
    assert!(report.degraded, "调用失败 → 规则兜底 + degraded");
    assert_eq!(report.counts.core, 1);
    assert_eq!(inventory.total_files, 1);
}

/// 显式固定 mtime（canonical 规则需要确定性的新旧信号，不依赖写入时序）。
fn set_mtime(root: &str, rel: &str, when: std::time::SystemTime) {
    let path = std::path::PathBuf::from(root).join(rel);
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(when))
        .unwrap();
}

fn at_secs(secs: u64) -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
}

#[tokio::test]
async fn rule_triage_sorts_metadata_empty_and_navigation_files_into_noise() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "运维手册").await;
    write_kb_file(&root, "LICENSE.md", "MIT License\n\nPermission is granted, free of charge, to any person obtaining a copy.\n");
    write_kb_file(&root, "docs/contributing.md", "# Contributing\n\n请遵循提交规范：先跑测试再提交。\n");
    write_kb_file(&root, "empty.md", "   \n\n");
    write_kb_file(&root, "toc.md", "# 目录\n\n- [第一章 部署](chapter1.md)\n- [第二章 监控](chapter2.md)\n");
    write_kb_file(&root, "chapter1.md", "# 第一章 部署\n\n蓝绿部署把流量逐步切到新版本，先摘流量再发布。\n");

    let (inventory, report) = run_kb_triage(&knowledge, &[kb_id], "部署手册", None).await.unwrap();

    assert_eq!(inventory.total_files, 5);
    assert_eq!(report.files.len(), 5, "总数守恒：所有文件都出现在报告中");
    assert_eq!(report.counts.noise, 4);
    assert_eq!(report.counts.core, 1, "正文章节在降级模式下不被误杀为 noise");
    assert_eq!(report.excluded.len(), 4, "noise 全部进 excluded");
    for exclusion in &report.excluded {
        assert!(!exclusion.reason.is_empty(), "每个排除项必须有理由：{exclusion:?}");
    }
    let license = report
        .files
        .iter()
        .find(|file| file.rel_path == "LICENSE.md")
        .unwrap();
    assert_eq!(license.role, KbTriageRole::Noise);
    assert!(license.reason.contains("元数据"), "{}", license.reason);
    let toc = report.files.iter().find(|file| file.rel_path == "toc.md").unwrap();
    assert_eq!(toc.role, KbTriageRole::Noise);
    assert!(toc.reason.contains("导航"), "{}", toc.reason);
    assert_eq!(report.candidates().len(), 1);
}

#[tokio::test]
async fn identical_content_files_keep_the_newest_as_canonical() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "项目文档").await;
    let content = "# 使用指南\n\n第一步安装依赖，第二步运行迁移。\n";
    write_kb_file(&root, "guide.md", content);
    write_kb_file(&root, "archive/guide-old.md", content);
    set_mtime(&root, "archive/guide-old.md", at_secs(1_700_000_000));
    set_mtime(&root, "guide.md", at_secs(1_700_100_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "上手该项目", None).await.unwrap();

    assert_eq!(report.counts.duplicate, 1);
    assert_eq!(report.counts.core, 1);
    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "archive/guide-old.md");
    assert_eq!(duplicate.duplicate_of, "guide.md");
    assert!(
        duplicate.canonical_reason.contains("最新"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
    let excluded_paths: Vec<&str> = report.excluded.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(excluded_paths, ["archive/guide-old.md"]);
}

#[tokio::test]
async fn translation_suffix_copies_point_back_to_the_original() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "课程库").await;
    write_kb_file(&root, "guide.md", "# 指南\n\n中文原文正文：如何配置环境变量并运行测试。\n");
    write_kb_file(&root, "guide.en.md", "# Guide\n\nEnglish translation of the guide body.\n");

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "环境配置", None).await.unwrap();

    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "guide.en.md");
    assert_eq!(duplicate.duplicate_of, "guide.md");
    assert!(
        duplicate.canonical_reason.contains("原文"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
    let candidate_paths: Vec<&str> =
        report.candidates().iter().map(|file| file.rel_path.as_str()).collect();
    assert_eq!(candidate_paths, ["guide.md"], "翻译副本不重复成课");
    assert_eq!(report.counts.core, 1);
}

#[tokio::test]
async fn microsoft_layout_pairs_translations_and_orphans_stay_originals() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "多语言库").await;
    write_kb_file(&root, "en/intro.md", "# Intro\n\nThis handbook introduces the testing workflow.\n");
    write_kb_file(&root, "zh/intro.md", "# 简介\n\n本手册介绍测试工作流。\n");
    write_kb_file(&root, "ja/notes.md", "# 補足\n\nテストに関する補足資料（孤儿翻译，无英文/中文对应文件）。\n");

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "中文目标：测试工作流", None)
        .await
        .unwrap();

    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "en/intro.md");
    assert_eq!(duplicate.duplicate_of, "zh/intro.md");
    assert!(
        duplicate.canonical_reason.contains("目标"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
    let notes = report.files.iter().find(|file| file.rel_path == "ja/notes.md").unwrap();
    assert!(
        notes.role.is_candidate(),
        "孤儿翻译保守当原文，不当重复排除：{:?}",
        notes.role
    );
}

#[tokio::test]
async fn canonical_picks_the_original_and_the_newest_among_old_copies() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "报告库").await;
    let content = "# 季度报告\n\n收入、成本与留存的数据汇总正文。\n";
    write_kb_file(&root, "report.md", content);
    write_kb_file(&root, "backup/report.md", content);
    write_kb_file(&root, "report.en.md", "# Quarterly Report\n\nThe English translation of the quarterly report.\n");
    set_mtime(&root, "backup/report.md", at_secs(1_700_000_000));
    set_mtime(&root, "report.md", at_secs(1_700_200_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "季度经营复盘", None).await.unwrap();

    assert_eq!(report.counts.duplicate, 2, "翻译副本 + 旧版本都排除");
    assert_eq!(report.counts.core, 1, "canonical 原文进候选");
    let by_path: HashMap<&str, &KbTriageDuplicate> = report
        .duplicates
        .iter()
        .map(|dup| (dup.path.as_str(), dup))
        .collect();
    assert_eq!(by_path["report.en.md"].duplicate_of, "report.md");
    assert!(by_path["report.en.md"].canonical_reason.contains("原文"));
    assert_eq!(by_path["backup/report.md"].duplicate_of, "report.md");
    assert!(by_path["backup/report.md"].canonical_reason.contains("最新"));
    let canonical = report.files.iter().find(|file| file.rel_path == "report.md").unwrap();
    assert_eq!(canonical.role, KbTriageRole::Core);
}

#[tokio::test]
async fn unresolvable_translation_pairs_stay_uncertain_without_exclusion() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "含混库").await;
    write_kb_file(&root, "en/guide.md", "# Guide\n\nBoth directories hold the same English body by mistake.\n");
    write_kb_file(&root, "zh/guide.md", "# Guide\n\nBoth directories hold the same English body by mistake.\n");
    set_mtime(&root, "en/guide.md", at_secs(1_700_000_000));
    set_mtime(&root, "zh/guide.md", at_secs(1_700_000_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "中文目标（与两份英文正文都不一致）", None)
        .await
        .unwrap();

    assert_eq!(report.counts.uncertain, 2, "判不了翻译方向 → 双双 uncertain");
    assert_eq!(report.excluded.len(), 0, "无法判定时不自动排除");
    assert_eq!(report.candidates().len(), 2, "uncertain 保守保留进候选");
    for file in &report.files {
        assert!(file.reason.contains("无法判定"), "{}", file.reason);
    }
}
