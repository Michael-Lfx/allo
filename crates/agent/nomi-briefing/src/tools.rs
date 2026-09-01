use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use nomi_protocol::events::ToolCategory;
use nomi_tools::{Tool, ToolRegistry};
use nomi_types::tool::{JsonSchema, ToolResult};
use serde_json::{json, Value};

use crate::ir::ResearchDepth;
use crate::service::BriefingService;

pub fn wire_briefing_tools(registry: &mut ToolRegistry, data_dir: &Path) -> bool {
    let Ok(service) = BriefingService::open(data_dir) else {
        return false;
    };
    registry.register(Box::new(BriefingCreateTool {
        service: Arc::clone(&service),
    }));
    registry.register(Box::new(BriefingStatusTool { service }));
    true
}

struct BriefingCreateTool {
    service: Arc<BriefingService>,
}

struct BriefingStatusTool {
    service: Arc<BriefingService>,
}

#[async_trait]
impl Tool for BriefingCreateTool {
    fn name(&self) -> &str {
        "briefing_create"
    }

    fn description(&self) -> &str {
        "Create a sourced news briefing from the user's intent. The engine researches independent sources; source_urls are optional extras. Never invent today's news and do not use web_search to write the script. Defaults: 90s, 24h window, fast research."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "intent": { "type": "string", "description": "Topic or briefing brief" },
                "format_secs": { "type": "integer", "minimum": 30, "maximum": 300, "description": "Spoken length in seconds (30–300). Default 90." },
                "research_depth": { "type": "string", "enum": ["fast", "deep"] },
                "time_window_hours": { "type": "integer", "description": "How far back to look, in hours. Default 24." },
                "source_urls": { "type": "array", "items": { "type": "string" }, "description": "Optional URLs to prefer. The engine still searches if fewer than two independent domains." },
                "title": { "type": "string" },
                "tts_provider_id": { "type": "string" },
                "tts_model": { "type": "string" },
                "tts_voice": { "type": "string" },
                "image_provider_id": { "type": "string" },
                "image_model": { "type": "string" }
            },
            "required": ["intent"],
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }

    fn auto_approve_invocation(&self, _input: &Value, _category: ToolCategory) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let intent = input
            .get("intent")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if intent.is_empty() {
            return ToolResult::error("intent is required");
        }
        let format_secs = input
            .get("format_secs")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let depth = input
            .get("research_depth")
            .and_then(Value::as_str)
            .and_then(ResearchDepth::parse)
            .unwrap_or(ResearchDepth::Fast);
        let time_window_hours = input
            .get("time_window_hours")
            .and_then(Value::as_u64)
            .unwrap_or(24) as u32;
        let source_urls = input
            .get("source_urls")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let title = input
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string);
        let service = Arc::clone(&self.service);
        let created = match service.create(crate::service::CreateBriefingInput {
            intent: intent.to_string(),
            title,
            format_secs,
            depth,
            time_window_hours,
            source_urls,
            tts_provider_id: input
                .get("tts_provider_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            tts_model: input
                .get("tts_model")
                .and_then(Value::as_str)
                .map(str::to_string),
            tts_voice: input
                .get("tts_voice")
                .and_then(Value::as_str)
                .map(str::to_string),
            image_provider_id: input
                .get("image_provider_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            image_model: input
                .get("image_model")
                .and_then(Value::as_str)
                .map(str::to_string),
        }) {
            Ok(record) => record,
            Err(err) => return ToolResult::error(err.to_string()),
        };
        if let Err(err) = service.start_run(&created.id, true) {
            return ToolResult::error(err.to_string());
        }
        ToolResult::text(
            json!({
                "briefing_id": created.id,
                "status": "started",
                "title": created.title,
                "workspace": format!("/video-generation/briefing/{}", created.id),
            })
            .to_string(),
        )
    }
}

#[async_trait]
impl Tool for BriefingStatusTool {
    fn name(&self) -> &str {
        "briefing_status"
    }

    fn description(&self) -> &str {
        "Poll a news briefing created with briefing_create. Do not rewrite the script from memory."
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "briefing_id": { "type": "string" }
            },
            "required": ["briefing_id"],
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }

    fn auto_approve_invocation(&self, _input: &Value, _category: ToolCategory) -> bool {
        true
    }

    fn is_polling_invocation(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let id = input
            .get("briefing_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if id.is_empty() {
            return ToolResult::error("briefing_id is required");
        }
        match self.service.status(id) {
            Ok(snapshot) => ToolResult::text(
                json!({
                    "briefing_id": id,
                    "status": snapshot.status.as_str(),
                    "stage": snapshot.stage,
                    "message": snapshot.message,
                    "final_video": snapshot.final_video,
                })
                .to_string(),
            ),
            Err(err) => ToolResult::error(err.to_string()),
        }
    }
}
