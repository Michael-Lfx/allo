//! t3 分批展开的 fixture + scripted completer 断言(票面验收口径)。

use super::*;
use crate::generation::structure::topics;
use std::sync::{Arc, Mutex};

// ── fixture 助手:直接构造 inventory/triage/plan(纯数据,零 LLM)────────

#[derive(Default)]
struct NoopBroadcaster;

impl nomifun_realtime::UserEventSink for NoopBroadcaster {
    fn send_to_user(
        &self,
        _user_id: &str,
        _event: nomifun_api_types::WebSocketMessage<serde_json::Value>,
    ) {
    }
}

async fn expand_knowledge_service() -> (Arc<KnowledgeService>, tempfile::TempDir) {
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

fn write_kb_file(root: &str, rel: &str, content: &str) {
    let path = std::path::PathBuf::from(root).join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn inv_file(kb: &str, path: &str, headings: &[&str], preview: &str) -> KbInventoryFile {
    KbInventoryFile {
        rel_path: path.to_owned(),
        kb_id: kb.to_owned(),
        size: 0,
        modified_at: None,
        content_hash: format!("hash-{path}"),
        headings: headings.iter().map(|h| (*h).to_owned()).collect(),
        heading_levels: Vec::new(),
        preview: preview.chars().take(160).collect(),
        kind: KbFileKind::Markdown,
    }
}

fn single_base_inventory(kb: &str, files: Vec<KbInventoryFile>) -> KbInventory {
    KbInventory {
        bases: vec![KbInventoryBase {
            kb_id: kb.to_owned(),
            name: "教材库".to_owned(),
            namespace: String::new(),
            total_files: files.len(),
        }],
        total_files: files.len(),
        files,
        total_bytes: 0,
        skipped_hidden: 0,
        scan_scope: KbScanScope::Markdown,
    }
}

fn triage_report(kb: &str, files: &[(&str, KbTriageRole)]) -> KbTriageReport {
    let mut report = KbTriageReport {
        files: Vec::new(),
        counts: KbTriageCounts::default(),
        excluded: Vec::new(),
        duplicates: Vec::new(),
        degraded: false,
        warnings: Vec::new(),
    };
    for (path, role) in files {
        report.files.push(KbTriageFile {
            rel_path: (*path).to_owned(),
            kb_id: kb.to_owned(),
            role: *role,
            confidence: 0.9,
            reason: "fixture".to_owned(),
            duplicate_of: None,
        });
        match role {
            KbTriageRole::Core => report.counts.core += 1,
            KbTriageRole::Prerequisite => report.counts.prerequisite += 1,
            KbTriageRole::Practice => report.counts.practice += 1,
            KbTriageRole::Mixed => report.counts.mixed += 1,
            KbTriageRole::Noise => report.counts.noise += 1,
            KbTriageRole::Duplicate => report.counts.duplicate += 1,
            KbTriageRole::Uncertain => report.counts.uncertain += 1,
        }
    }
    report
}

fn anchor(kb: &str, path: &str, heading: Option<&str>, paragraph: Option<usize>) -> SourceAnchor {
    SourceAnchor {
        kb_id: kb.to_owned(),
        rel_path: path.to_owned(),
        heading_or_anchor: heading.map(str::to_owned),
        paragraph,
        snippet: None,
        anchor_uncertain: false,
    }
}

fn chapter(title: &str, anchors: Vec<SourceAnchor>) -> PlannedChapter {
    PlannedChapter {
        title: title.to_owned(),
        summary: format!("{title}章摘要"),
        learning_objectives: vec![format!("掌握{title}")],
        covers: Vec::new(),
        prerequisites: Vec::new(),
        source_anchors: anchors,
        content_type: ChapterContentType::Concept,
    }
}

/// plan 的覆盖台账:每个锚点一列 topic(covered 口径,t3 再按课时重对账)。
fn ledger_topics(plan_chapters: &[PlannedChapter]) -> Vec<CoverageTopic> {
    let mut topics: Vec<CoverageTopic> = Vec::new();
    for ch in plan_chapters {
        for anchor in &ch.source_anchors {
            topics.push(CoverageTopic {
                topic_id: format!("t{}", topics.len()),
                label: anchor
                    .heading_or_anchor
                    .clone()
                    .unwrap_or_else(|| anchor.rel_path.clone()),
                source_anchors: vec![anchor.clone()],
                assigned_chapters: vec![ch.title.clone()],
                status: TopicStatus::Covered,
                reason: None,
                level: None,
            });
        }
    }
    topics
}

fn plan_fixture(goal: &str, chapters: Vec<PlannedChapter>) -> SourcePlan {
    let coverage = CoverageLedger {
        topics: ledger_topics(&chapters),
        ..CoverageLedger::default()
    };
    SourcePlan {
        goal: goal.to_owned(),
        chapters,
        coverage,
        ..SourcePlan::default()
    }
}

// ── scripted completer:按系统提示词路由,记录提示词与批大小 ─────────────

/// 展开阶段的脚本 completer:`route(system, user) -> 回包`。
struct ExpandCompleter<F>
where
    F: Fn(&str, &str) -> Result<String, String> + Send + Sync,
{
    route: F,
    calls: std::sync::atomic::AtomicUsize,
    prompts: Mutex<Vec<String>>,
}

impl<F> ExpandCompleter<F>
where
    F: Fn(&str, &str) -> Result<String, String> + Send + Sync,
{
    fn new(route: F) -> Self {
        Self {
            route,
            calls: std::sync::atomic::AtomicUsize::new(0),
            prompts: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl<F> LearningCompleter for ExpandCompleter<F>
where
    F: Fn(&str, &str) -> Result<String, String> + Send + Sync,
{
    async fn complete(
        &self,
        _model_override: Option<(&str, &str)>,
        _system: &str,
        _user: &str,
        _max_tokens: u32,
    ) -> Result<String, AppError> {
        Err(AppError::Internal("展开测试只走 json 接缝".into()))
    }

    async fn complete_json(
        &self,
        _model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        _max_tokens: u32,
        _temperature: Option<f32>,
    ) -> Result<String, AppError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.prompts
            .lock()
            .unwrap()
            .push(format!("{system}

{user}"));
        (self.route)(system, user).map_err(AppError::Internal)
    }
}

/// 从展开提示词解析本批文件路径(`--- FILE: path ---` 行)。
fn prompt_file_paths(user: &str) -> Vec<String> {
    user.lines()
        .filter_map(|line| line.strip_prefix("--- FILE: "))
        .map(|rest| rest.strip_suffix(" ---").unwrap_or(rest).to_owned())
        .collect()
}

// ── slice 1:干净教材展开 ────────────────────────────────────────────────

/// 干净教材:两章、每章一个批次、模型回包合规——蓝图模块按 plan 章节顺序
/// 落位,课时可回溯来源,覆盖报告成章。
#[tokio::test]
async fn expansion_produces_modules_in_plan_chapter_order() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "ch01.md",
        "# 排序入门\n\n排序的基本概念。\n\n## 比较排序\n\n比较排序的下界。\n",
    );
    write_kb_file(&root, "ch02.md", "# 图\n\n图的表示。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![
            inv_file(&kb, "ch01.md", &["排序入门", "比较排序"], "排序的基本概念。"),
            inv_file(&kb, "ch02.md", &["图"], "图的表示。"),
        ],
    );
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core), ("ch02.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会排序与图",
        vec![
            chapter(
                "排序入门",
                vec![
                    anchor(&kb, "ch01.md", Some("排序入门"), None),
                    anchor(&kb, "ch01.md", Some("比较排序"), None),
                ],
            ),
            chapter("图", vec![anchor(&kb, "ch02.md", Some("图"), None)]),
        ],
    );
    let completer = ExpandCompleter::new(|system, user| {
        assert!(system.contains("章节展开"), "展开阶段的系统提示词");
        if user.contains("章节:「排序入门」") {
            Ok(r#"{"sections": [{"title": "排序入门", "lessons": [
                {"title": "排序入门", "purpose": "掌握排序概念", "sources": ["ch01.md"], "est": 20, "content_type": "concept"},
                {"title": "比较排序", "purpose": "理解排序下界", "sources": ["ch01.md"], "anchor": "比较排序", "est": 25, "content_type": "concept"}
            ]}]}"#
                .to_owned())
        } else {
            Ok(r#"{"sections": [{"title": "图", "lessons": [
                {"title": "图", "purpose": "理解图的表示", "sources": ["ch02.md"], "est": 15, "content_type": "concept"}
            ]}]}"#
                .to_owned())
        }
    });
    let outcome = run_kb_expansion(
        &knowledge,
        &inventory,
        &triage,
        &plan,
        "学会排序与图",
        Some(&completer),
        None,
    )
    .await
    .unwrap()
    .expect("干净教材应展开出蓝图");
    let blueprint = outcome.blueprint;
    assert_eq!(
        blueprint
            .modules
            .iter()
            .map(|module| module.title.as_str())
            .collect::<Vec<_>>(),
        vec!["排序入门", "图"],
        "模块顺序 = plan 章节顺序(标题忠实)"
    );
    assert_eq!(blueprint.modules[0].lessons.len(), 2);
    assert_eq!(blueprint.modules[0].lessons[0].sources[0].path, "ch01.md");
    assert_eq!(
        blueprint.modules[0].lessons[1].anchor.as_deref(),
        Some("比较排序"),
        "拆分课时带 H2 原文锚点"
    );
    assert_eq!(blueprint.modules[1].lessons.len(), 1);
    let coverage = outcome.coverage;
    assert_eq!(coverage.chapters.len(), 2);
    assert!(coverage.chapters.iter().all(|c| !c.skipped), "两章都有课时");
    assert_eq!(coverage.lessons.len(), 3);
    assert!(
        coverage.topics.iter().all(|t| t.status == TopicStatus::Covered),
        "锚点 topic 全部被课时覆盖: {:?}",
        coverage.topics
    );
    assert_eq!(completer.calls(), 2, "每章一批");
}

// ── slice 2:来源/锚点防幻觉(validSet)──────────────────────────────────

/// 编造的文件路径 → 课时整体拒收;编造的锚点标题 → 锚点值拒收
/// (课时保留、anchor_uncertain=true + warning,不落编造文本)。
#[tokio::test]
async fn fabricated_paths_and_anchors_are_rejected_by_validset() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "ch01.md",
        "# 排序入门\n\n排序的基本概念。\n\n## 比较排序\n\n比较排序的下界。\n",
    );
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![inv_file(&kb, "ch01.md", &["排序入门", "比较排序"], "排序的基本概念。")],
    );
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会排序",
        vec![chapter(
            "排序入门",
            vec![
                anchor(&kb, "ch01.md", Some("排序入门"), None),
                anchor(&kb, "ch01.md", Some("比较排序"), None),
            ],
        )],
    );
    let completer = ExpandCompleter::new(|system, user| {
        assert!(system.contains("章节展开"));
        assert_eq!(prompt_file_paths(user), vec!["ch01.md"]);
        Ok(r#"{"sections": [{"title": "排序入门", "lessons": [
            {"title": "幽灵课时", "purpose": "编造来源", "sources": ["ghost.md"]},
            {"title": "锚点幻觉", "purpose": "编造锚点", "sources": ["ch01.md"], "anchor": "原文没有的标题"},
            {"title": "比较排序", "purpose": "合法课时", "sources": ["ch01.md"], "anchor": "比较排序"}
        ]}]}"#
            .to_owned())
    });
    let outcome = run_kb_expansion(
        &knowledge,
        &inventory,
        &triage,
        &plan,
        "学会排序",
        Some(&completer),
        None,
    )
    .await
    .unwrap()
    .expect("合法课时仍在");
    let blueprint = outcome.blueprint;
    let lessons: Vec<&BlueprintLesson> = blueprint.modules.iter().flat_map(|m| m.lessons.iter()).collect();
    assert_eq!(lessons.len(), 2, "编造来源的课时被拒,只剩两条: {:?}", lessons.iter().map(|l| &l.title).collect::<Vec<_>>());
    assert!(
        lessons.iter().all(|lesson| lesson
            .cited_sources()
            .iter().all(|span| span.path == "ch01.md")),
        "ghost.md 不落库"
    );
    let hallucinated = lessons.iter().find(|l| l.title == "锚点幻觉").unwrap();
    assert_eq!(hallucinated.anchor, None, "编造的锚点值不落库");
    let valid = lessons.iter().find(|l| l.title == "比较排序").unwrap();
    assert_eq!(valid.anchor.as_deref(), Some("比较排序"));
    let report = outcome
        .coverage
        .lessons
        .iter()
        .find(|l| l.title == "锚点幻觉")
        .unwrap();
    assert!(report.anchor_uncertain, "编造锚点 → anchor_uncertain");
    assert!(
        outcome
            .coverage
            .warnings
            .iter()
            .any(|warning| warning.contains("ghost.md 不在章节文件清单")),
        "防幻觉 warning: {:?}",
        outcome.coverage.warnings
    );
    assert!(
        outcome
            .coverage
            .warnings
            .iter()
            .any(|warning| warning.contains("原文没有的标题") && warning.contains("防幻觉")),
        "锚点拒收 warning: {:?}",
        outcome.coverage.warnings
    );
}

// ── slice 3:截断自愈 / 单文件兜底 / 空结构硬失败 ────────────────────────

/// 通用展开脚本:按提示词里的文件清单逐文件产一课(标题 = 文件名)。
fn generic_expansion_reply(user: &str) -> String {
    let paths = prompt_file_paths(user);
    let lessons: Vec<String> = paths
        .iter()
        .map(|path| {
            format!(
                "{{\"title\": \"{path}\", \"purpose\": \"p\", \"sources\": [\"{path}\"]}}"
            )
        })
        .collect();
    format!(
        "{{\"sections\": [{{\"title\": \"\", \"lessons\": [{}]}}]}}",
        lessons.join(", ")
    )
}

/// 模拟 40 文件批 JSON 截断:批 >2 文件时回包从中间剪断;≤2 文件时成功。
/// 断言拆半后覆盖全部文件,批大小序列 8→4→2/2→4→2/2。
#[tokio::test]
async fn truncated_batch_json_splits_until_all_files_are_covered() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    let mut inventory_files = Vec::new();
    let mut triage_files = Vec::new();
    let mut anchors = Vec::new();
    for index in 0..8 {
        let path = format!("f{index}.md");
        write_kb_file(&root, &path, &format!("# 标题{index}\n\n正文{index}。\n"));
        inventory_files.push(inv_file(kb_id.as_str(), &path, &[&format!("标题{index}")], &format!("正文{index}。")));
        triage_files.push((path.clone(), KbTriageRole::Core));
        anchors.push(anchor(kb_id.as_str(), &path, Some(&format!("标题{index}")), None));
    }
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, inventory_files);
    let triage_entries: Vec<(&str, KbTriageRole)> = triage_files
        .iter()
        .map(|(path, role)| (path.as_str(), *role))
        .collect();
    let triage = triage_report(&kb, &triage_entries);
    let plan = plan_fixture("学会 Rust", vec![chapter("第 1 章", anchors)]);
    let completer = ExpandCompleter::new(|system, user| {
        assert!(system.contains("章节展开"));
        if prompt_file_paths(user).len() > 2 {
            // 从字符串中间剪断(模拟输出 token 截断)。
            let reply = generic_expansion_reply(user);
            Ok(reply.chars().take(reply.chars().count() / 2).collect())
        } else {
            Ok(generic_expansion_reply(user))
        }
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会 Rust", Some(&completer), None)
        .await
        .unwrap()
        .expect("拆半后展开成功");
    let cited: HashSet<&str> = outcome
        .blueprint
        .modules
        .iter()
        .flat_map(|module| module.lessons.iter())
        .flat_map(|lesson| lesson.cited_sources().into_iter().map(|span| span.path.as_str()))
        .collect();
    for index in 0..8 {
        assert!(cited.contains(format!("f{index}.md").as_str()), "f{index}.md 被覆盖");
    }
    let batch_sizes: Vec<usize> = completer
        .prompts()
        .iter()
        .map(|prompt| prompt_file_paths(prompt).len())
        .collect();
    assert_eq!(batch_sizes, vec![8, 4, 2, 2, 4, 2, 2], "整批 → 左半递归到底 → 右半递归到底");
}

/// 单文件兜底:某文件在单文件批上仍解析失败 → H1/文件名兜底一课
/// (fallback=true + warning),不丢内容、不拖垮同批其它文件。
#[tokio::test]
async fn single_file_parse_failure_falls_back_to_one_lesson() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(&root, "good.md", "# 好文件\n\n正文。\n");
    write_kb_file(&root, "bad.md", "# 坏文件\n\n正文。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![
            inv_file(&kb, "good.md", &["好文件"], "正文。"),
            inv_file(&kb, "bad.md", &["坏文件"], "正文。"),
        ],
    );
    let triage = triage_report(&kb, &[("good.md", KbTriageRole::Core), ("bad.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会",
        vec![chapter(
            "第 1 章",
            vec![
                anchor(&kb, "good.md", Some("好文件"), None),
                anchor(&kb, "bad.md", Some("坏文件"), None),
            ],
        )],
    );
    let completer = ExpandCompleter::new(|_system, user| {
        let paths = prompt_file_paths(user);
        if paths == ["bad.md"] {
            Ok("这不是 JSON".to_owned())
        } else if paths.len() == 1 {
            Ok(generic_expansion_reply(user))
        } else {
            // 整批只报 good.md(部分覆盖),bad.md 递归到单文件后仍失败。
            Ok(r#"{"sections": [{"title": "", "lessons": [{"title": "好文件", "purpose": "p", "sources": ["good.md"]}]}]}"#.to_owned())
        }
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", Some(&completer), None)
        .await
        .unwrap()
        .expect("兜底课仍在");
    let lessons: Vec<&BlueprintLesson> = outcome.blueprint.modules.iter().flat_map(|m| m.lessons.iter()).collect();
    let fallback = lessons.iter().find(|l| l.title == "坏文件").expect("坏文件有兜底课");
    assert!(
        outcome.coverage.lessons.iter().any(|l| l.title == "坏文件" && l.fallback),
        "兜底课标记 fallback=true"
    );
    assert_eq!(fallback.cited_sources()[0].path, "bad.md");
    assert!(outcome.coverage.degraded, "兜底发生 → degraded");
    assert!(
        outcome.coverage.warnings.iter().any(|w| w.contains("bad.md") && w.contains("兜底")),
        "兜底 warning: {:?}",
        outcome.coverage.warnings
    );
}

/// 空结构硬失败:全部章节零课时 → Err,不落空课程。
#[tokio::test]
async fn empty_structure_fails_hard() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(&root, "ch01.md", "# 标题\n\n正文。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, vec![inv_file(&kb, "ch01.md", &["标题"], "正文。")]);
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会",
        vec![chapter("第 1 章", vec![anchor(&kb, "ch01.md", Some("标题"), None)])],
    );
    let completer = ExpandCompleter::new(|_system, _user| {
        Ok(r#"{"sections": [{"title": "", "lessons": []}]}"#.to_owned())
    });
    let error = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", Some(&completer), None)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("0 课时"),
        "空结构错误可读: {error}"
    );
}

/// 单章零课时不拖垮全课:该章显式 skipped + reason,其余章照常。
#[tokio::test]
async fn an_empty_chapter_is_skipped_with_reason_but_the_course_survives() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(&root, "promo.md", "# 推广页\n\n出版社介绍。\n");
    write_kb_file(&root, "core.md", "# 正文\n\n核心内容。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![
            inv_file(&kb, "promo.md", &["推广页"], "出版社介绍。"),
            inv_file(&kb, "core.md", &["正文"], "核心内容。"),
        ],
    );
    let triage = triage_report(&kb, &[("promo.md", KbTriageRole::Mixed), ("core.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会",
        vec![
            chapter("推广", vec![anchor(&kb, "promo.md", Some("推广页"), None)]),
            chapter("正文", vec![anchor(&kb, "core.md", Some("正文"), None)]),
        ],
    );
    let completer = ExpandCompleter::new(|_system, user| {
        if user.contains("章节:「推广」") {
            Ok(r#"{"sections": []}"#.to_owned())
        } else {
            Ok(generic_expansion_reply(user))
        }
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", Some(&completer), None)
        .await
        .unwrap()
        .expect("全课不因单章失败而失败");
    let skipped = outcome.coverage.chapters.iter().find(|c| c.chapter == "推广").unwrap();
    assert!(skipped.skipped);
    assert!(skipped.skip_reason.as_deref().unwrap_or_default().contains("未产出课时"));
    assert_eq!(outcome.blueprint.modules.len(), 1, "只有正文章成模块");
    assert_eq!(outcome.blueprint.modules[0].title, "正文");
}

// ── slice 4:混合文件 / 无标题文件 / 代码文件 ────────────────────────────

/// 混合文件:批里只给该章锚点命中的段(会议记录不进提示词),lesson 只
/// 引核心 heading;文件不被整文件排除。
#[tokio::test]
async fn mixed_files_only_offer_the_chapters_anchored_segments() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "mixed.md",
        "# 核心章节\n\n核心方法正文。\n\n# 会议记录\n\n2024 年会纪要内容。\n",
    );
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![inv_file(&kb, "mixed.md", &["核心章节", "会议记录"], "核心方法正文。")],
    );
    let triage = triage_report(&kb, &[("mixed.md", KbTriageRole::Mixed)]);
    let mut plan = plan_fixture(
        "学会核心方法",
        vec![chapter("核心", vec![anchor(&kb, "mixed.md", Some("核心章节"), None)])],
    );
    // t2 台账口径:混合文件未进章节的段是 uncertain topic(保留可见)。
    plan.coverage.topics.push(CoverageTopic {
        topic_id: "t-meeting".to_owned(),
        label: "会议记录".to_owned(),
        source_anchors: vec![anchor(&kb, "mixed.md", Some("会议记录"), None)],
        assigned_chapters: Vec::new(),
        status: TopicStatus::Uncertain,
        reason: None,
        level: None,
    });
    let completer = ExpandCompleter::new(|system, user| {
        assert!(system.contains("章节展开"));
        let prompt = user.to_owned();
        assert!(
            prompt.contains("核心方法正文。"),
            "核心段进批"
        );
        assert!(
            !prompt.contains("2024 年会纪要内容。"),
            "会议记录段不进批(只取该章 topic 的锚点段):{}",
            prompt
        );
        Ok(r#"{"sections": [{"title": "核心", "lessons": [
            {"title": "核心章节", "purpose": "p", "sources": ["mixed.md"], "anchor": "核心章节"}
        ]}]}"#
            .to_owned())
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会核心方法", Some(&completer), None)
        .await
        .unwrap()
        .expect("混合文件展开成功");
    let lessons: Vec<&BlueprintLesson> = outcome.blueprint.modules.iter().flat_map(|m| m.lessons.iter()).collect();
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].anchor.as_deref(), Some("核心章节"), "lesson 只引核心 heading");
    let file = outcome.coverage.files.iter().find(|f| f.path == "mixed.md").unwrap();
    assert_eq!(file.lessons.len(), 1, "文件未被整文件排除");
    // 会议记录 topic(plan 台账里未覆盖)显式 uncertain 可见,不静默。
    let meeting = outcome
        .coverage
        .topics
        .iter()
        .find(|t| t.label == "会议记录")
        .expect("会议记录 topic 在台账里可见");
    assert_eq!(meeting.status, TopicStatus::Uncertain);
    assert!(
        outcome
            .coverage
            .warnings
            .iter()
            .any(|w| w.contains("会议记录") && w.contains("未被任何课时覆盖")),
        "缺口醒目披露: {:?}",
        outcome.coverage.warnings
    );
}

/// 无标题文件:按段落序号成课(anchor = 第 N 段),内容不丢;无法定位时
/// anchor_uncertain + warning。
#[tokio::test]
async fn headingless_files_anchor_lessons_by_paragraph() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "notes.md",
        "第一段引入内容。\n\n第二段是核心机制说明。\n\n第三段是收尾。\n",
    );
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, vec![inv_file(&kb, "notes.md", &[], "第一段引入内容。")]);
    let triage = triage_report(&kb, &[("notes.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会机制",
        vec![chapter("机制", vec![anchor(&kb, "notes.md", None, Some(2))])],
    );
    let completer = ExpandCompleter::new(|system, user| {
        assert!(system.contains("章节展开"));
        assert!(
            user.contains("第二段是核心机制说明。"),
            "锚点段内容进批"
        );
        assert!(user.contains("按段落序号锚定"), "无标题文件的锚定方式披露");
        Ok(r#"{"sections": [{"title": "机制", "lessons": [
            {"title": "核心机制", "purpose": "p", "sources": ["notes.md"], "paragraph": 2}
        ]}]}"#
            .to_owned())
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会机制", Some(&completer), None)
        .await
        .unwrap()
        .expect("无标题文件成课");
    let lessons: Vec<&BlueprintLesson> = outcome.blueprint.modules.iter().flat_map(|m| m.lessons.iter()).collect();
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].anchor.as_deref(), Some("第 2 段"), "段落序号锚");
    let report = &outcome.coverage.lessons[0];
    assert_eq!(report.sources[0].paragraph, Some(2));
    assert!(!report.anchor_uncertain, "段落锚是精确定位");
    assert!(
        outcome.coverage.topics.iter().any(|t| t.status == TopicStatus::Covered),
        "段落 topic 被课时覆盖"
    );
}

/// 代码为主文件:结构信号披露「代码为主」(整体一课的提示信号);无标题
/// 代码文件兜底课时用文件名(不发明「第 N 章」)。
#[tokio::test]
async fn code_files_are_flagged_and_fall_back_to_the_file_name() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "demo.rs.md",
        "代码示例说明:\n\n```rust\nfn main() {\n    let sorted = vec![3, 1, 2];\n    sorted.sort();\n    println!(\"{:?}\", sorted);\n}\n\nfn helper() -> u32 {\n    42\n}\n\nfn another() -> u32 {\n    7\n}\n```\n",
    );
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, vec![inv_file(&kb, "demo.rs.md", &[], "代码示例说明:")]);
    let triage = triage_report(&kb, &[("demo.rs.md", KbTriageRole::Practice)]);
    let plan = plan_fixture(
        "学会排序实现",
        vec![chapter("练习", vec![SourceAnchor {
            kb_id: kb.clone(),
            rel_path: "demo.rs.md".to_owned(),
            heading_or_anchor: None,
            paragraph: None,
            snippet: None,
            anchor_uncertain: false,
        }])],
    );
    let completer = ExpandCompleter::new(|system, user| {
        assert!(system.contains("章节展开"));
        assert!(user.contains("代码为主"), "代码文件信号披露:{}", user);
        // 整批解析失败 → 单文件兜底(H1 缺失 → 文件名)。
        Ok("{{{bad".to_owned())
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会排序实现", Some(&completer), None)
        .await
        .unwrap()
        .expect("兜底成课");
    let lessons: Vec<&BlueprintLesson> = outcome.blueprint.modules.iter().flat_map(|m| m.lessons.iter()).collect();
    assert_eq!(lessons.len(), 1, "代码文件整体一课(兜底)");
    assert_eq!(lessons[0].title, "demo.rs", "文件名兜底标题(无 H1)");
    let report = &outcome.coverage.lessons[0];
    assert!(report.fallback);
    assert!(report.anchor_uncertain, "无标题兜底课 anchor_uncertain");
    assert_eq!(report.content_type, ChapterContentType::Practice, "practice 角色归属");
}

// ── slice 5:调用失败上抛 / 取消 / 大教材 / 提示词规则 / 无 completer ────

/// 调用层失败:不重试不拆半,原样上抛,错误结构化(章/批/文件可追溯)。
#[tokio::test]
async fn call_failures_propagate_with_chapter_context() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(&root, "ch01.md", "# 标题\n\n正文。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, vec![inv_file(&kb, "ch01.md", &["标题"], "正文。")]);
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会",
        vec![chapter("重要的一章", vec![anchor(&kb, "ch01.md", Some("标题"), None)])],
    );
    let completer = ExpandCompleter::new(|_system, _user| Err("provider down".to_owned()));
    let error = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", Some(&completer), None)
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("重要的一章"), "错误带章上下文: {message}");
    assert!(message.contains("ch01.md"), "错误带文件上下文: {message}");
    assert!(message.contains("provider down"), "原始错误不吞: {message}");
    assert_eq!(completer.calls(), 1, "调用层失败不拆半重试");
}

/// 用户取消:预取消 → Conflict(CANCELLED_MESSAGE),completer 零调用。
#[tokio::test]
async fn cancellation_propagates_as_conflict() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(&root, "ch01.md", "# 标题\n\n正文。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, vec![inv_file(&kb, "ch01.md", &["标题"], "正文。")]);
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会",
        vec![chapter("章", vec![anchor(&kb, "ch01.md", Some("标题"), None)])],
    );
    let completer = ExpandCompleter::new(|_system, _user| Ok("{}".to_owned()));
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let error = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", Some(&completer), Some(&token))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(ref message) if message.contains("取消")), "{error}");
    assert_eq!(completer.calls(), 0);
}

/// 无 completer:Ok(None),调用方回退现行引擎路径。
#[tokio::test]
async fn missing_completer_returns_none() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(&root, "ch01.md", "# 标题\n\n正文。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, vec![inv_file(&kb, "ch01.md", &["标题"], "正文。")]);
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会",
        vec![chapter("章", vec![anchor(&kb, "ch01.md", Some("标题"), None)])],
    );
    assert!(run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", None, None)
        .await
        .unwrap()
        .is_none());
}

/// 提示词承载粒度/标题忠实/合并规则(长文按 H3 拆、短文合并、不发明编号、
/// 每课 3000-8000 字目标)与逐标题字数(供模型自适应拆分)。
#[tokio::test]
async fn expand_prompt_carries_granularity_rules_and_heading_sizes() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "ch01.md",
        "# 长章\n\n导语。\n\n## 大节\n\n很长的小节。\n\n### 子节甲\n\n内容。\n",
    );
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![inv_file(&kb, "ch01.md", &["长章", "大节", "子节甲"], "导语。")],
    );
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core)]);
    let plan = plan_fixture(
        "学会",
        vec![chapter("长章", vec![anchor(&kb, "ch01.md", Some("长章"), None)])],
    );
    let completer = ExpandCompleter::new(|_system, _user| Ok(r#"{"sections": []}"#.to_owned()));
    let _ = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", Some(&completer), None).await;
    let prompt = completer.prompts()[0].clone();
    for rule in [
        "3000-8000",
        "按 H3 拆分",
        "不足 1000 字",
        "合并",
        "不得发明「第 N 章/课」",
        "原书有编号则保留",
    ] {
        assert!(prompt.contains(rule), "提示词缺规则「{rule}」:\n{prompt}");
    }
    assert!(
        prompt.contains("大节(") && prompt.contains("字)"),
        "标题清单带逐段字数: {}",
        prompt.lines().filter(|l| l.starts_with("- ")).collect::<Vec<_>>().join(" / ")
    );
}

/// 大教材(105 文件 / 20 章):每章一批展开,模块顺序 = plan 章节顺序
/// (无未解释倒挂),所有章节有课时,每课来源可回溯,覆盖报告成章。
#[tokio::test]
async fn large_textbook_expands_all_chapters_in_plan_order() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    let mut inventory_files = Vec::new();
    let mut triage_files: Vec<(String, KbTriageRole)> = Vec::new();
    let mut chapters = Vec::new();
    let files_per_chapter = 6;
    for chapter_index in 0..20 {
        let mut anchors = Vec::new();
        for file_index in 0..files_per_chapter {
            let path = format!("ch{chapter_index:02}_{file_index}.md");
            write_kb_file(&root, &path, &format!("# 第{}章标题{file_index}\n\n第 {chapter_index} 章第 {file_index} 节正文。\n", chapter_index + 1));
            inventory_files.push(inv_file(
                kb_id.as_str(),
                &path,
                &[&format!("第{}章标题{file_index}", chapter_index + 1)],
                &format!("第 {chapter_index} 章第 {file_index} 节正文。"),
            ));
            triage_files.push((path.clone(), KbTriageRole::Core));
            anchors.push(anchor(kb_id.as_str(), &path, Some(&format!("第{}章标题{file_index}", chapter_index + 1)), None));
        }
        chapters.push(chapter(&format!("第 {} 章", chapter_index + 1), anchors));
    }
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, inventory_files);
    let triage_entries: Vec<(&str, KbTriageRole)> = triage_files
        .iter()
        .map(|(path, role)| (path.as_str(), *role))
        .collect();
    let triage = triage_report(&kb, &triage_entries);
    let plan = plan_fixture("通读教材", chapters);
    let completer = ExpandCompleter::new(|system, user| {
        assert!(system.contains("章节展开"));
        Ok(generic_expansion_reply(user))
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "通读教材", Some(&completer), None)
        .await
        .unwrap()
        .expect("大教材展开成功");
    assert_eq!(completer.calls(), 20, "每章一批(20 章,无截断重试)");
    assert_eq!(
        outcome
            .blueprint
            .modules
            .iter()
            .map(|module| module.title.as_str())
            .collect::<Vec<_>>(),
        (1..=20).map(|index| format!("第 {index} 章").to_string()).collect::<Vec<String>>(),
        "模块顺序 = plan 章节顺序"
    );
    assert!(
        outcome.coverage.chapters.iter().all(|chapter| !chapter.skipped),
        "所有章都有课时"
    );
    let valid_paths: HashSet<&str> = inventory.files.iter().map(|file| file.rel_path.as_str()).collect();
    let lessons: Vec<&BlueprintLesson> = outcome
        .blueprint
        .modules
        .iter()
        .flat_map(|module| module.lessons.iter())
        .collect();
    assert_eq!(lessons.len(), 20 * files_per_chapter);
    assert!(lessons.iter().all(|lesson| lesson
        .cited_sources()
        .iter()
        .all(|span| valid_paths.contains(span.path.as_str()))));
    assert!(
        outcome.coverage.files.iter().all(|file| !file.lessons.is_empty()),
        "每个文件都有课时归属(覆盖报告可读)"
    );
    assert!(
        outcome.coverage.topics.iter().all(|topic| topic.status == TopicStatus::Covered),
        "全部 topic 覆盖"
    );
}


/// 超过 40 文件的章:按 40/批分块,块序 = 章锚点序,全覆盖。
#[tokio::test]
async fn chapters_beyond_forty_files_are_chunked_in_anchor_order() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    let mut inventory_files = Vec::new();
    let mut triage_files: Vec<(String, KbTriageRole)> = Vec::new();
    let mut anchors = Vec::new();
    for index in 0..45 {
        let path = format!("big{index:02}.md");
        write_kb_file(&root, &path, &format!("# 标题{index}\n\n正文{index}。\n"));
        inventory_files.push(inv_file(kb_id.as_str(), &path, &[&format!("标题{index}")], &format!("正文{index}。")));
        triage_files.push((path.clone(), KbTriageRole::Core));
        anchors.push(anchor(kb_id.as_str(), &path, Some(&format!("标题{index}")), None));
    }
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(&kb, inventory_files);
    let triage_entries: Vec<(&str, KbTriageRole)> = triage_files
        .iter()
        .map(|(path, role)| (path.as_str(), *role))
        .collect();
    let triage = triage_report(&kb, &triage_entries);
    let plan = plan_fixture("学会", vec![chapter("大章", anchors)]);
    let completer = ExpandCompleter::new(|_system, user| Ok(generic_expansion_reply(user)));
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会", Some(&completer), None)
        .await
        .unwrap()
        .expect("大章展开成功");
    let batch_sizes: Vec<usize> = completer
        .prompts()
        .iter()
        .map(|prompt| prompt_file_paths(prompt).len())
        .collect();
    assert_eq!(batch_sizes, vec![40, 5], "40 上限分块,锚点序");
    let lessons: Vec<&BlueprintLesson> = outcome.blueprint.modules.iter().flat_map(|m| m.lessons.iter()).collect();
    assert_eq!(lessons.len(), 45, "全覆盖");
    assert_eq!(
        lessons.first().unwrap().cited_sources()[0].path,
        "big00.md",
        "锚点序在首"
    );
}

// ── slice 6:覆盖对账父子滚入(h2)────────────────────────────────────────

/// 带层级的台账 topic(h2 父子滚入夹具:level 1=章、2=节;t2 口径先记
/// covered,t3 按课时锚点重对账)。
fn leveled_topic(kb: &str, path: &str, heading: &str, level: u8) -> CoverageTopic {
    CoverageTopic {
        topic_id: format!("{path}#{heading}"),
        label: heading.to_owned(),
        source_anchors: vec![anchor(kb, path, Some(heading), None)],
        assigned_chapters: Vec::new(),
        status: TopicStatus::Covered,
        reason: None,
        level: Some(level),
    }
}

/// h2 票面单测一:「H1 + 2×H2,一 H2 成课」——章级 H1 由被覆盖的子节滚入
/// covered(rolled_up),不再发「未被任何课时覆盖」假警报;另一 H2 保持
/// 自身状态(缺口逐条可见)。
#[tokio::test]
async fn chapter_topics_roll_up_coverage_from_covered_subsection_lessons() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "ch01.md",
        "# 第三章 ArrayList\n\n线性表与数组实现。\n\n## 3.1 划分方法\n\n把数组划分为逻辑段。\n\n## 3.2 复杂度\n\n均摊复杂度分析。\n",
    );
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![inv_file(
            &kb,
            "ch01.md",
            &["第三章 ArrayList", "3.1 划分方法", "3.2 复杂度"],
            "线性表与数组实现。",
        )],
    );
    let triage = triage_report(&kb, &[("ch01.md", KbTriageRole::Core)]);
    let mut plan = plan_fixture(
        "学会数组实现",
        vec![chapter(
            "数组",
            vec![
                anchor(&kb, "ch01.md", Some("第三章 ArrayList"), None),
                anchor(&kb, "ch01.md", Some("3.1 划分方法"), None),
                anchor(&kb, "ch01.md", Some("3.2 复杂度"), None),
            ],
        )],
    );
    plan.coverage.topics = vec![
        leveled_topic(&kb, "ch01.md", "第三章 ArrayList", 1),
        leveled_topic(&kb, "ch01.md", "3.1 划分方法", 2),
        leveled_topic(&kb, "ch01.md", "3.2 复杂度", 2),
    ];
    let completer = ExpandCompleter::new(|_system, user| {
        assert!(user.contains("章节:「数组」"));
        Ok(r#"{"sections": [{"title": "数组", "lessons": [
            {"title": "划分 MyArrayList 的方法", "purpose": "掌握划分", "sources": ["ch01.md"], "anchor": "3.1 划分方法", "est": 20, "content_type": "concept"}
        ]}]}"#
            .to_owned())
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会数组实现", Some(&completer), None)
        .await
        .unwrap()
        .expect("章内一节成课,展开成功");
    let topic = |label: &str| {
        outcome
            .coverage
            .topics
            .iter()
            .find(|t| t.label == label)
            .unwrap_or_else(|| panic!("topic「{label}」应在台账里"))
    };
    let chapter_topic = topic("第三章 ArrayList");
    assert_eq!(
        chapter_topic.status,
        TopicStatus::Covered,
        "任一后代成课 → 章级 topic 滚入 covered: {:?}",
        chapter_topic
    );
    assert!(chapter_topic.rolled_up, "滚入带 rolled_up 标记");
    assert_eq!(
        chapter_topic.assigned_lessons,
        vec!["划分 MyArrayList 的方法".to_owned()],
        "滚入覆盖可回溯到后代的课时"
    );
    let covered_sub = topic("3.1 划分方法");
    assert_eq!(covered_sub.status, TopicStatus::Covered);
    assert!(!covered_sub.rolled_up, "直接覆盖不带滚入标记");
    let gap = topic("3.2 复杂度");
    assert_eq!(gap.status, TopicStatus::Uncertain, "子缺口保持自身状态");
    assert!(!gap.rolled_up);
    assert!(
        outcome
            .coverage
            .warnings
            .iter()
            .any(|w| w.contains("3.2 复杂度") && w.contains("未被任何课时覆盖")),
        "子缺口警告逐条可见: {:?}",
        outcome.coverage.warnings
    );
    assert!(
        !outcome
            .coverage
            .warnings
            .iter()
            .any(|w| w.contains("第三章 ArrayList")),
        "章级假警报消灭(章已由子节滚入覆盖): {:?}",
        outcome.coverage.warnings
    );
}

/// h2 票面单测二:「H1 无任何子成课且无 skip」——警告照发,维持现状三态;
/// 同时钉住滚入不跨文件:后一个文件被覆盖的 H2 不是本章 H1 的后代。
#[tokio::test]
async fn zero_covered_descendants_keep_the_uncovered_warning() {
    let (knowledge, _data_dir) = expand_knowledge_service().await;
    let (kb_id, root) = create_blank_base(&knowledge, "教材库").await;
    write_kb_file(
        &root,
        "ch01.md",
        "# 第三章 ArrayList\n\n线性表与数组实现。\n\n## 3.1 划分方法\n\n把数组划分为逻辑段。\n\n## 3.2 复杂度\n\n均摊复杂度分析。\n",
    );
    // 以 H2 开头的补充文件:全局看像 ch01 章的子节,但不在同一文件——
    // 跨文件滚入会把它误认作「第三章」的后代。
    write_kb_file(&root, "ch02.md", "## 图的补充\n\n图遍历的补充说明。\n");
    let kb = kb_id.as_str().to_owned();
    let inventory = single_base_inventory(
        &kb,
        vec![
            inv_file(
                &kb,
                "ch01.md",
                &["第三章 ArrayList", "3.1 划分方法", "3.2 复杂度"],
                "线性表与数组实现。",
            ),
            inv_file(&kb, "ch02.md", &["图的补充"], "图遍历的补充说明。"),
        ],
    );
    let triage = triage_report(
        &kb,
        &[("ch01.md", KbTriageRole::Core), ("ch02.md", KbTriageRole::Core)],
    );
    // 只有 ch02 成课;ch01 的章/节全都没课时锚点,也没显式 skip。
    let mut plan = plan_fixture(
        "学会图遍历补充",
        vec![chapter("图补充", vec![anchor(&kb, "ch02.md", Some("图的补充"), None)])],
    );
    plan.coverage.topics = vec![
        leveled_topic(&kb, "ch01.md", "第三章 ArrayList", 1),
        leveled_topic(&kb, "ch01.md", "3.1 划分方法", 2),
        leveled_topic(&kb, "ch01.md", "3.2 复杂度", 2),
        leveled_topic(&kb, "ch02.md", "图的补充", 2),
    ];
    let completer = ExpandCompleter::new(|_system, user| {
        assert!(user.contains("章节:「图补充」"));
        Ok(r#"{"sections": [{"title": "图补充", "lessons": [
            {"title": "图的补充说明", "purpose": "补全图遍历", "sources": ["ch02.md"], "anchor": "图的补充", "est": 15, "content_type": "concept"}
        ]}]}"#
            .to_owned())
    });
    let outcome = run_kb_expansion(&knowledge, &inventory, &triage, &plan, "学会图遍历补充", Some(&completer), None)
        .await
        .unwrap()
        .expect("另一文件的课时让课程非空");
    let topic = |label: &str| {
        outcome
            .coverage
            .topics
            .iter()
            .find(|t| t.label == label)
            .unwrap_or_else(|| panic!("topic「{label}」应在台账里"))
    };
    let chapter_topic = topic("第三章 ArrayList");
    assert_eq!(
        chapter_topic.status,
        TopicStatus::Uncertain,
        "零 covered 后代 → 维持现状三态,不滚入: {:?}",
        chapter_topic
    );
    assert!(!chapter_topic.rolled_up);
    assert!(
        outcome
            .coverage
            .warnings
            .iter()
            .any(|w| w.contains("第三章 ArrayList") && w.contains("未被任何课时覆盖")),
        "真缺口的章级警告保留(滚入不产生新静默): {:?}",
        outcome.coverage.warnings
    );
    for label in ["3.1 划分方法", "3.2 复杂度"] {
        assert_eq!(topic(label).status, TopicStatus::Uncertain, "子缺口保持自身状态");
        assert!(
            outcome
                .coverage
                .warnings
                .iter()
                .any(|w| w.contains(label) && w.contains("未被任何课时覆盖")),
            "子缺口警告逐条可见"
        );
    }
    let supplement = topic("图的补充");
    assert_eq!(supplement.status, TopicStatus::Covered, "另一文件的课时正常覆盖");
    assert!(!supplement.rolled_up);
}
