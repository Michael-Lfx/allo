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
    pub fn finish_lesson_draft(&self, draft_id: &str) -> Result<LessonOutput, AppError> {
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
            "explanation": "因为买方持有权利。"
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
            .finish_lesson_draft(&view.draft_id)
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
        assert!(service.finish_lesson_draft(&view.draft_id).is_err());
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
        let output = service.finish_lesson_draft(&view.draft_id).unwrap();
        assert_eq!(output.activities.len(), 3);
        assert!(service.live_lesson_draft_for_lesson("lesson-x").is_none());
    }
}
