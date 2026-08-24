use super::*;

impl LearningService {

    // ── Experimental concept graph (rough file persistence) ─────────────
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

    /// Decompose a learning goal into an atomic-concept prerequisite DAG via
    /// one model call (plus one validation retry) and persist it as JSON.
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
        let completer = self
            .course_completer
            .read()
            .map_err(|_| AppError::Internal("learning course completer lock poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Conflict("concept graph generation is not configured".into())
            })?;
        let model_override = request.provider_id.as_ref().zip(request.model.as_deref());
        let graph =
            crate::concept_graph::generate_concept_graph(completer.as_ref(), model_override, topic)
                .await?;
        let dir = self.concept_graph_dir()?;
        tokio::fs::create_dir_all(&dir).await.map_err(|error| {
            AppError::Internal(format!("failed to create concept graph dir: {error}"))
        })?;
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

}
