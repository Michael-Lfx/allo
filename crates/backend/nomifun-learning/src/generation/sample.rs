use super::*;


/// Sample the knowledge base's markdown documents for course generation.
/// Shared by the synchronous pipeline and the background job runner so the
/// snapshot stored in `learning_course_jobs` has the same shape everywhere.
pub(crate) async fn sample_base_files(
    knowledge: &KnowledgeService,
    knowledge_base_id: &str,
) -> Result<Vec<(String, String)>, AppError> {
    // A local folder's public `root_path` is its read-only source location.
    // Course generation samples the app-managed Markdown projection so it
    // sees converted documents while the source remains available.
    let content_root = knowledge
        .content_root_for_base(knowledge_base_id)
        .await?;
    if !content_root.is_dir() {
        return Err(AppError::BadRequest(
            "selected knowledge base content directory does not exist".into(),
        ));
    }
    // Wider sampling than the knowledge-overview default: more files, larger
    // excerpts, higher total — the multi-stage pipeline has the budget to
    // read them and the lessons need richer grounding.
    let samples = autogen::sample_base_files_with_budget(
        &content_root,
        autogen::LEARNING_SAMPLE_MAX_FILES,
        autogen::LEARNING_SAMPLE_MAX_PER_FILE,
        autogen::LEARNING_SAMPLE_MAX_TOTAL,
    )
    .await;
    if samples.is_empty() {
        return Err(AppError::BadRequest(
            "knowledge base has no markdown documents to generate a course from".into(),
        ));
    }
    Ok(samples)
}

