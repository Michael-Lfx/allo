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
