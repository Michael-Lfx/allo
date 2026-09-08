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
        draft.refresh_audit();
        if draft
            .findings
            .iter()
            .any(|finding| finding.severity == crate::learning_graph::SEV_DANGER)
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
            concepts: Vec::new(),
            concept_keys: vec!["c1".into()],
            excerpt: None,
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: None,
            forbidden_concepts: String::new(),
        }
    }

    fn activity_json(kind: &str, extra: serde_json::Value) -> ActivityPack {
        let mut value = serde_json::json!({
            "kind": kind,
            "prompt": "期权的本质是什么？",
            "explanation": "因为买方持有权利。",
            "concepts": ["c1"]
        });
        if let (Some(object), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            object.extend(extra.clone());
        }
        serde_json::from_value(value).unwrap()
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
