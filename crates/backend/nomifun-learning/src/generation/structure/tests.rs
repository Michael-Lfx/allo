use super::*;

fn node(id: &str, label: &str) -> ConceptGraphNode {
    ConceptGraphNode {
        id: id.to_owned(),
        label: label.to_owned(),
        description: String::new(),
        weight: 0.5,
    }
}

fn edge(src: &str, dst: &str, rationale: &str) -> ConceptGraphEdge {
    ConceptGraphEdge {
        src: src.to_owned(),
        dst: dst.to_owned(),
        relation: ConceptEdgeRelation::DependsOn,
        rationale: rationale.to_owned(),
    }
}

fn chapter(title: &str, covers: &[&str]) -> PlannedChapter {
    PlannedChapter {
        title: title.to_owned(),
        summary: String::new(),
        learning_objectives: Vec::new(),
        covers: covers.iter().map(|c| c.to_string()).collect(),
        prerequisites: Vec::new(),
        source_anchors: Vec::new(),
        content_type: ChapterContentType::Concept,
    }
}

// ── DFS 去环 ───────────────────────────────────────────────────────────────

#[test]
fn break_cycles_drops_the_shortest_rationale_edge_and_warns() {
    let mut graph = ConceptGraph {
        nodes: vec![node("c1", "一"), node("c2", "二"), node("c3", "三")],
        edges: vec![
            edge("c1", "c2", "比较长的理由甲甲甲甲"),
            edge("c2", "c3", "短理由"),
            edge("c3", "c1", "比较长的理由乙乙乙乙"),
        ],
    };
    let mut warnings = Vec::new();
    let residual = postprocess::break_cycles(&mut graph, &mut warnings);

    assert!(!residual, "单环一次去边后无残留");
    assert_eq!(graph.edges.len(), 2, "恰丢一条边");
    assert!(
        !graph.edges.iter().any(|e| e.rationale == "短理由"),
        "丢 rationale 最短的边(置信度代理)"
    );
    assert_eq!(warnings.len(), 1, "每次丢边记一条 warning");
    assert!(warnings[0].contains("短理由"), "{}", warnings[0]);
}

#[test]
fn break_cycles_ignores_non_ordering_relations() {
    let mut graph = ConceptGraph {
        nodes: vec![node("c1", "一"), node("c2", "二")],
        edges: vec![ConceptGraphEdge {
            src: "c1".into(),
            dst: "c2".into(),
            relation: ConceptEdgeRelation::Related,
            rationale: "相关不构成顺序约束".into(),
        }],
    };
    let mut warnings = Vec::new();
    assert!(!postprocess::break_cycles(&mut graph, &mut warnings));
    assert_eq!(graph.edges.len(), 1, "related/extends 边不去");
    assert!(warnings.is_empty());
}

#[test]
fn break_cycles_caps_removals_at_twenty_and_reports_residuals() {
    // 21 个互不共享节点的两节点环:21 次去边才能清空,上限 20 → 有残留。
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for index in 0..21 {
        let (a, b) = (format!("a{index}"), format!("b{index}"));
        nodes.push(node(&a, &a));
        nodes.push(node(&b, &b));
        edges.push(edge(&a, &b, "甲"));
        edges.push(edge(&b, &a, "乙"));
    }
    let mut graph = ConceptGraph { nodes, edges };
    let mut warnings = Vec::new();
    let residual = postprocess::break_cycles(&mut graph, &mut warnings);

    assert!(residual, "超出去边上限后仍有环");
    assert_eq!(warnings.len(), STRUCTURE_CYCLE_BREAK_MAX_EDGES + 1, "20 条丢边 + 1 条残留");
    assert!(warnings.last().unwrap().contains("残留"), "{:?}", warnings.last());
}

// ── Kahn 拓扑排序 ──────────────────────────────────────────────────────────

#[test]
fn topo_sort_reorders_inverted_prerequisites_and_breaks_ties_by_original_order() {
    // 章序 [B(依赖 A), A, C(独立)] → A 先行;A 与 C 谁先?平局按原序,C 在
    // 索引 2、A 在索引 1 —— A、C 相互独立,排序结果保原相对序。
    let chapters = vec![chapter("B 章", &["cb"]), chapter("A 章", &["ca"]), chapter("C 章", &["cc"])];
    let graph = ConceptGraph {
        nodes: vec![node("ca", "a"), node("cb", "b"), node("cc", "c")],
        edges: vec![edge("cb", "ca", "B 依赖 A")],
    };
    let mut warnings = Vec::new();
    let (sorted, unresolved) = postprocess::topo_sort_chapters(&chapters, &graph, &mut warnings);

    let titles: Vec<&str> = sorted.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["A 章", "B 章", "C 章"], "前置倒挂被修复");
    assert!(unresolved.is_empty());
    assert!(warnings.is_empty(), "正常拓扑不产生 warning");
}

#[test]
fn topo_sort_honors_declared_prerequisites_by_title() {
    let mut declared = chapter("B 章", &[]);
    declared.prerequisites = vec!["A 章".to_owned()];
    let chapters = vec![declared, chapter("A 章", &[])];
    let mut warnings = Vec::new();
    let (sorted, unresolved) =
        postprocess::topo_sort_chapters(&chapters, &ConceptGraph::default(), &mut warnings);

    let titles: Vec<&str> = sorted.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["A 章", "B 章"], "章节声明的 prerequisites 也进拓扑");
    assert!(unresolved.is_empty());
}

#[test]
fn topo_sort_reports_unknown_prerequisite_titles_as_unresolved() {
    let mut declared = chapter("B 章", &[]);
    declared.prerequisites = vec!["不存在的章".to_owned()];
    let chapters = vec![chapter("A 章", &[]), declared];
    let mut warnings = Vec::new();
    let (sorted, unresolved) =
        postprocess::topo_sort_chapters(&chapters, &ConceptGraph::default(), &mut warnings);

    assert_eq!(sorted.len(), 2, "未知前置不改变排序,只披露");
    assert_eq!(unresolved, vec!["B 章 → 不存在的章".to_owned()]);
    assert!(warnings.len() == 1 && warnings[0].contains("不存在的章"), "{warnings:?}");
}

#[test]
fn topo_sort_falls_back_to_original_order_when_a_cycle_survives() {
    // 绕过 break_cycles 直接喂含环的图:章际互为前置 → 回退原序 + warning。
    let chapters = vec![chapter("A 章", &["c1"]), chapter("B 章", &["c2"])];
    let graph = ConceptGraph {
        nodes: vec![node("c1", "一"), node("c2", "二")],
        edges: vec![edge("c1", "c2", "甲"), edge("c2", "c1", "乙")],
    };
    let mut warnings = Vec::new();
    let (sorted, _) = postprocess::topo_sort_chapters(&chapters, &graph, &mut warnings);

    let titles: Vec<&str> = sorted.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["A 章", "B 章"], "有环回退原序");
    assert!(warnings.iter().any(|w| w.contains("回退原序")), "{warnings:?}");
}

// ── Jaccard 覆盖补齐 ───────────────────────────────────────────────────────

#[test]
fn jaccard_completion_attaches_uncovered_concepts_to_the_closest_chapter() {
    let mut chapters = vec![
        chapter("数组与链表的基本操作", &["c_array"]),
        chapter("图的深度优先遍历", &["c_dfs"]),
    ];
    let graph = ConceptGraph {
        nodes: vec![
            node("c_array", "数组"),
            node("c_dfs", "深度优先遍历"),
            node("c_free", "链表"),
        ],
        edges: vec![],
    };
    postprocess::complete_uncovered_concepts(&mut chapters, &graph);

    assert!(chapters[0].covers.contains(&"c_free".to_owned()), "未覆盖概念挂到最相关章");
    assert!(!chapters[1].covers.contains(&"c_free".to_owned()));
    assert_eq!(chapters[0].covers.len(), 2);
}

#[test]
fn jaccard_completion_keeps_already_covered_concepts_untouched() {
    let mut chapters = vec![chapter("数组", &["c_array"]), chapter("遍历", &["c_bfs"])];
    let graph = ConceptGraph {
        nodes: vec![node("c_array", "数组"), node("c_bfs", "广度优先")],
        edges: vec![],
    };
    postprocess::complete_uncovered_concepts(&mut chapters, &graph);
    assert_eq!(chapters[0].covers, vec!["c_array".to_owned()]);
    assert_eq!(chapters[1].covers, vec!["c_bfs".to_owned()]);
}

// ── Overview 兜底 ──────────────────────────────────────────────────────────

#[test]
fn overview_fallback_synthesizes_one_evidence_backed_chapter() {
    let graph = ConceptGraph {
        nodes: vec![node("c1", "一"), node("c2", "二")],
        edges: vec![],
    };
    let anchors = vec![SourceAnchor {
        kb_id: "kb".into(),
        rel_path: "notes/a.md".into(),
        heading_or_anchor: None,
        paragraph: None,
        snippet: None,
        anchor_uncertain: true,
    }];
    let fallback = postprocess::overview_fallback("这些证据支撑入门课程。", &graph, anchors);

    assert_eq!(fallback.title, "Overview");
    assert_eq!(fallback.summary, "这些证据支撑入门课程。");
    assert_eq!(fallback.covers, vec!["c1".to_owned(), "c2".to_owned()], "兜底章覆盖全部概念");
    assert_eq!(fallback.source_anchors.len(), 1);
    assert!(fallback.source_anchors[0].anchor_uncertain, "path 级锚点标 uncertain");
}

// ── topic 拆分(topics.rs)─────────────────────────────────────────────────

#[test]
fn heading_files_split_one_topic_per_inventory_heading() {
    let mut warnings = Vec::new();
    let seeds = topics::split_file_topics(
        "notes/a.md",
        &["一元二次方程".to_owned(), "配方法".to_owned(), "判别式".to_owned()],
        Some("正文"),
        &mut warnings,
    );
    assert_eq!(seeds.len(), 3);
    assert_eq!(seeds[0].label, "一元二次方程");
    assert!(seeds[0].heading);
    assert_eq!(seeds[0].paragraph, None);
    assert!(!seeds[0].anchor_uncertain);
    assert!(warnings.is_empty());
}

#[test]
fn headingless_files_split_into_paragraph_topics() {
    let mut warnings = Vec::new();
    let seeds = topics::split_file_topics(
        "plain.md",
        &[],
        Some("第一段讲动态规划。\n\n第二段讲状态转移方程。\n\n第三段讲滚动数组优化。"),
        &mut warnings,
    );
    assert_eq!(seeds.len(), 3, "段落序号定位,不编造标题");
    assert_eq!(seeds[0].label, "第 1 段");
    assert_eq!(seeds[0].paragraph, Some(1));
    assert!(!seeds[0].heading);
    assert!(!seeds[0].anchor_uncertain, "段落锚点是精确锚,不标 uncertain");
    assert!(warnings.is_empty());
}

#[test]
fn unlocatable_content_falls_back_to_a_path_level_uncertain_topic() {
    let mut warnings = Vec::new();
    let seeds = topics::split_file_topics("weird.md", &[], None, &mut warnings);
    assert_eq!(seeds.len(), 1);
    assert!(seeds[0].anchor_uncertain, "path 级兜底标 anchor_uncertain");
    assert!(warnings.len() == 1 && warnings[0].contains("weird.md"), "{warnings:?}");
}

// ── 覆盖对账 ───────────────────────────────────────────────────────────────

fn anchor(kb: &str, path: &str, heading: Option<&str>) -> SourceAnchor {
    SourceAnchor {
        kb_id: kb.to_owned(),
        rel_path: path.to_owned(),
        heading_or_anchor: heading.map(str::to_owned),
        paragraph: None,
        snippet: None,
        anchor_uncertain: false,
    }
}

fn topic_file(path: &str, role: KbTriageRole, seeds: Vec<topics::TopicSeed>) -> topics::TopicFile {
    topics::TopicFile {
        rel_path: path.to_owned(),
        kb_id: "kb".to_owned(),
        role,
        seeds,
    }
}

fn seed(label: &str, heading: bool) -> topics::TopicSeed {
    topics::TopicSeed {
        topic_id: format!("x/{label}"),
        label: label.to_owned(),
        paragraph: if heading { None } else { Some(1) },
        heading,
        anchor_uncertain: false,
    }
}

#[test]
fn reconcile_marks_covered_skipped_and_uncertain_topics() {
    let files = vec![topic_file(
        "mixed.md",
        KbTriageRole::Mixed,
        vec![seed("排序算法", true), seed("会议纪要", true), seed("推广段落", true)],
    )];
    let mut ch = chapter("排序入门", &[]);
    ch.source_anchors = vec![anchor("kb", "mixed.md", Some("排序算法"))];
    let skipped = vec![topics::SkippedTopic {
        rel_path: "mixed.md".to_owned(),
        heading: "会议纪要".to_owned(),
        reason: "与学习目标无关的会议记录".to_owned(),
    }];

    let ledger = topics::reconcile_coverage(&files, &[ch], &skipped);

    assert_eq!(ledger.topics.len(), 3);
    let by_label: HashMap<&str, &CoverageTopic> = ledger
        .topics
        .iter()
        .map(|t| (t.label.as_str(), t))
        .collect();
    let sorting = by_label["排序算法"];
    assert_eq!(sorting.status, TopicStatus::Covered);
    assert_eq!(sorting.assigned_chapters, vec!["排序入门".to_owned()]);
    let meeting = by_label["会议纪要"];
    assert_eq!(meeting.status, TopicStatus::Skipped);
    assert!(meeting.reason.as_deref().unwrap().contains("会议"));
    assert_eq!(by_label["推广段落"].status, TopicStatus::Uncertain, "未覆盖也未显式跳过 → uncertain 可见");

    // 文件聚合:mixed 文件部分覆盖,不整文件排除。
    assert_eq!(ledger.files.len(), 1);
    assert_eq!(ledger.files[0].status, FileCoverageStatus::Partial);
    assert_eq!(ledger.files[0].assigned_chapters, vec!["排序入门".to_owned()]);
}

#[test]
fn path_level_anchors_cover_every_topic_of_the_file() {
    let files = vec![topic_file(
        "noheading.md",
        KbTriageRole::Core,
        vec![seed("第 1 段", false), seed("第 2 段", false)],
    )];
    let mut ch = chapter("兜底章", &[]);
    ch.source_anchors = vec![SourceAnchor {
        kb_id: "kb".into(),
        rel_path: "noheading.md".into(),
        heading_or_anchor: None,
        paragraph: None,
        snippet: None,
        anchor_uncertain: true,
    }];
    let ledger = topics::reconcile_coverage(&files, &[ch], &[]);
    assert!(
        ledger.topics.iter().all(|t| t.status == TopicStatus::Covered),
        "path 级锚点覆盖全文件 topic:{:?}",
        ledger.topics
    );
    assert_eq!(ledger.files[0].status, FileCoverageStatus::Covered);
}

#[test]
fn paragraph_anchors_cover_only_their_own_paragraph_topic() {
    let files = vec![topic_file(
        "plain.md",
        KbTriageRole::Core,
        vec![
            topics::TopicSeed {
                topic_id: "p1".into(),
                label: "第 1 段".into(),
                paragraph: Some(1),
                heading: false,
                anchor_uncertain: false,
            },
            topics::TopicSeed {
                topic_id: "p2".into(),
                label: "第 2 段".into(),
                paragraph: Some(2),
                heading: false,
                anchor_uncertain: false,
            },
        ],
    )];
    let mut ch = chapter("第一章", &[]);
    ch.source_anchors = vec![SourceAnchor {
        kb_id: "kb".into(),
        rel_path: "plain.md".into(),
        heading_or_anchor: None,
        paragraph: Some(1),
        snippet: None,
        anchor_uncertain: false,
    }];
    let ledger = topics::reconcile_coverage(&files, &[ch], &[]);
    let by_label: HashMap<&str, &CoverageTopic> =
        ledger.topics.iter().map(|t| (t.label.as_str(), t)).collect();
    assert_eq!(by_label["第 1 段"].status, TopicStatus::Covered);
    assert_eq!(by_label["第 2 段"].status, TopicStatus::Uncertain);
    assert_eq!(ledger.files[0].status, FileCoverageStatus::Partial);
}

#[test]
fn all_skipped_files_report_skipped_status() {
    let files = vec![topic_file(
        "meeting.md",
        KbTriageRole::Mixed,
        vec![seed("周会纪要", true)],
    )];
    let skipped = vec![topics::SkippedTopic {
        rel_path: "meeting.md".to_owned(),
        heading: "周会纪要".to_owned(),
        reason: "与目标无关".to_owned(),
    }];
    let ledger = topics::reconcile_coverage(&files, &[], &skipped);
    assert_eq!(ledger.topics[0].status, TopicStatus::Skipped);
    assert_eq!(ledger.files[0].status, FileCoverageStatus::Skipped);
}

// ── slice 2:查询生成 / 检索均衡 / 证据摘要 ───────────────────────────────

fn hit(kb: &str, path: &str, heading: &str, snippet: &str) -> nomifun_knowledge::service::KnowledgeSearchHit {
    nomifun_knowledge::service::KnowledgeSearchHit {
        kb_id: KnowledgeBaseId::new(),
        kb_name: format!("库{kb}"),
        rel_path: path.to_owned(),
        heading: heading.to_owned(),
        snippet: snippet.to_owned(),
        score: 10,
    }
}

#[test]
fn plan_search_pairs_are_query_major_and_cap_total_calls_without_starving_bases() {
    let queries: Vec<String> = (0..10).map(|i| format!("查询{i}")).collect();
    let bases: Vec<(String, String)> = (0..6)
        .map(|i| (format!("kb{i}"), format!("库{i}")))
        .collect();
    let (pairs, truncated) = plan_search_pairs(&queries, &bases);

    assert!(truncated, "10×6=60 对 > 48 上限,需要截断");
    assert_eq!(pairs.len(), STRUCTURE_SEARCH_MAX_CALLS);
    // query-major:第 0 个查询的 6 个库排在最前。
    assert_eq!(pairs[0], (0, 0));
    assert_eq!(pairs[5], (0, 5));
    assert_eq!(pairs[6], (1, 0));
    // 截断后每个库都还有调用(预算被砍不饿死后面的库:保留的 8 个查询 × 全部 6 库)。
    for base_index in 0..6 {
        assert!(
            pairs.iter().any(|&(_, b)| b == base_index),
            "库 {base_index} 被饿死"
        );
    }
}

#[test]
fn balanced_merge_round_robins_sources_instead_of_global_scores() {
    // 库 A 高分命中多,库 B 只有一条:轮询保证 B 的证据不被全局分数淹没。
    let per_base = vec![
        (
            "kb-a".to_owned(),
            vec![
                hit("a", "a1.md", "H1", "片段一"),
                hit("a", "a2.md", "H2", "片段二"),
                hit("a", "a3.md", "H3", "片段三"),
            ],
        ),
        ("kb-b".to_owned(), vec![hit("b", "b1.md", "HB", "片段乙")]),
    ];
    let merged = balanced_merge(&per_base, STRUCTURE_EVIDENCE_MAX_CHUNKS);

    assert_eq!(merged.len(), 4);
    assert_eq!(merged[0].rel_path, "a1.md", "每来源内部排序轮询取样");
    assert_eq!(merged[1].rel_path, "b1.md", "第二来源第二位就出现");
    assert_eq!(merged[2].rel_path, "a2.md");
}

#[test]
fn balanced_merge_dedupes_repeated_hits_and_caps_the_budget() {
    let repeated = hit("a", "a1.md", "H1", "同一命中");
    let per_base = vec![(
        "kb-a".to_owned(),
        vec![repeated.clone(), repeated.clone(), hit("a", "a2.md", "H2", "片段二")],
    )];
    let merged = balanced_merge(&per_base, STRUCTURE_EVIDENCE_MAX_CHUNKS);
    assert_eq!(merged.len(), 2, "同 (path, heading) 只留一次");

    let many: Vec<_> = (0..40)
        .map(|i| hit("a", &format!("f{i:02}.md"), "H", "片段"))
        .collect();
    let merged = balanced_merge(&[("kb-a".to_owned(), many)], 5);
    assert_eq!(merged.len(), 5, "总量预算生效");
}

#[test]
fn deterministic_queries_partition_goal_terms_with_a_fallback_topic() {
    let queries = deterministic_queries("学会排序算法与复杂度分析", &[]);
    assert!(!queries.is_empty());
    assert!(queries.len() <= 4);
    assert!(
        queries.iter().all(|q| !q.trim().is_empty()),
        "查询不为空:{queries:?}"
    );
    // 词条切自原文:至少一条查询包含「排序」这个二元片段。
    assert!(queries.iter().any(|q| q.contains("排序")), "{queries:?}");

    let fallback = deterministic_queries("", &["金融笔记".to_owned()]);
    assert!(
        fallback.iter().any(|q| q.contains("金融笔记")),
        "空目标回退库名:{fallback:?}"
    );
}

/// 按系统提示词路由的脚本 completer(结构发现的四个 LLM 阶段共用)。
struct StageCompleter<F>
where
    F: Fn(&str, &str) -> Result<String, String> + Send + Sync,
{
    route: F,
    calls: std::sync::atomic::AtomicUsize,
    prompts: std::sync::Mutex<Vec<(String, String)>>,
}

impl<F> StageCompleter<F>
where
    F: Fn(&str, &str) -> Result<String, String> + Send + Sync,
{
    fn new(route: F) -> Self {
        Self {
            route,
            calls: std::sync::atomic::AtomicUsize::new(0),
            prompts: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl<F> LearningCompleter for StageCompleter<F>
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
        Err(AppError::Internal("结构发现测试只走 json 接缝".into()))
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
        self.prompts.lock().unwrap().push((system.to_owned(), user.to_owned()));
        (self.route)(system, user).map_err(|message| AppError::Internal(message))
    }
}

fn empty_inventory() -> KbInventory {
    KbInventory {
        bases: vec![KbInventoryBase {
            kb_id: "kb".into(),
            name: "测试库".into(),
            namespace: String::new(),
            total_files: 1,
        }],
        files: vec![],
        total_files: 0,
        total_bytes: 0,
        skipped_hidden: 0,
        scan_scope: KbScanScope::Markdown,
    }
}

#[tokio::test]
async fn generate_queries_normalizes_llm_output_into_four_to_eight_queries() {
    // 模型给了 10 条(含 1 条重复、1 条空白)→ 去重截断到 8。
    let completer = StageCompleter::new(|_system, _user| {
        Ok(r#"{"queries": ["排序概述", "排序机制", "排序案例", "排序易错点", "排序应用", "排序对比", "排序历史", "排序进阶", "排序概述", "  "]}"#.into())
    });
    let queries = generate_queries(&completer, "学会排序", &empty_inventory()).await.unwrap();
    assert_eq!(queries.len(), STRUCTURE_QUERY_MAX, "超过 8 条截到 8");
    assert!(queries.iter().all(|q| !q.trim().is_empty()));
    assert_eq!(queries.iter().collect::<HashSet<_>>().len(), queries.len(), "去重");

    // 模型只给 2 条 → 用确定性查询补齐到下限 4。
    let completer = StageCompleter::new(|_system, _user| {
        Ok(r#"{"queries": ["排序概念", "排序实现"]}"#.into())
    });
    let queries = generate_queries(&completer, "学会排序算法", &empty_inventory()).await.unwrap();
    assert!(queries.len() >= STRUCTURE_QUERY_MIN, "补齐到下限:{queries:?}");
    assert_eq!(queries[0], "排序概念");
}

#[tokio::test]
async fn generate_queries_surfaces_call_failures_for_the_caller_to_fall_back() {
    let completer = StageCompleter::new(|_system, _user| Err("provider down".into()));
    let outcome = generate_queries(&completer, "学会排序", &empty_inventory()).await;
    assert!(outcome.is_err(), "调用失败上抛,由编排层做确定性降级");
}

#[tokio::test]
async fn summarize_evidence_digests_balanced_chunks_with_clamps() {
    let completer = StageCompleter::new(|system, user| {
        assert!(system.contains("证据摘要"), "路由标记:{system}");
        assert!(user.contains("排序复杂度"), "证据片段进提示词:{user}");
        let concepts: Vec<String> = (0..25).map(|i| format!("概念{i}")).collect();
        let notes: Vec<String> = (0..7).map(|i| format!("注{i}")).collect();
        Ok(serde_json::json!({
            "summary": "这些证据足以支撑一门排序入门课。",
            "gaps": ["外部排序的磁盘模型"],
            "concepts": concepts,
            "notes": notes,
        })
        .to_string())
    });
    let chunks = vec![EvidenceChunk {
        kb_id: "kb".into(),
        kb_name: "测试库".into(),
        rel_path: "notes/sorting.md".into(),
        heading: "排序复杂度".into(),
        snippet: "比较排序的下界是 n log n。".into(),
    }];
    let digest = summarize_evidence(&completer, "学会排序", &chunks).await.unwrap();

    assert_eq!(digest.summary, "这些证据足以支撑一门排序入门课。");
    assert_eq!(digest.gaps, vec!["外部排序的磁盘模型".to_owned()]);
    assert_eq!(digest.concepts.len(), 20, "候选概念上限 20");
    assert_eq!(digest.notes.len(), 5, "notes ≤5");
}

#[tokio::test]
async fn summarize_evidence_fails_cleanly_for_the_caller_to_fall_back() {
    let completer = StageCompleter::new(|_system, _user| Err("provider down".into()));
    let outcome = summarize_evidence(&completer, "学会排序", &[]).await;
    assert!(outcome.is_err());
}

// ── slice 3:草稿 / 评审 / 修订 ───────────────────────────────────────────

/// 双文件 inventory:一个带标题、一个无标题(段落锚)。
fn plan_inventory() -> KbInventory {
    KbInventory {
        bases: vec![KbInventoryBase {
            kb_id: "kb-1".into(),
            name: "算法库".into(),
            namespace: String::new(),
            total_files: 2,
        }],
        files: vec![
            KbInventoryFile {
                rel_path: "ch1.md".into(),
                kb_id: "kb-1".into(),
                size: 100,
                modified_at: None,
                content_hash: "a".repeat(64),
                headings: vec!["排序算法".into(), "快速排序".into()],
                preview: "比较排序。".into(),
                kind: KbFileKind::Markdown,
            },
            KbInventoryFile {
                rel_path: "plain.md".into(),
                kb_id: "kb-1".into(),
                size: 80,
                modified_at: None,
                content_hash: "b".repeat(64),
                headings: vec![],
                preview: "无标题正文。".into(),
                kind: KbFileKind::Markdown,
            },
        ],
        total_files: 2,
        total_bytes: 180,
        skipped_hidden: 0,
        scan_scope: KbScanScope::Markdown,
    }
}

fn plan_digest() -> EvidenceDigest {
    EvidenceDigest {
        summary: "证据支撑排序入门。".into(),
        gaps: vec![],
        concepts: vec!["排序".into(), "快速排序".into()],
        notes: vec![],
    }
}

fn plan_chunks() -> Vec<EvidenceChunk> {
    vec![EvidenceChunk {
        kb_id: "kb-1".into(),
        kb_name: "算法库".into(),
        rel_path: "ch1.md".into(),
        heading: "排序算法".into(),
        snippet: "比较排序的下界。".into(),
    }]
}

const DRAFT_REPLY: &str = r#"{
  "concepts": [
    {"id": "c1", "label": "排序", "description": "把序列变有序", "weight": 0.9},
    {"id": "c2", "label": "快速排序", "description": "分治排序", "weight": 0.8},
    {"id": "c1", "label": "重复概念", "description": "同 id 保留首个", "weight": 0.5}
  ],
  "edges": [
    {"src": "c2", "dst": "c1", "relation": "depends_on", "rationale": "先懂排序"},
    {"src": "c1", "dst": "cX", "relation": "depends_on", "rationale": "指向未知概念"},
    {"src": "c1", "dst": "c1", "relation": "depends_on", "rationale": "自环"},
    {"src": "c1", "dst": "c2", "relation": "歪关系", "rationale": "未知关系"}
  ],
  "chapters": [
    {
      "title": "排序基础",
      "summary": "从比较排序开始",
      "learning_objectives": ["理解下界"],
      "covers": ["c1", "cX"],
      "prerequisites": [],
      "source_anchors": [
        {"path": "ch1.md", "heading": "排序算法", "snippet": "比较排序的下界。"},
        {"path": "ch1.md", "heading": "不存在的标题", "snippet": "幻觉标题"},
        {"path": "幽灵.md", "heading": "不存在"},
        {"path": "plain.md", "paragraph": 2, "snippet": "第二段"}
      ],
      "content_type": "concept"
    },
    {
      "title": "排序基础",
      "summary": "重名章节",
      "covers": ["c2"],
      "source_anchors": [],
      "content_type": "practice"
    }
  ],
  "skipped_topics": [
    {"path": "ch1.md", "heading": "会议纪要", "reason": "与目标无关"},
    {"path": "幽灵.md", "heading": "x", "reason": "路径不存在"}
  ]
}"#;

#[tokio::test]
async fn draft_plan_resolves_anchors_and_normalizes_the_reply() {
    let completer = StageCompleter::new(|_system, _user| Ok(DRAFT_REPLY.to_owned()));
    let mut draft = draft_plan(&completer, "学会排序", &plan_digest(), &plan_chunks(), &plan_inventory(), None)
        .await
        .unwrap();

    // 概念去重(id 重复保留首个)。
    assert_eq!(draft.graph.nodes.len(), 2);
    assert_eq!(draft.graph.nodes[0].label, "排序");
    // 边清洗:未知端点丢、自环丢、未知关系降级 related。
    assert_eq!(draft.graph.edges.len(), 2);
    assert!(draft.graph.edges.iter().any(|e| e.relation == ConceptEdgeRelation::Related), "未知关系降级 related");
    // 章节重名去重(前置/拓扑按标题引用)。
    assert_eq!(draft.chapters.len(), 2);
    assert_ne!(draft.chapters[0].title, draft.chapters[1].title);
    // covers 只留已知概念。
    assert_eq!(draft.chapters[0].covers, vec!["c1".to_owned()]);

    // 锚点对账:正常 heading 锚、幻觉标题(保留但 uncertain)、幽灵路径(丢弃)。
    let anchors = &draft.chapters[0].source_anchors;
    assert_eq!(anchors.len(), 3, "幽灵路径被丢弃:{anchors:?}");
    assert_eq!(anchors[0].kb_id, "kb-1");
    assert!(!anchors[0].anchor_uncertain, "真实标题是精确锚");
    assert!(anchors[1].anchor_uncertain, "标题对不上原文 → uncertain");
    // 段落锚。
    assert_eq!(anchors[2].paragraph, Some(2));
    assert!(anchors[2].heading_or_anchor.is_none());

    // skipped 只留可回源路径。
    assert_eq!(draft.skipped.len(), 1);
    assert_eq!(draft.skipped[0].heading, "会议纪要");

    assert!(
        draft.warnings.iter().any(|w| w.contains("幽灵.md")),
        "幻觉路径进 warning:{:?}",
        draft.warnings
    );
    assert!(
        draft.unresolved.iter().any(|u| u.contains("幽灵.md")),
        "不可回源锚点进 unresolved:{:?}",
        draft.unresolved
    );

    // 提示词面:证据 + 文件标题清单都进了。
    let prompts = completer.prompts.lock().unwrap();
    let user = &prompts[0].1;
    assert!(user.contains("排序算法"), "文件标题清单:{user}");
    assert!(user.contains("ch1.md"));
    assert!(user.contains("证据支撑排序入门"), "证据摘要:{user}");
}

#[tokio::test]
async fn draft_plan_carries_revision_feedback_when_given() {
    let completer = StageCompleter::new(|_system, _user| Ok(DRAFT_REPLY.to_owned()));
    let _ = draft_plan(
        &completer,
        "学会排序",
        &plan_digest(),
        &plan_chunks(),
        &plan_inventory(),
        Some("- [coverage] 第一章漏锚点,补上"),
    )
    .await
    .unwrap();
    let prompts = completer.prompts.lock().unwrap();
    assert!(prompts[0].1.contains("修订指令"), "{}", prompts[0].1);
    assert!(prompts[0].1.contains("漏锚点"), "{}", prompts[0].1);
}

#[tokio::test]
async fn critique_plan_normalizes_categories_and_reports_findings() {
    let completer = StageCompleter::new(|system, _user| {
        assert!(system.contains("结构评审"), "路由标记:{system}");
        Ok(r#"{"findings": [
            {"category": "coverage", "chapter": "排序基础", "issue": "漏了外排序", "fix_hint": "补一章"},
            {"category": "乱编类别", "chapter": "", "issue": "其他问题", "fix_hint": "看着办"}
        ]}"#
            .to_owned())
    });
    let draft = draft_plan_from_reply(DRAFT_REPLY, &plan_inventory()).unwrap();
    let findings = critique_plan(&completer, "学会排序", &draft).await.unwrap();

    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].category, "coverage");
    assert_eq!(findings[1].category, "other", "未知类别归 other");
    assert_eq!(findings[0].fix_hint, "补一章");
}

#[tokio::test]
async fn critique_plan_returns_no_findings_when_clean() {
    let completer = StageCompleter::new(|_system, _user| {
        Ok(r#"{"findings": []}"#.to_owned())
    });
    let draft = draft_plan_from_reply(DRAFT_REPLY, &plan_inventory()).unwrap();
    let findings = critique_plan(&completer, "学会排序", &draft).await.unwrap();
    assert!(findings.is_empty());
}


