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

// ── 门面重试环（e4：learnhub contentgen facade 的 MAX_ATTEMPTS 形态移植）──
//
// E2E 实录：瞬断流错误在 provider 层仅一次重试（loop_core 的开流单重试），
// 第二断即 BAD_GATEWAY 裸抛调用方，33s 失败尝试直接回错误页。本环在
// 「课时循环入口」（引擎调用的门面）再兜一层：≤3 次带反馈的重试，错误
// 先过两桶分类——传输/流/超时/空响应归可重试桶，数据性失败原样上抛。

/// 门面重试环的尝试预算（learnhub contentgen `MAX_ATTEMPTS` 同值 3）：LLM
/// 抖动两三次内应收敛；预算含首试。
const LESSON_GENERATE_ATTEMPTS: usize = 3;

/// 瞬断退避基数（learnhub contentgen 层内短退避同值）：300ms × attempt。
/// 只缓冲瞬态，不弃已得——段级幂等跳过（e1）保证已落库的节在重试中保留。
const LESSON_RETRY_BACKOFF_MS: u64 = 300;

/// 单次尝试的显式超时（judge 90s 形态的移植落点之一）：引擎管线自带
/// 600s 总预算（ADR-0004，`TOTAL_TIMEOUT_SECS`），正常路径由它先到且以
/// `Timeout` 变体上抛（可重试桶）；本限时是外加的死人开关，覆盖引擎预算
/// 之外的悬挂面（provider 解析、审计、落库前读）——后台任务脱离请求生命
/// 周期后（e4），任何悬挂都占着一枚生成闸许可，必须有界。600s + 60s 余量。
const LESSON_ATTEMPT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(660);

/// 错误两桶（learnhub infra/llm.rs 口径，e4 对齐）：流解码错误/传输故障/
/// 超时/空响应 = 可重试（重试裁决归门面重试环）；数据性失败（课程快照
/// 缺失、审计门终判、归档冲突等）重试无意义，立即上抛。可重试判定单源，
/// 与 image_analyze 的 `is_retryable` 同形。
fn is_retryable_lesson_error(error: &AppError) -> bool {
    matches!(
        error,
        AppError::BadGateway(_) | AppError::Timeout(_) | AppError::RateLimited
    )
}

/// 同错误熔断（learnhub coach.rs `step_guard` 同款，e4 课时重试环移植）：
/// 同一步骤（此处 = 同一次任务体的生成步）连续 2 次同指纹失败 = 熔断
/// loud，不再烧掉剩余尝试；成功清零。指纹 = 错误 Display 逐字。状态内存
/// 态、作用域 = 一次任务体的重试环（跨提交不持久——崩溃恢复的正确性靠
/// 段级幂等兜底，不靠熔断，learnhub 同注）。
#[derive(Default)]
struct LessonStepBreaker {
    last_fingerprint: Option<String>,
}

impl LessonStepBreaker {
    /// 记一次失败；与上一记同指纹 = 熔断触发（true）。
    fn trips_on(&mut self, fingerprint: &str) -> bool {
        if self.last_fingerprint.as_deref() == Some(fingerprint) {
            return true;
        }
        self.last_fingerprint = Some(fingerprint.to_owned());
        false
    }

    fn clear(&mut self) {
        self.last_fingerprint = None;
    }
}

/// 一次课时生成的重试读数（门面重试环的出口面；smoke content 档瞬断臂
/// 判据数据源 + 进程内诊断）。内存态仅诊断，与生长的 [`GrowthRunStats`]
/// 同作风。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LessonRunStats {
    /// 实际消耗的尝试次数（含首试；成功 = 停在那一次）。
    pub attempts_used: usize,
    /// success | exhausted | circuit_open。
    pub stop_reason: String,
    /// 末次失败的错误指纹（success = None）。
    pub last_error: Option<String>,
}

/// 课时生成任务的目标行投影（`load_lesson_generation_target` 的产物，双模
/// 入口共用同一套守卫）：调度守卫（404/409）过闸后的最小字段集。
struct LessonGenerationTarget {
    course_id: LearningCourseId,
    lesson_title: String,
    /// 学习图节点路径（course_kind == learning_graph）：无蓝图快照，只走
    /// agent 引擎。
    graph: bool,
    /// 课时已有内容（幂等短路面：传统课时的同步臂见此即返回现视图）。
    content_generated: bool,
    teaching_style: TeachingStyle,
    /// 蓝图坐标（传统路径解析模块/课时锚）。
    module_position: i64,
    lesson_position: i64,
}

/// 异步臂的 202 响应体（e4 双模化）：任务键 + 轮询面提示。前端拿键轮询
/// `GET /api/learning/generation-tasks`（或听 `learning.lesson-generation`
/// WS 流），终态后拉课时详情；`?wait=true` 同步臂的调用方（冒烟/脚本）
/// 不受影响。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LessonGenerationAccepted {
    /// 任务键（`lesson:{course_id}:{lesson_id}`；generation-tasks 快照与
    /// 取消端点的定位键）。
    pub task_key: String,
    pub lesson_id: String,
    pub course_id: String,
    /// 登记时点的任务状态快照（queued | running）；此后以轮询为准。
    pub state: String,
    /// 轮询面提示：任务快照端点路径。
    pub poll: &'static str,
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

    /// 课时生成目标行：调度守卫过闸后的最小投影（[`Self::load_lesson_generation_target`]
    /// 产物，双模入口共用同一套 404/409 守卫与幂等读数）。
    async fn load_lesson_generation_target(
        &self,
        lesson_id: &LearningLessonId,
    ) -> Result<LessonGenerationTarget, AppError> {
        let row = sqlx::query(
            "SELECT l.position, l.content_generated, l.title AS lesson_title, m.course_id, \
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

        Ok(LessonGenerationTarget {
            course_id: parse_id(row.try_get("course_id").map_err(internal)?)?,
            lesson_title: row.try_get("lesson_title").map_err(internal)?,
            graph: row.try_get::<String, _>("course_kind").map_err(internal)?
                == CourseKind::LearningGraph.as_str(),
            content_generated: row.try_get::<i64, _>("content_generated").map_err(internal)? != 0,
            teaching_style: TeachingStyle::try_from(
                row.try_get::<String, _>("teaching_style")
                    .map_err(internal)?
                    .as_str(),
            )
            .map_err(AppError::Internal)?,
            module_position: row.try_get("module_position").map_err(internal)?,
            lesson_position: row.try_get("position").map_err(internal)?,
        })
    }

    /// Generate the study document and activities for one on-demand lesson and
    /// persist them, returning the updated lesson view. Idempotent: a lesson
    /// that already has content returns its current view unchanged.
    ///
    /// 同步臂（e4 双模化）：`?wait=true` 查询参数与冒烟/脚本的既有语义——
    /// 分钟级阻塞直至发布，HTTP 断开即中断（历史行为原样保留）。默认入口
    /// 是异步臂 [`Self::start_lesson_generation`]。
    pub async fn generate_lesson_content(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        request: &GenerateLessonRequest,
    ) -> Result<LessonView, AppError> {
        let target = self.load_lesson_generation_target(lesson_id).await?;
        if target.content_generated {
            let enrollment = self.enrollment_id_for(user_id, &target.course_id).await?;
            return self.lesson_view(lesson_id, enrollment.as_ref()).await;
        }
        let task = self.begin_generation_task(
            crate::service::KIND_LESSON,
            target.course_id.as_str(),
            Some(lesson_id.as_str()),
            &format!("课时内容 · {}", target.lesson_title),
        ).await?;
        self.run_lesson_generation_with_task(user_id, lesson_id, request, task, target)
            .await
    }

    /// 异步臂（e4，HTTP 默认）：登记任务 → 立即返回 202 面（任务键 + 轮询
    /// 提示）；生成体在 spawn 出的**脱离任务**里等闸准入、跑完落库——断连
    /// **不取消**（learnhub「分钟级任务一律不占请求生命周期」红线的移植；
    /// 此前两套模型并存：注册表记了任务、HTTP 却同步阻塞，调用方断连即静默
    /// 取消）。登记同步落 queued 记录，满闸时 202 照常秒回（排队在任务侧
    /// 等待）。同键已有在跑/排队任务 = Conflict（防双击双跑，与同步臂同面）；
    /// 课时已生成 = 幂等：任务照常登记，任务体短路 done。
    pub async fn start_lesson_generation(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        request: &GenerateLessonRequest,
    ) -> Result<LessonGenerationAccepted, AppError> {
        let target = self.load_lesson_generation_target(lesson_id).await?;
        let task_key = self.generation_tasks.pre_register(
            crate::service::KIND_LESSON,
            target.course_id.as_str(),
            Some(lesson_id.as_str()),
            &format!("课时内容 · {}", target.lesson_title),
        )?;
        // 登记时点的状态快照（queued | running）进 202 体；此后以轮询为准。
        let state = self
            .generation_tasks
            .snapshot()
            .into_iter()
            .find(|task| task.key == task_key)
            .map(|task| task.state)
            .unwrap_or_else(|| crate::service::STATE_RUNNING.to_owned());
        let course_id = target.course_id.as_str().to_owned();
        let service = self.clone();
        let owner = user_id.clone();
        let lid = lesson_id.clone();
        let req = request.clone();
        let admission_key = task_key.clone();
        tokio::spawn(async move {
            match service.generation_tasks.begin_registered(admission_key).await {
                Ok(task) => {
                    let _ = service
                        .run_lesson_generation_with_task(&owner, &lid, &req, task, target)
                        .await;
                }
                // 排队期取消 / 记录消失：终态已由取消侧或注册表落账，无可收尾。
                Err(error) => {
                    tracing::warn!(%error, "detached lesson generation was never admitted")
                }
            }
        });
        Ok(LessonGenerationAccepted {
            task_key,
            lesson_id: lesson_id.as_str().to_owned(),
            course_id,
            state,
            poll: "/api/learning/generation-tasks",
        })
    }

    /// 双模共用的任务体：任务登记之后的一切（幂等短路面、分流、门面重试
    /// 环、落库、任务收尾）。同步臂在请求生命周期内 await 本方法；异步臂在
    /// 脱离任务里跑它——两条臂从此共享同一份行为。
    async fn run_lesson_generation_with_task(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        request: &GenerateLessonRequest,
        task: crate::service::GenerationTaskGuard,
        target: LessonGenerationTarget,
    ) -> Result<LessonView, AppError> {
        // 幂等短路面（异步臂）：课时已生成（登记到准入的间隙内被并行任务
        // 完成，或重复提交）——如实 done，不重付预算。
        if target.content_generated {
            task.finish_done("课时内容已就绪");
            let enrollment = self.enrollment_id_for(user_id, &target.course_id).await?;
            return self.lesson_view(lesson_id, enrollment.as_ref()).await;
        }
        task.set_step("generate", "正在生成本课时的分节正文与检索练习…");
        // 相位耗时读数（e1）：用量句柄随两条生成路径下传（图节点 = 上下文
        // 进引擎的 provider 计数装饰器；传统课时 = 上下文 + 兜底管线的计数
        // completer）。两条路径全程持有，finish 行聚合落档。
        let usage = task.usage();
        // 课程类型分流：学习图节点没有蓝图快照，走图专属路径——上下文来自
        // 课程行与前置边表，且只走 agent 引擎。
        let result = if target.graph {
            self.generate_graph_lesson_content(
                user_id,
                lesson_id,
                &target.course_id,
                request,
                usage,
                &task,
            )
            .await
        } else {
            self.generate_traditional_lesson_content(
                user_id,
                lesson_id,
                request,
                target,
                usage,
                &task,
            )
            .await
        };
        match result {
            Ok((view, stats)) => {
                match stats.filter(|stats| stats.attempts_used > 1) {
                    Some(stats) => task.finish_done(format!(
                        "课时内容已发布（第 {} 次尝试成功，瞬断已自愈）",
                        stats.attempts_used
                    )),
                    None => task.finish_done("课时内容已发布"),
                }
                Ok(view)
            }
            Err(error) => {
                // 耗尽 = 结构化错误：原因进任务 finish 行（异步臂调用方经
                // 任务快照读因），同步臂收同文的结构化错误——不再裸 502。
                task.finish_failed(error.to_string());
                Err(error)
            }
        }
    }

    /// 门面重试环（e4，learnhub contentgen facade `MAX_ATTEMPTS` 形态移植，
    /// 落点 = 课时循环入口）：引擎调用包三件瞬断加固——
    /// ①错误两桶（[`is_retryable_lesson_error`]）：流解码/传输/超时/空响应
    ///   = 可重试，数据性失败原样上抛；
    /// ②≤3 次带反馈重试：`retry_feedback` 回灌上次瞬断指纹 + 段级幂等提示
    ///   （e1 原语在工具面执法，这里补认知）；失败退避 300ms×attempt；
    /// ③单次尝试显式超时（[`LESSON_ATTEMPT_DEADLINE`]）：超时 = 可重试失败
    ///   只弃其果（judge 90s 形态；引擎自身 600s 预算先行，本限时是脱离
    ///   任务的死人开关）；
    /// 外加同错误熔断（[`LessonStepBreaker`]，coach `step_guard` 同款）：
    /// 连续 2 次同指纹失败 = 熔断 loud，不烧掉剩余尝试。
    async fn generate_lesson_with_retry(
        &self,
        user_id: &UserId,
        engine: &Arc<dyn LessonContentAgentEngine>,
        mut context: LessonGenerationContext,
        model_override: Option<(&str, &str)>,
        task: &crate::service::GenerationTaskGuard,
        attempt_deadline: std::time::Duration,
    ) -> Result<(LessonOutput, LessonRunStats), AppError> {
        let mut breaker = LessonStepBreaker::default();
        let mut last_fingerprint = String::new();
        for attempt in 1..=LESSON_GENERATE_ATTEMPTS {
            if attempt > 1 {
                context.retry_feedback = Some(format!(
                    "第 {} 次尝试因瞬断中断（{}）；已写好的节已在草稿里，对它们再调用 \
                     ls_set_section_body 会被幂等跳过——接着写未完成的节即可。",
                    attempt - 1,
                    last_fingerprint
                ));
                task.set_step(
                    "generate",
                    &format!("瞬断重试：第 {attempt}/{LESSON_GENERATE_ATTEMPTS} 次尝试…"),
                );
                self.emit_lesson_event(serde_json::json!({
                    "phase": "retry",
                    "lesson_id": context.lesson_id,
                    "attempt": attempt,
                    "last_error": last_fingerprint,
                }));
            }
            // 单次尝试显式超时：超时按可重试失败处理，只弃其果。
            let outcome = tokio::time::timeout(
                attempt_deadline,
                self.generate_or_resume_lesson(user_id, engine, &context, model_override),
            )
            .await;
            let fingerprint = match outcome {
                Ok(Ok(output)) => {
                    breaker.clear();
                    let stats = LessonRunStats {
                        attempts_used: attempt,
                        stop_reason: "success".to_owned(),
                        last_error: None,
                    };
                    self.record_lesson_run_stats(stats.clone());
                    return Ok((output, stats));
                }
                Ok(Err(error)) => {
                    let fingerprint = error.to_string();
                    if !is_retryable_lesson_error(&error) {
                        // 数据性失败（第二桶）：重试无意义，原样上抛。
                        self.record_lesson_run_stats(LessonRunStats {
                            attempts_used: attempt,
                            stop_reason: "fatal".to_owned(),
                            last_error: Some(fingerprint),
                        });
                        return Err(error);
                    }
                    fingerprint
                }
                Err(_elapsed) => format!(
                    "单次尝试超过 {}s 显式限时（超时按可重试失败处理，只弃其果）",
                    attempt_deadline.as_secs()
                ),
            };
            last_fingerprint = fingerprint.clone();
            // 同错误熔断：连续 2 次同指纹失败 = CircuitOpen loud。
            if breaker.trips_on(&fingerprint) {
                self.record_lesson_run_stats(LessonRunStats {
                    attempts_used: attempt,
                    stop_reason: "circuit_open".to_owned(),
                    last_error: Some(fingerprint.clone()),
                });
                return Err(AppError::BadGateway(format!(
                    "课时内容生成熔断中止（同一步骤连续 2 次同一错误，重试无意义）：{fingerprint}"
                )));
            }
            // 短退避（learnhub contentgen 层内退避同款）：只缓冲瞬态，不弃
            // 已得——已落库的节在重试中由段级幂等跳过原样保留。
            if attempt < LESSON_GENERATE_ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_millis(
                    LESSON_RETRY_BACKOFF_MS * attempt as u64,
                ))
                .await;
            }
        }
        self.record_lesson_run_stats(LessonRunStats {
            attempts_used: LESSON_GENERATE_ATTEMPTS,
            stop_reason: "exhausted".to_owned(),
            last_error: Some(last_fingerprint.clone()),
        });
        Err(AppError::BadGateway(format!(
            "课时内容生成在 {LESSON_GENERATE_ATTEMPTS} 次尝试后仍失败（瞬断重试预算耗尽）：{last_fingerprint}"
        )))
    }

    /// 最近一次课时生成的重试读数（smoke content 档瞬断臂判据数据源；内存态
    /// 仅诊断，与 [`GrowthRunStats`] 同作风）。
    pub fn lesson_run_stats(&self) -> Option<LessonRunStats> {
        self.lesson_run_stats
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn record_lesson_run_stats(&self, stats: LessonRunStats) {
        if let Ok(mut guard) = self.lesson_run_stats.lock() {
            *guard = Some(stats);
        }
    }

    /// 传统课时路径（蓝图快照流）：grounding 装配 → 上下文 → 门面重试环
    /// （引擎在册）或 legacy 一次性兜底管线（测试/未接引擎）→ 落库。
    /// 返回（课时视图，重试读数——兜底管线无重试环恒 `None`）。
    async fn generate_traditional_lesson_content(
        &self,
        user_id: &UserId,
        lesson_id: &LearningLessonId,
        request: &GenerateLessonRequest,
        target: LessonGenerationTarget,
        usage: crate::completer::LlmUsageMeter,
        task: &crate::service::GenerationTaskGuard,
    ) -> Result<(LessonView, Option<LessonRunStats>), AppError> {
        let course_id = &target.course_id;
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

        let module_position: i64 = target.module_position;
        let lesson_position: i64 = target.lesson_position;
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
            // 相位耗时读数（e1）：引擎分支进 provider 计数装饰器；兜底分支
            // 不经上下文（generate_lesson 直收蓝图），其计数在下方 completer
            // 包装上。
            usage: Some(usage.clone()),
            // 门面重试环的反馈槽（e4）：首试为空；重试臂在重试环内改写。
            retry_feedback: None,
        };
        self.emit_lesson_event(serde_json::json!({
            "phase": "started",
            "lesson_id": lesson_id.as_str(),
            "title": lesson.title,
            "module": module.title,
        }));
        let (output, stats) = match self.lesson_engine() {
            // Agent loop path: the injected two-loop engine owns the whole
            // lifecycle (draft + `ls_*` tools, audit-gated publish); its
            // LoopContext emits the round/audit progress frames itself.
            // A live draft from a failed run resumes instead of restarting.
            // e4：调用包门面重试环（两桶分类 + ≤3 次带反馈 + 显式超时 +
            // 同错误熔断），瞬断在门面吸收，耗尽才结构化上抛。
            Some(engine) => {
                let result = self
                    .generate_lesson_with_retry(
                        user_id,
                        &engine,
                        context,
                        model_override.map(|(provider, model)| (provider.as_str(), model)),
                        task,
                        LESSON_ATTEMPT_DEADLINE,
                    )
                    .await;
                match &result {
                    Ok((output, _stats)) => self.emit_lesson_event(serde_json::json!({
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
                let (output, stats) = result?;
                (output, Some(stats))
            }
            // Fallback: the legacy two-stage one-shot pipeline (tests and
            // direct calls), wrapped with the same terminal events so the
            // UI stays uniform. 无门面重试环（legacy 面，读数恒 None）。
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
                // 相位耗时读数（e1）：兜底管线经计数 completer 上报（调用数
                // + 输入/输出字符量），与 agent 分支同一 finish 行聚合。
                let counting =
                    crate::service::CountingCompleter::new(completer.as_ref(), &usage);
                let output = match generate_lesson(
                    &counting,
                    model_override,
                    &blueprint,
                    module,
                    lesson,
                    module_position as usize,
                    lesson_position as usize,
                    total_lessons,
                    next_lesson_title,
                    &crate::generation::render_grounding_fragments(&context.excerpts),
                    target.teaching_style,
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
                        return Err(AppError::UnprocessableEntity(format!(
                            "lesson '{}' failed to generate: {error}",
                            lesson.title
                        )));
                    }
                };
                (output, None)
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

        let enrollment = self.enrollment_id_for(user_id, course_id).await?;
        let view = self.lesson_view(lesson_id, enrollment.as_ref()).await?;
        Ok((view, stats))
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
        usage: crate::completer::LlmUsageMeter,
        task: &crate::service::GenerationTaskGuard,
    ) -> Result<(LessonView, Option<LessonRunStats>), AppError> {
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
                // Known Names（n3）：工作词边界名单行；None = 空名单（行省略）。
                known_names: topology.known_names,
                // teaches 档位（D4）：意图画像深度角（节点档位行）。
                teaches_tiers: topology.teaches_tiers,
                // 先例摘要（#180）：已建立语境视野（空 = 块省略）。
                precedents: topology.precedents,
            }),
            // 学习图节点的目标在 graph.goal；本字段只承载传统课程目标。
            goal: None,
            // 防超纲黑名单：下游节点标题清单（概念网反查，ADR-0009）。
            forbidden_concepts: topology.forbidden_concepts,
            // 相位耗时读数（e1）：随上下文进引擎，provider 计数装饰器上报。
            usage: Some(usage),
            // 门面重试环的反馈槽（e4）：首试为空；重试臂在重试环内改写。
            retry_feedback: None,
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
        // e4：调用包门面重试环（两桶分类 + ≤3 次带反馈 + 显式超时 + 同错误
        // 熔断），瞬断在门面吸收，耗尽才结构化上抛。
        let lesson_title = context.lesson_title.clone();
        let result = self
            .generate_lesson_with_retry(
                user_id,
                &engine,
                context,
                model_override.map(|(provider, model)| (provider.as_str(), model)),
                task,
                LESSON_ATTEMPT_DEADLINE,
            )
            .await;
        match &result {
            Ok((output, _stats)) => self.emit_lesson_event(serde_json::json!({
                "phase": "completed",
                "lesson_id": lesson_id.as_str(),
                "title": lesson_title,
                "activities": output.activities.len(),
                "est": output.estimated_minutes,
                "visuals": crate::models::visual_distribution(&output.sections),
            })),
            Err(error) => self.emit_lesson_event(serde_json::json!({
                "phase": "failed",
                "lesson_id": lesson_id.as_str(),
                "title": lesson_title,
                "error": error.to_string(),
            })),
        }
        let (output, stats) = result?;
        self.persist_lesson_output(lesson_id, &output).await?;

        let enrollment = self.enrollment_id_for(user_id, course_id).await?;
        let view = self.lesson_view(lesson_id, enrollment.as_ref()).await?;
        Ok((view, Some(stats)))
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
        // teaches 档位（D4）：(正名, 中文档位)——意图画像的深度角，装配点
        // 在 lesson_user_text 的节点块（此前「档位决定内容形态」纪律引用
        // 模型读不到的值）。档位解析失败落「?」（前置段渲染同款宽容）。
        let mut teaches_tiers: Vec<(String, String)> = Vec::new();
        let mut assumes: Vec<(String, String, String, Option<String>)> = Vec::new();
        for (role, canonical, tier, definition, ref_name) in declares {
            match role.as_str() {
                "teaches" => {
                    let tier_label = crate::learning_graph::ConceptTier::try_from_str(&tier)
                        .map(crate::learning_graph::tier_zh)
                        .unwrap_or("?");
                    teaches.push(canonical.clone());
                    teaches_tiers.push((canonical, tier_label.to_owned()));
                }
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

        // 6) 首现闭包（j07 概念面 + n3 先例共用一次不动点）：全图其余节点的
        //    teaches/assumes（现存且启用课程——已删墓碑与归档课不参与闭包，
        //    archived 不夺首现；并行分支因闭包只沿 assumes 链生长而天然不
        //    夺）。节点标题与课程 id 随行（先例归因面：归因限本课程、行名用
        //    节点标题）。本节点无概念挂接（teaches/assumes 双空）时整段缺席。
        let assume_names: Vec<String> = assumes
            .iter()
            .map(|(canonical, _, _, _)| canonical.clone())
            .collect();
        let closure = if teaches.is_empty() && assumes.is_empty() {
            None
        } else {
            let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
                "SELECT lc.lesson_id, lc.role, reg.canonical, l.title AS node_title, m.course_id \
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
            let mut candidates: HashMap<String, (String, String, Vec<String>, Vec<String>)> =
                HashMap::new();
            for (node, role, canonical, node_title, node_course) in rows {
                let entry = candidates.entry(node).or_default();
                if role == "teaches" {
                    entry.2.push(canonical);
                } else {
                    entry.3.push(canonical);
                }
                // 同一节点的标题/课程逐行重复，任取一行即可。
                entry.0 = node_title;
                entry.1 = node_course;
            }
            let nodes: Vec<(String, Vec<String>, Vec<String>)> = candidates
                .iter()
                .map(|(node, (_, _, teaches, assumes))| {
                    (node.clone(), teaches.clone(), assumes.clone())
                })
                .collect();
            let meta: HashMap<String, (String, String)> = candidates
                .into_iter()
                .map(|(node, (title, course, _, _))| (node, (title, course)))
                .collect();
            Some((first_appearance_with_teachers(&teaches, &assume_names, &nodes), meta))
        };

        // 7) 概念面（j07）：首现/非首现 + 引用名 + 别名池。
        let concept_faces = closure.as_ref().map(|(appearance, _)| {
            let returning: Vec<String> = teaches
                .iter()
                .filter(|concept| !appearance.first_seen.contains(concept))
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
            crate::lesson_draft::GraphConceptFaces {
                first_seen: appearance.first_seen.clone(),
                returning,
                ref_names,
                // 投影词表在闭包外单独查询（active_aliases_for）。
                aliases: Vec::new(),
            }
        });
        // 投影词表（R3）：本节点概念（teaches ∪ assumes）的活跃别名——
        // 概念面在场的必要条件（全空 = 段缺席），别名查询只挂接面非空时跑。
        let concept_faces = match concept_faces {
            Some(mut faces) => {
                let mut all_concepts: Vec<String> = teaches.clone();
                all_concepts.extend(assume_names.iter().cloned());
                faces.aliases = self.active_aliases_for(&all_concepts).await?;
                (!faces.is_empty()).then_some(faces)
            }
            None => None,
        };

        // 8) 先例摘要（#180，n3 D5 口径）：闭包内教出本点涉及概念的节点，
        //    **限本课程**（跨课程视角由前置路径承载）；先行节标题复用
        //    taught_sections_for_lessons（零正文 = 空 sections 仍入行）；
        //    按 node_id 升序稳定（归因纯函数面已排序）。
        let precedents: Vec<crate::lesson_draft::LessonPrecedent> = match closure.as_ref() {
            None => Vec::new(),
            Some((appearance, meta)) => {
                let mut teachers: Vec<(String, Vec<String>)> = appearance
                    .teachers
                    .iter()
                    .filter(|(node, _)| {
                        meta.get(node)
                            .is_some_and(|(_, course)| course == course_id.as_str())
                    })
                    .cloned()
                    .collect();
                let teacher_ids: Vec<String> =
                    teachers.iter().map(|(node, _)| node.clone()).collect();
                let taught = self.taught_sections_for_lessons(&teacher_ids).await?;
                teachers
                    .drain(..)
                    .filter_map(|(node, concepts)| {
                        let (node_title, _) = meta.get(&node)?;
                        // 零正文（先行节点未生成节）= 空 sections 仍入行
                        // （learnhub「文件缺席 = 空」同形——行名与概念仍在）。
                        let sections = taught
                            .get(&node)
                            .map(|rows| {
                                rows.iter()
                                    .map(|(title, _)| title.clone())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        Some(crate::lesson_draft::LessonPrecedent {
                            concept: concepts.join("、"),
                            node_name: node_title.clone(),
                            sections,
                        })
                    })
                    .collect()
            }
        };

        Ok(GraphNodeTopology {
            prerequisite_path,
            upcoming_nodes,
            forbidden_concepts,
            total_nodes: total_nodes.max(0) as usize,
            concept_faces,
            known_names: self.course_known_names(course_id).await?,
            teaches_tiers,
            precedents,
        })
    }

    /// 课程已知概念（Known Names，n3 D1 口径，learnhub #177 域中立移植）：
    /// **本课程图**全部节点 teaches ∪ **本课程**已会声明（m2 锚表
    /// learning_basis_declarations）的显示名并集，BTreeSet 升序稳定。与
    /// learnhub 的记档差异：声明引用悬空（concept_id 不在登记表或已退役）
    /// = 如实跳过该条并 warn，不 Broken 炸——allo 撤销语义「声明可撤、
    /// 概念不灭」下悬空引用是合法态（learnhub 引用纪律 Broken 不适用）。
    /// 空名单 = 空表（渲染面整行省略）。
    async fn course_known_names(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<Option<Vec<String>>, AppError> {
        let mut known: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        // 已教半边：本课程图全部节点的 teaches（登记表正名；退役概念不占
        // 名单——名字已退场，不再当工作词；已删墓碑课的节点不参与——
        // deleted_at IS NULL 与闭包查询同律，删课节点的供给不出名单）。
        let taught: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT reg.canonical \
             FROM learning_lesson_concepts lc \
             JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id AND c.deleted_at IS NULL \
             JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
             WHERE m.course_id = ? AND lc.role = 'teaches' AND reg.deprecated = 0 \
             ORDER BY reg.canonical",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        for (canonical,) in taught {
            known.insert(canonical);
        }
        // 声明半边：本课程锚行的 concept_ids_json → 登记表正名；悬空/退役
        // 引用如实跳过 + warn（不阻断生成——名单是边界指称面，不是硬门）。
        for (_, _, concept_ids_json) in self
            .declared_anchor_rows_for_course(course_id)
            .await?
        {
            let ids: Vec<String> = match serde_json::from_str(&concept_ids_json) {
                Ok(ids) => ids,
                Err(error) => {
                    tracing::warn!(
                        course_id = course_id.as_str(),
                        error = %error,
                        "known_names: unreadable declaration concept_ids_json skipped"
                    );
                    continue;
                }
            };
            for concept_id in ids {
                let row: Option<(String, i64)> = sqlx::query_as(
                    "SELECT canonical, deprecated FROM learning_concept_registry \
                     WHERE concept_id = ?",
                )
                .bind(&concept_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
                match row {
                    Some((canonical, deprecated)) if deprecated == 0 => {
                        known.insert(canonical);
                    }
                    Some((canonical, _)) => {
                        tracing::warn!(
                            course_id = course_id.as_str(),
                            concept_id = %concept_id,
                            canonical = %canonical,
                            "known_names: deprecated declared concept skipped"
                        );
                    }
                    None => {
                        tracing::warn!(
                            course_id = course_id.as_str(),
                            concept_id = %concept_id,
                            "known_names: dangling declared concept skipped"
                        );
                    }
                }
            }
        }
        Ok((!known.is_empty()).then(|| known.into_iter().collect()))
    }

    /// 一组正名概念的活跃别名（R3 投影词表；退役别名不占池）。正名 →
    /// 别名列表，仅多名概念入表。
    async fn active_aliases_for(        &self,
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
            degraded: false,
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
                degraded: false,
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

    /// 节点 tier → 题面难度铸值（e5 difficulty 消解案，装配面注入）：
    /// 读本课 teaches 行的档位，取最深档映射 1-3（know=辨析 / apply=应用 /
    /// teach=综合——节点教得最深的概念决定题面难度上限锚，learnhub「以骨架
    /// 行的难度档为准」的输入锚同义）。无 teaches 行（传统课/概念绑定整体
    /// 下线的旧课）或档位不可解析 → None——出题输出契约已撤 difficulty，
    /// None 兜回模型自报值（存量 agent 路径）或保持空（读侧 serde 缺省容
    /// 错）。查询失败同样降级为 None：铸值是尽力注入，不阻断发布。
    /// executor 由调用方给：persist 面传在开事务上（同池并发查询会饿死
    /// 单连接池），加题面传池。
    async fn injected_activity_difficulty<'e, E>(
        executor: E,
        lesson_id: &LearningLessonId,
    ) -> Option<u8>
    where
        E: sqlx::SqliteExecutor<'e>,
    {
        let tiers: Vec<String> = sqlx::query_scalar(
            "SELECT tier FROM learning_lesson_concepts \
             WHERE lesson_id = ? AND role = 'teaches'",
        )
        .bind(lesson_id.as_str())
        .fetch_all(executor)
        .await
        .ok()?;
        tiers
            .iter()
            .filter_map(|tier| {
                crate::learning_graph::ConceptTier::try_from_str(tier.trim())
            })
            .max()
            .map(crate::learning_graph::ConceptTier::difficulty_value)
    }

    /// 生成事务落库尾（传统与学习图两条路径共用）：更新课时行（文档/时长/
    /// 生成标记）并替换全部活动。概念绑定随 per-course 概念体系整体下线
    /// （ADR-0009），活动不再携带 concept 绑定。
    async fn persist_lesson_output(
        &self,
        lesson_id: &LearningLessonId,
        output: &LessonOutput,
    ) -> Result<(), AppError> {
        // 节点意图超载（k03/#178）：观察行，非门禁——capacity_note 在 #178
        // 语义下是上游 est 复议的信号（先砍雄心再砍内容仍收敛不住时的必写
        // 指名原因），留痕便于真机对照卡与事后盘点（learnhub over_capacity
        // 日志行同款；telemetry 键沿用 over_capacity）。
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

        // e5 difficulty 消解（装配面注入）：难度不指望模型输出——按本课
        // teaches 档位铸值一次，全体活动继承；无档位可依时兜回模型自报值
        // （存量 agent 路径）或保持 None。查询走同一事务（不另借池连接）。
        let injected_difficulty =
            Self::injected_activity_difficulty(&mut *transaction, lesson_id).await;
        for (position, activity) in output.activities.iter().enumerate() {
            let activity_id = LearningActivityId::new();
            let config = StoredActivityConfig {
                options: activity.options.clone(),
                answer: activity.answer.clone(),
                explanation: activity.explanation.clone(),
                distractors: activity.distractors.clone(),
                tol: activity.tol,
                matches: matching_candidates(activity),
                difficulty: injected_difficulty.or(activity.difficulty),
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
        let (prompt, mut config) = validate_question_payload(
            request.kind,
            &request.prompt,
            &request.options,
            &request.answer,
            &request.explanation,
            &request.distractors,
        )?;
        // e5 difficulty 消解：手动加题与生成装配同一注入面——课时题继承
        // 节点 teaches 档位（无档位可依时保持 None，读侧容错），保证 quiz
        // tier 取数在整张题库上口径一致。
        config.difficulty =
            Self::injected_activity_difficulty(&self.pool, lesson_id)
                .await
                .or(config.difficulty);
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
/// 前置/后续/禁止清单的预渲染文本 + 概念面投影（j07）+ 课程已知概念
/// （Known Names，n3）+ teaches 档位（D4）+ 先例摘要（#180）。
pub(crate) struct GraphNodeTopology {
    pub prerequisite_path: String,
    pub upcoming_nodes: String,
    pub forbidden_concepts: String,
    pub total_nodes: usize,
    pub concept_faces: Option<crate::lesson_draft::GraphConceptFaces>,
    /// 课程已知概念（Known Names）：`None` = 名单为空（行省略）。
    pub known_names: Option<Vec<String>>,
    /// 本节点 teaches 概念的档位（正名，中文档位）。
    pub teaches_tiers: Vec<(String, String)>,
    /// 先例摘要（限本课程，按 node_id 升序稳定）。
    pub precedents: Vec<crate::lesson_draft::LessonPrecedent>,
}

/// 首现判定 + 祖先归因（二轮 g2/j07 + n3 D5 扩展，纯函数——SQL 收集在
/// 调用方）：祖先闭包不动点。起点 = 本节点 assumes；循环扫候选节点，凡
/// teaches 命中当前扫描集即纳入、并入其 assumes、teaches 计入祖先已教集合，
/// 直至不动点。本节点 teaches 中不在祖先已教集合的 = 首现。归因（#180 先
/// 例）：闭包内每个节点教出的本点涉及概念（teaches ∪ assumes），按 node_id
/// 升序稳定——同一概念被多个祖先教出时在每个教师下各记一次（learnhub 同
/// 形）。并行分支因闭包只沿 assumes 链生长而天然不夺首现；archived 节点
/// （归档/已删课程）由调用方过滤后不参与扫描。
#[derive(Debug, Clone, Default)]
pub(crate) struct FirstAppearance {
    /// 本节点 teaches 中的首现概念（正名；闭包未覆盖）。
    pub first_seen: Vec<String>,
    /// 闭包内教出本点涉及概念的节点：(node_id, 概念正名清单升序)。
    pub teachers: Vec<(String, Vec<String>)>,
}

fn first_appearance_with_teachers(
    self_teaches: &[String],
    self_assumes: &[String],
    candidates: &[(String, Vec<String>, Vec<String>)],
) -> FirstAppearance {
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
    // 归因：闭包内节点的 teaches ∩ 本点涉及概念（teaches ∪ assumes）。
    let involved: std::collections::HashSet<&str> = self_teaches
        .iter()
        .chain(self_assumes)
        .map(String::as_str)
        .collect();
    let mut per_node: std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> =
        std::collections::BTreeMap::new();
    for (node, teaches, _) in candidates {
        if !included.contains(node.as_str()) {
            continue;
        }
        for concept in teaches {
            if involved.contains(concept.as_str()) {
                per_node.entry(node.as_str()).or_default().insert(concept.as_str());
            }
        }
    }
    FirstAppearance {
        first_seen: self_teaches
            .iter()
            .filter(|concept| !ancestor_taught.contains(concept.as_str()))
            .cloned()
            .collect(),
        teachers: per_node
            .into_iter()
            .map(|(node, concepts)| {
                (node.to_owned(), concepts.into_iter().map(str::to_owned).collect())
            })
            .collect(),
    }
}

/// 首现判定（纯函数，j07 不动点的首现半边）。生产消费面已迁
/// [`first_appearance_with_teachers`]（带节点归因，n3 先例共用同一次不动
/// 点）；本函数留存供旧测试/对照使用，不再承载生产调用。
fn first_appearance_set(
    self_teaches: &[String],
    self_assumes: &[String],
    candidates: &[(String, Vec<String>, Vec<String>)],
) -> Vec<String> {
    first_appearance_with_teachers(self_teaches, self_assumes, candidates).first_seen
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

    /// 先例归因（n3 D5 扩展）：闭包内节点教出的本点涉及概念（teaches ∪
    /// assumes）归到教师节点名下（node_id 升序稳定）；首现概念无教师
    /// （本点自己教）；并行分支不入归因；闭包外节点（教但不可达）不入。
    #[test]
    fn first_appearance_attributes_teachers_within_the_closure() {
        // 本节点 teaches [C, D]、assumes [A]；n1 教 A（assumes [B]）、
        // n2 教 [B, D]；sibling 教 C（并行不入闭包）。
        let candidates = vec![
            ("n1".to_owned(), vec!["A".to_owned()], vec!["B".to_owned()]),
            ("n2".to_owned(), vec!["B".to_owned(), "D".to_owned()], vec![]),
            (
                "sibling".to_owned(),
                vec!["C".to_owned()],
                vec![],
            ),
        ];
        let appearance = first_appearance_with_teachers(
            &["C".to_owned(), "D".to_owned()],
            &["A".to_owned()],
            &candidates,
        );
        // 首现 = C（无闭包节点教它）；D 由 n2 教出 → 非首现。
        assert_eq!(appearance.first_seen, vec!["C".to_owned()]);
        // 归因：n1 → [A]（本点 assumes 的概念，其教师在闭包上）；n2 → [D]。
        // sibling 不入（不在闭包）。
        assert_eq!(
            appearance.teachers,
            vec![
                ("n1".to_owned(), vec!["A".to_owned()]),
                ("n2".to_owned(), vec!["D".to_owned()]),
            ]
        );
    }

    /// n3 确定性布景（零 LLM，temp SQLite）：两节点图课 + 声明锚（含悬空
    /// 与退役引用）+ 先行节点两节正文 → `graph_node_topology` 投影 Known
    /// Names（本课已教 ∪ 本课声明并集、升序；悬空/退役声明引用如实跳过不
    /// 炸）、teaches 档位（中文档位）、先例归因（限本课程 + 先行节标题，
    /// 零正文 = 空 sections 仍入）。
    #[tokio::test]
    async fn graph_topology_projects_known_names_tiers_and_precedents() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        let pool = service.pool_for_tests();
        let now = now_ms();
        let course_id = LearningCourseId::new().into_string();
        let module_id = LearningModuleId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, learning_goal, \
              learning_scope, created_at, updated_at) \
             VALUES (?, 'known 课程', '', 'general', 1, 'learning_graph', 'g', '', ?, ?)",
        )
        .bind(&course_id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
             VALUES (?, ?, '学习图', '', 0)",
        )
        .bind(&module_id)
        .bind(&course_id)
        .execute(pool)
        .await
        .unwrap();
        let lesson_a = LearningLessonId::new().into_string();
        let lesson_b = LearningLessonId::new().into_string();
        for (lesson_id, title, position) in [
            (&lesson_a, "收益率与对数收益率", 0),
            (&lesson_b, "波动率的直观刻画", 1),
        ] {
            sqlx::query(
                "INSERT INTO learning_lessons \
                 (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
                  content_generated) VALUES (?, ?, ?, '', 'p', ?, 20, 0)",
            )
            .bind(lesson_id)
            .bind(&module_id)
            .bind(title)
            .bind(position)
            .execute(pool)
            .await
            .unwrap();
        }
        let concept_alpha = LearningConceptId::new().into_string();
        let concept_beta = LearningConceptId::new().into_string();
        for (concept_id, canonical, aliases) in [
            (&concept_alpha, "对数收益率", serde_json::json!([])),
            (&concept_beta, "历史波动率", serde_json::json!([])),
        ] {
            sqlx::query(
                "INSERT INTO learning_concept_registry \
                 (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
                 VALUES (?, ?, ?, 'd', ?, ?)",
            )
            .bind(concept_id)
            .bind(canonical)
            .bind(aliases.to_string())
            .bind(now)
            .bind(now)
            .execute(pool)
            .await
            .unwrap();
        }
        for (lesson_id, role, concept_id, tier) in [
            (&lesson_a, "teaches", &concept_alpha, "apply"),
            (&lesson_b, "assumes", &concept_alpha, "apply"),
            (&lesson_b, "teaches", &concept_beta, "know"),
        ] {
            sqlx::query(
                "INSERT INTO learning_lesson_concepts \
                 (lesson_id, concept_id, role, tier) VALUES (?, ?, ?, ?)",
            )
            .bind(lesson_id)
            .bind(concept_id)
            .bind(role)
            .bind(tier)
            .execute(pool)
            .await
            .unwrap();
        }
        // 声明锚：α（合法）+ 悬空 id + 退役概念——后两者如实跳过 + warn，
        // 不炸（allo 撤销语义「声明可撤、概念不灭」下的合法态，票面记档）。
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, deprecated, created_at, updated_at) \
             VALUES (?, '退役概念', '[]', 'd', 1, ?, ?)",
        )
        .bind(LearningConceptId::new().into_string())
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_basis_declarations \
             (course_id, doors_json, blocks_json, concept_ids_json, declared_at, note) \
             VALUES (?, '[]', '[]', ?, ?, '布景声明')",
        )
        .bind(&course_id)
        .bind(
            serde_json::json!([
                concept_alpha,
                "00000000-0000-7fff-8fff-00000000dead",
                LearningConceptId::new().into_string()
            ])
            .to_string(),
        )
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        // 先行节点两节 ready 正文（先例的先行节标题面）。
        for (section_key, title, position) in
            [("s1", "概念：对数收益的定义", 0), ("s2", "例题：两日换算", 1)]
        {
            sqlx::query(
                "INSERT INTO learning_lesson_sections \
                 (section_key, lesson_id, kind, title, points, visual, body_md, degraded, \
                  status, version, position, created_at, updated_at) \
                 VALUES (?, ?, 'concept', ?, 'p', '表格', 'b', 0, 'ready', 1, ?, ?, ?)",
            )
            .bind(section_key)
            .bind(&lesson_a)
            .bind(title)
            .bind(position)
            .bind(now)
            .bind(now)
            .execute(pool)
            .await
            .unwrap();
        }

        let course = LearningCourseId::parse(&course_id).unwrap();
        let lesson_b_id = LearningLessonId::parse(&lesson_b).unwrap();
        let topology = service.graph_node_topology(&course, &lesson_b_id).await.unwrap();

        // Known Names：本课已教（α 由 L1、β 由 L2）∪ 声明（α；悬空与退役
        // 引用跳过）——升序稳定，且退役概念不入名单。
        assert_eq!(
            topology.known_names,
            Some(vec!["历史波动率".to_owned(), "对数收益率".to_owned()]),
            "known names = taught ∪ declared (dangling/deprecated skipped): {:?}",
            topology.known_names
        );
        // teaches 档位：β（know → 知道），中文档位。
        assert_eq!(
            topology.teaches_tiers,
            vec![("历史波动率".to_owned(), "知道".to_owned())]
        );
        // 先例：α 的教师 L1（本课程），概念归到 L1 名下 + 先行节标题；
        // β 首现无先例。
        assert_eq!(topology.precedents.len(), 1, "{:?}", topology.precedents);
        let precedent = &topology.precedents[0];
        assert_eq!(precedent.concept, "对数收益率");
        assert_eq!(precedent.node_name, "收益率与对数收益率");
        assert_eq!(
            precedent.sections,
            vec!["概念：对数收益的定义".to_owned(), "例题：两日换算".to_owned()]
        );
    }

    /// Known Names 空名单：图课零挂接（双空）时名单为空 → `None`（行省略，
    /// 渲染面整行省略的下游语义）。
    #[tokio::test]
    async fn graph_topology_known_names_absent_when_course_vocabulary_is_empty() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        let pool = service.pool_for_tests();
        let now = now_ms();
        let course_id = LearningCourseId::new().into_string();
        let module_id = LearningModuleId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, learning_goal, \
              learning_scope, created_at, updated_at) \
             VALUES (?, '空课', '', 'general', 1, 'learning_graph', 'g', '', ?, ?)",
        )
        .bind(&course_id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
             VALUES (?, ?, '学习图', '', 0)",
        )
        .bind(&module_id)
        .bind(&course_id)
        .execute(pool)
        .await
        .unwrap();
        let lesson = LearningLessonId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
              content_generated) VALUES (?, ?, '孤节点', '', 'p', 0, 20, 0)",
        )
        .bind(&lesson)
        .bind(&module_id)
        .execute(pool)
        .await
        .unwrap();
        let course = LearningCourseId::parse(&course_id).unwrap();
        let lesson_id = LearningLessonId::parse(&lesson).unwrap();
        let topology = service.graph_node_topology(&course, &lesson_id).await.unwrap();
        assert_eq!(topology.known_names, None, "零挂接零声明 = 名单缺席");
        assert!(topology.precedents.is_empty());
        assert!(topology.teaches_tiers.is_empty());
    }

    /// Known Names 围栏（评审修复钉，红→绿）：已删节点 teaches 出不了名单
    /// ——课程软删成墓碑（deleted_at 置位）后，其节点的供给不再算工作词
    /// 表（与闭包查询 deleted_at IS NULL 同律）。
    #[tokio::test]
    async fn known_names_exclude_deleted_course_teaches() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        let pool = service.pool_for_tests();
        let now = now_ms();
        let course_id = LearningCourseId::new().into_string();
        let module_id = LearningModuleId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, learning_goal, \
              learning_scope, created_at, updated_at) \
             VALUES (?, '待删课', '', 'general', 1, 'learning_graph', 'g', '', ?, ?)",
        )
        .bind(&course_id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_modules (module_id, course_id, title, description, position) \
             VALUES (?, ?, '学习图', '', 0)",
        )
        .bind(&module_id)
        .bind(&course_id)
        .execute(pool)
        .await
        .unwrap();
        let lesson = LearningLessonId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_lessons \
             (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
              content_generated) VALUES (?, ?, '供给节点', '', 'p', 0, 20, 0)",
        )
        .bind(&lesson)
        .bind(&module_id)
        .execute(pool)
        .await
        .unwrap();
        let concept = LearningConceptId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
             VALUES (?, '对数收益率', '[]', 'd', ?, ?)",
        )
        .bind(&concept)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO learning_lesson_concepts \
             (lesson_id, concept_id, role, tier) VALUES (?, ?, 'teaches', 'apply')",
        )
        .bind(&lesson)
        .bind(&concept)
        .execute(pool)
        .await
        .unwrap();

        // 活课基线：teaches 正名入名单。
        let course = LearningCourseId::parse(&course_id).unwrap();
        assert_eq!(
            service.course_known_names(&course).await.unwrap(),
            Some(vec!["对数收益率".to_owned()])
        );

        // 课程软删（墓碑）→ 其节点 teaches 出不了名单（旧实现无
        // deleted_at 过滤时此处仍返回该概念 = 红）。
        sqlx::query("UPDATE learning_courses SET deleted_at = ? WHERE course_id = ?")
            .bind(now)
            .bind(&course_id)
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            service.course_known_names(&course).await.unwrap(),
            None,
            "已删节点的 teaches 不得入名单"
        );
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

    // ── 门面重试环（e4）：两桶分类 / ≤3 带反馈 / 显式超时 / 同错误熔断 ──

    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    /// 错误两桶（e4，learnhub infra/llm.rs 口径）：流解码/传输/超时/空响应
    /// （统一落 BadGateway/Timeout/RateLimited 变体）= 可重试；数据性失败
    /// （冲突/不存在/审计门终判/内部故障）重试无意义 = 原样上抛。
    #[test]
    fn error_taxonomy_splits_retryable_from_fatal() {
        assert!(is_retryable_lesson_error(&AppError::BadGateway(
            "LLM stream error: connection reset".into()
        )));
        assert!(is_retryable_lesson_error(&AppError::Timeout(
            "agent timed out after 600s".into()
        )));
        assert!(is_retryable_lesson_error(&AppError::RateLimited));
        assert!(!is_retryable_lesson_error(&AppError::Conflict(
            "课程已归档".into()
        )));
        assert!(!is_retryable_lesson_error(&AppError::NotFound(
            "learning lesson x".into()
        )));
        assert!(!is_retryable_lesson_error(&AppError::UnprocessableEntity(
            "exhausted 3 repair loops".into()
        )));
        assert!(!is_retryable_lesson_error(&AppError::Internal(
            "lock poisoned".into()
        )));
    }

    /// 脚本化假引擎：generate/resume 按脚本步进（每步一段行为），记录调用
    /// 次数与 resume 时收到的 retry_feedback——门面重试环的红绿样直接驱动
    /// 它。
    struct ScriptedLessonEngine {
        service: Arc<LearningService>,
        generate_calls: AtomicUsize,
        resume_calls: AtomicUsize,
        /// 每次尝试的行为：Err = 该尝试以给定错误失败；Ok = 成功发布。
        /// generate 与 resume 共用同一脚本（重试环先 generate，草稿存活
        /// 后续尝试走 resume）。
        steps: Mutex<VecDeque<Result<LessonOutput, AppError>>>,
        /// resume 尝试收到的 retry_feedback（最后一次；None = 未收到）。
        seen_retry_feedback: Mutex<Option<String>>,
        /// resume 尝试看到的草稿里已落库的节（最后一次；段级续跑钉子）。
        seen_resume_sections: Mutex<Vec<String>>,
        /// 「首败留草稿」的上下文（见 [`Self::arm_seed_on_failure`]）。
        seed_on_failure: Mutex<Option<LessonGenerationContext>>,
    }

    impl ScriptedLessonEngine {
        fn new(
            service: Arc<LearningService>,
            steps: Vec<Result<LessonOutput, AppError>>,
        ) -> Arc<Self> {
            Arc::new(Self {
                service,
                generate_calls: AtomicUsize::new(0),
                resume_calls: AtomicUsize::new(0),
                steps: Mutex::new(steps.into()),
                seen_retry_feedback: Mutex::new(None),
                seen_resume_sections: Mutex::new(Vec::new()),
                seed_on_failure: Mutex::new(None),
            })
        }

        /// 设置「首败留草稿」：generate 步失败前先以该上下文创建草稿并写
        /// 入 s1（模拟「循环写过一节后瞬断」——e1 段级幂等的存活面），使
        /// 重试走 e1 的 resume 续跑。
        fn arm_seed_on_failure(&self, context: &LessonGenerationContext) {
            *self.seed_on_failure.lock().unwrap() = Some(context.clone());
        }

        /// 预置「上次会话」存活草稿：manifest + s1 已落库正文（过节级质检
        /// 门的长正文，visual 表格兑现承诺）。
        fn seed_draft(&self, context: &LessonGenerationContext) -> String {
            let view = self.service.create_lesson_draft(context.clone()).unwrap();
            self.service
                .patch_lesson_draft(
                    &view.draft_id,
                    vec![LessonOp::SetSectionManifest {
                        sections: serde_json::from_value(serde_json::json!([
                            { "section_key": "s1", "kind": "concept", "title": "概念：复利", "points": "利滚利", "visual": "表格" },
                            { "section_key": "s2", "kind": "practice", "title": "练习：复利判断", "points": "自测", "visual": "无" }
                        ]))
                        .unwrap(),
                        capacity_note: None,
                    },
                    LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: format!(
                            "## 概念：复利\n\n| 期数 | 终值 |\n| --- | --- |\n| 1 | 本金×(1+r) |\n\n{}\n",
                            "这是一个足够长的正文段落，用于通过节级质检门的长度下限要求。".repeat(12)
                        ),
                    },
                ])
                .unwrap();
            view.draft_id
        }
    }

    fn fake_output() -> LessonOutput {
        LessonOutput {
            summary: "## 概念：复利\n\n完成的正文。".into(),
            estimated_minutes: 15,
            activities: Vec::new(),
            sections: Vec::new(),
            degraded_keys: Vec::new(),
            capacity_note: None,
            viz_lint_events: 0,
        }
    }

    fn retry_context() -> LessonGenerationContext {
        LessonGenerationContext {
            lesson_id: "lesson-retry".into(),
            course_title: "测试课程".into(),
            course_description: String::new(),
            module_title: "模块一".into(),
            module_index: 0,
            lesson_title: "课时一".into(),
            lesson_index: 0,
            total_lessons: 1,
            next_lesson_title: None,
            purpose: "理解复利".into(),
            excerpt: None,
            excerpts: Vec::new(),
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: None,
            goal: None,
            forbidden_concepts: String::new(),
            usage: None,
            retry_feedback: None,
        }
    }

    #[async_trait::async_trait]
    impl LessonContentAgentEngine for ScriptedLessonEngine {
        async fn generate(
            &self,
            _user_id: &UserId,
            _context: &LessonGenerationContext,
            _model_override: Option<(&str, &str)>,
        ) -> Result<LessonOutput, AppError> {
            self.generate_calls.fetch_add(1, AtomicOrdering::SeqCst);
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("script exhausted in generate");
            if step.is_err() {
                // 「循环写过一节后瞬断」形态：失败前把草稿与已落库的节留下。
                if let Some(context) = self
                    .seed_on_failure
                    .lock()
                    .unwrap()
                    .take()
                {
                    self.seed_draft(&context);
                }
            }
            step
        }

        async fn resume(
            &self,
            _user_id: &UserId,
            draft_id: &str,
            context: &LessonGenerationContext,
            _model_override: Option<(&str, &str)>,
        ) -> Result<LessonOutput, AppError> {
            self.resume_calls.fetch_add(1, AtomicOrdering::SeqCst);
            *self.seen_retry_feedback.lock().unwrap() = context.retry_feedback.clone();
            let view = self.service.inspect_lesson_draft(draft_id).unwrap();
            *self.seen_resume_sections.lock().unwrap() = view
                .section_bodies
                .iter()
                .map(|body| body.section_key.clone())
                .collect();
            self.steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("script exhausted in resume")
        }
    }

    async fn retry_test_service() -> (Arc<LearningService>, tempfile::TempDir) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = Arc::new(LearningService::new(database.pool().clone()));
        (service, tempfile::tempdir().unwrap())
    }

    /// 红绿样（e4 核心臂）：瞬断一次 → 门面重试环吸收，第二次尝试经 e1
    /// 的草稿续跑（resume）完成发布；段级续跑钉子——resume 时草稿里已落库
    /// 的 s1 原样在库（不重写），retry_feedback 已回灌给开场轮。
    #[tokio::test]
    async fn retry_loop_absorbs_a_transient_failure_and_resumes_the_draft() {
        let (service, _dir) = retry_test_service().await;
        let context = retry_context();
        let engine = ScriptedLessonEngine::new(
            Arc::clone(&service),
            vec![
                Err(AppError::BadGateway("瞬断：连接被重置".into())),
                Ok(fake_output()),
            ],
        );
        engine.arm_seed_on_failure(&context);
        service.set_lesson_engine(engine.clone());
        let task = service
            .begin_generation_task(crate::service::KIND_LESSON, "c", Some("lesson-retry"), "t")
            .await
            .unwrap();
        let engine_dyn =
            engine.clone() as Arc<dyn LessonContentAgentEngine>;
        let (output, stats) = service
            .generate_lesson_with_retry(
                &nomifun_common::UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
                &engine_dyn,
                context.clone(),
                None,
                &task,
                std::time::Duration::from_secs(10),
            )
            .await
            .expect("second attempt publishes");
        assert_eq!(output.summary, fake_output().summary);
        assert_eq!(
            engine.generate_calls.load(AtomicOrdering::SeqCst),
            1,
            "首试走 generate"
        );
        assert_eq!(
            engine.resume_calls.load(AtomicOrdering::SeqCst),
            1,
            "重试走 resume（e1 草稿续跑）"
        );
        assert_eq!(stats.attempts_used, 2);
        assert_eq!(stats.stop_reason, "success");
        // 段级续跑钉子：resume 时草稿里已落库的 s1 在库（幂等跳过原语保证
        // 它不被重写——重试环天然受益）。
        assert_eq!(
            engine.seen_resume_sections.lock().unwrap().last(),
            Some(&"s1".to_owned()),
            "seeded section must still be in the draft at resume time"
        );
        // 反馈回灌钉子：resume 开场带 retry_feedback（上次瞬断指纹 + 不重
        // 写提示）。
        let feedback = engine.seen_retry_feedback.lock().unwrap().clone();
        let feedback = feedback.expect("resume must carry retry feedback");
        assert!(feedback.contains("瞬断：连接被重置"), "{feedback}");
        assert!(feedback.contains("幂等跳过"), "{feedback}");
        // 服务级读数面与返回值一致（smoke content 档判据数据源）。
        assert_eq!(service.lesson_run_stats().unwrap().attempts_used, 2);
    }

    /// 同错误熔断（step_guard 同款）：连续两次同指纹失败 = CircuitOpen loud，
    /// 第三次尝试不烧（重试预算不趟浑水）。
    #[tokio::test]
    async fn same_error_twice_trips_the_circuit_breaker() {
        let (service, _dir) = retry_test_service().await;
        let engine = ScriptedLessonEngine::new(
            Arc::clone(&service),
            vec![
                Err(AppError::BadGateway("same upstream error".into())),
                Err(AppError::BadGateway("same upstream error".into())),
            ],
        );
        service.set_lesson_engine(engine.clone());
        let task = service
            .begin_generation_task(crate::service::KIND_LESSON, "c", Some("lesson-retry"), "t")
            .await
            .unwrap();
        let engine_dyn = engine.clone() as Arc<dyn LessonContentAgentEngine>;
        let error = service
            .generate_lesson_with_retry(
                &nomifun_common::UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
                &engine_dyn,
                retry_context(),
                None,
                &task,
                std::time::Duration::from_secs(10),
            )
            .await
            .expect_err("circuit opens");
        assert!(
            error.to_string().contains("熔断中止"),
            "circuit-open error must be loud: {error}"
        );
        assert!(
            error.to_string().contains("same upstream error"),
            "fingerprint carried in the structured reason: {error}"
        );
        assert_eq!(
            engine.generate_calls.load(AtomicOrdering::SeqCst),
            2,
            "second identical failure opens the circuit before attempt 3"
        );
        let stats = service.lesson_run_stats().unwrap();
        assert_eq!(stats.stop_reason, "circuit_open");
        assert_eq!(stats.attempts_used, 2);
    }

    /// 数据性失败（第二桶）原样上抛：不重试、不包装、如实失败一次。
    #[tokio::test]
    async fn fatal_errors_pass_through_without_retry() {
        let (service, _dir) = retry_test_service().await;
        let engine = ScriptedLessonEngine::new(
            Arc::clone(&service),
            vec![Err(AppError::Conflict("课程已归档".into()))],
        );
        service.set_lesson_engine(engine.clone());
        let task = service
            .begin_generation_task(crate::service::KIND_LESSON, "c", Some("lesson-retry"), "t")
            .await
            .unwrap();
        let engine_dyn = engine.clone() as Arc<dyn LessonContentAgentEngine>;
        let error = service
            .generate_lesson_with_retry(
                &nomifun_common::UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
                &engine_dyn,
                retry_context(),
                None,
                &task,
                std::time::Duration::from_secs(10),
            )
            .await
            .expect_err("fatal error propagates");
        assert!(
            matches!(error, AppError::Conflict(ref message) if message.contains("课程已归档")),
            "original error variant preserved: {error}"
        );
        assert_eq!(engine.generate_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(service.lesson_run_stats().unwrap().stop_reason, "fatal");
    }

    /// 错误两桶的边界：同一轮不同的可重试错误各自记指纹，不触发熔断，预
    /// 算耗尽后以结构化错误收场（不裸 502 文案）。
    #[tokio::test]
    async fn distinct_retryable_errors_exhaust_the_budget_structured() {
        let (service, _dir) = retry_test_service().await;
        let engine = ScriptedLessonEngine::new(
            Arc::clone(&service),
            vec![
                Err(AppError::BadGateway("first cut".into())),
                Err(AppError::BadGateway("second cut".into())),
                Err(AppError::Timeout("hung past the clock".into())),
            ],
        );
        service.set_lesson_engine(engine.clone());
        let task = service
            .begin_generation_task(crate::service::KIND_LESSON, "c", Some("lesson-retry"), "t")
            .await
            .unwrap();
        let engine_dyn = engine.clone() as Arc<dyn LessonContentAgentEngine>;
        let error = service
            .generate_lesson_with_retry(
                &nomifun_common::UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
                &engine_dyn,
                retry_context(),
                None,
                &task,
                std::time::Duration::from_secs(10),
            )
            .await
            .expect_err("budget exhausts");
        let text = error.to_string();
        assert!(text.contains("3 次尝试后仍失败"), "{text}");
        assert!(text.contains("hung past the clock"), "{text}");
        assert_eq!(engine.generate_calls.load(AtomicOrdering::SeqCst), 3);
        let stats = service.lesson_run_stats().unwrap();
        assert_eq!(stats.stop_reason, "exhausted");
    }

    /// 单次尝试显式超时（judge 形态）：引擎悬挂 → 超时按可重试失败处理
    /// （指纹进重试环），同指纹两连 → 熔断收场；悬挂的 future 只弃其果。
    #[tokio::test]
    async fn attempt_timeout_is_a_retryable_failure_and_trips_the_breaker() {
        struct HangingEngine {
            calls: AtomicUsize,
        }
        #[async_trait::async_trait]
        impl LessonContentAgentEngine for HangingEngine {
            async fn generate(
                &self,
                _user_id: &UserId,
                _context: &LessonGenerationContext,
                _model_override: Option<(&str, &str)>,
            ) -> Result<LessonOutput, AppError> {
                self.calls.fetch_add(1, AtomicOrdering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok(fake_output())
            }
            async fn resume(
                &self,
                _user_id: &UserId,
                _draft_id: &str,
                _context: &LessonGenerationContext,
                _model_override: Option<(&str, &str)>,
            ) -> Result<LessonOutput, AppError> {
                panic!("timeout test never resumes (no draft)");
            }
        }
        let (service, _dir) = retry_test_service().await;
        let hanging = Arc::new(HangingEngine { calls: AtomicUsize::new(0) });
        service.set_lesson_engine(hanging.clone());
        let task = service
            .begin_generation_task(crate::service::KIND_LESSON, "c", Some("lesson-retry"), "t")
            .await
            .unwrap();
        let error = service
            .generate_lesson_with_retry(
                &nomifun_common::UserId::parse("0190f5fe-7c00-7a00-8000-000000000001").unwrap(),
                &(hanging.clone() as Arc<dyn LessonContentAgentEngine>),
                retry_context(),
                None,
                &task,
                std::time::Duration::from_millis(50),
            )
            .await
            .expect_err("timeout fingerprints trip the breaker");
        assert!(
            error.to_string().contains("熔断中止")
                && error.to_string().contains("显式限时"),
            "timeout treated as retryable, breaker opens on the repeat: {error}"
        );
        assert_eq!(hanging.calls.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(service.lesson_run_stats().unwrap().stop_reason, "circuit_open");
    }
}
