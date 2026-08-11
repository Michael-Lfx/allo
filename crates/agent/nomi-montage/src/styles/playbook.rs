//! Style playbooks — reusable visual/tonal direction sheets referenced by
//! `proposal_packet.style_playbook` and injected into creative stage prompts.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{MontageError, MontageResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StylePlaybook {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub visual_language: Vec<String>,
    #[serde(default)]
    pub pacing: String,
    #[serde(default)]
    pub tone: Vec<String>,
    #[serde(default)]
    pub prompt_modifiers: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
    #[serde(default)]
    pub reference_notes: Vec<String>,
}

impl StylePlaybook {
    /// A compact clause suitable for appending to image/video generation prompts.
    pub fn prompt_clause(&self) -> String {
        let mut parts = self.prompt_modifiers.clone();
        if !self.tone.is_empty() {
            parts.push(format!("tone: {}", self.tone.join(", ")));
        }
        if parts.is_empty() {
            self.description.clone()
        } else {
            parts.join("; ")
        }
    }
}

#[derive(Debug, Clone)]
pub struct StyleRegistry {
    playbooks: BTreeMap<String, StylePlaybook>,
}

impl StyleRegistry {
    pub fn load_embedded() -> MontageResult<Self> {
        Self::load_from_dir(&crate::assets_root().join("styles"))
    }

    pub fn load_from_dir(dir: &Path) -> MontageResult<Self> {
        if !dir.is_dir() {
            return Err(MontageError::msg(format!(
                "styles directory missing: {}",
                dir.display()
            )));
        }
        let mut playbooks = BTreeMap::new();
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
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| MontageError::msg(format!("reading {}: {e}", path.display())))?;
            let playbook: StylePlaybook = serde_yaml::from_str(&raw)?;
            playbooks.insert(playbook.name.clone(), playbook);
        }
        Ok(Self { playbooks })
    }

    pub fn get(&self, name: &str) -> Option<&StylePlaybook> {
        self.playbooks.get(name)
    }

    pub fn list(&self) -> Vec<&StylePlaybook> {
        self.playbooks.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_embedded_playbooks() {
        let registry = StyleRegistry::load_embedded().expect("styles load");
        assert!(registry.list().len() >= 3, "expect at least 3 style playbooks");
    }
}
