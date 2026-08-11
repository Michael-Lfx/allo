//! Loads and validates `assets/pipeline_defs/*.yaml` into [`PipelineManifest`].

use std::collections::BTreeMap;
use std::path::Path;

use jsonschema::Validator;

use crate::error::{MontageError, MontageResult};

use super::manifest::PipelineManifest;

/// In-memory registry of every embedded pipeline manifest, keyed by `name`.
#[derive(Debug, Clone)]
pub struct PipelineRegistry {
    pipelines: BTreeMap<String, PipelineManifest>,
}

impl PipelineRegistry {
    /// Load and schema-validate every YAML file under `assets/pipeline_defs/`.
    pub fn load_embedded() -> MontageResult<Self> {
        Self::load_from_dir(&crate::assets_root().join("pipeline_defs"))
    }

    pub fn load_from_dir(dir: &Path) -> MontageResult<Self> {
        let schema_path = crate::assets_root()
            .join("schemas")
            .join("pipelines")
            .join("pipeline_manifest.schema.json");
        let validator = load_validator(&schema_path)?;

        let mut pipelines = BTreeMap::new();
        if !dir.is_dir() {
            return Err(MontageError::msg(format!(
                "pipeline_defs directory missing: {}",
                dir.display()
            )));
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
            })
            .collect();
        entries.sort();

        for path in entries {
            let manifest = load_one(&path, &validator)?;
            let name = manifest.name.clone();
            if pipelines.insert(name.clone(), manifest).is_some() {
                return Err(MontageError::ManifestInvalid(
                    name,
                    "duplicate pipeline name across pipeline_defs/*.yaml".into(),
                ));
            }
        }
        Ok(Self { pipelines })
    }

    pub fn get(&self, name: &str) -> Option<&PipelineManifest> {
        self.pipelines.get(name)
    }

    pub fn require(&self, name: &str) -> MontageResult<&PipelineManifest> {
        self.get(name)
            .ok_or_else(|| MontageError::PipelineNotFound(name.to_string()))
    }

    pub fn list(&self) -> Vec<&PipelineManifest> {
        self.pipelines.values().collect()
    }

    pub fn len(&self) -> usize {
        self.pipelines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pipelines.is_empty()
    }
}

fn load_validator(schema_path: &Path) -> MontageResult<Validator> {
    let raw = std::fs::read_to_string(schema_path).map_err(|e| {
        MontageError::msg(format!(
            "reading pipeline manifest schema {}: {e}",
            schema_path.display()
        ))
    })?;
    let schema: serde_json::Value = serde_json::from_str(&raw)?;
    jsonschema::options()
        .build(&schema)
        .map_err(|e| MontageError::msg(format!("compiling pipeline manifest schema: {e}")))
}

fn load_one(path: &Path, validator: &Validator) -> MontageResult<PipelineManifest> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| MontageError::msg(format!("reading {}: {e}", path.display())))?;
    let value: serde_json::Value = serde_yaml::from_str(&raw)?;

    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| format!("at {}: {e}", e.instance_path()))
        .collect();
    if !errors.is_empty() {
        return Err(MontageError::ManifestInvalid(
            path.display().to_string(),
            errors.join("; "),
        ));
    }

    let manifest: PipelineManifest = serde_json::from_value(value).map_err(|e| {
        MontageError::ManifestInvalid(path.display().to_string(), format!("shape mismatch: {e}"))
    })?;

    if manifest.stages.is_empty() {
        return Err(MontageError::ManifestInvalid(
            manifest.name,
            "pipeline must declare at least one stage".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for stage in &manifest.stages {
        if !seen.insert(stage.name.as_str()) {
            return Err(MontageError::ManifestInvalid(
                manifest.name.clone(),
                format!("duplicate stage name '{}'", stage.name),
            ));
        }
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_all_embedded_pipelines() {
        let registry = PipelineRegistry::load_embedded().expect("embedded pipelines load");
        assert!(
            registry.len() >= 8,
            "expected at least 8 pipeline_defs, got {}",
            registry.len()
        );
        assert!(registry.get("framework-smoke").is_some());
        assert!(registry.get("cinematic").is_some());
    }

    #[test]
    fn every_stage_skill_path_exists_on_disk() {
        let registry = PipelineRegistry::load_embedded().expect("embedded pipelines load");
        let skills_root = crate::assets_root().join("skills");
        for manifest in registry.list() {
            let ep_path = skills_root.join(format!("{}.md", manifest.orchestration.skill));
            assert!(
                ep_path.is_file(),
                "missing EP skill file for {}: {}",
                manifest.name,
                ep_path.display()
            );
            for stage in &manifest.stages {
                let stage_path = skills_root.join(format!("{}.md", stage.skill));
                assert!(
                    stage_path.is_file(),
                    "missing stage skill file for {}/{}: {}",
                    manifest.name,
                    stage.name,
                    stage_path.display()
                );
            }
        }
    }
}
