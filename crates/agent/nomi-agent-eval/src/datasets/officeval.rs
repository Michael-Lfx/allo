//! OmegaUse-OfficeVal — long-horizon office-suite tasks from Hugging Face.
//!
//! Official scoring uses the Python verifiers in
//! [baidu-frontier-research/OmegaUse-OfficeVal](https://github.com/baidu-frontier-research/OmegaUse-OfficeVal)
//! over a 100-task ZIP. This adapter only runs a local subset: it copies input
//! artifacts into an isolated workspace and applies a deliverable-attempt gate.
//! Do not publish the local pass rate as an official OfficeVal score.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::corpus::CorpusError;
use crate::types::{Case, CaseBudgets, Manifest, ScorerSpec, SCHEMA_VERSION};
use crate::workspace::{copy_dir_contents, safe_join};

use super::{clamp_limit, http_get_first_ok, write_cached_manifest, DatasetError};

pub const SUITE_OFFICEVAL: &str = "omegause_officeval";

pub const OFFICEVAL_DATASET_URL: &str =
    "https://huggingface.co/datasets/baidu-frontier-research/OmegaUse-OfficeVal";

const TASK_URL: &str = "https://huggingface.co/datasets/baidu-frontier-research/OmegaUse-OfficeVal/resolve/main/task-en";
const TASK_URL_MIRROR: &str = "https://hf-mirror.com/datasets/baidu-frontier-research/OmegaUse-OfficeVal/resolve/main/task-en";

const FILES_DIR: &str = "omegause_officeval";
const DEFAULT_TIMEOUT_SECS: u64 = 2700;
const DEFAULT_MAX_TURNS: u32 = 48;
const DEFAULT_CASE_LIMIT: usize = 4;

#[derive(Debug, Deserialize)]
struct OfficeValTask {
    id: String,
    instruction: String,
    #[serde(default)]
    operation_intent: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    human_labor_time: Option<u64>,
    #[serde(default)]
    origin_files: Vec<OfficeValOriginFile>,
}

#[derive(Debug, Deserialize)]
struct OfficeValOriginFile {
    #[serde(default)]
    url: String,
    #[serde(default)]
    dest: String,
}

pub async fn load_officeval(
    cache_dir: &Path,
    limit: Option<usize>,
) -> Result<Manifest, DatasetError> {
    let limit = clamp_limit(limit.or(Some(DEFAULT_CASE_LIMIT)));
    let cached = cache_dir.join(format!("omegause_officeval.limit{limit}.json"));
    if cached.exists() {
        return Ok(crate::corpus::load_manifest(&cached)?);
    }
    let mut cases = Vec::new();
    for index in 1..=limit {
        let id = format!("officeval_{index:03}");
        let task = load_task_json(cache_dir, &id).await?;
        let files_root = case_files_dir(cache_dir, &id);
        download_origin_files(&task, &files_root).await?;
        if let Some(case) = task_to_case(&task, &files_root)? {
            cases.push(case);
        }
    }
    if cases.is_empty() {
        return Err(DatasetError::EmptySuite(SUITE_OFFICEVAL.into()));
    }
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        corpus_version: format!("omegause-officeval-{limit}"),
        suite: SUITE_OFFICEVAL.into(),
        cases,
    };
    crate::corpus::validate_manifest(&manifest)?;
    write_cached_manifest(&cached, &manifest)?;
    Ok(manifest)
}

/// Copy cached Input artifacts for one OfficeVal case into the live workspace.
pub fn copy_officeval_case_files(
    cache_dir: &Path,
    case_id: &str,
    workspace: &Path,
) -> Result<(), CorpusError> {
    let src = case_files_dir(cache_dir, case_id);
    if !src.is_dir() {
        return Err(CorpusError::Invalid(format!(
            "OmegaUse-OfficeVal inputs missing for {case_id}; download the dataset first"
        )));
    }
    copy_dir_contents(&src, workspace)
}

pub fn officeval_json_to_case(json: &str) -> Result<Case, DatasetError> {
    let task: OfficeValTask = serde_json::from_str(json)?;
    task_to_case(&task, Path::new("."))?
        .ok_or_else(|| DatasetError::EmptySuite(SUITE_OFFICEVAL.into()))
}

async fn load_task_json(cache_dir: &Path, id: &str) -> Result<OfficeValTask, DatasetError> {
    let path = cache_dir.join(FILES_DIR).join("tasks").join(format!("{id}.json"));
    let text = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        let primary = format!("{TASK_URL}/{id}.json");
        let mirror = format!("{TASK_URL_MIRROR}/{id}.json");
        let bytes = http_get_first_ok(&[&primary, &mirror], 120).await?;
        let text = String::from_utf8(bytes).map_err(|e| DatasetError::Download {
            url: primary,
            message: format!("invalid utf-8: {e}"),
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &text)?;
        text
    };
    Ok(serde_json::from_str(&text)?)
}

async fn download_origin_files(
    task: &OfficeValTask,
    files_root: &Path,
) -> Result<(), DatasetError> {
    std::fs::create_dir_all(files_root.join("Input"))?;
    for file in &task.origin_files {
        let Some(name) = safe_dest_name(&file.dest) else {
            continue;
        };
        let dest = files_root.join("Input").join(&name);
        if dest.exists() {
            continue;
        }
        if file.url.trim().is_empty() {
            continue;
        }
        let urls = mirror_urls(&file.url);
        let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
        let bytes = http_get_first_ok(&refs, 180).await?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, bytes)?;
    }
    Ok(())
}

fn task_to_case(task: &OfficeValTask, files_root: &Path) -> Result<Option<Case>, DatasetError> {
    let id = task.id.trim();
    if id.is_empty() || task.instruction.trim().is_empty() {
        return Ok(None);
    }
    let mut input_names = Vec::new();
    let mut blobs = Vec::new();
    for file in &task.origin_files {
        let Some(name) = safe_dest_name(&file.dest) else {
            continue;
        };
        let relative = format!("Input/{name}");
        safe_join(Path::new("."), &relative).map_err(|e| {
            DatasetError::Corpus(CorpusError::Invalid(format!("unsafe dest {relative}: {e}")))
        })?;
        input_names.push(name);
        blobs.push(relative);
    }
    if files_root.exists() {
        if let Ok(entries) = std::fs::read_dir(files_root.join("Input")) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let relative = format!("Input/{name}");
                if !blobs.iter().any(|item| item == &relative) {
                    blobs.push(relative);
                    input_names.push(name);
                }
            }
        }
    }
    input_names.sort();
    blobs.sort();
    let file_list = if input_names.is_empty() {
        "(no input files listed)".into()
    } else {
        input_names
            .iter()
            .map(|name| format!("- Input/{name}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let domain = task.domain.trim();
    let intent = task.operation_intent.trim();
    let labor = task
        .human_labor_time
        .map(|minutes| format!(" Human labor time for this task is about {minutes} minutes."))
        .unwrap_or_default();
    let prompt = format!(
        "You are an office assistant. The eval workspace already contains the input artifacts.\n\
Input files:\n{file_list}\n\n\
{instruction}\n\n\
Write or edit the required Office deliverable (Word / Excel / PowerPoint / PDF). \
You may edit files under Input/ in place, or write new files at the workspace root. \
Do not write test files or copy the inputs elsewhere just to rename them.\
{labor}\n\
Domain: {domain}. Operation: {intent}.",
        instruction = task.instruction.trim(),
    );
    let extensions = expected_extensions(&task.instruction);
    Ok(Some(Case {
        id: id.replace('/', "-").replace(' ', "_"),
        category: if domain.is_empty() {
            "officeval".into()
        } else {
            domain.to_owned()
        },
        prompt,
        enabled: true,
        budgets: CaseBudgets {
            max_turns: Some(DEFAULT_MAX_TURNS),
            max_tokens: Some(65536),
        },
        scorers: vec![ScorerSpec::OfficeDeliverable {
            extensions,
        }],
        notes: Some(format!(
            "OmegaUse-OfficeVal {id}. Local deliverable-attempt gate only — not an official score."
        )),
        task_profile: Some("office".into()),
        workspace_files: Default::default(),
        workspace_blobs: blobs,
        timeout_secs: Some(DEFAULT_TIMEOUT_SECS),
        advisory_scorers: vec![],
        isolation: Some("officeval".into()),
        trial: 0,
    }))
}

fn case_files_dir(cache_dir: &Path, case_id: &str) -> PathBuf {
    cache_dir.join(FILES_DIR).join("files").join(case_id)
}

fn safe_dest_name(dest: &str) -> Option<String> {
    let dest = dest.trim();
    if dest.is_empty() {
        return None;
    }
    let path = Path::new(dest);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let name = path.file_name()?.to_str()?.trim();
    if name.is_empty() || name == "." || name.contains("..") {
        return None;
    }
    Some(name.to_owned())
}

fn mirror_urls(url: &str) -> Vec<String> {
    let mut urls = vec![url.to_owned()];
    if let Some(rest) = url.strip_prefix("https://huggingface.co/") {
        urls.push(format!("https://hf-mirror.com/{rest}"));
    }
    urls
}

fn expected_extensions(instruction: &str) -> Vec<String> {
    let lower = instruction.to_ascii_lowercase();
    let mut extensions = Vec::new();
    if lower.contains("word") || lower.contains("docx") || lower.contains(".doc") {
        extensions.extend(["docx".into(), "doc".into()]);
    }
    if lower.contains("excel")
        || lower.contains("spreadsheet")
        || lower.contains("xlsx")
        || lower.contains("workbook")
    {
        extensions.extend(["xlsx".into(), "xlsm".into()]);
    }
    if lower.contains("powerpoint")
        || lower.contains("pptx")
        || lower.contains("presentation")
        || lower.contains("slides")
    {
        extensions.push("pptx".into());
    }
    if lower.contains("pdf") {
        extensions.push("pdf".into());
    }
    if extensions.is_empty() {
        vec![
            "docx".into(),
            "xlsx".into(),
            "xlsm".into(),
            "pptx".into(),
            "pdf".into(),
        ]
    } else {
        extensions.sort();
        extensions.dedup();
        extensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TurnTranscript;
    use tempfile::tempdir;

    const SAMPLE: &str = r#"{
        "id": "officeval_001",
        "instruction": "Modify the provided Word documents according to the following requirements:\n1. Delete unused sections.\n2. Keep the headings.",
        "operation_intent": "Restructure",
        "domain": "Education & Examination",
        "human_labor_time": 204,
        "task_price_proxy": 50,
        "price_source": "estimated_price",
        "origin_files": [
            {
                "url": "https://huggingface.co/datasets/baidu-frontier-research/OmegaUse-OfficeVal/resolve/main/task_files/officeval_001/plan.docx",
                "dest": "plan.docx"
            }
        ]
    }"#;

    #[test]
    fn adapter_builds_office_case_without_claiming_official_score() {
        let case = officeval_json_to_case(SAMPLE).unwrap();
        assert_eq!(case.id, "officeval_001");
        assert_eq!(case.task_profile.as_deref(), Some("office"));
        assert_eq!(case.isolation.as_deref(), Some("officeval"));
        assert_eq!(case.timeout_secs, Some(DEFAULT_TIMEOUT_SECS));
        assert!(case.prompt.contains("Input/plan.docx"));
        assert!(case.prompt.contains("Modify the provided Word documents"));
        assert!(case.workspace_blobs.iter().any(|p| p == "Input/plan.docx"));
        match &case.scorers[0] {
            ScorerSpec::OfficeDeliverable { extensions } => {
                assert!(extensions.iter().any(|e| e == "docx"));
            }
            other => panic!("unexpected scorer {other:?}"),
        }
        assert!(case
            .notes
            .as_deref()
            .unwrap_or("")
            .contains("not an official score"));
    }

    #[test]
    fn deliverable_gate_rejects_untouched_inputs() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Input")).unwrap();
        std::fs::write(dir.path().join("Input/plan.docx"), b"PK\x03\x04orig").unwrap();
        let t = TurnTranscript {
            workspace: Some(dir.path().to_path_buf()),
            ..TurnTranscript::default()
        };
        let result = crate::scorer::score_one(
            &ScorerSpec::OfficeDeliverable {
                extensions: vec!["docx".into()],
            },
            &t,
        );
        assert!(!result.passed, "{result:?}");
    }

    #[test]
    fn deliverable_gate_accepts_in_place_edit() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Input")).unwrap();
        std::fs::write(dir.path().join("Input/plan.docx"), b"PK\x03\x04edited").unwrap();
        let t = TurnTranscript {
            workspace: Some(dir.path().to_path_buf()),
            tool_names: vec!["Edit".into()],
            ..TurnTranscript::default()
        };
        let result = crate::scorer::score_one(
            &ScorerSpec::OfficeDeliverable {
                extensions: vec!["docx".into()],
            },
            &t,
        );
        assert!(result.passed, "{result:?}");
    }

    #[test]
    fn dest_name_rejects_traversal() {
        assert!(safe_dest_name("../secret.docx").is_none());
        assert!(safe_dest_name("Input/../secret.docx").is_none());
        assert_eq!(safe_dest_name("Input/plan.docx").as_deref(), Some("plan.docx"));
    }
}
