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

/// 生长修复循环的轮数上限（g4：与结构门面同款 ≤3——首轮 + 至多 2 次带条款
/// 报告的重裁；形状违约/传输错误同样入轮次预算）。该上限只是成本/超时护栏，
/// 并非"两次即弃"的语义。
const GROWTH_MAX_ATTEMPTS: usize = 3;

impl LearningService {

    // ── Course creation（建课 = 学习目标 → 知识结构生成 → 首生长）────────

    /// 学习图课程创建入口（`GenerateCourseRequest.course_kind = learning_graph`）：
    /// 描述即学习目标，程度/基础声明可选（g2 输入最小化）。课程落库后
    /// **知识结构生成在后台启动**（双产物契约门 → structure_json 快照原子
    /// 落列，WS structure_* 事件驱动），随后**首生长**接续——接口立即返回
    /// 课程详情，进度走 WS 与 generation_registry（状态/取消端点共用）。
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
        let proficiency_statement = optional_statement(
            request.proficiency_statement.as_deref(),
            "proficiency statement",
        )?;
        let learner_basis =
            optional_statement(request.learner_basis.as_deref(), "learner basis")?;
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let _slot = self.acquire_generation_slot(user_id, "learning-graph-create".to_owned())?;
        // 提前校验生成依赖可用（未配置 provider 时建课即明确报错，而不是
        // 让它静默进入后台任务）。
        self.course_completer()?;

        let course_id = LearningCourseId::new();
        let module_id = LearningModuleId::new();
        let now = now_ms();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO learning_courses \
             (course_id, title, description, domain, version, course_kind, learning_goal, \
              created_at, updated_at) \
             VALUES (?, ?, '', 'general', 1, 'learning_graph', ?, ?, ?)",
        )
        .bind(course_id.as_str())
        .bind(&topic)
        .bind(&topic)
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
        transaction.commit().await.map_err(internal)?;

        // 结构生成 → 首生长都在后台串行（首生的门口参照来自结构产物的地形；
        // 结构失败不阻断生长——铁律：结构对生长不供内容）。后台任务要求
        // 'static：模型偏好先持有化再入任务。
        let service = self.clone();
        let owner = user_id.clone();
        let course = course_id.clone();
        let structure_input = crate::learning_graph::structure_gen::StructureInput {
            goal: topic.clone(),
            proficiency_statement,
            learner_basis,
        };
        let spawn_override = model_override
            .map(|(provider, model)| (provider.clone(), model.to_owned()));
        tokio::spawn(async move {
            let spawn_override = spawn_override
                .as_ref()
                .map(|(provider, model)| (provider, model.as_str()));
            if let Err(error) = service
                .run_structure_generation(&course, &owner, spawn_override, &structure_input)
                .await
            {
                tracing::warn!(course = course.as_str(), %error, "structure generation failed");
            }
            if let Err(error) = service.kick_growth(&owner, &course, true, spawn_override).await {
                tracing::warn!(course = course.as_str(), %error, "initial growth kick failed");
            }
        });

        self.course_detail(&course_id, Some(user_id)).await
    }

    // ── Structure generation（知识结构生成：双产物契约门 → 快照落列）────

    /// 运行知识结构生成：门面双产物过 gd.* 契约门 + 门口对账后，快照信封
    /// 原子写入 `structure_json`（重跑覆盖）。登记 generation_tasks 长任务
    /// （token 真取消——循环头检查 + 两次 LLM 调用 select! 竞速 + 快照写前
    /// 检查，写前取消 = 零快照）；WS structure_started/completed/failed/
    /// cancelled 四事件驱动前端；失败明示原因（含 provider 不支持
    /// json_object），课程保留、快照不动（重试 = 本方法重入）。
    pub async fn run_structure_generation(
        &self,
        course_id: &LearningCourseId,
        user_id: &UserId,
        model_override: Option<(&ProviderId, &str)>,
        input: &crate::learning_graph::structure_gen::StructureInput,
    ) -> Result<(), AppError> {
        self.require_live_graph_course(course_id).await?;
        // 每课程一把 slot 锁：进行中的生成让重复触发报冲突。
        let slot_key = format!("learning-structure-{}", course_id.as_str());
        let _slot = self.acquire_generation_slot(user_id, slot_key)?;
        let mut task = self.begin_generation_task(
            crate::service::KIND_STRUCTURE,
            course_id.as_str(),
            None,
            "知识结构生成",
        )?;
        let token = task.token();
        task.set_step("structure", "正在生成知识结构与前置地形…");
        self.emit_graph_event(
            course_id,
            "structure_started",
            serde_json::json!({
                "phase": "started",
                "text": "正在生成知识结构与前置地形…",
            }),
        );
        let completer = self.course_completer()?;
        let result = crate::learning_graph::structure_gen::generate_structure_artifacts(
            completer.as_ref(),
            model_override,
            input,
            Some(&token),
        )
        .await;
        match result {
            Ok(artifacts) => {
                // 快照写前检查点（三轮 t2）：写前取消 = 零快照（磁盘无半
                // 成品的 allo 等价语义）。
                if token.is_cancelled() {
                    task.finish_cancelled();
                    self.emit_graph_event(
                        course_id,
                        "structure_cancelled",
                        serde_json::json!({
                            "phase": "cancelled",
                            "outcome": "cancelled",
                        }),
                    );
                    return Err(AppError::Conflict(
                        crate::service::CANCELLED_MESSAGE.to_owned(),
                    ));
                }
                let envelope = crate::learning_graph::structure_gen::snapshot_envelope(
                    &artifacts.structure,
                    &artifacts.terrain,
                    now_ms(),
                );
                let snapshot = serde_json::to_string(&envelope).map_err(internal)?;
                sqlx::query(
                    "UPDATE learning_courses SET structure_json = ?, updated_at = ? \
                     WHERE course_id = ?",
                )
                .bind(&snapshot)
                .bind(now_ms())
                .bind(course_id.as_str())
                .execute(&self.pool)
                .await
                .map_err(internal)?;
                let blocks = artifacts
                    .structure
                    .get("blocks")
                    .and_then(|blocks| blocks.as_array())
                    .map(Vec::len)
                    .unwrap_or(0);
                let doors = artifacts
                    .terrain
                    .get("doors")
                    .and_then(|doors| doors.as_array())
                    .map(Vec::len)
                    .unwrap_or(0);
                tracing::info!(
                    course = course_id.as_str(),
                    blocks,
                    doors,
                    "knowledge structure snapshot stored"
                );
                task.finish_done(format!("双产物落列（{blocks} 块 / {doors} 门）"));
                self.emit_graph_event(
                    course_id,
                    "structure_completed",
                    serde_json::json!({
                        "phase": "completed",
                        "blocks": blocks,
                        "doors": doors,
                    }),
                );
                Ok(())
            }
            Err(error) => {
                let cancelled = token.is_cancelled()
                    || error.to_string().contains(crate::service::CANCELLED_MESSAGE);
                if cancelled {
                    task.finish_cancelled();
                    self.emit_graph_event(
                        course_id,
                        "structure_cancelled",
                        serde_json::json!({
                            "phase": "cancelled",
                            "outcome": "cancelled",
                        }),
                    );
                } else {
                    task.finish_failed(error.to_string());
                    self.emit_graph_event(
                        course_id,
                        "structure_failed",
                        serde_json::json!({
                            "phase": "failed",
                            "error": error.to_string(),
                        }),
                    );
                }
                Err(error)
            }
        }
    }

    /// 重跑知识结构生成（g2：重生成 = 换参考地图不换课，快照原子覆盖）。
    /// 输入从存量恢复：目标 = learning_goal（回退标题）；程度/基础声明从现有
    /// 快照回读（无快照则缺省映射）。后台执行，进度走 WS 事件。
    pub async fn regenerate_structure(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<bool, AppError> {
        self.require_live_graph_course(course_id).await?;
        let course = sqlx::query(
            "SELECT title, learning_goal, structure_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let snapshot: Option<String> = course.try_get("structure_json").map_err(internal)?;
        let envelope: Option<serde_json::Value> = snapshot
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(internal)?;
        let structure = envelope
            .as_ref()
            .and_then(|envelope| envelope.get("structure"));
        let terrain = envelope.as_ref().and_then(|envelope| envelope.get("terrain"));
        let proficiency = structure
            .and_then(|structure| structure.get("proficiency_statement"))
            .and_then(|value| value.as_str())
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned);
        // 地形的 learner_basis_assumed 是回执（缺省时为「纯零基础（未声明）」）——
        // 回读它等于原样重申同一假设，重生成语义不变。
        let basis = terrain
            .and_then(|terrain| terrain.get("learner_basis_assumed"))
            .and_then(|value| value.as_str())
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned);
        let input = crate::learning_graph::structure_gen::StructureInput {
            goal: if goal.trim().is_empty() { title } else { goal },
            proficiency_statement: proficiency,
            learner_basis: basis,
        };
        let service = self.clone();
        let owner = user_id.clone();
        let course = course_id.clone();
        let spawn_override =
            model_override.map(|(provider, model)| (provider.clone(), model.to_owned()));
        tokio::spawn(async move {
            let spawn_override = spawn_override
                .as_ref()
                .map(|(provider, model)| (provider, model.as_str()));
            if let Err(error) = service
                .run_structure_generation(&course, &owner, spawn_override, &input)
                .await
            {
                tracing::warn!(course = course.as_str(), %error, "structure regeneration failed");
            }
        });
        Ok(true)
    }

    /// 结构挂载回合（j08，三轮 g1「结构转活文档·上」）：把待挂集节点逐条
    /// 挂到结构块（导航事实）。显式命令触发、单课串行（slot 锁）、可取消；
    /// 待挂集空 = skipped（零 LLM 零写）；落盘 = 挂载行单行原子覆盖
    /// （merge upsert：本轮替换、未评估保留、孤儿清出）。
    pub async fn run_structure_mount(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<crate::models::MountRoundSummary, AppError> {
        use crate::learning_graph::mounting;
        self.require_live_graph_course(course_id).await?;
        let slot_key = format!("learning-mount-{}", course_id.as_str());
        let _slot = self.acquire_generation_slot(user_id, slot_key)?;
        let mut task = self.begin_generation_task(
            crate::service::KIND_STRUCTURE_MOUNT,
            course_id.as_str(),
            None,
            "结构挂载",
        )?;
        let token = task.token();
        task.set_step("mount", "正在挂载节点到结构块…");
        self.emit_graph_event(
            course_id,
            "structure_mount_started",
            serde_json::json!({ "phase": "started" }),
        );

        // 结构快照与宪章（宪章三项 + 结构参考行的数据源）。无结构 = 冲突
        // （先跑结构生成再挂载）。
        let course = sqlx::query(
            "SELECT title, learning_goal, structure_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let structure_raw: String = sqlx::query_scalar(
            "SELECT structure_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .filter(|raw: &String| !raw.trim().is_empty())
        .ok_or_else(|| {
            AppError::Conflict("课程还没有知识结构快照——先生成结构再挂载".into())
        })?;
        let envelope: serde_json::Value = serde_json::from_str(&structure_raw).map_err(internal)?;
        let structure = envelope
            .get("structure")
            .ok_or_else(|| AppError::Internal("结构快照信封缺 structure".into()))?;
        let proficiency = structure
            .get("proficiency_statement")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let basis = envelope
            .get("terrain")
            .and_then(|terrain| terrain.get("learner_basis_assumed"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let hash = mounting::fnv1a_64(structure_raw.as_bytes());

        // 块参考行与 slug 集合。
        let mut block_rows: Vec<(String, String)> = Vec::new();
        if let Some(blocks) = structure.get("blocks").and_then(|value| value.as_array()) {
            for block in blocks {
                let slug = block.get("slug").and_then(|value| value.as_str()).unwrap_or("");
                let name = block.get("name").and_then(|value| value.as_str()).unwrap_or("");
                if slug.trim().is_empty() {
                    continue;
                }
                let spine = block.get("spine").and_then(|value| value.as_str()).unwrap_or("");
                let stage = block.get("stage").and_then(|value| value.as_str()).unwrap_or("");
                let note = block.get("note").and_then(|value| value.as_str()).unwrap_or("");
                block_rows.push((slug.to_owned(), name.to_owned()));
                let _ = (spine, stage, note);
            }
        }
        let block_slugs: std::collections::HashSet<String> =
            block_rows.iter().map(|(slug, _)| slug.clone()).collect();

        // 现存节点（现存且启用课程内的课时，图守卫已过；标题逐字）。
        let node_rows: Vec<String> = sqlx::query_scalar(
            "SELECT l.title FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? ORDER BY l.position, l.lesson_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let node_titles = node_rows;
        let node_title_set: std::collections::HashSet<String> =
            node_titles.iter().cloned().collect();

        // 既有挂载行 + stale 判定（hash 失配 = 结构变过，全量重评）。
        let existing_row: Option<(String, i64)> = sqlx::query_as(
            "SELECT mounts_json, structure_hash FROM learning_structure_mounts WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let (existing, stale) = match &existing_row {
            Some((mounts_json, stored_hash)) => {
                let stored = (*stored_hash) as u64;
                let value: serde_json::Value =
                    serde_json::from_str(mounts_json).map_err(internal)?;
                (mounting::parse_mount_entries(&value), stored != hash)
            }
            None => (Vec::new(), true),
        };

        // 待挂集；空 = skipped（零 LLM 零写）。
        let pending = if stale {
            node_titles.clone()
        } else {
            mounting::pending_nodes(&node_titles, &existing, &block_slugs)
        };
        if pending.is_empty() {
            task.finish_done("挂载跳过：无待挂节点");
            self.emit_graph_event(
                course_id,
                "structure_mount_completed",
                serde_json::json!({ "phase": "completed", "outcome": "skipped" }),
            );
            return Ok(crate::models::MountRoundSummary {
                status: "skipped".into(),
                pending_nodes: 0,
                mounted_nodes: 0,
                outside_nodes: existing
                    .iter()
                    .filter(|entry| entry.blocks.is_empty())
                    .count(),
                total_entries: existing.len(),
                stale,
                structure_hash: hash as i64,
            });
        }

        // 回合输入（七件）。
        let structure_blocks = block_rows
            .iter()
            .map(|(slug, name)| format!("- {slug} | {name}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pending_lines = pending
            .iter()
            .map(|title| format!("- {title}"))
            .collect::<Vec<_>>()
            .join("\n");
        let existing_text = if existing.is_empty() {
            "（无——首次挂载）".to_owned()
        } else {
            let items: Vec<serde_json::Value> =
                existing.iter().map(mounting::MountEntry::to_json).collect();
            serde_json::json!({ "mounts": items }).to_string()
        };
        let round_input = mounting::MountRoundInput {
            goal: if goal.trim().is_empty() { &title } else { goal.as_str() },
            proficiency_statement: &proficiency,
            learner_basis: &basis,
            structure_blocks: &structure_blocks,
            pending_nodes: &pending_lines,
            existing_mounts: &existing_text,
        };

        let completer = self.course_completer()?;
        let evaluated = mounting::run_mount_round(
            completer.as_ref(),
            model_override,
            &round_input,
            &block_slugs,
            &node_title_set,
            Some(&token),
        )
        .await;
        let evaluated = match evaluated {
            Ok(entries) => entries,
            Err(error) => {
                let cancelled = token.is_cancelled()
                    || error.to_string().contains(crate::service::CANCELLED_MESSAGE);
                if cancelled {
                    task.finish_cancelled();
                    self.emit_graph_event(
                        course_id,
                        "structure_mount_cancelled",
                        serde_json::json!({ "phase": "cancelled" }),
                    );
                } else {
                    task.finish_failed(error.to_string());
                    self.emit_graph_event(
                        course_id,
                        "structure_mount_failed",
                        serde_json::json!({ "phase": "failed", "error": error.to_string() }),
                    );
                }
                return Err(error);
            }
        };
        // 写前检查点（与结构生成同款）。
        if token.is_cancelled() {
            task.finish_cancelled();
            self.emit_graph_event(
                course_id,
                "structure_mount_cancelled",
                serde_json::json!({ "phase": "cancelled" }),
            );
            return Err(AppError::Conflict(
                crate::service::CANCELLED_MESSAGE.to_owned(),
            ));
        }

        // 单行原子覆盖（merge upsert）。
        let merged = mounting::merge_mounts(
            &existing,
            &evaluated,
            &pending,
            &node_title_set,
            &block_slugs,
        );
        let payload = mounting::entries_json(&merged);
        sqlx::query(
            "INSERT INTO learning_structure_mounts (course_id, mounts_json, structure_hash, updated_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(course_id) DO UPDATE SET \
               mounts_json = excluded.mounts_json, \
               structure_hash = excluded.structure_hash, \
               updated_at = excluded.updated_at",
        )
        .bind(course_id.as_str())
        .bind(&payload)
        .bind(hash as i64)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        let mounted = evaluated.iter().filter(|entry| !entry.blocks.is_empty()).count();
        let outside = evaluated.iter().filter(|entry| entry.blocks.is_empty()).count();
        task.finish_done(format!(
            "挂载完成：{}/{} 挂入块，{} 结构外",
            mounted,
            pending.len(),
            outside
        ));
        self.emit_graph_event(
            course_id,
            "structure_mount_completed",
            serde_json::json!({
                "phase": "completed",
                "outcome": "mounted",
                "mounted": mounted,
                "pending": pending.len(),
            }),
        );
        Ok(crate::models::MountRoundSummary {
            status: "mounted".into(),
            pending_nodes: pending.len(),
            mounted_nodes: mounted,
            outside_nodes: outside,
            total_entries: merged.len(),
            stale: false,
            structure_hash: hash as i64,
        })
    }

    /// 结构修补回合（j09，结构转活文档·下）：触发门槛 = findings 非空 ∪
    /// 挂载观察非空（皆空 = skipped 零 LLM 零写）；ops 空 = noop（LLM 已跑
    /// 不写盘）；ops 应用后 gd.* 全量重过零放宽 → 结构快照原子写 → 修补审
    /// 计行落表。**级联口径（ADR-0017）**：本回合不动挂载——hash 变即既有
    /// 挂载待重评估（mounts_stale），下回合全量重挂，不建逐 op 跟迁。
    pub async fn run_structure_repair(
        &self,
        user_id: &UserId,
        course_id: &LearningCourseId,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<crate::models::StructureRepairSummary, AppError> {
        use crate::learning_graph::structure_ops;
        self.require_live_graph_course(course_id).await?;
        let slot_key = format!("learning-repair-{}", course_id.as_str());
        let _slot = self.acquire_generation_slot(user_id, slot_key)?;
        let mut task = self.begin_generation_task(
            crate::service::KIND_STRUCTURE_REPAIR,
            course_id.as_str(),
            None,
            "结构修补",
        )?;
        let token = task.token();
        task.set_step("repair", "正在按留痕与观察修补结构…");
        self.emit_graph_event(
            course_id,
            "structure_repair_started",
            serde_json::json!({ "phase": "started" }),
        );

        // 结构快照 + 宪章。
        let course = sqlx::query(
            "SELECT title, learning_goal, structure_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let structure_raw: String = sqlx::query_scalar(
            "SELECT structure_json FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .filter(|raw: &String| !raw.trim().is_empty())
        .ok_or_else(|| {
            AppError::Conflict("课程还没有知识结构快照——先生成结构再修补".into())
        })?;
        let envelope: serde_json::Value = serde_json::from_str(&structure_raw).map_err(internal)?;
        let structure = envelope
            .get("structure")
            .cloned()
            .ok_or_else(|| AppError::Internal("结构快照信封缺 structure".into()))?;
        let terrain = envelope.get("terrain").cloned().unwrap_or(serde_json::Value::Null);
        let proficiency = structure
            .get("proficiency_statement")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let basis = envelope
            .get("terrain")
            .and_then(|terrain| terrain.get("learner_basis_assumed"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let old_hash = crate::learning_graph::mounting::fnv1a_64(structure_raw.as_bytes());

        // 触发面：findings 行 + 挂载观察。
        let finding_rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT source, node, field, issue, advice FROM learning_structure_findings \
             WHERE course_id = ? ORDER BY ts, id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let findings_text = if finding_rows.is_empty() {
            "（无留痕）".to_owned()
        } else {
            finding_rows
                .iter()
                .map(|(source, node, field, issue, advice)| {
                    format!(
                        "- [{source}] {node}.{field}: {issue}（建议：{advice}）",
                        node = node.trim(),
                        field = field.trim()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let blocks_for_obs: Vec<(String, String)> = structure
            .get("blocks")
            .and_then(|value| value.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| {
                        let slug = block.get("slug")?.as_str()?.to_owned();
                        let name = block.get("name")?.as_str()?.to_owned();
                        Some((slug, name))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mounts_json: Option<String> = sqlx::query_scalar(
            "SELECT mounts_json FROM learning_structure_mounts WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let has_mount_row = mounts_json.is_some();
        let mount_entries: Vec<crate::learning_graph::mounting::MountEntry> = mounts_json
            .map(|raw| {
                crate::learning_graph::mounting::parse_mount_entries(
                    &serde_json::from_str::<serde_json::Value>(&raw).unwrap_or(serde_json::Value::Null),
                )
            })
            .unwrap_or_default();
        // 挂载观察只在挂载回合已跑过时派生（无挂载行的课程没有挂载事实，
        // 「空块」无从谈起——修补触发门槛不被未挂载课程常驻点燃）。
        let observations = if has_mount_row {
            crate::learning_graph::mounting::mount_observations(&blocks_for_obs, &mount_entries)
        } else {
            Vec::new()
        };
        let observations_text = if observations.is_empty() {
            "（无观察）".to_owned()
        } else {
            observations
                .iter()
                .map(|(kind, detail)| format!("- [{kind}] {detail}"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // 触发门槛：皆空 = skipped（零 LLM 零写）。
        if finding_rows.is_empty() && observations.is_empty() {
            task.finish_done("修补跳过：无留痕与观察");
            self.emit_graph_event(
                course_id,
                "structure_repair_completed",
                serde_json::json!({ "phase": "completed", "outcome": "skipped" }),
            );
            return Ok(crate::models::StructureRepairSummary {
                status: "skipped".into(),
                old_hash: old_hash as i64,
                new_hash: old_hash as i64,
                applied: Vec::new(),
                cascade: Vec::new(),
                mounts_stale: false,
                noop: false,
                skipped: true,
            });
        }

        // 回合输入（七件）。
        let structure_snapshot = {
            let mut lines: Vec<String> = blocks_for_obs
                .iter()
                .map(|(slug, name)| format!("- {name}（{slug}）"))
                .collect();
            if let Some(edges) = structure.get("edges").and_then(|value| value.as_array()) {
                for edge in edges {
                    let source = edge.get("source").and_then(|v| v.as_str()).unwrap_or("");
                    let target = edge.get("target").and_then(|v| v.as_str()).unwrap_or("");
                    let edge_type = edge.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let tier = edge.get("tier").and_then(|v| v.as_str()).unwrap_or("");
                    lines.push(format!("  边：{source} -[{edge_type}/{tier}]-> {target}"));
                }
            }
            lines.join("\n")
        };
        let round_input = structure_ops::RepairRoundInput {
            goal: if goal.trim().is_empty() { &title } else { goal.as_str() },
            proficiency_statement: &proficiency,
            learner_basis: &basis,
            structure_snapshot: &structure_snapshot,
            findings: &findings_text,
            mount_observations: &observations_text,
        };

        let completer = self.course_completer()?;
        let ops = structure_ops::run_repair_round(
            completer.as_ref(),
            model_override,
            &round_input,
            &structure,
            Some(&token),
        )
        .await;
        let ops = match ops {
            Ok(ops) => ops,
            Err(error) => {
                let cancelled = token.is_cancelled()
                    || error.to_string().contains(crate::service::CANCELLED_MESSAGE);
                if cancelled {
                    task.finish_cancelled();
                    self.emit_graph_event(
                        course_id,
                        "structure_repair_cancelled",
                        serde_json::json!({ "phase": "cancelled" }),
                    );
                } else {
                    task.finish_failed(error.to_string());
                    self.emit_graph_event(
                        course_id,
                        "structure_repair_failed",
                        serde_json::json!({ "phase": "failed", "error": error.to_string() }),
                    );
                }
                return Err(error);
            }
        };
        if token.is_cancelled() {
            task.finish_cancelled();
            self.emit_graph_event(
                course_id,
                "structure_repair_cancelled",
                serde_json::json!({ "phase": "cancelled" }),
            );
            return Err(AppError::Conflict(
                crate::service::CANCELLED_MESSAGE.to_owned(),
            ));
        }

        let noop = ops.is_empty();
        let (new_structure, cascade) = if noop {
            (structure.clone(), Vec::new())
        } else {
            let mut candidate = structure.clone();
            let cascade = match structure_ops::apply_ops(&mut candidate, &ops) {
                Ok(cascade) => cascade,
                Err(error) => {
                    let message = format!("修补意图无法应用（结构与审计均未改动）：{error}");
                    task.finish_failed(message.clone());
                    return Err(AppError::UnprocessableEntity(message));
                }
            };
            // st.ops.apply：gd.* 全量重过零放宽——任一违规拒整回合。
            let mut violations = crate::learning_graph::structure_contract::validate_structure(&candidate);
            violations.extend(crate::learning_graph::structure_contract::reconcile_doors(
                &candidate,
                &terrain,
            ));
            let red: Vec<_> = violations.iter().filter(|item| !item.warning).collect();
            if !red.is_empty() {
                let owned: Vec<crate::learning_graph::structure_contract::ContractViolation> =
                    red.into_iter().cloned().collect();
                let report = crate::learning_graph::structure_contract::format_violations(&owned);
                task.finish_failed(report.clone());
                return Err(AppError::UnprocessableEntity(format!(
                    "修补后的结构未通过全量复核（结构与审计均未改动）：\n{report}"
                )));
            }
            (candidate, cascade)
        };
        let new_structure_raw = serde_json::to_string(&serde_json::json!({
            "version": envelope.get("version").cloned().unwrap_or(serde_json::json!(1)),
            "generated_at": envelope.get("generated_at").cloned().unwrap_or(serde_json::json!(now_ms())),
            "structure": new_structure,
            "terrain": terrain,
        }))
        .map_err(internal)?;
        let new_hash = crate::learning_graph::mounting::fnv1a_64(new_structure_raw.as_bytes());
        if !noop {
            // 结构快照原子写（唯一权威）+ 修补审计行。
            sqlx::query("UPDATE learning_courses SET structure_json = ?, updated_at = ? WHERE course_id = ?")
                .bind(&new_structure_raw)
                .bind(now_ms())
                .bind(course_id.as_str())
                .execute(&self.pool)
                .await
                .map_err(internal)?;
            let ops_log: Vec<serde_json::Value> = ops
                .iter()
                .map(|op| {
                    serde_json::json!({
                        "op": op.label(),
                        "reason": op.reason(),
                    })
                })
                .collect();
            sqlx::query(
                "INSERT INTO learning_structure_repairs \
                 (ts, course_id, old_hash, new_hash, result, ops_json, note) \
                 VALUES (?, ?, ?, ?, 'applied', ?, ?)",
            )
            .bind(now_ms())
            .bind(course_id.as_str())
            .bind(old_hash as i64)
            .bind(new_hash as i64)
            .bind(serde_json::to_string(&ops_log).map_err(internal)?.as_str())
            .bind(cascade.join("；"))
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        }
        // noop 不落审计行（learnhub 同款「无修补动作不留档案」——评估过、
        // 无修补动作是合法结论，但没发生的事不造档案）。
        let applied: Vec<String> = ops.iter().map(|op| op.label()).collect();
        let mounts_stale = old_hash != new_hash;
        task.finish_done(if noop {
            "修补完成：本轮无修补动作（noop）".to_owned()
        } else {
            format!("修补完成：{} 条 op 应用", applied.len())
        });
        self.emit_graph_event(
            course_id,
            "structure_repair_completed",
            serde_json::json!({
                "phase": "completed",
                "outcome": if noop { "noop" } else { "applied" },
                "applied": applied.len(),
            }),
        );
        Ok(crate::models::StructureRepairSummary {
            status: if noop { "noop".into() } else { "applied".into() },
            old_hash: old_hash as i64,
            new_hash: new_hash as i64,
            applied,
            cascade,
            mounts_stale,
            noop,
            skipped: false,
        })
    }

    /// findings warn 通道落表（j09）：逐条追加进
    /// `learning_structure_findings`（source = concept-review）；坏行（issue
    /// 空）跳过 warn 不 Broken——留痕是线索不是第二真相。
    async fn record_review_findings(
        &self,
        course_id: &LearningCourseId,
        findings: &[crate::learning_graph::ReviewFinding],
    ) {
        let now = now_ms();
        for finding in findings {
            if finding.issue.trim().is_empty() {
                tracing::warn!(
                    course = course_id.as_str(),
                    "review finding with empty issue skipped (线索行坏档不落)"
                );
                continue;
            }
            let result = sqlx::query(
                "INSERT INTO learning_structure_findings \
                 (ts, course_id, source, node, field, issue, advice) \
                 VALUES (?, ?, 'concept-review', ?, ?, ?, ?)",
            )
            .bind(now)
            .bind(course_id.as_str())
            .bind(finding.node.trim())
            .bind(finding.field.trim())
            .bind(finding.issue.trim())
            .bind(finding.advice.trim())
            .execute(&self.pool)
            .await;
            if let Err(error) = result {
                tracing::warn!(
                    course = course_id.as_str(),
                    error = %error,
                    "review finding insert failed — 留痕不阻断生长"
                );
            }
        }
    }

    /// 学习图课程守卫（对齐软删语义 + 现存且启用口径）：存在、未软删、未
    /// 归档、且是学习图课程。已归档 = 人面错误（可恢复），不是 404。
    async fn require_live_graph_course(&self, course_id: &LearningCourseId) -> Result<(), AppError> {
        let row: Option<(String, Option<i64>, i64)> = sqlx::query_as(
            "SELECT course_kind, deleted_at, enabled FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let Some((kind, deleted_at, enabled)) = row else {
            return Err(AppError::NotFound(format!("course {course_id}")));
        };
        if deleted_at.is_some() {
            return Err(AppError::NotFound(format!("course {course_id}")));
        }
        if enabled == 0 {
            return Err(AppError::Conflict(
                "课程已归档，恢复后可继续学习".into(),
            ));
        }
        if kind != CourseKind::LearningGraph.as_str() {
            return Err(AppError::NotFound(format!("learning graph course {course_id}")));
        }
        Ok(())
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
        // 安排面守卫（三轮 g3）：生长只服务现存且启用的学习图课程——归档课
        // 的手动/自动触发都给人面错误（可恢复），不再烧教练预算。
        self.require_live_graph_course(course_id).await?;
        let ready_count = self.ready_count(course_id, user_id).await?;
        if !force && ready_count >= crate::learning_graph::READY_TRIGGER {
            return Ok(false);
        }
        // 停摆臂只约束自动触发；手动 force 永远豁免（ADR-0010：只留空图臂）。
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

    /// 停摆臂（ADR-0010 裁决：只留空图一臂）：课程还没有任何学习节点时
    /// 自动触发退出（建课瞬间的首生长 force 走，不占自动触发）。生长无
    /// 自动终态——「课程长完了」不再有机器语义，收尾交给学习者归档。
    async fn growth_stalled(&self, course_id: &LearningCourseId) -> Result<bool, AppError> {
        let learning_nodes: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        Ok(learning_nodes == 0)
    }

    /// 一轮生长：上下文组装 → 教练起草/重裁 → 结构门 →（打回重裁，≤3 轮）→
    /// AI 概念评审 →（打回同上）→ 事务落库（铸名/节点/概念网/批次）。
    /// 全程登记 generation_tasks 长任务（token 真取消 + 快照可见）；结束发
    /// WS 终态帧（取消 = 中性 cancelled，不报错误）。
    async fn run_growth(
        &self,
        engine: &crate::learning_graph::GrowthRunner,
    ) -> Result<(), AppError> {
        let course_id = &engine.course_id;
        let mut task = self.begin_growth_task(course_id).await?;
        let token = task.token();
        task.set_step("draft", "教练正在阅读结构参考与花名册，起草本批节点…");
        let model_label = match engine.model_override() {
            Some((provider, model)) => format!("{}/{}", provider.as_str(), model),
            None => "default".to_owned(),
        };
        tracing::info!(
            course = course_id.as_str(),
            model = %model_label,
            "learning graph growth run started"
        );
        // phase 字段对齐前端过程视图的形状约定（started/round/failed）。
        self.emit_graph_event(
            course_id,
            "growth_started",
            serde_json::json!({
                "phase": "started",
                "text": "教练正在阅读结构参考与花名册，起草本批节点…",
            }),
        );
        // 批行状态机：生长开始即落 pending 行（slot 串行保证此课程此刻
        // 不可能有别的生长在跑，残留的 pending 一律是上次中断 → 转 failed）。
        let pending = self.begin_pending_batch(course_id).await?;
        let result = self.growth_body(engine, &pending, token.clone()).await;
        match &result {
            Ok(applied) => {
                task.finish_done(if applied.idle { "就绪已满，本批无可长方向" } else { "批次已落库" });
                // 空手批的 pending 行已在 growth_body 内自清；WS 以
                // outcome=idle 收尾——前端刷新就绪读数即可，无历史噪音。
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
                // 取消 = 中性终态（growth_cancelled，phase=cancelled），不
                // 报错误——中断的本批零落库（事务原子），批行如实转 failed。
                let cancelled = token.is_cancelled()
                    || error.to_string().contains(crate::service::CANCELLED_MESSAGE);
                self.fail_pending_batch(&pending.batch_id).await;
                if cancelled {
                    task.finish_cancelled();
                    self.emit_graph_event(
                        course_id,
                        "growth_cancelled",
                        serde_json::json!({
                            "phase": "cancelled",
                            "outcome": "cancelled",
                        }),
                    );
                } else {
                    task.finish_failed(error.to_string());
                    self.emit_graph_event(
                        course_id,
                        "growth_failed",
                        serde_json::json!({
                            "phase": "failed",
                            "error": error.to_string(),
                        }),
                    );
                }
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
        token: tokio_util::sync::CancellationToken,
    ) -> Result<GrowthApplied, AppError> {
        use crate::learning_graph::growth_retry::{
            AttemptFail, is_batch_size_violation, normalize_report, redraft_capacity_report,
            redraft_guard_violation, should_stop_early,
        };
        let course_id = &engine.course_id;
        let inner_completer = self.course_completer()?;
        // 真取消（三轮 t2）：全部 completer 调用与 token select! 竞速——调用
        // 前已取消 = 零调用；调用中被取消 = 断 HTTP。批事务原子落库 =
        // 中断零落库。
        let cancellation = crate::service::CancellationCompleter::new(
            inner_completer.as_ref(),
            token.clone(),
        );
        let completer: &dyn LearningCompleter = &cancellation;
        let model_override = engine
            .model_override
            .as_ref()
            .map(|(provider, model)| (provider, model.as_str()));
        let context = self.build_coach_context(course_id, engine, inner_completer.as_ref()).await?;
        tracing::info!(
            course = course_id.as_str(),
            prompt_chars = context.render().len(),
            "coach context assembled"
        );

        // 修复循环（g4：与结构门面同款——≤3 轮，每轮把**逐条款报告**
        // （ng.*）注入重试装配）：教练起草/重裁 → 形状与语义门/概念评审打回
        // 时把报告喂回重裁再试，直到某一稿同时过门与评审即落库；仅当预算
        // 耗尽才失败。首轮解析失败（无上一稿可点修）直接带报告重新起草。
        // 重试机制（二轮 t1）：同指纹早停（连败两次即停）+「未点名被改」
        // 机械守卫（豁免早停、原地重试恰一次不占预算）+ 重裁出口容量定向
        // 打回；每轮起草/重裁/评审前后有取消检查点（loop 头 + select! 竞速）。
        let cancel_err = || {
            AppError::Conflict(crate::service::CANCELLED_MESSAGE.to_owned())
        };
        let mut previous: Option<crate::learning_graph::ProposedBatch> = None;
        let mut feedback = String::new();
        let mut last = String::from("(未执行)");
        let mut prev_fingerprint: Option<String> = None;
        let mut batch: Option<crate::learning_graph::ProposedBatch> = None;
        let mut batch_rulings: Option<HashMap<String, String>> = None;
        let mut attempts_used = 0usize;
        let mut stop_reason = "exhausted";
        'budget: for attempt in 1..=GROWTH_MAX_ATTEMPTS {
            attempts_used = attempt;
            if token.is_cancelled() {
                stop_reason = "cancelled";
                break 'budget;
            }
            let mut in_place_retried = false;
            // ── 起草/重裁（guard 的原地重试在本 attempt 内恰一次）────────
            let mut candidate = 'drafting: loop {
                if token.is_cancelled() {
                    // stop_reason 已由外层/本轮置位；无赋值必要。
                    break 'budget;
                }
                let drafted = match previous.as_ref() {
                    Some(previous) => {
                        crate::learning_graph::coach_redraft(
                            completer,
                            model_override,
                            &context,
                            previous,
                            &feedback,
                        )
                        .await
                        .map_err(|error| AttemptFail::generation(error.to_string()))
                    }
                    None => crate::learning_graph::coach_draft(
                        completer,
                        model_override,
                        &context,
                        &feedback,
                    )
                    .await
                    .map_err(|error| AttemptFail::generation(error.to_string())),
                };
                let outcome: Result<crate::learning_graph::ProposedBatch, AttemptFail> =
                    match drafted {
                        Ok(candidate) => {
                            // 「未点名被改」机械守卫（仅重裁路径有上一稿可比）。
                            if let Some(prev_ref) = previous.as_ref() {
                                if let Some(violation) =
                                    redraft_guard_violation(prev_ref, &candidate, &feedback)
                                {
                                    if !in_place_retried {
                                        // 原地重试恰一次（不占轮预算）：带守
                                        // 卫原因对最新一稿点修，评审上下文保留。
                                        in_place_retried = true;
                                        tracing::warn!(
                                            course = course_id.as_str(),
                                            attempt,
                                            "coach redraft edited unnamed nodes — in-place retry with the guard reason"
                                        );
                                        feedback = violation;
                                        previous = Some(candidate);
                                        continue 'drafting;
                                    }
                                    // 二次越权 = 本 attempt 失败，计轮推进
                                    //（guard 豁免早停——固定文案 ≠ 同输出）。
                                    tracing::warn!(
                                        course = course_id.as_str(),
                                        attempt,
                                        "coach redraft violated the point-edit guard twice — attempt consumed"
                                    );
                                    Err(AttemptFail::guard(violation))
                                } else {
                                    Ok(candidate)
                                }
                            } else {
                                Ok(candidate)
                            }
                        }
                        Err(mut fail) => {
                            // 容量守卫（重裁出口）：ng.batch.size 整批违规改判
                            // 定向重装指令（专用条款码），起草轮执法单源不变。
                            if previous.is_some() && is_batch_size_violation(&fail.report) {
                                fail.report = redraft_capacity_report(
                                    previous.as_ref().expect("redraft has previous"),
                                );
                            }
                            Err(fail)
                        }
                    };
                let candidate = match outcome {
                    Ok(candidate) => candidate,
                    Err(fail) => {
                        // 本 attempt 失败收口：早停判据 + 报告回灌。
                        let guard = fail.guard;
                        let fingerprint = normalize_report(&fail.report);
                        if should_stop_early(
                            attempt,
                            &fingerprint,
                            prev_fingerprint.as_deref(),
                            guard,
                        ) {
                            tracing::warn!(
                                course = course_id.as_str(),
                                attempt,
                                "growth failed with the same fingerprint twice — stopping early (retry budget not burned)"
                            );
                            stop_reason = "early_stop";
                            last = fail.report;
                            break 'budget;
                        }
                        prev_fingerprint = if guard { None } else { Some(fingerprint) };
                        last = fail.report.clone();
                        feedback = fail.report;
                        tracing::warn!(
                            course = course_id.as_str(),
                            attempt,
                            report = %feedback,
                            "coach draft failed — regenerating with feedback"
                        );
                        continue 'budget;
                    }
                };
                break 'drafting candidate;
            };
            tracing::info!(
                course = course_id.as_str(),
                attempt,
                nodes = candidate.nodes.len(),
                mints = candidate.mints.len(),
                batch_json = %serde_json::to_string(&candidate).unwrap_or_default(),
                "coach proposed growth batch"
            );
            let mut outcome = self.gate_batch(course_id, &mut candidate).await?;
            if !outcome.errors.is_empty() {
                let report = crate::learning_graph::format_gate_report(&outcome.errors);
                tracing::warn!(
                    course = course_id.as_str(),
                    attempt,
                    gate_count = outcome.errors.len(),
                    report = %report,
                    "growth batch rejected by structural gates"
                );
                self.emit_graph_event(course_id, "growth_redraft", serde_json::json!({
                    "stage": "gates",
                    "problems": report,
                }));
                // 指纹 = 门报告结构化序列化（字段序稳定即确定性）。
                let fingerprint = serde_json::to_string(&outcome.errors)
                    .unwrap_or_else(|_| normalize_report(&report));
                if should_stop_early(attempt, &fingerprint, prev_fingerprint.as_deref(), false) {
                    tracing::warn!(
                        course = course_id.as_str(),
                        attempt,
                        "growth rejected by the same gates twice — stopping early"
                    );
                    stop_reason = "early_stop";
                    last = report;
                    break 'budget;
                }
                prev_fingerprint = Some(fingerprint);
                last = report.clone();
                feedback = report;
                previous = Some(candidate);
                continue;
            }
            if token.is_cancelled() {
                stop_reason = "cancelled";
                return Err(cancel_err());
            }
            let candidates_text = Self::render_nomination_candidates(&outcome.nominations);
            let mut review = crate::learning_graph::concept_review(
                completer,
                model_override,
                &candidate,
                &context,
                &candidates_text,
            )
            .await;
            // findings warn 通道（j09）：数据缺陷观察落 findings 表（追加、
            // 坏行跳过）——第一轮生长即开始留痕，修补回合的输入源。留痕是
            // 线索不是第二真相：reject 照旧打回，findings 不参与判定。
            if !review.findings.is_empty() {
                self.record_review_findings(course_id, &review.findings).await;
            }
            // ── 复用裁决先行（j04 + 2026-10-19 真机发现）：模型在 reject 里
            // 也会附 reuse（近义铸名的正确出路）。裁决先于 problems 处置：能
            // 被裁决覆盖的问题（mint 重叠类，issue 里带「提名名」）从报告中
            // 剔除——落库侧 j04 机械通道会把该 mint 转为宿主活跃别名，无需折
            // 返模型。此处不改写 candidate：红裁路径的 previous/点修守卫基线
            // 必须保持模型自己的输出。
            //
            // 裁决核验的事实边界是**登记表全量**（t3）：候选集只是评审的可见
            // 面之一，正确宿主常常在近邻子集之外（真机 run-1792395614887）。
            // 候选集归属降级为日志注记；反伪造边界不变——concept_id 必须真实
            // 存在且未退役（即 register_alias_tx 可接受的宿主）。
            let mut rulings: HashMap<String, String> = HashMap::new();
            for ruling in &review.reuse {
                let key = ruling.name.trim().to_lowercase();
                let Some(plan) = outcome
                    .nominations
                    .iter()
                    .find(|plan| plan.name.to_lowercase() == key)
                else {
                    tracing::warn!(
                        course = course_id.as_str(),
                        name = %ruling.name,
                        "review reuse ruling dropped — name not in this batch"
                    );
                    continue;
                };
                let concept_id = ruling.concept_id.trim();
                if !outcome.registry_concept_ids.contains(concept_id) {
                    tracing::warn!(
                        course = course_id.as_str(),
                        name = %ruling.name,
                        concept_id,
                        "review reuse ruling dropped — concept_id is not an active registry concept"
                    );
                    continue;
                }
                if !plan
                    .candidates
                    .iter()
                    .any(|candidate| candidate.concept_id == concept_id)
                {
                    tracing::info!(
                        course = course_id.as_str(),
                        name = %ruling.name,
                        concept_id,
                        "review reuse ruling host outside the nomination candidate set — accepted (registry-valid)"
                    );
                }
                rulings.insert(key, concept_id.to_owned());
            }
            // ── 评审建议机械转化（t4）：advice 指向「裁为别名/复用」且宿主可
            // 机械解析的 teaches/mints 问题，由代码直接执行——合成 reuse 裁决
            // （复用 j04 落池通道：不铸新行、落宿主活跃别名、引用随裁决改
            // 写），问题从报告剔除，不折返模型。真机实录（run-1792403482606
            // call 3）这类问题每次打回烧一轮重裁预算（点修越权高发段）。解析
            // 面从窄（见 advice_alias 模块），解析失败的问题照常打回。转化不
            // 进点修守卫基线（不改写 candidate；记录进批次 note 留痕）。
            let mut advice_converted: Vec<(String, String, String)> = Vec::new();
            {
                let nomination_names: HashSet<String> = outcome
                    .nominations
                    .iter()
                    .map(|plan| plan.name.trim().to_lowercase())
                    .collect();
                let parsed = crate::learning_graph::advice_alias::advice_reuse_conversions(
                    &review.problems,
                    &nomination_names,
                    &outcome.registry_name_hosts,
                    &outcome.registry_concept_ids,
                );
                let mut dropped: Vec<usize> = Vec::new();
                for (index, name, host_id) in parsed {
                    if rulings.contains_key(&name) {
                        // 评审显式裁决优先；advice 只是补位。
                        continue;
                    }
                    let canonical = outcome
                        .registry_name_hosts
                        .values()
                        .find_map(|(id, canonical)| (id == &host_id).then_some(canonical.clone()))
                        .unwrap_or_else(|| host_id.clone());
                    tracing::warn!(
                        course = course_id.as_str(),
                        name = %name,
                        host = %canonical,
                        concept_id = %host_id,
                        "review advice mechanically converted to a reuse ruling — problem dropped"
                    );
                    advice_converted.push((name.clone(), canonical, host_id.clone()));
                    rulings.insert(name, host_id);
                    dropped.push(index);
                }
                if !dropped.is_empty() {
                    let mut kept = Vec::with_capacity(review.problems.len());
                    for (index, problem) in review.problems.drain(..).enumerate() {
                        if !dropped.contains(&index) {
                            kept.push(problem);
                        }
                    }
                    review.problems = kept;
                }
            }
            // 撞池机械复用（gate 面已吸收红条）：裁决合入——机械事实优先于
            // 评审/advice 裁决（撞池名落别的宿主会在落池时 Conflict）。
            for (name, host_id) in &outcome.auto_reusals {
                rulings.insert(name.clone(), host_id.clone());
            }
            // ── 评审「移出本批」建议机械执行（t9）：同批内容依赖类问题的
            // 唯一正路是摘节点（评审判据 2 明文；gate 只能看 assumes 按名
            // 引用，看不见内容级依赖）。advice 含「移出本批」且 node 精确
            // 匹配批内标题 → 摘除留痕，问题剔除；否则照常打回。真机实录
            // （run-1792473152116 call 3）：这类 reject 走重裁必触发点修越
            // 权烧光预算——「移出本批」是逐字可执行的动作，不折返模型。
            let mut removed_nodes: Vec<String> = Vec::new();
            if review.problems.iter().any(|problem| problem.advice.contains("移出本批")) {
                let mut kept_nodes = Vec::with_capacity(candidate.nodes.len());
                for node in candidate.nodes.drain(..) {
                    let remove = review.problems.iter().any(|problem| {
                        problem.advice.contains("移出本批")
                            && !problem.node.trim().is_empty()
                            && problem.node.trim() == node.title.trim()
                    });
                    if remove {
                        removed_nodes.push(node.title.trim().to_owned());
                    } else {
                        kept_nodes.push(node);
                    }
                }
                candidate.nodes = kept_nodes;
                if !removed_nodes.is_empty() {
                    let mut kept_problems = Vec::with_capacity(review.problems.len());
                    for problem in review.problems.drain(..) {
                        let resolved = problem.advice.contains("移出本批")
                            && removed_nodes.iter().any(|title| title == problem.node.trim());
                        if !resolved {
                            kept_problems.push(problem);
                        }
                    }
                    review.problems = kept_problems;
                    tracing::warn!(
                        course = course_id.as_str(),
                        nodes = ?removed_nodes,
                        "review node-removal advice executed mechanically — nodes dropped from the batch"
                    );
                }
            }
            if !rulings.is_empty() && !review.problems.is_empty() {
                let resolved_mask: Vec<bool> = review
                    .problems
                    .iter()
                    .map(|problem| {
                        matches!(problem.field.as_str(), "teaches" | "mints")
                            && rulings.keys().any(|key| {
                                // 「」/『』两种书名习惯都认（真机实录混用）。
                                problem.issue.contains(&format!("「{key}」"))
                                    || problem.issue.contains(&format!("『{key}』"))
                            })
                    })
                    .collect();
                let resolved_count = resolved_mask.iter().filter(|resolved| **resolved).count();
                if resolved_count > 0 {
                    tracing::warn!(
                        course = course_id.as_str(),
                        resolved = resolved_count,
                        "review problems resolved by reuse rulings — dropped from the report"
                    );
                    let mut kept = Vec::with_capacity(review.problems.len());
                    for (problem, resolved) in review.problems.drain(..).zip(resolved_mask) {
                        if !resolved {
                            kept.push(problem);
                        }
                    }
                    review.problems = kept;
                }
            }
            if !review.problems.is_empty() {
                let report = crate::learning_graph::format_review_problems(&review.problems);
                tracing::warn!(
                    course = course_id.as_str(),
                    attempt,
                    problem_count = review.problems.len(),
                    report = %report,
                    "growth batch rejected by concept review"
                );
                self.emit_graph_event(course_id, "growth_redraft", serde_json::json!({
                    "stage": "concept_review",
                    "problems": report,
                }));
                let fingerprint = serde_json::to_string(&review.problems)
                    .unwrap_or_else(|_| normalize_report(&report));
                if should_stop_early(attempt, &fingerprint, prev_fingerprint.as_deref(), false) {
                    tracing::warn!(
                        course = course_id.as_str(),
                        attempt,
                        "growth rejected by the same review problems twice — stopping early"
                    );
                    stop_reason = "early_stop";
                    last = report;
                    break 'budget;
                }
                prev_fingerprint = Some(fingerprint);
                last = report.clone();
                feedback = report;
                previous = Some(candidate);
                continue;
            }
            // new_alias 必须裁复用或直定宿主，否则整批打回指点改走 mints
            // （rulings 已在 problems 处置前核验构建；直定宿主在此并入）。
            let mut reuse_failures: Vec<String> = Vec::new();
            for nomination in &candidate.new_aliases {
                let key = nomination.name.trim().to_lowercase();
                let direct = outcome
                    .nominations
                    .iter()
                    .find(|plan| plan.name.to_lowercase() == key)
                    .and_then(|plan| plan.direct_host.clone());
                match rulings.get(&key).cloned().or(direct) {
                    Some(host) => {
                        rulings.insert(key, host);
                    }
                    None => reuse_failures.push(format!(
                        "- [ng.aliases.not_reuse] 别名提名「{}」未被裁决为复用——别名提名的语义是\
                         「既有概念的新名字」，必须裁复用；若它其实是新概念，不要走 new_aliases，\
                         改走 mints 铸名（mints 提名可自由新铸）",
                        nomination.name.trim()
                    )),
                }
            }
            if !reuse_failures.is_empty() {
                let report = reuse_failures.join("\n");
                tracing::warn!(
                    course = course_id.as_str(),
                    attempt,
                    report = %report,
                    "new_alias nominations without a reuse ruling — whole batch returned with pointer"
                );
                let fingerprint = normalize_report(&report);
                if should_stop_early(attempt, &fingerprint, prev_fingerprint.as_deref(), false) {
                    stop_reason = "early_stop";
                    last = report;
                    break 'budget;
                }
                prev_fingerprint = Some(fingerprint);
                last = report.clone();
                feedback = report;
                previous = Some(candidate);
                continue;
            }
            // 门与评审都过：立即落库，不再对同一稿重复评估（评审非确定，
            // 重复跑可能把已过稿误杀）。机械转化留痕进批次 note（t4：转化面
            // 数据随行；note 不在点修守卫比对面）。
            if !advice_converted.is_empty()
                || !outcome.auto_reusals.is_empty()
                || !removed_nodes.is_empty()
            {
                let mut traces = Vec::new();
                for (name, canonical, _) in &advice_converted {
                    traces.push(format!("评审建议机械转化「{name}」→复用「{canonical}」"));
                }
                for name in outcome.auto_reusals.keys() {
                    traces.push(format!("撞池机械复用「{name}」"));
                }
                for title in &removed_nodes {
                    traces.push(format!("评审摘除「{title}」（移出本批）"));
                }
                if !candidate.note.trim().is_empty() {
                    candidate.note.push('；');
                }
                candidate
                    .note
                    .push_str(&format!("机械复用留痕：{}", traces.join("、")));
            }
            batch = Some(candidate);
            batch_rulings = Some(rulings);
            stop_reason = "success";
            break;
        }
        self.record_growth_stats(attempts_used, stop_reason);
        if stop_reason == "cancelled" {
            return Err(cancel_err());
        }
        let Some(batch) = batch else {
            tracing::warn!(
                course = course_id.as_str(),
                attempts = GROWTH_MAX_ATTEMPTS,
                stop_reason,
                last = %last,
                "growth failed: retry budget exhausted"
            );
            return Err(AppError::UnprocessableEntity(format!(
                "生长批次 {GROWTH_MAX_ATTEMPTS} 轮均未通过（最后一轮原因见下）：\n{last}"
            )));
        };
        // 落库前检查点（写前取消 = 零落库；事务原子兜底同律）。
        if token.is_cancelled() {
            return Err(cancel_err());
        }
        if batch.nodes.is_empty() {
            // 空手批：不落档案（run_growth 删 pending 行），无节点可补
            // ——就绪已满或确实无可长方向。
            self.delete_pending_batch(&pending.batch_id).await;
            let ready_count = self.ready_count_for_owner(course_id, &engine.user_id).await?;
            return Ok(GrowthApplied {
                batch_id: pending.batch_id.clone(),
                node_count: 0,
                note: batch.note.clone(),
                ready_count,
                idle: true,
            });
        }
        self.apply_batch(
            course_id,
            engine,
            pending,
            &batch,
            batch_rulings.as_ref().unwrap_or(&HashMap::new()),
        )
        .await
    }

    /// 提名候选集的评审可见面渲染（j04）：每条提名一节，直定宿主注明。
    fn render_nomination_candidates(
        nominations: &[crate::learning_graph::NominationPlan],
    ) -> String {
        let mut sections = Vec::new();
        for plan in nominations {
            let mut lines = Vec::new();
            if let Some(host) = &plan.direct_host {
                lines.push(format!(
                    "- 直定宿主：{host}（定义归一化逐字相同——机械绑定，无需裁决）"
                ));
            }
            for candidate in &plan.candidates {
                lines.push(format!(
                    "- {}（{}）：{}",
                    candidate.canonical, candidate.concept_id, candidate.reason
                ));
            }
            sections.push(format!("提名「{}」的候选：\n{}", plan.name, lines.join("\n")));
        }
        sections.join("\n")
    }

    /// 最近一次生长的重试读数（smoke `attempts_used`/`stop_reason` 的数据
    /// 源；读数面非门禁）。内存态，仅诊断。
    pub(crate) fn record_growth_stats(&self, attempts_used: usize, stop_reason: &str) {
        if let Ok(mut stats) = self.growth_run_stats.lock() {
            *stats = Some(GrowthRunStats {
                attempts_used,
                stop_reason: stop_reason.to_owned(),
            });
        }
    }

    pub fn growth_run_stats(&self) -> Option<GrowthRunStats> {
        self.growth_run_stats
            .lock()
            .ok()
            .and_then(|stats| stats.clone())
    }

    /// 教练上下文（g3 九块）：结构参考材料与地形门口从本课 structure_json
    /// 快照重读（课程内铁律：只读本课快照，不触碰概念登记表）；无快照 =
    /// 两块均为空，教练退回自行盘点。
    async fn build_coach_context(
        &self,
        course_id: &LearningCourseId,
        engine: &crate::learning_graph::GrowthRunner,
        _completer: &dyn LearningCompleter,
    ) -> Result<crate::learning_graph::CoachContext, AppError> {
        let course = sqlx::query(
            "SELECT title, learning_goal, structure_json \
             FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let title: String = course.try_get("title").map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;
        let snapshot: Option<String> = course.try_get("structure_json").map_err(internal)?;
        let envelope = snapshot
            .as_deref()
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()
            .map_err(internal)?;
        let structure = envelope
            .as_ref()
            .and_then(|envelope| envelope.get("structure").cloned());
        let terrain = envelope
            .as_ref()
            .and_then(|envelope| envelope.get("terrain").cloned());
        // 块 1 附注（程度/基础声明）：结构/地形产物的标量回执。
        let proficiency_statement = structure
            .as_ref()
            .map(|structure| {
                structure
                    .get("proficiency_statement")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_owned()
            })
            .unwrap_or_default();
        let learner_basis = terrain
            .as_ref()
            .map(|terrain| {
                terrain
                    .get("learner_basis_assumed")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_owned()
            })
            .unwrap_or_default();
        // 块 2：结构参考材料（全量块摊开，呈现序 = 难度档/前置链深/主干序/块序）。
        let structure_reference = structure
            .as_ref()
            .map(crate::learning_graph::structure_view::render_reference_lines)
            .unwrap_or_default();
        // 块 3：前置地形门口。
        let terrain_doors = terrain
            .as_ref()
            .map(render_terrain_doors)
            .unwrap_or_default();

        let ready_count = self.ready_count_for_owner(course_id, &engine.user_id).await?;
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
            proficiency_statement,
            learner_basis,
            structure_reference,
            terrain_doors,
            roster,
            taught_summary,
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

    /// 覆盖读数（共用存量）：total = 课程全部学习节点，met = completed、
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
             WHERE m.course_id = ? \
             ORDER BY l.position, l.lesson_id",
        )
        .bind(user_value)
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

    /// 结构门所需的全部事实（全局已教账按 canonical 名折叠）+ 提名候选集
    /// 装配（j04 复用通道：defnear 机械面）。近形归一（ng.refs.nearform）会
    /// 就地改写批内引用名——因此拿 `&mut`。
    async fn gate_batch(
        &self,
        course_id: &LearningCourseId,
        batch: &mut crate::learning_graph::ProposedBatch,
    ) -> Result<crate::learning_graph::GateOutcome, AppError> {
        // 已教账（名字 → 最高档）：全部**存活**课程的 teaches（孤儿课时不算
        // 供给，口径与 taught_ledger 一致）。
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT reg.canonical, lc.tier FROM learning_lesson_concepts lc \
             JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
             JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id AND c.deleted_at IS NULL \
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

        // 登记表全部活跃名字（canonical ∪ 活跃别名，小写）+ 名字宿主映射
        // （撞名消息指引）+ 近邻装配条目（j04 复用通道）。
        let registry_entries = self.registry_entries().await?;
        let mut registry_names: HashSet<String> = HashSet::new();
        let mut name_owner: HashMap<String, String> = HashMap::new();
        for entry in &registry_entries {
            registry_names.insert(entry.canonical.trim().to_lowercase());
            name_owner.insert(
                entry.canonical.trim().to_lowercase(),
                entry.canonical.trim().to_owned(),
            );
            for alias in &entry.active_aliases {
                registry_names.insert(alias.trim().to_lowercase());
                name_owner.insert(
                    alias.trim().to_lowercase(),
                    entry.canonical.trim().to_owned(),
                );
            }
        }

        // advice 机械转化的宿主解析面（t4）：活跃名 → (concept_id, canonical)。
        // 撞库歧义名不参与机械解析（活跃池按单点登记应唯一，防御性收窄）。
        let mut ambiguous_names: HashSet<String> = HashSet::new();
        let mut registry_name_hosts: HashMap<String, (String, String)> = HashMap::new();
        for entry in &registry_entries {
            let canonical = entry.canonical.trim().to_owned();
            let mut names: Vec<&str> = vec![entry.canonical.as_str()];
            names.extend(entry.active_aliases.iter().map(String::as_str));
            for name in names {
                let key = name.trim().to_lowercase();
                match registry_name_hosts.get(&key) {
                    Some((existing_id, _)) if existing_id != &entry.concept_id => {
                        ambiguous_names.insert(key);
                    }
                    Some(_) => {}
                    None => {
                        registry_name_hosts
                            .insert(key, (entry.concept_id.clone(), canonical.clone()));
                    }
                }
            }
        }
        for key in &ambiguous_names {
            registry_name_hosts.remove(key);
        }

        // 课程已有节点标题（批内标题与课程内唯一）。
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

        let (mut errors, nearform_warns) = crate::learning_graph::validate_batch(
            batch,
            &coverage,
            &registry_names,
            &name_owner,
            &existing_titles,
        );
        for warn in &nearform_warns {
            tracing::warn!(
                course = course_id.as_str(),
                kind = warn.kind,
                "{}",
                warn.message
            );
        }

        // 铸名撞登记表现有名 → 机械复用（t4 扩展；run-1792402566347 实录：
        // 撞名红条打回后模型只会重提同名，烧光重裁预算）。名字唯一池下
        // 「直接引用即可」与「复用落别名」同旨——后者是机械可判定的，不送
        // 模型；对应 ng.mints.collision 红条在此吸收。窄条件：canonical 撞
        // 池且未入池别名不撞**别的**概念（否则落池 Conflict，照常红条打回）。
        let mut auto_reusals: HashMap<String, String> = HashMap::new();
        let mut auto_reusal_names: Vec<String> = Vec::new();
        for mint in &batch.mints {
            let canonical = mint.canonical.trim();
            let key = canonical.to_lowercase();
            let Some((host_id, _)) = registry_name_hosts.get(&key) else {
                continue;
            };
            let alias_conflict = mint.aliases.iter().any(|alias| {
                registry_name_hosts
                    .get(&alias.trim().to_lowercase())
                    .is_some_and(|(owner, _)| owner != host_id)
            });
            if alias_conflict {
                continue;
            }
            tracing::warn!(
                course = course_id.as_str(),
                name = canonical,
                concept_id = %host_id,
                "mint canonical already owned by the registry — mechanically reused (j04 pooling), collision error absorbed"
            );
            auto_reusal_names.push(canonical.to_owned());
            auto_reusals.insert(key, host_id.clone());
        }
        if !auto_reusal_names.is_empty() {
            errors.retain(|error| {
                !(error.kind == "ng.mints.collision"
                    && auto_reusal_names
                        .iter()
                        .any(|name| error.message.contains(&format!("「{name}」"))))
            });
        }

        // 同批互不供给的机械剪枝（learnhub 约束过滤纯代码同旨——可机械判定
        // 的不送模型）：节点 assumes 引用了「本课程内未教过、但本批兄弟正在
        // teach」的概念时，评审必打回且修法只有移出本批；与其烧一轮点修重
        // 裁，本批直接摘除该节点留痕，它的需求下一批自然再长。注意口径是
        // **课程内**已教：全局账（跨课程）只供覆盖律，不供本课教学序。
        if errors.is_empty() {
            let course_taught: HashSet<String> = sqlx::query_as(
                "SELECT reg.canonical FROM learning_lesson_concepts lc \
                 JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id \
                 JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
                 JOIN learning_modules m ON m.module_id = l.module_id \
                 WHERE m.course_id = ? AND lc.role = 'teaches'",
            )
            .bind(course_id.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|(canonical,): (String,)| canonical.trim().to_lowercase())
            .collect();
            let sibling_teaches: HashSet<String> = batch
                .nodes
                .iter()
                .flat_map(|node| node.teaches.iter())
                .map(|concept| concept.name.trim().to_lowercase())
                .collect();
            let before = batch.nodes.len();
            batch.nodes.retain(|node| {
                let sibling_dependent = node.assumes.iter().any(|assumption| {
                    let key = assumption.name.trim().to_lowercase();
                    !course_taught.contains(&key) && sibling_teaches.contains(&key)
                });
                if sibling_dependent {
                    tracing::warn!(
                        course = course_id.as_str(),
                        node = %node.title,
                        "pruned node assuming same-batch teaches（同批互不供给；移出本批，下一批再长）"
                    );
                }
                !sibling_dependent
            });
            if batch.nodes.len() < before {
                tracing::warn!(
                    course = course_id.as_str(),
                    pruned = before - batch.nodes.len(),
                    remaining = batch.nodes.len(),
                    "growth batch pruned for same-batch dependencies"
                );
            }
        }

        // 提名候选集装配（j04 复用通道的机械面）：每条 mints canonical 与
        // new_aliases 提名各装配一份；定义归一化直定宿主在此定案（不送裁
        // 决）；new_aliases 无近邻 = 门级打回（登记表空/无近邻）。
        let mut nominations: Vec<crate::learning_graph::NominationPlan> = Vec::new();
        let mut push_nomination = |nominations: &mut Vec<crate::learning_graph::NominationPlan>,
                                   name: &str,
                                   definition: &str| {
            let name = name.trim();
            if name.is_empty() || nominations.iter().any(|plan| plan.name == name) {
                return;
            }
            let nomination = crate::learning_graph::defnear::Nomination {
                name: name.to_owned(),
                definition: definition.to_owned(),
            };
            let direct_host = crate::learning_graph::defnear::definition_exact_host(
                &nomination,
                &registry_entries,
            )
            .map(|entry| entry.concept_id.clone());
            let candidates =
                crate::learning_graph::defnear::assemble_neighbors(&nomination, &registry_entries);
            nominations.push(crate::learning_graph::NominationPlan {
                name: name.to_owned(),
                direct_host,
                candidates,
            });
        };
        for mint in &batch.mints {
            push_nomination(&mut nominations, &mint.canonical, &mint.definition);
        }
        for nomination in &batch.new_aliases {
            push_nomination(&mut nominations, &nomination.name, &nomination.definition);
            // 无近邻 = 门级打回（登记表空或机械面找不到任何候选）。
            let plan = nominations
                .iter()
                .find(|plan| plan.name == nomination.name.trim())
                .expect("just pushed");
            if plan.candidates.is_empty() {
                errors.push(crate::learning_graph::GateError {
                    kind: "ng.aliases.no_neighbor",
                    message: format!(
                        "别名提名「{}」在登记表找不到任何近邻候选（登记表为空或无近邻）——\
                         别名提名必须指向既有概念；这是新概念就改走 mints 铸名",
                        nomination.name.trim()
                    ),
                });
            }
        }
        // 深度水位门已退役（g3：档位比较是精度剧场）——难度诚实性由 AI 概念
        // 评审门作语义兜底。
        Ok(crate::learning_graph::GateOutcome {
            errors,
            nominations,
            registry_concept_ids: registry_entries
                .iter()
                .map(|entry| entry.concept_id.clone())
                .collect(),
            registry_name_hosts,
            auto_reusals,
        })
    }

    /// 落库：铸名（含复用落池）→ 节点 → 概念网 → 完成裁决 → 批次档案，单事务。
    /// `rulings` = 评审复用裁决（j04：提名名 → 宿主 concept_id；mints 复用
    /// 不铸新行、new_aliases 按裁决落池）。
    async fn apply_batch(
        &self,
        course_id: &LearningCourseId,
        engine: &crate::learning_graph::GrowthRunner,
        pending: &PendingBatch,
        batch: &crate::learning_graph::ProposedBatch,
        rulings: &HashMap<String, String>,
    ) -> Result<GrowthApplied, AppError> {
        use crate::learning_graph::alias_records::{register_alias_tx, AliasSpec};
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

        // 0) 评审复用裁决先行：提名名 → 宿主（引用解析与落池都按它走）。
        let mut name_to_concept: HashMap<String, String> = HashMap::new();
        for (name, concept_id) in rulings {
            name_to_concept.insert(name.clone(), concept_id.clone());
        }
        // new_aliases 经登记单点落池（四保证：宿主非废弃/活跃占池/幂等/复活）。
        for nomination in &batch.new_aliases {
            let Some(concept_id) = rulings.get(&nomination.name.trim().to_lowercase()) else {
                return Err(AppError::Internal(format!(
                    "new alias {} landed without a reuse ruling (gate miss)",
                    nomination.name
                )));
            };
            let spec = AliasSpec::projection(
                &nomination.name,
                Some(&nomination.definition),
                Some(&nomination.source),
            );
            match register_alias_tx(&mut transaction, concept_id, &spec).await? {
                outcome => tracing::info!(
                    concept = concept_id,
                    alias = %spec.name,
                    ?outcome,
                    "new alias registered via the single registration point"
                ),
            }
        }

        // 1) 铸名（canonical/别名唯一性已过门；事务内幂等重查防并发；
        // deprecated = 0 与活跃别名同口径——退役概念不接新名，退役名不占池）。
        for mint in &batch.mints {
            let canonical_key = mint.canonical.trim().to_lowercase();
            // 评审复用裁决：不铸新行——提名 canonical + 未入池别名落为该
            // 概念的活跃别名记录（复用即落池，二轮 g3 ①）。
            if let Some(ruling_host) = rulings.get(&canonical_key) {
                let host = ruling_host.clone();
                let spec = AliasSpec::projection(&mint.canonical, Some(&mint.definition), None);
                register_alias_tx(&mut transaction, &host, &spec).await?;
                for alias in &mint.aliases {
                    let alias = alias.trim();
                    if alias.is_empty() {
                        continue;
                    }
                    let spec = AliasSpec::projection(alias, Some(&mint.definition), None);
                    register_alias_tx(&mut transaction, &host, &spec).await?;
                }
                name_to_concept.insert(canonical_key, host.clone());
                for alias in &mint.aliases {
                    name_to_concept.insert(alias.trim().to_lowercase(), host.clone());
                }
                continue;
            }
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT concept_id FROM learning_concept_registry \
                 WHERE (lower(canonical) = ? \
                 OR EXISTS (SELECT 1 FROM json_each(aliases_json) je \
                            WHERE lower(COALESCE(json_extract(je.value, '$.name'), je.value)) = ? \
                              AND COALESCE(json_extract(je.value, '$.status'), 'active') = 'active')) \
                   AND deprecated = 0",
            )
            .bind(&canonical_key)
            .bind(&canonical_key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(internal)?;
            let concept_id = match existing {
                Some(concept_id) => concept_id,
                None => {
                    let concept_id = LearningConceptId::new().into_string();
                    let aliases = serde_json::to_string(
                        &mint.aliases
                            .iter()
                            .filter(|alias| !alias.trim().is_empty())
                            .map(|alias| {
                                serde_json::json!({
                                    "name": alias.trim(),
                                    "status": crate::learning_graph::alias_records::ALIAS_ACTIVE,
                                })
                            })
                            .collect::<Vec<_>>(),
                    )
                    .map_err(internal)?;
                    sqlx::query(
                        "INSERT INTO learning_concept_registry \
                         (concept_id, canonical, aliases_json, definition, source, created_at, updated_at) \
                         VALUES (?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&concept_id)
                    .bind(mint.canonical.trim())
                    .bind(aliases)
                    .bind(mint.definition.trim())
                    // attestation 出处（t8）：纯簿记，缺省 NULL。
                    .bind(mint.source.as_deref().map(str::trim).filter(|s| !s.is_empty()))
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
            // est 缺省 10（专注工作量尺寸锚；列名保留旧名 estimated_minutes）。
            let est = node.est.unwrap_or(10).max(1) as i64;
            sqlx::query(
                "INSERT INTO learning_lessons \
                 (lesson_id, module_id, title, summary, purpose, position, estimated_minutes, \
                  difficulty, content_generated) \
                 VALUES (?, ?, ?, '', ?, ?, ?, ?, 0)",
            )
            .bind(lesson_id.as_str())
            .bind(&module_id)
            .bind(node.title.trim())
            .bind(node.purpose.trim())
            .bind(next_position + offset as i64)
            .bind(est)
            .bind(i64::from(node.difficulty.unwrap_or(2)))
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
                        // 引用解析同口径（二轮 g3 deprecated 修复）：退役行
                        // 不接引用；退役名不占池（活跃别名才可解析）。
                        let resolved: Option<String> = sqlx::query_scalar(
                            "SELECT concept_id FROM learning_concept_registry \
                             WHERE (lower(canonical) = ? \
                             OR EXISTS (SELECT 1 FROM json_each(aliases_json) je \
                                        WHERE lower(COALESCE(json_extract(je.value, '$.name'), je.value)) = ? \
                                          AND COALESCE(json_extract(je.value, '$.status'), 'active') = 'active')) \
                               AND deprecated = 0",
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
                    // ref_name：节点引用该概念时使用的名字（j04——跨课程反查
                    // 按投影名分组的数据源；空名写 NULL）。
                    let ref_name = {
                        let name = concept.name.trim();
                        (!name.is_empty()).then(|| name.to_owned())
                    };
                    sqlx::query(
                        "INSERT INTO learning_lesson_concepts \
                         (lesson_id, concept_id, role, tier, ref_name) VALUES (?, ?, ?, ?, ?)",
                    )
                    .bind(lesson_id.as_str())
                    .bind(&concept_id)
                    .bind(role)
                    .bind(concept.tier.as_str())
                    .bind(ref_name)
                    .execute(&mut *transaction)
                    .await
                    .map_err(internal)?;
                }
            }
            node_ids.push(lesson_id.into_string());
        }

        // 4) 批次档案转正（pending 行已在生长开始时落库，seq 早已分配）。
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
        })
    }

    // ── Readiness（就绪集合的读取侧）────────────────────────────────────

    /// 全局已教账：concept_id → 最高已教档位（跨课程，但只计**存活课程**——
    /// 已删课程的孤儿课时不再供给：它们不可达、不可推荐，却会把新课的入口
    /// 层向上顶（2026-09-29 实测：删课重建后新课首批直接长出 ε-δ/∀∃，因为
    /// 幽灵供给「已覆盖」了底层格）。
    pub(super) async fn taught_ledger(&self) -> Result<crate::learning_graph::ConceptCoverage, AppError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT lc.concept_id, lc.tier FROM learning_lesson_concepts lc \
             JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
             JOIN learning_modules m ON m.module_id = l.module_id \
             JOIN learning_courses c ON c.course_id = m.course_id AND c.deleted_at IS NULL \
             WHERE lc.role = 'teaches'",
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

        let candidates = lesson_rows
            .into_iter()
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

    // ── Views（图视图 / 学习记录 / 概念表）───────────────────────────────

    /// 知识结构读侧投影（wayfinder 票 t1）：课程不存在或已软删 → 404；
    /// 否则 200 恒答（structure: null = 未生成）。IO 只有这一次快照行读，
    /// 重算是 `structure_view::build_structure_view` 纯函数；**不 join 概念
    /// 登记表**（课程边界铁律）。
    pub async fn structure_view(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<crate::models::LearningStructureView, AppError> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT structure_json FROM learning_courses \
             WHERE course_id = ? AND deleted_at IS NULL",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let Some((snapshot,)) = row else {
            return Err(AppError::NotFound(format!("course {course_id}")));
        };
        let envelope = snapshot
            .as_deref()
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()
            .map_err(internal)?;
        // 挂载三态投影的数据面（j08）：现存节点标题 + 挂载行（一课一行）+
        // 当前结构 hash（与落盘 hash 比对 = stale）。
        let node_titles: Vec<String> = sqlx::query_scalar(
            "SELECT l.title FROM learning_lessons l \
             JOIN learning_modules m ON m.module_id = l.module_id \
             WHERE m.course_id = ? ORDER BY l.position, l.lesson_id",
        )
        .bind(course_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mount_row: Option<(String, i64)> = sqlx::query_as(
            "SELECT mounts_json, structure_hash FROM learning_structure_mounts WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let current_hash =
            crate::learning_graph::mounting::fnv1a_64(snapshot.as_deref().unwrap_or("").as_bytes());
        Ok(crate::learning_graph::structure_view::build_structure_view(
            course_id.clone(),
            envelope.as_ref(),
            &node_titles,
            mount_row.as_ref().map(|(json, hash)| (json.as_str(), *hash)),
            current_hash,
        ))
    }

    /// 组装学习图课程视图：就绪集推荐 + 水位读数。
    /// DAG 画布已不存在——节点本体就是模块课时，图视图只承载生长语义
    /// （结构读侧投影走独立的 structure 视图接口）。
    pub(super) async fn assemble_learning_graph_view(
        &self,
        course_id: &LearningCourseId,
        user_id: Option<&UserId>,
    ) -> Result<crate::models::LearningGraphView, AppError> {
        let course = sqlx::query(
            "SELECT learning_goal FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        let goal: String = course.try_get("learning_goal").map_err(internal)?;

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
                 JOIN learning_lessons l ON l.lesson_id = lc.lesson_id \
                 JOIN learning_modules m ON m.module_id = l.module_id \
                 JOIN learning_courses c ON c.course_id = m.course_id \
                    AND c.deleted_at IS NULL \
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
                    est: row.try_get("estimated_minutes").map_err(internal)?,
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
        // 概念 × 本课程节点（教/假定 + 档位 + 学习者状态 + 投影名 ref_name）。
        let ref_rows = sqlx::query(
            "SELECT reg.concept_id, reg.canonical, reg.aliases_json, reg.definition,                     lc.role, lc.tier, lc.ref_name, l.lesson_id, l.title,                     COALESCE(p.status, 'not_started') AS status              FROM learning_lesson_concepts lc              JOIN learning_lessons l ON l.lesson_id = lc.lesson_id              JOIN learning_modules m ON m.module_id = l.module_id              JOIN learning_concept_registry reg ON reg.concept_id = lc.concept_id              LEFT JOIN learning_enrollments e ON e.course_id = m.course_id AND e.user_id = ?              LEFT JOIN learning_lesson_progress p                ON p.lesson_id = l.lesson_id AND p.enrollment_id = e.enrollment_id              WHERE m.course_id = ?              ORDER BY reg.canonical, l.position, l.lesson_id",
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
            "SELECT lc.concept_id, c.title FROM learning_lesson_concepts lc              JOIN learning_lessons l ON l.lesson_id = lc.lesson_id              JOIN learning_modules m ON m.module_id = l.module_id              JOIN learning_courses c ON c.course_id = m.course_id AND c.deleted_at IS NULL              WHERE lc.role = 'teaches' AND m.course_id <> ?",
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
                        // 别名带定义与出处（j04 结构化记录；老形状双读）。
                        aliases: {
                            let aliases_json: String =
                                row.try_get("aliases_json").map_err(internal)?;
                            crate::learning_graph::alias_records::parse_alias_records(
                                &aliases_json,
                            )
                            .into_iter()
                            .filter(|record| {
                                record.status
                                    == crate::learning_graph::alias_records::ALIAS_ACTIVE
                            })
                            .map(|record| crate::models::GraphConceptAliasView {
                                name: record.name,
                                definition: record.definition,
                                attestation: record.attestation,
                            })
                            .collect()
                        },
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
                // 投影名（j04）：节点引用该概念时使用的名字；老行为空。
                ref_name: row
                    .try_get::<Option<String>, _>("ref_name")
                    .map_err(internal)?
                    .filter(|name| !name.trim().is_empty()),
                status: LessonStatus::try_from(status_text.as_str()).map_err(AppError::Internal)?,
            });
        }
        Ok(rows)
    }

    // ── Status/cancel（generation_tasks 注册表薄封装，端点不变）────────────

    /// 登记一个长任务（三轮 t2 统一注册表）：同键并发拒绝（Conflict），
    /// 守卫 drop 未收尾按 failed 留痕；token 供任务体接取消检查点。
    pub(crate) fn begin_generation_task(
        &self,
        kind: &'static str,
        course_key: &str,
        node_id: Option<&str>,
        topic: &str,
    ) -> Result<crate::service::GenerationTaskGuard, AppError> {
        self.generation_tasks.begin(kind, course_key, node_id, topic)
    }

    /// 生成状态（后台指示条的数据源）：在跑的最新任务带主题与已运行秒数。
    pub fn generation_status(&self) -> crate::models::LearningGraphGenerationStatus {
        match self.generation_tasks.running_newest() {
            Some((topic, elapsed_secs)) => crate::models::LearningGraphGenerationStatus {
                running: true,
                topic: Some(topic),
                elapsed_secs: Some(elapsed_secs),
            },
            None => crate::models::LearningGraphGenerationStatus {
                running: false,
                topic: None,
                elapsed_secs: None,
            },
        }
    }

    /// 长任务快照（`GET /api/learning/generation-tasks`）。
    pub fn generation_task_snapshot(&self) -> Vec<crate::service::GenerationTaskView> {
        self.generation_tasks.snapshot()
    }

    /// 统一取消（按任务键）：取消胜出 = cancelled 终态；任务已终态（竞速
    /// 落败）= 如实返回其终态；键不存在 = 404。
    pub fn cancel_generation_task(&self, key: &str) -> Result<String, AppError> {
        match self.generation_tasks.cancel(key)? {
            Some(state) => Ok(state.to_owned()),
            None => self
                .generation_tasks
                .snapshot()
                .into_iter()
                .find(|task| task.key == key)
                .map(|task| task.state)
                .ok_or_else(|| AppError::NotFound(format!("generation task {key}"))),
        }
    }

    /// 置位取消：全部可取消的在跑任务（legacy 全局取消端点的转发面）。
    /// 返回置位时是否存在可取消的在跑任务。
    pub fn cancel_generation(&self) -> bool {
        self.generation_tasks.cancel_running() > 0
    }

    /// 大纲流的取消旗标视图（agent 循环经 CancellableProvider 轮询）：
    /// 最新在跑 outline 任务的 token；无在跑 = 未触发的空令牌。
    pub fn generation_cancel_flag(&self) -> tokio_util::sync::CancellationToken {
        self.generation_tasks.running_token(crate::service::KIND_OUTLINE)
    }

    pub fn generation_cancel_requested(&self) -> bool {
        self.generation_cancel_flag().is_cancelled()
    }

    fn generation_running(&self) -> bool {
        self.generation_tasks
            .snapshot()
            .iter()
            .any(|task| task.state == crate::service::STATE_RUNNING)
    }

    /// 生长任务的注册：主题取课程标题前缀，便于前端指示条展示。
    async fn begin_growth_task(
        &self,
        course_id: &LearningCourseId,
    ) -> Result<crate::service::GenerationTaskGuard, AppError> {
        let title: Option<String> = sqlx::query_scalar(
            "SELECT title FROM learning_courses WHERE course_id = ?",
        )
        .bind(course_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        self.begin_generation_task(
            crate::service::KIND_GROWTH,
            course_id.as_str(),
            None,
            &format!(
                "学习图生长 · {title}",
                title = title.unwrap_or_else(|| course_id.as_str().to_owned())
            ),
        )
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

    fn course_completer(&self) -> Result<Arc<dyn LearningCompleter>, AppError> {
        self.course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| AppError::Conflict("learning graph growth is not configured".into()))
    }

    /// 登记表全名集合（canonical ∪ 别名，小写、未退役）。
    /// 登记表活跃条目（j04 复用通道的装配面）：canonical + 定义 + 活跃别名
    /// （退役不占）。deprecated = 0 单源口径。
    async fn registry_entries(
        &self,
    ) -> Result<Vec<crate::learning_graph::defnear::RegistryEntry>, AppError> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT concept_id, canonical, definition, aliases_json \
             FROM learning_concept_registry WHERE deprecated = 0",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|(concept_id, canonical, definition, aliases_json)| {
                let records =
                    crate::learning_graph::alias_records::parse_alias_records(&aliases_json);
                crate::learning_graph::defnear::RegistryEntry {
                    concept_id,
                    canonical,
                    definition,
                    active_aliases: records
                        .into_iter()
                        .filter(|record| {
                            record.status == crate::learning_graph::alias_records::ALIAS_ACTIVE
                        })
                        .map(|record| record.name)
                        .collect(),
                }
            })
            .collect())
    }

    /// 花名册：每节点一行的紧凑文本（状态 | 标题 | teaches | assumes）。
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
                    // 档位词表用英文（t7：教练面单源词表——花名册是模型
                    // 照抄档位的主要源头）。
                    let tier = crate::learning_graph::ConceptTier::try_from_str(tier)
                        .map(|tier| tier.as_str())
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
            // 档位词表用英文（t7：已教账是模型照抄中文档位的根因源头）。
            .map(|(canonical, tier)| format!("{canonical}@{}", tier.as_str()))
            .collect::<Vec<_>>()
            .join("、"))
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
        // 登记表切片 + 概念别名池（二轮 g3 池可见；j04 别名记录带定义）：
        // 单名概念只列正名；多名概念列「正名=别名1/别名2」，别名带投影定义
        // （j07 R3 投影词表的数据源），超长截断。
        let mut lines = Vec::new();
        let mut pool_lines = Vec::new();
        for (canonical, aliases_json) in rows {
            let records = crate::learning_graph::alias_records::parse_alias_records(&aliases_json);
            let active: Vec<&crate::learning_graph::alias_records::AliasRecord> = records
                .iter()
                .filter(|record| record.status == crate::learning_graph::alias_records::ALIAS_ACTIVE)
                .collect();
            lines.push(canonical.clone());
            if !active.is_empty() {
                let pool: Vec<String> = active
                    .iter()
                    .map(|record| match &record.definition {
                        Some(definition) if !definition.trim().is_empty() => {
                            format!("{}（{}）", record.name, definition.trim())
                        }
                        _ => record.name.clone(),
                    })
                    .collect();
                pool_lines.push(format!(
                    "{}={}",
                    canonical,
                    truncate_text(&pool.join("/"), 120)
                ));
            }
        }
        let mut text = lines.join("\n");
        if !pool_lines.is_empty() {
            text.push_str("\n\n【概念别名池（正名=别名；多名概念才列）】\n");
            text.push_str(&pool_lines.join("\n"));
        }
        Ok(text)
    }
}

/// 展示截断（池行超长保护）。
fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}

/// 可选声明输入的归一：去空白、空串视为未提供、限长（结构产物标量的上游）。
fn optional_statement(raw: Option<&str>, label: &str) -> Result<Option<String>, AppError> {
    let Some(text) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > 500 {
        return Err(AppError::BadRequest(format!("{label} is too long")));
    }
    Ok(Some(text.to_owned()))
}

/// 地形门口的教练上下文渲染（课程内读侧重算）：每门一行
/// `- [G1]（硬前置）判据`，判据即地形产物原文。
fn render_terrain_doors(terrain: &serde_json::Value) -> String {
    let Some(doors) = terrain.get("doors").and_then(|doors| doors.as_array()) else {
        return String::new();
    };
    doors
        .iter()
        .filter_map(|door| {
            let id = door.get("id")?.as_str()?.trim();
            let criterion = door.get("criterion")?.as_str()?.trim();
            if id.is_empty() || criterion.is_empty() {
                return None;
            }
            let kind = match door.get("kind").and_then(|kind| kind.as_str()) {
                Some("self_check") => "可自检",
                _ => "硬前置",
            };
            Some(format!("- [{id}]（{kind}）{criterion}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 生长进行中的批行（生长开始时落库的 pending 行；seq 已在落库时分配）。
pub(crate) struct PendingBatch {
    pub batch_id: String,
}

/// 一批生长落库的结果快照（WS 终态帧的数据源）：idle = 空手批（就绪已满
/// 或无可长方向，不落档案）。
pub(crate) struct GrowthApplied {
    pub batch_id: String,
    pub node_count: usize,
    pub note: String,
    pub ready_count: usize,
    pub idle: bool,
}
