//! Built-in `PreSendHook`s for the ACP prompt pipeline.
//!
//! Each hook reads a one-shot flag on `AcpSession` (or a `pending_*`
//! field), consumes it, and prepends its block to the prompt. Failures
//! are reported via `ctx.runtime.emit(AgentStreamEvent::AcpPromptHookWarning(..))`
//! and the prompt is returned in a gracefully-degraded form.

mod poi_prefetch;

pub use poi_prefetch::PoiPrefetchHook;

use crate::capability::first_message_injector::{InjectionConfig, inject_first_message_prefix};
use crate::capability::model_identity_reminder::render_model_identity_reminder;
use crate::capability::prompt_pipeline::{PreSendHook, PromptCtx};
use crate::protocol::events::AgentStreamEvent;
use nomifun_api_types::AcpPromptHookWarningPayload;

#[derive(Default)]
pub struct SessionNewPreludeHook;

#[async_trait::async_trait]
impl PreSendHook for SessionNewPreludeHook {
    async fn pre_send(&self, ctx: &mut PromptCtx<'_>, prompt: String) -> String {
        if !ctx.session.take_pending_session_new_prelude() {
            return prompt;
        }

        let metadata = &ctx.params.metadata;
        let config = InjectionConfig {
            preset_context: ctx.params.preset_context.as_deref(),
            skills: &ctx.params.config.skills,
            custom_workspace: ctx.params.workspace.is_custom,
            native_skill_support: metadata
                .native_skills_dirs
                .as_ref()
                .is_some_and(|v: &Vec<String>| !v.is_empty()),
        };

        inject_first_message_prefix(&prompt, ctx.skill_manager, config).await
    }
}

/// Deliver the knowledge-base retrieval-protocol section on the first prompt of
/// every session activation.
#[derive(Default)]
pub struct KnowledgeContextHook;

#[async_trait::async_trait]
impl PreSendHook for KnowledgeContextHook {
    async fn pre_send(&self, ctx: &mut PromptCtx<'_>, prompt: String) -> String {
        if !ctx.session.take_pending_knowledge_prelude() {
            return prompt;
        }
        match ctx.params.knowledge_context.as_deref() {
            Some(section) if !section.is_empty() => {
                format!("[Knowledge Bases]\n{section}\n[/Knowledge Bases]\n\n{prompt}")
            }
            _ => prompt,
        }
    }
}

#[derive(Default)]
pub struct ModelIdentityReminderHook;

#[async_trait::async_trait]
impl PreSendHook for ModelIdentityReminderHook {
    async fn pre_send(&self, ctx: &mut PromptCtx<'_>, prompt: String) -> String {
        let Some(model) = ctx.session.take_pending_model_notice() else {
            return prompt;
        };

        let label = ctx
            .session
            .model_info()
            .and_then(|m| {
                m.available_models
                    .iter()
                    .find(|am| am.model_id.0.as_ref() == model.as_str())
                    .map(|am| am.name.clone())
            })
            .unwrap_or_else(|| model.as_str().to_owned());

        let reminder = render_model_identity_reminder(&label);
        format!("{reminder}{prompt}")
    }
}

/// Emit a non-blocking toast warning back to the UI via the stream channel.
pub(crate) fn emit_hook_warning(ctx: &PromptCtx<'_>, hook: &'static str, message: impl Into<String>) {
    let payload = AcpPromptHookWarningPayload {
        hook: hook.to_owned(),
        message: message.into(),
    };
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    ctx.runtime.emit(AgentStreamEvent::AcpPromptHookWarning(value));
}

#[cfg(test)]
mod tests {
    //! Full-path hook tests live in tests/prompt_pipeline_integration.rs
    //! where a real AcpSession + AcpSessionParams + AgentRuntimeState triple
    //! is already wired for assertion. This module keeps unit-level
    //! property checks around the helpers that don't need ctx.
    use super::*;

    #[test]
    fn emit_hook_warning_payload_shape() {
        let payload = AcpPromptHookWarningPayload {
            hook: "session_new_prelude".into(),
            message: "boom".into(),
        };
        let v = serde_json::to_value(&payload).unwrap();
        assert_eq!(v["hook"], "session_new_prelude");
        assert_eq!(v["message"], "boom");
    }
}
