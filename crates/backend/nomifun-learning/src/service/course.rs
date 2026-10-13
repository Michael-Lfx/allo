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
        let (blueprint_json, samples_json, learning_goal, samples_meta_json) = outline
            .as_ref()
            .map(|snapshot| {
                (
                    Some(snapshot.blueprint_json.as_str()),
                    Some(snapshot.samples_json.as_str()),
                    Some(snapshot.learning_goal.as_str()),
                    Some(snapshot.samples_meta_json.as_deref()),
                )
            })
            .unwrap_or((None, None, None, None));
        let content_generated: i64 = if is_outline { 0 } else { 1 };
        let now = now_ms();
        let course_id = LearningCourseId::new();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, source_kb_id, source_kb_ids, version, blueprint_json, samples_json, \
              samples_meta_json, learning_goal, teaching_style, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                let (source_path, source_start, source_end) = lesson
                    .source
                    .as_ref()
                    .map(|source| {
                        (
                            Some(source.path.trim()),
                            source.start,
                            source.end,
                        )
                    })
                    .unwrap_or((None, None, None));
                sqlx::query(
                    "INSERT INTO learning_lessons \
                     (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
                      content_generated, source_path, source_start, source_end) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(lesson_id.as_str())
                .bind(module_id.as_str())
                .bind(lesson.title.trim())
                .bind(lesson.summary.trim())
                .bind(lesson.purpose.trim())
                .bind(lesson_position as i64)
                .bind(lesson.estimated_minutes)
                .bind(content_generated)
                .bind(source_path)
                .bind(source_start)
                .bind(source_end)
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
        let rows = sqlx::query(
            "SELECT c.course_id, c.title, c.description, c.domain, c.course_kind, c.source_kb_id, c.source_kb_ids, c.version, c.updated_at, \
                    c.learning_goal, c.samples_meta_json, \
                    EXISTS(SELECT 1 FROM learning_enrollments e \
                           WHERE e.course_id = c.course_id AND e.user_id = ?) AS enrolled, \
                    (SELECT COUNT(*) FROM learning_lessons l \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE m.course_id = c.course_id) AS total_lessons, \
                    (SELECT COUNT(*) FROM learning_lesson_progress p \
                     JOIN learning_enrollments e ON e.enrollment_id = p.enrollment_id \
                     JOIN learning_lessons l ON l.lesson_id = p.lesson_id \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE e.user_id = ? AND m.course_id = c.course_id AND p.status = 'completed') AS completed_lessons \
             FROM learning_courses c WHERE c.deleted_at IS NULL \
             ORDER BY c.updated_at DESC, c.course_id",
        )
        .bind(user_id.as_str())
        .bind(user_id.as_str())
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
                    c.learning_goal, c.samples_meta_json, \
                    EXISTS(SELECT 1 FROM learning_enrollments e \
                           WHERE e.course_id = c.course_id AND e.user_id = ?) AS enrolled, \
                    (SELECT COUNT(*) FROM learning_lessons l \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE m.course_id = c.course_id) AS total_lessons, \
                    (SELECT COUNT(*) FROM learning_lesson_progress p \
                     JOIN learning_enrollments e ON e.enrollment_id = p.enrollment_id \
                     JOIN learning_lessons l ON l.lesson_id = p.lesson_id \
                     JOIN learning_modules m ON m.module_id = l.module_id \
                     WHERE e.user_id = ? AND m.course_id = c.course_id AND p.status = 'completed') AS completed_lessons \
             FROM learning_courses c \
             WHERE c.course_id = ? AND c.deleted_at IS NULL",
        )
        .bind(user_value)
        .bind(user_value)
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
                        l.content_generated, l.source_path, l.source_start, l.source_end, \
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
                // 采样命名空间还原到来源库；单库/旧课程裸路径不解析。
                let source = source_path.map(|path| {
                    kb_sources.resolve_source(
                        path,
                        lesson_row.try_get("source_start").ok().flatten(),
                        lesson_row.try_get("source_end").ok().flatten(),
                    )
                });
                let content_generated: i64 =
                    lesson_row.try_get("content_generated").map_err(internal)?;
                lessons.push(LessonView {
                    id: lesson_id.clone(),
                    title: lesson_row.try_get("title").map_err(internal)?,
                    summary: lesson_row.try_get("summary").map_err(internal)?,
                    purpose: lesson_row.try_get("purpose").map_err(internal)?,
                    position: lesson_row.try_get("position").map_err(internal)?,
                    estimated_minutes: lesson_row
                        .try_get("estimated_minutes")
                        .map_err(internal)?,
                    generated: content_generated != 0,
                    source,
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

        Ok(CourseDetail {
            course,
            enrollment_id,
            modules,
            next_lesson_id,
            due_review_count,
            graph,
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
            "SELECT l.lesson_id, l.title, l.summary, l.purpose, l.position, l.estimated_minutes, l.content_generated, l.source_path, l.source_start, l.source_end, \
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
        let source = source_path.map(|path| {
            kb_sources.resolve_source(
                path,
                row.try_get("source_start").ok().flatten(),
                row.try_get("source_end").ok().flatten(),
            )
        });
        let content_generated: i64 = row.try_get("content_generated").map_err(internal)?;
        let activities = self.lesson_activities(&id).await?;
        let sections = self.lesson_sections(&id).await?;
        Ok(LessonView {
            id,
            title: row.try_get("title").map_err(internal)?,
            summary: row.try_get("summary").map_err(internal)?,
            purpose: row.try_get("purpose").map_err(internal)?,
            position: row.try_get("position").map_err(internal)?,
            estimated_minutes: row.try_get("estimated_minutes").map_err(internal)?,
            generated: content_generated != 0,
            source,
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

    /// Deletes a course. With `delete_reviews` the learner's enrollment and
    /// all derived data plus the course content are wiped. Otherwise the
    /// catalog row and the course's graph edges disappear, while content,
    /// questions and review data stay so orphaned concepts remain reviewable.
    pub async fn delete_course(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
        delete_reviews: bool,
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
        let enrollment_ids: Vec<String> = sqlx::query_scalar(
            "SELECT enrollment_id FROM learning_enrollments WHERE course_id = ? AND user_id = ?",
        )
        .bind(course_id.as_str())
        .bind(user_id.as_str())
        .fetch_all(&mut *transaction)
        .await
        .map_err(internal)?;
        if delete_reviews {
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
            sqlx::query("DELETE FROM learning_enrollments WHERE course_id = ? AND user_id = ?")
                .bind(course_id.as_str())
                .bind(user_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
        }
        // 课程内容（课时/模块/概念网边）无论勾选与否都物理删除——复习保留
        // 的只是题目数据，不靠课时行续命。孤儿课时曾把新课的入口层向上顶
        // （全局已教账把幽灵供给当真），这条语义从根上掐掉污染源。
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
        if delete_reviews {
            // 概念登记表跟勾选走：全删时清掉不再被任何课时引用的概念——
            // 概念是跨课程共享资产，只收紧无主条目，不误删他课在用的。
            sqlx::query(
                "DELETE FROM learning_concept_registry WHERE concept_id NOT IN (\
                    SELECT DISTINCT concept_id FROM learning_lesson_concepts)",
            )
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
            if lesson.title.trim().is_empty() || lesson.estimated_minutes <= 0 {
                return Err(AppError::BadRequest(
                    "each lesson needs a title and positive estimated_minutes".into(),
                ));
            }
            if let Some(source) = &lesson.source {
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
        updated_at: row.try_get("updated_at").map_err(internal)?,
        tags: Vec::new(),
        learning_goal,
        sampling,
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
