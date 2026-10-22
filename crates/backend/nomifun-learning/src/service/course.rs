use super::*;

impl LearningService {

    pub async fn import_course(&self, pack: CoursePack) -> Result<CourseDetail, AppError> {
        self.import_course_with_context(pack, None).await
    }

    /// Import an on-demand course skeleton: modules, lessons (title + purpose +
    /// source + concepts, no body yet), concepts, plus the persisted outline
    /// snapshot (blueprint + samples + goal + sampling disclosure) that
    /// deferred lesson generation will use.
    pub(crate) async fn import_course_outline(
        &self,
        pack: CoursePack,
        snapshot: OutlineSnapshot,
    ) -> Result<CourseDetail, AppError> {
        self.import_course_with_context(pack, Some(snapshot)).await
    }

    async fn import_course_with_context(
        &self,
        pack: CoursePack,
        outline: Option<OutlineSnapshot>,
    ) -> Result<CourseDetail, AppError> {
        validate_pack(&pack)?;
        // 来源库列表（t4）：outline 快照带归一后的完整列表（首个 = 主来源），
        // 非 outline 的导入（教程播种/旧调用）回退单列的单元素视图。逐库核验
        // 存在——建课不接受指向不存在库的课程。
        let source_kb_ids: Vec<KnowledgeBaseId> = outline
            .as_ref()
            .map(|snapshot| snapshot.source_kb_ids.clone())
            .filter(|ids| !ids.is_empty())
            .unwrap_or_else(|| pack.source_kb_id.iter().cloned().collect());
        for kb_id in &source_kb_ids {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_bases WHERE knowledge_base_id = ?")
                    .bind(kb_id.as_str())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(internal)?;
            if exists == 0 {
                return Err(AppError::BadRequest(format!(
                    "knowledge base {kb_id} does not exist"
                )));
            }
        }
        let primary_kb_id = source_kb_ids.first().cloned();
        let source_kb_ids_json = serde_json::to_string(
            &source_kb_ids
                .iter()
                .map(KnowledgeBaseId::as_str)
                .collect::<Vec<_>>(),
        )
        .map_err(|error| AppError::Internal(format!("failed to serialize source kb ids: {error}")))?;

        let is_outline = outline.is_some();
        let (blueprint_json, samples_json, learning_goal, samples_meta_json, course_source_plan_json) =
            outline
                .as_ref()
                .map(|snapshot| {
                    (
                        Some(snapshot.blueprint_json.as_str()),
                        Some(snapshot.samples_json.as_str()),
                        Some(snapshot.learning_goal.as_str()),
                        Some(snapshot.samples_meta_json.as_deref()),
                        snapshot.course_source_plan_json.as_deref(),
                    )
                })
                .unwrap_or((None, None, None, None, None));
        let content_generated: i64 = if is_outline { 0 } else { 1 };
        let now = now_ms();
        let course_id = LearningCourseId::new();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        // 来源计划快照（t4）与蓝图/采样同 INSERT:同一事务,导入中途失败
        // (含 069 CHECK 拒绝非对象 JSON)零残留。NULL = 旧课程/描述流/未走
        // 新管线,不回填。
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, source_kb_id, source_kb_ids, version, blueprint_json, samples_json, \
              samples_meta_json, learning_goal, teaching_style, course_source_plan_json, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(course_id.as_str())
        .bind(pack.title.trim())
        .bind(pack.description.trim())
        .bind(pack.domain.trim())
        .bind(primary_kb_id.as_ref().map(KnowledgeBaseId::as_str))
        .bind(&source_kb_ids_json)
        .bind(pack.version)
        .bind(blueprint_json)
        .bind(samples_json)
        .bind(samples_meta_json.flatten())
        .bind(learning_goal.unwrap_or_default())
        .bind(pack.teaching_style.as_str())
        .bind(course_source_plan_json)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;

        for (module_position, module) in pack.modules.iter().enumerate() {
            let module_id = LearningModuleId::new();
            sqlx::query(
                "INSERT INTO learning_modules \
                 (module_id, course_id, title, description, position) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(module_id.as_str())
            .bind(course_id.as_str())
            .bind(module.title.trim())
            .bind(module.description.trim())
            .bind(module_position as i64)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;

            for (lesson_position, lesson) in module.lessons.iter().enumerate() {
                let lesson_id = LearningLessonId::new();
                // 引用清单（t4 评审补齐的多引用）：首个 = 主来源，三列
                //（source_path/start/end）是它的兼容位，两者同源写入。
                let cited = lesson.cited_sources();
                let (source_path, source_start, source_end) = cited
                    .first()
                    .map(|source| (Some(source.path.trim()), source.start, source.end))
                    .unwrap_or((None, None, None));
                let source_paths_json = serde_json::to_string(
                    &cited
                        .iter()
                        .map(|source| {
                            serde_json::json!({
                                "path": source.path.trim(),
                                "start": source.start,
                                "end": source.end,
                            })
                        })
                        .collect::<Vec<_>>(),
                )
                .map_err(internal)?;
                sqlx::query(
                    "INSERT INTO learning_lessons \
                     (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
                      content_generated, source_path, source_start, source_end, source_paths_json) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(lesson_id.as_str())
                .bind(module_id.as_str())
                .bind(lesson.title.trim())
                .bind(lesson.summary.trim())
                .bind(lesson.purpose.trim())
                .bind(lesson_position as i64)
                .bind(lesson.est)
                .bind(content_generated)
                .bind(source_path)
                .bind(source_start)
                .bind(source_end)
                .bind(source_paths_json)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;

                for (activity_position, activity) in lesson.activities.iter().enumerate() {
                    let activity_id = LearningActivityId::new();
                    let config = StoredActivityConfig {
                        options: activity.options.clone(),
                        answer: activity.answer.clone(),
                        explanation: activity.explanation.clone(),
                        distractors: activity.distractors.clone(),
                        tol: activity.tol,
                        matches: super::lesson::matching_candidates(activity),
                        difficulty: activity.difficulty,
                        evidence: activity
                            .evidence
                            .as_deref()
                            .map(str::trim)
                            .filter(|evidence| !evidence.is_empty())
                            .map(str::to_owned),
                    };
                    sqlx::query(
                        "INSERT INTO learning_activities \
                         (activity_id, lesson_id, kind, prompt, config_json, position) \
                         VALUES (?, ?, ?, ?, ?, ?)",
                    )
                    .bind(activity_id.as_str())
                    .bind(lesson_id.as_str())
                    .bind(activity.kind.as_str())
                    .bind(activity.prompt.trim())
                    .bind(serde_json::to_string(&config).map_err(internal)?)
                    .bind(activity_position as i64)
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
                }
            }
        }

        transaction.commit().await.map_err(internal)?;
        self.course_detail(&course_id, None).await
    }

    /// 现存知识库 id 集合（t4 读侧过滤的唯一事实源）：来源库列表 / 课时
    /// 归属按它剔除已删库 id——单列 `source_kb_id` 的 SetNull 引用语义在
    /// 多库列表下由应用层读侧承担（不外泄悬空 id）。
    pub(super) async fn existing_knowledge_base_ids(&self) -> Result<HashSet<String>, AppError> {
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT knowledge_base_id FROM knowledge_bases")
                .fetch_all(&self.pool)
                .await
                .map_err(internal)?;
        Ok(ids.into_iter().collect())
    }

    pub async fn list_courses(&self, user_id: &UserId) -> Result<Vec<CourseSummary>, AppError> {
        // 目录列出现存课程（归档课也在列——带徽标可恢复）；安排面收窄由
        // 各消费面按「现存且启用」口径自筛（三轮 g3），目录本身不藏归档课。
        let rows = sqlx::query(
            "SELECT c.course_id, c.title, c.description, c.domain, c.course_kind, c.source_kb_id, c.source_kb_ids, c.version, c.updated_at, \
                    c.learning_goal, c.samples_meta_json, c.enabled, c.course_source_plan_json, \
                    EXISTS(SELECT 1 FROM learning_enrollments e \
                           WHERE e.course_id = c.course_id AND e.user_id = ?) AS enrolled, \
                    (SELECT COUNT(*) FROM learning_lessons l \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE m.course_id = c.course_id) AS total_lessons, \
                    (SELECT COUNT(*) FROM learning_lesson_progress p \
                     JOIN learning_enrollments e ON e.enrollment_id = p.enrollment_id \
                     JOIN learning_lessons l ON l.lesson_id = p.lesson_id \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE e.user_id = ? AND m.course_id = c.course_id AND p.status = 'completed') AS completed_lessons, \
                    (SELECT COUNT(DISTINCT x.node_id) FROM learning_xp_events x \
                     WHERE x.course_id = c.course_id AND x.user_id = ? \
                       AND x.kind = 'node-completed' AND x.node_id IS NOT NULL) AS settled_nodes, \
                    (SELECT COALESCE(SUM(x.earned_xp), 0) FROM learning_xp_events x \
                     WHERE x.course_id = c.course_id AND x.user_id = ?) AS total_xp, \
                    (SELECT COUNT(*) FROM learning_review_items r \
                     JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
                     WHERE e.user_id = ? AND e.course_id = c.course_id \
                       AND r.archived_at IS NULL AND r.edit_pending_at IS NULL \
                       AND r.due_at <= ?) AS due_today \
             FROM learning_courses c WHERE c.deleted_at IS NULL \
             ORDER BY c.updated_at DESC, c.course_id",
        )
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .bind(now_ms())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let existing = self.existing_knowledge_base_ids().await?;
        let mut courses = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut course = course_summary_from_row(row)?;
            course.apply_existing_kb_ids(&existing);
            course.tags = self.course_tags(&course.id).await?;
            courses.push(course);
        }
        Ok(courses)
    }

    pub async fn course_detail(
        &self,
        course_id: &LearningCourseId,
        user_id: Option<&UserId>,
    ) -> Result<CourseDetail, AppError> {
        let user_value = user_id.map(UserId::as_str);
        let row = sqlx::query(
            "SELECT c.course_id, c.title, c.description, c.domain, c.course_kind, c.source_kb_id, c.source_kb_ids, c.version, c.updated_at, \
                    c.learning_goal, c.samples_meta_json, c.enabled, c.course_source_plan_json, \
                    EXISTS(SELECT 1 FROM learning_enrollments e \
                           WHERE e.course_id = c.course_id AND e.user_id = ?) AS enrolled, \
                    (SELECT COUNT(*) FROM learning_lessons l \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE m.course_id = c.course_id) AS total_lessons, \
                    (SELECT COUNT(*) FROM learning_lesson_progress p \
                     JOIN learning_enrollments e ON e.enrollment_id = p.enrollment_id \
                     JOIN learning_lessons l ON l.lesson_id = p.lesson_id \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE e.user_id = ? AND m.course_id = c.course_id AND p.status = 'completed') AS completed_lessons, \
                    (SELECT COUNT(DISTINCT x.node_id) FROM learning_xp_events x \
                     WHERE x.course_id = c.course_id AND x.user_id = ? \
                       AND x.kind = 'node-completed' AND x.node_id IS NOT NULL) AS settled_nodes, \
                    (SELECT COALESCE(SUM(x.earned_xp), 0) FROM learning_xp_events x \
                     WHERE x.course_id = c.course_id AND x.user_id = ?) AS total_xp, \
                    (SELECT COUNT(*) FROM learning_review_items r \
                     JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
                     WHERE e.user_id = ? AND e.course_id = c.course_id \
                       AND r.archived_at IS NULL AND r.edit_pending_at IS NULL \
                       AND r.due_at <= ?) AS due_today \
             FROM learning_courses c \
             WHERE c.course_id = ? AND c.deleted_at IS NULL",
        )
        .bind(user_value)
        .bind(user_value)
        .bind(user_value)
        .bind(user_value)
        .bind(user_value)
        .bind(now_ms())
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning course {course_id}")))?;
        let existing = self.existing_knowledge_base_ids().await?;
        // 来源库读侧材料（t4）：过滤已删库 id 后的列表 + 采样披露的逐库归属
        // ——课时 source 的 (kb_id, rel_path) 解析与「来源：库A、库B」共用。
        let kb_sources = CourseKbSources::from_row(&row)?.filtered(&existing);
        let course = course_summary_from_row(&row)?;
        let course = CourseSummary {
            source_kb_id: kb_sources.primary(),
            source_kb_ids: kb_sources.ids.clone(),
            tags: self.course_tags(&course.id).await?,
            ..course
        };

        // Opening a course detail is an implicit join: enrollment is created
        // on first view so every downstream practice flow has a grouping key.
        let enrollment_id = if let Some(user_id) = user_id {
            Some(self.ensure_enrollment(course_id, user_id).await?)
        } else {
            None
        };
        let enrollment_value = enrollment_id
            .as_ref()
            .map(LearningEnrollmentId::as_str);

        let module_rows = sqlx::query(
            "SELECT module_id, title, description, position FROM learning_modules \
             WHERE course_id = ? ORDER BY position, module_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut modules = Vec::with_capacity(module_rows.len());
        for module_row in module_rows {
            let module_id: LearningModuleId =
                parse_id(module_row.try_get("module_id").map_err(internal)?)?;
            let lesson_rows = sqlx::query(
                "SELECT l.lesson_id, l.title, l.summary, l.purpose, l.position, l.estimated_minutes, \
                        l.content_generated, l.source_path, l.source_start, l.source_end, l.source_paths_json, \
                        COALESCE(p.status, 'not_started') AS status \
                 FROM learning_lessons l \
                 LEFT JOIN learning_lesson_progress p \
                   ON p.lesson_id = l.lesson_id AND p.enrollment_id = ? \
                 WHERE l.module_id = ? ORDER BY l.position, l.lesson_id",
            )
            .bind(enrollment_value)
            .bind(module_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            let mut lessons = Vec::with_capacity(lesson_rows.len());
            for lesson_row in lesson_rows {
                let lesson_id: LearningLessonId =
                    parse_id(lesson_row.try_get("lesson_id").map_err(internal)?)?;
                let status_text: String = lesson_row.try_get("status").map_err(internal)?;
                let status = LessonStatus::try_from(status_text.as_str())
                    .map_err(AppError::Internal)?;
                let source_path: Option<String> =
                    lesson_row.try_get("source_path").map_err(internal)?;
                // 课时 source 的 (kb_id, 库内相对路径) 解析（t4）：多库路径按
                // 采样命名空间还原到来源库；单库/旧课程裸路径不解析。多引用
                //（t4 评审补齐）逐条解析，位序保留。
                let sources = kb_sources.resolve_sources(
                    lesson_row
                        .try_get::<Option<String>, _>("source_paths_json")
                        .map_err(internal)?
                        .as_deref(),
                    source_path,
                    lesson_row.try_get("source_start").ok().flatten(),
                    lesson_row.try_get("source_end").ok().flatten(),
                );
                let source = sources.first().cloned();
                let content_generated: i64 =
                    lesson_row.try_get("content_generated").map_err(internal)?;
                lessons.push(LessonView {
                    id: lesson_id.clone(),
                    title: lesson_row.try_get("title").map_err(internal)?,
                    summary: lesson_row.try_get("summary").map_err(internal)?,
                    purpose: lesson_row.try_get("purpose").map_err(internal)?,
                    position: lesson_row.try_get("position").map_err(internal)?,
                    est: lesson_row.try_get("estimated_minutes").map_err(internal)?,
                    generated: content_generated != 0,
                    source,
                    sources,
                    status,
                    activities: self.lesson_activities(&lesson_id).await?,
                    // 目录视图不加载节正文（体积大）；分节经课时详情接口
                    // 获取，目录双读 summary。
                    sections: Vec::new(),
                });
            }
            modules.push(ModuleView {
                id: module_id,
                title: module_row.try_get("title").map_err(internal)?,
                description: module_row.try_get("description").map_err(internal)?,
                position: module_row.try_get("position").map_err(internal)?,
                lessons,
            });
        }

        // 学习图课程：大纲被「下一步推荐节点」取代——next_lesson_id 不参与，
        // 换成图视图（就绪集推荐 + 水位读数）。传统课程路径零变化。
        let graph = if course.course_kind == CourseKind::LearningGraph {
            Some(
                self.assemble_learning_graph_view(course_id, user_id)
                    .await?,
            )
        } else {
            None
        };
        let next_lesson_id = if graph.is_some() {
            None
        } else {
            recommend_next_lesson(&modules)
        };
        let due_review_count = if let Some(enrollment_id) = &enrollment_id {
            // Items are seeded per objective question when the lesson is
            // completed, so every row already represents a due-able card.
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM learning_review_items r \
                 WHERE r.enrollment_id = ? AND r.due_at <= ? AND r.archived_at IS NULL \
                 AND r.edit_pending_at IS NULL",
            )
            .bind(enrollment_id.as_str())
            .bind(now_ms())
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?
        } else {
            0
        };

        // 来源计划诊断明细（t5）：详情端点带全量 excluded/duplicates（限长）。
        // 摘要视图已在 `course_summary_from_row` 里构建;这里按同一列的原始
        // JSON 再解析一次（单课程详情页,一次本地读,可接受）。
        let source_plan_raw: Option<String> = row
            .try_get("course_source_plan_json")
            .map_err(internal)?;
        let source_plan_detail = parse_source_plan_detail(source_plan_raw.as_deref());

        Ok(CourseDetail {
            course,
            enrollment_id,
            modules,
            next_lesson_id,
            due_review_count,
            graph,
            source_plan_detail,
        })
    }

    /// Resolve the enrollment for a user/course pair without creating one.
    pub(super) async fn enrollment_id_for(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
    ) -> Result<Option<LearningEnrollmentId>, AppError> {
        let id: Option<String> = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments WHERE user_id = ? AND course_id = ?",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        id.map(parse_id).transpose()
    }

    /// A single lesson's public view (mirrors the per-lesson projection in
    /// course_detail), used as the response of on-demand generation.
    pub(super) async fn lesson_view(
        &self,
        lesson_id: &LearningLessonId,
        enrollment_id: Option<&LearningEnrollmentId>,
    ) -> Result<LessonView, AppError> {
        let enrollment_value = enrollment_id.map(LearningEnrollmentId::as_str);
        let row = sqlx::query(
            "SELECT l.lesson_id, l.title, l.summary, l.purpose, l.position, l.estimated_minutes, l.content_generated, l.source_path, l.source_start, l.source_end, l.source_paths_json, \
                    c.source_kb_id AS course_source_kb_id, c.source_kb_ids AS course_source_kb_ids, c.samples_meta_json AS course_samples_meta_json, \
                    COALESCE(p.status, 'not_started') AS status \
             FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id \
             LEFT JOIN learning_lesson_progress p ON p.lesson_id = l.lesson_id AND p.enrollment_id = ? \
             WHERE l.lesson_id = ?",
        )
        .bind(enrollment_value)
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning lesson {lesson_id}")))?;
        let id: LearningLessonId = parse_id(row.try_get("lesson_id").map_err(internal)?)?;
        let status_text: String = row.try_get("status").map_err(internal)?;
        let status = LessonStatus::try_from(status_text.as_str()).map_err(AppError::Internal)?;
        // 课程行的来源库材料（t4）：课时 source 按采样命名空间解析回
        // (kb_id, 库内相对路径)，与目录视图同一把尺。
        let existing = self.existing_knowledge_base_ids().await?;
        let kb_sources = CourseKbSources::from_course_columns(&row)?.filtered(&existing);
        let source_path: Option<String> = row.try_get("source_path").map_err(internal)?;
        // 多引用（t4 评审补齐）：列表逐条解析，位序保留；列表为空回退三列。
        let sources = kb_sources.resolve_sources(
            row.try_get::<Option<String>, _>("source_paths_json")
                .map_err(internal)?
                .as_deref(),
            source_path,
            row.try_get("source_start").ok().flatten(),
            row.try_get("source_end").ok().flatten(),
        );
        let source = sources.first().cloned();
        let content_generated: i64 = row.try_get("content_generated").map_err(internal)?;
        let activities = self.lesson_activities(&id).await?;
        let sections = self.lesson_sections(&id).await?;
        Ok(LessonView {
            id,
            title: row.try_get("title").map_err(internal)?,
            summary: row.try_get("summary").map_err(internal)?,
            purpose: row.try_get("purpose").map_err(internal)?,
            position: row.try_get("position").map_err(internal)?,
            est: row.try_get("estimated_minutes").map_err(internal)?,
            generated: content_generated != 0,
            source,
            sources,
            status,
            activities,
            sections,
        })
    }

    /// The lesson's section rows in presentation order. Empty for legacy
    /// single-document lessons (the dual-read fallback renders `summary`).
    pub(super) async fn lesson_sections(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<Vec<SectionView>, AppError> {
        let rows = sqlx::query(
            "SELECT section_key, kind, title, points, visual, body_md, degraded, status, version, position \
             FROM learning_lesson_sections WHERE lesson_id = ? ORDER BY position, section_key",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut sections = Vec::with_capacity(rows.len());
        for row in rows {
            let kind_text: String = row.try_get("kind").map_err(internal)?;
            sections.push(SectionView {
                section_key: row.try_get("section_key").map_err(internal)?,
                kind: SectionKind::try_from(kind_text.as_str()).map_err(AppError::Internal)?,
                title: row.try_get("title").map_err(internal)?,
                points: row.try_get("points").map_err(internal)?,
                visual: row.try_get("visual").map_err(internal)?,
                body_md: row.try_get("body_md").map_err(internal)?,
                degraded: row.try_get::<i64, _>("degraded").map_err(internal)? != 0,
                status: row.try_get("status").map_err(internal)?,
                version: row.try_get("version").map_err(internal)?,
                position: row.try_get("position").map_err(internal)?,
            });
        }
        Ok(sections)
    }

    /// Renames a course display title (三轮 g3): only `title` moves — the
    /// learning goal, structure snapshot and all learning data stay untouched.
    /// Uniqueness follows the live-catalog rule: a soft-deleted tombstone row
    /// does not block the new name, a live duplicate is a 409.
    pub async fn rename_course(
        &self,
        course_id: &LearningCourseId,
        title: &str,
    ) -> Result<(), AppError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(AppError::BadRequest("course title must not be empty".into()));
        }
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM learning_courses WHERE course_id = ? AND deleted_at IS NULL",
        )
        .bind(course_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("course {course_id}")));
        }
        let collision: Option<String> = sqlx::query_scalar(
            "SELECT course_id FROM learning_courses \
             WHERE title = ? AND deleted_at IS NULL AND course_id <> ?",
        )
        .bind(title)
        .bind(course_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?;
        if collision.is_some() {
            return Err(AppError::Conflict(format!(
                "course title already exists: {title}"
            )));
        }
        sqlx::query("UPDATE learning_courses SET title = ?, updated_at = ? WHERE course_id = ?")
            .bind(title)
            .bind(now_ms())
            .bind(course_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

    /// Archives or restores a course (三轮 g3): flips `enabled` only — zero
    /// data loss either way. Archived courses drop out of every arrangement
    /// surface via the existing-and-enabled filter and reappear on restore.
    pub async fn set_course_archived(
        &self,
        course_id: &LearningCourseId,
        archived: bool,
    ) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE learning_courses SET enabled = ?, updated_at = ? \
             WHERE course_id = ? AND deleted_at IS NULL",
        )
        .bind(if archived { 0 } else { 1 })
        .bind(now_ms())
        .bind(course_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("course {course_id}")));
        }
        Ok(())
    }

    /// Deletes a course (三轮 g3 语义对齐 learnhub：账本永远保留): the course
    /// row becomes a tombstone and the course content (lessons/modules/concept
    /// edges/activities) is physically removed, while review cards, attempts,
    /// progress and the enrollment stay — the arrangement surfaces stop
    /// listing the course via the existing-and-enabled filter, the history
    /// surfaces keep the record. There is no option to wipe reviews anymore;
    /// pausing without deletion is what archiving is for.
    pub async fn delete_course(
        &self,
        course_id: &LearningCourseId,
        _user_id: &UserId,
    ) -> Result<(), AppError> {
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let exists: Option<String> =
            sqlx::query_scalar("SELECT course_id FROM learning_courses WHERE course_id = ?")
                .bind(course_id.as_str())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(internal)?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("course {course_id}")));
        }
        // 课程内容（课时/模块/概念网边）物理删除——复习保留的只是题目数据，
        // 不靠课时行续命。孤儿课时曾把新课的入口层向上顶（全局已教账把幽灵
        // 供给当真），这条语义从根上掐掉污染源。
        let content_sql = [
            "DELETE FROM learning_lesson_concepts WHERE lesson_id IN (\
                SELECT l.lesson_id FROM learning_lessons l \
                JOIN learning_modules m ON m.module_id = l.module_id \
                WHERE m.course_id = ?)",
            "DELETE FROM learning_activities WHERE lesson_id IN (\
                SELECT l.lesson_id FROM learning_lessons l \
                JOIN learning_modules m ON m.module_id = l.module_id \
                WHERE m.course_id = ?)",
            "DELETE FROM learning_question_tags WHERE source = 'course' AND question_id IN (\
                SELECT a.activity_id FROM learning_activities a \
                JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                JOIN learning_modules m ON m.module_id = l.module_id \
                WHERE m.course_id = ?)",
            "DELETE FROM learning_lessons WHERE module_id IN (\
                SELECT module_id FROM learning_modules WHERE course_id = ?)",
            "DELETE FROM learning_modules WHERE course_id = ?",
        ];
        for sql in content_sql {
            sqlx::query(sql)
                .bind(course_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
        }
        sqlx::query("DELETE FROM learning_course_tags WHERE course_id = ?")
            .bind(course_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        // 软删除：课程行留作墓碑（名称即标识、deleted_at 即删除状态位），
        // 遗留数据凭 course_id 归账，后续清理时有据可查。
        sqlx::query(
            "UPDATE learning_courses SET deleted_at = ?, updated_at = ? WHERE course_id = ?",
        )
        .bind(now_ms())
        .bind(now_ms())
        .bind(course_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

    /// Version of the course with the given title, if one exists. The
    /// tutorial seed uses this to decide between importing (absent), skipping
    /// (same version) and replacing (stale version) the preset course.
    pub(crate) async fn course_version_by_title(
        &self,
        title: &str,
    ) -> Result<Option<i64>, AppError> {
        let version: Option<i64> =
            sqlx::query_scalar(
                "SELECT version FROM learning_courses WHERE title = ? AND deleted_at IS NULL",
            )
            .bind(title)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        Ok(version)
    }

    /// Deletes every course with the given title and all of its data —
    /// enrollments, progress, attempts, reviews, concepts, lessons and
    /// activities — regardless of which user created them. Used by the
    /// tutorial seed to replace a stale preset course version.
    pub(crate) async fn delete_courses_by_title(&self, title: &str) -> Result<(), AppError> {
        let course_ids: Vec<String> =
            sqlx::query_scalar(
                "SELECT course_id FROM learning_courses WHERE title = ? AND deleted_at IS NULL",
            )
            .bind(title)
                .fetch_all(&self.pool)
                .await
                .map_err(internal)?;
        if course_ids.is_empty() {
            return Ok(());
        }
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        for course_id in &course_ids {
            let enrollment_ids: Vec<String> = sqlx::query_scalar(
                "SELECT enrollment_id FROM learning_enrollments WHERE course_id = ?",
            )
            .bind(course_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(internal)?;
            for enrollment_id in &enrollment_ids {
                for table in [
                    "learning_review_items",
                    "learning_lesson_progress",
                    "learning_attempts",
                ] {
                    sqlx::query(&format!("DELETE FROM {table} WHERE enrollment_id = ?"))
                        .bind(enrollment_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(internal)?;
                }
            }
            sqlx::query("DELETE FROM learning_enrollments WHERE course_id = ?")
                .bind(course_id)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            let content_sql = [
                "DELETE FROM learning_activities WHERE lesson_id IN (\
                    SELECT l.lesson_id FROM learning_lessons l \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_question_tags WHERE source = 'course' AND question_id IN (\
                    SELECT a.activity_id FROM learning_activities a \
                    JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_lessons WHERE module_id IN (\
                    SELECT module_id FROM learning_modules WHERE course_id = ?)",
                "DELETE FROM learning_modules WHERE course_id = ?",
            ];
            for sql in content_sql {
                sqlx::query(sql)
                    .bind(course_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
            }
            sqlx::query("DELETE FROM learning_course_tags WHERE course_id = ?")
                .bind(course_id)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            sqlx::query("DELETE FROM learning_courses WHERE course_id = ?")
                .bind(course_id)
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
        }
        transaction.commit().await.map_err(internal)?;
        Ok(())
    }

}

pub(super) fn recommend_next_lesson(modules: &[ModuleView]) -> Option<LearningLessonId> {
    // 进行中的课时优先；否则第一个未满足（未完成且未跳过）的课时。
    // 概念掌握度驱动的推荐随 per-course 概念体系一并退役（ADR-0009）。
    modules
        .iter()
        .flat_map(|module| &module.lessons)
        .find(|lesson| lesson.status == LessonStatus::InProgress)
        .or_else(|| {
            modules
                .iter()
                .flat_map(|module| &module.lessons)
                .find(|lesson| !lesson.status.satisfies())
        })
        .map(|lesson| lesson.id.clone())
}

pub(crate) fn validate_pack(pack: &CoursePack) -> Result<(), AppError> {
    if pack.title.trim().is_empty() {
        return Err(AppError::BadRequest("course title is required".into()));
    }
    if pack.domain.trim().is_empty() {
        return Err(AppError::BadRequest("course domain is required".into()));
    }
    if pack.version <= 0 {
        return Err(AppError::BadRequest("course version must be positive".into()));
    }
    if pack.modules.is_empty() {
        return Err(AppError::BadRequest("course must contain at least one module".into()));
    }
    for module in &pack.modules {
        if module.title.trim().is_empty() || module.lessons.is_empty() {
            return Err(AppError::BadRequest(
                "each module needs a title and at least one lesson".into(),
            ));
        }
        for lesson in &module.lessons {
            if lesson.title.trim().is_empty() || lesson.est <= 0 {
                return Err(AppError::BadRequest(
                    "each lesson needs a title and positive est".into(),
                ));
            }
            // 引用逐条校验（t4 评审补齐的多引用）：任一条 span 非法即拒收。
            for source in lesson.cited_sources() {
                if source.path.trim().is_empty()
                    || source.start.is_some_and(|value| value < 0)
                    || source.end.is_some_and(|value| value < 0)
                    || matches!((source.start, source.end), (Some(start), Some(end)) if end < start)
                {
                    return Err(AppError::BadRequest(format!(
                        "lesson {} has an invalid source span",
                        lesson.title
                    )));
                }
            }
            for activity in &lesson.activities {
                if activity.prompt.trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "activity prompt is required".into(),
                    ));
                }
                // Shared per-kind shape rules (all nine kinds) with the
                // manual-authoring bounds (2-5 options).
                if let Err(error) = activity.validate_shape((2, 5), true) {
                    return Err(AppError::BadRequest(error));
                }
            }
        }
    }
    Ok(())
}

fn course_summary_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<CourseSummary, AppError> {
    let source_kb_id = row
        .try_get::<Option<String>, _>("source_kb_id")
        .map_err(internal)?
        .map(parse_id)
        .transpose()?;
    // 来源库列表（t4）：列上是 JSON 数组；坏值（历史手改等）视为空——诊断
    // 字段不该让课程列表整体失败。列表空时回退单列的单元素视图（旧行兼容，
    // 由 `apply_existing_kb_ids` 的过滤链保证）。
    let source_kb_ids = parse_source_kb_ids(
        row.try_get::<Option<String>, _>("source_kb_ids")
            .map_err(internal)?
            .as_deref(),
    );
    let source_kb_ids = if source_kb_ids.is_empty() {
        source_kb_id.iter().cloned().collect()
    } else {
        source_kb_ids
    };
    // 目标列 NOT NULL DEFAULT ''：空串即"无目标"（旧课程/描述流）。
    let learning_goal = row
        .try_get::<Option<String>, _>("learning_goal")
        .map_err(internal)?
        .map(|goal| goal.trim().to_owned())
        .filter(|goal| !goal.is_empty());
    let samples_meta_json: Option<String> =
        row.try_get("samples_meta_json").map_err(internal)?;
    let sampling = samples_meta_json
        .as_deref()
        .and_then(parse_sampling_view);
    // 来源计划快照摘要（t4）：NULL → None（字段不出现）；未知版本/坏形状
    // → available=false（诊断字段不让课程列表整体失败）。
    let source_plan_json: Option<String> = row
        .try_get("course_source_plan_json")
        .map_err(internal)?;
    let source_plan = parse_source_plan_view(source_plan_json.as_deref());
    Ok(CourseSummary {
        id: parse_id(row.try_get("course_id").map_err(internal)?)?,
        title: row.try_get("title").map_err(internal)?,
        description: row.try_get("description").map_err(internal)?,
        domain: row.try_get("domain").map_err(internal)?,
        course_kind: CourseKind::try_from(row.try_get::<String, _>("course_kind").map_err(internal)?.as_str())
            .map_err(AppError::Internal)?,
        // 主来源：列表首个（过滤后由 `apply_existing_kb_ids` 修正）。
        source_kb_id: source_kb_ids.first().cloned().or(source_kb_id),
        source_kb_ids,
        version: row.try_get("version").map_err(internal)?,
        enrolled: row.try_get::<i64, _>("enrolled").map_err(internal)? != 0,
        total_lessons: row.try_get("total_lessons").map_err(internal)?,
        completed_lessons: row.try_get("completed_lessons").map_err(internal)?,
        enabled: row.try_get::<i64, _>("enabled").map_err(internal)? != 0,
        // 进度尺 = 结算口径（j05 账本落地）：结算节点数；账本无记录（存量
        // 课程/未结算）回退完成数——不因切换口径把卡片进度归零。
        settled_or_completed: {
            let settled: i64 = row.try_get("settled_nodes").map_err(internal)?;
            let completed: i64 = row.try_get("completed_lessons").map_err(internal)?;
            if settled > 0 { settled } else { completed }
        },
        due_today: row.try_get("due_today").map_err(internal)?,
        // XP 账本（j05）已落地：课程累计净 XP（含检测线与乱猜负账）。
        total_xp: Some(row.try_get("total_xp").map_err(internal)?),
        updated_at: row.try_get("updated_at").map_err(internal)?,
        tags: Vec::new(),
        learning_goal,
        sampling,
        source_plan,
    })
}

/// `source_kb_ids` 列的 JSON 数组解析（t4）：只收合法库 id，坏值/坏 JSON
/// 视为空（读侧过滤链会再剔已删库）。
fn parse_source_kb_ids(raw: Option<&str>) -> Vec<KnowledgeBaseId> {
    let Some(raw) = raw else { return Vec::new() };
    let Ok(ids) = serde_json::from_str::<Vec<String>>(raw) else {
        return Vec::new();
    };
    ids.iter().filter_map(|id| KnowledgeBaseId::parse(id).ok()).collect()
}

impl CourseSummary {
    /// 来源库的读侧过滤（t4）：剔除已删库 id（不外泄悬空 id），主来源重算为
    /// 过滤后首个——单列 `source_kb_id` 的 SetNull 语义由应用层读侧承担。
    fn apply_existing_kb_ids(&mut self, existing: &HashSet<String>) {
        self.source_kb_ids.retain(|id| existing.contains(id.as_str()));
        self.source_kb_id = self.source_kb_ids.first().cloned();
    }
}

/// 课程行的来源读侧材料（t4）：过滤后的来源库列表 + 采样披露的逐库归属。
/// 课时 source 的 `(kb_id, rel_path)` 解析（目录/详情两个视图）与「来源：
/// 库A、库B」展示都由它承载。
#[derive(Default, Clone)]
pub(super) struct CourseKbSources {
    pub(super) ids: Vec<KnowledgeBaseId>,
    /// 逐库归属（**不做 id 过滤**：路径命名空间需要它认领已删库的路径，
    /// 由 `resolve_source` 再把归属过滤成"现存库或无归属"）。
    pub(super) bases: Vec<SampleBaseDisclosure>,
}

impl CourseKbSources {
    /// 从课程行（`source_kb_id` / `source_kb_ids` / `samples_meta_json` 三列）。
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, AppError> {
        Self::from_course_columns(row)
    }

    /// 课程列前缀固定的读取（lesson_view 的 join 别名列）。
    pub(super) fn from_course_columns(row: &sqlx::sqlite::SqliteRow) -> Result<Self, AppError> {
        let (ids_raw, meta): (Option<String>, Option<String>) = (
            row.try_get("course_source_kb_ids")
                .or_else(|_| row.try_get("source_kb_ids"))
                .map_err(internal)?,
            row.try_get("course_samples_meta_json")
                .or_else(|_| row.try_get("samples_meta_json"))
                .map_err(internal)?,
        );
        let single: Option<String> = row
            .try_get("course_source_kb_id")
            .or_else(|_| row.try_get("source_kb_id"))
            .map_err(internal)?;
        let mut ids = parse_source_kb_ids(ids_raw.as_deref());
        if ids.is_empty() {
            if let Some(single) = single.as_deref().and_then(|id| KnowledgeBaseId::parse(id).ok()) {
                ids.push(single);
            }
        }
        let bases = meta
            .as_deref()
            .and_then(|raw| serde_json::from_str::<SampleDisclosure>(raw).ok())
            .map(|disclosure| disclosure.bases)
            .unwrap_or_default();
        Ok(Self { ids, bases })
    }

    /// 读侧过滤已删库 id：列表剔除；逐库归属保留（见 `bases` 注释）。
    pub(super) fn filtered(mut self, existing: &HashSet<String>) -> Self {
        self.ids.retain(|id| existing.contains(id.as_str()));
        self
    }

    pub(super) fn primary(&self) -> Option<KnowledgeBaseId> {
        self.ids.first().cloned()
    }

    /// 课时 source 的库归属解析（t4）：多库路径按命名空间还原成
    /// `(kb_id, 库内相对路径)`；单库/旧课程裸路径原样（`kb_id = None`，读侧
    /// 回退课程主来源）；已删库的路径解析成"无归属"且路径保持原样（前端会
    /// 读不到原文并如实报错，不外泄悬空 id）。
    fn resolve_source(&self, path: String, start: Option<i64>, end: Option<i64>) -> SourceSpan {
        let resolved = base_for_sample_path(&path, &self.bases, &self.ids)
            .filter(|id| self.ids.iter().any(|candidate| candidate == id));
        let path = match &resolved {
            Some(id) => rel_path_for_base(&path, namespace_for_base(&self.bases, id)).to_owned(),
            None => path,
        };
        SourceSpan { path, start, end, kb_id: resolved }
    }

    /// 课时视图的来源清单（t4 评审补齐的多引用）：`source_paths_json` 列表
    /// 逐条解析（位序 = 引用顺序，首个 = 主来源）；列表为空（老行/未回填）
    /// 回退三列的单条——`resolve_source` 同一把尺，双读不漂移。
    fn resolve_sources(
        &self,
        paths_json: Option<&str>,
        primary_path: Option<String>,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Vec<SourceSpan> {
        let listed: Vec<StoredSourcePath> = paths_json
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();
        if listed.is_empty() {
            return primary_path
                .map(|path| self.resolve_source(path, start, end))
                .into_iter()
                .collect();
        }
        listed
            .into_iter()
            .map(|entry| self.resolve_source(entry.path, entry.start, entry.end))
            .collect()
    }
}

/// `learning_lessons.source_paths_json` 的落库元素：与 `SourceSpan` 的路径
/// 三字段同构（库归属不落库——读侧按采样命名空间实时解析，与单列路径同一
/// 口径）。未知字段（将来新增）忽略。
#[derive(Debug, Clone, serde::Deserialize)]
struct StoredSourcePath {
    path: String,
    #[serde(default)]
    start: Option<i64>,
    #[serde(default)]
    end: Option<i64>,
}

/// 摘要视图的示例/摘要列表限长（t5）：列表端点不背全量清单,详情端点用
/// [`SOURCE_PLAN_DETAIL_MAX`] 的更高上限。
const SOURCE_PLAN_SKIPPED_EXAMPLES_MAX: usize = 20;
const SOURCE_PLAN_DUPLICATE_EXAMPLES_MAX: usize = 20;
const SOURCE_PLAN_UNCERTAIN_LABELS_MAX: usize = 20;
pub(super) const SOURCE_PLAN_WARNINGS_MAX: usize = 20;
const SOURCE_PLAN_GAPS_MAX: usize = 20;
const SOURCE_PLAN_CHAPTER_SUMMARIES_MAX: usize = 50;
/// 详情端点诊断明细限长（票面「详情保留完整 excluded/duplicates,可限长」）。
const SOURCE_PLAN_DETAIL_MAX: usize = 200;

/// 快照信封的解析结果（t5）：`available=false` 时信封不可用（未知版本/
/// v1 形状损坏），读侧降级不报错。解析成功的信封供摘要/明细两个视图共用。
enum ParsedSourcePlan {
    /// 未知版本（含读不出 version 的坏 JSON）：version 保留用于披露。
    UnknownVersion(u64),
    /// v1 但形状损坏（历史手改等）：版本可读但内容不可信。
    BrokenShape(u64),
    Available(crate::generation::CourseSourcePlanSnapshot),
}

fn parse_source_plan_snapshot(raw: &str) -> ParsedSourcePlan {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return ParsedSourcePlan::UnknownVersion(0);
    };
    let version = value.get("version").and_then(Value::as_u64);
    match version {
        Some(version)
            if version
                == u64::from(crate::generation::COURSE_SOURCE_PLAN_SNAPSHOT_VERSION) =>
        {
            match serde_json::from_value::<crate::generation::CourseSourcePlanSnapshot>(value) {
                Ok(snapshot) => ParsedSourcePlan::Available(snapshot),
                Err(_) => ParsedSourcePlan::BrokenShape(version),
            }
        }
        // 未知版本（version 缺失 = 0:不可信,同样降级）。
        other => ParsedSourcePlan::UnknownVersion(other.unwrap_or(0)),
    }
}

/// t1 分诊三角色文件 → topic 去向的对账辅助（t5 覆盖面）:rel_path →
/// role 映射来自覆盖报告的文件台账（文件只是锚载体,topic 按锚点归属）。
fn topic_role_by_path(
    coverage: &crate::generation::SourceCoverage,
) -> std::collections::HashMap<&str, crate::generation::KbTriageRole> {
    coverage
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.role))
        .collect()
}

/// 来源计划快照列的摘要解析（t4 落地、t5 细化分面,仿 `parse_sampling_view`）：
/// NULL → `None`（旧课程/描述流——字段不出现,不伪造空计划）;未知版本/坏
/// 形状 → `available=false` 降级,不报错。完整信封留在库里,摘要只读计数与
/// ≤20 的示例（列表端点不背全量清单）。
fn parse_source_plan_view(raw: Option<&str>) -> Option<crate::models::CourseSourcePlanView> {
    let raw = raw?;
    let snapshot = match parse_source_plan_snapshot(raw) {
        ParsedSourcePlan::Available(snapshot) => snapshot,
        ParsedSourcePlan::UnknownVersion(version) => {
            return Some(unavailable_source_plan_view(version));
        }
        ParsedSourcePlan::BrokenShape(version) => {
            return Some(unavailable_source_plan_view(version));
        }
    };
    let counts = snapshot.triage.counts;
    let triage = crate::models::CourseSourcePlanTriageView {
        total_files: counts.core
            + counts.prerequisite
            + counts.practice
            + counts.mixed
            + counts.noise
            + counts.duplicate
            + counts.uncertain,
        core: counts.core,
        prerequisite: counts.prerequisite,
        practice: counts.practice,
        mixed: counts.mixed,
        noise: counts.noise,
        duplicate: counts.duplicate,
        uncertain: counts.uncertain,
        degraded: snapshot.triage.degraded,
        batch_repair_count: (snapshot.triage.batch_repair_count > 0)
            .then_some(snapshot.triage.batch_repair_count),
        still_missing_count: (snapshot.triage.still_missing_count > 0)
            .then_some(snapshot.triage.still_missing_count),
        warnings: snapshot
            .triage
            .warnings
            .iter()
            .take(SOURCE_PLAN_WARNINGS_MAX)
            .cloned()
            .collect(),
    };
    // 章节/来源映射摘要:章课时数与 skipped 来自 t3 覆盖报告的章对账。
    let chapter_lessons: std::collections::HashMap<&str, (&usize, &bool, &Option<String>)> =
        snapshot
            .coverage
            .chapters
            .iter()
            .map(|report| (report.chapter.as_str(), (&report.lessons, &report.skipped, &report.skip_reason)))
            .collect();
    let chapter_summaries = snapshot
        .plan
        .chapters
        .iter()
        .take(SOURCE_PLAN_CHAPTER_SUMMARIES_MAX)
        .map(|chapter| {
            let (lessons, skipped, skip_reason) = chapter_lessons
                .get(chapter.title.as_str())
                .copied()
                .unwrap_or((&0, &false, &None));
            let mut source_paths = std::collections::BTreeSet::new();
            for anchor in &chapter.source_anchors {
                source_paths.insert(anchor.rel_path.as_str());
            }
            crate::models::CourseSourcePlanChapterSummaryView {
                title: chapter.title.clone(),
                concepts: chapter.covers.len(),
                lessons: *lessons,
                skipped: *skipped,
                skip_reason: skip_reason.clone(),
                source_files: source_paths.len(),
            }
        })
        .collect();
    let plan = crate::models::CourseSourcePlanPlanView {
        chapters: snapshot.plan.chapters.len(),
        concepts: snapshot.plan.concept_graph.nodes.len(),
        edges: snapshot.plan.concept_graph.edges.len(),
        gaps: snapshot
            .plan
            .evidence
            .gaps
            .iter()
            .take(SOURCE_PLAN_GAPS_MAX)
            .cloned()
            .collect(),
        chapter_summaries,
        warnings: snapshot
            .plan
            .warnings
            .iter()
            .take(SOURCE_PLAN_WARNINGS_MAX)
            .cloned()
            .collect(),
    };
    // 覆盖面:topic 四态 + 前置/练习角色去向（topic 锚点 → 文件角色对账）。
    let role_by_path = topic_role_by_path(&snapshot.coverage);
    let mut topics_covered = 0usize;
    let mut topics_skipped = 0usize;
    let mut prerequisite_topics = 0usize;
    let mut practice_topics = 0usize;
    let mut anchor_uncertain_topics = 0usize;
    let mut uncertain = Vec::new();
    let mut skipped_examples = Vec::new();
    for topic in &snapshot.coverage.topics {
        match topic.status {
            crate::generation::TopicStatus::Covered => {
                topics_covered += 1;
                match role_by_path.get(topic.anchor.rel_path.as_str()) {
                    Some(crate::generation::KbTriageRole::Prerequisite) => {
                        prerequisite_topics += 1;
                    }
                    Some(crate::generation::KbTriageRole::Practice) => {
                        practice_topics += 1;
                    }
                    _ => {}
                }
            }
            crate::generation::TopicStatus::Skipped => {
                topics_skipped += 1;
                if skipped_examples.len() < SOURCE_PLAN_SKIPPED_EXAMPLES_MAX {
                    skipped_examples.push(crate::models::CourseSourcePlanSkippedTopicView {
                        label: topic.label.clone(),
                        reason: topic.reason.clone(),
                    });
                }
            }
            crate::generation::TopicStatus::Uncertain => {
                if uncertain.len() < SOURCE_PLAN_UNCERTAIN_LABELS_MAX {
                    uncertain.push(topic.label.clone());
                }
            }
        }
        if topic.anchor.anchor_uncertain {
            anchor_uncertain_topics += 1;
        }
    }
    let coverage = crate::models::CourseSourcePlanCoverageView {
        topics_covered,
        topics_skipped,
        prerequisite_topics,
        practice_topics,
        anchor_uncertain_topics,
        uncertain,
        skipped_examples,
        fallback_lessons: snapshot
            .coverage
            .lessons
            .iter()
            .filter(|lesson| lesson.fallback)
            .count(),
        warnings: snapshot
            .coverage
            .warnings
            .iter()
            .take(SOURCE_PLAN_WARNINGS_MAX)
            .cloned()
            .collect(),
    };
    let duplicates = crate::models::CourseSourcePlanDuplicatesView {
        count: snapshot.triage.duplicates.len(),
        examples: snapshot
            .triage
            .duplicates
            .iter()
            .take(SOURCE_PLAN_DUPLICATE_EXAMPLES_MAX)
            .map(|duplicate| crate::models::CourseSourcePlanDuplicateExampleView {
                path: duplicate.path.clone(),
                duplicate_of: duplicate.duplicate_of.clone(),
                canonical_reason: duplicate.canonical_reason.clone(),
            })
            .collect(),
    };
    Some(crate::models::CourseSourcePlanView {
        snapshot: crate::models::CourseSourcePlanSnapshotView {
            version: Some(u64::from(snapshot.version)),
            available: true,
        },
        triage,
        plan,
        coverage,
        duplicates,
    })
}

/// 不可用快照的降级摘要（未知版本/坏形状）：版本保留披露、计数全零、
/// `available=false`——读侧不报错、不伪造可信计数。
fn unavailable_source_plan_view(version: u64) -> crate::models::CourseSourcePlanView {
    crate::models::CourseSourcePlanView {
        snapshot: crate::models::CourseSourcePlanSnapshotView {
            version: (version > 0).then_some(version),
            available: false,
        },
        ..Default::default()
    }
}

/// 详情端点的来源计划诊断明细（t5）：全量 excluded（path + role + reason）
/// 与 duplicates,限长 [`SOURCE_PLAN_DETAIL_MAX`];列 NULL → `None`（字段不
/// 出现）;不可用信封 → 仍返回空明细（available=false 由摘要视图披露,详情
/// 明细零条目即可,不需要第二种降级叙事）。
fn parse_source_plan_detail(raw: Option<&str>) -> Option<crate::models::CourseSourcePlanDetailView> {
    let raw = raw?;
    let snapshot = match parse_source_plan_snapshot(raw) {
        ParsedSourcePlan::Available(snapshot) => snapshot,
        _ => return Some(crate::models::CourseSourcePlanDetailView::default()),
    };
    Some(crate::models::CourseSourcePlanDetailView {
        excluded: snapshot
            .triage
            .excluded
            .iter()
            .take(SOURCE_PLAN_DETAIL_MAX)
            .map(|entry| crate::models::CourseSourcePlanExcludedView {
                path: entry.path.clone(),
                role: serde_json::to_value(entry.role)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                reason: entry.reason.clone(),
            })
            .collect(),
        duplicates: snapshot
            .triage
            .duplicates
            .iter()
            .take(SOURCE_PLAN_DETAIL_MAX)
            .map(|duplicate| crate::models::CourseSourcePlanDuplicateExampleView {
                path: duplicate.path.clone(),
                duplicate_of: duplicate.duplicate_of.clone(),
                canonical_reason: duplicate.canonical_reason.clone(),
            })
            .collect(),
    })
}

/// 采样披露的对外视图上限：低分未采样的示例路径（UI 披露够用，响应不背
/// 全量清单）。
const LOW_SCORE_EXAMPLES_MAX: usize = 20;

/// 解析库里的采样披露快照（t2）为对外视图；无法解析的历史值视为无披露
/// （诊断字段不该让课程列表整体失败）。逐库归属（t4）一并映射——解析不出
/// 库 id 的条目跳过（不造悬空 id）。
fn parse_sampling_view(raw: &str) -> Option<CourseSamplingView> {
    let disclosure: crate::generation::SampleDisclosure = serde_json::from_str(raw).ok()?;
    Some(CourseSamplingView {
        goal: disclosure.goal,
        total_files: disclosure.total_files,
        sampled_files: disclosure.sampled_files,
        coherence_files: disclosure.coherence_files,
        goal_fallback: disclosure.goal_fallback,
        low_score_files: disclosure.low_score_files.len(),
        low_score_examples: disclosure
            .low_score_files
            .iter()
            .take(LOW_SCORE_EXAMPLES_MAX)
            .cloned()
            .collect(),
        bases: disclosure
            .bases
            .iter()
            .filter_map(|base| {
                Some(crate::models::CourseSamplingBaseView {
                    kb_id: base.kb_id.parse().ok()?,
                    name: base.name.clone(),
                    namespace: base.namespace.clone(),
                    total_files: base.total_files,
                    sampled_files: base.sampled_files,
                    goal_fallback: base.goal_fallback,
                })
            })
            .collect(),
    })
}
