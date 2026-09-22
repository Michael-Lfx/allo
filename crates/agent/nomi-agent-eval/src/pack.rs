//! Import a folder-shaped business eval pack (`T0N_Prompt.txt` + `Input/`).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::corpus::{validate_manifest, CorpusError};
use crate::types::{
    is_imported_suite, Case, CaseBudgets, Manifest, ScorerSpec, SCHEMA_VERSION,
};
use crate::workspace::{copy_dir_contents, list_relative_files, safe_join};

const PREAMBLE: &str = "【评测工作区】当前工作目录即本题隔离工作区。请从相对路径读取 Input 下的资料（如有），并将指定产物写在工作区根目录。不要写入用户桌面。";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImportedPack {
    pub suite: String,
    pub title: String,
    pub cases: usize,
    pub pack_path: String,
}

pub fn packs_dir(data_dir: impl AsRef<Path>) -> PathBuf {
    data_dir.as_ref().join("diagnostics/agent-evals/packs")
}

pub fn pack_corpus_path(packs_root: impl AsRef<Path>, suite: &str) -> PathBuf {
    packs_root.as_ref().join(suite).join("corpus.json")
}

pub fn pack_case_dir(packs_root: impl AsRef<Path>, suite: &str, case_id: &str) -> PathBuf {
    packs_root.as_ref().join(suite).join("cases").join(case_id)
}

pub fn load_pack_manifest(
    packs_root: impl AsRef<Path>,
    suite: &str,
) -> Result<Manifest, CorpusError> {
    if !is_imported_suite(suite) {
        return Err(CorpusError::Invalid(format!(
            "not an imported pack suite: {suite}"
        )));
    }
    crate::corpus::load_manifest(pack_corpus_path(packs_root, suite))
}

pub fn list_imported_pack_suites(packs_root: impl AsRef<Path>) -> Vec<crate::datasets::SuiteDescriptor> {
    let root = packs_root.as_ref();
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut suites = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(suite) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_imported_suite(suite) {
            continue;
        }
        let Ok(manifest) = load_pack_manifest(root, suite) else {
            continue;
        };
        let count = manifest.cases.iter().filter(|c| c.enabled).count();
        suites.push(crate::datasets::imported_pack_descriptor(
            suite,
            pack_title(suite, &manifest),
            count,
        ));
    }
    suites.sort_by(|a, b| a.id.cmp(&b.id));
    suites
}

/// Copy a discovered pack into `{packs_root}/{suite}/` and write `corpus.json`.
pub fn import_business_pack(
    source_root: impl AsRef<Path>,
    packs_root: impl AsRef<Path>,
) -> Result<ImportedPack, CorpusError> {
    let source_root = source_root.as_ref();
    if !source_root.is_dir() {
        return Err(CorpusError::Invalid(format!(
            "pack root is not a directory: {}",
            source_root.display()
        )));
    }
    let discovered = discover_cases(source_root)?;
    if discovered.is_empty() {
        return Err(CorpusError::Invalid(
            "folder has no T0N_Prompt.txt cases (expected 01_Web_Research/T01_Prompt.txt, …)"
                .into(),
        ));
    }
    let suite = format!("imported-{}", sanitize_pack_slug(source_root));
    let pack_dir = packs_root.as_ref().join(&suite);
    if pack_dir.exists() {
        fs::remove_dir_all(&pack_dir)?;
    }
    fs::create_dir_all(&pack_dir)?;

    let mut cases = Vec::new();
    for item in discovered {
        let case_dir = pack_dir.join("cases").join(&item.id);
        fs::create_dir_all(&case_dir)?;
        let mut blobs = Vec::new();
        if let Some(input) = item.input_dir.as_ref() {
            let dest_input = case_dir.join("Input");
            copy_dir_contents(input, &dest_input)?;
            blobs = list_relative_files(&case_dir)?;
        }
        for relative in &blobs {
            safe_join(&case_dir, relative)?;
        }
        cases.push(build_case(&item, blobs));
    }

    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        corpus_version: format!("business-pack-{}", chrono::Utc::now().date_naive()),
        suite: suite.clone(),
        cases,
    };
    validate_manifest(&manifest)?;
    let corpus_path = pack_dir.join("corpus.json");
    fs::write(
        &corpus_path,
        serde_json::to_string_pretty(&manifest).map_err(CorpusError::from)?,
    )?;

    Ok(ImportedPack {
        suite: suite.clone(),
        title: pack_title(&suite, &manifest),
        cases: manifest.cases.len(),
        pack_path: pack_dir.to_string_lossy().into_owned(),
    })
}

struct DiscoveredCase {
    id: String,
    category: String,
    raw_prompt: String,
    input_dir: Option<PathBuf>,
}

fn discover_cases(root: &Path) -> Result<Vec<DiscoveredCase>, CorpusError> {
    let mut found = Vec::new();
    collect_prompt_files(root, 0, &mut found)?;
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found)
}

fn collect_prompt_files(
    dir: &Path,
    depth: usize,
    out: &mut Vec<DiscoveredCase>,
) -> Result<(), CorpusError> {
    if depth > 3 {
        return Ok(());
    }
    let entries = fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_prompt_files(&path, depth + 1, out)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id) = prompt_case_id(name) else {
            continue;
        };
        if out.iter().any(|c| c.id == id) {
            continue;
        }
        let raw_prompt = fs::read_to_string(&path)?;
        if raw_prompt.trim().is_empty() {
            return Err(CorpusError::Invalid(format!(
                "{name} is empty"
            )));
        }
        let parent = path.parent().unwrap_or(dir);
        let input_dir = parent.join("Input");
        out.push(DiscoveredCase {
            id,
            category: category_for_parent(parent),
            raw_prompt,
            input_dir: input_dir.is_dir().then_some(input_dir),
        });
    }
    Ok(())
}

fn prompt_case_id(file_name: &str) -> Option<String> {
    let lower = file_name.to_ascii_lowercase();
    let stem = lower.strip_suffix("_prompt.txt")?;
    let digits = stem.strip_prefix('t')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("t{digits}"))
}

fn category_for_parent(parent: &Path) -> String {
    let name = parent
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.contains("web") || name.contains("research") {
        "web_research".into()
    } else if name.contains("project") || name.contains("consolidat") {
        "project_consolidation".into()
    } else if name.contains("data") || name.contains("analys") {
        "data_analysis".into()
    } else {
        "business".into()
    }
}

fn build_case(item: &DiscoveredCase, blobs: Vec<String>) -> Case {
    let prompt = rewrite_prompt(&item.raw_prompt);
    let (scorers, advisory, timeout_secs, max_turns) = match item.id.as_str() {
        "t01" => t01_scorers(),
        "t02" => t02_scorers(),
        "t03" => t03_scorers(),
        _ => generic_scorers(&prompt),
    };
    Case {
        id: item.id.clone(),
        category: item.category.clone(),
        prompt,
        enabled: true,
        budgets: CaseBudgets {
            max_turns: Some(max_turns),
            max_tokens: None,
        },
        scorers,
        advisory_scorers: advisory,
        isolation: Some("business".into()),
        trial: 0,
        notes: Some("imported business pack".into()),
        task_profile: Some("office".into()),
        workspace_files: Default::default(),
        workspace_blobs: blobs,
        timeout_secs: Some(timeout_secs),
    }
}

fn t01_scorers() -> (Vec<ScorerSpec>, Vec<ScorerSpec>, u64, u32) {
    (
        vec![
            ScorerSpec::FileExists {
                path: "AI_PC_Procurement_Brief.md".into(),
            },
            ScorerSpec::KeywordCoverage {
                keywords: vec![
                    "ASUS".into(),
                    "Dell".into(),
                    "HP".into(),
                    "Lenovo".into(),
                ],
                path: "AI_PC_Procurement_Brief.md".into(),
                minimum: 4,
            },
            ScorerSpec::FileRegex {
                path: "AI_PC_Procurement_Brief.md".into(),
                pattern: r"https?://|來源|来源".into(),
                minimum_hits: 1,
            },
        ],
        vec![ScorerSpec::ToolCalled {
            name: "web_search".into(),
        }],
        2700,
        48,
    )
}

fn t02_scorers() -> (Vec<ScorerSpec>, Vec<ScorerSpec>, u64, u32) {
    (
        vec![
            ScorerSpec::FileExists {
                path: "Project_Summary.md".into(),
            },
            ScorerSpec::FileExists {
                path: "Action_Items.xlsx".into(),
            },
            ScorerSpec::XlsxHeaders {
                path: "Action_Items.xlsx".into(),
                headers: vec![
                    "Priority".into(),
                    "Action Item".into(),
                    "Owner".into(),
                    "Due Date".into(),
                    "Related Issue".into(),
                    "Status".into(),
                    "Source".into(),
                ],
                sheet: None,
            },
            ScorerSpec::KeywordCoverage {
                keywords: vec![
                    "Issue".into(),
                    "Risk".into(),
                    "風險".into(),
                    "风险".into(),
                    "需求".into(),
                ],
                path: "Project_Summary.md".into(),
                minimum: 2,
            },
        ],
        vec![ScorerSpec::KeywordCoverage {
            keywords: vec!["衝突".into(), "冲突".into(), "conflict".into(), "不一致".into()],
            path: "Project_Summary.md".into(),
            minimum: 1,
        }],
        2700,
        48,
    )
}

fn t03_scorers() -> (Vec<ScorerSpec>, Vec<ScorerSpec>, u64, u32) {
    (
        vec![
            ScorerSpec::FileExists {
                path: "Management_Summary.md".into(),
            },
            ScorerSpec::FileExists {
                path: "Management_Summary.xlsx".into(),
            },
            ScorerSpec::XlsxSheets {
                path: "Management_Summary.xlsx".into(),
                names: vec![
                    "Region Summary".into(),
                    "Product Summary".into(),
                    "Key Findings".into(),
                ],
            },
        ],
        vec![
            ScorerSpec::KeywordCoverage {
                keywords: vec![
                    "Region".into(),
                    "Product".into(),
                    "Revenue".into(),
                    "區域".into(),
                    "区域".into(),
                    "产品".into(),
                    "產品".into(),
                ],
                path: "Management_Summary.md".into(),
                minimum: 2,
            },
            ScorerSpec::XlsxTotalsClose {
                source: "Input/Sales_Result.xlsx".into(),
                output: "Management_Summary.xlsx".into(),
                tolerance: 0.01,
            },
        ],
        1800,
        40,
    )
}

fn generic_scorers(prompt: &str) -> (Vec<ScorerSpec>, Vec<ScorerSpec>, u64, u32) {
    let scorers = extract_output_names(prompt)
        .into_iter()
        .map(|path| ScorerSpec::FileExists { path })
        .collect::<Vec<_>>();
    let scorers = if scorers.is_empty() {
        vec![ScorerSpec::MaxTurns { max: 64 }]
    } else {
        scorers
    };
    (scorers, Vec::new(), 1800, 40)
}

fn extract_output_names(prompt: &str) -> Vec<String> {
    let mut names = Vec::new();
    for token in prompt.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| c == '，' || c == '。' || c == ',' || c == '.');
        if cleaned.ends_with(".md") || cleaned.ends_with(".xlsx") {
            if let Some(name) = Path::new(cleaned).file_name().and_then(|n| n.to_str()) {
                if !names.iter().any(|existing: &String| existing == name) {
                    names.push(name.to_owned());
                }
            }
        }
    }
    names
}

pub fn rewrite_prompt(raw: &str) -> String {
    let mut text = raw.replace('\r', "");
    let replacements = [
        (
            "AI_Agent_Business_Benchmark_v1\\02_Project_Consolidation\\Input",
            "Input",
        ),
        (
            "AI_Agent_Business_Benchmark_v1/02_Project_Consolidation/Input",
            "Input",
        ),
        (
            "AI_Agent_Business_Benchmark_v1\\03_Data_Analysis\\Input\\Sales_Result.xlsx",
            "Input/Sales_Result.xlsx",
        ),
        (
            "AI_Agent_Business_Benchmark_v1/03_Data_Analysis/Input/Sales_Result.xlsx",
            "Input/Sales_Result.xlsx",
        ),
        ("將最終結果另存為桌面的 ", "將最終結果寫入工作區根目錄的 "),
        ("将最终结果另存为桌面的 ", "将最终结果写入工作区根目录的 "),
        ("另存為桌面的 ", "寫入工作區根目錄的 "),
        ("另存为桌面的 ", "写入工作区根目录的 "),
        ("A. 桌面 ", "A. 工作區根目錄 "),
        ("B. 桌面 ", "B. 工作區根目錄 "),
        ("請分析桌面 ", "請分析工作區 "),
        ("请分析桌面 ", "请分析工作区 "),
    ];
    for (from, to) in replacements {
        text = text.replace(from, to);
    }
    format!("{PREAMBLE}\n\n{text}")
}

fn sanitize_pack_slug(root: &Path) -> String {
    let raw = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("business-pack");
    let mut slug: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        "business-pack".into()
    } else {
        slug
    }
}

fn pack_title(suite: &str, manifest: &Manifest) -> String {
    let label = suite
        .strip_prefix("imported-")
        .unwrap_or(suite)
        .replace('-', " ");
    format!("Business pack · {label} ({} cases)", manifest.cases.len())
}

pub fn copy_imported_case_files(
    packs_root: impl AsRef<Path>,
    suite: &str,
    case_id: &str,
    workspace: &Path,
) -> Result<(), CorpusError> {
    if !is_imported_suite(suite) {
        return Ok(());
    }
    let src = pack_case_dir(packs_root, suite, case_id);
    if !src.is_dir() {
        return Ok(());
    }
    copy_dir_contents(&src, workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rewrite_maps_desktop_paths() {
        let rewritten = rewrite_prompt(
            "請分析桌面 AI_Agent_Business_Benchmark_v1\\02_Project_Consolidation\\Input 資料夾\nA. 桌面 Project_Summary.md",
        );
        assert!(rewritten.contains(PREAMBLE));
        assert!(rewritten.contains("請分析工作區 Input"));
        assert!(rewritten.contains("A. 工作區根目錄 Project_Summary.md"));
        assert!(!rewritten.contains("桌面 Project"));
    }

    #[test]
    fn import_discovers_three_tasks() {
        let src = tempdir().unwrap();
        let root = src.path();
        fs::create_dir_all(root.join("01_Web_Research")).unwrap();
        fs::write(
            root.join("01_Web_Research/T01_Prompt.txt"),
            "Write AI_PC_Procurement_Brief.md",
        )
        .unwrap();
        fs::create_dir_all(root.join("02_Project_Consolidation/Input")).unwrap();
        fs::write(
            root.join("02_Project_Consolidation/T02_Prompt.txt"),
            "Read Input and write Project_Summary.md",
        )
        .unwrap();
        fs::write(
            root.join("02_Project_Consolidation/Input/notes.docx"),
            b"PK\x03\x04fake",
        )
        .unwrap();
        fs::create_dir_all(root.join("03_Data_Analysis/Input")).unwrap();
        fs::write(
            root.join("03_Data_Analysis/T03_Prompt.txt"),
            "Analyze Input/Sales_Result.xlsx",
        )
        .unwrap();
        fs::write(root.join("03_Data_Analysis/Input/Sales_Result.xlsx"), b"PK").unwrap();

        let dest = tempdir().unwrap();
        let imported = import_business_pack(root, dest.path()).unwrap();
        assert!(imported.suite.starts_with("imported-"));
        assert_eq!(imported.cases, 3);
        let manifest = load_pack_manifest(dest.path(), &imported.suite).unwrap();
        assert_eq!(manifest.cases.len(), 3);
        assert!(manifest.cases.iter().all(|c| c.isolation.as_deref() == Some("business")));
        let t02 = manifest.cases.iter().find(|c| c.id == "t02").unwrap();
        assert!(t02.workspace_blobs.iter().any(|p| p.replace('\\', "/").contains("Input/notes.docx")));
        assert!(t02
            .scorers
            .iter()
            .any(|s| matches!(s, ScorerSpec::KeywordCoverage { path, .. } if path == "Project_Summary.md")));
        assert!(t02
            .advisory_scorers
            .iter()
            .any(|s| matches!(s, ScorerSpec::KeywordCoverage { .. })));
        let t03 = manifest.cases.iter().find(|c| c.id == "t03").unwrap();
        assert!(t03
            .advisory_scorers
            .iter()
            .any(|s| matches!(s, ScorerSpec::XlsxTotalsClose { .. })));
        let t01 = manifest.cases.iter().find(|c| c.id == "t01").unwrap();
        assert!(t01.scorers.iter().any(|s| matches!(s, ScorerSpec::FileRegex { .. })));
        assert!(t01
            .advisory_scorers
            .iter()
            .any(|s| matches!(s, ScorerSpec::ToolCalled { .. })));
        assert!(dest
            .path()
            .join(&imported.suite)
            .join("cases/t02/Input/notes.docx")
            .is_file());
        assert_eq!(
            crate::types::IsolationKind::resolve(t02.isolation.as_deref(), &imported.suite),
            crate::types::IsolationKind::Business
        );
    }
}
