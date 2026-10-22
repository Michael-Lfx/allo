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
    assert_eq!(
        equations.heading_levels,
        [1u8, 2, 2],
        "标题层级与 headings 平行(H1 章 / H2 节),供覆盖对账建父子链"
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

/// h3：同内容对里规整文件名（无备份/错位标记）优先于 mtime——即使旧名
/// 副本（guide-old 带 old 标记）更新，canonical 仍是规整的 guide.md。
#[tokio::test]
async fn identical_content_files_pick_the_regular_name_over_a_newer_marked_copy() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "项目文档").await;
    let content = "# 使用指南\n\n第一步安装依赖，第二步运行迁移。\n";
    write_kb_file(&root, "guide.md", content);
    write_kb_file(&root, "archive/guide-old.md", content);
    set_mtime(&root, "guide.md", at_secs(1_700_000_000));
    set_mtime(&root, "archive/guide-old.md", at_secs(1_700_900_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "上手该项目", None).await.unwrap();

    assert_eq!(report.counts.duplicate, 1);
    assert_eq!(report.counts.core, 1);
    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "archive/guide-old.md");
    assert_eq!(duplicate.duplicate_of, "guide.md");
    assert!(
        duplicate.canonical_reason.contains("规整"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
    let excluded_paths: Vec<&str> = report.excluded.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(excluded_paths, ["archive/guide-old.md"]);
}

/// h3 验收:数字章前缀的正身 vs mtime 更新的备份副本(z9-…-备份)——规整
/// 文件名优先于 mtime,canonical 落在 05 而不是系统性更新的备份。
#[tokio::test]
async fn regular_chapter_prefix_beats_a_newer_backup_copy_as_canonical() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "算法讲义").await;
    let content = "# 链表\n\n单链表与双链表的结构、插入与删除操作。\n";
    write_kb_file(&root, "05-LinkedList.md", content);
    write_kb_file(&root, "z9-LinkedList-备份.md", content);
    set_mtime(&root, "05-LinkedList.md", at_secs(1_700_000_000));
    set_mtime(&root, "z9-LinkedList-备份.md", at_secs(1_700_900_000));

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "数据结构入门", None).await.unwrap();

    assert_eq!(report.counts.duplicate, 1);
    assert_eq!(report.counts.core, 1, "正身 05 进候选");
    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "z9-LinkedList-备份.md");
    assert_eq!(duplicate.duplicate_of, "05-LinkedList.md");
    assert!(
        duplicate.canonical_reason.contains("规整"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
    let canonical = report
        .files
        .iter()
        .find(|file| file.rel_path == "05-LinkedList.md")
        .unwrap();
    assert_eq!(canonical.role, KbTriageRole::Core);
}

/// h3 验收:错位超序副本(999-错位-…)即使 mtime 更新也不当正身,canonical
/// 落在数字章前缀规整的 11。
#[tokio::test]
async fn regular_chapter_prefix_beats_a_newer_misfiled_copy_as_canonical() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "算法讲义乙").await;
    let content = "# 哈希表\n\n哈希函数、冲突处理与负载均衡的取舍。\n";
    write_kb_file(&root, "11-哈希.md", content);
    write_kb_file(&root, "999-错位-哈希.md", content);
    set_mtime(&root, "11-哈希.md", at_secs(1_700_000_000));
    set_mtime(&root, "999-错位-哈希.md", at_secs(1_700_900_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "哈希入门", None).await.unwrap();

    assert_eq!(report.counts.duplicate, 1);
    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "999-错位-哈希.md");
    assert_eq!(duplicate.duplicate_of, "11-哈希.md");
    assert!(
        duplicate.canonical_reason.contains("规整"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
}

/// h3:超序占位前缀(≥900,如 999-)即使文件名不带备份/错位字样也降权
/// ——数字章前缀的「规整」要落在正常序号范围内。
#[tokio::test]
async fn a_placeholder_sized_numeric_prefix_alone_is_demoted() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "算法讲义丙").await;
    let content = "# 哈希表\n\n开放寻址与链地址法的实现对比。\n";
    write_kb_file(&root, "11-哈希.md", content);
    write_kb_file(&root, "999-哈希.md", content);
    set_mtime(&root, "11-哈希.md", at_secs(1_700_000_000));
    set_mtime(&root, "999-哈希.md", at_secs(1_700_900_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "哈希入门", None).await.unwrap();

    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "999-哈希.md");
    assert_eq!(duplicate.duplicate_of, "11-哈希.md");
    assert!(
        duplicate.canonical_reason.contains("规整"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
}

/// h3:数字章前缀对同样干净的无前缀名加成——即使无前缀副本 mtime 更新,
/// 带规整章号的文件当正身。
#[tokio::test]
async fn a_numbered_name_beats_an_equally_clean_unnumbered_copy() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "指南库").await;
    let content = "# 学习指南\n\n环境配置、依赖安装与首个示例。\n";
    write_kb_file(&root, "05-guide.md", content);
    write_kb_file(&root, "guide.md", content);
    set_mtime(&root, "05-guide.md", at_secs(1_700_000_000));
    set_mtime(&root, "guide.md", at_secs(1_700_900_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "上手指南", None).await.unwrap();

    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "guide.md");
    assert_eq!(duplicate.duplicate_of, "05-guide.md");
    assert!(
        duplicate.canonical_reason.contains("规整"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
}

/// h3:规整度同分(都无前缀、无降权标记)→ mtime 分高下,「最新」语义保留。
#[tokio::test]
async fn equally_regular_names_fall_back_to_newest_mtime() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "手册库").await;
    let content = "# 部署手册\n\n蓝绿部署与回滚步骤。\n";
    write_kb_file(&root, "guide.md", content);
    write_kb_file(&root, "docs/manual.md", content);
    set_mtime(&root, "guide.md", at_secs(1_700_000_000));
    set_mtime(&root, "docs/manual.md", at_secs(1_700_900_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "部署入门", None).await.unwrap();

    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "guide.md");
    assert_eq!(duplicate.duplicate_of, "docs/manual.md");
    assert!(
        duplicate.canonical_reason.contains("最新"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
}

/// h3:规整度与 mtime 全平手 → 路径字典序兜底(确定性,纯规则可复现)。
#[tokio::test]
async fn a_full_tie_falls_back_to_stable_path_order() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "笔记库").await;
    let content = "# 备忘\n\n同一份笔记的两份拷贝,完全同文。\n";
    write_kb_file(&root, "notes/b.md", content);
    write_kb_file(&root, "notes/a.md", content);
    set_mtime(&root, "notes/b.md", at_secs(1_700_000_000));
    set_mtime(&root, "notes/a.md", at_secs(1_700_000_000));

    let (_inventory, report) = run_kb_triage(&knowledge, &[kb_id], "备忘速览", None).await.unwrap();

    let duplicate = &report.duplicates[0];
    assert_eq!(duplicate.path, "notes/b.md");
    assert_eq!(duplicate.duplicate_of, "notes/a.md");
    assert!(
        duplicate.canonical_reason.contains("路径字典序"),
        "canonical_reason: {}",
        duplicate.canonical_reason
    );
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

/// h3：翻译副本层照旧（非翻译后缀原文优先），同 hash 层改为规整文件名
/// 优先——backup/ 目录副本即使 mtime 更新也不当正身。
#[tokio::test]
async fn canonical_picks_the_regular_original_over_a_newer_backup_copy() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "报告库").await;
    let content = "# 季度报告\n\n收入、成本与留存的数据汇总正文。\n";
    write_kb_file(&root, "report.md", content);
    write_kb_file(&root, "backup/report.md", content);
    write_kb_file(&root, "report.en.md", "# Quarterly Report\n\nThe English translation of the quarterly report.\n");
    set_mtime(&root, "report.md", at_secs(1_700_000_000));
    set_mtime(&root, "backup/report.md", at_secs(1_700_900_000));

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
    assert!(
        by_path["backup/report.md"]
            .canonical_reason
            .contains("规整"),
        "canonical_reason: {}",
        by_path["backup/report.md"].canonical_reason
    );
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

// ── LLM 分诊（scripted completer）───────────────────────────────────────────

/// 记录提示词、按当前批的文件列表决定回包的假 completer（分诊只用 json 接缝）。
struct RespondingCompleter<F>
where
    F: Fn(&[String]) -> Result<String, String> + Send + Sync,
{
    respond: F,
    calls: std::sync::atomic::AtomicUsize,
    prompts: std::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl<F> LearningCompleter for RespondingCompleter<F>
where
    F: Fn(&[String]) -> Result<String, String> + Send + Sync,
{
    async fn complete(
        &self,
        _model_override: Option<(&str, &str)>,
        _system: &str,
        _user: &str,
        _max_tokens: u32,
    ) -> Result<String, AppError> {
        Err(AppError::Internal("分诊测试只走 complete_json 接缝".into()))
    }

    async fn complete_json(
        &self,
        _model_override: Option<(&str, &str)>,
        _system: &str,
        user: &str,
        _max_tokens: u32,
        _temperature: Option<f32>,
    ) -> Result<String, AppError> {
        self.prompts.lock().unwrap().push(user.to_owned());
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let paths = prompt_paths(user);
        (self.respond)(&paths).map_err(|message| AppError::Internal(message))
    }
}

/// 从批提示词的编号清单行解析路径（`{n}. {path} :: …` 行；目录树的
/// `- {path}` 行不带编号，不会误命中）。
fn prompt_paths(user: &str) -> Vec<String> {
    user.lines()
        .filter_map(|line| {
            let (number, rest) = line.split_once(". ")?;
            if number.is_empty() || !number.chars().all(|char| char.is_ascii_digit()) {
                return None;
            }
            Some(rest.split(" ::").next().unwrap_or(rest).to_owned())
        })
        .collect()
}

fn reply_json(entries: &[(&str, &str, f32, &str)]) -> String {
    format!(
        "{{\"files\": [{}]}}",
        entries
            .iter()
            .map(|(path, role, confidence, reason)| {
                format!(
                    "{{\"path\": \"{path}\", \"role\": \"{role}\", \"confidence\": {confidence}, \"reason\": \"{reason}\"}}"
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// 模拟「不可自救」的截断回包：第一个条目都还没写完就在字符串值中间硬截断
/// （`parse_json_object` 的截断自救也救不回，只能走批拆半）。
fn truncated_reply(paths: &[String]) -> String {
    let first = paths.first().map(String::as_str).unwrap_or("x.md");
    format!("{{\"files\": [{{\"path\": \"{first}\", \"role\": \"co")
}

#[tokio::test]
async fn llm_triage_assigns_roles_and_mixed_files_join_candidates() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "课程素材").await;
    write_kb_file(&root, "ch1.md", "# 核心章节\n\n本章讲排序算法的实现与复杂度分析。\n");
    write_kb_file(&root, "lab.md", "# 实验\n\n动手实现快速排序并测量耗时。\n");
    write_kb_file(&root, "mixed.md", "# 会议纪要\n\n前半是排序算法正文，后半附团队周会记录与推广段落。\n");
    write_kb_file(&root, "receipt.md", "# 收据\n\n超市购物小票：牛奶 12 元，面包 6 元。\n");

    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            let entries: Vec<(&str, &str, f32, &str)> = paths
                .iter()
                .map(|path| match path.as_str() {
                    "ch1.md" => ("ch1.md", "core", 0.9, "排序算法是课程主体"),
                    "lab.md" => ("lab.md", "practice", 0.85, "动手实验"),
                    "mixed.md" => ("mixed.md", "mixed", 0.8, "正文相关但混入会议与推广段"),
                    _ => ("receipt.md", "noise", 0.9, "消费收据与目标无关"),
                })
                .collect();
            Ok(reply_json(&entries))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会排序算法", Some(&completer)).await.unwrap();

    assert!(!report.degraded);
    assert_eq!(inventory.total_files, 4);
    assert_eq!(report.counts.core, 1);
    assert_eq!(report.counts.practice, 1);
    assert_eq!(report.counts.mixed, 1);
    assert_eq!(report.counts.noise, 1);
    let mixed = report.files.iter().find(|file| file.rel_path == "mixed.md").unwrap();
    assert_eq!(mixed.role, KbTriageRole::Mixed);
    assert!(
        report
            .candidates()
            .iter()
            .any(|file| file.rel_path == "mixed.md"),
        "mixed 文件不整文件排除，进入 t2 候选"
    );
    let excluded_paths: Vec<&str> = report.excluded.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(excluded_paths, ["receipt.md"]);
    assert!(report.excluded[0].reason.contains("收据"));

    // 提示词面：目标 + 目录树 + 每文件标题/预览都进了提示词。
    let prompts = completer.prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 1, "4 个文件一批一次调用");
    let prompt = &prompts[0];
    assert!(prompt.contains("学会排序算法"), "{prompt}");
    assert!(prompt.contains("- ch1.md"), "目录树：{prompt}");
    assert!(prompt.contains("核心章节"), "文件标题：{prompt}");
    assert!(prompt.contains("复杂度分析"), "文件预览：{prompt}");
}

#[tokio::test]
async fn more_than_forty_files_split_into_multiple_batches() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "大库").await;
    for index in 0..45 {
        write_kb_file(&root, &format!("f{index:02}.md"), &format!("# 第{index}篇\n\n正文内容 {index}。\n"));
    }
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            Ok(reply_json(
                &paths
                    .iter()
                    .map(|path| (path.as_str(), "core", 0.9, "相关内容"))
                    .collect::<Vec<_>>(),
            ))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "批量内容", Some(&completer)).await.unwrap();

    assert_eq!(inventory.total_files, 45);
    assert_eq!(
        completer.calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "45 文件按 ≤40/批切两批"
    );
    assert_eq!(report.counts.core, 45);
    assert!(!report.degraded);
}

#[tokio::test]
async fn truncated_batch_replies_split_half_and_still_cover_every_file() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "截断库").await;
    for index in 0..40 {
        write_kb_file(&root, &format!("f{index:02}.md"), &format!("# 第{index}篇\n\n正文 {index}。\n"));
    }
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            if paths.len() > 10 {
                Ok(truncated_reply(paths))
            } else {
                Ok(reply_json(
                    &paths
                        .iter()
                        .map(|path| (path.as_str(), "core", 0.9, "小批成功"))
                        .collect::<Vec<_>>(),
                ))
            }
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "截断演练", Some(&completer)).await.unwrap();

    assert_eq!(inventory.total_files, 40);
    assert_eq!(report.counts.core, 40, "拆半后仍覆盖全部文件");
    assert_eq!(report.counts.uncertain, 0);
    assert!(!report.degraded);
    assert_eq!(
        completer.calls.load(std::sync::atomic::Ordering::SeqCst),
        7,
        "40 失败 → 20×2 失败 → 10×4 成功"
    );
}

// ── h1：批回包完整性（标签前缀回显 / 宽容解析 / 定向补判 / 单文件兜底）──────

/// h1 根因（真机 run-1792650549242 整批回包）：模型把清单行「文件 」标签回显
/// 进 path，其余形状完全合规、条数齐全。归一化匹配后整批一次命中，不再
/// 拆半/单文件兜底/落 uncertain。
#[tokio::test]
async fn label_prefixed_path_echoes_are_normalized_back_to_requested_paths() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "回显库").await;
    write_kb_file(
        &root,
        "01-前言.md",
        "# 前言\n\n前言介绍本书背景和预备条件。\n",
    );
    write_kb_file(
        &root,
        "02-接口.md",
        "# 接口\n\n本章讲接口设计与实现约定。\n",
    );
    write_kb_file(
        &root,
        "03-算法分析.md",
        "# 算法分析\n\n本章讲复杂度分析与摊还分析。\n",
    );
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            // 真机形状：每条 path 都带「文件 」标签前缀（原样摘自归档回包）。
            let entries: Vec<(String, &str, f32, &str)> = paths
                .iter()
                .map(|path| (format!("文件 {path}"), "core", 0.9, "与学习目标直接相关"))
                .collect();
            let body = entries
                .iter()
                .map(|(path, role, confidence, reason)| {
                    format!(
                        "{{\"path\": \"{path}\", \"role\": \"{role}\", \"confidence\": {confidence}, \"reason\": \"{reason}\"}}"
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!("{{\"files\": [{body}]}}"))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "数据结构入门", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.core, 3, "带前缀回显的条目应全部命中请求集");
    assert_eq!(report.counts.uncertain, 0, "前缀回显不再落 uncertain");
    assert!(!report.degraded);
    assert_eq!(
        completer.calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "归一化后整批一次成功，不触发拆半或单文件兜底"
    );
    for path in ["01-前言.md", "02-接口.md", "03-算法分析.md"] {
        let file = report.files.iter().find(|file| file.rel_path == path).unwrap();
        assert_eq!(file.role, KbTriageRole::Core, "{path} 应按请求路径落判定");
    }
}

/// 真机 call#6：单文件兜底批的回包 JSON 完整、判定合理，仅 path 带「文件 」
/// 前缀——此前被当幻觉路径丢弃后落 uncertain（verify-clean 7 条 uncertain
/// 的逐一来源）。归一化后应拿到判定。
#[tokio::test]
async fn single_file_prefixed_echo_replies_land_verdicts() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "单文件回显库").await;
    write_kb_file(
        &root,
        "01-前言.md",
        "# 前言\n\n前言介绍本书背景和预备条件。\n",
    );
    let completer = RespondingCompleter {
        respond: |_paths: &[String]| {
            // 原样摘自归档 call#6 回包（req=1，JSON 完整闭合）。
            Ok(r#"{"files": [{"path": "文件 01-前言.md", "role": "prerequisite", "confidence": 0.9, "reason": "前言介绍本书背景和预备条件，是学习前的背景说明，非核心教学内容。"}]}"#.to_owned())
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "数据结构入门", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.prerequisite, 1, "单文件前缀回显应命中判定");
    assert_eq!(report.counts.uncertain, 0);
    let preface = &report.files[0];
    assert_eq!(preface.role, KbTriageRole::Prerequisite);
    assert_eq!(
        completer.calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "一次调用即命中"
    );
}

/// 真机 call#22：同一回包里 1 条精确、1 条带前缀的混合形态也应全部命中。
#[tokio::test]
async fn mixed_exact_and_prefixed_entries_all_land() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "混合回显库").await;
    write_kb_file(&root, "16-爬取维基百科.md", "# 爬取维基百科\n\n爬虫流程与缓存策略。\n");
    write_kb_file(&root, "17-布尔搜索.md", "# 布尔搜索\n\n布尔查询的解析与求值。\n");
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            // 第一条原样回显、第二条带「文件 」前缀（call#22 形态）。
            let entries: Vec<(String, &str, f32, &str)> = paths
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    let echoed = if index == 0 {
                        path.clone()
                    } else {
                        format!("文件 {path}")
                    };
                    (echoed, "core", 0.85, "课程正文")
                })
                .collect();
            let body = entries
                .iter()
                .map(|(path, role, confidence, reason)| {
                    format!(
                        "{{\"path\": \"{path}\", \"role\": \"{role}\", \"confidence\": {confidence}, \"reason\": \"{reason}\"}}"
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!("{{\"files\": [{body}]}}"))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "搜索入门", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.core, 2, "精确与带前缀条目都命中");
    assert_eq!(report.counts.uncertain, 0);
    assert_eq!(
        completer.calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

/// h1 宽容解析：除数组外，接受「files 的值是按 rel_path 为键的对象」形状
/// （条目的 path 从键补齐）。真机未观察到该形态，但属协议变体的合理防御。
#[tokio::test]
async fn object_keyed_reply_shapes_are_parsed() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "对象形状库").await;
    write_kb_file(&root, "ch1.md", "# 核心章节\n\n本章讲排序算法的实现。\n");
    write_kb_file(&root, "lab.md", "# 实验\n\n动手实现快速排序并测量耗时。\n");
    let completer = RespondingCompleter {
        respond: |_paths: &[String]| {
            Ok(r#"{"files": {"ch1.md": {"role": "core", "confidence": 0.9, "reason": "课程正文"}, "lab.md": {"role": "practice", "confidence": 0.85, "reason": "动手实验"}}}"#.to_owned())
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会排序算法", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.core, 1, "对象按键形状的回包可解析");
    assert_eq!(report.counts.practice, 1);
    assert_eq!(report.counts.uncertain, 0);
}

/// h1 宽容解析：条目字段名的小差异按同义词映射（path/rel_path、role/kind）。
#[tokio::test]
async fn synonym_entry_field_names_are_mapped() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "同义字段库").await;
    write_kb_file(&root, "ch1.md", "# 核心章节\n\n本章讲排序算法的实现。\n");
    write_kb_file(&root, "lab.md", "# 实验\n\n动手实现快速排序并测量耗时。\n");
    let completer = RespondingCompleter {
        respond: |_paths: &[String]| {
            Ok(r#"{"files": [{"rel_path": "ch1.md", "kind": "core", "confidence": 0.9, "reason": "课程正文"}, {"rel_path": "lab.md", "kind": "practice", "confidence": 0.85, "reason": "动手实验"}]}"#.to_owned())
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会排序算法", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.core, 1, "rel_path/kind 字段名变体按同义词映射");
    assert_eq!(report.counts.practice, 1);
    assert_eq!(report.counts.uncertain, 0);
}

/// h1 提示词硬化：清单行带编号、不再使用可被回显的「文件 」标签（根因的
/// 提示词侧修复）；明确「必须恰好返回 N 条、按清单顺序、path 原样回显、
/// 遗漏任何文件都算失败」。
#[tokio::test]
async fn triage_prompts_number_files_and_demand_exact_coverage() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "提示词库").await;
    write_kb_file(&root, "ch1.md", "# 核心章节\n\n本章讲排序算法的实现。\n");
    write_kb_file(&root, "lab.md", "# 实验\n\n动手实现快速排序并测量耗时。\n");
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            Ok(reply_json(
                &paths
                    .iter()
                    .map(|path| (path.as_str(), "core", 0.9, "课程正文"))
                    .collect::<Vec<_>>(),
            ))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, _report) =
        run_kb_triage(&knowledge, &[kb_id], "学会排序算法", Some(&completer)).await.unwrap();

    let prompts = completer.prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 1);
    let prompt = &prompts[0];
    assert!(prompt.contains("1. ch1.md ::"), "清单行带编号：{prompt}");
    assert!(prompt.contains("2. lab.md ::"), "按输入顺序编号：{prompt}");
    assert!(
        !prompt.contains("- 文件 "),
        "不再使用可被回显进 path 的「文件 」标签行：{prompt}"
    );
    assert!(prompt.contains("必须恰好返回 2 条判定"), "恰好 N 条契约：{prompt}");
    assert!(prompt.contains("原样回显"), "path 原样回显契约：{prompt}");
    assert!(prompt.contains("遗漏任何文件都算失败"), "遗漏即失败契约：{prompt}");
    // 系统提示词与清单行格式同步硬化。
    assert!(TRIAGE_SYSTEM.contains("遗漏任何文件都算失败"));
    assert!(TRIAGE_SYSTEM.contains("原样回显"));
}

/// h1 定向补判：整批部分覆盖时只对缺失子集重发，且补判提示词换成
/// 「只判定以下 N 个文件」的定向措辞（不再原样重发同形状全批提示词）。
#[tokio::test]
async fn partial_coverage_reasks_only_the_missing_subset_with_a_directed_prompt() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "补判库").await;
    write_kb_file(&root, "ch1.md", "# 核心章节\n\n本章讲排序算法的实现。\n");
    write_kb_file(&root, "lab.md", "# 实验\n\n动手实现快速排序并测量耗时。\n");
    write_kb_file(&root, "notes.md", "# 备注\n\n补充说明与延伸阅读。\n");
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            // 首批（3 文件）只回 2/3 条（模拟真机「数组缺 k 条」）；定向
            // 补判批只来缺失的那 1 个文件，回满。
            let omit_first_round = paths.len() > 1;
            let covered: Vec<(&str, &str, f32, &str)> = paths
                .iter()
                .filter(|path| !(omit_first_round && path.as_str() == "notes.md"))
                .map(|path| (path.as_str(), "core", 0.9, "课程正文"))
                .collect();
            Ok(reply_json(&covered))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会排序算法", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.core, 3, "缺失子集经定向补判后全覆盖");
    assert_eq!(report.counts.uncertain, 0);
    let prompts = completer.prompts.lock().unwrap().clone();
    assert_eq!(prompts.len(), 2, "首批 + 补判批各一次调用");
    assert_eq!(prompt_paths(&prompts[0]), ["ch1.md", "lab.md", "notes.md"]);
    assert_eq!(prompt_paths(&prompts[1]), ["notes.md"], "补判批只含缺失子集");
    assert!(
        prompts[1].contains("只判定以下 1 个文件"),
        "补判批用定向措辞：{}",
        prompts[1]
    );
}

/// h1 单文件批专门化：模型可能直接回「裸单条目对象」（根对象就是判定，
/// 无 files 包裹）——单条目形状下可解析。
#[tokio::test]
async fn bare_single_entry_replies_parse_for_single_file_batches() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "裸对象库").await;
    write_kb_file(&root, "ch1.md", "# 核心章节\n\n本章讲排序算法的实现。\n");
    let completer = RespondingCompleter {
        respond: |_paths: &[String]| {
            Ok(r#"{"path": "ch1.md", "role": "core", "confidence": 0.9, "reason": "课程正文"}"#.to_owned())
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会排序算法", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.core, 1, "裸单条目形状按单条目解析");
    assert_eq!(report.counts.uncertain, 0);
}

/// h1 单文件兜底修复：「单文件批合法空回包（模型显式无判定）」与「解析
/// 失败」在最终 uncertain 理由里必须可区分。
#[tokio::test]
async fn single_file_explicit_empty_is_distinguishable_from_parse_failure() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "空回包库").await;
    write_kb_file(&root, "a.md", "# 甲\n\n正文甲。\n");
    write_kb_file(&root, "empty.md", "# 乙\n\n正文乙。\n");
    write_kb_file(&root, "broken.md", "# 丙\n\n正文丙。\n");
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            if paths.len() == 1 && paths[0] == "empty.md" {
                // 合法空回包：JSON 完整，files 为空数组——模型显式不给判定。
                return Ok(r#"{"files": []}"#.to_owned());
            }
            if paths.len() == 1 && paths[0] == "broken.md" {
                // 解析失败：字符串中间硬截断。
                return Ok("{\"files\": [{\"path\": \"broken.md\", \"ro".to_owned());
            }
            let covered: Vec<(&str, &str, f32, &str)> = paths
                .iter()
                .filter(|path| !matches!(path.as_str(), "empty.md" | "broken.md"))
                .map(|path| (path.as_str(), "core", 0.9, "正常判定"))
                .collect();
            Ok(reply_json(&covered))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "单点失败", Some(&completer)).await.unwrap();

    assert_eq!(report.counts.uncertain, 2, "两条兜底路径都保守 uncertain");
    let empty = report
        .files
        .iter()
        .find(|file| file.rel_path == "empty.md")
        .unwrap();
    assert!(
        empty.reason.contains("显式无判定"),
        "合法空回包要能看出是模型显式无判定：{}",
        empty.reason
    );
    let broken = report
        .files
        .iter()
        .find(|file| file.rel_path == "broken.md")
        .unwrap();
    assert!(
        !broken.reason.contains("显式无判定") && broken.reason.contains("解析失败"),
        "真解析失败要能看出是解析失败：{}",
        broken.reason
    );
}

/// h1 计数披露：批补判次数（拆半/缺条目子集的重发次数）与仍缺条目数（全部
/// 手段用尽仍未拿到模型判定的文件数）进分诊报告；补判成功时仍缺为 0，
/// 补判穷尽时如实大于 0。
#[tokio::test]
async fn repair_and_still_missing_counts_are_disclosed() {
    // 场景一：缺条目 → 定向补判成功 → 补判 1 次、仍缺 0。
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "计数库甲").await;
    write_kb_file(&root, "ch1.md", "# 核心\n\n正文一。\n");
    write_kb_file(&root, "notes.md", "# 备注\n\n补充说明。\n");
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            let omit_first_round = paths.len() > 1;
            let covered: Vec<(&str, &str, f32, &str)> = paths
                .iter()
                .filter(|path| !(omit_first_round && path.as_str() == "notes.md"))
                .map(|path| (path.as_str(), "core", 0.9, "课程正文"))
                .collect();
            Ok(reply_json(&covered))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };
    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会正文", Some(&completer)).await.unwrap();
    assert_eq!(report.batch_repair_count, 1, "缺条目补判一次");
    assert_eq!(report.still_missing_count, 0, "补判成功后仍缺为 0");

    // 场景二：单文件兜底仍缺 → 补判 ≥1 次、仍缺 1 条（uncertain 语义不变）。
    let (kb_id, root) = create_blank_base(&knowledge, "计数库乙").await;
    write_kb_file(&root, "a.md", "# 甲\n\n正文甲。\n");
    write_kb_file(&root, "ghost.md", "# 乙\n\n正文乙。\n");
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            // 回包永远只判 a.md：整批部分覆盖 → ghost.md 单文件补判仍缺。
            let covered: Vec<(&str, &str, f32, &str)> = paths
                .iter()
                .filter(|path| path.as_str() == "a.md")
                .map(|path| (path.as_str(), "core", 0.9, "课程正文"))
                .collect();
            Ok(reply_json(&covered))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };
    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会正文", Some(&completer)).await.unwrap();
    assert_eq!(report.counts.uncertain, 1);
    assert!(
        report.batch_repair_count >= 1,
        "单文件兜底计入补判次数：{}",
        report.batch_repair_count
    );
    assert_eq!(report.still_missing_count, 1, "穷尽后仍缺如实披露");
}

#[tokio::test]
async fn a_single_file_that_never_parses_becomes_uncertain_without_dropping() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "单点坏库").await;
    write_kb_file(&root, "a.md", "# 甲\n\n正文甲。\n");
    write_kb_file(&root, "broken.md", "# 坏\n\n正文坏。\n");
    write_kb_file(&root, "c.md", "# 丙\n\n正文丙。\n");
    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            if paths.len() == 1 && paths[0] == "broken.md" {
                // 单文件兜底也截断：字符串中间硬截断，解析必败。
                return Ok("{\"files\": [{\"path\": \"broken.md\", \"ro".to_owned());
            }
            let covered: Vec<(&str, &str, f32, &str)> = paths
                .iter()
                .filter(|path| path.as_str() != "broken.md")
                .map(|path| (path.as_str(), "core", 0.9, "正常判定"))
                .collect();
            Ok(reply_json(&covered))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "单点失败", Some(&completer)).await.unwrap();

    assert!(!report.degraded, "解析失败不是调用失败，不触发降级");
    let broken = report
        .files
        .iter()
        .find(|file| file.rel_path == "broken.md")
        .expect("单文件失败也不丢条目");
    assert_eq!(broken.role, KbTriageRole::Uncertain);
    assert!(broken.reason.contains("保守"), "{}", broken.reason);
    assert_eq!(report.counts.core, 2);
    assert_eq!(report.counts.uncertain, 1);
    assert!(
        report
            .candidates()
            .iter()
            .any(|file| file.rel_path == "broken.md"),
        "uncertain 保守保留并可见"
    );
}

/// 票面验收：混合脏库（core 章节 + 收据/会议记录/剪藏 + LICENSE/配置 +
/// 翻译副本 + 同 hash 重复 + 缺失标题 + 空导航页 + mixed 文件 + 判不准的
/// 存根）全量分诊，总数守恒、逐项有理由、保守保留。
#[tokio::test]
async fn dirty_kb_triage_conserves_every_file_and_accounts_for_each_exclusion() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "脏知识库").await;
    let chapter = "# 排序算法\n\n比较排序的下界与线性时间排序的实现分析。\n";
    write_kb_file(&root, "ch1.md", chapter);
    write_kb_file(&root, "ch2.md", "# 图算法\n\n最短路径与最小生成树的正确性证明。\n");
    write_kb_file(&root, "receipts/2024-01.md", "# 收据\n\n便利店购物：饭团 8 元，咖啡 15 元。\n");
    write_kb_file(&root, "meetings/weekly.md", "# 周会纪要\n\n讨论了排期、报销与团建地点。\n");
    write_kb_file(&root, "clippings/promo.md", "# 剪藏\n\n某产品发布会通稿摘录，与课程无关。\n");
    write_kb_file(&root, "LICENSE.md", "MIT License\n\nPermission is granted, free of charge.\n");
    write_kb_file(&root, "config.md", "# 配置说明\n\n构建脚本的参数开关与环境变量列表。\n");
    write_kb_file(&root, "guide.md", "# 指南\n\n中文原文：算法课的学习路线与参考资料。\n");
    write_kb_file(&root, "guide.zh-CN.md", "# 指南\n\n中文译本副本（与 guide.md 成对出现）。\n");
    write_kb_file(&root, "archive/ch1-old.md", chapter);
    write_kb_file(&root, "noheading.md", "这一篇没有标题，但正文讲的是动态规划的状态设计。\n");
    write_kb_file(&root, "toc.md", "# 目录\n\n- [第一章](ch1.md)\n- [第二章](ch2.md)\n");
    write_kb_file(&root, "mixed.md", "# 混合\n\n前半是算法正文，后半附团队周会记录与推广段落。\n");
    write_kb_file(&root, "stub.md", "# 存根\n\n只有一句话。\n");
    set_mtime(&root, "archive/ch1-old.md", at_secs(1_700_000_000));
    set_mtime(&root, "ch1.md", at_secs(1_700_100_000));

    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            let entries: Vec<(&str, &str, f32, &str)> = paths
                .iter()
                .map(|path| match path.as_str() {
                    "ch1.md" | "ch2.md" | "noheading.md" | "guide.md" => {
                        (path.as_str(), "core", 0.9, "算法课程正文")
                    }
                    "mixed.md" => ("mixed.md", "mixed", 0.8, "正文相关但混入会议与推广段"),
                    "stub.md" => ("stub.md", "uncertain", 0.3, "内容太短判不准"),
                    "receipts/2024-01.md" => ("receipts/2024-01.md", "noise", 0.95, "消费收据"),
                    "meetings/weekly.md" => ("meetings/weekly.md", "noise", 0.9, "会议纪要"),
                    "clippings/promo.md" => ("clippings/promo.md", "noise", 0.9, "推广剪藏"),
                    "config.md" => ("config.md", "noise", 0.85, "构建配置说明"),
                    other => (other, "uncertain", 0.3, "未预期路径"),
                })
                .collect();
            Ok(reply_json(&entries))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会基础算法", Some(&completer)).await.unwrap();

    // 总数守恒：清点、报告、计数三处口径一致，14 个文件一个不少。
    assert_eq!(inventory.total_files, 14);
    assert_eq!(report.files.len(), 14, "所有文件都出现在报告中");
    assert_eq!(
        report.counts.core + report.counts.prerequisite + report.counts.practice
            + report.counts.mixed
            + report.counts.noise
            + report.counts.duplicate
            + report.counts.uncertain,
        14
    );
    assert!(!report.degraded);

    // core 章节不被误杀。
    let ch1 = report.files.iter().find(|file| file.rel_path == "ch1.md").unwrap();
    assert_eq!(ch1.role, KbTriageRole::Core);
    assert_eq!(report.counts.core, 4, "ch1/ch2/noheading/guide 四篇 core");

    // mixed 不整文件排除，进 t2 候选。
    assert!(report.candidates().iter().any(|file| file.rel_path == "mixed.md"));

    // noise + duplicate 全部有 reason 且都在 excluded。
    assert_eq!(report.excluded.len(), 8, "6 噪声 + 2 重复");
    for exclusion in &report.excluded {
        assert!(!exclusion.reason.is_empty(), "{exclusion:?}");
    }

    // duplicate 有 duplicate_of + canonical_reason；翻译副本不重复成课。
    assert_eq!(report.duplicates.len(), 2);
    let translation = report
        .duplicates
        .iter()
        .find(|dup| dup.path == "guide.zh-CN.md")
        .expect("翻译副本按规则标 duplicate");
    assert_eq!(translation.duplicate_of, "guide.md");
    assert!(!translation.canonical_reason.is_empty());
    let candidate_paths: Vec<&str> =
        report.candidates().iter().map(|file| file.rel_path.as_str()).collect();
    assert!(!candidate_paths.contains(&"guide.zh-CN.md"), "翻译副本不成课");
    assert!(!candidate_paths.contains(&"archive/ch1-old.md"), "同 hash 旧副本不成课");
    let old_copy = report
        .duplicates
        .iter()
        .find(|dup| dup.path == "archive/ch1-old.md")
        .expect("同 hash 重复按规则标 duplicate");
    assert_eq!(old_copy.duplicate_of, "ch1.md");
    assert!(
        old_copy.canonical_reason.contains("规整"),
        "h3：同 hash 层按规整文件名选正身：{}",
        old_copy.canonical_reason
    );

    // uncertain 保留并可见。
    let stub = report.files.iter().find(|file| file.rel_path == "stub.md").unwrap();
    assert_eq!(stub.role, KbTriageRole::Uncertain);
    assert!(candidate_paths.contains(&"stub.md"));

    // 空导航页与 LICENSE 由规则判噪声（不依赖模型）。
    assert_eq!(
        report
            .files
            .iter()
            .find(|file| file.rel_path == "toc.md")
            .unwrap()
            .role,
        KbTriageRole::Noise
    );
    assert_eq!(
        report
            .files
            .iter()
            .find(|file| file.rel_path == "LICENSE.md")
            .unwrap()
            .role,
        KbTriageRole::Noise
    );
}

#[tokio::test]
async fn multi_kb_triage_namespaces_paths_and_skips_deleted_bases() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_a, root_a) = create_blank_base(&knowledge, "库甲").await;
    let (kb_b, root_b) = create_blank_base(&knowledge, "库乙").await;
    write_kb_file(&root_a, "docs/sorting.md", "# 排序\n\n甲库的排序笔记正文。\n");
    write_kb_file(&root_b, "sorting.md", "# 排序\n\n乙库的另一篇排序正文（内容不同）。\n");
    let deleted = KnowledgeBaseId::new();
    let deleted_str = deleted.as_str().to_owned();

    let (inventory, report) =
        run_kb_triage(&knowledge, &[kb_a.clone(), kb_b.clone(), deleted], "排序入门", None)
            .await
            .unwrap();

    assert_eq!(inventory.total_files, 2, "已删库 id 不读");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("不存在或不可读") && warning.contains(deleted_str.as_str())),
        "缺库披露：{:?}",
        report.warnings
    );
    let paths: Vec<&str> = inventory.files.iter().map(|file| file.rel_path.as_str()).collect();
    assert_eq!(
        paths,
        ["库乙/sorting.md", "库甲/docs/sorting.md"],
        "多库路径带命名空间前缀，输出按引用路径字典序（乙 < 甲）"
    );
    let file_yi = &inventory.files[0];
    let file_jia = &inventory.files[1];
    assert_eq!(file_yi.kb_id, kb_b.as_str());
    assert_eq!(file_jia.kb_id, kb_a.as_str());
    assert_eq!(inventory.bases.len(), 2);
    assert_eq!(inventory.bases[0].namespace, "库甲");
    assert_eq!(inventory.bases[1].namespace, "库乙");
    assert_eq!(report.counts.core, 2, "两库正文都降级进 core");
    assert!(report.degraded);
}

// ── 评审修复:LLM duplicate 判定 + 大库目录树如实披露 ─────────────────────

/// 评审修复 S1:LLM 判 duplicate 但给不出可核对的 duplicate_of(缺失或指向
/// 库外)时,该文件保守降级 uncertain——可见、不进 excluded,不再是
/// 「整文件静默排除」(冒烟实证的 LinkedList 章丢失根因)。
#[tokio::test]
async fn llm_duplicate_verdicts_without_a_resolvable_duplicate_of_stay_uncertain() {
    let (knowledge, _data_dir) = triage_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "重复判定库").await;
    write_kb_file(
        &root,
        "original.md",
        "# 原文\n\n链表章节正文:单链表与双链表的结构和实现。\n",
    );
    write_kb_file(
        &root,
        "no-evidence.md",
        "# 旧版甲\n\n疑似旧版本正文甲:模型判 duplicate 但未给 duplicate_of。\n",
    );
    write_kb_file(
        &root,
        "ghost-evidence.md",
        "# 旧版乙\n\n疑似旧版本正文乙:模型给的 duplicate_of 指向库外路径。\n",
    );
    write_kb_file(
        &root,
        "good-copy.md",
        "# 旧版丙\n\n与原文重复的旧版本正文丙:duplicate_of 可核对。\n",
    );

    let completer = RespondingCompleter {
        respond: |paths: &[String]| {
            let entries: Vec<String> = paths
                .iter()
                .map(|path| match path.as_str() {
                    "original.md" => {
                        r#"{"path": "original.md", "role": "core", "confidence": 0.9, "reason": "课程正文"}"#.to_owned()
                    }
                    "no-evidence.md" => {
                        r#"{"path": "no-evidence.md", "role": "duplicate", "confidence": 0.8, "reason": "疑似重复旧版"}"#.to_owned()
                    }
                    "ghost-evidence.md" => {
                        r#"{"path": "ghost-evidence.md", "role": "duplicate", "confidence": 0.8, "reason": "疑似重复旧版", "duplicate_of": "ghost.md"}"#.to_owned()
                    }
                    "good-copy.md" => {
                        r#"{"path": "good-copy.md", "role": "duplicate", "confidence": 0.85, "reason": "与原文重复", "duplicate_of": "original.md"}"#.to_owned()
                    }
                    other => format!(
                        r#"{{"path": "{other}", "role": "uncertain", "confidence": 0.3, "reason": "未预期路径"}}"#
                    ),
                })
                .collect();
            Ok(format!("{{\"files\": [{}]}}", entries.join(", ")))
        },
        calls: std::sync::atomic::AtomicUsize::new(0),
        prompts: std::sync::Mutex::new(Vec::new()),
    };

    let (_inventory, report) =
        run_kb_triage(&knowledge, &[kb_id], "学会链表", Some(&completer)).await.unwrap();

    // 无 duplicate_of / 指向库外:保守 uncertain,可见、不排除。
    for path in ["no-evidence.md", "ghost-evidence.md"] {
        let file = report
            .files
            .iter()
            .find(|file| file.rel_path == path)
            .unwrap();
        assert_eq!(
            file.role,
            KbTriageRole::Uncertain,
            "{path}:无可核对的 duplicate_of 不得整文件排除"
        );
        assert!(
            file.reason.contains("duplicate_of") && file.reason.contains("保守"),
            "{path} reason: {}",
            file.reason
        );
        assert!(file.duplicate_of.is_none(), "{path}:降级后不带 duplicate_of");
        assert!(
            report.candidates().iter().any(|file| file.rel_path == path),
            "{path}:uncertain 保守保留进候选"
        );
        assert!(
            !report.excluded.iter().any(|entry| entry.path == path),
            "{path}:不进 excluded"
        );
    }
    // 可核对的 duplicate_of:duplicate 判定照常成立。
    let good = report
        .files
        .iter()
        .find(|file| file.rel_path == "good-copy.md")
        .unwrap();
    assert_eq!(good.role, KbTriageRole::Duplicate);
    assert_eq!(good.duplicate_of.as_deref(), Some("original.md"));
    assert!(report.excluded.iter().any(|entry| entry.path == "good-copy.md"));
    // 汇总不变量:excluded/duplicates 台账不出现无可核对 duplicate_of 的条目。
    let ledger: Vec<&str> = report.duplicates.iter().map(|dup| dup.path.as_str()).collect();
    assert_eq!(ledger, ["good-copy.md"], "duplicates 台账只收可核对条目");
    for entry in &report.excluded {
        if entry.role == KbTriageRole::Duplicate {
            let duplicate_of = &report
                .files
                .iter()
                .find(|file| file.rel_path == entry.path)
                .unwrap()
                .duplicate_of;
            assert!(
                duplicate_of.as_deref().is_some_and(|target| !target.trim().is_empty()),
                "excluded 里的 duplicate({})必须带可核对的 duplicate_of",
                entry.path
            );
        }
    }
    assert_eq!(report.counts.duplicate, 1);
    assert_eq!(report.counts.uncertain, 2);
    assert_eq!(report.counts.core, 1);
}

/// 评审修复 S4:分诊目录树超上限时如实标「截断,仅显示前 N/共 M」,不再
/// 声称「全部清点文件」;未超上限时保持全量声称。
#[test]
fn triage_prompt_labels_truncated_directory_trees_honestly() {
    fn synthetic_inventory(count: usize) -> KbInventory {
        KbInventory {
            bases: vec![KbInventoryBase {
                kb_id: "kb-1".into(),
                name: "大库".into(),
                namespace: String::new(),
                total_files: count,
            }],
            files: (0..count)
                .map(|index| KbInventoryFile {
                    rel_path: format!("f{index:03}.md"),
                    kb_id: "kb-1".into(),
                    size: 10,
                    modified_at: None,
                    content_hash: format!("{index:064x}"),
                    headings: vec![format!("第{index}篇")],
                    heading_levels: vec![1],
                    preview: "正文预览。".into(),
                    kind: KbFileKind::Markdown,
                })
                .collect(),
            total_files: count,
            total_bytes: 0,
            skipped_hidden: 0,
            scan_scope: KbScanScope::Markdown,
        }
    }
    fn first_scanned(inventory: &KbInventory) -> Vec<ScannedFile> {
        inventory
            .files
            .iter()
            .map(|file| ScannedFile {
                file: file.clone(),
                base_rel: file.rel_path.clone(),
                langless_key: file.rel_path.clone(),
                lang_tag: None,
                cjk_ratio: 0.5,
                is_empty: false,
                pure_navigation: false,
            })
            .take(1)
            .collect()
    }

    // ≤400:全量清单,声称「全部清点文件」。
    let small = synthetic_inventory(400);
    let batch = first_scanned(&small);
    let batch_refs: Vec<&ScannedFile> = batch.iter().collect();
    let prompt = build_triage_user_prompt("学会全部内容", &small, &batch_refs, BatchPass::Initial);
    assert!(
        prompt.contains("目录树（全部清点文件）"),
        "未超上限保持全量声称:{prompt}"
    );
    assert!(prompt.contains("- f399.md"), "第 400 条在列:{prompt}");

    // >400:如实标注截断与总数,不再声称全部。
    let large = synthetic_inventory(401);
    let batch = first_scanned(&large);
    let batch_refs: Vec<&ScannedFile> = batch.iter().collect();
    let prompt = build_triage_user_prompt("学会全部内容", &large, &batch_refs, BatchPass::Initial);
    assert!(
        prompt.contains("截断") && prompt.contains("共 401"),
        "截断必须如实披露:{prompt}"
    );
    assert!(
        !prompt.contains("目录树（全部清点文件）"),
        "不再声称全部:{prompt}"
    );
    assert!(prompt.contains("- f399.md"), "前 400 条仍在:{prompt}");
    assert!(
        !prompt.contains("- f400.md"),
        "第 401 条如实不显示(仅计数披露):{prompt}"
    );
}
