//! Hermes-compatible tool handler surface used by migrated media workflows.

use async_trait::async_trait;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid tool parameters: {0}")]
    InvalidParams(String),

    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Tool timed out: {0}")]
    Timeout(String),

    #[error("Schema violation: {0}")]
    SchemaViolation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: StructuredJsonSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructuredJsonSchema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

impl StructuredJsonSchema {
    pub fn object(properties: IndexMap<String, Value>, required: Vec<String>) -> Self {
        Self {
            schema_type: Some("object".into()),
            properties: Some(properties),
            required: Some(required),
            additional_properties: Some(false),
        }
    }
}

/// Alias used by migrated Hermes media workflow code.
pub type JsonSchema = StructuredJsonSchema;

pub fn tool_schema(
    name: impl Into<String>,
    description: impl Into<String>,
    params_schema: StructuredJsonSchema,
) -> ToolSchema {
    ToolSchema {
        name: name.into(),
        description: description.into(),
        parameters: params_schema,
    }
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    async fn execute(&self, params: Value) -> Result<String, ToolError>;
    fn schema(&self) -> ToolSchema;
}
