//! Workflow plan / step schema.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: String,
    pub version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub inputs: Value,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub on_fail: Option<WorkflowFailAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowFailAction {
    #[serde(default)]
    pub retry_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub workflow_id: String,
    #[serde(default, alias = "version")]
    pub template_version: u32,
    #[serde(default)]
    pub inputs: Value,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
    #[serde(default)]
    pub estimated_steps: u32,
}

impl WorkflowPlan {
    pub fn from_definition(def: &WorkflowDefinition, inputs: Value) -> Self {
        Self {
            workflow_id: def.id.clone(),
            template_version: def.version,
            inputs,
            estimated_steps: def.steps.len() as u32,
            steps: def.steps.clone(),
        }
    }

    /// Fill missing template metadata when the model passes a partial plan from `media_workflow_plan`.
    pub fn normalize(&mut self) -> Result<(), String> {
        let def = super::templates::builtin_template(&self.workflow_id).ok_or_else(|| {
            format!(
                "unknown workflow_id '{}' — cannot resolve template_version/steps",
                self.workflow_id
            )
        })?;
        if self.template_version == 0 {
            self.template_version = def.version;
        }
        if self.steps.is_empty() {
            self.steps = def.steps.clone();
            if self.estimated_steps == 0 {
                self.estimated_steps = def.steps.len() as u32;
            }
        }
        if self.inputs.is_null() {
            self.inputs = def.inputs.clone();
        }
        Ok(())
    }
}

/// Parse a workflow plan from tool JSON, tolerating LLM-trimmed objects.
pub fn parse_workflow_plan(plan_val: serde_json::Value) -> Result<WorkflowPlan, String> {
    let mut plan: WorkflowPlan =
        serde_json::from_value(plan_val).map_err(|e| format!("invalid plan: {e}"))?;
    plan.normalize()?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_plan_without_template_version() {
        let raw = json!({
            "workflow_id": "txt2img",
            "inputs": { "prompt": "a red apple on a wooden table" }
        });
        let plan = parse_workflow_plan(raw).expect("parse");
        assert_eq!(plan.workflow_id, "txt2img");
        assert!(plan.template_version > 0);
        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn parse_plan_accepts_version_alias() {
        let raw = json!({
            "workflow_id": "txt2img",
            "version": 1,
            "inputs": { "prompt": "test" },
            "steps": []
        });
        let plan = parse_workflow_plan(raw).expect("parse");
        assert_eq!(plan.template_version, 1);
        assert!(!plan.steps.is_empty());
    }
}
