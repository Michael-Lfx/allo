use std::path::Path;

use super::*;


/// Sampling budget: at most this many files feed the course-generation
/// prompts. Wider than the knowledge-overview default (more files, larger
/// excerpts, higher total): the multi-stage pipeline has the budget to read
/// them and the lessons need richer grounding. The whole sampled corpus is
/// the INPUT side of the context-window equation — the output budgets live
/// in [`crate::generation::completer`].
pub(crate) const LEARNING_SAMPLE_MAX_FILES: usize = 40;
pub(crate) const LEARNING_SAMPLE_MAX_PER_FILE: usize = 8 * 1024;
pub(crate) const LEARNING_SAMPLE_MAX_TOTAL: usize = 160 * 1024;

/// Sample the markdown corpus under `root` with explicit budgets. Owned by
/// the learning pipeline (not the knowledge autogen module): course
/// generation is the only caller, and keeping the budget constants next to
/// the pipeline keeps the input-size contract local.
pub(crate) async fn sample_base_files_with_budget(
    root: &Path,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> Vec<(String, String)> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        sample_base_files_blocking(&root, max_files, max_per_file, max_total)
    })
    .await
    .unwrap_or_default()
}

fn sample_base_files_blocking(
    root: &Path,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> Vec<(String, String)> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut rels: Vec<String> = walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && is_md(e.path()))
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
            (rel != "README.md").then_some(rel)
        })
        .collect();
    rels.sort();

    let mut samples = Vec::new();
    let mut total = 0usize;
    for rel in rels.into_iter().take(max_files) {
        if total >= max_total {
            break;
        }
        let budget = max_per_file.min(max_total - total);
        let Some(excerpt) = read_prefix_lossy(&root.join(&rel), budget) else {
            continue;
        };
        if excerpt.trim().is_empty() {
            continue;
        }
        total += excerpt.len();
        samples.push((rel, excerpt));
    }
    samples
}

/// Read at most `limit` bytes from the start of `path`, lossily decoded
/// (a multi-byte char cut at the boundary degrades to U+FFFD, never a panic).
fn read_prefix_lossy(path: &Path, limit: usize) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; limit];
    let mut read = 0usize;
    loop {
        match file.read(&mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => return None,
        }
        if read == buf.len() {
            break;
        }
    }
    Some(String::from_utf8_lossy(&buf[..read]).into_owned())
}

fn is_md(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}


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
    let samples = sample_base_files_with_budget(
        &content_root,
        LEARNING_SAMPLE_MAX_FILES,
        LEARNING_SAMPLE_MAX_PER_FILE,
        LEARNING_SAMPLE_MAX_TOTAL,
    )
    .await;
    if samples.is_empty() {
        return Err(AppError::BadRequest(
            "knowledge base has no markdown documents to generate a course from".into(),
        ));
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sampling_skips_readme_and_caps_budgets() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), "# old readme").unwrap();
        for i in 0..25 {
            std::fs::write(root.join(format!("f{i:02}.md")), format!("# 文件 {i}\n正文")).unwrap();
        }
        std::fs::write(root.join("big.md"), "x".repeat(LEARNING_SAMPLE_MAX_PER_FILE * 2)).unwrap();

        // Explicit 20-file budget: the corpus has 26 non-README files, so the
        // cap (not the corpus) decides the sample count.
        let samples = sample_base_files_with_budget(root, 20, LEARNING_SAMPLE_MAX_PER_FILE, LEARNING_SAMPLE_MAX_TOTAL)
            .await;
        assert_eq!(samples.len(), 20, "{:?}", samples.iter().map(|s| &s.0).collect::<Vec<_>>());
        assert!(samples.iter().all(|(rel, _)| rel != "README.md"));
        let big = samples.iter().find(|(rel, _)| rel == "big.md").expect("big.md sampled (sorted first)");
        assert!(big.1.len() <= LEARNING_SAMPLE_MAX_PER_FILE);
        let total: usize = samples.iter().map(|(_, s)| s.len()).sum();
        assert!(total <= LEARNING_SAMPLE_MAX_TOTAL);
    }

    #[tokio::test]
    async fn sampling_with_learning_budget_keeps_larger_excerpts_under_caps() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join("big.md"), "x".repeat(LEARNING_SAMPLE_MAX_PER_FILE * 2)).unwrap();
        std::fs::write(root.join("other.md"), "# 正文\n内容").unwrap();

        let samples = sample_base_files_with_budget(
            root,
            LEARNING_SAMPLE_MAX_FILES,
            LEARNING_SAMPLE_MAX_PER_FILE,
            LEARNING_SAMPLE_MAX_TOTAL,
        )
        .await;
        let total: usize = samples.iter().map(|(_, s)| s.len()).sum();
        assert!(total <= LEARNING_SAMPLE_MAX_TOTAL);
        let big = samples.iter().find(|(rel, _)| rel == "big.md").expect("big.md sampled");
        assert!(big.1.len() <= LEARNING_SAMPLE_MAX_PER_FILE);
    }

    #[tokio::test]
    async fn sampling_empty_or_missing_root() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(sample_base_files_with_budget(dir.path(), 1, 1, 1).await.is_empty());
        assert!(sample_base_files_with_budget(&dir.path().join("nope"), 1, 1, 1).await.is_empty());
    }
}
