//! Optional SkillSearch tool. Registered only when the catalog is large enough
//! that the L1 listing may omit skills.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use nomi_protocol::events::ToolCategory;
use nomi_skills::dynamic_skill_selector::{
    CheapSkillSelector, WeightedLexicalSkillSelector, documents_from_skills,
    SKILL_SEARCH_CATALOG_THRESHOLD,
};
use nomi_skills::types::SkillMetadata;
use nomi_types::tool::{JsonSchema, ToolResult};

use nomi_tools::Tool;

pub fn should_register_skill_search(skill_count: usize) -> bool {
    skill_count > SKILL_SEARCH_CATALOG_THRESHOLD
}

pub struct SkillSearchTool {
    skills: Arc<Vec<SkillMetadata>>,
}

impl SkillSearchTool {
    pub fn new(skills: Arc<Vec<SkillMetadata>>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for SkillSearchTool {
    fn name(&self) -> &str {
        "SkillSearch"
    }

    fn description(&self) -> &str {
        "Search the skill catalog when a needed skill is missing from the system listing. \
         Returns matching catalog ids to pass to the Skill tool. Do not use this when the \
         needed skill is already listed."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Task or skill keywords to search for"
                }
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let query = input["query"].as_str().unwrap_or("").trim();
        if query.is_empty() {
            return ToolResult {
                content: "Error: query is required".to_string(),
                is_error: true,
                images: Vec::new(),
            };
        }
        let visible: Vec<SkillMetadata> = self
            .skills
            .iter()
            .filter(|skill| !skill.disable_model_invocation)
            .cloned()
            .collect();
        let documents = documents_from_skills(&visible);
        let ranked = WeightedLexicalSkillSelector.select(query, &documents, 8, &[]);
        if ranked.candidate_ids.is_empty() {
            return ToolResult {
                content: "No matching skills.".to_string(),
                is_error: false,
                images: Vec::new(),
            };
        }
        let mut lines = vec!["Matching skills (call Skill with the id):".to_string()];
        for id in &ranked.candidate_ids {
            if let Some(skill) = visible.iter().find(|item| {
                item.catalog_id.as_deref() == Some(id.as_str()) || item.name == *id
            }) {
                lines.push(format!(
                    "- {} (`{}`): {}",
                    skill.name,
                    skill.catalog_id.as_deref().unwrap_or(skill.name.as_str()),
                    skill.description
                ));
            }
        }
        ToolResult {
            content: lines.join("\n"),
            is_error: false,
            images: Vec::new(),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_search_stays_off_for_small_catalogs() {
        assert!(!should_register_skill_search(15));
        assert!(!should_register_skill_search(SKILL_SEARCH_CATALOG_THRESHOLD));
        assert!(should_register_skill_search(SKILL_SEARCH_CATALOG_THRESHOLD + 1));
    }
}
