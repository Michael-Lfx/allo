//! Lesson draft facade — the `ls_*` agent tool set's backing store.
//! Mirrors `service/learning_graph.rs`: drafts live in memory with a TTL,
//! every patch re-runs the deterministic audit, and `finish_lesson_draft`
//! is the single publish path — a live re-audit gates it (DANGER findings
//! block), then the draft converts into the generation stage's
//! [`LessonOutput`], which the synchronous pipeline persists into the
//! lesson. A failed run keeps the draft (plus the lesson→draft mapping) so
//! a retry resumes instead of rebuilding from scratch.

use super::*;

/// Drafts older than this are evicted (same TTL as the learning-graph
/// drafts): an agent session never outlives the generation timeout by much,
/// so anything older is an abandoned draft (crashed or timed-out session,
/// client gave up) leaking memory.
const LESSON_DRAFT_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

impl LearningService {
    /// Snapshot one lesson draft (drafts are small; cloning under the read
    /// lock is cheaper than holding the lock across any mutation). An entry
    /// older than [`LESSON_DRAFT_TTL`] is reported as not found — only an
    /// abandoned draft can age out.
    fn lesson_draft(&self, draft_id: &str) -> Result<LessonDraft, AppError> {
        self.lesson_drafts
            .read()
            .map_err(|_| AppError::Internal("lesson draft lock poisoned".into()))?
            .get(draft_id)
            .filter(|(_, activity)| activity.elapsed() < LESSON_DRAFT_TTL)
            .map(|(draft, _)| draft.clone())
            .ok_or_else(|| AppError::NotFound(format!("lesson draft {draft_id}")))
    }

    /// The live draft id for a lesson, if any: the retry path's resume key.
    /// A stale mapping (draft expired) is treated as no draft and dropped.
    pub fn live_lesson_draft_for_lesson(&self, lesson_id: &str) -> Option<String> {
        let draft_id = self
            .lesson_draft_ids
            .lock()
            .ok()?
            .get(lesson_id)
            .cloned()?;
        match self.lesson_draft(&draft_id) {
            Ok(_) => Some(draft_id),
            Err(_) => {
                if let Ok(mut ids) = self.lesson_draft_ids.lock() {
                    ids.remove(lesson_id);
                }
                None
            }
        }
    }

    /// Start a lesson draft from the generation context. Synchronous —
    /// unlike the outline draft there is no scope analysis to run; the
    /// grounding (excerpt or course brief) rides on the context. Registers
    /// the lesson→draft mapping for the resume path.
    pub fn create_lesson_draft(
        &self,
        context: LessonGenerationContext,
    ) -> Result<LessonDraftView, AppError> {
        let lesson_id = context.lesson_id.clone();
        let draft_id = generate_id();
        let draft = LessonDraft::new(context);
        let view = draft.view(&draft_id);
        let now = std::time::Instant::now();
        self.lesson_drafts
            .write()
            .map_err(|_| AppError::Internal("lesson draft lock poisoned".into()))?
            .insert(draft_id.clone(), (draft, now));
        if let Ok(mut ids) = self.lesson_draft_ids.lock() {
            ids.insert(lesson_id, draft_id.clone());
        }
        tracing::info!(session = %draft_id, "lesson draft created");
        Ok(view)
    }

    /// Apply a batch of lesson ops and return the per-op verdicts plus a
    /// fresh audit snapshot. Refreshes the TTL timestamp: an active patch
    /// session never ages out.
    pub fn patch_lesson_draft(
        &self,
        draft_id: &str,
        ops: Vec<LessonOp>,
    ) -> Result<LessonPatchReport, AppError> {
        let mut draft = self.lesson_draft(draft_id)?;
        let report = draft.apply_ops(ops);
        self.lesson_drafts
            .write()
            .map_err(|_| AppError::Internal("lesson draft lock poisoned".into()))?
            .insert(draft_id.to_owned(), (draft, std::time::Instant::now()));
        Ok(report)
    }

    /// Overview: document shape, activity list and the audit summary.
    pub fn inspect_lesson_draft(
        &self,
        draft_id: &str,
    ) -> Result<LessonInspectView, AppError> {
        Ok(self.lesson_draft(draft_id)?.inspect())
    }

    /// u06 压缩实验的原文取回通道：按 section_key 返回该节**已落库的完整
    /// 正文**（ls_inspect 可选 detail 面——历史被压缩后模型的按需取回口）。
    /// 只在草稿存活期内可用；未知 key 报 NotFound（模型可读，转 Err 文本）。
    /// 只读，不触审计不 bump revision。
    pub fn lesson_draft_section_body(
        &self,
        draft_id: &str,
        section_key: &str,
    ) -> Result<String, AppError> {
        let draft = self.lesson_draft(draft_id)?;
        let (body, _) = draft
            .section_bodies
            .get(section_key)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "lesson draft {draft_id} has no stored body for section {section_key}"
                ))
            })?;
        Ok(body.clone())
    }

    /// f1 相位化进度的清单读数：draft 当前节清单的 (section_key, title)
    /// 快照，按清单序——课时代理的 `ls_set_section_body` handler 据此换算
    /// 本节位次（k/N）与节标题，组装 section 进度帧。无清单（单篇旧草稿）、
    /// 草稿已过期/不存在都返回空表：调用面空表即不发帧（best-effort，
    /// 观测面绝不 fail 工具调用）。
    pub fn lesson_draft_manifest(&self, draft_id: &str) -> Vec<(String, String)> {
        self.lesson_draft(draft_id)
            .ok()
            .and_then(|draft| draft.section_manifest)
            .map(|manifest| {
                manifest
                    .into_iter()
                    .map(|section| (section.section_key, section.title))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Full findings text — the repair loop's primary input. The audit is
    /// re-run live so the report never reflects a stale cached snapshot.
    pub fn audit_lesson_draft(&self, draft_id: &str) -> Result<String, AppError> {
        let mut draft = self.lesson_draft(draft_id)?;
        draft.refresh_audit();
        Ok(draft.audit_report())
    }

    /// Publish a draft: the deterministic audit gate has the last word.
    /// Danger-grade findings block publishing (the draft survives, so the
    /// agent can keep repairing); a clean draft converts into the
    /// generation stage's [`LessonOutput`] and is removed from the store
    /// (with its resume mapping). The caller (synchronous pipeline or
    /// engine loop) persists it.
    ///
    /// `math_verify_failed`（w1/蓝图 4.5/A3；w1 评审修复口径）：**未复核修
    /// 正**的 verify_math 失败数（ls_finish handler 从计数槽的 unresolved
    /// 槽读出传入——Fail 置增、任一复核 Pass 清零）。> 0 时在 finish 门
    /// refresh 之后追加 warning 留痕（viz_block_lint_dropped 同位——audit
    /// 整表重算会覆盖先加的条目）；不挡发布（发布面无法重放核验，danger
    /// 会把「一道题改不对」放大成整课失败——Q1 裁决）；0 = 零痕迹。自修正
    /// 会话（核验失败→打回→复核过→过闸）零 warning——累计失败数只进观测
    /// 读数面，不进留痕门（自修正不是失败）。
    pub fn finish_lesson_draft(
        &self,
        draft_id: &str,
        math_verify_failed: u32,
    ) -> Result<LessonOutput, AppError> {
        // The finish gate re-runs the deterministic audit LIVE on the
        // draft's current state — the cached findings snapshot is never
        // trusted at the publish boundary, so a draft that was never
        // patched cannot slip through with zero findings.
        let mut draft = self.lesson_draft(draft_id)?;
        // f3 终检兜底（4.4/A4，发布前）：对每节正文跑 lint——仍坏的围栏块
        // 系统剔除 + 该节 visual 声明同步降级 + degraded 标记；剔后重审，
        // 坏块不再挡发布（承诺门按新声明自然过、demo 硬门对 degraded 节
        // 豁免、文字下限天然不撞——prose 剥围栏计算）。
        let dropped_findings = draft.sweep_broken_visual_blocks();
        draft.refresh_audit();
        // 剔除 findings 在审计重算之后追加（audit 整表重算会覆盖先加的
        // 条目）——阻塞报告与 publish 面都能看见剔除事实。
        draft.findings.extend(dropped_findings);
        // w1 verify_math 留痕：同为 refresh 之后追加（整表重算覆盖面同上）。
        // w1 评审修复：计数语义 = 未复核修正数（传入侧见计数槽 unresolved），
        // 文案随之点名「未复核修正」而非累计失败。
        if math_verify_failed > 0 {
            draft
                .findings
                .push(crate::lesson_draft::LessonFinding {
                    severity: crate::course_outline::SEV_WARNING,
                    kind: "math_verify_failed".to_owned(),
                    message: format!("本课 verify_math 有 {math_verify_failed} 次失败未复核修正"),
                });
            tracing::warn!(
                target: "nomifun_learning",
                math_verify_failed,
                "math_verify_failed"
            );
        }
        if draft
            .findings
            .iter()
            .any(|finding| finding.severity == crate::course_outline::SEV_DANGER)
        {
            return Err(AppError::UnprocessableEntity(format!(
                "lesson draft still fails the audit gate:\n{}",
                draft.audit_report()
            )));
        }
        let output = draft.to_output();
        self.lesson_drafts
            .write()
            .map_err(|_| AppError::Internal("lesson draft lock poisoned".into()))?
            .remove(draft_id);
        if let Ok(mut ids) = self.lesson_draft_ids.lock() {
            ids.remove(draft.context.lesson_id.as_str());
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ActivityPack;

    fn context() -> LessonGenerationContext {
        LessonGenerationContext {
            lesson_id: "lesson-x".into(),
            course_title: "测试课程".into(),
            course_description: String::new(),
            module_title: "模块一".into(),
            module_index: 0,
            lesson_title: "课时一".into(),
            lesson_index: 0,
            total_lessons: 1,
            next_lesson_title: None,
            purpose: "理解期权的定义".into(),
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

    fn activity_json(kind: &str, extra: serde_json::Value) -> ActivityPack {
        let mut value = serde_json::json!({
            "kind": kind,
            "prompt": "期权的本质是什么？",
            "explanation": "因为买方持有权利。",
            // sq1（R-c）evidence_missing 升 danger 后，发布面 fixture 须带
            // 逐字引用才能过 finish 闸（已知变更：判底揭底有源）。
            "evidence": "买方持有权利，卖方负有义务。"
        });
        if let (Some(object), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            object.extend(extra.clone());
        }
        serde_json::from_value(value).unwrap()
    }

    fn planned_pack(key: &str, kind: crate::models::SectionKind, visual: &str)
        -> crate::models::SectionPack
    {
        crate::models::SectionPack {
            section_key: key.into(),
            kind,
            title: format!("t-{key}"),
            points: String::new(),
            visual: visual.into(),
            body_md: String::new(),
            degraded: false,
        }
    }

    /// f3 两连败兜底（4.4/A4）：修复轮救不回的坏块由终检系统剔除 + 该节
    /// visual 声明同步降级 + degraded 标记穿出（degraded_keys /
    /// viz_lint_events）——demo 硬门对 degraded 节豁免（唯一豁免门）、概念
    /// 节承诺门按新声明自然过、文字下限不撞（prose 剥围栏计算），发布不
    /// 阻断。存量坏块以字段直塞构造（op 面已被 lint 拦死，该形态只来自
    /// 重规划保留/跨版本续跑等旁路——即终检兜底面对的存量形态）。
    #[tokio::test]
    async fn finish_sweeps_broken_visual_blocks_degrades_and_publishes() {
        use crate::models::SectionKind;

        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        let view = service.create_lesson_draft(context()).unwrap();

        let prose =
            "这是一个足够长的正文段落，用于通过节级质检门的长度下限要求。".repeat(8);
        let bad_mermaid = "```mermaid\nflowchart TD\n\
             A[[\"第1期 本金 P] -->|\"算利息 P×利率\"| B[利息1\"]\n```";
        let mut draft = LessonDraft::new(context());
        let report = draft.apply_ops(vec![LessonOp::SetSectionManifest {
            sections: vec![
                planned_pack("s1", SectionKind::Concept, "流程图"),
                planned_pack("s2", SectionKind::Demo, "流程图"),
                planned_pack("s3", SectionKind::Practice, "无"),
            ],
            capacity_note: None,
        }]);
        assert_eq!(report.accepted.len(), 1, "manifest accepted");
        draft.section_bodies.insert(
            "s1".to_owned(),
            (
                format!("## t-s1\n\n{bad_mermaid}\n\n{prose}"),
                Some(planned_pack("s1", SectionKind::Concept, "流程图")),
            ),
        );
        draft.section_bodies.insert(
            "s2".to_owned(),
            (
                format!("## t-s2\n\n{bad_mermaid}\n\n{prose}"),
                Some(planned_pack("s2", SectionKind::Demo, "流程图")),
            ),
        );
        draft.section_bodies.insert(
            "s3".to_owned(),
            (
                "## t-s3\n\n完成以下判断题，检验对复利滚雪球的理解；作答后阅读解析回顾关键区别。".to_owned(),
                Some(planned_pack("s3", SectionKind::Practice, "无")),
            ),
        );
        draft.activities = vec![
            activity_json("single_choice", serde_json::json!({
                "options": ["权利", "义务", "债务"], "answer": "权利"
            })),
            activity_json("true_false", serde_json::json!({ "answer": true })),
            activity_json("reflection", serde_json::json!({ "answer": null })),
        ];
        draft.estimated_minutes = 15;

        // 直塞草稿覆盖存储（tests 在 service 模块树内，可注入存量形态）。
        service
            .lesson_drafts
            .write()
            .expect("draft lock")
            .insert(view.draft_id.clone(), (draft, std::time::Instant::now()));

        let output = service
            .finish_lesson_draft(&view.draft_id, 0)
            .expect("剔块降声明后发布成功（demo 硬门 degraded 豁免）");
        assert_eq!(
            output.degraded_keys,
            vec!["s1".to_owned(), "s2".to_owned()],
            "degraded_keys 按清单序穿出"
        );
        assert_eq!(output.viz_lint_events, 2, "剔除事件计数穿出");
        let s1 = output
            .sections
            .iter()
            .find(|section| section.section_key == "s1")
            .unwrap();
        assert_eq!(s1.visual, "无", "概念节声明按余块形态降级");
        assert!(!s1.body_md.contains("```mermaid"), "坏块整体剔除");
        assert!(s1.body_md.contains("这是一个足够长的正文段落"), "节其余保留");
        assert!(s1.degraded, "degraded 标记随节穿出");
        let s2 = output
            .sections
            .iter()
            .find(|section| section.section_key == "s2")
            .unwrap();
        assert_eq!(s2.visual, "无");
        assert!(s2.degraded, "demo 节同样 degraded（硬门豁免的前提标记）");
        assert!(
            !output
                .summary
                .contains("第1期 本金 P"),
            "拼装 summary 不再含坏块"
        );
        // 草稿与续跑映射随发布清除（finish 门既有语义）。
        assert!(service.live_lesson_draft_for_lesson("lesson-x").is_none());
    }

    /// 续跑映射生命周期：create 登记 → 失败期间可查（重试定位草稿）→
    /// finish 发布后清除。这是课时级断点续跑的服务端查找键。
    #[tokio::test]
    async fn draft_mapping_follows_the_lifecycle() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        assert!(service.live_lesson_draft_for_lesson("lesson-x").is_none());

        let view = service.create_lesson_draft(context()).unwrap();
        assert_eq!(
            service.live_lesson_draft_for_lesson("lesson-x").as_deref(),
            Some(view.draft_id.as_str()),
            "the mapping locates the live draft for a retry"
        );

        // 门禁未过时发布被拒，草稿与映射保留（续跑的前提）。
        assert!(service.finish_lesson_draft(&view.draft_id, 0).is_err());
        assert!(service.live_lesson_draft_for_lesson("lesson-x").is_some());

        // 补齐到门禁通过后发布：草稿移除，映射清除。
        let body = "这是一个足够长的正文段落，用于通过课时级文档契约的长度下限要求。".repeat(12);
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![
                    LessonOp::SetDocument {
                        document: format!("## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n"),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("single_choice", serde_json::json!({
                            "options": ["权利", "义务", "债务"], "answer": "权利"
                        })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("true_false", serde_json::json!({ "answer": true })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("reflection", serde_json::json!({ "answer": null })),
                    },
                ],
            )
            .unwrap();
        let output = service.finish_lesson_draft(&view.draft_id, 0).unwrap();
        assert_eq!(output.activities.len(), 3);
        assert!(service.live_lesson_draft_for_lesson("lesson-x").is_none());
    }

    /// w1 verify_math 发布面留痕（蓝图 4.5/A3；w1 评审修复口径）：传入计数
    /// > 0 → finish 门 refresh 之后追加 `math_verify_failed` warning
    /// （viz_block_lint_dropped 同位）——不挡发布（干净草稿照样 Ok）；草稿
    /// 另有 danger 时 warning 随阻塞报告可见（留痕的消费面）；计数 = 0 →
    /// 零痕迹（自修正会话——核验失败后复核 Pass——不留痕）。
    #[tokio::test]
    async fn finish_appends_math_verify_failed_warning_and_never_blocks() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        let body = "这是一个足够长的正文段落，用于通过课时级文档契约的长度下限要求。".repeat(12);

        // 干净草稿 + failed=3 → 发布成功（warning 不挡发布）。
        let view = service.create_lesson_draft(context()).unwrap();
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![
                    LessonOp::SetDocument {
                        document: format!("## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n"),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("single_choice", serde_json::json!({
                            "options": ["权利", "义务", "债务"], "answer": "权利"
                        })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("true_false", serde_json::json!({ "answer": true })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("reflection", serde_json::json!({ "answer": null })),
                    },
                ],
            )
            .unwrap();
        let output = service
            .finish_lesson_draft(&view.draft_id, 3)
            .expect("warning 是信息性留痕，不挡发布");
        assert_eq!(output.activities.len(), 3);

        // 带 danger 的草稿：failed=2 → 阻塞报告携带 warning（留痕可见）。
        let flagged = service.create_lesson_draft(context()).unwrap();
        service
            .patch_lesson_draft(
                &flagged.draft_id,
                vec![LessonOp::SetDocument {
                    document: format!("## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n"),
                }],
            )
            .unwrap();
        let blocked = service
            .finish_lesson_draft(&flagged.draft_id, 2)
            .expect_err("缺活动 = danger，发布被拒");
        let report = blocked.to_string();
        assert!(report.contains("math_verify_failed"), "{report}");
        assert!(report.contains("verify_math 有 2 次失败未复核修正"), "{report}");

        // 同草稿 failed=0 → 阻塞报告零 math_verify 痕迹。
        let blocked = service
            .finish_lesson_draft(&flagged.draft_id, 0)
            .expect_err("缺活动 = danger，发布被拒");
        assert!(
            !blocked.to_string().contains("math_verify_failed"),
            "failed=0 必须零痕迹: {blocked}"
        );
    }

    /// sq2 正文半修复环（scope_boundary_hit 升 danger 的门语义重钉，旧
    /// report-only 断言随票面裁决退役）：未豁免命中 → finish 被拒（danger
    /// 挡闸、报告点名 scope_boundary_hit）→ 正文改写为否定性边界句 → 过闸
    /// 发布。「不该无痕地生成」的处置面 = 修复轮改文，不再是带病发布。
    #[tokio::test]
    async fn scope_boundary_hit_blocks_finish_and_the_repair_loop_publishes() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        let mut ctx = context();
        ctx.graph = Some(crate::lesson_draft::GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: Some("教四则运算；不引入方程".into()),
        });
        let view = service.create_lesson_draft(ctx).unwrap();
        let body = "这是一个足够长的正文段落，用于通过课时级文档契约的长度下限要求。".repeat(12);
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![
                    LessonOp::SetDocument {
                        document: format!(
                            "## 描述\n{body}\n## 例子·方程的初步认识\n{body}\n## 验证\n{body}\n"
                        ),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("single_choice", serde_json::json!({
                            "options": ["权利", "义务", "债务"], "answer": "权利"
                        })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("true_false", serde_json::json!({ "answer": true })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("reflection", serde_json::json!({ "answer": null })),
                    },
                ],
            )
            .unwrap();
        // 命中在案：sq2 起 danger 级（挡 finish 闸）。
        let findings = service
            .inspect_lesson_draft(&view.draft_id)
            .unwrap()
            .findings;
        let hits: Vec<_> = findings
            .iter()
            .filter(|f| f.kind == "scope_boundary_hit")
            .collect();
        assert_eq!(hits.len(), 1, "越界命中在案：{findings:?}");
        assert_eq!(hits[0].severity, crate::course_outline::SEV_DANGER);
        let blocked = service
            .finish_lesson_draft(&view.draft_id, 0)
            .expect_err("未豁免 scope 命中 = danger，发布被拒");
        assert!(blocked.to_string().contains("scope_boundary_hit"), "{blocked}");

        // 修复环：正文改成否定性边界句（豁免保留——诚实写边界不报）→ 过闸。
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![LessonOp::SetDocument {
                    document: format!(
                        "## 描述\n{body}本课不引入方程，后续课程再展开。\n## 例子\n{body}\n## 验证\n{body}\n"
                    ),
                }],
            )
            .unwrap();
        let output = service
            .finish_lesson_draft(&view.draft_id, 0)
            .expect("否定性边界句豁免 → 修复后发布成功");
        assert_eq!(output.activities.len(), 3, "发布成功（管线照常走完）");
    }

    /// sq2 题面半修复环（票面 Acceptance「audit 修复环测试：题面命中 →
    /// 改写 → 过闸发布」）：真实 scope 原文（「不教指针/引用语法，不教回
    /// 收细节，留待后续」）+ 已知概念名单（「引用」已立名）——越界题
    ///（判定轴 = 回收路径）→ finish 被拒且报告点名 activity_scope_boundary_
    /// hit → update_activity 换用已教判据重写 → 过闸发布。
    #[tokio::test]
    async fn activity_scope_boundary_hit_blocks_finish_and_repair_publishes() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        let mut ctx = context();
        ctx.graph = Some(crate::lesson_draft::GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: Some(vec!["引用".to_owned(), "局部变量".to_owned()]),
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: Some("教栈上临时量与堆上长存数据的分界；不教指针/引用语法，不教回收细节，留待后续".into()),
        });
        let view = service.create_lesson_draft(ctx).unwrap();
        let body = "这是一个足够长的正文段落，用于通过课时级文档契约的长度下限要求。".repeat(12);
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![
                    LessonOp::SetDocument {
                        document: format!(
                            "## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n"
                        ),
                    },
                    // 命中臂（票面实锤形态，position 7 的 s4 单选题重演）：
                    // 正确选项判定轴整条建立在排除概念「回收」上。
                    LessonOp::AddActivity {
                        activity: activity_json("single_choice", serde_json::json!({
                            "prompt": "下列哪种写法的回收路径与持有者判断正确？",
                            "options": ["值仍在堆区，再也用不到", "由引用计数立即回收"],
                            "answer": "值仍在堆区，再也用不到"
                        })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("true_false", serde_json::json!({ "answer": true })),
                    },
                    LessonOp::AddActivity {
                        activity: activity_json("reflection", serde_json::json!({ "answer": null })),
                    },
                ],
            )
            .unwrap();
        let blocked = service
            .finish_lesson_draft(&view.draft_id, 0)
            .expect_err("题面命中 scope 排除概念 = danger，发布被拒");
        assert!(
            blocked
                .to_string()
                .contains("activity_scope_boundary_hit"),
            "{blocked}"
        );

        // 修复环：换用本节点已教判据重写（判定轴 = 已教概念「局部变量/
        // 栈区」，零排除词）→ 过闸发布。
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![LessonOp::UpdateActivity {
                    position: 0,
                    activity: activity_json("single_choice", serde_json::json!({
                        "prompt": "主流程建立局部变量并在返回后不再使用它，这个值存放在哪里？",
                        "options": ["栈区", "堆区", "寄存器"],
                        "answer": "堆区",
                        "evidence": "这是一个足够长的正文段落。"
                    })),
                }],
            )
            .unwrap();
        let output = service
            .finish_lesson_draft(&view.draft_id, 0)
            .expect("已教判据重写后过闸发布");
        assert_eq!(output.activities.len(), 3, "发布成功（管线照常走完）");
    }

    /// f1 相位化进度的清单读数：`lesson_draft_manifest` 返回 (section_key,
    /// title) 快照（清单序）；无清单的草稿与未知 draft_id 都返回空表——
    /// 课时代理的 section 帧据此换算位次（k/N）与节标题，空表 = 不发帧。
    #[tokio::test]
    async fn manifest_readout_lists_keys_and_titles_in_order() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());
        assert!(service.lesson_draft_manifest("no-such-draft").is_empty());

        let view = service.create_lesson_draft(context()).unwrap();
        assert!(
            service.lesson_draft_manifest(&view.draft_id).is_empty(),
            "no manifest yet"
        );

        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![LessonOp::SetSectionManifest {
                    sections: vec![
                        planned_pack("s1", crate::models::SectionKind::Concept, "表格"),
                        planned_pack("s2", crate::models::SectionKind::Practice, "无"),
                    ],
                    capacity_note: None,
                }],
            )
            .unwrap();
        assert_eq!(
            service.lesson_draft_manifest(&view.draft_id),
            vec![
                ("s1".to_owned(), "t-s1".to_owned()),
                ("s2".to_owned(), "t-s2".to_owned()),
            ],
            "(key, title) pairs in manifest order"
        );
    }
}
