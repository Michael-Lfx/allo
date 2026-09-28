use super::*;

/// The single implicit module every learning-graph course hangs its node
/// lessons under (`learning_lessons.module_id` is NOT NULL). The graph UI
/// never renders modules; this row only satisfies the relational shape.
pub(crate) const GRAPH_MODULE_TITLE: &str = "学习图";

/// 「下一步推荐学习的节点」一次最多同时展示的数量。
pub(crate) const GRAPH_RECOMMEND_LIMIT: usize = 10;

/// 概念登记表邻近切片的行数上限（防上下文爆炸；登记表远超该值时教练以
/// 花名册/已教账为主，切片只服务撞名预防）。
const REGISTRY_EXCERPT_LIMIT: usize = 200;

impl LearningService {

    // ── Endpoint proposal（建课向导第二步）───────────────────────────────

    /// 为学习目标提议 1-3 条终点锚（scope 分析 + 单次 LLM 提议）。尽力而
    /// 为：提议失败返回空列表，向导引导用户手填。
    pub async fn propose_graph_endpoints(
        &self,
        _user_id: &UserId,
        request: &crate::models::ProposeEndpointsRequest,
    ) -> Result<Vec<crate::models::ProposedEndpointView>, AppError> {
        let topic = request.description.trim();
        if topic.is_empty() {
            return Err(AppError::BadRequest(
                "learning graph topic must not be empty".into(),
            ));
        }
        if topic.chars().count() > 200 {
            return Err(AppError::BadRequest("learning graph topic is too long".into()));
        }
        let completer = self.course_completer()?;
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let scope = crate::learning_graph::analyze_scope(
            completer.as_ref(),
            model_override,
            topic,
        )
        .await;
        let endpoints = crate::learning_graph::propose_endpoints(
            completer.as_ref(),
            model_override,
            topic,
            scope.as_ref(),
        )
        .await;
        Ok(endpoints
            .into_iter()
            .map(|endpoint| crate::models::ProposedEndpointView {
                title: endpoint.title,
                goal_note: endpoint.goal_note,
            })
            .collect())
    }

    // ── Course creation（建课 = 目标 + 终点锚 + 首生长）────────────────────

    /// 学习图课程创建入口（`GenerateCourseRequest.course_kind = learning_graph`）：
    /// 描述即学习目标，`endpoints` 为用户在向导里确认/编辑过的终点锚列表
    /// （为空时由 AI 提议）。课程与终点锚同步落库，罗盘尽力初画，随后
    /// **首生长在后台启动**——接口立即返回课程详情，生长进度走 WS 与
    /// generation_registry（状态/取消端点共用）。
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
        if topic.is_empty() {
            return Err(AppError::BadRequest(
                "learning graph topic must not be empty".into(),
            ));
        }
        if topic.chars().count() > 200 {
            return Err(AppError::BadRequest("learning graph topic is too long".into()));
        }
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let _slot = self.acquire_generation_slot(user_id, "learning-graph-create".to_owned())?;

        let completer = self.course_completer()?;
        // Scope 分析：尽力而为，失败降级为无参考起步。
        let scope = crate::learning_graph::analyze_scope(
            completer.as_ref(),
            model_override,
            &topic,
        )
        .await;
        // 终点锚：用户确认过的优先；没有提议成功的（向导阶段 AI 提议失败
        // 且用户未手填）才现场再提一次。
        let mut endpoints = request
            .endpoints
            .iter()
            .map(|endpoint| crate::learning_graph::ProposedEndpoint {
                title: endpoint.title.trim().to_owned(),
                goal_note: endpoint.goal_note.trim().to_owned(),
            })
            .filter(|endpoint| !endpoint.title.is_empty())
            .collect::<Vec<_>>();
        if endpoints.is_empty() {
            endpoints = crate::learning_graph::propose_endpoints(
                completer.as_ref(),
                model_override,
                &topic,
                scope.as_ref(),
            )
            .await;
        }

        let course_id = LearningCourseId::new();
        let module_id = LearningModuleId::new();
        let now = now_ms();
        let goal_text = scope
            .as_ref()
            .map(|scope| scope.goal.clone())
            .unwrap_or_default();
        let scope_text = scope
            .as_ref()
            .map(|scope| {
                format!("{}（基线：{}）", scope.scope, scope.baseline)
            })
            .unwrap_or_default();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, learning_goal, \
              learning_scope, created_at, updated_at) \
             VALUES (?, ?, '', 'general', 1, 'learning_graph', ?, ?, ?, ?)",
        )
        .bind(course_id.as_str())
        .bind(&topic)
        .bind(if goal_text.is_empty() { topic.as_str() } else { goal_text.as_str() })
        .bind(&scope_text)
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
        for endpoint in &endpoints {
            insert_endpoint(
                &mut transaction,
                &course_id,
                &module_id,
                &endpoint.title,
                &endpoint.goal_note,
                now,
            )
            .await?;
        }
        transaction.commit().await.map_err(internal)?;

        // 罗盘初画 + 首生长都在后台：创建接口不等模型。后台任务要求
        // 'static：模型偏好先持有化再入任务。
        let service = self.clone();
        let owner = user_id.clone();
        let course = course_id.clone();
        let spawn_override = model_override
            .map(|(provider, model)| (provider.clone(), model.to_owned()));
        tokio::spawn(async move {
            let spawn_override = spawn_override
                .as_ref()
                .map(|(provider, model)| (provider, model.as_str()));
            let _ = service
                .regen_compass(&course, &owner, spawn_override)
                .await;
            let _ = service.kick_growth(&owner, &course, true, spawn_override).await;
        });

        self.course_detail(&course_id, Some(user_id)).await
    }

    // ── Growth（就绪 < 3 触发 / 手动 / 建课后首生长，共用一条管道）────────

    /// 触发一次生长。`force = false` 时（节点完成自动触发）就绪存量不低于
    /// [`READY_TRIGGER`] 就什么都不做；`force = true`（手动/首生长）总是
    /// 补到 [`READY_TARGET`]。返回是否真的启动了后台生长。
    pub async fn kick_growth(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        force: bool,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<bool, AppError> {
        let ready_count = self.ready_count(course_id, user_id).await?;
        if !force && ready_count >= crate::learning_graph::READY_TRIGGER {
            return Ok(false);
        }
        // 停摆两臂只约束自动触发；手动 force 永远豁免（ADR-0009 Amendment 1）。
        if !force && self.growth_stalled(course_id).await? {
            return Ok(false);
        }
        // 每课程一把 slot 锁：进行中的生长让重复触发快速退出（自动触发
        // 静默容忍，手动触发报冲突）。
        let slot_key = format!("graph-growth-{}", course_id.as_str());
        let slot = match self.acquire_generation_slot(user_id, slot_key) {
            Ok(slot) => slot,
            Err(_) if !force => return Ok(false),
            Err(error) => return Err(error),
        };
        let engine = crate::learning_graph::GrowthRunner {
            course_id: course_id.clone(),
            user_id: user_id.clone(),
            model_override: model_override
                .map(|(provider, model)| (provider.to_owned(), model.to_owned())),
        };
        let service = self.clone();
        tokio::spawn(async move {
            let _slot = slot; // RAII：任务结束（含 panic）即释放
            if let Err(error) = service.run_growth(&engine).await {
                tracing::warn!(course = engine.course_id.as_str(), %error, "learning graph growth failed");
            }
        });
        Ok(true)
    }

    /// 节点进度变化的生长钩子：完成或跳过都可能改变就绪存量。best-effort，
    /// 任何失败只留日志——不打断学习者当下的操作。
    pub async fn maybe_kick_growth(&self, user_id: &UserId, lesson_id: &LearningLessonId) {
        let course_id: Option<String> = sqlx::query_scalar(
            "SELECT m.course_id FROM learning_modules m \
             JOIN learning_lessons l ON l.module_id = m.module_id \
             WHERE l.lesson_id = ?",
        )
        .bind(lesson_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let Some(course_id) = course_id else {
            return;
        };
        let Ok(course_id) = LearningCourseId::parse(&course_id) else {
            return;
        };
        let kind = self.course_kind(&course_id).await;
        if kind != Some(CourseKind::LearningGraph) {
            return;
        }
        let _ = self.kick_growth(user_id, &course_id, false, None).await;
    }

    /// 停摆两臂（ADR-0009 Amendment 1）：臂 1 = 空图（无学习节点，建课瞬间
    /// 由首生长 force 走，不占自动触发）；臂 2 = 课程有终点且全部被教练标记
    /// 完成。零终点课程第二臂不成立（无裁决即无完成）。返回 true = 自动
    /// 触发退出。
    async fn growth_stalled(&self, course_id: &LearningCourseId) -> Result<bool, AppError> {
        let learning_nodes: i64 = sqlx::query_scalar(
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
        if learning_nodes == 0 {
            return Ok(true);
        }
        let (endpoints, completed): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(completed), 0) \
             FROM learning_course_endpoints WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        Ok(endpoints > 0 && endpoints == completed)
    }

    /// 一轮生长：上下文组装 → 教练单次起草 → 结构门 →（打回重裁 ×1）→
    /// AI 概念评审 →（打回重裁 ×1）→ 事务落库（铸名/节点/概念网/批次）。
    /// 全程经 generation_registry 暴露状态、可取消；结束发 WS 终态帧。
    async fn run_growth(
        &self,
        engine: &crate::learning_graph::GrowthRunner,
    ) -> Result<(), AppError> {
        let course_id = &engine.course_id;
        let _run = self.begin_growth_run(course_id);
        // phase 字段对齐前端过程视图的形状约定（started/round/failed）。
        self.emit_graph_event(
            course_id,
            "growth_started",
            serde_json::json!({
                "phase": "started",
                "text": "教练正在阅读罗盘与花名册，起草本批节点…",
            }),
        );
        // 批行状态机：生长开始即落 pending 行（slot 串行保证此课程此刻
        // 不可能有别的生长在跑，残留的 pending 一律是上次中断 → 转 failed）。
        let pending = self.begin_pending_batch(course_id).await?;
        let result = self.growth_body(engine, &pending).await;
        match &result {
            Ok(applied) => {
                // 空手批的 pending 行已在 growth_body 内自清；WS 以
                // outcome=idle 收尾——前端刷新就绪读数即可，无历史噪音。
                if applied.verdicts > 0 {
                    // 教练裁决改了完成位：与终点编辑同权，触发罗盘重画。
                    self.schedule_compass_regen(course_id, &engine.user_id);
                }
                self.emit_graph_event(
                    course_id,
                    "growth_completed",
                    serde_json::json!({
                        "phase": "completed",
                        "outcome": if applied.idle { "idle" } else { "applied" },
                        "batch": applied.batch_id,
                        "nodes": applied.node_count,
                        "note": applied.note,
                        "ready": applied.ready_count,
                    }),
                );
            }
            Err(error) => {
                // 失败留痕：pending 行转 failed，历史时间线可见可重试。
                self.fail_pending_batch(&pending.batch_id).await;
                let cancelled = self.generation_cancel_requested();
                self.emit_graph_event(
                    course_id,
                    if cancelled { "growth_cancelled" } else { "growth_failed" },
                    serde_json::json!({
                        "phase": "failed",
                        "error": error.to_string(),
                    }),
                );
            }
        }
        result.map(|_| ())
    }

    /// 预留一条 pending 批行（先于任何 LLM 调用）：seq 单调分配，残留的
    /// pending（上次崩溃/重启的中断痕迹）就地转 failed。
    async fn begin_pending_batch(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<PendingBatch, AppError> {
        sqlx::query(
            "UPDATE learning_growth_batches SET status = 'failed' \
             WHERE course_id = ? AND status = 'pending'",
        )
        .bind(course_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        let seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq) + 1, 1) FROM learning_growth_batches WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let batch_id = LearningGrowthBatchId::new().into_string();
        sqlx::query(
            "INSERT INTO learning_growth_batches \
             (batch_id, course_id, seq, node_ids_json, status, created_at) \
             VALUES (?, ?, ?, '[]', 'pending', ?)",
        )
        .bind(&batch_id)
        .bind(course_id.as_str())
        .bind(seq)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(PendingBatch { batch_id })
    }

    async fn fail_pending_batch(&self, batch_id: &str) {
        let _ = sqlx::query(
            "UPDATE learning_growth_batches SET status = 'failed' WHERE batch_id = ?",
        )
        .bind(batch_id)
        .execute(&self.pool)
        .await;
    }

    async fn delete_pending_batch(&self, batch_id: &str) {
        let _ = sqlx::query("DELETE FROM learning_growth_batches WHERE batch_id = ?")
            .bind(batch_id)
            .execute(&self.pool)
            .await;
    }

    async fn growth_body(
        &self,
        engine: &crate::learning_graph::GrowthRunner,
        pending: &PendingBatch,
    ) -> Result<GrowthApplied, AppError> {
        let course_id = &engine.course_id;
        let completer = self.course_completer()?;
        let model_override = engine
            .model_override
            .as_ref()
            .map(|(provider, model)| (provider, model.as_str()));
        let context = self.build_coach_context(course_id, engine, completer.as_ref()).await?;

        // 至多两稿：首稿 + 一次打回重裁（结构门与概念评审共用重裁预算）。
        let mut batch = crate::learning_graph::coach_draft(
            completer.as_ref(),
            model_override,
            &context,
        )
        .await?;
        for attempt in 0..2 {
            let gates = self.gate_batch(course_id, &batch).await?;
            if !gates.is_empty() {
                if attempt == 1 {
                    return Err(AppError::UnprocessableEntity(format!(
                        "生长批次两稿均未通过结构门：\n{}",
                        crate::learning_graph::format_gate_report(&gates)
                    )));
                }
                self.emit_graph_event(course_id, "growth_redraft", serde_json::json!({
                    "stage": "gates",
                    "problems": crate::learning_graph::format_gate_report(&gates),
                }));
                batch = crate::learning_graph::coach_redraft(
                    completer.as_ref(),
                    model_override,
                    &context,
                    &crate::learning_graph::format_gate_report(&gates),
                )
                .await?;
                continue;
            }
            let problems = crate::learning_graph::concept_review(
                completer.as_ref(),
                model_override,
                &batch,
                &context,
            )
            .await;
            if !problems.is_empty() {
                if attempt == 1 {
                    return Err(AppError::UnprocessableEntity(format!(
                        "生长批次两稿均未通过概念评审：\n{}",
                        crate::learning_graph::format_review_problems(&problems)
                    )));
                }
                self.emit_graph_event(course_id, "growth_redraft", serde_json::json!({
                    "stage": "concept_review",
                    "problems": crate::learning_graph::format_review_problems(&problems),
                }));
                batch = crate::learning_graph::coach_redraft(
                    completer.as_ref(),
                    model_override,
                    &context,
                    &crate::learning_graph::format_review_problems(&problems),
                )
                .await?;
            }
        }
        if batch.nodes.is_empty() {
            // 空手批：不落档案（run_growth 删 pending 行），终点裁决仍生效
            // ——「裁决终点完成」不需要以落一批节点为代价。
            let mut transaction = self.pool.begin().await.map_err(internal)?;
            let verdicts =
                self.apply_endpoint_verdicts(&mut transaction, course_id, &batch).await?;
            transaction.commit().await.map_err(internal)?;
            // 行内自清：pending 在本分支内删除后再返回，外部观察者永远看
            // 不到"裁决已生效但仍挂着 pending 行"的中间态。
            self.delete_pending_batch(&pending.batch_id).await;
            let ready_count = self.ready_count_for_owner(course_id, &engine.user_id).await?;
            return Ok(GrowthApplied {
                batch_id: pending.batch_id.clone(),
                node_count: 0,
                note: batch.note.clone(),
                ready_count,
                idle: true,
                verdicts,
            });
        }
        self.apply_batch(course_id, engine, pending, &batch).await
    }

    /// 教练上下文：目标/范围/罗盘/花名册/已教账/终点读数/登记表切片。
    /// 罗盘缺失时先重画一次（尽力而为，缺失以空罗盘继续）。
    async fn build_coach_context(
        &self,
        course_id: &LearningCourseId,
        engine: &crate::learning_graph::GrowthRunner,
        completer: &dyn LearningCompleter,
    ) -> Result<crate::learning_graph::CoachContext, AppError> {
        let course = sqlx::query(
            "SELECT title, learning_goal, learning_scope, compass_md, compass_updated_at \
             FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let scope: String = course.try_get("learning_scope").map_err(internal)?;
        let compass: Option<String> = course.try_get("compass_md").map_err(internal)?;

        // 罗盘缺失（创建时初画失败）则现场重画一次。
        let compass = match compass {
            Some(compass) if !compass.trim().is_empty() => compass,
            _ => {
                let endpoints = self.endpoint_pairs(course_id).await?;
                let goal_or_title = if goal.is_empty() { &title } else { &goal };
                let drawn = crate::learning_graph::draw_compass(
                    completer,
                    engine.model_override(),
                    goal_or_title,
                    &endpoints,
                    None,
                )
                .await
                .ok();
                match drawn {
                    Some(drawn) => {
                        let _ = sqlx::query(
                            "UPDATE learning_courses SET compass_md = ?, compass_updated_at = ? \
                             WHERE course_id = ?",
                        )
                        .bind(&drawn)
                        .bind(now_ms())
                        .bind(course_id.as_str())
                        .execute(&self.pool)
                        .await;
                        drawn
                    }
                    None => String::new(),
                }
            }
        };

        // 罗盘陈旧读数：重画之后又落了多少个已应用批次。
        let compass_updated_at: Option<i64> =
            course.try_get("compass_updated_at").map_err(internal)?;
        let compass_stale: i64 = match compass_updated_at {
            Some(at) => sqlx::query_scalar(
                "SELECT COUNT(*) FROM learning_growth_batches \
                 WHERE course_id = ? AND status = 'applied' AND created_at > ?",
            )
            .bind(course_id.as_str())
            .bind(at)
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?,
            None => 0,
        };
        let compass_stale_batches = compass_stale as usize;

        let ready_count = self.ready_count_for_owner(course_id, &engine.user_id).await?;
        let endpoints = self.render_endpoint_list(course_id).await?;
        let coverage_gauge = self.render_coverage_gauge(course_id, &engine.user_id).await?;
        // 行为摘要先折（卡点节点要在花名册行上加 ⚑）。
        let digest = self.fold_behavior_digest(course_id, &engine.user_id).await?;
        let stalled: Vec<String> = digest
            .as_ref()
            .map(|digest| digest.blockers.iter().map(|b| b.lesson_id.clone()).collect())
            .unwrap_or_default();
        let roster = self.render_roster(course_id, &engine.user_id, &stalled).await?;
        let lesson_titles = self.lesson_titles(course_id).await?;
        let behavior_digest = digest
            .map(|digest| digest.render(&lesson_titles))
            .unwrap_or_default();
        let taught_summary = self.render_taught_summary().await?;
        let registry_excerpt = self.render_registry_excerpt().await?;

        Ok(crate::learning_graph::CoachContext {
            goal: if goal.is_empty() { title } else { goal },
            scope_reference: scope,
            compass,
            compass_stale_batches,
            roster,
            taught_summary,
            endpoints,
            coverage_gauge,
            behavior_digest,
            ready_count,
            registry_excerpt,
        })
    }

    /// 课程内课时标题表（行为摘要渲染卡点用）。
    async fn lesson_titles(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<HashMap<String, String>, AppError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT l.lesson_id, l.title FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().collect())
    }

    /// 行为摘要折叠（ADR-0009 Amendment 1 三件：趋势 / 卡点 / 真实保留率）。
    /// 返回 None = 课程无作答历史（窗口为空，教练上下文整块省略）。
    async fn fold_behavior_digest(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<Option<crate::learning_graph::BehaviorDigest>, AppError> {
        let settings = self.scheduler_settings().await;
        let tz = settings.tz_offset_minutes;
        let now = now_ms();
        let today = crate::scheduler::review_day_number(now, tz);
        // 窗口候选作答：近 60 学习日足够覆盖 7/10 双条款窗口。
        let attempt_rows: Vec<(String, i64, i64, f64)> = sqlx::query_as(
            "SELECT l.lesson_id, a.created_at, a.passed, a.score FROM learning_attempts a \
             JOIN learning_enrollments e ON e.enrollment_id = a.enrollment_id AND e.user_id = ? \
             JOIN learning_activities act ON act.activity_id = a.activity_id \
             JOIN learning_lessons l ON l.lesson_id = act.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? AND a.created_at >= ?",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .bind(now - 60 * 24 * 60 * 60 * 1000)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        if attempt_rows.is_empty() {
            return Ok(None);
        }
        let attempts: Vec<crate::learning_graph::DigestAttempt> = attempt_rows
            .iter()
            .map(|(lesson_id, created_at, passed, score)| {
                crate::learning_graph::DigestAttempt {
                    lesson_id: lesson_id.clone(),
                    day: crate::scheduler::review_day_number(*created_at, tz),
                    correct: *passed == 1 || *score >= 0.6,
                }
            })
            .collect();

        // 停滞基线（全历史）：最近一次答对的学习日，从未答对则首次作答日。
        let base_rows: Vec<(String, Option<i64>, i64)> = sqlx::query_as(
            "SELECT l.lesson_id, \
                    MAX(CASE WHEN a.passed = 1 OR a.score >= 0.6 THEN a.created_at END), \
                    MIN(a.created_at) \
             FROM learning_attempts a \
             JOIN learning_enrollments e ON e.enrollment_id = a.enrollment_id AND e.user_id = ? \
             JOIN learning_activities act ON act.activity_id = a.activity_id \
             JOIN learning_lessons l ON l.lesson_id = act.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? GROUP BY l.lesson_id",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let stagnation_base: HashMap<String, i64> = base_rows
            .into_iter()
            .map(|(lesson_id, last_correct, first_attempt)| {
                let base = last_correct.unwrap_or(first_attempt);
                (lesson_id, crate::scheduler::review_day_number(base, tz))
            })
            .collect();

        // 到期推进（真实保留率口径）：本课程的课程卡、auto/self 评分。
        let push_rows: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT rl.review_day, rl.rating FROM learning_review_log rl \
             JOIN learning_review_items ri ON ri.review_item_id = rl.item_id \
             JOIN learning_enrollments e ON e.enrollment_id = ri.enrollment_id AND e.user_id = ? \
             JOIN learning_activities act ON act.activity_id = ri.activity_id \
             JOIN learning_lessons l ON l.lesson_id = act.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? AND rl.source = 'course' \
               AND rl.rating_source IN ('auto', 'self')",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let pushes: Vec<crate::learning_graph::DigestPush> = push_rows
            .into_iter()
            .map(|(day, rating)| crate::learning_graph::DigestPush { day, passed: rating >= 2 })
            .collect();

        Ok(Some(crate::learning_graph::fold_behavior_digest(
            &attempts, &pushes, today, &stagnation_base,
        )))
    }

    /// 覆盖读数（共用存量）：total = 课程全部非终点节点，met = completed、
    /// skipped 单列，附概念档位足迹（ADR-0009 Amendment 1）。
    async fn render_coverage_gauge(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<String, AppError> {
        let user_value = user_id.as_str();
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT COALESCE(p.status, 'not_started'), l.title FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             LEFT JOIN learning_enrollments e ON e.course_id = m.course_id AND e.user_id = ? \
             LEFT JOIN learning_lesson_progress p \
               ON p.lesson_id = l.lesson_id AND p.enrollment_id = e.enrollment_id \
             WHERE m.course_id = ? AND l.lesson_id NOT IN \
               (SELECT lesson_id FROM learning_course_endpoints WHERE course_id = ?) \
             ORDER BY l.position, l.lesson_id",
        )
        .bind(user_value)
        .bind(course_id.as_str())
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut met = 0usize;
        let mut skipped = 0usize;
        let mut pending_titles = Vec::new();
        for (status, title) in &rows {
            match LessonStatus::try_from(status.as_str()).map_err(AppError::Internal)? {
                LessonStatus::Completed => met += 1,
                LessonStatus::Skipped => skipped += 1,
                _ => pending_titles.push(title.clone()),
            }
        }
        // 概念档位足迹：本课程 teaches 的概念按最高档折叠计数。
        let tier_rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT lc.concept_id, lc.tier FROM learning_lesson_concepts lc \
             JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? AND lc.role = 'teaches'",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut best: HashMap<String, crate::learning_graph::ConceptTier> = HashMap::new();
        for (concept_id, tier) in &tier_rows {
            let Some(tier) = crate::learning_graph::ConceptTier::try_from_str(tier) else {
                continue;
            };
            let entry = best.entry(concept_id.clone()).or_insert(tier);
            if tier > *entry {
                *entry = tier;
            }
        }
        let mut counts = [0usize; 3];
        for tier in best.values() {
            counts[*tier as usize] += 1;
        }
        let tier_footprint = [
            (crate::learning_graph::ConceptTier::Teach, counts[2]),
            (crate::learning_graph::ConceptTier::Apply, counts[1]),
            (crate::learning_graph::ConceptTier::Know, counts[0]),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect();
        Ok(crate::learning_graph::render_coverage_gauge(
            &crate::learning_graph::CoverageReading {
                total: rows.len(),
                met,
                skipped,
                pending_titles,
                tier_footprint,
            },
        ))
    }

    /// 结构门所需的全部事实（全局已教账按 canonical 名折叠）。
    async fn gate_batch(
        &self,
        course_id: &LearningCourseId,
        batch: &crate::learning_graph::ProposedBatch,
    ) -> Result<Vec<crate::learning_graph::GateError>, AppError> {
        // 已教账（名字 → 最高档）：全部课程的 teaches。
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT reg.canonical, lc.tier FROM learning_lesson_concepts lc \
             JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
             WHERE lc.role = 'teaches'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut coverage = crate::learning_graph::ConceptCoverage::new();
        for (canonical, tier) in rows {
            let Some(tier) = crate::learning_graph::ConceptTier::try_from_str(&tier) else {
                continue;
            };
            let entry = coverage.entry(canonical.trim().to_lowercase()).or_insert(tier);
            if tier > *entry {
                *entry = tier;
            }
        }

        // 登记表全部名字（canonical ∪ 别名，小写）。
        let registry_names = self.registry_name_set().await?;

        // 终点标题 + 课程已有节点标题。
        let endpoint_titles: HashSet<String> = self
            .endpoint_pairs(course_id)
            .await?
            .into_iter()
            .map(|(title, _)| title.trim().to_lowercase())
            .collect();
        let existing_titles: HashSet<String> = sqlx::query_scalar(
            "SELECT lower(l.title) FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?
        .into_iter()
        .collect();

        Ok(crate::learning_graph::validate_batch(
            batch,
            &coverage,
            &registry_names,
            &endpoint_titles,
            &existing_titles,
        ))
    }

    /// 落库：铸名 → 节点 → 概念网 → 完成裁决 → 批次档案，单事务。
    async fn apply_batch(
        &self,
        course_id: &LearningCourseId,
        engine: &crate::learning_graph::GrowthRunner,
        pending: &PendingBatch,
        batch: &crate::learning_graph::ProposedBatch,
    ) -> Result<GrowthApplied, AppError> {
        // 模块与下一 position。
        let module_id: String = sqlx::query_scalar(
            "SELECT module_id FROM learning_modules WHERE course_id = ? \
             ORDER BY position LIMIT 1",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let next_position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(l.position) + 1, 0) FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let now = now_ms();

        // 1) 铸名（canonical/别名唯一性已过门；事务内幂等重查防并发）。
        let mut name_to_concept: HashMap<String, String> = HashMap::new();
        for mint in &batch.mints {
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT concept_id FROM learning_concept_registry \
                 WHERE lower(canonical) = ? \
                 OR EXISTS (SELECT 1 FROM json_each(aliases_json) \
                            WHERE lower(json_each.value) = ?)",
            )
            .bind(mint.canonical.trim().to_lowercase())
            .bind(mint.canonical.trim().to_lowercase())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(internal)?;
            let concept_id = match existing {
                Some(concept_id) => concept_id,
                None => {
                    let concept_id = LearningConceptId::new().into_string();
                    let aliases = serde_json::to_string(
                        &mint.aliases.iter().map(|alias| alias.trim()).collect::<Vec<_>>(),
                    )
                    .map_err(internal)?;
                    sqlx::query(
                        "INSERT INTO learning_concept_registry \
                         (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
                         VALUES (?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&concept_id)
                    .bind(mint.canonical.trim())
                    .bind(aliases)
                    .bind(mint.definition.trim())
                    .bind(now)
                    .bind(now)
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
                    concept_id
                }
            };
            name_to_concept.insert(mint.canonical.trim().to_lowercase(), concept_id.clone());
            for alias in &mint.aliases {
                name_to_concept.insert(alias.trim().to_lowercase(), concept_id.clone());
            }
        }

        // 2) 节点 + 3) 概念网。
        let mut node_ids: Vec<String> = Vec::with_capacity(batch.nodes.len());
        for (offset, node) in batch.nodes.iter().enumerate() {
            let lesson_id = LearningLessonId::new();
            let minutes = node.minutes.unwrap_or(10).max(1) as i64;
            sqlx::query(
                "INSERT INTO learning_lessons \
                 (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
                  content_generated) \
                 VALUES (?, ?, ?, '', ?, ?, ?, 0)",
            )
            .bind(lesson_id.as_str())
            .bind(&module_id)
            .bind(node.title.trim())
            .bind(node.purpose.trim())
            .bind(next_position + offset as i64)
            .bind(minutes)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
            for concept in node.teaches.iter().chain(node.assumes.iter()) {
                let role = if node.teaches.contains(concept) { "teaches" } else { "assumes" };
                // 同名引用同一概念：teaches 里出现过的名字先入账，assumes
                // 引用同一名字时不再插（role 不同可共存，名字相同且 role
                // 相同才是重复）。
                let key = concept.name.trim().to_lowercase();
                let concept_id = match name_to_concept.get(&key) {
                    Some(concept_id) => concept_id.clone(),
                    None => {
                        let resolved: Option<String> = sqlx::query_scalar(
                            "SELECT concept_id FROM learning_concept_registry \
                             WHERE lower(canonical) = ? \
                             OR EXISTS (SELECT 1 FROM json_each(aliases_json) \
                                        WHERE lower(json_each.value) = ?)",
                        )
                        .bind(&key)
                        .bind(&key)
                        .fetch_optional(&mut *transaction)
                        .await
                        .map_err(internal)?;
                        resolved.ok_or_else(|| {
                            AppError::Internal(format!(
                                "concept {key} unresolved at apply time (gate miss)"
                            ))
                        })?
                    }
                };
                let exists: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM learning_lesson_concepts \
                     WHERE lesson_id = ? AND concept_id = ? AND role = ?",
                )
                .bind(lesson_id.as_str())
                .bind(&concept_id)
                .bind(role)
                .fetch_one(&mut *transaction)
                .await
                .map_err(internal)?;
                if exists == 0 {
                    sqlx::query(
                        "INSERT INTO learning_lesson_concepts \
                         (lesson_id, concept_id, role, tier) VALUES (?, ?, ?, ?)",
                    )
                    .bind(lesson_id.as_str())
                    .bind(&concept_id)
                    .bind(role)
                    .bind(concept.tier.as_str())
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
                }
            }
            node_ids.push(lesson_id.into_string());
        }

        // 4) 终点完成裁决（教练的标记位，机器从不自动置位；双向可重开）。
        let verdicts = self.apply_endpoint_verdicts(&mut transaction, course_id, batch).await?;

        // 5) 批次档案转正（pending 行已在生长开始时落库，seq 早已分配）。
        let node_ids_json = serde_json::to_string(&node_ids).map_err(internal)?;
        sqlx::query(
            "UPDATE learning_growth_batches SET node_ids_json = ?, note = ?, status = 'applied' \
             WHERE batch_id = ?",
        )
        .bind(&node_ids_json)
        .bind(batch.note.trim())
        .bind(pending.batch_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        transaction.commit().await.map_err(internal)?;

        let ready_count = self.ready_count_for_owner(course_id, &engine.user_id).await?;
        Ok(GrowthApplied {
            batch_id: pending.batch_id.clone(),
            node_count: node_ids.len(),
            note: batch.note.clone(),
            ready_count,
            idle: false,
            verdicts,
        })
    }

    /// 应用终点完成裁决（教练双向：置完成或重开）。返回裁决条数。
    async fn apply_endpoint_verdicts(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        course_id: &LearningCourseId,
        batch: &crate::learning_graph::ProposedBatch,
    ) -> Result<usize, AppError> {
        for verdict in &batch.completed_endpoints {
            sqlx::query(
                "UPDATE learning_course_endpoints SET completed = ? \
                 WHERE course_id = ? AND lower(title) = ?",
            )
            .bind(if verdict.completed { 1 } else { 0 })
            .bind(course_id.as_str())
            .bind(verdict.title.trim().to_lowercase())
            .execute(&mut **transaction)
            .await
            .map_err(internal)?;
        }
        Ok(batch.completed_endpoints.len())
    }

    // ── Readiness（就绪集合的读取侧）────────────────────────────────────

    /// 全局已教账：concept_id → 最高已教档位（跨课程）。
    async fn taught_ledger(&self) -> Result<crate::learning_graph::ConceptCoverage, AppError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT concept_id, tier FROM learning_lesson_concepts WHERE role = 'teaches'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let teaches = rows
            .into_iter()
            .filter_map(|(concept_id, tier)| {
                crate::learning_graph::ConceptTier::try_from_str(&tier)
                    .map(|tier| (concept_id, tier))
            });
        Ok(crate::learning_graph::taught_ledger(teaches))
    }

    /// 一门课程的就绪候选（全部节点及其 assumes + 学习者满足状态）。
    async fn ready_candidates(
        &self,
        course_id: &LearningCourseId,
        user_id: Option<&UserId>,
    ) -> Result<Vec<crate::learning_graph::ReadyCandidate>, AppError> {
        let lesson_rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT l.lesson_id, l.title, p.status FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             LEFT JOIN learning_enrollments e ON e.course_id = m.course_id AND e.user_id = ? \
             LEFT JOIN learning_lesson_progress p \
               ON p.lesson_id = l.lesson_id AND p.enrollment_id = e.enrollment_id \
             WHERE m.course_id = ? \
             ORDER BY l.position, l.lesson_id",
        )
        .bind(user_id.map(UserId::as_str).unwrap_or(""))
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;

        let assumes: HashMap<String, Vec<(String, crate::learning_graph::ConceptTier)>> =
            sqlx::query_as(
                "SELECT lc.lesson_id, lc.concept_id, lc.tier FROM learning_lesson_concepts lc \
                 JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
                 JOIN learning_modules m ON m.module_id = l.module_id \
                 WHERE m.course_id = ? AND lc.role = 'assumes'",
            )
            .bind(course_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?
            .into_iter()
            .fold(
                HashMap::<String, Vec<(String, crate::learning_graph::ConceptTier)>>::new(),
                |mut map, (lesson_id, concept_id, tier): (String, String, String)| {
                    if let Some(tier) = crate::learning_graph::ConceptTier::try_from_str(&tier) {
                        map.entry(lesson_id).or_insert_with(Vec::new).push((concept_id, tier));
                    }
                    map
                },
            );

        // 终点标记课时不是学习节点，永不进候选。
        let endpoint_lessons: HashSet<String> = sqlx::query_scalar(
            "SELECT lesson_id FROM learning_course_endpoints WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?
        .into_iter()
        .collect();

        let candidates = lesson_rows
            .into_iter()
            .filter(|(lesson_id, _, _)| !endpoint_lessons.contains(lesson_id))
            .map(|(lesson_id, title, status)| {
                let satisfied = status
                    .as_deref()
                    .and_then(|status| LessonStatus::try_from(status).ok())
                    .map(|status| status.satisfies())
                    .unwrap_or(false);
                let assumes_for_lesson = assumes.get(&lesson_id).cloned().unwrap_or_default();
                crate::learning_graph::ReadyCandidate {
                    lesson_id,
                    title,
                    assumes: assumes_for_lesson,
                    satisfied,
                }
            })
            .collect();
        Ok(candidates)
    }

    /// 就绪集（推荐序 = 花名册序）。
    pub(super) async fn ready_set_for(
        &self,
        course_id: &LearningCourseId,
        user_id: Option<&UserId>,
    ) -> Result<Vec<LearningLessonId>, AppError> {
        let ledger = self.taught_ledger().await?;
        let candidates = self.ready_candidates(course_id, user_id).await?;
        Ok(crate::learning_graph::ready_set(&candidates, &ledger)
            .into_iter()
            .map(parse_id::<LearningLessonId>)
            .collect::<Result<Vec<_>, AppError>>()?)
    }

    async fn ready_count_for_owner(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<usize, AppError> {
        Ok(self.ready_set_for(course_id, Some(user_id)).await?.len())
    }

    /// 触发判定用的就绪存量。无 enrollment 时按零进度者视角（建课瞬间的
    /// 首生长即此口径）。
    async fn ready_count(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<usize, AppError> {
        self.ready_count_for_owner(course_id, user_id).await
    }

    // ── Endpoints（终点锚 CRUD + 罗盘重画）───────────────────────────────

    pub async fn add_endpoint(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        request: &crate::models::EndpointInput,
    ) -> Result<crate::models::GraphEndpointView, AppError> {
        self.require_graph_course(course_id).await?;
        let title = request.title.trim();
        if title.is_empty() {
            return Err(AppError::BadRequest("endpoint title must not be empty".into()));
        }
        let module_id: String = sqlx::query_scalar(
            "SELECT module_id FROM learning_modules WHERE course_id = ? ORDER BY position LIMIT 1",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("course {course_id}")))?;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let module: LearningModuleId = LearningModuleId::parse(&module_id)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let view = insert_endpoint(
            &mut transaction,
            course_id,
            &module,
            title,
            request.goal_note.trim(),
            now_ms(),
        )
        .await?;
        transaction.commit().await.map_err(internal)?;
        self.schedule_compass_regen(course_id, user_id);
        Ok(view)
    }

    pub async fn update_endpoint(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        endpoint_id: &LearningEndpointId,
        request: &crate::models::EndpointUpdateInput,
    ) -> Result<(), AppError> {
        let title_changed = if let Some(title) = request.title.as_deref() {
            let title = title.trim();
            if title.is_empty() {
                return Err(AppError::BadRequest("endpoint title must not be empty".into()));
            }
            sqlx::query(
                "UPDATE learning_course_endpoints SET title = ? \
                 WHERE endpoint_id = ? AND course_id = ?",
            )
            .bind(title)
            .bind(endpoint_id.as_str())
            .bind(course_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(internal)?
            .rows_affected()
                == 1
        } else {
            false
        };
        let note_changed = if let Some(note) = request.goal_note.as_deref() {
            sqlx::query(
                "UPDATE learning_course_endpoints SET goal_note = ? \
                 WHERE endpoint_id = ? AND course_id = ?",
            )
            .bind(note.trim())
            .bind(endpoint_id.as_str())
            .bind(course_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(internal)?
            .rows_affected()
                == 1
        } else {
            false
        };
        // 完成位切换：用户与教练裁决同权（置完成/重开，ADR-0009 Amendment 1）。
        let completed_changed = if let Some(completed) = request.completed {
            sqlx::query(
                "UPDATE learning_course_endpoints SET completed = ? \
                 WHERE endpoint_id = ? AND course_id = ?",
            )
            .bind(if completed { 1 } else { 0 })
            .bind(endpoint_id.as_str())
            .bind(course_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(internal)?
            .rows_affected()
                == 1
        } else {
            false
        };
        // 终点标题变化同步到标记课时行（同一标题在课程内唯一）。
        if title_changed {
            sqlx::query(
                "UPDATE learning_lessons SET title = (SELECT title FROM learning_course_endpoints \
                 WHERE endpoint_id = ?) WHERE lesson_id = (SELECT lesson_id \
                 FROM learning_course_endpoints WHERE endpoint_id = ?)",
            )
            .bind(endpoint_id.as_str())
            .bind(endpoint_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        }
        let _ = user_id;
        if title_changed || note_changed || completed_changed {
            self.schedule_compass_regen(course_id, user_id);
        }
        Ok(())
    }

    pub async fn delete_endpoint(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        endpoint_id: &LearningEndpointId,
    ) -> Result<(), AppError> {
        let lesson_id: Option<String> = sqlx::query_scalar(
            "SELECT lesson_id FROM learning_course_endpoints WHERE endpoint_id = ? AND course_id = ?",
        )
        .bind(endpoint_id.as_str())
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let Some(lesson_id) = lesson_id else {
            return Err(AppError::NotFound(format!("endpoint {endpoint_id}")));
        };
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query("DELETE FROM learning_lesson_progress WHERE lesson_id = ?")
            .bind(&lesson_id)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM learning_lessons WHERE lesson_id = ?")
            .bind(&lesson_id)
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM learning_course_endpoints WHERE endpoint_id = ?")
            .bind(endpoint_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        self.schedule_compass_regen(course_id, user_id);
        Ok(())
    }

    /// 手动重画罗盘（UI 罗盘卡的"重画"入口；与终点变更触发的自动重画
    /// 同管道，ADR-0009 Amendment 1）。
    pub async fn redraw_graph_compass(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
    ) -> Result<(), AppError> {
        self.require_graph_course(course_id).await?;
        self.regen_compass(course_id, user_id, None).await
    }

    /// 终点任何变更（增/删/改）都会重画罗盘：后台尽力执行，失败留待下次
    /// 变更或生长前的兜底重画。
    pub(crate) fn schedule_compass_regen(&self, course_id: &LearningCourseId, user_id: &UserId) {
        let service = self.clone();
        let course = course_id.clone();
        let owner = user_id.clone();
        tokio::spawn(async move {
            if let Err(error) = service.regen_compass(&course, &owner, None).await {
                tracing::warn!(course = course.as_str(), %error, "compass regen failed");
            }
        });
    }

    pub(crate) async fn regen_compass(
        &self,
        course_id: &LearningCourseId,
        _user_id: &UserId,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<(), AppError> {
        let completer = self.course_completer()?;
        let course = sqlx::query(
            "SELECT title, learning_goal FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound(format!("course {course_id}")))?;
        let title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let endpoints = self.endpoint_pairs(course_id).await?;
        let taught = self.render_taught_summary().await?;
        let taught_summary = if taught.is_empty() { None } else { Some(taught.as_str()) };
        let compass = crate::learning_graph::draw_compass(
            completer.as_ref(),
            model_override,
            if goal.is_empty() { title.as_str() } else { goal.as_str() },
            &endpoints,
            taught_summary,
        )
        .await?;
        sqlx::query(
            "UPDATE learning_courses SET compass_md = ?, compass_updated_at = ? WHERE course_id = ?",
        )
        .bind(&compass)
        .bind(now_ms())
        .bind(course_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.emit_graph_event(course_id, "compass_updated", serde_json::json!({}));
        Ok(())
    }

    // ── Views（图视图 / 学习记录 / 概念表）───────────────────────────────

    /// 组装学习图课程视图：终点锚 + 罗盘 + 就绪集推荐 + 水位读数。
    /// DAG 画布已不存在——节点本体就是模块课时，图视图只承载生长语义。
    pub(super) async fn assemble_learning_graph_view(
        &self,
        course_id: &LearningCourseId,
        user_id: Option<&UserId>,
    ) -> Result<crate::models::LearningGraphView, AppError> {
        let course = sqlx::query(
            "SELECT learning_goal, learning_scope, compass_md, compass_updated_at \
             FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let scope: String = course.try_get("learning_scope").map_err(internal)?;
        let compass: Option<String> = course.try_get("compass_md").map_err(internal)?;
        let compass_updated_at: Option<i64> =
            course.try_get("compass_updated_at").map_err(internal)?;

        let endpoint_rows = sqlx::query(
            "SELECT endpoint_id, lesson_id, title, goal_note, completed, declared_at \
             FROM learning_course_endpoints WHERE course_id = ? ORDER BY declared_at, endpoint_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let endpoints = endpoint_rows
            .iter()
            .map(|row| {
                Ok(crate::models::GraphEndpointView {
                    endpoint_id: row.try_get("endpoint_id").map_err(internal)?,
                    lesson_id: parse_id(row.try_get::<String, _>("lesson_id").map_err(internal)?)?,
                    title: row.try_get("title").map_err(internal)?,
                    goal_note: row.try_get("goal_note").map_err(internal)?,
                    completed: row.try_get::<i64, _>("completed").map_err(internal)? != 0,
                    declared_at: row.try_get("declared_at").map_err(internal)?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;

        // R 软闸分桶：结构就绪集合不变（ready_count 是水位口径），被挡的
        // 候选从推荐里剔除并出建议（ADR-0009 Amendment 1）。
        let (ready, blocked) = match user_id {
            Some(user_id) => self.ready_partition(course_id, user_id).await?,
            None => (self.ready_set_for(course_id, None).await?, Vec::new()),
        };
        let ready_count = ready.len();
        let recommended = ready.into_iter().take(GRAPH_RECOMMEND_LIMIT).collect();

        Ok(crate::models::LearningGraphView {
            goal,
            scope,
            compass,
            compass_updated_at,
            endpoints,
            recommended,
            blocked,
            ready_count,
            ready_target: crate::learning_graph::READY_TARGET,
            ready_trigger: crate::learning_graph::READY_TRIGGER,
            growth_running: self.generation_running(),
        })
    }

    /// R 软闸分桶（ADR-0009 Amendment 1）：就绪候选按「供给节点代表预测
    /// 回忆率 ≥ 0.85」分为推荐与被挡。供给节点 = 教该候选所假定概念的节点
    /// （跨课程）；代表 R = 该节点为该用户名下全部复习卡的最小预测回忆率，
    /// 无卡视为满血 1.0。被挡条目按 R 升序最多 3 条。
    pub(super) async fn ready_partition(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<(Vec<LearningLessonId>, Vec<crate::models::GraphBlockedView>), AppError> {
        const R_GATE: f64 = 0.85;
        const BLOCKED_LIMIT: usize = 3;
        let candidates = self.ready_candidates(course_id, Some(user_id)).await?;
        let ledger = self.taught_ledger().await?;
        let ready: Vec<&crate::learning_graph::ReadyCandidate> = candidates
            .iter()
            .filter(|candidate| {
                !candidate.satisfied
                    && candidate.assumes.iter().all(|(concept_id, tier)| {
                        ledger.get(concept_id).is_some_and(|taught| taught >= tier)
                    })
            })
            .collect();
        if ready.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // 供给表：概念 → 教它的节点（跨课程，不含被评估的候选自身）。
        let assumed_concepts: HashSet<String> = ready
            .iter()
            .flat_map(|candidate| candidate.assumes.iter().map(|(concept_id, _)| concept_id.clone()))
            .collect();
        let mut teachers: HashMap<String, Vec<String>> = HashMap::new();
        if !assumed_concepts.is_empty() {
            let mut query = sqlx::QueryBuilder::new(
                "SELECT lc.concept_id, lc.lesson_id FROM learning_lesson_concepts lc \
                 WHERE lc.role = 'teaches' AND lc.concept_id IN (",
            );
            let mut separated = query.separated(", ");
            for concept in &assumed_concepts {
                separated.push_bind(concept.clone());
            }
            query.push(")");
            let rows = query.build().fetch_all(&self.pool).await.map_err(internal)?;
            for row in rows {
                let concept_id: String = row.try_get("concept_id").map_err(internal)?;
                let lesson_id: String = row.try_get("lesson_id").map_err(internal)?;
                teachers.entry(concept_id).or_default().push(lesson_id);
            }
        }

        // 供给节点集合及其标题。
        let supplier_ids: HashSet<String> = teachers
            .values()
            .flatten()
            .cloned()
            .collect();
        if supplier_ids.is_empty() {
            let ready_ids: Vec<LearningLessonId> = ready
                .iter()
                .map(|candidate| parse_id(candidate.lesson_id.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok((ready_ids, Vec::new()));
        }
        let mut titles: HashMap<String, String> = HashMap::new();
        {
            let ids: Vec<String> = supplier_ids.iter().cloned().collect();
            let mut query = sqlx::QueryBuilder::new(
                "SELECT lesson_id, title FROM learning_lessons WHERE lesson_id IN (",
            );
            let mut separated = query.separated(", ");
            for lesson_id in &ids {
                separated.push_bind(lesson_id.clone());
            }
            query.push(")");
            let rows = query.build().fetch_all(&self.pool).await.map_err(internal)?;
            for row in rows {
                let lesson_id: String = row.try_get("lesson_id").map_err(internal)?;
                let title: String = row.try_get("title").map_err(internal)?;
                titles.insert(lesson_id, title);
            }
        }

        // 供给节点代表 R：该节点名下（该用户）全部复习卡的最小预测回忆率；
        // 无卡/从未推进（stability 0）视为满血。顺带数到期题。
        let settings = self.scheduler_settings().await;
        let now = now_ms();
        let mut card_rows: HashMap<String, Vec<(f64, Option<i64>)>> = HashMap::new();
        {
            let ids: Vec<String> = supplier_ids.iter().cloned().collect();
            let mut query = sqlx::QueryBuilder::new(
                "SELECT l.lesson_id, ri.stability_days, ri.last_reviewed_at, ri.due_at, \
                        ri.archived_at \
                 FROM learning_review_items ri \
                 JOIN learning_enrollments e ON e.enrollment_id = ri.enrollment_id AND e.user_id = ? \
                 JOIN learning_activities a ON a.activity_id = ri.activity_id \
                 JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                 WHERE l.lesson_id IN (",
            );
            let mut separated = query.separated(", ");
            separated.push_bind(user_id.as_str());
            for lesson_id in &ids {
                separated.push_bind(lesson_id.clone());
            }
            query.push(")");
            let rows = query.build().fetch_all(&self.pool).await.map_err(internal)?;
            for row in rows {
                let lesson_id: String = row.try_get("lesson_id").map_err(internal)?;
                let stability: f64 = row.try_get("stability_days").map_err(internal)?;
                let last_reviewed_at: Option<i64> =
                    row.try_get("last_reviewed_at").map_err(internal)?;

                card_rows.entry(lesson_id).or_default().push((stability, last_reviewed_at));
            }
        }
        let mut supplier_r: HashMap<String, f64> = HashMap::new();
        let mut supplier_due: HashMap<String, i64> = HashMap::new();
        for (lesson_id, cards) in &card_rows {
            let mut worst = 1.0f64;
            for (stability, last_reviewed_at) in cards {
                let elapsed = last_reviewed_at
                    .map(|last| crate::scheduler::days_elapsed_between(last, now, settings.tz_offset_minutes))
                    .unwrap_or(0);
                if let Some(r) = crate::scheduler::predicted_retrievability(*stability, elapsed, &settings) {
                    worst = worst.min(r);
                }
            }
            supplier_r.insert(lesson_id.clone(), worst);
            supplier_due.insert(lesson_id.clone(), 0);
        }
        // 到期题数（未归档、已到期）。
        {
            let ids: Vec<String> = supplier_ids.iter().cloned().collect();
            let mut query = sqlx::QueryBuilder::new(
                "SELECT l.lesson_id, COUNT(*) AS due FROM learning_review_items ri \
                 JOIN learning_enrollments e ON e.enrollment_id = ri.enrollment_id AND e.user_id = ? \
                 JOIN learning_activities a ON a.activity_id = ri.activity_id \
                 JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
                 WHERE l.lesson_id IN (",
            );
            let mut separated = query.separated(", ");
            separated.push_bind(user_id.as_str());
            for lesson_id in &ids {
                separated.push_bind(lesson_id.clone());
            }
            query.push(") AND ri.due_at <= ? AND ri.archived_at IS NULL GROUP BY l.lesson_id");
            let rows = query
                .build()
                .bind(now)
                .fetch_all(&self.pool)
                .await
                .map_err(internal)?;
            for row in rows {
                let lesson_id: String = row.try_get("lesson_id").map_err(internal)?;
                let due: i64 = row.try_get("due").map_err(internal)?;
                supplier_due.insert(lesson_id, due);
            }
        }

        // 分桶：候选的最差供给节点决定去留。
        let mut blocked: Vec<crate::models::GraphBlockedView> = Vec::new();
        let mut blocked_ids: HashSet<String> = HashSet::new();
        for candidate in &ready {
            let mut worst: Option<(f64, String)> = None;
            for (concept_id, _) in &candidate.assumes {
                for supplier in teachers.get(concept_id).into_iter().flatten() {
                    let r = supplier_r.get(supplier).copied().unwrap_or(1.0);
                    match &worst {
                        Some((best_r, _)) if *best_r <= r => {}
                        _ => worst = Some((r, supplier.clone())),
                    }
                }
            }
            if let Some((r, supplier_id)) = worst {
                if r < R_GATE {
                    blocked_ids.insert(candidate.lesson_id.clone());
                    blocked.push(crate::models::GraphBlockedView {
                        lesson_id: parse_id(candidate.lesson_id.clone())?,
                        title: candidate.title.clone(),
                        supplier_lesson_id: parse_id(supplier_id.clone())?,
                        supplier_title: titles.get(&supplier_id).cloned().unwrap_or_default(),
                        r,
                        due_count: supplier_due.get(&supplier_id).copied().unwrap_or(0),
                    });
                }
            }
        }
        blocked.sort_by(|a, b| a.r.partial_cmp(&b.r).unwrap_or(std::cmp::Ordering::Equal));
        blocked.truncate(BLOCKED_LIMIT);
        let ready_ids: Vec<LearningLessonId> = ready
            .iter()
            .filter(|candidate| !blocked_ids.contains(&candidate.lesson_id))
            .map(|candidate| parse_id(candidate.lesson_id.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((ready_ids, blocked))
    }

    /// 学习记录：批次时间线（倒序），每批带节点行与学习者进度。
    pub async fn graph_history(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<crate::models::GraphHistoryView, AppError> {
        let enrollment = self.enrollment_id_for(user_id, course_id).await?;
        let enrollment_value = enrollment.as_ref().map(LearningEnrollmentId::as_str);
        let batches = sqlx::query(
            "SELECT batch_id, seq, status, node_ids_json, note, created_at \
             FROM learning_growth_batches WHERE course_id = ? \
             ORDER BY seq DESC, batch_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut views = Vec::with_capacity(batches.len());
        for batch in &batches {
            let batch_id: String = batch.try_get("batch_id").map_err(internal)?;
            let node_ids_json: String = batch.try_get("node_ids_json").map_err(internal)?;
            let node_ids: Vec<String> =
                serde_json::from_str(&node_ids_json).unwrap_or_default();
            let mut nodes = Vec::with_capacity(node_ids.len());
            for node_id in node_ids {
                let Ok(lesson_id) = parse_id::<LearningLessonId>(node_id.clone()) else {
                    continue;
                };
                let row = sqlx::query(
                    "SELECT l.title, l.estimated_minutes, COALESCE(p.status, 'not_started') AS status, \
                            p.completed_at \
                     FROM learning_lessons l \
                     LEFT JOIN learning_lesson_progress p \
                       ON p.lesson_id = l.lesson_id AND p.enrollment_id = ? \
                     WHERE l.lesson_id = ?",
                )
                .bind(enrollment_value)
                .bind(&node_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
                let Some(row) = row else { continue };
                let status_text: String = row.try_get("status").map_err(internal)?;
                nodes.push(crate::models::GraphNodeHistoryView {
                    lesson_id,
                    title: row.try_get("title").map_err(internal)?,
                    estimated_minutes: row.try_get("estimated_minutes").map_err(internal)?,
                    status: LessonStatus::try_from(status_text.as_str())
                        .map_err(AppError::Internal)?,
                    completed_at: row.try_get("completed_at").map_err(internal)?,
                });
            }
            views.push(crate::models::GraphBatchView {
                batch_id,
                seq: batch.try_get("seq").map_err(internal)?,
                status: batch.try_get("status").map_err(internal)?,
                note: batch.try_get("note").map_err(internal)?,
                created_at: batch.try_get("created_at").map_err(internal)?,
                nodes,
            });
        }
        Ok(crate::models::GraphHistoryView { batches: views })
    }

    /// 概念表：本课程涉及的概念（教/假定 × 档位 × 节点）+ 跨课程来源标注。
    pub async fn graph_concepts(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
    ) -> Result<Vec<crate::models::GraphConceptRowView>, AppError> {
        // 概念 × 本课程节点（教/假定 + 档位 + 学习者状态）。
        let ref_rows = sqlx::query(
            "SELECT reg.concept_id, reg.canonical, reg.aliases_json, reg.definition,                     lc.role, lc.tier, l.lesson_id, l.title,                     COALESCE(p.status, 'not_started') AS status              FROM learning_lesson_concepts lc              JOIN learning_lessons l ON l.lesson_id = lc.lesson_id              JOIN learning_modules m ON m.module_id = l.module_id              JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id              LEFT JOIN learning_enrollments e ON e.course_id = m.course_id AND e.user_id = ?              LEFT JOIN learning_lesson_progress p                ON p.lesson_id = l.lesson_id AND p.enrollment_id = e.enrollment_id              WHERE m.course_id = ?              ORDER BY reg.canonical, l.position, l.lesson_id",
        )
        .bind(user_id.as_str())
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        if ref_rows.is_empty() {
            return Ok(Vec::new());
        }
        // 跨课程来源：还有哪些别的课程在教这些概念。
        let mut other_courses: HashMap<String, Vec<String>> = HashMap::new();
        for row in &sqlx::query(
            "SELECT lc.concept_id, c.title FROM learning_lesson_concepts lc              JOIN learning_lessons l ON l.lesson_id = lc.lesson_id              JOIN learning_modules m ON m.module_id = l.module_id              JOIN learning_courses c ON c.course_id = m.course_id              WHERE lc.role = 'teaches' AND m.course_id <> ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?
        {
            let concept_id: String = row.try_get("concept_id").map_err(internal)?;
            let title: String = row.try_get("title").map_err(internal)?;
            let courses = other_courses.entry(concept_id).or_default();
            if !courses.contains(&title) {
                courses.push(title);
            }
        }

        // 按概念聚合（查询已按 canonical 排序，顺序稳定）。
        let mut rows: Vec<crate::models::GraphConceptRowView> = Vec::new();
        let mut index_by_concept: HashMap<String, usize> = HashMap::new();
        for row in &ref_rows {
            let concept_id: String = row.try_get("concept_id").map_err(internal)?;
            let index = match index_by_concept.get(&concept_id) {
                Some(index) => *index,
                None => {
                    let aliases_json: String = row.try_get("aliases_json").map_err(internal)?;
                    let index = rows.len();
                    rows.push(crate::models::GraphConceptRowView {
                        concept_id: concept_id.clone(),
                        canonical: row.try_get("canonical").map_err(internal)?,
                        aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
                        definition: row.try_get("definition").map_err(internal)?,
                        refs: Vec::new(),
                        other_courses: other_courses.get(&concept_id).cloned().unwrap_or_default(),
                    });
                    index_by_concept.insert(concept_id.clone(), index);
                    index
                }
            };
            let status_text: String = row.try_get("status").map_err(internal)?;
            rows[index].refs.push(crate::models::GraphConceptRefView {
                lesson_id: parse_id(row.try_get::<String, _>("lesson_id").map_err(internal)?)?,
                title: row.try_get("title").map_err(internal)?,
                role: row.try_get("role").map_err(internal)?,
                tier: row.try_get("tier").map_err(internal)?,
                status: LessonStatus::try_from(status_text.as_str()).map_err(AppError::Internal)?,
            });
        }
        Ok(rows)
    }

    // ── Status/cancel（与大纲生成流共用注册表，端点不变）──────────────────

    /// 登记一次生成：清零取消旗标并记录主题/开始时刻，返回 RAII 守卫，
    /// drop 时注销。slot 串行化保证同一 (user, key) 至多一个运行。
    pub(crate) fn begin_generation(&self, topic: &str) -> GenerationRunGuard<'_> {
        self.generation_registry
            .cancel
            .store(false, AtomicOrdering::Relaxed);
        *self
            .generation_registry
            .run
            .lock()
            .expect("generation run lock poisoned") = Some(GenerationRun {
            topic: topic.to_owned(),
            started_at: std::time::Instant::now(),
        });
        GenerationRunGuard(self)
    }

    /// 生成状态（后台指示条的数据源）：进行中时带主题与已运行秒数。
    pub fn generation_status(&self) -> crate::models::LearningGraphGenerationStatus {
        let run_guard = self
            .generation_registry
            .run
            .lock()
            .expect("generation run lock poisoned");
        match run_guard.as_ref() {
            Some(run) => crate::models::LearningGraphGenerationStatus {
                running: true,
                topic: Some(run.topic.clone()),
                elapsed_secs: Some(run.started_at.elapsed().as_secs()),
            },
            None => crate::models::LearningGraphGenerationStatus {
                running: false,
                topic: None,
                elapsed_secs: None,
            },
        }
    }

    /// 置位取消旗标；返回置位时是否存在进行中的生成。
    pub fn cancel_generation(&self) -> bool {
        let has_run = self
            .generation_registry
            .run
            .lock()
            .expect("generation run lock poisoned")
            .is_some();
        self.generation_registry
            .cancel
            .store(true, AtomicOrdering::Relaxed);
        has_run
    }

    pub fn generation_cancel_requested(&self) -> bool {
        self.generation_registry.cancel.load(AtomicOrdering::Relaxed)
    }

    pub fn generation_cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.generation_registry.cancel)
    }

    fn generation_running(&self) -> bool {
        self.generation_registry
            .run
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// 生长运行的注册：主题取课程标题前缀，便于前端指示条展示。
    async fn begin_growth_run(&self, course_id: &LearningCourseId) -> GenerationRunGuard<'_> {
        let title: Option<String> = sqlx::query_scalar(
            "SELECT title FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        self.begin_generation(&format!(
            "学习图生长 · {title}",
            title = title.unwrap_or_else(|| course_id.as_str().to_owned())
        ))
    }

    fn emit_graph_event(&self, course_id: &LearningCourseId, event: &str, mut fields: Value) {
        if let Some(object) = fields.as_object_mut() {
            object.insert("course_id".into(), serde_json::json!(course_id.as_str()));
            object.insert("kind".into(), serde_json::json!("learning_graph"));
            object.insert("event".into(), serde_json::json!(event));
        }
        self.emit_course_event(fields);
    }

    // ── Small helpers ──────────────────────────────────────────────────────

    async fn course_kind(&self, course_id: &LearningCourseId) -> Option<CourseKind> {
        let kind: Option<String> = sqlx::query_scalar(
            "SELECT course_kind FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        kind.and_then(|kind| CourseKind::try_from(kind.as_str()).ok())
    }

    async fn require_graph_course(&self, course_id: &LearningCourseId) -> Result<(), AppError> {
        if self.course_kind(course_id).await == Some(CourseKind::LearningGraph) {
            Ok(())
        } else {
            Err(AppError::NotFound(format!("learning graph course {course_id}")))
        }
    }

    fn course_completer(&self) -> Result<Arc<dyn LearningCompleter>, AppError> {
        self.course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| AppError::Conflict("learning graph growth is not configured".into()))
    }

    /// 终点（标题, 程度声明）对。
    async fn endpoint_pairs(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<Vec<(String, String)>, AppError> {
        sqlx::query_as(
            "SELECT title, goal_note FROM learning_course_endpoints \
             WHERE course_id = ? ORDER BY declared_at, endpoint_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)
    }

    /// 登记表全名集合（canonical ∪ 别名，小写、未退役）。
    async fn registry_name_set(&self) -> Result<HashSet<String>, AppError> {
        let canonicals: Vec<String> = sqlx::query_scalar(
            "SELECT lower(canonical) FROM learning_concept_registry WHERE deprecated = 0",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let alias_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT lower(je.value) FROM learning_concept_registry reg, \
             json_each(reg.aliases_json) je WHERE reg.deprecated = 0",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut names: HashSet<String> = canonicals.into_iter().collect();
        names.extend(alias_rows.into_iter().map(|(name,)| name));
        Ok(names)
    }

    /// 花名册：每节点一行的紧凑文本（状态 | 标题 | 分钟 | teaches | assumes）。
    async fn render_roster(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
        stalled: &[String],
    ) -> Result<String, AppError> {
        let candidates = self.ready_candidates(course_id, Some(user_id)).await?;
        let concepts: HashMap<String, Vec<(String, String, String)>> = sqlx::query_as(
            "SELECT lc.lesson_id, reg.canonical, lc.tier, lc.role \
             FROM learning_lesson_concepts lc \
             JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
             WHERE m.course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?
        .into_iter()
        .fold(HashMap::new(), |mut map, (lesson_id, canonical, tier, role)| {
            map.entry(lesson_id).or_insert_with(Vec::new).push((canonical, tier, role));
            map
        });
        let stalled: HashSet<&str> = stalled.iter().map(String::as_str).collect();
        let mut lines = Vec::with_capacity(candidates.len());
        for candidate in &candidates {
            let status = if candidate.satisfied {
                "已满足".to_owned()
            } else if stalled.contains(candidate.lesson_id.as_str()) {
                "⚑ 未开始（近期答错集中）".to_owned()
            } else {
                "未开始".to_owned()
            };
            let mut parts: Vec<String> = Vec::new();
            if let Some(concepts) = concepts.get(candidate.lesson_id.as_str()) {
                for (canonical, tier, role) in concepts {
                    let tier = crate::learning_graph::ConceptTier::try_from_str(tier)
                        .map(crate::learning_graph::tier_zh)
                        .unwrap_or("?");
                    parts.push(format!("{role}:{canonical}@{tier}"));
                }
            }
            lines.push(format!(
                "{status} | {title} | {concepts}",
                status = status,
                title = candidate.title,
                concepts = parts.join(" "),
            ));
        }
        Ok(lines.join("\n"))
    }

    /// 已教概念账文本（canonical@最高档）。
    async fn render_taught_summary(&self) -> Result<String, AppError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT reg.canonical, lc.tier FROM learning_lesson_concepts lc \
             JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
             WHERE lc.role = 'teaches' GROUP BY reg.canonical",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut items: Vec<(String, crate::learning_graph::ConceptTier)> = rows
            .into_iter()
            .filter_map(|(canonical, tier)| {
                crate::learning_graph::ConceptTier::try_from_str(&tier)
                    .map(|tier| (canonical, tier))
            })
            .collect();
        items.sort();
        Ok(items
            .into_iter()
            .map(|(canonical, tier)| format!("{canonical}@{}", crate::learning_graph::tier_zh(tier)))
            .collect::<Vec<_>>()
            .join("、"))
    }

    /// 终点锚清单（标题 + 程度声明）；覆盖读数单列
    /// [`Self::render_coverage_gauge`]。
    async fn render_endpoint_list(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<String, AppError> {
        let endpoints = self.endpoint_pairs(course_id).await?;
        Ok(endpoints
            .into_iter()
            .map(|(title, note)| format!("- {title}：{note}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// 登记表切片（canonical（别名）一行一个，截断到上限）。
    async fn render_registry_excerpt(&self) -> Result<String, AppError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT canonical, aliases_json FROM learning_concept_registry \
             WHERE deprecated = 0 ORDER BY canonical LIMIT ?",
        )
        .bind(REGISTRY_EXCERPT_LIMIT as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|(canonical, aliases_json)| {
                let aliases: Vec<String> = serde_json::from_str(&aliases_json).unwrap_or_default();
                if aliases.is_empty() {
                    canonical
                } else {
                    format!("{canonical}（{}）", aliases.join("、"))
                }
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// 一条终点锚 + 它的零正文标记课时行（同一事务内成对插入）。
async fn insert_endpoint(
    transaction: &mut Transaction<'_, Sqlite>,
    course_id: &LearningCourseId,
    module_id: &LearningModuleId,
    title: &str,
    goal_note: &str,
    now: i64,
) -> Result<crate::models::GraphEndpointView, AppError> {
    let endpoint_id = LearningEndpointId::new().into_string();
    let lesson_id = LearningLessonId::new();
    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(l.position) + 1, 0) FROM learning_lessons l \
         JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?",
    )
    .bind(course_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(internal)?;
    sqlx::query(
        "INSERT INTO learning_lessons \
         (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
          content_generated) \
         VALUES (?, ?, ?, '', '', ?, 1, 0)",
    )
    .bind(lesson_id.as_str())
    .bind(module_id.as_str())
    .bind(title.trim())
    .bind(position)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    sqlx::query(
        "INSERT INTO learning_course_endpoints \
         (endpoint_id, course_id, lesson_id, title, goal_note, declared_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&endpoint_id)
    .bind(course_id.as_str())
    .bind(lesson_id.as_str())
    .bind(title.trim())
    .bind(goal_note.trim())
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(internal)?;
    Ok(crate::models::GraphEndpointView {
        endpoint_id,
        lesson_id,
        title: title.trim().to_owned(),
        goal_note: goal_note.trim().to_owned(),
        completed: false,
        declared_at: now,
    })
}

/// 生长进行中的批行（生长开始时落库的 pending 行；seq 已在落库时分配）。
pub(crate) struct PendingBatch {
    pub batch_id: String,
}

/// 一批生长落库的结果快照（WS 终态帧的数据源）：idle = 空手批（不落档案，
/// 终点裁决可能仍已应用），verdicts = 本批应用的终点裁决条数。
pub(crate) struct GrowthApplied {
    pub batch_id: String,
    pub node_count: usize,
    pub note: String,
    pub ready_count: usize,
    pub idle: bool,
    pub verdicts: usize,
}
