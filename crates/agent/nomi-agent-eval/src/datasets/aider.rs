//! Aider Polyglot (Exercism Python) — coding-agent cases with a pytest oracle.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::types::{Case, CaseBudgets, Manifest, ScorerSpec, SCHEMA_VERSION};

use super::{
    clamp_limit, http_get_first_ok, write_cached_manifest, DatasetError, DEFAULT_DOWNLOAD_LIMIT,
};

pub const SUITE_AIDER_POLYGLOT: &str = "aider_polyglot";

pub const POLYGLOT_ZIP_URL: &str =
    "https://codeload.github.com/Aider-AI/polyglot-benchmark/zip/refs/heads/main";
const POLYGLOT_ZIP_URLS: &[&str] = &[
    POLYGLOT_ZIP_URL,
    "https://github.com/Aider-AI/polyglot-benchmark/archive/refs/heads/main.zip",
];
const POLYGLOT_ZIP_CACHE: &str = "aider-polyglot.zip";
const PRACTICE_MARKER: &str = "/python/exercises/practice/";

pub async fn load_aider_polyglot(
    cache_dir: &Path,
    limit: Option<usize>,
) -> Result<Manifest, DatasetError> {
    let limit = clamp_limit(limit.or(Some(DEFAULT_DOWNLOAD_LIMIT)));
    let cached = cache_dir.join(format!("aider_polyglot.limit{limit}.json"));
    if cached.exists() {
        return Ok(crate::corpus::load_manifest(&cached)?);
    }
    let zip_path = cache_dir.join(POLYGLOT_ZIP_CACHE);
    let bytes = if zip_path.exists() {
        std::fs::read(&zip_path)?
    } else {
        let bytes = http_get_first_ok(POLYGLOT_ZIP_URLS, 180).await?;
        if let Some(parent) = zip_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&zip_path, &bytes)?;
        bytes
    };
    let manifest = aider_zip_to_manifest(&bytes, limit)?;
    write_cached_manifest(&cached, &manifest)?;
    Ok(manifest)
}

/// Convert a polyglot-benchmark zip into coding-agent cases.
///
/// Canonical `.meta/example.py` solutions are dropped so the agent cannot copy them.
/// Unit tests stay in the workspace (Aider protocol: edit stub, run tests, iterate).
pub fn aider_zip_to_manifest(bytes: &[u8], limit: usize) -> Result<Manifest, DatasetError> {
    let exercises = collect_python_exercises(bytes)?;
    let mut cases = Vec::new();
    for exercise in exercises {
        if cases.len() >= limit {
            break;
        }
        if let Some(case) = exercise_to_case(exercise) {
            cases.push(case);
        }
    }
    if cases.is_empty() {
        return Err(DatasetError::Archive(
            "Aider polyglot zip produced no Python practice cases".into(),
        ));
    }
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        corpus_version: format!("aider-polyglot-python-{limit}"),
        suite: SUITE_AIDER_POLYGLOT.into(),
        cases,
    };
    crate::corpus::validate_manifest(&manifest)?;
    Ok(manifest)
}

struct Exercise {
    slug: String,
    files: BTreeMap<String, String>,
}

fn collect_python_exercises(bytes: &[u8]) -> Result<Vec<Exercise>, DatasetError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| DatasetError::Archive(e.to_string()))?;
    let mut by_slug: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| DatasetError::Archive(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let Some(relative) = practice_relative(&name) else {
            continue;
        };
        let Some((slug, rest)) = relative.split_once('/') else {
            continue;
        };
        if is_skipped_exercise_file(rest) {
            continue;
        }
        let mut contents = String::new();
        if entry.read_to_string(&mut contents).is_err() {
            continue;
        }
        by_slug
            .entry(slug.to_owned())
            .or_default()
            .insert(rest.to_owned(), contents);
    }
    Ok(by_slug
        .into_iter()
        .map(|(slug, files)| Exercise { slug, files })
        .collect())
}

fn practice_relative(name: &str) -> Option<&str> {
    let idx = name.find(PRACTICE_MARKER)?;
    let rest = &name[idx + PRACTICE_MARKER.len()..];
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

fn is_skipped_exercise_file(relative: &str) -> bool {
    let normalized = relative.replace('\\', "/");
    normalized == ".meta/example.py"
        || normalized.ends_with("/.meta/example.py")
        || normalized.contains("/__pycache__/")
        || normalized.ends_with(".pyc")
}

fn exercise_to_case(exercise: Exercise) -> Option<Case> {
    let tests: Vec<String> = exercise
        .files
        .keys()
        .filter(|path| is_test_file(path))
        .cloned()
        .collect();
    if tests.is_empty() {
        return None;
    }
    let stub = exercise.files.keys().find(|path| is_stub_file(path))?.clone();
    let instructions = exercise
        .files
        .iter()
        .filter(|(path, _)| path.replace('\\', "/").starts_with(".docs/") && path.ends_with(".md"))
        .map(|(_, body)| body.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let file_list = exercise
        .files
        .keys()
        .map(|path| format!("- `{path}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut pytest_args = vec!["-m".into(), "pytest".into()];
    pytest_args.extend(tests.iter().cloned());
    pytest_args.push("-q".into());
    let pytest_hint = format!("python -m pytest {} -q", tests.join(" "));
    let prompt = format!(
        "You are a coding agent in an isolated workspace. This is Exercism Python practice `{slug}` \
from the Aider polyglot benchmark.\n\n\
{instructions}\n\n\
Workspace files:\n{file_list}\n\n\
Implement the stub (`{stub}`) so the unit tests pass. You may read and run the tests. \
Do not copy a canonical example (none is provided). Prefer Edit/Write on the solution files; \
do not weaken the tests.\n\n\
You may verify with:\n`{pytest_hint}`\n",
        slug = exercise.slug,
        instructions = if instructions.trim().is_empty() {
            "Read the files in the workspace and implement the missing behavior.".to_owned()
        } else {
            instructions
        },
        file_list = file_list,
        stub = stub,
        pytest_hint = pytest_hint,
    );
    Some(Case {
        id: format!("aider-{}", exercise.slug),
        category: "aider_polyglot".into(),
        prompt,
        enabled: true,
            budgets: CaseBudgets {
                max_turns: Some(24),
                max_tokens: Some(65536),
            },
        scorers: vec![
            ScorerSpec::FileExists { path: stub },
            ScorerSpec::PythonModule { args: pytest_args },
        ],
        notes: Some(exercise.slug),
        task_profile: Some("coding".into()),
        workspace_files: exercise.files,
        timeout_secs: Some(600),
    })
}

fn is_test_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.ends_with("_test.py") || name.starts_with("test_") && name.ends_with(".py")
}

fn is_stub_file(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.ends_with(".py")
        && !is_test_file(&normalized)
        && !normalized.starts_with(".meta/")
        && !normalized.starts_with(".docs/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn sample_zip() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let files = [
            (
                "polyglot-benchmark-main/python/exercises/practice/wordy/.docs/instructions.md",
                "# Wordy\nParse and evaluate simple math word problems.\n",
            ),
            (
                "polyglot-benchmark-main/python/exercises/practice/wordy/wordy.py",
                "def answer(question):\n    pass\n",
            ),
            (
                "polyglot-benchmark-main/python/exercises/practice/wordy/wordy_test.py",
                "import unittest\nfrom wordy import answer\nclass T(unittest.TestCase):\n    def test_ok(self):\n        self.assertEqual(answer('What is 1 plus 1?'), 2)\n",
            ),
            (
                "polyglot-benchmark-main/python/exercises/practice/wordy/.meta/example.py",
                "def answer(question):\n    return 2\n",
            ),
        ];
        for (name, body) in files {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_adapter_hides_canonical_example() {
        let manifest = aider_zip_to_manifest(&sample_zip(), 8).unwrap();
        assert_eq!(manifest.suite, SUITE_AIDER_POLYGLOT);
        assert_eq!(manifest.cases.len(), 1);
        let case = &manifest.cases[0];
        assert_eq!(case.id, "aider-wordy");
        assert!(!case.workspace_files.contains_key(".meta/example.py"));
        assert!(case.workspace_files.contains_key("wordy.py"));
        assert!(case.workspace_files.contains_key("wordy_test.py"));
        assert!(case.prompt.contains("wordy.py"));
        assert!(matches!(
            case.scorers[1],
            ScorerSpec::PythonModule { .. }
        ));
    }
}
