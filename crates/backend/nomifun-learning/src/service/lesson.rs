use super::*;

impl LearningService {

    /// Repair a lesson figure that failed to render: the broken source and
    /// the renderer error go to the course completer, and the corrected
    /// figure body comes back for in-place re-rendering. Stateless — nothing
    /// is persisted.
    pub async fn repair_figure(
        &self,
        request: &crate::models::RepairFigureRequest,
    ) -> Result<crate::models::RepairFigureResponse, AppError> {
        const MAX_CODE_CHARS: usize = 100_000;
        const MAX_ERROR_CHARS: usize = 2_000;
        let language = request.language.trim();
        if language != "svg" && language != "jsxgraph" {
            return Err(AppError::UnprocessableEntity(format!(
                "unsupported figure language '{language}'"
            )));
        }
        if request.code.trim().is_empty() {
            return Err(AppError::UnprocessableEntity("figure code is empty".into()));
        }
        if request.code.chars().count() > MAX_CODE_CHARS {
            return Err(AppError::UnprocessableEntity("figure code is too long to repair".into()));
        }
        let error: String = request.error.chars().take(MAX_ERROR_CHARS).collect();
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed course generation is not configured".into())
            })?;
        let code = crate::generation::repair_figure(completer.as_ref(), None, language, &request.code, &error)
            .await
            .map_err(|error| {
                AppError::UnprocessableEntity(format!("figure repair failed: {error}"))
            })?;
        if code.trim().is_empty() {
            return Err(AppError::UnprocessableEntity(
                "figure repair returned an empty figure".into(),
            ));
        }
        Ok(crate::models::RepairFigureResponse { code })
    }

    /// 引擎生成课时内容：存在仍存活的草稿（上次会话失败/超时留下，TTL
    /// 1 小时内）时续跑而非从零重建（兑现迁移 050 的断点续跑承诺），否则
    /// 全新生成。传统与学习图两条课时路径共用。
    async fn generate_or_resume_lesson(
        &self,
        user_id: &UserId,
        engine: &Arc<dyn LessonContentAgentEngine>,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, AppError> {
        match self.live_lesson_draft_for_lesson(context.lesson_id.as_str()) {
            Some(draft_id) => {
                self.emit_lesson_event(serde_json::json!({
                    "phase": "resumed",
                    "lesson_id": context.lesson_id,
                    "draft_id": draft_id,
                }));
                engine.resume(user_id, &draft_id, context, model_override).await
            }
            None => engine.generate(user_id, context, model_override).await,
        }
    }

    /// Full lesson view for one lesson, including the sectioned body. The
    /// course-detail catalog deliberately omits section bodies (a 200-node
    /// graph course would carry hundreds of kilobytes), so the frontend
    /// fetches them here when the learner actually opens a lesson.
    pub async fn lesson_detail(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
    ) -> Result<LessonView, AppError> {
        let course_id: String = sqlx::query_scalar(
            "SELECT m.course_id FROM learning_modules m \
             JOIN learning_lessons l ON l.module_id = m.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;
        let course_id = parse_id::<LearningCourseId>(course_id)?;
        let enrollment = self.enrollment_id_for(user_id, &course_id).await?;
        self.lesson_view(lesson_id, enrollment.as_ref()).await
    }

    /// Generate the study document and activities for one on-demand lesson and
    /// persist them, returning the updated lesson view. Idempotent: a lesson
    /// that already has content returns its current view unchanged.
    pub async fn generate_lesson_content(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        request: &GenerateLessonRequest,
    ) -> Result<LessonView, AppError> {
        let row = sqlx::query(
            "SELECT l.module_id, l.position, l.content_generated, m.course_id, \
                    m.position AS module_position, c.course_kind, c.teaching_style \
             FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;

        let course_id: LearningCourseId = parse_id(row.try_get("course_id").map_err(internal)?)?;
        let teaching_style: TeachingStyle = TeachingStyle::try_from(
            row.try_get::<String, _>("teaching_style")
                .map_err(internal)?
                .as_str(),
        )
        .map_err(AppError::Internal)?;
        let content_generated: i64 = row.try_get("content_generated").map_err(internal)?;
        if content_generated != 0 {
            let enrollment = self.enrollment_id_for(user_id, &course_id).await?;
            return self.lesson_view(lesson_id, enrollment.as_ref()).await;
        }

        // 课程类型分流（幂等检查已共享）：学习图节点没有蓝图快照，走图
        // 专属路径——上下文来自课程行与前置边表，且只走 agent 引擎。
        if row.try_get::<String, _>("course_kind").map_err(internal)?
            == CourseKind::LearningGraph.as_str()
        {
            return self
                .generate_graph_lesson_content(user_id, lesson_id, &course_id, request)
                .await;
        }

        let snapshot = sqlx::query(
            "SELECT title, blueprint_json, samples_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let course_title: String = snapshot.try_get("title").map_err(internal)?;
        let blueprint_json: Option<String> = snapshot.try_get("blueprint_json").map_err(internal)?;
        let samples_json: Option<String> = snapshot.try_get("samples_json").map_err(internal)?;
        let blueprint_json = blueprint_json.ok_or_else(|| {
            AppError::Conflict("course outline is missing its blueprint snapshot".into())
        })?;
        let samples_json = samples_json.ok_or_else(|| {
            AppError::Conflict("course outline is missing its source samples".into())
        })?;
        let blueprint: Blueprint = serde_json::from_str(&blueprint_json).map_err(internal)?;
        let samples: Vec<(String, String)> = serde_json::from_str(&samples_json).map_err(internal)?;

        let module_position: i64 = row.try_get("module_position").map_err(internal)?;
        let lesson_position: i64 = row.try_get("position").map_err(internal)?;
        let module = blueprint
            .modules
            .get(module_position as usize)
            .ok_or_else(|| AppError::Internal("outline module position out of range".into()))?;
        let lesson = module
            .lessons
            .get(lesson_position as usize)
            .ok_or_else(|| AppError::Internal("outline lesson position out of range".into()))?;

        let excerpt: Option<LessonExcerpt> = lesson.source.as_ref().and_then(|source| {
            samples
                .iter()
                .find(|(path, _)| path == &source.path)
                .map(|(path, text)| LessonExcerpt {
                    path: path.clone(),
                    text: text.clone(),
                })
        });
        let total_lessons: usize = blueprint
            .modules
            .iter()
            .map(|module| module.lessons.len())
            .sum();
        let next_lesson_title = module
            .lessons
            .get(lesson_position as usize + 1)
            .map(|next| next.title.as_str());

        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let context = LessonGenerationContext {
            lesson_id: lesson_id.as_str().to_owned(),
            course_title,
            course_description: blueprint.description.clone(),
            module_title: module.title.clone(),
            module_index: module_position as usize,
            lesson_title: lesson.title.clone(),
            lesson_index: lesson_position as usize,
            total_lessons,
            next_lesson_title: next_lesson_title.map(str::to_owned),
            purpose: lesson.purpose.clone(),
            concepts: blueprint
                .concepts
                .iter()
                .filter(|concept| lesson.concepts.contains(&concept.key))
                .cloned()
                .collect(),
            concept_keys: lesson.concepts.clone(),
            excerpt,
            outline_tree: crate::generation::build_outline_tree(
                &blueprint,
                module_position as usize,
                lesson_position as usize,
            ),
            adjacent_context: crate::generation::build_adjacent_context(
                &blueprint,
                &samples,
                module_position as usize,
                lesson_position as usize,
            ),
            // 传统课时永远不走图分支。
            graph: None,
            forbidden_concepts: crate::generation::forbidden_concepts_text(&blueprint, lesson),
        };
        self.emit_lesson_event(serde_json::json!({
            "phase": "started",
            "lesson_id": lesson_id.as_str(),
            "title": lesson.title,
            "module": module.title,
        }));
        let output = match self.lesson_engine() {
            // Agent loop path: the injected two-loop engine owns the whole
            // lifecycle (draft + `ls_*` tools, audit-gated publish); its
            // LoopContext emits the round/audit progress frames itself.
            // A live draft from a failed run resumes instead of restarting.
            Some(engine) => {
                let result = self
                    .generate_or_resume_lesson(
                        user_id,
                        &engine,
                        &context,
                        model_override.map(|(provider, model)| (provider.as_str(), model)),
                    )
                    .await;
                match &result {
                    Ok(output) => self.emit_lesson_event(serde_json::json!({
                        "phase": "completed",
                        "lesson_id": lesson_id.as_str(),
                        "title": lesson.title,
                        "activities": output.activities.len(),
                        "estimated_minutes": output.estimated_minutes,
                    })),
                    Err(error) => self.emit_lesson_event(serde_json::json!({
                        "phase": "failed",
                        "lesson_id": lesson_id.as_str(),
                        "title": lesson.title,
                        "error": error.to_string(),
                    })),
                }
                result?
            }
            // Fallback: the legacy two-stage one-shot pipeline (tests and
            // direct calls), wrapped with the same terminal events so the
            // UI stays uniform.
            None => {
                let completer = self
                    .course_completer
                    .read()
                    .map_err(|_| {
                        AppError::Internal("learning course completer lock poisoned".into())
                    })?
                    .clone()
                    .ok_or_else(|| {
                        AppError::Conflict(
                            "knowledge-backed course generation is not configured".into(),
                        )
                    })?;
                match generate_lesson(
                    completer.as_ref(),
                    model_override,
                    &blueprint,
                    module,
                    lesson,
                    module_position as usize,
                    lesson_position as usize,
                    total_lessons,
                    next_lesson_title,
                    context.excerpt.as_ref().map(|e| e.text.as_str()).unwrap_or_default(),
                    teaching_style,
                )
                .await
                {
                    Ok(output) => {
                        self.emit_lesson_event(serde_json::json!({
                            "phase": "completed",
                            "lesson_id": lesson_id.as_str(),
                            "title": lesson.title,
                            "activities": output.activities.len(),
                            "estimated_minutes": output.estimated_minutes,
                        }));
                        output
                    }
                    Err(error) => {
                        self.emit_lesson_event(serde_json::json!({
                            "phase": "failed",
                            "lesson_id": lesson_id.as_str(),
                            "title": lesson.title,
                            "error": error,
                        }));
                        return Err(AppError::UnprocessableEntity(format!(
                            "lesson '{}' failed to generate: {error}",
                            lesson.title
                        )));
                    }
                }
            }
        };

        if !output.degraded_keys.is_empty() {
            // 降级兜底可见:哪些节以 visual=无 纯文字保底,便于事后重试。
            self.emit_lesson_event(serde_json::json!({
                "phase": "degraded",
                "lesson_id": lesson_id.as_str(),
                "sections": output.degraded_keys,
            }));
        }
        let concepts = self.concept_map_for_course(&course_id).await?;
        self.persist_lesson_output(lesson_id, &output, &concepts, &lesson.concepts)
            .await?;

        let enrollment = self.enrollment_id_for(user_id, &course_id).await?;
        self.lesson_view(lesson_id, enrollment.as_ref()).await
    }

    /// 学习图课程节点的内容生成（beta）。学习图课程没有蓝图快照：上下文
    /// 来自课程行（学习目标/学习范围）与前置边表（前置整条路径 + 下游
    /// 节点），节点与边一次载入后在 Rust 内计算（≤500 节点，不引入 SQL
    /// 递归 CTE）。只走注入的 agent 引擎——fallback 一次性管线消费
    /// `&Blueprint`，对图节点不可复用。
    async fn generate_graph_lesson_content(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        course_id: &LearningCourseId,
        request: &GenerateLessonRequest,
    ) -> Result<LessonView, AppError> {
        let lesson = sqlx::query(
            "SELECT l.title, l.purpose, l.position, m.title AS module_title \
             FROM learning_lessons l JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let lesson_title: String = lesson.try_get("title").map_err(internal)?;
        let purpose: String = lesson.try_get("purpose").map_err(internal)?;
        let lesson_position: i64 = lesson.try_get("position").map_err(internal)?;
        let module_title: String = lesson.try_get("module_title").map_err(internal)?;

        let course = sqlx::query(
            "SELECT title, learning_goal, learning_scope FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let course_title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course
            .try_get::<Option<String>, _>("learning_goal")
            .map_err(internal)?
            .unwrap_or_default();
        let scope: String = course
            .try_get::<Option<String>, _>("learning_scope")
            .map_err(internal)?
            .unwrap_or_default();

        // 一次载入全部节点与前置边（≤500 节点），内存算祖先闭包与后代。
        let (prerequisite_path, upcoming_nodes, forbidden_concepts, total_nodes) =
            self.graph_node_topology(course_id, lesson_id).await?;

        let context = LessonGenerationContext {
            lesson_id: lesson_id.as_str().to_owned(),
            course_title,
            // 图节点没有课程简报；目标/范围在 graph 段渲染。
            course_description: String::new(),
            module_title,
            module_index: 0,
            lesson_title,
            lesson_index: lesson_position.max(0) as usize,
            total_lessons: total_nodes,
            // 衔接语义由 graph.upcoming_nodes 承担。
            next_lesson_title: None,
            purpose,
            concepts: Vec::new(),
            concept_keys: Vec::new(),
            excerpt: None,
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: Some(GraphLessonContext {
                goal,
                scope,
                prerequisite_path,
                upcoming_nodes,
            }),
            // 防超纲黑名单：可及后代节点标题清单（graph_node_topology 推导）。
            forbidden_concepts,
        };

        self.emit_lesson_event(serde_json::json!({
            "phase": "started",
            "lesson_id": lesson_id.as_str(),
            "title": context.lesson_title,
            "module": context.module_title,
        }));
        let engine = self.lesson_engine().ok_or_else(|| {
            AppError::Conflict(
                "learning-graph lesson content generation requires the agent engine".into(),
            )
        })?;
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let result = self
            .generate_or_resume_lesson(
                user_id,
                &engine,
                &context,
                model_override.map(|(provider, model)| (provider.as_str(), model)),
            )
            .await;
        match &result {
            Ok(output) => self.emit_lesson_event(serde_json::json!({
                "phase": "completed",
                "lesson_id": lesson_id.as_str(),
                "title": context.lesson_title,
                "activities": output.activities.len(),
                "estimated_minutes": output.estimated_minutes,
            })),
            Err(error) => self.emit_lesson_event(serde_json::json!({
                "phase": "failed",
                "lesson_id": lesson_id.as_str(),
                "title": context.lesson_title,
                "error": error.to_string(),
            })),
        }
        let output = result?;
        // 学习图课程没有概念行：空 concept_map + 空 default_keys（audit
        // 保证活动不携带概念绑定）。
        self.persist_lesson_output(lesson_id, &output, &HashMap::new(), &[])
            .await?;

        let enrollment = self.enrollment_id_for(user_id, course_id).await?;
        self.lesson_view(lesson_id, enrollment.as_ref()).await
    }

    /// 学习图节点的拓扑上下文（节点内容生成与单节重写共用）：一次载入全
    /// 部节点与前置边（≤500 节点，不引入 SQL 递归 CTE），在 Rust 内计算
    /// ——前置已教摘要路径、后续节点段、可及后代禁止清单，以及全图节点数。
    async fn graph_node_topology(
        &self,
        course_id: &LearningCourseId,
        lesson_id: &LearningLessonId,
    ) -> Result<(String, String, String, usize), AppError> {
        let node_rows = sqlx::query(
            "SELECT l.lesson_id, l.title, l.position FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? ORDER BY l.position, l.lesson_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut nodes: HashMap<String, (i64, String)> = HashMap::with_capacity(node_rows.len());
        for node in &node_rows {
            nodes.insert(
                node.try_get::<String, _>("lesson_id").map_err(internal)?,
                (
                    node.try_get::<i64, _>("position").map_err(internal)?,
                    node.try_get::<String, _>("title").map_err(internal)?,
                ),
            );
        }
        let edge_rows = sqlx::query(
            "SELECT lesson_id, prerequisite_lesson_id, reason FROM learning_graph_prerequisites \
             WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        // 拥有化后再建邻接表，借用不挂在行对象上。
        let mut edges: Vec<(String, String, String)> = Vec::with_capacity(edge_rows.len());
        for edge in &edge_rows {
            edges.push((
                edge.try_get("lesson_id").map_err(internal)?,
                edge.try_get("prerequisite_lesson_id").map_err(internal)?,
                edge.try_get("reason").map_err(internal)?,
            ));
        }
        let mut predecessors: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut successors: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for (to, from, reason) in &edges {
            predecessors.entry(to.as_str()).or_default().push(from.as_str());
            successors
                .entry(from.as_str())
                .or_default()
                .push((to.as_str(), reason.as_str()));
        }

        let target = lesson_id.as_str();
        // 前置整条路径：沿前驱反向遍历出祖先闭包（发布时已保证无环，
        // visited 去重兜底防御），按拓扑序渲染，离节点最近者优先保留。
        let mut ancestors: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&str> = predecessors.get(target).cloned().unwrap_or_default();
        while let Some(current) = stack.pop() {
            if !ancestors.insert(current) {
                continue;
            }
            if let Some(parents) = predecessors.get(current) {
                stack.extend(parents.iter().copied());
            }
        }
        let mut path: Vec<(i64, &str, &str)> = ancestors
            .iter()
            .filter_map(|id| {
                nodes
                    .get(*id)
                    .map(|(position, title)| (*position, title.as_str(), *id))
            })
            .collect();
        path.sort_unstable_by_key(|(position, _, _)| *position);
        // 前置已教内容摘要（learnhub contextPack §2「前置摘要」）：闭包里
        // 已生成的课时附上其实际教过的节标题+要点。只有名字时模型不知道前
        // 置具体教了什么，重讲一遍是最高频的失败模式——摘要是防重复讲授的
        // 实质机制（未生成的前置仍只给标题）。
        let ancestor_ids: Vec<String> = ancestors.iter().map(|id| (*id).to_owned()).collect();
        let taught = self.taught_sections_for_lessons(&ancestor_ids).await?;
        let prerequisite_path = render_prerequisite_path(&path, &taught);

        // 后续节点：直接后继按拓扑序列出（带 reason），可及后代总数沿后继
        // 正向遍历计数（含直接后继）。
        let mut direct: Vec<(i64, &str, &str)> = successors
            .get(target)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id, reason)| {
                nodes
                    .get(id)
                    .map(|(position, title)| (*position, title.as_str(), reason))
            })
            .collect();
        direct.sort_unstable_by_key(|(position, _, _)| *position);
        let mut reachable: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&str> = successors
            .get(target)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        while let Some(current) = stack.pop() {
            if !reachable.insert(current) {
                continue;
            }
            if let Some(children) = successors.get(current) {
                stack.extend(children.iter().map(|(id, _)| *id));
            }
        }
        let upcoming_nodes = render_upcoming_nodes(&direct, reachable.len());

        // 防超纲黑名单（learnhub「禁止使用的概念」的学习图变体）：可及后
        // 代节点全集按拓扑位置降序（越靠下游越先列）截前 200。学习图课程
        // 没有概念表，黑名单的原料就是后代节点标题；前置/后续段落只是软引
        // 导，明确的「不得出现」清单才是防超纲的实质机制。
        let mut descendants: Vec<(i64, &str)> = reachable
            .iter()
            .filter_map(|id| nodes.get(*id).map(|(position, title)| (*position, title.as_str())))
            .collect();
        descendants.sort_unstable_by_key(|(position, _)| std::cmp::Reverse(*position));
        let forbidden_concepts = render_forbidden_descendants(&descendants);

        Ok((prerequisite_path, upcoming_nodes, forbidden_concepts, nodes.len()))
    }

    /// 单节重写（迁移 051/ADR-0002 的节级操作语义）：只重写一节正文并原
    /// 地更新（version+1），其他节与题目不动，summary 由全部节重新拼装。
    /// 走确定性单节管线（生成 + 定位修复 + 节级质检门 + visual 降级兜底）
    /// ——单节没有规划需求，agent 循环的多步自主价值用不上，一次有界调用
    /// 更省更稳。承诺事实源是节表里落库的 `visual` 声明（迁移 051）。
    pub async fn rewrite_lesson_section(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        section_key: &str,
        request: &GenerateLessonRequest,
    ) -> Result<LessonView, AppError> {
        let row = sqlx::query(
            "SELECT l.position, l.content_generated, m.course_id, c.course_kind, c.teaching_style \
             FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;
        let course_id: LearningCourseId = parse_id(row.try_get("course_id").map_err(internal)?)?;
        let course_kind = row.try_get::<String, _>("course_kind").map_err(internal)?;
        let teaching_style: TeachingStyle = TeachingStyle::try_from(
            row.try_get::<String, _>("teaching_style")
                .map_err(internal)?
                .as_str(),
        )
        .map_err(AppError::Internal)?;
        if row.try_get::<i64, _>("content_generated").map_err(internal)? == 0 {
            return Err(AppError::Conflict(
                "lesson content has not been generated yet".into(),
            ));
        }

        // 既有分节 = 单节重写的前提；按清单位置给出节任务与衔接参照。
        let sections = self.lesson_sections(lesson_id).await?;
        if sections.is_empty() {
            return Err(AppError::Conflict(
                "lesson has no sectioned content (legacy single-document lessons cannot be \
                 rewritten per section)"
                    .into(),
            ));
        }
        let position = sections
            .iter()
            .position(|section| section.section_key == section_key)
            .ok_or_else(|| {
                AppError::NotFound(format!("section {section_key} in lesson {lesson_id}"))
            })?;
        let current = &sections[position];
        let planned = crate::models::SectionPack {
            section_key: current.section_key.clone(),
            kind: current.kind,
            title: current.title.clone(),
            points: current.points.clone(),
            visual: current.visual.clone(),
            body_md: String::new(),
        };
        let previous_body = if position > 0 {
            Some(sections[position - 1].body_md.as_str())
        } else {
            None
        };
        let next_section_title = sections.get(position + 1).map(|s| s.title.as_str());

        // 防超纲与 grounded 上下文按课程类型分流（与两条生成路径同口径）。
        let (grounding, forbidden): (Option<(String, String)>, String) =
            if course_kind == CourseKind::LearningGraph.as_str() {
                let (prerequisite_path, _upcoming, forbidden, _total) =
                    self.graph_node_topology(&course_id, lesson_id).await?;
                let course = sqlx::query(
                    "SELECT learning_goal, learning_scope FROM learning_courses WHERE course_id = ?",
                )
                .bind(course_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(internal)?;
                let goal: String = course
                    .try_get::<Option<String>, _>("learning_goal")
                    .map_err(internal)?
                    .unwrap_or_default();
                let scope: String = course
                    .try_get::<Option<String>, _>("learning_scope")
                    .map_err(internal)?
                    .unwrap_or_default();
                let mut context_text = format!("学习目标：{}\n学习范围：{}", goal.trim(), scope.trim());
                if !prerequisite_path.trim().is_empty() {
                    context_text.push_str(&format!(
                        "\n\n前置路径（学习者已掌握，不要重复讲授）：\n{prerequisite_path}"
                    ));
                }
                (Some(("学习图节点上下文".into(), context_text)), forbidden)
            } else {
                let (forbidden, excerpt) = self.traditional_lesson_grounding(&course_id, lesson_id).await?;
                let grounding = (!excerpt.is_empty())
                    .then(|| ("引用摘录（正文必须忠于它）".into(), excerpt));
                (grounding, forbidden)
            };

        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed course generation is not configured".into())
            })?;
        let lesson_row: (String, String, String) = sqlx::query_as(
            "SELECT l.title, l.purpose, c.title FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let lesson_title = lesson_row.0;
        let lesson_purpose = lesson_row.1;

        let grounding_refs = grounding
            .as_ref()
            .map(|(label, text)| (label.as_str(), text.as_str()));
        let manifest: Vec<crate::models::SectionPack> = sections
            .iter()
            .map(|section| crate::models::SectionPack {
                section_key: section.section_key.clone(),
                kind: section.kind,
                title: section.title.clone(),
                points: section.points.clone(),
                visual: section.visual.clone(),
                body_md: String::new(),
            })
            .collect();
        let (body, degraded) = crate::generation::rewrite_section_body(
            completer.as_ref(),
            request.provider_id.as_ref().zip(request.model.as_deref()),
            teaching_style,
            &lesson_row.2,
            &lesson_title,
            &lesson_purpose,
            &planned,
            position,
            &manifest,
            previous_body,
            next_section_title,
            grounding_refs,
            &forbidden,
            crate::models::ComplexityTier::Mid,
        )
        .await
        .map_err(|error| {
            AppError::UnprocessableEntity(format!(
                "section '{section_key}' failed to rewrite: {error}"
            ))
        })?;
        if degraded {
            // 降级兜底可见:该节以 visual=无 纯文字保底,便于事后重试。
            self.emit_lesson_event(serde_json::json!({
                "phase": "degraded",
                "lesson_id": lesson_id.as_str(),
                "sections": [section_key],
            }));
        }
        self.persist_rewritten_section(lesson_id, section_key, &body).await?;

        let enrollment = self.enrollment_id_for(user_id, &course_id).await?;
        self.lesson_view(lesson_id, enrollment.as_ref()).await
    }

    /// 传统课时的 grounding 与防超纲黑名单（蓝图快照派生，与生成路径同
    /// 源）；没有快照的课程（教程课）给空黑名单与空摘录。
    async fn traditional_lesson_grounding(
        &self,
        course_id: &LearningCourseId,
        lesson_id: &LearningLessonId,
    ) -> Result<(String, String), AppError> {
        let snapshot = sqlx::query(
            "SELECT blueprint_json, samples_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let Some(snapshot) = snapshot else {
            return Ok((String::new(), String::new()));
        };
        let blueprint_json: Option<String> = snapshot.try_get("blueprint_json").map_err(internal)?;
        let samples_json: Option<String> = snapshot.try_get("samples_json").map_err(internal)?;
        let (Some(blueprint_json), Some(samples_json)) = (blueprint_json, samples_json) else {
            return Ok((String::new(), String::new()));
        };
        let blueprint: Blueprint = serde_json::from_str(&blueprint_json).map_err(internal)?;
        let samples: Vec<(String, String)> = serde_json::from_str(&samples_json).map_err(internal)?;
        let coordinates: Option<(i64, i64)> = sqlx::query_as(
            "SELECT m.position, l.position FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let Some((module_position, lesson_position)) = coordinates else {
            return Ok((String::new(), String::new()));
        };
        let Some(module) = blueprint.modules.get(module_position as usize) else {
            return Ok((String::new(), String::new()));
        };
        let Some(lesson) = module.lessons.get(lesson_position as usize) else {
            return Ok((String::new(), String::new()));
        };
        let excerpt = lesson
            .source
            .as_ref()
            .and_then(|source| {
                samples
                    .iter()
                    .find(|(path, _)| path == &source.path)
                    .map(|(_, text)| text.clone())
            })
            .unwrap_or_default();
        Ok((crate::generation::forbidden_concepts_text(&blueprint, lesson), excerpt))
    }

    /// 单节落库：正文原地替换（version+1），summary 由全部节重新拼装
    /// （双读回退的文本同步）。单事务——节更新与 summary 拼装同生共死。
    async fn persist_rewritten_section(
        &self,
        lesson_id: &LearningLessonId,
        section_key: &str,
        body: &str,
    ) -> Result<(), AppError> {
        let now = now_ms();
        let body = crate::generation::fix_mermaid_quotes(body.trim());
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let updated = sqlx::query(
            "UPDATE learning_lesson_sections \
             SET body_md = ?, status = 'ready', version = version + 1, updated_at = ? \
             WHERE lesson_id = ? AND section_key = ?",
        )
        .bind(&body)
        .bind(now)
        .bind(lesson_id.as_str())
        .bind(section_key)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        if updated.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "section {section_key} in lesson {lesson_id}"
            )));
        }
        let bodies: Vec<String> = sqlx::query_scalar(
            "SELECT body_md FROM learning_lesson_sections \
             WHERE lesson_id = ? ORDER BY position, section_key",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&mut *transaction)
        .await
        .map_err(internal)?;
        let summary = bodies
            .iter()
            .map(|body| body.trim())
            .collect::<Vec<_>>()
            .join("\n\n");
        sqlx::query("UPDATE learning_lessons SET summary = ?, updated_at = ? WHERE lesson_id = ?")
            .bind(&summary)
            .bind(now)
            .bind(lesson_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

    /// 批量取一组课时的已生成节摘要（`status='ready'` 节的标题+要点，按
    /// 节位置排序），按 `lesson_id` 分组——学习图节点生成的前置摘要原料。
    /// 未生成或生成中的课时不会出现在返回值里（调用方据此只给标题）。
    async fn taught_sections_for_lessons(
        &self,
        lesson_ids: &[String],
    ) -> Result<HashMap<String, Vec<(String, String)>>, AppError> {
        if lesson_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut query = sqlx::QueryBuilder::new(
            "SELECT lesson_id, title, points FROM learning_lesson_sections \
             WHERE status = 'ready' AND lesson_id IN (",
        );
        let mut separated = query.separated(", ");
        for id in lesson_ids {
            separated.push_bind(id);
        }
        query.push(") ORDER BY lesson_id, position");
        let rows = query.build().fetch_all(&self.pool).await.map_err(internal)?;
        let mut taught: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for row in rows {
            let lesson_id: String = row.try_get("lesson_id").map_err(internal)?;
            let title: String = row.try_get("title").map_err(internal)?;
            let points: String = row.try_get("points").map_err(internal)?;
            taught.entry(lesson_id).or_default().push((title, points));
        }
        Ok(taught)
    }

    /// 生成事务落库尾（传统与学习图两条路径共用）：更新课时行（文档/时长/
    /// 生成标记）、替换全部活动并绑定概念。`default_keys` 是活动未显式绑定
    /// 概念时的回退（传统课时 = 整课概念）；学习图节点传空——节点不绑定
    /// 概念，活动若携带绑定会在空 `concept_map` 上以 unknown key 失败。
    async fn persist_lesson_output(
        &self,
        lesson_id: &LearningLessonId,
        output: &LessonOutput,
        concept_map: &HashMap<String, LearningConceptId>,
        default_keys: &[String],
    ) -> Result<(), AppError> {
        let now = now_ms();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        // 分节输出：节清单整体替换（幂等重生成语义），summary 存拼装文本
        // （双读回退）。单篇文档输出（agent 旧契约）不动节表。
        if !output.sections.is_empty() {
            sqlx::query("DELETE FROM learning_lesson_sections WHERE lesson_id = ?")
                .bind(lesson_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            for (position, section) in output.sections.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO learning_lesson_sections \
                     (section_key, lesson_id, kind, title, points, visual, body_md, status, version, position, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, 'ready', 1, ?, ?, ?)",
                )
                .bind(section.section_key.trim())
                .bind(lesson_id.as_str())
                .bind(section.kind.as_str())
                .bind(section.title.trim())
                .bind(section.points.trim())
                .bind(section.visual.trim())
                .bind(section.body_md.trim())
                .bind(position as i64)
                .bind(now)
                .bind(now)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            }
        }
        sqlx::query(
            "UPDATE learning_lessons SET summary = ?, estimated_minutes = ?, content_generated = 1 WHERE lesson_id = ?",
        )
        .bind(output.summary.trim())
        .bind(output.estimated_minutes)
        .bind(lesson_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;

        // Replace any prior partial activities (idempotent re-generation).
        sqlx::query(
            "DELETE FROM learning_activity_concepts WHERE activity_id IN (SELECT activity_id FROM learning_activities WHERE lesson_id = ?)",
        )
        .bind(lesson_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        sqlx::query("DELETE FROM learning_activities WHERE lesson_id = ?")
            .bind(lesson_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;

        for (position, activity) in output.activities.iter().enumerate() {
            let activity_id = LearningActivityId::new();
            let config = StoredActivityConfig {
                options: activity.options.clone(),
                answer: activity.answer.clone(),
                explanation: activity.explanation.clone(),
                distractors: activity.distractors.clone(),
                tol: activity.tol,
                matches: matching_candidates(activity),
                difficulty: activity.difficulty,
            };
            // 「general」是提示词对跨节综合题的约定写法，落库归一为 NULL。
            let section_key = activity
                .section_key
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty() && !key.eq_ignore_ascii_case("general"));
            sqlx::query(
                "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, config_json, section_key, position) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(activity_id.as_str())
            .bind(lesson_id.as_str())
            .bind(activity.kind.as_str())
            .bind(activity.prompt.trim())
            .bind(serde_json::to_string(&config).map_err(internal)?)
            .bind(section_key)
            .bind(position as i64)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;

            let activity_concepts = if activity.concepts.is_empty() {
                default_keys
            } else {
                activity.concepts.as_slice()
            };
            for concept_key in activity_concepts {
                let concept_id = concept_map.get(concept_key).ok_or_else(|| {
                    AppError::Internal(format!("unknown concept key {concept_key}"))
                })?;
                sqlx::query(
                    "INSERT INTO learning_activity_concepts (activity_id, concept_id) VALUES (?, ?)",
                )
                .bind(activity_id.as_str())
                .bind(concept_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            }
        }
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

    /// Manually appends an activity to a generated lesson. The lesson must
    /// belong to a course the learner is enrolled in (the enrollment is
    /// created on demand like every other practice flow). An empty
    /// `concept_ids` binds the activity to every concept of the lesson;
    /// when the lesson is already completed, an objective question is also
    /// admitted to the review queue immediately via the idempotent seeder.
    pub async fn create_lesson_activity(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        request: CreateLessonActivityRequest,
    ) -> Result<LessonView, AppError> {
        let (prompt, config) = validate_question_payload(
            request.kind,
            &request.prompt,
            &request.options,
            &request.answer,
            &request.explanation,
            &request.distractors,
        )?;
        // Resolve the enrollment through the lesson, creating it on demand
        // exactly like update_lesson_progress does for the first write.
        let enrollment_id: Option<String> = sqlx::query_scalar(
            "SELECT e.enrollment_id FROM learning_enrollments e \
             JOIN learning_modules m ON m.course_id = e.course_id \
             JOIN learning_lessons l ON l.module_id = m.module_id \
             WHERE e.user_id = ? AND l.lesson_id = ?",
        )
        .bind(user_id.as_str())
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let enrollment_id = match enrollment_id {
            Some(enrollment_id) => enrollment_id,
            None => {
                let course_id: String = sqlx::query_scalar(
                    "SELECT m.course_id FROM learning_modules m \
                     JOIN learning_lessons l ON l.module_id = m.module_id \
                     WHERE l.lesson_id = ?",
                )
                .bind(lesson_id.as_str())
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?
                .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;
                let enrollment =
                    self.ensure_enrollment(&parse_id(course_id)?, user_id).await?;
                enrollment.as_str().to_owned()
            }
        };
        let enrollment: LearningEnrollmentId = parse_id(enrollment_id)?;

        // Concept bindings: an empty list defaults to every concept of the
        // lesson, matching course-generation semantics.
        let lesson_concept_ids = self.lesson_concepts(lesson_id).await?;
        let concept_ids: Vec<LearningConceptId> = if request.concept_ids.is_empty() {
            lesson_concept_ids.clone()
        } else {
            for concept_id in &request.concept_ids {
                if !lesson_concept_ids.contains(concept_id) {
                    return Err(AppError::BadRequest(format!(
                        "concept {concept_id} is not bound to this lesson"
                    )));
                }
            }
            request.concept_ids.clone()
        };

        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM learning_activities WHERE lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;

        // A completed lesson admits objective questions into the review
        // queue right away; the seeder keeps its own idempotence.
        let completed: Option<String> = sqlx::query_scalar(
            "SELECT status FROM learning_lesson_progress \
             WHERE enrollment_id = ? AND lesson_id = ? AND status = 'completed'",
        )
        .bind(enrollment.as_str())
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;

        let activity_id = LearningActivityId::new();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_activities \
             (activity_id, lesson_id, kind, prompt, config_json, position) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(activity_id.as_str())
        .bind(lesson_id.as_str())
        .bind(request.kind.as_str())
        .bind(&prompt)
        .bind(serde_json::to_string(&config).map_err(internal)?)
        .bind(position)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        for concept_id in &concept_ids {
            sqlx::query(
                "INSERT INTO learning_activity_concepts (activity_id, concept_id) VALUES (?, ?)",
            )
            .bind(activity_id.as_str())
            .bind(concept_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        }
        if completed.is_some() && request.kind != ActivityKind::Reflection {
            let now = now_ms();
            let tz_offset_minutes = self.tz_offset_minutes().await;
            ensure_review_item(
                &mut transaction,
                &enrollment,
                activity_id.as_str(),
                now,
                tz_offset_minutes,
            )
            .await?;
        }
        transaction.commit().await.map_err(internal)?;

        self.lesson_view(lesson_id, Some(&enrollment)).await
    }

    /// Generates ONE additional activity draft for an existing lesson, in the
    /// learner-chosen kind, grounded in the finished lesson document, its
    /// cited excerpt, and the lesson's concepts — with every existing
    /// question listed so the model must cover new ground. The draft is
    /// returned for preview and nothing is persisted.
    pub async fn generate_lesson_activity(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        request: GenerateLessonActivityRequest,
    ) -> Result<GeneratedLessonActivity, AppError> {
        // The lesson must belong to a course the learner is enrolled in;
        // generation is a read-only preview, so no enrollment is created.
        let enrolled: Option<String> = sqlx::query_scalar(
            "SELECT e.enrollment_id FROM learning_enrollments e \
             JOIN learning_modules m ON m.course_id = e.course_id \
             JOIN learning_lessons l ON l.module_id = m.module_id \
             WHERE e.user_id = ? AND l.lesson_id = ?",
        )
        .bind(user_id.as_str())
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        if enrolled.is_none() {
            return Err(AppError::NotFound(format!("learning lesson {lesson_id}")));
        }

        let row = sqlx::query(
            "SELECT l.title, l.position, l.summary, l.content_generated, m.course_id, \
                    m.position AS module_position, m.title AS module_title \
             FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;
        let course_id: LearningCourseId = parse_id(row.try_get("course_id").map_err(internal)?)?;
        let content_generated: i64 = row.try_get("content_generated").map_err(internal)?;
        if content_generated == 0 {
            return Err(AppError::Conflict(
                "lesson content has not been generated yet".into(),
            ));
        }
        let summary: String = row.try_get("summary").map_err(internal)?;
        let module_title: String = row.try_get("module_title").map_err(internal)?;
        let lesson_title: String = row.try_get("title").map_err(internal)?;

        let snapshot = sqlx::query(
            "SELECT title, blueprint_json, samples_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let course_title: String = snapshot.try_get("title").map_err(internal)?;
        let blueprint_json: Option<String> = snapshot.try_get("blueprint_json").map_err(internal)?;
        let samples_json: Option<String> = snapshot.try_get("samples_json").map_err(internal)?;

        // Prefer the outline snapshot when present. Courses imported without
        // one (e.g. the built-in tutorial) fall back to concepts reconstructed
        // from the database and an empty excerpt.
        let (concepts, lesson_concept_keys, excerpt) =
            if let (Some(blueprint_json), Some(samples_json)) = (blueprint_json, samples_json) {
                let blueprint: Blueprint = serde_json::from_str(&blueprint_json).map_err(internal)?;
                let samples: Vec<(String, String)> =
                    serde_json::from_str(&samples_json).map_err(internal)?;
                let module_position: i64 = row.try_get("module_position").map_err(internal)?;
                let lesson_position: i64 = row.try_get("position").map_err(internal)?;
                let module = blueprint
                    .modules
                    .get(module_position as usize)
                    .ok_or_else(|| AppError::Internal("outline module position out of range".into()))?;
                let lesson = module
                    .lessons
                    .get(lesson_position as usize)
                    .ok_or_else(|| AppError::Internal("outline lesson position out of range".into()))?;
                let excerpt = lesson
                    .source
                    .as_ref()
                    .and_then(|source| {
                        samples
                            .iter()
                            .find(|(path, _)| path == &source.path)
                            .map(|(_, excerpt)| excerpt.as_str())
                    })
                    .unwrap_or_default()
                    .to_string();
                (blueprint.concepts, lesson.concepts.clone(), excerpt)
            } else {
                (
                    self.course_concepts_from_db(&course_id).await?,
                    self.lesson_concept_keys(lesson_id).await?,
                    String::new(),
                )
            };
        let existing_questions = self.existing_lesson_questions(lesson_id).await?;

        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("knowledge-backed course generation is not configured".into())
            })?;
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let activity = generate_lesson_activity(
            completer.as_ref(),
            model_override,
            request.kind,
            request.focus.trim(),
            course_title.trim(),
            module_title.trim(),
            lesson_title.trim(),
            &concepts,
            &lesson_concept_keys,
            &summary,
            &excerpt,
            &existing_questions,
        )
        .await
        .map_err(|error| {
            AppError::UnprocessableEntity(format!("failed to generate lesson activity: {error}"))
        })?;

        // Suggested bindings: the model's own concept keys when present,
        // otherwise every concept of the lesson.
        let concept_ids = if activity.concepts.is_empty() {
            self.lesson_concepts(lesson_id).await?
        } else {
            let concept_map = self.concept_map_for_course(&course_id).await?;
            let mut ids = Vec::with_capacity(activity.concepts.len());
            for key in &activity.concepts {
                let concept_id = concept_map
                    .get(key)
                    .ok_or_else(|| AppError::Internal(format!("unknown concept key {key}")))?;
                ids.push(concept_id.clone());
            }
            ids
        };

        Ok(GeneratedLessonActivity {
            kind: activity.kind,
            prompt: activity.prompt,
            options: activity.options,
            answer: activity.answer,
            explanation: activity.explanation,
            distractors: activity.distractors,
            concept_ids,
        })
    }

    /// Every concept of a course as prompt-ready packs, used when the outline
    /// snapshot is missing (courses imported without one, e.g. the tutorial).
    async fn course_concepts_from_db(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<Vec<ConceptPack>, AppError> {
        let rows = sqlx::query(
            "SELECT concept_key, title, description FROM learning_concepts \
             WHERE course_id = ? ORDER BY title",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut concepts = Vec::with_capacity(rows.len());
        for row in rows {
            concepts.push(ConceptPack {
                key: row.try_get("concept_key").map_err(internal)?,
                title: row.try_get("title").map_err(internal)?,
                description: row.try_get("description").map_err(internal)?,
                prerequisites: Vec::new(),
            });
        }
        Ok(concepts)
    }

    /// The concept keys bound to a lesson, for the generation prompt when the
    /// blueprint snapshot is unavailable.
    async fn lesson_concept_keys(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<Vec<String>, AppError> {
        let keys: Vec<String> = sqlx::query_scalar(
            "SELECT c.concept_key FROM learning_lesson_concepts lc \
             JOIN learning_concepts c ON c.concept_id = lc.concept_id \
             WHERE lc.lesson_id = ? ORDER BY c.concept_key",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(keys)
    }

    /// Every question already present in a lesson, ready for the
    /// single-addition generation prompt's novelty requirement.
    async fn existing_lesson_questions(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<Vec<crate::generation::ExistingLessonQuestion>, AppError> {
        let rows = sqlx::query(
            "SELECT kind, prompt, config_json FROM learning_activities \
             WHERE lesson_id = ? ORDER BY position, activity_id",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut questions = Vec::with_capacity(rows.len());
        for row in rows {
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            let config: StoredActivityConfig = serde_json::from_str(
                &row.try_get::<String, _>("config_json").map_err(internal)?,
            )
            .map_err(internal)?;
            questions.push(crate::generation::ExistingLessonQuestion {
                kind: ActivityKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?,
                prompt: row.try_get("prompt").map_err(internal)?,
                answer: config.answer,
                explanation: config.explanation,
            });
        }
        Ok(questions)
    }

}

// ── 学习图节点上下文渲染 ────────────────────────────────────────────────────

/// 提示词里前置路径的渲染上限：只保留离节点最近的一段，更早的合并概括
/// （500 节点级图的祖先闭包可能极长）。
const GRAPH_PATH_RENDER_LIMIT: usize = 20;

/// 单个前置课时在摘要里最多列出的已教节数（节清单硬上限 8，取齐即可）。
const GRAPH_TAUGHT_SECTION_LIMIT: usize = 8;

/// 直接后继的渲染上限：超过时列前 10 个并注明总数。
const GRAPH_SUCCESSOR_RENDER_LIMIT: usize = 10;

/// 下游节点禁止清单的条目上限（learnhub 黑名单同款截断）。
const GRAPH_FORBIDDEN_LIMIT: usize = 200;

/// 前置路径渲染：按拓扑序全局编号；超出上限时只渲染离节点最近的一段，
/// 更早的合并成概括行。已生成的前置课时在其条目下附「实际教过的节标题+
/// 要点」摘要（learnhub contextPack §2）——这是防重复讲授的实质机制；
/// 未生成的前置只给标题。节点内容是共享资产，不标注任何用户进度。
fn render_prerequisite_path(
    path: &[(i64, &str, &str)],
    taught: &HashMap<String, Vec<(String, String)>>,
) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    let start = path.len().saturating_sub(GRAPH_PATH_RENDER_LIMIT);
    if start > 0 {
        lines.push(format!(
            "……（更早还有 {start} 个前置节点，均已掌握，此处省略）"
        ));
    }
    for (offset, (_, title, id)) in path[start..].iter().enumerate() {
        lines.push(format!("{}. {}", start + offset + 1, title.trim()));
        if let Some(sections) = taught.get(*id) {
            for (section_title, points) in sections.iter().take(GRAPH_TAUGHT_SECTION_LIMIT) {
                let points = points.trim();
                let summary = if points.is_empty() {
                    section_title.trim().to_owned()
                } else {
                    format!("{}：{points}", section_title.trim())
                };
                lines.push(format!("   · {summary}"));
            }
        }
    }
    lines.join("\n")
}

/// 后续节点渲染：直接后继按拓扑序列出（reason 非空时附注），超出上限
/// 截断并注明总数；末行总述可及下游规模，供衔接句的分寸参考。
fn render_upcoming_nodes(direct: &[(i64, &str, &str)], reachable: usize) -> String {
    if direct.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    for (_, title, reason) in direct.iter().take(GRAPH_SUCCESSOR_RENDER_LIMIT) {
        let reason = reason.trim();
        if reason.is_empty() {
            lines.push(format!("- {}", title.trim()));
        } else {
            lines.push(format!("- {}（{}）", title.trim(), reason));
        }
    }
    if direct.len() > GRAPH_SUCCESSOR_RENDER_LIMIT {
        lines.push(format!("……等共 {} 个直接后继", direct.len()));
    }
    if reachable > direct.len() {
        lines.push(format!(
            "下游共 {reachable} 个节点（含上列直接后继）——保持衔接，不要展开。"
        ));
    }
    lines.join("\n")
}

/// 下游节点禁止清单渲染（learnhub「禁止使用的概念」的学习图变体）：正文
/// 不得出现这些名称、不得引用其结论。按拓扑位置降序排列（越靠下游越先
/// 列），超出上限截断并注明总数；空集返回空串（该节点是图的终点）。
fn render_forbidden_descendants(descendants: &[(i64, &str)]) -> String {
    if descendants.is_empty() {
        return String::new();
    }
    let total = descendants.len();
    let mut text = format!(
        "禁止提前讲授的下游节点（尚未到达——正文不得出现这些名称，不得引用其结论，不得为它们做铺垫；共 {total} 个）：\n"
    );
    let listed: Vec<String> = descendants
        .iter()
        .take(GRAPH_FORBIDDEN_LIMIT)
        .map(|(_, title)| format!("- {}", title.trim()))
        .collect();
    text.push_str(&listed.join("\n"));
    text
}

/// Matching questions expose their right-column candidates to the UI (a
/// scrambled copy is rendered per client); every other kind stores none.
pub(super) fn matching_candidates(activity: &crate::models::ActivityPack) -> Vec<String> {
    if activity.kind != ActivityKind::Matching {
        return Vec::new();
    }
    activity
        .answer
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Shared payload validation for course activities and custom questions so
/// `evaluate` keeps working for both. Returns the trimmed prompt and the
/// persisted config. The per-kind shape rules live in
/// [`crate::models::ActivityPack::validate_shape`]; this wrapper maps them to
/// HTTP bad-requests and builds the stored config.
pub(super) fn validate_question_payload(
    kind: ActivityKind,
    prompt: &str,
    options: &[String],
    answer: &Value,
    explanation: &str,
    distractors: &[String],
) -> Result<(String, StoredActivityConfig), AppError> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(AppError::BadRequest(
            "question prompt must not be empty".into(),
        ));
    }
    let trim_all = |values: &[String]| -> Vec<String> {
        values
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect()
    };
    let distractors = trim_all(distractors);
    // Manual authoring accepts 2-5 options (generation demands 3-5).
    let pack = crate::models::ActivityPack {
        kind,
        prompt: prompt.to_owned(),
        options: trim_all(options),
        answer: answer.clone(),
        explanation: explanation.to_owned(),
        concepts: Vec::new(),
        distractors,
        tol: None,
        section_key: None,
        difficulty: None,
    };
    pack.validate_shape((2, 5), false)
        .map_err(AppError::BadRequest)?;
    // fill_in_blank keeps its distractor rule through validate_shape; the
    // numeric tol is learner-authored so it rides in the config as given.
    let matches = matching_candidates(&pack);
    let config = StoredActivityConfig {
        options: pack.options,
        answer: pack.answer,
        explanation: pack.explanation,
        distractors: pack.distractors,
        tol: pack.tol,
        matches,
        difficulty: pack.difficulty,
    };
    Ok((prompt.to_string(), config))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前置路径渲染：已生成的前置附「实际教过的节标题+要点」摘要，未生
    /// 成的前置只有标题行（learnhub contextPack §2 的前置摘要语义）。
    #[test]
    fn prerequisite_path_carries_taught_section_summaries() {
        let path = vec![
            (0i64, "什么是衍生品", "lesson-a"),
            (1i64, "期权的定义与分类", "lesson-b"),
        ];
        let mut taught = HashMap::new();
        taught.insert(
            "lesson-b".to_owned(),
            vec![
                (
                    "概念：期权定义".to_owned(),
                    "买方持有权利、卖方承担义务".to_owned(),
                ),
                ("例题：认购与认沽".to_owned(), "四类基本头寸".to_owned()),
            ],
        );
        let text = render_prerequisite_path(&path, &taught);
        assert!(text.contains("1. 什么是衍生品"), "{text}");
        assert!(text.contains("2. 期权的定义与分类"), "{text}");
        assert!(text.contains("· 概念：期权定义：买方持有权利、卖方承担义务"), "{text}");
        assert!(text.contains("· 例题：认购与认沽：四类基本头寸"), "{text}");
        // 未生成的前置（lesson-a）不产生摘要行：标题行 + 前置的 3 行摘要。
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text}");
    }

    /// 下游禁止清单：非空时附「不得出现/不得引用」约束与总数；空集（终
    /// 点节点）返回空串，提示词不渲染该段。
    #[test]
    fn forbidden_descendants_render_the_blacklist_contract() {
        assert_eq!(render_forbidden_descendants(&[]), "");
        let titles: Vec<String> = (0..(GRAPH_FORBIDDEN_LIMIT + 5))
            .map(|i| format!("下游单元{i}"))
            .collect();
        let descendants: Vec<(i64, &str)> =
            titles.iter().enumerate().map(|(i, title)| (i as i64, title.as_str())).collect();
        let text = render_forbidden_descendants(&descendants);
        assert!(text.contains(&format!("共 {} 个", descendants.len())), "{text}");
        assert!(text.contains("- 下游单元0"));
        // 超限截断：只列前 200 条。
        assert!(!text.contains("- 下游单元200"), "{text}");
        assert!(text.contains(format!("- 下游单元{}", GRAPH_FORBIDDEN_LIMIT - 1).as_str()));
    }
}
