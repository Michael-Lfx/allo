//! ClassEval — class-level Python coding with a hidden unittest oracle.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::types::{Case, CaseBudgets, Manifest, ScorerSpec, SCHEMA_VERSION};

use super::{
    clamp_limit, http_get_first_ok, write_cached_manifest, DatasetError, DEFAULT_DOWNLOAD_LIMIT,
};

pub const SUITE_CLASSEVAL: &str = "classeval";

pub const CLASSEVAL_URL: &str =
    "https://raw.githubusercontent.com/FudanSELab/ClassEval/master/data/ClassEval_data.json";
const CLASSEVAL_URLS: &[&str] = &[
    CLASSEVAL_URL,
    "https://cdn.jsdelivr.net/gh/FudanSELab/ClassEval@master/data/ClassEval_data.json",
];
const CLASSEVAL_CACHE: &str = "classeval.json";

pub async fn load_classeval(
    cache_dir: &Path,
    limit: Option<usize>,
) -> Result<Manifest, DatasetError> {
    let limit = clamp_limit(limit.or(Some(DEFAULT_DOWNLOAD_LIMIT)));
    let cached = cache_dir.join(format!("classeval.limit{limit}.json"));
    if cached.exists() {
        return Ok(crate::corpus::load_manifest(&cached)?);
    }
    let json_path = cache_dir.join(CLASSEVAL_CACHE);
    let text = if json_path.exists() {
        std::fs::read_to_string(&json_path)?
    } else {
        let bytes = http_get_first_ok(CLASSEVAL_URLS, 120).await?;
        let text = String::from_utf8(bytes).map_err(|e| DatasetError::Download {
            url: CLASSEVAL_URL.into(),
            message: format!("invalid utf-8: {e}"),
        })?;
        if let Some(parent) = json_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&json_path, &text)?;
        text
    };
    let manifest = classeval_json_to_manifest(&text, limit)?;
    write_cached_manifest(&cached, &manifest)?;
    Ok(manifest)
}

#[derive(Debug, Deserialize)]
struct ClassEvalRow {
    task_id: String,
    skeleton: String,
    test: String,
    #[serde(default)]
    import_statement: Vec<String>,
    class_name: String,
}

pub fn classeval_json_to_manifest(json: &str, limit: usize) -> Result<Manifest, DatasetError> {
    let rows: Vec<ClassEvalRow> = serde_json::from_str(json)?;
    let mut cases = Vec::new();
    for row in rows {
        if cases.len() >= limit {
            break;
        }
        if row.skeleton.trim().is_empty()
            || row.test.trim().is_empty()
            || row.class_name.trim().is_empty()
        {
            continue;
        }
        let id = row.task_id.replace('/', "-").replace(' ', "_");
        let mut workspace_files = BTreeMap::new();
        workspace_files.insert("solution.py".into(), row.skeleton.clone());
        let prompt = format!(
            "Complete the Python class `{class}` in solution.py. Fill every method so the hidden \
unit tests pass. The skeleton is already in the workspace — edit it in place. Do not write test \
files. Do not rename the class.\n\n\
```python\n{skeleton}\n```\n",
            class = row.class_name,
            skeleton = row.skeleton.trim_end()
        );
        cases.push(Case {
            id,
            category: "classeval".into(),
            prompt,
            enabled: true,
            budgets: CaseBudgets {
                max_turns: Some(20),
                max_tokens: Some(65536),
            },
            scorers: vec![ScorerSpec::PythonHiddenCheck {
                entry_point: "_module".into(),
                test: hidden_unittest_check(&row.test, &row.import_statement),
            }],
            notes: Some(row.class_name),
            task_profile: Some("coding".into()),
            workspace_files,
            timeout_secs: Some(300),
        });
    }
    if cases.is_empty() {
        return Err(DatasetError::Corpus(crate::corpus::CorpusError::Invalid(
            "ClassEval JSON produced no cases".into(),
        )));
    }
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        corpus_version: format!("classeval-fudan-{limit}"),
        suite: SUITE_CLASSEVAL.into(),
        cases,
    };
    crate::corpus::validate_manifest(&manifest)?;
    Ok(manifest)
}

fn hidden_unittest_check(test_source: &str, imports: &[String]) -> String {
    format!(
        r#"def check(module):
    import io, unittest
    ns = dict(module.__dict__)
    for stmt in {imports:?}:
        exec(stmt, ns, ns)
    exec({test_source:?}, ns, ns)
    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    for obj in list(ns.values()):
        if isinstance(obj, type) and issubclass(obj, unittest.TestCase) and obj is not unittest.TestCase:
            suite.addTests(loader.loadTestsFromTestCase(obj))
    buf = io.StringIO()
    result = unittest.TextTestRunner(stream=buf, verbosity=2).run(suite)
    if result.testsRun == 0 or not result.wasSuccessful():
        raise AssertionError(buf.getvalue() or 'no tests collected')
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_hides_tests_and_canonical_solution() {
        let json = r#"[{
            "task_id": "ClassEval_0",
            "class_name": "Adder",
            "skeleton": "class Adder:\n    def add(self, a, b):\n        pass\n",
            "test": "import unittest\nclass AdderTest(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(Adder().add(1, 2), 3)\n",
            "import_statement": [],
            "solution_code": "class Adder:\n    def add(self, a, b):\n        return a + b\n"
        }]"#;
        let manifest = classeval_json_to_manifest(json, 8).unwrap();
        assert_eq!(manifest.suite, SUITE_CLASSEVAL);
        assert_eq!(manifest.cases.len(), 1);
        let case = &manifest.cases[0];
        assert_eq!(case.id, "ClassEval_0");
        assert!(case
            .workspace_files
            .get("solution.py")
            .unwrap()
            .contains("pass"));
        assert!(!case.prompt.contains("return a + b"));
        match &case.scorers[0] {
            ScorerSpec::PythonHiddenCheck { test, .. } => {
                assert!(test.contains("unittest"));
                assert!(test.contains("AdderTest"));
            }
            other => panic!("unexpected scorer {other:?}"),
        }
    }

    #[test]
    fn hidden_unittest_accepts_implemented_class() {
        let json = r#"[{
            "task_id": "ClassEval_0",
            "class_name": "Adder",
            "skeleton": "class Adder:\n    def add(self, a, b):\n        pass\n",
            "test": "import unittest\nclass AdderTest(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(Adder().add(1, 2), 3)\n",
            "import_statement": []
        }]"#;
        let manifest = classeval_json_to_manifest(json, 1).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("solution.py"),
            "class Adder:\n    def add(self, a, b):\n        return a + b\n",
        )
        .unwrap();
        let t = crate::types::TurnTranscript {
            workspace: Some(dir.path().to_path_buf()),
            ..crate::types::TurnTranscript::default()
        };
        let result = crate::scorer::score_one(&manifest.cases[0].scorers[0], &t);
        if result.detail.as_deref() == Some("python_not_found") {
            return;
        }
        assert!(result.passed, "{result:?}");
    }
}
