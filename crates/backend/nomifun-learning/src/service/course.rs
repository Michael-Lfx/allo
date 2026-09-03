use super::*;

impl LearningService {

    pub async fn import_course(&self, pack: CoursePack) -> Result<CourseDetail, AppError> {
        self.import_course_with_context(pack, None).await
    }

    /// Import an on-demand course skeleton: modules, lessons (title + purpose +
    /// source + concepts, no body yet), concepts, plus the persisted blueprint
    /// and source samples that deferred lesson generation will use.
    pub(crate) async fn import_course_outline(
        &self,
        pack: CoursePack,
        blueprint_json: String,
        samples_json: String,
    ) -> Result<CourseDetail, AppError> {
        self.import_course_with_context(pack, Some((blueprint_json, samples_json)))
            .await
    }

    async fn import_course_with_context(
        &self,
        pack: CoursePack,
        outline: Option<(String, String)>,
    ) -> Result<CourseDetail, AppError> {
        validate_pack(&pack)?;
        if let Some(kb_id) = &pack.source_kb_id {
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

        let is_outline = outline.is_some();
        let (blueprint_json, samples_json) = outline
            .as_ref()
            .map(|(blueprint, samples)| (Some(blueprint.as_str()), Some(samples.as_str())))
            .unwrap_or((None, None));
        let content_generated: i64 = if is_outline { 0 } else { 1 };
        let now = now_ms();
        let course_id = LearningCourseId::new();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, source_kb_id, version, blueprint_json, samples_json, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(course_id.as_str())
        .bind(pack.title.trim())
        .bind(pack.description.trim())
        .bind(pack.domain.trim())
        .bind(pack.source_kb_id.as_ref().map(KnowledgeBaseId::as_str))
        .bind(pack.version)
        .bind(blueprint_json)
        .bind(samples_json)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;

        let mut concepts = HashMap::new();
        for concept in &pack.concepts {
            let concept_id = LearningConceptId::new();
            sqlx::query(
                "INSERT INTO learning_concepts \
                 (concept_id, course_id, concept_key, title, description) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(concept_id.as_str())
            .bind(course_id.as_str())
            .bind(concept.key.trim())
            .bind(concept.title.trim())
            .bind(concept.description.trim())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
            concepts.insert(concept.key.clone(), concept_id);
        }

        for concept in &pack.concepts {
            let concept_id = &concepts[&concept.key];
            for prerequisite in &concept.prerequisites {
                sqlx::query(
                    "INSERT INTO learning_concept_prerequisites \
                     (concept_id, prerequisite_concept_id) VALUES (?, ?)",
                )
                .bind(concept_id.as_str())
                .bind(concepts[prerequisite].as_str())
                .execute(&mut *transaction)
                .await
                .map_err(internal)?;
            }
        }

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

                for concept_key in &lesson.concepts {
                    sqlx::query(
                        "INSERT INTO learning_lesson_concepts (lesson_id, concept_id) VALUES (?, ?)",
                    )
                    .bind(lesson_id.as_str())
                    .bind(concepts[concept_key].as_str())
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
                }

                for (activity_position, activity) in lesson.activities.iter().enumerate() {
                    let activity_id = LearningActivityId::new();
                    let config = StoredActivityConfig {
                        options: activity.options.clone(),
                        answer: activity.answer.clone(),
                        explanation: activity.explanation.clone(),
                        distractors: activity.distractors.clone(),
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

                    let activity_concepts = if activity.concepts.is_empty() {
                        &lesson.concepts
                    } else {
                        &activity.concepts
                    };
                    for concept_key in activity_concepts {
                        sqlx::query(
                            "INSERT INTO learning_activity_concepts \
                             (activity_id, concept_id) VALUES (?, ?)",
                        )
                        .bind(activity_id.as_str())
                        .bind(concepts[concept_key].as_str())
                        .execute(&mut *transaction)
                        .await
                        .map_err(internal)?;
                    }
                }
            }
        }

        transaction.commit().await.map_err(internal)?;
        self.course_detail(&course_id, None).await
    }

    pub async fn list_courses(&self, user_id: &UserId) -> Result<Vec<CourseSummary>, AppError> {
        let rows = sqlx::query(
            "SELECT c.course_id, c.title, c.description, c.domain, c.course_kind, c.source_kb_id, c.version, c.updated_at, \
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
             FROM learning_courses c ORDER BY c.updated_at DESC, c.course_id",
        )
        .bind(user_id.as_str())
        .bind(user_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut courses = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut course = course_summary_from_row(row)?;
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
            "SELECT c.course_id, c.title, c.description, c.domain, c.course_kind, c.source_kb_id, c.version, c.updated_at, \
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
             FROM learning_courses c WHERE c.course_id = ?",
        )
        .bind(user_value)
        .bind(user_value)
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("learning course {course_id}")))?;
        let course = course_summary_from_row(&row)?;
        let course = CourseSummary {
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
                let source = source_path.map(|path| SourceSpan {
                    path,
                    start: lesson_row.try_get("source_start").ok().flatten(),
                    end: lesson_row.try_get("source_end").ok().flatten(),
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
                    concepts: self.lesson_concepts(&lesson_id).await?,
                    activities: self.lesson_activities(&lesson_id).await?,
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

        let concepts = self
            .course_concepts(course_id, enrollment_id.as_ref())
            .await?;
        // 学习图课程：大纲被「下一步推荐节点」取代——next_lesson_id 不参与，
        // 换成图视图（结构 + 就绪集推荐 ≤10）。传统课程路径零变化。
        let graph = if course.course_kind == CourseKind::LearningGraph {
            Some(
                self.assemble_learning_graph_view(course_id, &modules)
                    .await?,
            )
        } else {
            None
        };
        let next_lesson_id = if graph.is_some() {
            None
        } else {
            recommend_next_lesson(&modules, &concepts)
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
            concepts,
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

    /// Concept-key to concept-id map for a course, used to bind activities when
    /// inserting deferred lesson content.
    pub(super) async fn concept_map_for_course(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<HashMap<String, LearningConceptId>, AppError> {
        let rows = sqlx::query(
            "SELECT concept_key, concept_id FROM learning_concepts WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            let key: String = row.try_get("concept_key").map_err(internal)?;
            let id: LearningConceptId = parse_id(row.try_get("concept_id").map_err(internal)?)?;
            map.insert(key, id);
        }
        Ok(map)
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
            "SELECT l.lesson_id, l.title, l.summary, l.purpose, l.position, l.estimated_minutes, l.content_generated, l.source_path, l.source_start, l.source_end, COALESCE(p.status, 'not_started') AS status FROM learning_lessons l LEFT JOIN learning_lesson_progress p ON p.lesson_id = l.lesson_id AND p.enrollment_id = ? WHERE l.lesson_id = ?",
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
        let source_path: Option<String> = row.try_get("source_path").map_err(internal)?;
        let source = source_path.map(|path| SourceSpan {
            path,
            start: row.try_get("source_start").ok().flatten(),
            end: row.try_get("source_end").ok().flatten(),
        });
        let content_generated: i64 = row.try_get("content_generated").map_err(internal)?;
        let concepts = self.lesson_concepts(&id).await?;
        let activities = self.lesson_activities(&id).await?;
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
            concepts,
            activities,
        })
    }

    /// Deletes a course. With `delete_reviews` the learner's enrollment and
    /// all derived data plus the course content are wiped. Otherwise only
    /// the catalog row disappears, so orphaned concepts stay reviewable.
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
                    "learning_mastery_states",
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
            let content_sql = [
                "DELETE FROM learning_activity_concepts WHERE activity_id IN (\
                    SELECT a.activity_id FROM learning_activities a \
                    JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
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
                "DELETE FROM learning_lesson_concepts WHERE lesson_id IN (\
                    SELECT l.lesson_id FROM learning_lessons l \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_lessons WHERE module_id IN (\
                    SELECT module_id FROM learning_modules WHERE course_id = ?)",
                "DELETE FROM learning_concept_prerequisites WHERE concept_id IN (\
                    SELECT concept_id FROM learning_concepts WHERE course_id = ?)",
                "DELETE FROM learning_graph_prerequisites WHERE course_id = ?",
                "DELETE FROM learning_concepts WHERE course_id = ?",
                "DELETE FROM learning_modules WHERE course_id = ?",
            ];
            for sql in content_sql {
                sqlx::query(sql)
                    .bind(course_id.as_str())
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
            }
        }
        sqlx::query("DELETE FROM learning_course_tags WHERE course_id = ?")
            .bind(course_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM learning_courses WHERE course_id = ?")
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
            sqlx::query_scalar("SELECT version FROM learning_courses WHERE title = ?")
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
            sqlx::query_scalar("SELECT course_id FROM learning_courses WHERE title = ?")
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
                    "learning_mastery_states",
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
                "DELETE FROM learning_activity_concepts WHERE activity_id IN (\
                    SELECT a.activity_id FROM learning_activities a \
                    JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
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
                "DELETE FROM learning_lesson_concepts WHERE lesson_id IN (\
                    SELECT l.lesson_id FROM learning_lessons l \
                    JOIN learning_modules m ON m.module_id = l.module_id \
                    WHERE m.course_id = ?)",
                "DELETE FROM learning_lessons WHERE module_id IN (\
                    SELECT module_id FROM learning_modules WHERE course_id = ?)",
                "DELETE FROM learning_concept_prerequisites WHERE concept_id IN (\
                    SELECT concept_id FROM learning_concepts WHERE course_id = ?)",
                "DELETE FROM learning_concepts WHERE course_id = ?",
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

const MASTERY_RECOMMENDATION_THRESHOLD: f64 = 0.8;

pub(super) fn recommend_next_lesson(
    modules: &[ModuleView],
    concepts: &[ConceptView],
) -> Option<LearningLessonId> {
    if let Some(lesson) = modules
        .iter()
        .flat_map(|module| &module.lessons)
        .find(|lesson| lesson.status == LessonStatus::InProgress)
    {
        return Some(lesson.id.clone());
    }
    let mastery: HashMap<&str, f64> = concepts
        .iter()
        .filter_map(|concept| {
            concept
                .mastery
                .map(|value| (concept.id.as_str(), value))
        })
        .collect();
    let concept_by_id: HashMap<&str, &ConceptView> = concepts
        .iter()
        .map(|concept| (concept.id.as_str(), concept))
        .collect();
    let lessons: Vec<&LessonView> = modules
        .iter()
        .flat_map(|module| &module.lessons)
        .collect();
    for lesson in lessons
        .iter()
        .copied()
        // skipped 与 completed 同为「已满足」：跳过的节点不再作为 next_lesson
        // 推荐（学习图的推荐走图视图就绪集，这里只影响传统课程的防御语义）。
        .filter(|lesson| !lesson.status.satisfies())
    {
        if lesson.concepts.is_empty() {
            return Some(lesson.id.clone());
        }
        let deficient: Vec<&ConceptView> = lesson
            .concepts
            .iter()
            .filter(|concept| {
                mastery.get(concept.as_str()).copied().unwrap_or(0.0)
                    < MASTERY_RECOMMENDATION_THRESHOLD
            })
            .filter_map(|concept| concept_by_id.get(concept.as_str()).copied())
            .collect();
        if deficient.is_empty() {
            continue;
        }
        for prerequisite in deficient
            .iter()
            .flat_map(|concept| &concept.prerequisites)
            .filter(|prerequisite| {
                mastery
                    .get(prerequisite.as_str())
                    .copied()
                    .unwrap_or(0.0)
                    < MASTERY_RECOMMENDATION_THRESHOLD
            })
        {
            if let Some(prerequisite_lesson) = lessons
                .iter()
                .find(|candidate| candidate.concepts.contains(prerequisite))
            {
                return Some(prerequisite_lesson.id.clone());
            }
        }
        return Some(lesson.id.clone());
    }
    None
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
    let mut concept_keys = HashSet::new();
    for concept in &pack.concepts {
        if concept.key.trim().is_empty() || concept.title.trim().is_empty() {
            return Err(AppError::BadRequest(
                "concept key and title are required".into(),
            ));
        }
        if !concept_keys.insert(concept.key.as_str()) {
            return Err(AppError::BadRequest(format!(
                "duplicate concept key: {}",
                concept.key
            )));
        }
    }
    for concept in &pack.concepts {
        for prerequisite in &concept.prerequisites {
            require_concept(&concept_keys, prerequisite)?;
            if prerequisite == &concept.key {
                return Err(AppError::BadRequest(format!(
                    "concept {} cannot require itself",
                    concept.key
                )));
            }
        }
    }
    validate_prerequisite_graph(pack)?;
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
            for concept in &lesson.concepts {
                require_concept(&concept_keys, concept)?;
            }
            for activity in &lesson.activities {
                if activity.prompt.trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "activity prompt is required".into(),
                    ));
                }
                for concept in &activity.concepts {
                    require_concept(&concept_keys, concept)?;
                }
                match activity.kind {
                    ActivityKind::SingleChoice => {
                        let Some(answer) = activity.answer.as_str() else {
                            return Err(AppError::BadRequest(
                                "single_choice answer must be a string".into(),
                            ));
                        };
                        if activity.options.len() < 2
                            || !activity.options.iter().any(|option| option == answer)
                        {
                            return Err(AppError::BadRequest(
                                "single_choice needs at least two options and an answer in options"
                                    .into(),
                            ));
                        }
                    }
                    ActivityKind::TrueFalse => {
                        if !activity.answer.is_boolean() {
                            return Err(AppError::BadRequest(
                                "true_false answer must be boolean".into(),
                            ));
                        }
                    }
                    ActivityKind::Reflection => {}
                    ActivityKind::FillInBlank => {
                        if !activity.prompt.contains("___") {
                            return Err(AppError::BadRequest(
                                "fill_in_blank prompt must contain a ___ blank".into(),
                            ));
                        }
                        let Some(answers) = activity.answer.as_array() else {
                            return Err(AppError::BadRequest(
                                "fill_in_blank answer must be a JSON array of accepted answers"
                                    .into(),
                            ));
                        };
                        if answers.is_empty() || answers.len() > 3 {
                            return Err(AppError::BadRequest(
                                "fill_in_blank must have 1-3 accepted answers".into(),
                            ));
                        }
                        if answers.iter().any(|accepted| {
                            !accepted
                                .as_str()
                                .is_some_and(|text| !text.trim().is_empty())
                        }) {
                            return Err(AppError::BadRequest(
                                "fill_in_blank accepted answers must be non-empty strings".into(),
                            ));
                        }
                        if activity.distractors.is_empty() {
                            return Err(AppError::BadRequest(
                                "fill_in_blank needs at least one distractor".into(),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Visited,
}

fn validate_prerequisite_graph(pack: &CoursePack) -> Result<(), AppError> {
    let prerequisites: HashMap<&str, Vec<&str>> = pack
        .concepts
        .iter()
        .map(|concept| {
            (
                concept.key.as_str(),
                concept
                    .prerequisites
                    .iter()
                    .map(String::as_str)
                    .collect(),
            )
        })
        .collect();
    let mut states = HashMap::new();
    for concept in prerequisites.keys() {
        visit_concept(concept, &prerequisites, &mut states)?;
    }
    Ok(())
}

fn visit_concept<'a>(
    concept: &'a str,
    prerequisites: &HashMap<&'a str, Vec<&'a str>>,
    states: &mut HashMap<&'a str, VisitState>,
) -> Result<(), AppError> {
    match states.get(concept) {
        Some(VisitState::Visited) => return Ok(()),
        Some(VisitState::Visiting) => {
            return Err(AppError::BadRequest(format!(
                "concept prerequisite cycle contains {concept}"
            )));
        }
        None => {}
    }
    states.insert(concept, VisitState::Visiting);
    for prerequisite in &prerequisites[concept] {
        visit_concept(prerequisite, prerequisites, states)?;
    }
    states.insert(concept, VisitState::Visited);
    Ok(())
}

fn require_concept(concepts: &HashSet<&str>, key: &str) -> Result<(), AppError> {
    if concepts.contains(key) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "unknown concept key: {key}"
        )))
    }
}

fn course_summary_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<CourseSummary, AppError> {
    let source_kb_id = row
        .try_get::<Option<String>, _>("source_kb_id")
        .map_err(internal)?
        .map(parse_id)
        .transpose()?;
    Ok(CourseSummary {
        id: parse_id(row.try_get("course_id").map_err(internal)?)?,
        title: row.try_get("title").map_err(internal)?,
        description: row.try_get("description").map_err(internal)?,
        domain: row.try_get("domain").map_err(internal)?,
        course_kind: CourseKind::try_from(row.try_get::<String, _>("course_kind").map_err(internal)?.as_str())
            .map_err(AppError::Internal)?,
        source_kb_id,
        version: row.try_get("version").map_err(internal)?,
        enrolled: row.try_get::<i64, _>("enrolled").map_err(internal)? != 0,
        total_lessons: row.try_get("total_lessons").map_err(internal)?,
        completed_lessons: row.try_get("completed_lessons").map_err(internal)?,
        updated_at: row.try_get("updated_at").map_err(internal)?,
        tags: Vec::new(),
    })
}
