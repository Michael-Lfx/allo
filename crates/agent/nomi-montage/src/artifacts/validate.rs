//! Loads and applies `assets/schemas/artifacts/*.json`.

use std::collections::BTreeMap;
use std::path::Path;

use jsonschema::Validator;

use crate::error::{MontageError, MontageResult};

/// Compiled artifact JSON Schemas, keyed by canonical artifact name
/// (filename stem, e.g. `script` for `schemas/artifacts/script.json`).
pub struct ArtifactRegistry {
    schemas: BTreeMap<String, Validator>,
}

impl ArtifactRegistry {
    pub fn load_embedded() -> MontageResult<Self> {
        Self::load_from_dir(&crate::assets_root().join("schemas").join("artifacts"))
    }

    pub fn load_from_dir(dir: &Path) -> MontageResult<Self> {
        if !dir.is_dir() {
            return Err(MontageError::msg(format!(
                "artifact schema directory missing: {}",
                dir.display()
            )));
        }
        let mut schemas = BTreeMap::new();
        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        entries.sort();
        for path in entries {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| MontageError::msg(format!("bad schema filename: {}", path.display())))?
                .to_string();
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| MontageError::msg(format!("reading {}: {e}", path.display())))?;
            let schema: serde_json::Value = serde_json::from_str(&raw)?;
            let validator = jsonschema::options()
                .build(&schema)
                .map_err(|e| MontageError::msg(format!("compiling schema '{name}': {e}")))?;
            schemas.insert(name, validator);
        }
        Ok(Self { schemas })
    }

    pub fn names(&self) -> Vec<&str> {
        self.schemas.keys().map(String::as_str).collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.schemas.contains_key(name)
    }

    /// Validate `value` against the schema named `name`.
    pub fn validate(&self, name: &str, value: &serde_json::Value) -> MontageResult<()> {
        let validator = self
            .schemas
            .get(name)
            .ok_or_else(|| MontageError::SchemaNotFound(name.to_string()))?;
        let errors: Vec<String> = validator
            .iter_errors(value)
            .map(|e| format!("at {}: {e}", e.instance_path()))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(MontageError::ArtifactInvalid(
                name.to_string(),
                errors.join("; "),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_every_declared_artifact_name() {
        let registry = ArtifactRegistry::load_embedded().expect("artifact schemas load");
        for name in crate::artifacts::names::ARTIFACT_NAMES {
            assert!(
                registry.has(name),
                "missing schema for declared artifact name: {name}"
            );
        }
    }

    #[test]
    fn rejects_missing_required_fields() {
        let registry = ArtifactRegistry::load_embedded().unwrap();
        let err = registry.validate("script", &serde_json::json!({})).unwrap_err();
        assert!(matches!(err, MontageError::ArtifactInvalid(_, _)));
    }
}
