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

/// 从批提示词里解析「待分诊文件清单」的路径（`- 文件 {path} :: …` 行）。
fn prompt_paths(user: &str) -> Vec<String> {
    user.lines()
        .filter_map(|line| line.strip_prefix("- 文件 "))
        .map(|rest| rest.split(" ::").next().unwrap_or(rest).to_owned())
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
