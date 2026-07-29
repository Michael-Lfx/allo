//! Request-level Cursor-style context usage breakdown.
//!
//! Built from the exact `LlmRequest` inputs (system sections, tool defs, and
//! messages) using the same local estimators as compaction, then calibrated to
//! the engine context occupancy gauge.

use std::collections::HashMap;

use nomi_types::context_usage::{ContextUsageBreakdown, SummarizedConversationProperties};
use nomi_types::message::Message;
use nomi_types::tool::ToolDef;

use crate::compact::auto::{extract_compact_metadata, is_compaction_artifact};
use crate::compact::estimate::{
    estimate_tokens_from_message, estimate_tokens_from_text, estimate_tokens_from_tool_def,
};
use crate::plan::prompt::plan_mode_instructions;

const SYSTEM_PROMPT_SECTIONS: &[&str] = &[
    "intro",
    "tool_guidance",
    "browser_preset",
    "memory",
    "toon",
    "environment",
];
const RULES_SECTIONS: &[&str] = &["custom", "agents_md"];
const SKILLS_SECTIONS: &[&str] = &["skills"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCategory {
    Definitions,
    McpDynamic,
    Delegate,
}

/// Inputs for one provider request's context breakdown.
pub struct ContextUsageRequest<'a> {
    pub system_prompt: &'a str,
    pub system_prompt_sections: &'a HashMap<&'static str, String>,
    pub tools: &'a [ToolDef],
    pub messages: &'a [Message],
    pub plan_mode_active: bool,
    pub turn_tail_extras: &'a [String],
}

pub fn estimate_context_usage(request: ContextUsageRequest<'_>) -> ContextUsageBreakdown {
    let mut breakdown = ContextUsageBreakdown::default();

    if request.system_prompt_sections.is_empty() {
        breakdown.system_prompt = estimate_tokens_from_text(request.system_prompt);
    } else {
        breakdown.system_prompt = estimate_named_sections(request.system_prompt_sections, SYSTEM_PROMPT_SECTIONS);
        breakdown.rules = estimate_named_sections(request.system_prompt_sections, RULES_SECTIONS);
        breakdown.skills = estimate_named_sections(request.system_prompt_sections, SKILLS_SECTIONS);
    }

    for tool in request.tools {
        let tokens = estimate_tokens_from_tool_def(tool);
        match classify_tool(tool) {
            ToolCategory::Definitions => breakdown.tool_definitions += tokens,
            ToolCategory::McpDynamic => breakdown.mcp_and_dynamic_tools += tokens,
            ToolCategory::Delegate => breakdown.delegate_definitions += tokens,
        }
    }

    for message in request.messages {
        let tokens = estimate_tokens_from_message(message);
        if is_compaction_artifact(message) {
            breakdown.summarized_conversation += tokens;
        } else {
            breakdown.conversation += tokens;
        }
    }

    if request.plan_mode_active {
        breakdown.rules += estimate_tokens_from_text(plan_mode_instructions());
    }

    for extra in request.turn_tail_extras {
        // Plan mode instructions are counted under Rules above.
        if request.plan_mode_active && extra.as_str() == plan_mode_instructions() {
            continue;
        }
        breakdown.conversation += estimate_tokens_from_text(extra);
    }

    breakdown.summarized = latest_summarized_properties(request.messages);
    breakdown
}

fn estimate_named_sections(
    sections: &HashMap<&'static str, String>,
    names: &[&str],
) -> u64 {
    names
        .iter()
        .filter_map(|name| sections.get(name))
        .map(|text| estimate_tokens_from_text(text))
        .sum()
}

fn classify_tool(tool: &ToolDef) -> ToolCategory {
    if is_delegate_tool(&tool.name) {
        return ToolCategory::Delegate;
    }
    if tool.name.starts_with("mcp__") || tool.deferred {
        return ToolCategory::McpDynamic;
    }
    ToolCategory::Definitions
}

fn is_delegate_tool(name: &str) -> bool {
    name == "nomi_delegate" || name.starts_with("agent__")
}

fn latest_summarized_properties(messages: &[Message]) -> Option<SummarizedConversationProperties> {
    messages.iter().rev().find_map(|message| {
        extract_compact_metadata(message).map(|meta| SummarizedConversationProperties {
            trigger: Some(meta.trigger),
            pre_compact_tokens: Some(meta.pre_compact_tokens),
            messages_summarized: Some(meta.messages_summarized),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_types::compact::{CompactMetadata, CompactTrigger};
    use nomi_types::message::{ContentBlock, Role};
    use serde_json::json;

    use crate::compact::auto::BOUNDARY_PREFIX;

    fn tool(name: &str, deferred: bool) -> ToolDef {
        ToolDef {
            name: name.into(),
            description: "desc".into(),
            input_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}}),
            deferred,
        }
    }

    #[test]
    fn buckets_system_sections_and_tools() {
        let mut sections = HashMap::new();
        sections.insert("intro", "a".repeat(400));
        sections.insert("agents_md", "b".repeat(200));
        sections.insert("skills", "c".repeat(120));
        let tools = vec![
            tool("Read", false),
            tool("mcp__demo__search", false),
            tool("nomi_delegate", true),
            tool("secret_tool", true),
        ];
        let messages = vec![Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "d".repeat(400),
            }],
        )];
        let breakdown = estimate_context_usage(ContextUsageRequest {
            system_prompt: "",
            system_prompt_sections: &sections,
            tools: &tools,
            messages: &messages,
            plan_mode_active: false,
            turn_tail_extras: &[],
        });
        assert_eq!(breakdown.system_prompt, 100);
        assert_eq!(breakdown.rules, 50);
        assert_eq!(breakdown.skills, 30);
        assert!(breakdown.tool_definitions > 0);
        assert!(breakdown.mcp_and_dynamic_tools > 0);
        assert!(breakdown.delegate_definitions > 0);
        assert_eq!(breakdown.conversation, 100);
        assert_eq!(breakdown.summarized_conversation, 0);
    }

    #[test]
    fn delegate_wins_over_deferred_classification() {
        assert_eq!(classify_tool(&tool("nomi_delegate", true)), ToolCategory::Delegate);
        assert_eq!(
            classify_tool(&tool("mcp__x__y", false)),
            ToolCategory::McpDynamic
        );
        assert_eq!(classify_tool(&tool("Read", false)), ToolCategory::Definitions);
    }

    #[test]
    fn summarized_messages_and_metadata() {
        let metadata = CompactMetadata {
            trigger: CompactTrigger::Auto,
            pre_compact_tokens: 120_000,
            messages_summarized: 18,
        };
        let boundary = Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: format!(
                    "{BOUNDARY_PREFIX}\n{}",
                    serde_json::to_string(&metadata).unwrap()
                ),
            }],
        );
        let summary = Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: format!("This session is being continued{}", "e".repeat(400)),
            }],
        );
        let live = Message::new(
            Role::User,
            vec![ContentBlock::Text {
                text: "f".repeat(80),
            }],
        );
        let messages = vec![boundary, summary, live];
        let sections = HashMap::new();
        let system_prompt = "g".repeat(40);
        let breakdown = estimate_context_usage(ContextUsageRequest {
            system_prompt: &system_prompt,
            system_prompt_sections: &sections,
            tools: &[],
            messages: &messages,
            plan_mode_active: false,
            turn_tail_extras: &[],
        });
        assert!(breakdown.summarized_conversation > breakdown.conversation);
        let props = breakdown.summarized.expect("metadata");
        assert_eq!(props.trigger, Some(CompactTrigger::Auto));
        assert_eq!(props.pre_compact_tokens, Some(120_000));
        assert_eq!(props.messages_summarized, Some(18));
    }

    #[test]
    fn plan_mode_counts_under_rules_not_turn_tail() {
        let sections = HashMap::new();
        let plan = plan_mode_instructions().to_string();
        let extras = vec![plan.clone(), "Current date: 2026-07-25".into()];
        let breakdown = estimate_context_usage(ContextUsageRequest {
            system_prompt: "",
            system_prompt_sections: &sections,
            tools: &[],
            messages: &[],
            plan_mode_active: true,
            turn_tail_extras: &extras,
        });
        assert_eq!(
            breakdown.rules,
            estimate_tokens_from_text(plan_mode_instructions())
        );
        assert_eq!(
            breakdown.conversation,
            estimate_tokens_from_text("Current date: 2026-07-25")
        );
    }
}
