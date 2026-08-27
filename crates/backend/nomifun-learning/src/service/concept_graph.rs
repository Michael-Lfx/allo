use super::*;

impl LearningService {

    // ── Experimental learning-unit network (rough file persistence) ─────
    // JSON files under `{data_dir}/learning-concept-graphs/`; expected to
    // move into the database once the feature matures.

    pub fn set_concept_graph_dir(&self, dir: PathBuf) {
        *self
            .concept_graph_dir
            .write()
            .expect("learning concept graph dir lock poisoned") = Some(dir);
    }

    fn concept_graph_dir(&self) -> Result<PathBuf, AppError> {
        self.concept_graph_dir
            .read()
            .map_err(|_| AppError::Internal("learning concept graph dir lock poisoned".into()))?
            .clone()
            .ok_or_else(|| AppError::Conflict("concept graphs are not configured".into()))
    }

    fn concept_graph_path(&self, id: &LearningConceptGraphId) -> Result<PathBuf, AppError> {
        Ok(self.concept_graph_dir()?.join(format!("{}.json", id.as_str())))
    }

    /// Decompose a learning goal into a learning-unit network and persist it
    /// as JSON. When the agent engine is injected, the two-loop agent
    /// pipeline owns the whole lifecycle (draft tools + audit-gated
    /// publish); otherwise the legacy one-shot generation runs (fallback
    /// for tests and direct calls).
    pub async fn generate_concept_graph(
        &self,
        user_id: &UserId,
        request: crate::concept_graph::GenerateConceptGraphRequest,
    ) -> Result<crate::concept_graph::ConceptGraphRecord, AppError> {
        let topic = request.topic.trim();
        if topic.is_empty() {
            return Err(AppError::BadRequest(
                "concept graph topic must not be empty".into(),
            ));
        }
        if topic.chars().count() > 200 {
            return Err(AppError::BadRequest("concept graph topic is too long".into()));
        }
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let engine = self
            .concept_graph_engine
            .read()
            .map_err(|_| AppError::Internal("concept graph engine lock poisoned".into()))?
            .clone();
        if let Some(engine) = engine {
            return engine
                .generate(
                    user_id,
                    topic,
                    model_override.map(|(provider, model)| (provider.as_str(), model)),
                )
                .await;
        }
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("concept graph generation is not configured".into())
            })?;
        let dir = self.concept_graph_dir()?;
        tokio::fs::create_dir_all(&dir).await.map_err(|error| {
            AppError::Internal(format!("failed to create concept graph dir: {error}"))
        })?;
        let session = LearningConceptGraphId::new().as_str().to_owned();
        let logger = crate::concept_graph::ConceptGraphLogger::new(&dir, &session);
        let graph = crate::concept_graph::generate_concept_graph(
            completer.as_ref(),
            model_override,
            topic,
            Some(&logger),
        )
        .await?;
        let record = crate::concept_graph::ConceptGraphRecord {
            id: LearningConceptGraphId::new().as_str().to_owned(),
            user_id: user_id.as_str().to_owned(),
            topic: topic.to_owned(),
            graph,
            created_at: now_ms(),
        };
        let path = dir.join(format!("{}.json", record.id));
        let json = serde_json::to_vec_pretty(&record).map_err(|error| {
            AppError::Internal(format!("failed to serialize concept graph: {error}"))
        })?;
        tokio::fs::write(&path, json).await.map_err(|error| {
            AppError::Internal(format!("failed to store concept graph: {error}"))
        })?;
        Ok(record)
    }

    pub async fn list_concept_graphs(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<crate::concept_graph::ConceptGraphSummary>, AppError> {
        let dir = self.concept_graph_dir()?;
        let mut read_dir = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(AppError::Internal(format!(
                    "failed to list concept graphs: {error}"
                )));
            }
        };
        let mut summaries = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            // Corrupt or foreign files are skipped, never fatal: this is
            // rough experimental storage.
            let Ok(contents) = tokio::fs::read(&path).await else {
                continue;
            };
            let Ok(record) =
                serde_json::from_slice::<crate::concept_graph::ConceptGraphRecord>(&contents)
            else {
                continue;
            };
            if record.user_id == user_id.as_str() {
                summaries.push(record.summary());
            }
        }
        summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(summaries)
    }

    pub async fn get_concept_graph(
        &self,
        user_id: &UserId,
        id: &LearningConceptGraphId,
    ) -> Result<crate::concept_graph::ConceptGraphRecord, AppError> {
        let path = self.concept_graph_path(id)?;
        let contents = tokio::fs::read(&path)
            .await
            .map_err(|_| AppError::NotFound(format!("concept graph {id}")))?;
        let record: crate::concept_graph::ConceptGraphRecord = serde_json::from_slice(&contents)
            .map_err(|error| {
                AppError::Internal(format!("corrupt concept graph file: {error}"))
            })?;
        if record.user_id != user_id.as_str() {
            return Err(AppError::NotFound(format!("concept graph {id}")));
        }
        Ok(record)
    }

    pub async fn delete_concept_graph(
        &self,
        user_id: &UserId,
        id: &LearningConceptGraphId,
    ) -> Result<(), AppError> {
        // Ownership check first so a foreign id 404s instead of deleting.
        let _ = self.get_concept_graph(user_id, id).await?;
        let path = self.concept_graph_path(id)?;
        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| AppError::Internal(format!("failed to delete concept graph: {error}")))
    }

    /// Manually triggered repair: the stored audit report is the decision
    /// basis, the model applies local patch operations (add/reverse/
    /// split/merge), and the merged graph is re-audited and persisted over
    /// the same record.
    pub async fn repair_concept_graph(
        &self,
        user_id: &UserId,
        id: &LearningConceptGraphId,
        request: crate::concept_graph::RepairConceptGraphRequest,
    ) -> Result<crate::concept_graph::ConceptGraphRecord, AppError> {
        let record = self.get_concept_graph(user_id, id).await?;
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("concept graph generation is not configured".into())
            })?;
        let graph = crate::concept_graph::repair_graph(completer.as_ref(), None, &record, &request)
            .await?;
        let mut updated = record;
        updated.graph = graph;
        let path = self.concept_graph_path(id)?;
        let json = serde_json::to_vec_pretty(&updated).map_err(|error| {
            AppError::Internal(format!("failed to serialize concept graph: {error}"))
        })?;
        tokio::fs::write(&path, json).await.map_err(|error| {
            AppError::Internal(format!("failed to store concept graph: {error}"))
        })?;
        Ok(updated)
    }

    // ── Draft store (the agent tool set's backing) ───────────────────────
    // In-memory only; `finish_concept_graph_draft` is the single publish
    // path to the JSON files above, gated by the deterministic audit.

    /// Snapshot one draft (drafts are small; cloning under the read lock is
    /// cheaper than holding the lock across any mutation).
    fn draft(&self, draft_id: &str) -> Result<DraftGraph, AppError> {
        self.concept_graph_drafts
            .read()
            .map_err(|_| AppError::Internal("concept graph draft lock poisoned".into()))?
            .get(draft_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("concept graph draft {draft_id}")))
    }

    /// Start a draft: resolve the scope reference first (best-effort — a
    /// failed scope analysis degrades to a scope-free draft), then register
    /// an empty graph the agent builds with `patch_concept_graph_draft`.
    pub async fn create_concept_graph_draft(
        &self,
        topic: &str,
        focus: Option<&str>,
        model_override: Option<(&ProviderId, &str)>,
    ) -> Result<crate::concept_graph::draft::DraftView, AppError> {
        let topic = topic.trim();
        if topic.is_empty() {
            return Err(AppError::BadRequest(
                "concept graph topic must not be empty".into(),
            ));
        }
        if topic.chars().count() > 200 {
            return Err(AppError::BadRequest("concept graph topic is too long".into()));
        }
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("concept graph generation is not configured".into())
            })?;
        let scope_topic = match focus {
            Some(focus) if !focus.trim().is_empty() => {
                format!("{topic}（用户补充：{}）", focus.trim())
            }
            _ => topic.to_owned(),
        };
        let scope = crate::concept_graph::analyze_scope(
            completer.as_ref(),
            model_override,
            &scope_topic,
            None,
        )
        .await;
        let draft = DraftGraph::new(topic.to_owned(), scope);
        let draft_id = LearningConceptGraphId::new().as_str().to_owned();
        let view = draft.view(&draft_id);
        self.concept_graph_drafts
            .write()
            .map_err(|_| AppError::Internal("concept graph draft lock poisoned".into()))?
            .insert(draft_id, draft);
        Ok(view)
    }

    /// Apply a batch of ops to a draft and return the per-op verdicts plus
    /// a fresh audit snapshot.
    pub fn patch_concept_graph_draft(
        &self,
        draft_id: &str,
        ops: Vec<crate::concept_graph::draft::GraphOp>,
    ) -> Result<crate::concept_graph::draft::PatchReport, AppError> {
        let mut draft = self.draft(draft_id)?;
        let report = draft.apply_ops(ops);
        self.concept_graph_drafts
            .write()
            .map_err(|_| AppError::Internal("concept graph draft lock poisoned".into()))?
            .insert(draft_id.to_owned(), draft);
        Ok(report)
    }

    /// Overview: sizes, workload, sub-domain distribution, entry/terminal
    /// units and the audit summary.
    pub fn inspect_concept_graph_draft(
        &self,
        draft_id: &str,
    ) -> Result<crate::concept_graph::draft::InspectView, AppError> {
        Ok(self.draft(draft_id)?.inspect())
    }

    /// Filtered unit list (name substring / sub-domain overlap / limit).
    pub fn query_concept_graph_draft(
        &self,
        draft_id: &str,
        filter: crate::concept_graph::draft::NodeQuery,
    ) -> Result<crate::concept_graph::draft::NodeListView, AppError> {
        Ok(self.draft(draft_id)?.query(&filter))
    }

    /// Ancestor/descendant closure around the given units.
    pub fn subgraph_concept_graph_draft(
        &self,
        draft_id: &str,
        nodes: Vec<String>,
        direction: crate::concept_graph::draft::SubgraphDirection,
        depth: Option<usize>,
    ) -> Result<crate::concept_graph::draft::SubgraphView, AppError> {
        Ok(self.draft(draft_id)?.subgraph(&nodes, direction, depth))
    }

    /// Full findings text plus the scope checklists and their live
    /// coverage — the repair loop's primary input. The audit is re-run
    /// live so the report never reflects a stale cached snapshot.
    pub fn audit_concept_graph_draft(&self, draft_id: &str) -> Result<String, AppError> {
        let mut draft = self.draft(draft_id)?;
        draft.refresh_audit();
        Ok(draft.audit_report())
    }

    /// The scope reference text (the generation loop's coverage
    /// checklist); `None` when no scope analysis ran for this draft.
    pub fn scope_reference_concept_graph_draft(
        &self,
        draft_id: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(self.draft(draft_id)?.scope_reference())
    }

    /// Publish a draft: the deterministic audit gate has the last word.
    /// Danger-grade findings block publishing (the draft survives, so the
    /// agent can keep repairing); a clean graph is written as a JSON record
    /// exactly like the legacy pipeline's output and the draft is removed.
    pub async fn finish_concept_graph_draft(
        &self,
        user_id: &UserId,
        draft_id: &str,
    ) -> Result<crate::concept_graph::ConceptGraphRecord, AppError> {
        // The finish gate re-runs the deterministic audit LIVE on the
        // draft's current state — the cached findings snapshot is never
        // trusted at the publish boundary, so a graph that was never
        // patched (or whose patches all failed) cannot slip through with
        // zero findings.
        let mut draft = self.draft(draft_id)?;
        draft.refresh_audit();
        if draft
            .graph
            .audit
            .findings
            .iter()
            .any(|finding| finding.severity == crate::concept_graph::SEV_DANGER)
        {
            return Err(AppError::UnprocessableEntity(format!(
                "concept graph draft still fails the audit gate:\n{}",
                draft.audit_report()
            )));
        }
        let dir = self.concept_graph_dir()?;
        tokio::fs::create_dir_all(&dir).await.map_err(|error| {
            AppError::Internal(format!("failed to create concept graph dir: {error}"))
        })?;
        let record = crate::concept_graph::ConceptGraphRecord {
            id: LearningConceptGraphId::new().as_str().to_owned(),
            user_id: user_id.as_str().to_owned(),
            topic: draft.topic,
            graph: draft.graph,
            created_at: now_ms(),
        };
        let path = dir.join(format!("{}.json", record.id));
        let json = serde_json::to_vec_pretty(&record).map_err(|error| {
            AppError::Internal(format!("failed to serialize concept graph: {error}"))
        })?;
        tokio::fs::write(&path, json).await.map_err(|error| {
            AppError::Internal(format!("failed to store concept graph: {error}"))
        })?;
        self.concept_graph_drafts
            .write()
            .map_err(|_| AppError::Internal("concept graph draft lock poisoned".into()))?
            .remove(draft_id);
        Ok(record)
    }

}
