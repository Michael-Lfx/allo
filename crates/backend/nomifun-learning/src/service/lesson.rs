use super::*;

/// 结构快照里的程度声明回执（内容生成的项目级 grounding；learning_scope 列
/// 随结构改造退役——快照标量是新的单源）。无快照或标量为空返回空串。
///
/// 边界说明（2026-10-13 评审裁定）：回执标量属**用户输入面**的记录，不是
/// 结构语义面——读取它不构成「结构供内容」违例（CONTEXT.md Structure 豁免注记）。
fn structure_degree_statement(snapshot: Option<String>) -> String {
    snapshot
        .as_deref()
        .and_then(|snapshot| serde_json::from_str::<serde_json::Value>(snapshot).ok())
        .and_then(|envelope| {
            envelope
                .get("structure")
                .and_then(|structure| structure.get("proficiency_statement"))
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// 课时检索的整步预算（t3，票面 20s）：检索 + 扩读在这个预算内完成，超时
/// 按「检索不可用」降级——只丢补充片段，不阻断课时生成。
const LESSON_RETRIEVAL_BUDGET: std::time::Duration = std::time::Duration::from_secs(20);

/// 检索命中的候选上限：命中按分降序，预算（合计 40KB）通常只装得下前几个
/// 全文片段；多读的命中不白读——它们已经进不了预算。
const LESSON_RETRIEVAL_HITS: usize = 5;

/// 课时 grounding 的装配结果（t3）：片段清单 + 读数（命中数/扩读数/是否
/// 降级），后者进进程内日志与 WS 事件——「课时写作真的接上库了吗」有据可查。
struct LessonGrounding {
    /// primary 在前（大纲引用的 source），其后是按相关度排序的补充片段。
    fragments: Vec<LessonExcerpt>,
    /// `search_bases` 返回的命中数（0 = 未检索或无命中）。
    hits: usize,
    /// 读入的补充片段数。
    supplements: usize,
    /// primary 是否来自实时读库（false = 快照兜底）。
    primary_live: bool,
    /// 是否发生降级（检索失败/超时/主 source 读不到，快照兜底）。
    degraded: bool,
}

/// 课时检索查询词（t3）：课时标题 + purpose 经 t2 的确定性切片器成词条，
/// 再追加目标词条集（建课时切好、随采样披露持久化——建课后稳定复用）。
/// 词条以空格连接交给 `search_bases`（检索侧按空白再切）；标题/purpose 在前，
/// 长目标挤不进词条上限时不会把课时自身的词条挤掉。
fn lesson_retrieval_terms(lesson_title: &str, purpose: &str, goal_terms: &[String]) -> Vec<String> {
    let mut terms = crate::generation::term_slices(&format!("{lesson_title} {purpose}"));
    for term in goal_terms {
        if terms.len() >= crate::generation::TERM_MAX_COUNT {
            break;
        }
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.clone());
        }
    }
    terms
}

/// 目标词条集（t3）：优先取采样披露里持久化的词条集（建课时的确定性切片，
/// 建课后稳定复用）；旧课程（披露里没有该字段）对目标文本重新切片——同一
/// 纯函数、结果一致；无目标则为空。
fn lesson_goal_terms(disclosure: Option<&SampleDisclosure>, goal: Option<&str>) -> Vec<String> {
    match disclosure {
        Some(disclosure) if !disclosure.goal_terms.is_empty() => disclosure.goal_terms.clone(),
        _ => goal.map(crate::generation::term_slices).unwrap_or_default(),
    }
}

/// 补充片段按路径字典序交付（课程式库专项：primary 读源文件全文 + 按结构序
/// 扩展，不因检索结果改写阅读线）。**全部** primary 在前不动（多引用课时
/// 按引用位序排列），其余补充按路径排序。
fn sort_supplements_by_path(fragments: &mut [LessonExcerpt]) {
    let primary_len = fragments
        .iter()
        .take_while(|fragment| fragment.role == ExcerptRole::Primary)
        .count();
    fragments[primary_len..].sort_by(|left, right| left.path.cmp(&right.path));
}

/// 检索扩读（t3）：`search_bases` 取相关文件命中 → 逐条 `read_file` 读全文 →
/// 补充片段（带库归属与命中章节，可溯源）。`cited` 是课时的**全部**主引用
/// `(kb_id, 库内相对路径)`——其中任一命中时跳过（它们已是 primary——锚定
/// 纪律：检索不替换主引用，也不重复占用预算；多引用课时逐条排除）。单文件
/// 读不到只跳过该条；检索本身失败向上传播（调用方降级为「无补充」，不阻断
/// 生成）。
///
/// 多知识库（t4）：检索与扩读天然多库——`search_bases` 收全量来源库，命中
/// 自带 `kb_id`/`kb_name`（补充片段按命中库归属，不必再猜）。
async fn read_lesson_supplements(
    knowledge: &KnowledgeService,
    kb_ids: &[KnowledgeBaseId],
    query: &str,
    cited: &[(&str, &str)],
) -> Result<(Vec<LessonExcerpt>, usize), AppError> {
    if kb_ids.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let hits = knowledge.search_bases(kb_ids, query, LESSON_RETRIEVAL_HITS).await?;
    let hit_count = hits.len();
    let mut supplements = Vec::with_capacity(hit_count);
    for hit in hits {
        // 主 source 自身命中：跳过（锚定纪律，按 (库, 库内路径) 判等）。
        if cited
            .iter()
            .any(|(kb_id, rel)| hit.kb_id.as_str() == *kb_id && hit.rel_path == *rel)
        {
            continue;
        }
        match knowledge.read_file(hit.kb_id.as_str(), &hit.rel_path).await {
            Ok(file) if !file.content.trim().is_empty() => supplements.push(LessonExcerpt {
                path: hit.rel_path,
                text: file.content,
                kb_id: Some(hit.kb_id.as_str().to_owned()),
                kb_name: Some(hit.kb_name),
                heading: Some(hit.heading).filter(|heading| !heading.trim().is_empty()),
                role: ExcerptRole::Supplement,
            }),
            // 命中文件读不到：跳过该条，不降级整步（主引用仍在）。
            Ok(_) | Err(_) => continue,
        }
    }
    Ok((supplements, hit_count))
}

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

    /// 课时 grounding 装配（t3「课时写作接回知识库」；多库 t4）：
    /// - primary = 大纲引用的 source 文件**全文**（`read_file` 实时读库，替代
    ///   快照里冻结的 8KB 前缀）；多库课程的 source 路径带 `{库名}/rel` 命名
    ///   空间——按 [`SourceSpan::kb_id`]（蓝图已标注）解析到来源库并还原库内
    ///   相对路径；读不到（库不可达/文件被删/读出空）降级为快照摘录——
    ///   **锚定纪律**：primary 永不被检索结果替换；
    /// - 补充 = 检索扩读：课时标题 + purpose 词条 + 目标词条集 → `search_bases`
    ///   （全量来源库）→ top-N 命中读全文（命中自带库归属），只作补充片段；
    /// - 检索失败/超时（20s 预算）→ 只丢补充片段，不阻断生成。
    /// 片段按优先级（primary 在前）过预算守卫（[`enforce_grounding_budget`]）。
    async fn assemble_lesson_grounding(
        &self,
        lesson: &crate::generation::BlueprintLesson,
        samples: &[(String, String)],
        lesson_title: &str,
        purpose: &str,
        goal_terms: &[String],
        sources: &CourseKbSources,
    ) -> LessonGrounding {
        let mut grounding = LessonGrounding {
            fragments: Vec::new(),
            hits: 0,
            supplements: 0,
            primary_live: false,
            degraded: false,
        };
        // 实时读库与检索都需要注入的知识服务与课程的来源库；二者缺一
        // （描述流/测试构造）只有快照可用，且不算降级——现状行为。
        let knowledge = self.injected_knowledge_service().ok();
        // 检索查询词（t3）：课时标题 + purpose 词条 + 目标词条集；同一套词条
        // 也用于超预算片段的相关度选摘（预算守卫）。
        let retrieval_terms = lesson_retrieval_terms(lesson_title, purpose, goal_terms);
        // primary 的来源库（t4；评审修复）：命名空间归属优先（路径是事实），
        // 缺省用蓝图 source 标注的 kb_id——两者都要落到**存活**来源库上。
        // 解析不到（已删库/未知前缀）＝无实时来源：走快照兜底，绝不回退到
        // 幸存库去读一个属于别的库的路径（读错库比读不到更坏）。单库/旧课程
        // 的裸路径由 `base_for_sample_path` 的唯一来源库回退承担。`cited` 保留
        // 蓝图里的原始路径——快照兜底按它取摘录（快照键与蓝图引用同源）。
        // 多引用（t4 评审补齐）：**每一条**被引用文件各成一个 primary 片段，
        // 位序 = 引用顺序（首条仍是主来源）；预算守卫统一封顶 40KB。
        let primary_sources: Vec<(Option<KnowledgeBaseId>, String, String)> = lesson
            .cited_sources()
            .into_iter()
            .map(|source| {
                let kb_id = base_for_sample_path(&source.path, &sources.bases, &sources.ids)
                    .or_else(|| source.kb_id.clone())
                    .filter(|id| sources.ids.iter().any(|candidate| candidate == id));
                let rel = kb_id
                    .as_ref()
                    .map(|id| rel_path_for_base(&source.path, namespace_for_base(&sources.bases, id)))
                    .unwrap_or_else(|| rel_path_for_base(&source.path, None));
                (kb_id, rel.to_owned(), source.path.clone())
            })
            .collect();
        // 逐条读库：实时读到 = primary 片段带库归属；读不到 = 快照兜底片段。
        // `primary_live` 只有在**全部**引用都读到实时文件时才成立（读数帧
        // 如实披露），任一读失败都算降级（库不可达/文件被删）。
        let mut primary_live = !primary_sources.is_empty();
        for (kb_id, rel, cited) in &primary_sources {
            let kb_name = match (knowledge.as_ref(), kb_id.as_ref()) {
                (Some(knowledge), Some(kb_id)) => knowledge
                    .get_base_info(kb_id.as_str())
                    .await
                    .ok()
                    .map(|base| base.name),
                _ => None,
            };
            let live = match (knowledge.as_ref(), kb_id.as_ref()) {
                (Some(knowledge), Some(kb_id)) => tokio::time::timeout(
                    LESSON_RETRIEVAL_BUDGET,
                    knowledge.read_file(kb_id.as_str(), rel),
                )
                .await
                .ok()
                .and_then(Result::ok),
                _ => None,
            };
            match live {
                Some(file) if !file.content.trim().is_empty() => {
                    grounding.fragments.push(LessonExcerpt {
                        path: rel.clone(),
                        text: file.content,
                        kb_id: kb_id.as_ref().map(KnowledgeBaseId::as_str).map(str::to_owned),
                        kb_name,
                        heading: None,
                        role: ExcerptRole::Primary,
                    });
                }
                _ => {
                    // 库不可达/文件被删：快照兜底（现状能力保留），披露降级。
                    primary_live = false;
                    if knowledge.is_some() {
                        grounding.degraded = true;
                        tracing::warn!(
                            kb_id = kb_id.as_ref().map(KnowledgeBaseId::as_str).unwrap_or_default(),
                            path = %rel,
                            "live lesson source read failed; falling back to the sampled snapshot"
                        );
                    }
                    if let Some(text) = samples
                        .iter()
                        .find(|(path, _)| path == cited)
                        .map(|(_, text)| text.clone())
                    {
                        grounding.fragments.push(LessonExcerpt::text_only(
                            rel.clone(),
                            text,
                        ));
                    }
                }
            }
        }
        grounding.primary_live = primary_live;
        if let Some(knowledge) = &knowledge {
            if !retrieval_terms.is_empty() && !sources.ids.is_empty() {
                // 全部主引用都从补充检索里排除（锚定纪律：不重复占用预算）。
                let cited: Vec<(&str, &str)> = primary_sources
                    .iter()
                    .filter_map(|(kb_id, rel, _)| {
                        kb_id.as_ref().map(|kb_id| (kb_id.as_str(), rel.as_str()))
                    })
                    .collect();
                let retrieved = tokio::time::timeout(
                    LESSON_RETRIEVAL_BUDGET,
                    read_lesson_supplements(
                        knowledge,
                        &sources.ids,
                        &retrieval_terms.join(" "),
                        &cited,
                    ),
                )
                .await;
                match retrieved {
                    // 检索失败不阻断生成：补充片段是可选的加分项。
                    Ok(Ok((supplements, hits))) => {
                        grounding.hits = hits;
                        grounding.fragments.extend(supplements);
                    }
                    Ok(Err(error)) => {
                        grounding.degraded = true;
                        tracing::warn!(
                            kb_ids = ?sources.ids,
                            error = %error,
                            "lesson retrieval failed; continuing without supplementary fragments"
                        );
                    }
                    Err(_) => {
                        grounding.degraded = true;
                        tracing::warn!(
                            kb_ids = ?sources.ids,
                            budget_s = LESSON_RETRIEVAL_BUDGET.as_secs(),
                            "lesson retrieval exceeded its budget; continuing without supplementary fragments"
                        );
                    }
                }
            }
        }
        grounding.fragments = crate::generation::enforce_grounding_budget(
            grounding.fragments,
            &retrieval_terms,
        );
        // 不倒退纪律：相关度决定读什么（选摘与预算丢弃都按分），结构决定按
        // 什么顺序读——交付顺序回到路径字典序，阅读线不因检索结果被打乱；
        // primary 恒在首位（锚定纪律）。
        sort_supplements_by_path(&mut grounding.fragments);
        // 预算可能丢掉了补充片段：读数与片段清单保持一致。
        grounding.supplements = grounding
            .fragments
            .iter()
            .filter(|fragment| fragment.role == ExcerptRole::Supplement)
            .count();
        grounding
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
            "SELECT l.module_id, l.position, l.content_generated, l.title AS lesson_title, m.course_id, \
                    m.position AS module_position, c.course_kind, c.teaching_style, \
                    c.deleted_at, c.enabled \
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

        // 安排面守卫（三轮 g3「现存且启用」）：已删课时的课程行只剩墓碑 →
        // 404；归档课的内容生成入口给人面错误（可恢复），不再烧生成预算。
        if row.try_get::<Option<i64>, _>("deleted_at").map_err(internal)?.is_some() {
            let course_id: String = row.try_get("course_id").map_err(internal)?;
            return Err(AppError::NotFound(format!("course {course_id}")));
        }
        if row.try_get::<i64, _>("enabled").map_err(internal)? == 0 {
            return Err(AppError::Conflict(
                "课程已归档，恢复后可继续学习".into(),
            ));
        }

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
        // 课时内容生成为长任务（三轮 t2 注册面：快照可见、断开请求即中断
        // ——循环内取消接线的显式差异见 ADR-0004，取消请求如实拒绝）。
        let lesson_title: String =
            row.try_get("lesson_title").map_err(internal)?;
        let mut task = self.begin_generation_task(
            crate::service::KIND_LESSON,
            course_id.as_str(),
            Some(lesson_id.as_str()),
            &format!("课时内容 · {lesson_title}"),
        )?;
        task.set_step("generate", "正在生成本课时的分节正文与检索练习…");
        if row.try_get::<String, _>("course_kind").map_err(internal)?
            == CourseKind::LearningGraph.as_str()
        {
            let result = self
                .generate_graph_lesson_content(user_id, lesson_id, &course_id, request)
                .await;
            return match result {
                Ok(view) => {
                    task.finish_done("课时内容已发布");
                    Ok(view)
                }
                Err(error) => {
                    task.finish_failed(error.to_string());
                    Err(error)
                }
            };
        }

        let snapshot = sqlx::query(
            "SELECT title, blueprint_json, samples_json, learning_goal, source_kb_id, source_kb_ids, \
                    samples_meta_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let course_title: String = snapshot.try_get("title").map_err(internal)?;
        // 目标（t1，传统课时）：课程行 learning_goal 的全文进生成上下文；
        // 旧课程列上为空串、语义为空（None），提示词不出现目标段。
        let course_goal: Option<String> = snapshot
            .try_get::<Option<String>, _>("learning_goal")
            .map_err(internal)?
            .map(|goal| goal.trim().to_owned())
            .filter(|goal| !goal.is_empty());
        // 来源库（t3/t4）：课时写作实时读库的目标库（多库为全量过滤后列表，
        // 已删库 id 不外泄）；描述流课程为空。逐库归属承载命名空间解析。
        let existing = self.existing_knowledge_base_ids().await?;
        let source_kbs = CourseKbSources::from_course_columns(&snapshot)?.filtered(&existing);
        let samples_meta_json: Option<String> =
            snapshot.try_get("samples_meta_json").map_err(internal)?;
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
        // 采样披露（t2/t3）：目标词条集的持久化落点。披露读不出（旧快照/坏
        // JSON）只丢词条集来源，不阻断生成——目标文本会重新切片兜底。
        let disclosure: Option<SampleDisclosure> = match samples_meta_json.as_deref() {
            Some(json) => match serde_json::from_str(json) {
                Ok(disclosure) => Some(disclosure),
                Err(error) => {
                    tracing::warn!(%error, "stored sampling disclosure is invalid; ignoring it");
                    None
                }
            },
            None => None,
        };

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

        // grounding（t3）：primary = 主 source 全文（实时读库），补充 = 检索
        // 扩读；快照摘录降级为兜底。锚定纪律：检索永不替换主 source。
        // 多库（t4）：primary 按 source 的库归属读（命名空间去前缀），检索与
        // 扩读覆盖全量来源库。
        let goal_terms = lesson_goal_terms(disclosure.as_ref(), course_goal.as_deref());
        let grounding = self
            .assemble_lesson_grounding(
                lesson,
                &samples,
                &lesson.title,
                &lesson.purpose,
                &goal_terms,
                &source_kbs,
            )
            .await;
        // 读数（票面「读数」项）：命中数/扩读文件数/是否降级——课时写作是否
        // 真的接上了库，事后可核对（WS 事件 + 进程内日志）。
        self.emit_lesson_event(serde_json::json!({
            "phase": "grounding",
            "lesson_id": lesson_id.as_str(),
            "primary_live": grounding.primary_live,
            "hits": grounding.hits,
            "supplements": grounding.supplements,
            "degraded": grounding.degraded,
        }));
        tracing::info!(
            lesson_id = lesson_id.as_str(),
            primary_live = grounding.primary_live,
            hits = grounding.hits,
            supplements = grounding.supplements,
            degraded = grounding.degraded,
            "lesson grounding assembled"
        );
        let excerpts = grounding.fragments;
        let excerpt: Option<LessonExcerpt> = excerpts.first().cloned();
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
            excerpts,
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
            goal: course_goal,
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
                        "est": output.estimated_minutes,
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
                    &crate::generation::render_grounding_fragments(&context.excerpts),
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
                            "est": output.estimated_minutes,
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
                        task.finish_failed(format!("lesson '{}' failed", lesson.title));
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
        task.finish_done("课时内容已发布");

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
            "SELECT l.title, l.purpose, l.position, l.estimated_minutes, \
                    m.title AS module_title \
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
        // 结构阶段给的节点尺寸锚（k03）：列 NOT NULL DEFAULT 10，恒有值。
        let estimated_minutes: i64 = lesson.try_get("estimated_minutes").map_err(internal)?;
        let module_title: String = lesson.try_get("module_title").map_err(internal)?;

        let course = sqlx::query(
            "SELECT title, learning_goal, structure_json FROM learning_courses WHERE course_id = ?",
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
        let degree_statement = structure_degree_statement(
            course.try_get("structure_json").map_err(internal)?,
        );

        // 一次载入概念网口径的拓扑上下文（前置 = assumes 的概念与跨课程
        // 教它的节点；禁止清单 = 下游节点标题；概念面 = 首现/引用名/别名池
        // 投影，j07）。
        let topology = self.graph_node_topology(course_id, lesson_id).await?;

        let context = LessonGenerationContext {
            lesson_id: lesson_id.as_str().to_owned(),
            course_title,
            // 图节点没有课程简报；目标/范围在 graph 段渲染。
            course_description: String::new(),
            module_title,
            module_index: 0,
            lesson_title,
            lesson_index: lesson_position.max(0) as usize,
            total_lessons: topology.total_nodes,
            // 衔接语义由 graph.upcoming_nodes 承担。
            next_lesson_title: None,
            purpose,
            excerpt: None,
            // 学习图节点不读库：grounding 走到课程目标/范围/前置段落为止。
            excerpts: Vec::new(),
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: Some(GraphLessonContext {
                goal,
                degree_statement,
                prerequisite_path: topology.prerequisite_path,
                upcoming_nodes: topology.upcoming_nodes,
                concept_faces: topology.concept_faces,
                est_minutes: Some(estimated_minutes),
            }),
            // 学习图节点的目标在 graph.goal；本字段只承载传统课程目标。
            goal: None,
            // 防超纲黑名单：下游节点标题清单（概念网反查，ADR-0009）。
            forbidden_concepts: topology.forbidden_concepts,
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
                "est": output.estimated_minutes,
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
    /// - 末位 = 本课程学习节点总数（终点标记课时除外）；
    /// - 概念面 = 首现/非首现（祖先闭包不动点）+ 引用名 + 活跃别名池
    ///   （j07 读侧投影，结构化进上下文、渲染进提示词、审计观察条共用）。
    async fn graph_node_topology(
        &self,
        course_id: &LearningCourseId,
        lesson_id: &LearningLessonId,
    ) -> Result<GraphNodeTopology, AppError> {
        // 1) 本课声明的概念（teaches/assumes，登记表 canonical + 档位 +
        //    definition——definition 随铸名落库，是零正文场景下的概念摘要；
        //    ref_name = 挂接时模型实际用名，j07 引用名开讲的数据源）。
        let declares: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT lc.role, reg.canonical, lc.tier, reg.definition, lc.ref_name \
             FROM learning_lesson_concepts lc \
             JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
             WHERE lc.lesson_id = ? ORDER BY reg.canonical",
        )
        .bind(lesson_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut teaches: Vec<String> = Vec::new();
        let mut assumes: Vec<(String, String, String, Option<String>)> = Vec::new();
        for (role, canonical, tier, definition, ref_name) in declares {
            match role.as_str() {
                "teaches" => teaches.push(canonical),
                _ => assumes.push((canonical, tier, definition, ref_name)),
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
            for (concept, _, _, _) in &assumes {
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

        // 3) 后续：同课程位于本课之后的节点，按 position 升序——衔接参照，
        //    不是依赖关系。
        let upcoming_rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT l.position, l.title FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? \
               AND l.position > (SELECT position FROM learning_lessons WHERE lesson_id = ?) \
             ORDER BY l.position, l.lesson_id",
        )
        .bind(course_id.as_str())
        .bind(lesson_id.as_str())
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

        // 5) 本课程学习节点总数。
        let total_nodes: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;

        // 6) 概念面（j07）：首现闭包 + 引用名 + 别名池。本节点无概念挂接
        //    （teaches/assumes 双空）时整体缺席。
        let concept_faces = if teaches.is_empty() && assumes.is_empty() {
            None
        } else {
            // 祖先闭包候选：全图其余节点的 teaches/assumes（现存且启用课程
            // ——已删墓碑与归档课不参与闭包，archived 不夺首现；并行分支因
            // 闭包只沿 assumes 链生长而天然不夺）。
            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT lc.lesson_id, lc.role, reg.canonical \
                 FROM learning_lesson_concepts lc \
                 JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
                 JOIN learning_modules m ON m.module_id = l.module_id \
                 JOIN learning_courses c ON c.course_id = m.course_id \
                 JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
                 WHERE lc.lesson_id <> ? \
                   AND c.deleted_at IS NULL AND c.enabled = 1",
            )
            .bind(lesson_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
            let mut candidates: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
            for (node, role, canonical) in rows {
                let entry = candidates.entry(node).or_default();
                if role == "teaches" {
                    entry.0.push(canonical);
                } else {
                    entry.1.push(canonical);
                }
            }
            let nodes: Vec<(String, Vec<String>, Vec<String>)> = candidates
                .into_iter()
                .map(|(node, (teaches, assumes))| (node, teaches, assumes))
                .collect();
            let assume_names: Vec<String> = assumes
                .iter()
                .map(|(canonical, _, _, _)| canonical.clone())
                .collect();
            let first_seen = first_appearance_set(&teaches, &assume_names, &nodes);
            let returning: Vec<String> = teaches
                .iter()
                .filter(|concept| !first_seen.contains(concept))
                .cloned()
                .collect();
            // 引用名：本节点挂接行里与正名不同的用名（正文以引用名开讲）。
            let ref_names: Vec<(String, String)> = assumes
                .iter()
                .filter_map(|(canonical, _, _, ref_name)| match ref_name {
                    Some(reference)
                        if !reference.trim().is_empty() && reference.trim() != canonical =>
                    {
                        Some((reference.trim().to_owned(), canonical.clone()))
                    }
                    _ => None,
                })
                .collect();
            // 投影词表（R3）：本节点概念（teaches ∪ assumes）的活跃别名。
            let mut all_concepts: Vec<String> = teaches.clone();
            all_concepts.extend(assumes.iter().map(|(canonical, _, _, _)| canonical.clone()));
            let aliases = self.active_aliases_for(&all_concepts).await?;
            let faces = crate::lesson_draft::GraphConceptFaces {
                first_seen,
                returning,
                ref_names,
                aliases,
            };
            (!faces.is_empty()).then_some(faces)
        };

        Ok(GraphNodeTopology {
            prerequisite_path,
            upcoming_nodes,
            forbidden_concepts,
            total_nodes: total_nodes.max(0) as usize,
            concept_faces,
        })
    }

    /// 一组正名概念的活跃别名（R3 投影词表；退役别名不占池）。正名 →
    /// 别名列表，仅多名概念入表。
    async fn active_aliases_for(
        &self,
        canonicals: &[String],
    ) -> Result<Vec<(String, Vec<String>)>, AppError> {
        if canonicals.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = sqlx::QueryBuilder::new(
            "SELECT reg.canonical, reg.aliases_json FROM learning_concept_registry reg \
             WHERE reg.canonical IN (",
        );
        let mut separated = query.separated(", ");
        for canonical in canonicals {
            separated.push_bind(canonical);
        }
        query.push(")");
        let rows = query.build().fetch_all(&self.pool).await.map_err(internal)?;
        let mut pool = Vec::new();
        for row in rows {
            let canonical: String = row.try_get("canonical").map_err(internal)?;
            let raw: String = row.try_get("aliases_json").map_err(internal)?;
            let names: Vec<String> =
                crate::learning_graph::alias_records::parse_alias_records(&raw)
                    .iter()
                    .filter(|record| {
                        record.status == crate::learning_graph::alias_records::ALIAS_ACTIVE
                    })
                    .map(|record| record.name.clone())
                    .collect();
            if !names.is_empty() {
                pool.push((canonical, names));
            }
        }
        pool.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(pool)
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
        // 图路径带概念面段（首现/引用名/别名池——单节重写与整卡生成同一
        // 名相口径，j07）。
        let (grounding, forbidden): (Option<(String, String)>, String) =
            if course_kind == CourseKind::LearningGraph.as_str() {
                let topology = self.graph_node_topology(&course_id, lesson_id).await?;
                let course = sqlx::query(
                    "SELECT learning_goal, structure_json FROM learning_courses WHERE course_id = ?",
                )
                .bind(course_id.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(internal)?;
                let goal: String = course
                    .try_get::<Option<String>, _>("learning_goal")
                    .map_err(internal)?
                    .unwrap_or_default();
                let degree_statement = structure_degree_statement(
                    course.try_get("structure_json").map_err(internal)?,
                );
                let mut context_text =
                    format!("学习目标：{}\n程度声明：{}", goal.trim(), degree_statement.trim());
                if !topology.prerequisite_path.trim().is_empty() {
                    context_text.push_str(&format!(
                        "\n\n前置路径（学习者已掌握，不要重复讲授）：\n{}",
                        topology.prerequisite_path
                    ));
                }
                if let Some(faces) = &topology.concept_faces {
                    context_text.push_str(&format!("\n\n{}", faces.render_text()));
                }
                (
                    Some(("学习图节点上下文".into(), context_text)),
                    topology.forbidden_concepts,
                )
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
            .cited_sources()
            // 传统（非引擎）路径只有一个 excerpt 槽：取主来源（首个引用）。
            .first()
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
        // 超容自报（k03）：观察行，非门禁——如实留痕便于真机对照卡与事后
        // 盘点（learnhub over_capacity 日志行同款）。
        if let Some(note) = output.capacity_note.as_deref() {
            tracing::warn!(
                lesson_id = lesson_id.as_str(),
                estimated_minutes = output.estimated_minutes,
                capacity_note = note,
                "over_capacity"
            );
        }
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
                evidence: activity
                    .evidence
                    .as_deref()
                    .map(str::trim)
                    .filter(|evidence| !evidence.is_empty())
                    .map(str::to_owned),
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
                let module = blueprint
                    .modules
                    .get(module_position as usize)
                    .ok_or_else(|| AppError::Internal("outline module position out of range".into()))?;
                let lesson = module
                    .lessons
                    .get(lesson_position as usize)
                    .ok_or_else(|| AppError::Internal("outline lesson position out of range".into()))?;
                let excerpt = lesson
                    .cited_sources()
                    // 这里的 excerpt 只有一个槽：取主来源（首个引用）。
                    .first()
                    .and_then(|source| {
                        samples
                            .iter()
                            .find(|(path, _)| path == &source.path)
                            .map(|(_, excerpt)| excerpt.clone())
                    })
                    .unwrap_or_default();
                excerpt
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
    pub(super) async fn existing_lesson_questions(
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

/// 学习图节点内容生成上下文的拓扑面（`graph_node_topology` 的产物）：
/// 前置/后续/禁止清单的预渲染文本 + 概念面投影（j07）。
pub(crate) struct GraphNodeTopology {
    pub prerequisite_path: String,
    pub upcoming_nodes: String,
    pub forbidden_concepts: String,
    pub total_nodes: usize,
    pub concept_faces: Option<crate::lesson_draft::GraphConceptFaces>,
}

/// 首现判定（二轮 g2 / j07，纯函数——SQL 收集在调用方）：祖先闭包不动点。
/// 起点 = 本节点 assumes；循环扫候选节点，凡 teaches 命中当前扫描集即纳入、
/// 并入其 assumes、teaches 计入祖先已教集合，直至不动点。本节点 teaches 中
/// 不在祖先已教集合的 = 首现。并行分支因闭包只沿 assumes 链生长而天然不
/// 夺首现；archived 节点（归档/已删课程）由调用方过滤后不参与扫描。
fn first_appearance_set(
    self_teaches: &[String],
    self_assumes: &[String],
    candidates: &[(String, Vec<String>, Vec<String>)],
) -> Vec<String> {
    let mut scan_set: std::collections::HashSet<&str> =
        self_assumes.iter().map(String::as_str).collect();
    let mut ancestor_taught: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut included: std::collections::HashSet<&str> = std::collections::HashSet::new();
    loop {
        let mut changed = false;
        for (node, teaches, assumes) in candidates {
            let hits = teaches.iter().any(|concept| scan_set.contains(concept.as_str()));
            if hits && included.insert(node.as_str()) {
                for concept in teaches {
                    ancestor_taught.insert(concept.as_str());
                }
                for concept in assumes {
                    scan_set.insert(concept.as_str());
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    self_teaches
        .iter()
        .filter(|concept| !ancestor_taught.contains(concept.as_str()))
        .cloned()
        .collect()
}

/// 前置段渲染（概念网口径，ADR-0009）：开头一条档位契约（概念已教到标注
/// 档位——不要重新讲授，只在需要时按该档位复述或引用；档位即停讲深度的
/// 标尺），逐条 assumes 概念列出「概念 @档位」+ 登记表 definition + 跨课程
/// 教它的节点（标题/课程 + 出生即带的 purpose + 已生成节的标题要点摘要）。
/// 零正文场景（教师节点尚未生成内容）不削弱前置段：purpose 与 definition
/// 都是出生即有的字段。没有任何节点教的概念按未覆盖兜底提示（正常情况下
/// 结构门已挡掉这种批次）。空串 = 本课零 assumes（第一批基节点）。
/// j07 引用名开讲：挂接行带 ref_name（与正名不同）时行名用引用名、正名
/// 括注——正文以引用名开讲，前置面与概念面同一名相。
fn render_prerequisite_path(
    assumes: &[(String, String, String, Option<String>)],
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
    for (concept, tier, definition, ref_name) in assumes {
        let tier = crate::learning_graph::ConceptTier::try_from_str(tier)
            .map(crate::learning_graph::tier_zh)
            .unwrap_or("?");
        // 引用名开讲（j07）：挂接用名与正名不同 = 行名引用名 + 正名括注。
        let display = match ref_name.as_deref().map(str::trim).filter(|name| {
            !name.is_empty() && *name != concept.as_str()
        }) {
            Some(reference) => format!("{reference}（正名：{concept}）"),
            None => concept.clone(),
        };
        match teachers.get(concept) {
            None => lines.push(format!(
                "- {display}（应已掌握到「{tier}」，但暂无节点教它——不要展开讲授）"
            )),
            Some(nodes) => {
                lines.push(format!("- {display}（学习者应已掌握到「{tier}」）："));
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
        evidence: None,
    };
    pack.validate_shape((2, 5), false)
        .map_err(AppError::BadRequest)?;
    // fill_in_blank keeps its distractor rule through validate_shape; the
    // numeric tol is learner-authored so it rides in the config as given.
    // 手工编辑面没有 evidence 字段：保留原值（t2 证据回引不随编辑抹除）。
    let matches = matching_candidates(&pack);
    let config = StoredActivityConfig {
        options: pack.options,
        answer: pack.answer,
        explanation: pack.explanation,
        distractors: pack.distractors,
        tol: pack.tol,
        matches,
        difficulty: pack.difficulty,
        evidence: None,
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
                None,
            ),
            ("相似三角形".to_owned(), "know".to_owned(), String::new(), None),
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
            None,
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

    /// 引用名开讲（j07）：挂接行 ref_name 与正名不同 = 行名引用名 + 正名
    /// 括注；ref_name 与正名相同（模型直接用了正名）不产生括注。
    #[test]
    fn prerequisite_path_renders_reference_names() {
        let assumes = vec![
            (
                "德尔塔".to_owned(),
                "apply".to_owned(),
                String::new(),
                Some("Delta".to_owned()),
            ),
            (
                "伽马".to_owned(),
                "know".to_owned(),
                String::new(),
                Some("伽马".to_owned()),
            ),
        ];
        let text = render_prerequisite_path(&assumes, &HashMap::new(), &HashMap::new());
        assert!(
            text.contains("- Delta（正名：德尔塔）（应已掌握到「会用」"),
            "{text}"
        );
        // 用名与正名相同：不重复括注。
        assert!(text.contains("- 伽马（"), "{text}");
        assert!(!text.contains("正名：伽马"), "{text}");
    }

    /// 首现判定（祖先闭包不动点）三形态：直系前置已教 = 非首现；间接
    /// （assumes 的 assumes）已教 = 非首现；闭包不可达 = 首现。
    #[test]
    fn first_appearance_walks_the_assumes_closure() {
        // 本节点 teaches [C, D]，assumes [A]；N1 教 A（直系教师）、
        // assumes [B]；N2 教 B、assumes []；N3 教 D 但只被 N1 assume？
        // N3 教 D 且 assumes [X]——D 是否进入闭包取决于链：A →(N1)→ B
        // →(N2)；D 的教师 N3 teaches 不含扫描集成员 → 不纳入 → D 首现。
        let candidates = vec![
            ("n1".to_owned(), vec!["A".to_owned()], vec!["B".to_owned()]),
            ("n2".to_owned(), vec!["B".to_owned()], vec![]),
            ("n3".to_owned(), vec!["D".to_owned()], vec!["X".to_owned()]),
        ];
        let first = first_appearance_set(
            &["C".to_owned(), "D".to_owned()],
            &["A".to_owned()],
            &candidates,
        );
        // C 无任何闭包节点教过 → 首现；D 的教师 N3 不在闭包上（它教的 D
        // 不在扫描集）→ D 也首现——闭包只沿 assumes 链生长。
        assert_eq!(first.len(), 2);

        // 改：N1 也教 D（顺带覆盖）→ D 成为非首现（直系前置顺带已教）。
        let candidates = vec![(
            "n1".to_owned(),
            vec!["A".to_owned(), "D".to_owned()],
            vec!["B".to_owned()],
        )];
        let first = first_appearance_set(
            &["C".to_owned(), "D".to_owned()],
            &["A".to_owned()],
            &candidates,
        );
        assert_eq!(first, vec!["C".to_owned()]);
    }

    /// 并行分支不夺首现：兄弟节点的 teaches 永不进入闭包（闭包只沿
    /// assumes 链生长，兄弟节点不在本节点的祖先链上）。
    #[test]
    fn first_appearance_ignores_parallel_branches() {
        // 本节点 teaches [C] assumes []；并行节点 N1 teaches [C]（兄弟，
        // 无 assumes 链相连）→ 不纳入 → C 首现。
        let candidates = vec![(
            "sibling".to_owned(),
            vec!["C".to_owned()],
            vec![],
        )];
        let first = first_appearance_set(&["C".to_owned()], &[], &candidates);
        assert_eq!(first, vec!["C".to_owned()]);
    }

    /// archived 节点不夺首现：调用方过滤（graph_node_topology 的 SQL 带
    /// deleted_at IS NULL AND enabled = 1），纯函数面验证过滤后的输入 =
    /// 该节点不在候选集即不进闭包。
    #[test]
    fn first_appearance_excludes_filtered_candidates() {
        // archived 教师被调用方剔除后，C 无闭包覆盖 → 首现（archived 不
        // 夺首现）。
        let candidates: Vec<(String, Vec<String>, Vec<String>)> = vec![];
        let first = first_appearance_set(&["C".to_owned()], &["A".to_owned()], &candidates);
        assert_eq!(first, vec!["C".to_owned()]);
    }

    /// 空 assumes：扫描集为空，无节点可纳入 → 全部 teaches 首现。
    #[test]
    fn first_appearance_all_first_with_empty_assumes() {
        let candidates = vec![(
            "n1".to_owned(),
            vec!["C".to_owned()],
            vec![],
        )];
        let first = first_appearance_set(&["C".to_owned()], &[], &candidates);
        assert_eq!(first, vec!["C".to_owned()]);
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
