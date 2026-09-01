use super::*;

/// Drafts older than this are evicted: an agent session never outlives the
/// generation timeout by much, so anything older is an abandoned draft
/// (crashed or timed-out session, client gave up) leaking memory.
const LEARNING_GRAPH_DRAFT_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

/// The single implicit module every learning-graph course hangs its node
/// lessons under (`learning_lessons.module_id` is NOT NULL). The graph UI
/// never renders modules; this row only satisfies the relational shape.
pub(crate) const GRAPH_MODULE_TITLE: &str = "学习图";

/// 「下一步推荐学习的节点」一次最多同时展示的数量。
pub(crate) const GRAPH_RECOMMEND_LIMIT: usize = 10;

impl LearningService {

    // ── Learning-graph course (beta, database-backed) ────────────────────
    // A published graph becomes a `course_kind = 'learning_graph'` course:
    // one implicit module, one `learning_lessons` row per node (Kahn
    // topological order as `position`), and one `learning_graph_prerequisites`
    // row per prerequisite edge. Progress, activities and reviews all key on
    // the lesson rows, so the graph course rides the whole lesson pipeline.

    /// Decompose a learning goal into a learning-unit network and publish it
    /// as a graph course. The agent engine is the SINGLE generation path: it
    /// owns the whole lifecycle (draft tools + audit-gated publish). A
    /// per-user slot guard serializes generations — the agent loop runs for
    /// minutes, and parallel runs would race the shared draft store, so a
    /// duplicate submit fails fast with a conflict instead.
    pub async fn generate_learning_graph(
        &self,
        user_id: &UserId,
        request: crate::learning_graph::GenerateLearningGraphRequest,
    ) -> Result<crate::learning_graph::LearningGraphRecord, AppError> {
        let topic = request.topic.trim();
        if topic.is_empty() {
            return Err(AppError::BadRequest(
                "learning graph topic must not be empty".into(),
            ));
        }
        if topic.chars().count() > 200 {
            return Err(AppError::BadRequest("learning graph topic is too long".into()));
        }
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let _slot = self.acquire_generation_slot(user_id, "learning-graph".to_owned())?;
        let engine = self
            .learning_graph_engine
            .read()
            .map_err(|_| AppError::Internal("learning graph engine lock poisoned".into()))?
            .clone();
        let engine = engine.ok_or_else(|| {
            AppError::Conflict("learning graph generation is not configured".into())
        })?;
        engine
            .generate(
                user_id,
                topic,
                model_override.map(|(provider, model)| (provider.as_str(), model)),
            )
            .await
    }

    /// Persist the generation provenance next to the audit snapshot
    /// (best-effort: a failed write only loses diagnostics, never the graph).
    pub async fn record_learning_graph_generation(
        &self,
        course_id: &LearningCourseId,
        provider: &str,
        model: &str,
    ) -> Result<(), AppError> {
        let generation = serde_json::json!({ "provider": provider, "model": model });
        sqlx::query(
            "UPDATE learning_courses \
             SET graph_meta_json = json_set(COALESCE(graph_meta_json, '{}'), '$.generation', json(?)), \
                 updated_at = ? \
             WHERE course_id = ?",
        )
        .bind(generation.to_string())
        .bind(now_ms())
        .bind(course_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }

    /// Every stored graph course, newest first. Courses are installation
    /// global (no owner column), like the rest of the catalog.
    pub async fn list_learning_graphs(
        &self,
    ) -> Result<Vec<crate::learning_graph::LearningGraphSummary>, AppError> {
        let rows = sqlx::query(
            "SELECT c.course_id, c.learning_goal, c.title, c.created_at, \
                    (SELECT COUNT(*) FROM learning_lessons l \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE m.course_id = c.course_id) AS node_count, \
                    (SELECT COUNT(*) FROM learning_graph_prerequisites p \
                     WHERE p.course_id = c.course_id) AS edge_count \
             FROM learning_courses c \
             WHERE c.course_kind = 'learning_graph' \
             ORDER BY c.created_at DESC, c.course_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut summaries = Vec::with_capacity(rows.len());
        for row in rows {
            summaries.push(crate::learning_graph::LearningGraphSummary {
                id: row.try_get("course_id").map_err(internal)?,
                topic: {
                    let goal: String = row.try_get("learning_goal").map_err(internal)?;
                    let title: String = row.try_get("title").map_err(internal)?;
                    if goal.trim().is_empty() { title } else { goal }
                },
                node_count: row.try_get::<i64, _>("node_count").map_err(internal)? as usize,
                edge_count: row.try_get::<i64, _>("edge_count").map_err(internal)? as usize,
                created_at: row.try_get("created_at").map_err(internal)?,
            });
        }
        Ok(summaries)
    }

    /// One stored graph course assembled from the relational shape: nodes are
    /// the course's lessons (topological `position` order), edges the
    /// prerequisite rows, and the audit snapshot rides in `graph_meta_json`.
    pub async fn get_learning_graph(
        &self,
        id: &LearningGraphId,
    ) -> Result<crate::learning_graph::LearningGraphRecord, AppError> {
        let course = sqlx::query(
            "SELECT c.course_id, c.title, c.learning_goal, c.graph_meta_json, c.created_at \
             FROM learning_courses c \
             WHERE c.course_id = ? AND c.course_kind = 'learning_graph'",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning graph {id}")))?;
        let course_id: String = course.try_get("course_id").map_err(internal)?;
        let title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let created_at: i64 = course.try_get("created_at").map_err(internal)?;
        let meta_raw: Option<String> = course.try_get("graph_meta_json").map_err(internal)?;

        let lesson_rows = sqlx::query(
            "SELECT l.lesson_id, l.title, l.purpose, l.estimated_minutes \
             FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? \
             ORDER BY l.position, l.lesson_id",
        )
        .bind(&course_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let nodes: Vec<crate::learning_graph::LearningGraphNode> = lesson_rows
            .iter()
            .map(|row| {
                let estimated: i64 = row.try_get("estimated_minutes").map_err(internal)?;
                let purpose: String = row.try_get("purpose").map_err(internal)?;
                Ok(crate::learning_graph::LearningGraphNode {
                    id: row.try_get("lesson_id").map_err(internal)?,
                    title: row.try_get("title").map_err(internal)?,
                    min: u16::try_from(estimated).ok(),
                    group: None,
                    necessity: if purpose.trim().is_empty() { None } else { Some(purpose) },
                    is_anchor: None,
                })
            })
            .collect::<Result<_, AppError>>()?;

        let edge_rows = sqlx::query(
            "SELECT lesson_id, prerequisite_lesson_id, reason \
             FROM learning_graph_prerequisites WHERE course_id = ?",
        )
        .bind(&course_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let edges: Vec<crate::learning_graph::LearningGraphEdge> = edge_rows
            .iter()
            .map(|row| {
                let reason: String = row.try_get("reason").map_err(internal)?;
                Ok(crate::learning_graph::LearningGraphEdge {
                    from: row.try_get("prerequisite_lesson_id").map_err(internal)?,
                    to: row.try_get("lesson_id").map_err(internal)?,
                    reason: if reason.trim().is_empty() { None } else { Some(reason) },
                })
            })
            .collect::<Result<_, AppError>>()?;

        // The audit snapshot is stored read-only at publish time; the read
        // path never re-runs the deterministic audit.
        let audit = meta_raw
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|meta| meta.get("audit").cloned())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();

        Ok(crate::learning_graph::LearningGraphRecord {
            id: course_id,
            topic: if goal.trim().is_empty() { title } else { goal },
            graph: crate::learning_graph::LearningGraphData { nodes, edges, audit },
            created_at,
        })
    }

    /// 学习图课程生成入口（`GenerateCourseRequest.course_kind = learning_graph`）：
    /// 描述即学习目标，走引擎生成（含 slot 串行化与审计门禁），成功后返回
    /// 图课程的 `CourseDetail`。
    pub(crate) async fn generate_learning_graph_course(
        &self,
        user_id: &UserId,
        request: &GenerateCourseRequest,
    ) -> Result<CourseDetail, AppError> {
        let topic = request
            .description
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let record = self
            .generate_learning_graph(
                user_id,
                crate::learning_graph::GenerateLearningGraphRequest {
                    topic,
                    provider_id: request.provider_id.clone(),
                    model: request.model.clone(),
                },
            )
            .await?;
        let course_id = LearningCourseId::parse(&record.id)
            .map_err(|error| AppError::Internal(format!("invalid learning graph id: {error}")))?;
        self.course_detail(&course_id, Some(user_id)).await
    }

    /// 组装学习图课程的图视图。课时行直接复用 `course_detail` 已构建的
    /// `ModuleView`（单隐含模块），图查询只有前置边一条；就绪集与层深
    /// 在内存内计算（≤500 节点，遍历成本可忽略）。
    ///
    /// TODO(推荐策略): 就绪集按发布时拓扑序排序即可用；未来接入 mastery
    /// 加权 / 下游解锁面优先时，把这里的排序抽成可替换策略。
    pub(super) async fn assemble_learning_graph_view(
        &self,
        course_id: &LearningCourseId,
        modules: &[ModuleView],
    ) -> Result<LearningGraphView, AppError> {
        let course = sqlx::query(
            "SELECT learning_goal, learning_scope, graph_meta_json \
             FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let scope: String = course.try_get("learning_scope").map_err(internal)?;
        let meta_raw: Option<String> = course.try_get("graph_meta_json").map_err(internal)?;
        let meta: Option<serde_json::Value> =
            meta_raw.and_then(|raw| serde_json::from_str(&raw).ok());

        let edge_rows = sqlx::query(
            "SELECT lesson_id, prerequisite_lesson_id, reason \
             FROM learning_graph_prerequisites WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut edges: Vec<GraphEdgeView> = Vec::with_capacity(edge_rows.len());
        for row in edge_rows {
            let to: String = row.try_get("lesson_id").map_err(internal)?;
            let from: String = row.try_get("prerequisite_lesson_id").map_err(internal)?;
            edges.push(GraphEdgeView {
                from: parse_id(from)?,
                to: parse_id(to)?,
                reason: row.try_get("reason").map_err(internal)?,
            });
        }

        let mut lessons: Vec<&LessonView> = modules.iter().flat_map(|m| &m.lessons).collect();
        // position 即发布时拓扑序：升序一遍即可算出每个节点的层深。
        lessons.sort_by_key(|lesson| lesson.position);
        let mut status_by_id: HashMap<&str, LessonStatus> = HashMap::with_capacity(lessons.len());
        for lesson in &lessons {
            status_by_id.insert(lesson.id.as_str(), lesson.status);
        }
        let mut prerequisites: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut prerequisite_count: HashMap<&str, i64> = HashMap::new();
        for edge in &edges {
            prerequisites
                .entry(edge.to.as_str())
                .or_default()
                .push(edge.from.as_str());
            *prerequisite_count.entry(edge.to.as_str()).or_default() += 1;
        }
        let mut depth: HashMap<&str, i64> = HashMap::with_capacity(lessons.len());
        for lesson in &lessons {
            let level = prerequisites
                .get(lesson.id.as_str())
                .map(|pres| {
                    pres.iter()
                        .filter_map(|pre| depth.get(*pre).copied())
                        .max()
                        .unwrap_or(-1)
                        + 1
                })
                .unwrap_or(0);
            depth.insert(lesson.id.as_str(), level.max(0));
        }
        let nodes: Vec<GraphNodeView> = lessons
            .iter()
            .map(|lesson| GraphNodeView {
                lesson_id: lesson.id.clone(),
                title: lesson.title.clone(),
                summary: lesson.summary.clone(),
                purpose: lesson.purpose.clone(),
                estimated_minutes: lesson.estimated_minutes,
                generated: lesson.generated,
                position: lesson.position,
                depth: depth.get(lesson.id.as_str()).copied().unwrap_or(0),
                status: lesson.status,
                prerequisite_count: prerequisite_count
                    .get(lesson.id.as_str())
                    .copied()
                    .unwrap_or(0),
            })
            .collect();

        // 就绪集：自身未满足（未完成且未跳过）且全部前置已满足（completed
        // 或 skipped），按拓扑序取前 10。in_progress 节点保留（可继续学）。
        let mut recommended: Vec<LearningLessonId> = Vec::new();
        for lesson in &lessons {
            if lesson.status.satisfies() {
                continue;
            }
            let ready = prerequisites
                .get(lesson.id.as_str())
                .map(|pres| {
                    pres.iter().all(|pre| {
                        status_by_id
                            .get(*pre)
                            .copied()
                            .unwrap_or(LessonStatus::NotStarted)
                            .satisfies()
                    })
                })
                .unwrap_or(true);
            if ready {
                recommended.push(lesson.id.clone());
                if recommended.len() >= GRAPH_RECOMMEND_LIMIT {
                    break;
                }
            }
        }

        Ok(LearningGraphView {
            goal,
            scope,
            nodes,
            edges,
            recommended,
            meta,
        })
    }

    // ── Draft store (the lg_* tool set's backing) ─────────────────────────
    // In-memory only; `finish_learning_graph_draft` is the single publish
    // path to the database, gated by the deterministic audit.

    /// Snapshot one draft (drafts are small; cloning under the read lock is
    /// cheaper than holding the lock across any mutation). An entry older
    /// than [`LEARNING_GRAPH_DRAFT_TTL`] is reported as not found — only an
    /// abandoned draft can age out, and every active tool call refreshes the
    /// timestamp.
    fn draft(&self, draft_id: &str) -> Result<DraftGraph, AppError> {
        self.learning_graph_drafts
            .read()
            .map_err(|_| AppError::Internal("learning graph draft lock poisoned".into()))?
            .get(draft_id)
            .filter(|(_, created)| created.elapsed() < LEARNING_GRAPH_DRAFT_TTL)
            .map(|(graph, _)| graph.clone())
            .ok_or_else(|| AppError::NotFound(format!("learning graph draft {draft_id}")))
    }

    /// Start a draft: resolve the scope reference first (best-effort — a
    /// failed scope analysis degrades to a scope-free draft), then register
    /// an empty graph the agent builds with `patch_learning_graph_draft`.
    pub async fn create_learning_graph_draft(
        &self,
        topic: &str,
        focus: Option<&str>,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<crate::learning_graph::draft::DraftView, AppError> {
        let topic = topic.trim();
        if topic.is_empty() {
            return Err(AppError::BadRequest(
                "learning graph topic must not be empty".into(),
            ));
        }
        if topic.chars().count() > 200 {
            return Err(AppError::BadRequest("learning graph topic is too long".into()));
        }
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("learning graph generation is not configured".into())
            })?;
        let scope_topic = match focus {
            Some(focus) if !focus.trim().is_empty() => {
                format!("{topic}（用户补充：{}）", focus.trim())
            }
            _ => topic.to_owned(),
        };
        let scope = crate::learning_graph::analyze_scope(
            completer.as_ref(),
            model_override,
            &scope_topic,
        )
        .await;
        let draft = DraftGraph::new(topic.to_owned(), scope);
        let draft_id = LearningGraphId::new().as_str().to_owned();
        let view = draft.view(&draft_id);
        let now = std::time::Instant::now();
        let mut drafts = self
            .learning_graph_drafts
            .write()
            .map_err(|_| AppError::Internal("learning graph draft lock poisoned".into()))?;
        // Lazy TTL sweep: abandoned drafts (crashed or timed-out sessions)
        // must not accumulate in memory.
        drafts.retain(|_, (_, created)| now.duration_since(*created) < LEARNING_GRAPH_DRAFT_TTL);
        drafts.insert(draft_id, (draft, now));
        Ok(view)
    }

    /// Apply a batch of ops to a draft and return the per-op verdicts plus
    /// a fresh audit snapshot.
    pub fn patch_learning_graph_draft(
        &self,
        draft_id: &str,
        ops: Vec<crate::learning_graph::draft::GraphOp>,
    ) -> Result<crate::learning_graph::draft::PatchReport, AppError> {
        let mut draft = self.draft(draft_id)?;
        let report = draft.apply_ops(ops);
        // Refresh the TTL timestamp: an active patch session never ages out.
        self.learning_graph_drafts
            .write()
            .map_err(|_| AppError::Internal("learning graph draft lock poisoned".into()))?
            .insert(draft_id.to_owned(), (draft, std::time::Instant::now()));
        Ok(report)
    }

    /// Overview: sizes, workload, sub-domain distribution, entry/terminal
    /// units and the audit summary.
    pub fn inspect_learning_graph_draft(
        &self,
        draft_id: &str,
    ) -> Result<crate::learning_graph::draft::InspectView, AppError> {
        Ok(self.draft(draft_id)?.inspect())
    }

    /// Filtered unit list (name substring / sub-domain overlap / limit).
    pub fn query_learning_graph_draft(
        &self,
        draft_id: &str,
        filter: crate::learning_graph::draft::NodeQuery,
    ) -> Result<crate::learning_graph::draft::NodeListView, AppError> {
        Ok(self.draft(draft_id)?.query(&filter))
    }

    /// Ancestor/descendant closure around the given units.
    pub fn subgraph_learning_graph_draft(
        &self,
        draft_id: &str,
        nodes: Vec<String>,
        direction: crate::learning_graph::draft::SubgraphDirection,
        depth: Option<usize>,
    ) -> Result<crate::learning_graph::draft::SubgraphView, AppError> {
        Ok(self.draft(draft_id)?.subgraph(&nodes, direction, depth))
    }

    /// Full findings text plus the scope checklists and their live
    /// coverage — the repair loop's primary input. The audit is re-run
    /// live so the report never reflects a stale cached snapshot.
    pub fn audit_learning_graph_draft(&self, draft_id: &str) -> Result<String, AppError> {
        let mut draft = self.draft(draft_id)?;
        draft.refresh_audit();
        Ok(draft.audit_report())
    }

    /// The scope reference text (the generation loop's coverage
    /// checklist); `None` when no scope analysis ran for this draft.
    pub fn scope_reference_learning_graph_draft(
        &self,
        draft_id: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(self.draft(draft_id)?.scope_reference())
    }

    /// Publish a draft: the deterministic audit gate has the last word.
    /// Danger-grade findings block publishing (the draft survives, so the
    /// agent can keep repairing); a clean graph lands in the database as a
    /// learning-graph course and the draft is removed.
    pub async fn finish_learning_graph_draft(
        &self,
        user_id: &UserId,
        draft_id: &str,
    ) -> Result<crate::learning_graph::LearningGraphRecord, AppError> {
        // The finish gate re-runs the deterministic audit LIVE on the
        // draft's current state — the cached findings snapshot is never
        // trusted at the publish boundary, so a graph that was never
        // patched (or whose patches all failed) cannot slip through with
        // zero findings.
        let mut draft = self.draft(draft_id)?;
        draft.refresh_audit();
        if draft
            .graph
            .audit
            .findings
            .iter()
            .any(|finding| finding.severity == crate::learning_graph::SEV_DANGER)
        {
            return Err(AppError::UnprocessableEntity(format!(
                "learning graph draft still fails the audit gate:\n{}",
                draft.audit_report()
            )));
        }
        let order = topological_order(&draft.graph)?;
        let course_id = LearningCourseId::new();
        let module_id = LearningModuleId::new();
        let now = now_ms();
        let scope_text = draft.scope_reference().unwrap_or_default();
        let meta = serde_json::json!({ "audit": draft.graph.audit });
        let mut transaction = self.pool.begin().await.map_err(internal)?;

        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, learning_goal, \
              learning_scope, graph_meta_json, created_at, updated_at) \
             VALUES (?, ?, '', 'general', 1, 'learning_graph', ?, ?, ?, ?, ?)",
        )
        .bind(course_id.as_str())
        .bind(draft.topic.trim())
        .bind(draft.topic.trim())
        .bind(&scope_text)
        .bind(serde_json::to_string(&meta).map_err(internal)?)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;

        sqlx::query(
            "INSERT INTO learning_modules \
             (module_id, course_id, title, description, position) VALUES (?, ?, ?, '', 0)",
        )
        .bind(module_id.as_str())
        .bind(course_id.as_str())
        .bind(GRAPH_MODULE_TITLE)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;

        // One lesson row per node, in topological order — `position` doubles
        // as the stable ordering key for the recommendation query.
        let mut lesson_ids: HashMap<String, String> = HashMap::with_capacity(order.len());
        for (position, node) in order.iter().enumerate() {
            let lesson_id = LearningLessonId::new();
            let minutes = node.min.filter(|value| *value >= 1).unwrap_or(10) as i64;
            sqlx::query(
                "INSERT INTO learning_lessons \
                 (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
                  content_generated) \
                 VALUES (?, ?, ?, '', ?, ?, ?, 0)",
            )
            .bind(lesson_id.as_str())
            .bind(module_id.as_str())
            .bind(node.title.trim())
            .bind(node.necessity.as_deref().unwrap_or("").trim())
            .bind(position as i64)
            .bind(minutes)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
            lesson_ids.insert(node.id.clone(), lesson_id.into_string());
        }

        for edge in &draft.graph.edges {
            let (Some(to), Some(from)) = (
                lesson_ids.get(&edge.to),
                lesson_ids.get(&edge.from),
            ) else {
                // Dangling references cannot survive normalization, but a
                // defensive skip beats a failed publish.
                continue;
            };
            sqlx::query(
                "INSERT INTO learning_graph_prerequisites \
                 (course_id, lesson_id, prerequisite_lesson_id, reason) VALUES (?, ?, ?, ?)",
            )
            .bind(course_id.as_str())
            .bind(to)
            .bind(from)
            .bind(edge.reason.as_deref().unwrap_or("").trim())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        }

        transaction.commit().await.map_err(internal)?;
        self.learning_graph_drafts
            .write()
            .map_err(|_| AppError::Internal("learning graph draft lock poisoned".into()))?
            .remove(draft_id);
        let _ = user_id;
        let id = parse_id::<LearningGraphId>(course_id.into_string())?;
        self.get_learning_graph(&id).await
    }

}

/// Kahn topological order over the draft graph (prerequisite `from` before
/// dependent `to`), tie-broken by the nodes' original order so `position`
/// stays deterministic across equal-depth units. The draft is cycle-free by
/// construction (normalization strips cycles); a defensive error keeps a
/// corrupted draft from publishing.
fn topological_order(
    graph: &crate::learning_graph::LearningGraphData,
) -> Result<Vec<&crate::learning_graph::LearningGraphNode>, AppError> {
    let mut index: HashMap<&str, usize> = HashMap::with_capacity(graph.nodes.len());
    for (position, node) in graph.nodes.iter().enumerate() {
        index.insert(node.id.as_str(), position);
    }
    let mut indegree: Vec<usize> = vec![0; graph.nodes.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    for edge in &graph.edges {
        let (Some(&from), Some(&to)) = (
            index.get(edge.from.as_str()),
            index.get(edge.to.as_str()),
        ) else {
            continue;
        };
        if from == to {
            continue;
        }
        indegree[to] += 1;
        dependents[from].push(to);
    }
    // A binary heap keyed by original index keeps the order stable.
    let mut ready: std::collections::BinaryHeap<std::cmp::Reverse<usize>> = indegree
        .iter()
        .enumerate()
        .filter(|(_, degree)| **degree == 0)
        .map(|(position, _)| std::cmp::Reverse(position))
        .collect();
    let mut order = Vec::with_capacity(graph.nodes.len());
    while let Some(std::cmp::Reverse(position)) = ready.pop() {
        order.push(&graph.nodes[position]);
        for &dependent in &dependents[position] {
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                ready.push(std::cmp::Reverse(dependent));
            }
        }
    }
    if order.len() != graph.nodes.len() {
        return Err(AppError::Internal(
            "learning graph draft still contains a prerequisite cycle".into(),
        ));
    }
    Ok(order)
}
