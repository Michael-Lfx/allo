//! Course outline draft facade — the `co_*` agent tool set's backing store.
//! Mirrors `service/concept_graph.rs`: drafts live in memory, every patch
//! re-runs the deterministic audit, and `finish_course_outline_draft` is the
//! single publish path — a live re-audit gates it (DANGER findings block),
//! then the draft converts into the generation stage's [`Blueprint`], which
//! the synchronous pipeline assembles and imports into the course catalog.

use super::*;

/// The scope-analysis topic is a best-effort hint, not a document: a long
/// course description is truncated at this many characters before the scope
/// call.
const SCOPE_TOPIC_MAX_CHARS: usize = 400;

impl LearningService {
    /// Snapshot one outline draft (drafts are small; cloning under the read
    /// lock is cheaper than holding the lock across any mutation).
    fn outline_draft(&self, draft_id: &str) -> Result<OutlineDraft, AppError> {
        self.course_outline_drafts
            .read()
            .map_err(|_| AppError::Internal("course outline draft lock poisoned".into()))?
            .get(draft_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("course outline draft {draft_id}")))
    }

    /// Start an outline draft: best-effort scope analysis first (a failed
    /// scope call degrades to a scope-free draft, exactly like the concept
    /// graph), then an empty outline the agent builds with
    /// `patch_course_outline_draft`. The sampled corpus rides on the brief.
    pub async fn create_course_outline_draft(
        &self,
        brief: crate::course_outline::OutlineBrief,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<crate::course_outline::draft::OutlineDraftView, AppError> {
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("course outline generation is not configured".into())
            })?;
        let scope_topic = match (&brief.description, &brief.knowledge_base) {
            (Some(description), _) => {
                description.trim().chars().take(SCOPE_TOPIC_MAX_CHARS).collect::<String>()
            }
            (_, Some(kb)) => {
                let mut topic = kb.name.trim().to_owned();
                if !kb.description.trim().is_empty() {
                    topic.push_str("：");
                    topic.push_str(kb.description.trim());
                }
                topic.chars().take(SCOPE_TOPIC_MAX_CHARS).collect()
            }
            _ => {
                return Err(AppError::BadRequest(
                    "course outline brief carries no generation source".into(),
                ))
            }
        };
        let scope = crate::concept_graph::analyze_scope(
            completer.as_ref(),
            model_override,
            &scope_topic,
            None,
        )
        .await;
        let draft_id = generate_id();
        let samples = brief.samples.clone();
        let draft = OutlineDraft::new(brief, samples, scope);
        let view = draft.view(&draft_id);
        self.course_outline_drafts
            .write()
            .map_err(|_| AppError::Internal("course outline draft lock poisoned".into()))?
            .insert(draft_id.clone(), draft);
        tracing::info!(session = %draft_id, "course outline draft created");
        Ok(view)
    }

    /// Apply a batch of outline ops and return the per-op verdicts plus a
    /// fresh audit snapshot.
    pub fn patch_course_outline_draft(
        &self,
        draft_id: &str,
        ops: Vec<crate::course_outline::draft::OutlineOp>,
    ) -> Result<crate::course_outline::draft::OutlinePatchReport, AppError> {
        let mut draft = self.outline_draft(draft_id)?;
        let report = draft.apply_ops(ops);
        self.course_outline_drafts
            .write()
            .map_err(|_| AppError::Internal("course outline draft lock poisoned".into()))?
            .insert(draft_id.to_owned(), draft);
        Ok(report)
    }

    /// Overview: sizes, module/lesson layout, concept bindings and the
    /// audit summary.
    pub fn inspect_course_outline_draft(
        &self,
        draft_id: &str,
    ) -> Result<crate::course_outline::draft::OutlineInspectView, AppError> {
        Ok(self.outline_draft(draft_id)?.inspect())
    }

    /// Filtered entity list (substring over keys/titles).
    pub fn query_course_outline_draft(
        &self,
        draft_id: &str,
        filter: crate::course_outline::draft::OutlineQuery,
    ) -> Result<crate::course_outline::draft::OutlineQueryView, AppError> {
        Ok(self.outline_draft(draft_id)?.query(&filter))
    }

    /// Full findings text plus the scope checklists and their live
    /// coverage — the repair loop's primary input. The audit is re-run
    /// live so the report never reflects a stale cached snapshot.
    pub fn audit_course_outline_draft(&self, draft_id: &str) -> Result<String, AppError> {
        let mut draft = self.outline_draft(draft_id)?;
        draft.refresh_audit();
        Ok(draft.audit_report())
    }

    /// The scope reference text (the generation loop's coverage
    /// checklist); `None` when no scope analysis ran for this draft.
    pub fn scope_reference_course_outline_draft(
        &self,
        draft_id: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(self
            .outline_draft(draft_id)?
            .scope
            .as_ref()
            .map(|scope| scope.scope.clone()))
    }

    /// Read one sampled file's excerpt by exact path (the `co_read` tool).
    /// `None` when the path is not part of the sampled corpus.
    pub fn read_course_outline_draft_sample(
        &self,
        draft_id: &str,
        path: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(self
            .outline_draft(draft_id)?
            .read_sample(path)
            .map(str::to_owned))
    }

    /// The sampled corpus' paths (surfaced on the draft view so the agent
    /// can pick grounding sources without guessing paths).
    pub fn course_outline_draft_sample_paths(
        &self,
        draft_id: &str,
    ) -> Result<Vec<String>, AppError> {
        Ok(self.outline_draft(draft_id)?.sample_paths())
    }

    /// Publish a draft: the deterministic audit gate has the last word.
    /// Danger-grade findings block publishing (the draft survives, so the
    /// agent can keep repairing); a clean draft converts into the
    /// generation stage's [`Blueprint`] and is removed from the store. The
    /// caller (synchronous pipeline or engine loop) assembles and imports
    /// the returned blueprint into the course catalog.
    pub fn finish_course_outline_draft(&self, draft_id: &str) -> Result<Blueprint, AppError> {
        // The finish gate re-runs the deterministic audit LIVE on the
        // draft's current state — the cached findings snapshot is never
        // trusted at the publish boundary, so a draft that was never
        // patched (or whose patches all failed) cannot slip through with
        // zero findings.
        let mut draft = self.outline_draft(draft_id)?;
        draft.refresh_audit();
        if draft
            .findings
            .iter()
            .any(|finding| finding.severity == crate::concept_graph::SEV_DANGER)
        {
            return Err(AppError::UnprocessableEntity(format!(
                "course outline draft still fails the audit gate:\n{}",
                draft.audit_report()
            )));
        }
        let blueprint = draft.to_blueprint();
        // Defensive: the deterministic draft→blueprint conversion must
        // satisfy the blueprint contract; a violation here is an
        // implementation bug, not model output.
        if let Err(error) = validate_blueprint(
            &blueprint,
            &draft.samples,
            draft.brief.module_count,
            draft.brief.lessons_per_module,
        ) {
            return Err(AppError::Internal(format!(
                "outline draft → blueprint conversion mismatch: {error}"
            )));
        }
        self.course_outline_drafts
            .write()
            .map_err(|_| AppError::Internal("course outline draft lock poisoned".into()))?
            .remove(draft_id);
        Ok(blueprint)
    }
}
