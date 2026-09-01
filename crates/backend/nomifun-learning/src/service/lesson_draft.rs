//! Lesson draft facade — the `ls_*` agent tool set's backing store.
//! Mirrors `service/course_outline.rs`: drafts live in memory, every patch
//! re-runs the deterministic audit, and `finish_lesson_draft` is the single
//! publish path — a live re-audit gates it (DANGER findings block), then
//! the draft converts into the generation stage's [`LessonOutput`], which
//! the synchronous pipeline persists into the lesson.

use super::*;

impl LearningService {
    /// Snapshot one lesson draft (drafts are small; cloning under the read
    /// lock is cheaper than holding the lock across any mutation).
    fn lesson_draft(&self, draft_id: &str) -> Result<LessonDraft, AppError> {
        self.lesson_drafts
            .read()
            .map_err(|_| AppError::Internal("lesson draft lock poisoned".into()))?
            .get(draft_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("lesson draft {draft_id}")))
    }

    /// Start a lesson draft from the generation context. Synchronous —
    /// unlike the outline draft there is no scope analysis to run; the
    /// grounding (excerpt or course brief) rides on the context.
    pub fn create_lesson_draft(
        &self,
        context: LessonGenerationContext,
    ) -> Result<LessonDraftView, AppError> {
        let draft_id = generate_id();
        let draft = LessonDraft::new(context);
        let view = draft.view(&draft_id);
        self.lesson_drafts
            .write()
            .map_err(|_| AppError::Internal("lesson draft lock poisoned".into()))?
            .insert(draft_id.clone(), draft);
        tracing::info!(session = %draft_id, "lesson draft created");
        Ok(view)
    }

    /// Apply a batch of lesson ops and return the per-op verdicts plus a
    /// fresh audit snapshot.
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
            .insert(draft_id.to_owned(), draft);
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
    /// generation stage's [`LessonOutput`] and is removed from the store.
    /// The caller (synchronous pipeline or engine loop) persists it.
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
        Ok(output)
    }
}
