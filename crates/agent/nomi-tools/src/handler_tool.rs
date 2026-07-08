//! Adapter: Hermes-style [`nomi_types::ToolHandler`] → nomi [`Tool`] registry surface.

use std::sync::Arc;

use async_trait::async_trait;
use nomi_protocol::events::ToolCategory;
use nomi_types::agent_tool::{StructuredJsonSchema, ToolHandler, ToolSchema};
use nomi_types::tool::{JsonSchema, ToolResult};
use serde_json::{Value, json};

use crate::Tool;

pub struct HandlerTool {
    handler: Arc<dyn ToolHandler>,
    schema: ToolSchema,
    category: ToolCategory,
}

impl HandlerTool {
    pub fn new(handler: Arc<dyn ToolHandler>) -> Self {
        let schema = handler.schema();
        Self {
            handler,
            schema,
            category: ToolCategory::Exec,
        }
    }

    pub fn with_category(mut self, category: ToolCategory) -> Self {
        self.category = category;
        self
    }
}

fn parameters_to_input_schema(params: &StructuredJsonSchema) -> JsonSchema {
    json!({
        "type": params.schema_type.as_deref().unwrap_or("object"),
        "properties": params.properties,
        "required": params.required,
        "additionalProperties": params.additional_properties,
    })
}

#[async_trait]
impl Tool for HandlerTool {
    fn name(&self) -> &str {
        &self.schema.name
    }

    fn description(&self) -> &str {
        &self.schema.description
    }

    fn input_schema(&self) -> JsonSchema {
        parameters_to_input_schema(&self.schema.parameters)
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        match self.handler.execute(input).await {
            Ok(text) => ToolResult::text(text),
            Err(err) => ToolResult::error(err.to_string()),
        }
    }

    fn category(&self) -> ToolCategory {
        self.category
    }
}
