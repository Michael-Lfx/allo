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
            // 大纲已无概念契约（ADR-0009）：传统课时没有防超纲黑名单。
            forbidden_concepts: String::new(),
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
                        "visuals": crate::models::visual_distribution(&output.sections),
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
                            "visuals": crate::models::visual_distribution(&output.sections),
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
        self.persist_lesson_output(lesson_id, &output).await?;

        let enrollment = self.enrollment_id_for(user_id, &course_id).await?;
        self.lesson_view(lesson_id, enrollment.as_ref()).await
    }

    /// 学习图课程节点的内容生成（beta）。学习图课程没有蓝图快照：上下文
    /// 来自课程行（学习目标/学习范围）与概念网（本课 assumes 的概念由哪些
    /// 节点跨课程教授 + 下游禁止清单，ADR-0009）。只走注入的 agent 引擎
    /// ——fallback 一次性管线消费 `&Blueprint`，对图节点不可复用。
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

        // 一次载入概念网口径的拓扑上下文（前置 = assumes 的概念与跨课程
        // 教它的节点；禁止清单 = 下游节点标题）。
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
            excerpt: None,
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: Some(GraphLessonContext {
                goal,
                scope,
                prerequisite_path,
                upcoming_nodes,
            }),
            // 防超纲黑名单：下游节点标题清单（概念网反查，ADR-0009）。
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
                "visuals": crate::models::visual_distribution(&output.sections),
            })),
            Err(error) => self.emit_lesson_event(serde_json::json!({
                "phase": "failed",
                "lesson_id": lesson_id.as_str(),
                "title": context.lesson_title,
                "error": error.to_string(),
            })),
        }
        let output = result?;
        self.persist_lesson_output(lesson_id, &output).await?;

        let enrollment = self.enrollment_id_for(user_id, course_id).await?;
        self.lesson_view(lesson_id, enrollment.as_ref()).await
    }

    /// 学习图节点的拓扑上下文（节点内容生成与单节重写共用），概念网口径
    /// （ADR-0009，无先修边）：
    /// - 前置段 = 本课每条 assumes 的概念（登记表 canonical + definition）
    ///   + 跨课程教该概念的节点摘要。摘要有两层：节点出生即带的
    ///   标题（动作句）+ purpose（教练批注，不依赖正文生成），以及已生成
    ///   节的标题要点（learnhub contextPack §2 的「前置摘要」）；档位本身
    ///   就是停讲深度的标尺（知道=辨认复述/会用=解题应用/能教=讲解纠错），
    ///   三者合起来让作者模型在教师节点还没有正文时也知道「教到哪了」。
    /// - 后续段 = 同课程位于本课之后的节点（按 position，衔接用）；
    /// - 禁止清单 = assumes 本课 teaches 概念的下游课程节点（标题+purpose，
    ///   防超纲）；
    /// - 末位 = 本课程学习节点总数（终点标记课时除外）。
    async fn graph_node_topology(
        &self,
        course_id: &LearningCourseId,
        lesson_id: &LearningLessonId,
    ) -> Result<(String, String, String, usize), AppError> {
        // 1) 本课声明的概念（teaches/assumes，登记表 canonical + 档位 +
        //    definition——definition 随铸名落库，是零正文场景下的概念摘要）。
        let declares: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT lc.role, reg.canonical, lc.tier, reg.definition \
             FROM learning_lesson_concepts lc \
             JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
             WHERE lc.lesson_id = ? ORDER BY reg.canonical",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut teaches: Vec<String> = Vec::new();
        let mut assumes: Vec<(String, String, String)> = Vec::new();
        for (role, canonical, tier, definition) in declares {
            match role.as_str() {
                "teaches" => teaches.push(canonical),
                _ => assumes.push((canonical, tier, definition)),
            }
        }

        // 2) 前置：每条 assumes 的概念由哪些节点（跨课程）教。
        //    concept canonical → [(lesson_id, 节点标题, 课程标题, purpose)]。
        let mut teachers: HashMap<String, Vec<(String, String, String, String)>> = HashMap::new();
        if !assumes.is_empty() {
            let mut query = sqlx::QueryBuilder::new(
                "SELECT reg.canonical, l.lesson_id, l.title, c.title AS course_title, l.purpose \
                 FROM learning_lesson_concepts lc \
                 JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
                 JOIN learning_modules m ON m.module_id = l.module_id \
                 JOIN learning_courses c ON c.course_id = m.course_id \
                 JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
                 WHERE lc.role = 'teaches' AND l.lesson_id <> ? AND reg.canonical IN (",
            );
            let mut separated = query.separated(", ");
            separated.push_bind(lesson_id.as_str());
            for (concept, _, _) in &assumes {
                separated.push_bind(concept);
            }
            query.push(") ORDER BY reg.canonical, c.title, l.title");
            let rows = query.build().fetch_all(&self.pool).await.map_err(internal)?;
            for row in rows {
                let concept: String = row.try_get("canonical").map_err(internal)?;
                let node_id: String = row.try_get("lesson_id").map_err(internal)?;
                let title: String = row.try_get("title").map_err(internal)?;
                let course_title: String = row.try_get("course_title").map_err(internal)?;
                let purpose: String = row.try_get("purpose").map_err(internal)?;
                teachers
                    .entry(concept)
                    .or_default()
                    .push((node_id, title, course_title, purpose));
            }
        }
        // 已生成前置节附「实际教过的节标题+要点」摘要（learnhub contextPack
        // §2「前置摘要」）：只有名字时模型不知道前置具体教了什么，重讲一遍
        // 是最高频的失败模式。
        let teacher_ids: Vec<String> = teachers
            .values()
            .flatten()
            .map(|(id, _, _, _)| id.clone())
            .collect();
        let taught = self.taught_sections_for_lessons(&teacher_ids).await?;
        let prerequisite_path = render_prerequisite_path(&assumes, &teachers, &taught);

        // 3) 后续：同课程位于本课之后的节点（终点标记课时除外），按
        //    position 升序——衔接参照，不是依赖关系。
        let upcoming_rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT l.position, l.title FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? \
               AND l.position > (SELECT position FROM learning_lessons WHERE lesson_id = ?) \
               AND l.lesson_id NOT IN \
                 (SELECT lesson_id FROM learning_course_endpoints WHERE course_id = ?) \
             ORDER BY l.position, l.lesson_id",
        )
        .bind(course_id.as_str())
        .bind(lesson_id.as_str())
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let upcoming_nodes = render_upcoming_nodes(&upcoming_rows);

        // 4) 禁止清单：assumes 本课 teaches 概念的下游课程节点（防超纲黑
        //    名单，ADR-0009 概念网变体；purpose 帮作者知道下游要拿这些概念
        //    去做什么，从而知道哪些铺垫该留给下游）。
        let mut forbidden_concepts = String::new();
        if !teaches.is_empty() {
            let mut query = sqlx::QueryBuilder::new(
                "SELECT DISTINCT c.title AS course_title, l.title, l.purpose \
                 FROM learning_lesson_concepts lc \
                 JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
                 JOIN learning_modules m ON m.module_id = l.module_id \
                 JOIN learning_courses c ON c.course_id = m.course_id \
                 JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
                 WHERE lc.role = 'assumes' AND l.lesson_id <> ? AND reg.canonical IN (",
            );
            let mut separated = query.separated(", ");
            separated.push_bind(lesson_id.as_str());
            for concept in &teaches {
                separated.push_bind(concept);
            }
            query.push(") ORDER BY c.title, l.title");
            let rows = query.build().fetch_all(&self.pool).await.map_err(internal)?;
            let mut descendants: Vec<(String, String, String)> = Vec::new();
            for row in rows {
                let course_title: String = row.try_get("course_title").map_err(internal)?;
                let title: String = row.try_get("title").map_err(internal)?;
                let purpose: String = row.try_get("purpose").map_err(internal)?;
                descendants.push((course_title, title, purpose));
            }
            forbidden_concepts = render_forbidden_descendants(&descendants);
        }

        // 5) 本课程学习节点总数（终点标记课时不是学习节点）。
        let total_nodes: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? AND l.lesson_id NOT IN \
               (SELECT lesson_id FROM learning_course_endpoints WHERE course_id = ?)",
        )
        .bind(course_id.as_str())
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;

        Ok((prerequisite_path, upcoming_nodes, forbidden_concepts, total_nodes.max(0) as usize))
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
            request.feedback.as_deref(),
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
        self.persist_rewritten_section(lesson_id, section_key, &body, degraded)
            .await?;

        let enrollment = self.enrollment_id_for(user_id, &course_id).await?;
        self.lesson_view(lesson_id, enrollment.as_ref()).await
    }

    /// 手动编辑节正文（ADR-0007）：仅覆盖 `body_md`（version+1，status 保持
    /// 'ready'，summary 由全部节重新拼装），不记录编辑来源。练习节不开放
    /// （题目是一等实体，编辑其指令性正文意义有限）；failed 节没有正文，
    /// 走既有 AI 重写路径而非手写。
    pub async fn update_lesson_section_body(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        section_key: &str,
        request: &crate::models::UpdateLessonSectionBodyRequest,
    ) -> Result<LessonView, AppError> {
        let row = sqlx::query(
            "SELECT l.content_generated, m.course_id FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;
        let course_id: LearningCourseId = parse_id(row.try_get("course_id").map_err(internal)?)?;
        if row.try_get::<i64, _>("content_generated").map_err(internal)? == 0 {
            return Err(AppError::Conflict(
                "lesson content has not been generated yet".into(),
            ));
        }

        let sections = self.lesson_sections(lesson_id).await?;
        let current = sections
            .iter()
            .find(|section| section.section_key == section_key)
            .ok_or_else(|| {
                AppError::NotFound(format!("section {section_key} in lesson {lesson_id}"))
            })?;
        if current.kind == crate::models::SectionKind::Practice {
            return Err(AppError::Conflict(
                "practice sections cannot be edited manually".into(),
            ));
        }
        if current.body_md.trim().is_empty() {
            return Err(AppError::Conflict(
                "section has no body yet; use the rewrite path instead".into(),
            ));
        }
        if request.body_md.trim().is_empty() {
            return Err(AppError::UnprocessableEntity(
                "body_md must not be empty".into(),
            ));
        }

        // 手动编辑视为接管：降级兜底标记随人工正文清除。
        self.persist_rewritten_section(lesson_id, section_key, &request.body_md, false)
            .await?;

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
        Ok((String::new(), excerpt))
    }

    /// 单节落库：正文原地替换（version+1），summary 由全部节重新拼装
    /// （双读回退的文本同步）。`degraded` 标记当前正文是否为降级纯文字
    /// 兜底（ADR-0008）——AI 重写降级时置位，兑现承诺的重写与手动编辑
    /// 清除。单事务——节更新与 summary 拼装同生共死。
    async fn persist_rewritten_section(
        &self,
        lesson_id: &LearningLessonId,
        section_key: &str,
        body: &str,
        degraded: bool,
    ) -> Result<(), AppError> {
        let now = now_ms();
        let body = crate::generation::fix_mermaid_quotes(body.trim());
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let updated = sqlx::query(
            "UPDATE learning_lesson_sections \
             SET body_md = ?, degraded = ?, status = 'ready', version = version + 1, updated_at = ? \
             WHERE lesson_id = ? AND section_key = ?",
        )
        .bind(&body)
        .bind(degraded)
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
        // learning_lessons 没有 updated_at 列（迁移 015/040 均未引入），
        // 只同步 summary 本身。
        sqlx::query("UPDATE learning_lessons SET summary = ? WHERE lesson_id = ?")
            .bind(&summary)
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
    /// 生成标记）并替换全部活动。概念绑定随 per-course 概念体系整体下线
    /// （ADR-0009），活动不再携带 concept 绑定。
    async fn persist_lesson_output(
        &self,
        lesson_id: &LearningLessonId,
        output: &LessonOutput,
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
                let degraded = output
                    .degraded_keys
                    .iter()
                    .any(|key| *key == section.section_key);
                sqlx::query(
                    "INSERT INTO learning_lesson_sections \
                     (section_key, lesson_id, kind, title, points, visual, body_md, degraded, status, version, position, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'ready', 1, ?, ?, ?)",
                )
                .bind(section.section_key.trim())
                .bind(lesson_id.as_str())
                .bind(section.kind.as_str())
                .bind(section.title.trim())
                .bind(section.points.trim())
                .bind(section.visual.trim())
                .bind(section.body_md.trim())
                .bind(degraded)
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
        }
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

    /// Manually appends an activity to a generated lesson. The lesson must
    /// belong to a course the learner is enrolled in (the enrollment is
    /// created on demand like every other practice flow). When the lesson is
    /// already completed, an objective question is also admitted to the
    /// review queue immediately via the idempotent seeder.
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
    /// learner-chosen kind, grounded in the finished lesson document and its
    /// cited excerpt — with every existing question listed so the model must
    /// cover new ground. The draft is returned for preview and nothing is
    /// persisted.
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

        // 摘录来自大纲快照（kb 流）；没有快照的课程（教程课）摘录为空。
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
        let excerpt = match (blueprint_json, samples_json) {
            (Some(blueprint_json), Some(samples_json)) => {
                let blueprint: Blueprint = serde_json::from_str(&blueprint_json).map_err(internal)?;
                let samples: Vec<(String, String)> =
                    serde_json::from_str(&samples_json).map_err(internal)?;
                let module_position: i64 = row.try_get("module_position").map_err(internal)?;
                let lesson_position: i64 = row.try_get("position").map_err(internal)?;
                blueprint
                    .modules
                    .get(module_position as usize)
                    .and_then(|module| module.lessons.get(lesson_position as usize))
                    .and_then(|lesson| lesson.source.as_ref())
                    .and_then(|source| {
                        samples
                            .iter()
                            .find(|(path, _)| path == &source.path)
                            .map(|(_, excerpt)| excerpt.clone())
                    })
                    .unwrap_or_default()
            }
            _ => String::new(),
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
            &summary,
            &excerpt,
            &existing_questions,
        )
        .await
        .map_err(|error| {
            AppError::UnprocessableEntity(format!("failed to generate lesson activity: {error}"))
        })?;

        Ok(GeneratedLessonActivity {
            kind: activity.kind,
            prompt: activity.prompt,
            options: activity.options,
            answer: activity.answer,
            explanation: activity.explanation,
            distractors: activity.distractors,
        })
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

/// 单个概念在教师节点清单里最多列出的节点数（概念网跨课程，防止清单爆炸）。
const GRAPH_TEACHERS_PER_CONCEPT: usize = 4;

/// 单个前置课时在摘要里最多列出的已教节数（节清单硬上限 8，取齐即可）。
const GRAPH_TAUGHT_SECTION_LIMIT: usize = 8;

/// 后续节点的渲染上限：超过时列前 10 个并注明总数。
const GRAPH_SUCCESSOR_RENDER_LIMIT: usize = 10;

/// 下游节点禁止清单的条目上限（learnhub 黑名单同款截断）。
const GRAPH_FORBIDDEN_LIMIT: usize = 200;

/// 前置段渲染（概念网口径，ADR-0009）：开头一条档位契约（概念已教到标注
/// 档位——不要重新讲授，只在需要时按该档位复述或引用；档位即停讲深度的
/// 标尺），逐条 assumes 概念列出「概念 @档位」+ 登记表 definition + 跨课程
/// 教它的节点（标题/课程 + 出生即带的 purpose + 已生成节的标题要点摘要）。
/// 零正文场景（教师节点尚未生成内容）不削弱前置段：purpose 与 definition
/// 都是出生即有的字段。没有任何节点教的概念按未覆盖兜底提示（正常情况下
/// 结构门已挡掉这种批次）。空串 = 本课零 assumes（第一批基节点）。
fn render_prerequisite_path(
    assumes: &[(String, String, String)],
    teachers: &HashMap<String, Vec<(String, String, String, String)>>,
    taught: &HashMap<String, Vec<(String, String)>>,
) -> String {
    if assumes.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "前置契约：以下概念已由其他节点教到标注档位，不要重新讲授，只在必要时按该档位复述或引用。".to_owned(),
        "（档位标尺：知道=能辨认复述，会用=能解题应用，能教=能讲解纠错——正文深度不得超过标注档位。）".to_owned(),
    ];
    for (concept, tier, definition) in assumes {
        let tier = crate::learning_graph::ConceptTier::try_from_str(tier)
            .map(crate::learning_graph::tier_zh)
            .unwrap_or("?");
        match teachers.get(concept) {
            None => lines.push(format!(
                "- {concept}（应已掌握到「{tier}」，但暂无节点教它——不要展开讲授）"
            )),
            Some(nodes) => {
                lines.push(format!("- {concept}（学习者应已掌握到「{tier}」）："));
                let definition = definition.trim();
                if !definition.is_empty() {
                    lines.push(format!("   定义：{definition}"));
                }
                for (node_id, title, course_title, purpose) in
                    nodes.iter().take(GRAPH_TEACHERS_PER_CONCEPT)
                {
                    let course = if course_title.trim().is_empty() {
                        String::new()
                    } else {
                        format!("，课程「{}」", course_title.trim())
                    };
                    lines.push(format!("   · {}{course}", title.trim()));
                    let purpose = purpose.trim();
                    if !purpose.is_empty() {
                        lines.push(format!("     本节练什么：{purpose}"));
                    }
                    if let Some(sections) = taught.get(node_id) {
                        for (section_title, points) in
                            sections.iter().take(GRAPH_TAUGHT_SECTION_LIMIT)
                        {
                            let points = points.trim();
                            let summary = if points.is_empty() {
                                section_title.trim().to_owned()
                            } else {
                                format!("{}：{points}", section_title.trim())
                            };
                            lines.push(format!("     · {summary}"));
                        }
                    }
                }
            }
        }
    }
    lines.join("\n")
}

/// 后续节点渲染：同课程位于本课之后的节点按 position 升序列出，超出上限
/// 截断并注明总数——衔接参照（标题即衔接方向），不是依赖关系。
fn render_upcoming_nodes(upcoming: &[(i64, String)]) -> String {
    if upcoming.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    for (_, title) in upcoming.iter().take(GRAPH_SUCCESSOR_RENDER_LIMIT) {
        lines.push(format!("- {}", title.trim()));
    }
    if upcoming.len() > GRAPH_SUCCESSOR_RENDER_LIMIT {
        lines.push(format!("……等共 {} 个后续节点", upcoming.len()));
    }
    lines.join("\n")
}

/// 下游节点禁止清单渲染（防超纲黑名单，ADR-0009 概念网变体）：正文不得
/// 出现这些名称、不得引用其结论。条目为（课程标题, 节点标题, purpose）——
/// purpose 让作者知道下游要拿本课概念去做什么，哪些铺垫该留给下游；超出
/// 上限截断并注明总数；空集返回空串（本课 teaches 的概念没有下游假定）。
fn render_forbidden_descendants(descendants: &[(String, String, String)]) -> String {
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
        .map(|(course_title, title, purpose)| {
            let course = if course_title.trim().is_empty() {
                String::new()
            } else {
                format!("（{}）", course_title.trim())
            };
            let purpose = purpose.trim();
            if purpose.is_empty() {
                format!("- {}{course}", title.trim())
            } else {
                format!("- {}{course}——{purpose}", title.trim())
            }
        })
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

    /// 前置段渲染（概念网口径，ADR-0009）：逐条 assumes 列「概念（应已掌
    /// 握到「档位」）」+ 跨课程教它的节点行（标题/课程）+ 已生成节的标题
    /// 要点摘要；没有任何节点教的概念按未覆盖兜底提示。
    #[test]
    fn prerequisite_path_carries_taught_section_summaries() {
        let assumes = vec![
            (
                "勾股定理".to_owned(),
                "apply".to_owned(),
                "直角三角形三边间的平方关系".to_owned(),
            ),
            ("相似三角形".to_owned(), "know".to_owned(), String::new()),
        ];
        let mut teachers: HashMap<String, Vec<(String, String, String, String)>> = HashMap::new();
        teachers.insert(
            "勾股定理".to_owned(),
            vec![(
                "lesson-b".to_owned(),
                "推导勾股定理".to_owned(),
                "几何基础".to_owned(),
                "从相似三角形出发证明 a²+b²=c²".to_owned(),
            )],
        );
        let mut taught: HashMap<String, Vec<(String, String)>> = HashMap::new();
        taught.insert(
            "lesson-b".to_owned(),
            vec![
                (
                    "概念：勾股定理".to_owned(),
                    "直角边形两直角边平方和等于斜边平方".to_owned(),
                ),
                ("例题：求斜边".to_owned(), "已知两直角边求斜边".to_owned()),
            ],
        );
        let text = render_prerequisite_path(&assumes, &teachers, &taught);
        // 档位契约头：不要重新讲授 + 档位即停讲深度标尺。
        assert!(text.contains("不要重新讲授"), "{text}");
        assert!(text.contains("能教=能讲解纠错"), "{text}");
        assert!(
            text.contains("勾股定理（学习者应已掌握到「会用」）"),
            "{text}"
        );
        // 登记表 definition 随铸名落库，是零正文场景的概念摘要。
        assert!(text.contains("定义：直角三角形三边间的平方关系"), "{text}");
        assert!(text.contains("推导勾股定理"), "{text}");
        assert!(text.contains("课程「几何基础」"), "{text}");
        // purpose 出生即有：教师节点没有正文也有「练什么」。
        assert!(text.contains("本节练什么：从相似三角形出发证明"), "{text}");
        assert!(
            text.contains("概念：勾股定理：直角边形两直角边平方和等于斜边平方"),
            "{text}"
        );
        assert!(text.contains("例题：求斜边：已知两直角边求斜边"), "{text}");
        // 无节点教的概念：兜底提示，不展开讲授。
        assert!(text.contains("相似三角形"), "{text}");
        assert!(text.contains("暂无节点教它"), "{text}");
    }

    /// 教师节点零正文（跨课程首学期常态）时前置段不塌缩：definition +
    /// purpose + 档位契约独立于已生成节摘要成立（learnhub 约束强化口径）。
    #[test]
    fn prerequisite_path_survives_ungenerated_teachers() {
        let assumes = vec![(
            "因式分解".to_owned(),
            "apply".to_owned(),
            "把多项式化成几个整式乘积".to_owned(),
        )];
        let mut teachers: HashMap<String, Vec<(String, String, String, String)>> = HashMap::new();
        teachers.insert(
            "因式分解".to_owned(),
            vec![(
                "lesson-a".to_owned(),
                "用提公因式法化简多项式".to_owned(),
                String::new(),
                "练会观察公因式并提取".to_owned(),
            )],
        );
        let text = render_prerequisite_path(&assumes, &teachers, &HashMap::new());
        assert!(text.contains("不要重新讲授"), "{text}");
        assert!(text.contains("定义：把多项式化成几个整式乘积"), "{text}");
        assert!(text.contains("本节练什么：练会观察公因式并提取"), "{text}");
        // 零 purpose 的教师也至少有标题行。
        let mut teachers = teachers;
        teachers.insert(
            "因式分解".to_owned(),
            vec![(
                "lesson-a".to_owned(),
                "用提公因式法化简多项式".to_owned(),
                String::new(),
                String::new(),
            )],
        );
        let text = render_prerequisite_path(&assumes, &teachers, &HashMap::new());
        assert!(text.contains("用提公因式法化简多项式"), "{text}");
        assert!(!text.contains("本节练什么"), "{text}");
    }

    /// 零 assumes（第一批基节点）不渲染前置段。
    #[test]
    fn prerequisite_path_empty_without_assumes() {
        let text = render_prerequisite_path(&[], &HashMap::new(), &HashMap::new());
        assert!(text.is_empty());
    }

    /// 下游禁止清单：非空时附「不得出现/不得引用」约束与总数，purpose 让
    /// 作者知道下游拿概念做什么；空集（终点节点）返回空串。
    #[test]
    fn forbidden_descendants_render_the_blacklist_contract() {
        assert_eq!(render_forbidden_descendants(&[]), "");
        let titles: Vec<String> = (0..(GRAPH_FORBIDDEN_LIMIT + 5))
            .map(|i| format!("下游单元{i}"))
            .collect();
        let descendants: Vec<(String, String, String)> = titles
            .iter()
            .map(|title| (String::from("几何基础"), title.clone(), String::new()))
            .collect();
        let text = render_forbidden_descendants(&descendants);
        assert!(text.contains(&format!("共 {} 个", descendants.len())), "{text}");
        assert!(text.contains("- 下游单元0（几何基础）"), "{text}");
        // 超限截断：只列前 200 条。
        assert!(!text.contains("- 下游单元200"), "{text}");
        assert!(text.contains(format!("- 下游单元{}", GRAPH_FORBIDDEN_LIMIT - 1).as_str()));
        // 带 purpose 的条目把「下游要做什么」写进黑名单。
        let text = render_forbidden_descendants(&[(
            "几何基础".to_owned(),
            "应用勾股定理解题".to_owned(),
            "练会已知两边求第三边".to_owned(),
        )]);
        assert!(text.contains("——练会已知两边求第三边"), "{text}");
    }
}
