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
                    m.position AS module_position, c.course_kind \
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
            Some(engine) => {
                let result = engine
                    .generate(
                        user_id,
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
                    &samples,
                    module,
                    lesson,
                    module_position as usize,
                    lesson_position as usize,
                    total_lessons,
                    next_lesson_title,
                    context.excerpt.as_ref().map(|e| e.text.as_str()).unwrap_or_default(),
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
        let mut path: Vec<(i64, &str)> = ancestors
            .iter()
            .filter_map(|id| nodes.get(*id).map(|(position, title)| (*position, title.as_str())))
            .collect();
        path.sort_unstable_by_key(|(position, _)| *position);
        let prerequisite_path = render_prerequisite_path(&path);

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

        let context = LessonGenerationContext {
            course_title,
            // 图节点没有课程简报；目标/范围在 graph 段渲染。
            course_description: String::new(),
            module_title,
            module_index: 0,
            lesson_title,
            lesson_index: lesson_position.max(0) as usize,
            total_lessons: nodes.len(),
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
        let result = engine
            .generate(
                user_id,
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
        let mut transaction = self.pool.begin().await.map_err(internal)?;
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
            };
            sqlx::query(
                "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, config_json, position) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(activity_id.as_str())
            .bind(lesson_id.as_str())
            .bind(activity.kind.as_str())
            .bind(activity.prompt.trim())
            .bind(serde_json::to_string(&config).map_err(internal)?)
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

/// 直接后继的渲染上限：超过时列前 10 个并注明总数。
const GRAPH_SUCCESSOR_RENDER_LIMIT: usize = 10;

/// 前置路径渲染：按拓扑序全局编号；超出上限时只渲染离节点最近的一段，
/// 更早的合并成概括行。节点内容是共享资产，不标注任何用户进度。
fn render_prerequisite_path(path: &[(i64, &str)]) -> String {
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
    for (offset, (_, title)) in path[start..].iter().enumerate() {
        lines.push(format!("{}. {}", start + offset + 1, title.trim()));
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

/// Shared payload validation for course activities and custom questions so
/// `evaluate` keeps working for both. Returns the trimmed prompt and the
/// persisted config.
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
    let config = match kind {
        ActivityKind::SingleChoice => {
            let options: Vec<String> = options
                .iter()
                .map(|option| option.trim().to_string())
                .filter(|option| !option.is_empty())
                .collect();
            let unique = options.iter().collect::<std::collections::HashSet<_>>();
            if unique.len() != options.len() || options.len() < 2 {
                return Err(AppError::BadRequest(
                    "single choice questions need at least two unique options".into(),
                ));
            }
            let Some(answer_text) = answer.as_str() else {
                return Err(AppError::BadRequest(
                    "single choice answer must be a string".into(),
                ));
            };
            if !options.iter().any(|option| option == answer_text) {
                return Err(AppError::BadRequest(
                    "single choice answer must be one of the options".into(),
                ));
            }
            StoredActivityConfig {
                options,
                answer: answer.clone(),
                explanation: explanation.to_string(),
                distractors: Vec::new(),
            }
        }
        ActivityKind::TrueFalse => {
            let Some(answer_bool) = answer.as_bool() else {
                return Err(AppError::BadRequest(
                    "true/false answer must be a boolean".into(),
                ));
            };
            StoredActivityConfig {
                options: Vec::new(),
                answer: Value::Bool(answer_bool),
                explanation: explanation.to_string(),
                distractors: Vec::new(),
            }
        }
        ActivityKind::Reflection => StoredActivityConfig {
            options: Vec::new(),
            answer: Value::Null,
            explanation: explanation.to_string(),
            distractors: Vec::new(),
        },
        ActivityKind::FillInBlank => {
            if !prompt.contains("___") {
                return Err(AppError::BadRequest(
                    "fill_in_blank prompt must contain a ___ blank".into(),
                ));
            }
            let Some(answers) = answer.as_array() else {
                return Err(AppError::BadRequest(
                    "fill_in_blank answer must be a JSON array of accepted answers".into(),
                ));
            };
            if answers.is_empty() || answers.len() > 3 {
                return Err(AppError::BadRequest(
                    "fill_in_blank must have 1-3 accepted answers".into(),
                ));
            }
            if answers.iter().any(|accepted| {
                !accepted.as_str().is_some_and(|text| !text.trim().is_empty())
            }) {
                return Err(AppError::BadRequest(
                    "fill_in_blank accepted answers must be non-empty strings".into(),
                ));
            }
            let distractors: Vec<String> = distractors
                .iter()
                .map(|distractor| distractor.trim().to_string())
                .filter(|distractor| !distractor.is_empty())
                .collect();
            StoredActivityConfig {
                options: Vec::new(),
                answer: answer.clone(),
                explanation: explanation.to_string(),
                distractors,
            }
        }
    };
    Ok((prompt.to_string(), config))
}
