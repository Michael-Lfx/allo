use super::*;
use crate::completer::LearningCompleter;
use nomifun_common::AppError;
use nomifun_realtime::UserEventSink;
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
