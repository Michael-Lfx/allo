//! Per-turn tool-efficiency accounting.
//!
//! Split out of the engine module, which keeps only loop control: this is a
//! self-contained accumulator plus the single log record it produces. Every
//! item is `pub(super)` because the engine drives it and its own test module
//! asserts on the counters directly.

use super::*;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ToolEfficiencyStats {
    pub(super) model_turn_attempts: usize,
    pub(super) model_turns_with_tools: usize,
    pub(super) total_tool_calls: usize,
    pub(super) max_calls_in_model_turn: usize,
    pub(super) exec_command_script_calls: usize,
    pub(super) batch_read_files_requested: usize,
    pub(super) error_results: usize,
    pub(super) skipped_after_prior_error: usize,
}

impl ToolEfficiencyStats {
    pub(super) fn observe_model_turn_attempt(&mut self) {
        self.model_turn_attempts = self.model_turn_attempts.saturating_add(1);
    }

    pub(super) fn observe_calls(&mut self, _registry: &ToolRegistry, blocks: &[ContentBlock]) {
        let calls = blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { name, input, .. } => Some((name.as_str(), input)),
                _ => None,
            })
            .collect::<Vec<_>>();
        if calls.is_empty() {
            return;
        }

        self.model_turns_with_tools = self.model_turns_with_tools.saturating_add(1);
        self.total_tool_calls = self.total_tool_calls.saturating_add(calls.len());
        self.max_calls_in_model_turn = self.max_calls_in_model_turn.max(calls.len());
        for (name, input) in &calls {
            if *name == "exec_command" && input.get("script").is_some() {
                self.exec_command_script_calls =
                    self.exec_command_script_calls.saturating_add(1);
            }
            if *name == "Read"
                && let Some(paths) = input.get("file_paths").and_then(Value::as_array)
            {
                self.batch_read_files_requested = self
                    .batch_read_files_requested
                    .saturating_add(paths.len());
            }
        }
    }

    pub(super) fn terminal_dimensions(
        &self,
        result: &Result<AgentResult, AgentError>,
    ) -> (&'static str, &'static str, &'static str, usize) {
        match result {
            Ok(result) => (
                "ok",
                match result.stop_reason {
                    StopReason::EndTurn => "end_turn",
                    StopReason::ToolUse => "tool_use",
                    StopReason::MaxTokens => "max_tokens",
                    StopReason::MaxTurns => "max_turns",
                    StopReason::Refusal => "refusal",
                },
                "none",
                result.turns,
            ),
            Err(error) => (
                "error",
                "error",
                match error {
                    AgentError::ApiError(_) => "api_error",
                    AgentError::Provider(_) => "provider_error",
                    AgentError::UserAborted => "user_aborted",
                    AgentError::ContextTooLong { .. } => "context_too_long",
                    AgentError::Stagnation(_) => "tool_stagnation",
                },
                self.model_turn_attempts,
            ),
        }
    }

    pub(super) fn observe_results(&mut self, blocks: &[ContentBlock]) {
        for block in blocks {
            let ContentBlock::ToolResult {
                content, is_error, ..
            } = block
            else {
                continue;
            };
            if *is_error {
                self.error_results = self.error_results.saturating_add(1);
            }
            if *is_error && content == SKIPPED_AFTER_PRIOR_ERROR {
                self.skipped_after_prior_error =
                    self.skipped_after_prior_error.saturating_add(1);
            }
        }
    }

    pub(super) fn log(
        &self,
        session_id: &str,
        msg_id: &str,
        result: &Result<AgentResult, AgentError>,
    ) {
        let (terminal, stop_reason, error_kind, agent_turns) =
            self.terminal_dimensions(result);
        tracing::info!(
            target: "nomi_agent::tool_efficiency",
            session_id,
            msg_id,
            agent_turns,
            stop_reason,
            terminal,
            error_kind,
            model_turn_attempts = self.model_turn_attempts,
            model_turns_with_tools = self.model_turns_with_tools,
            tool_calls_total = self.total_tool_calls,
            max_calls_in_model_turn = self.max_calls_in_model_turn,
            exec_command_script_calls = self.exec_command_script_calls,
            batch_read_files_requested = self.batch_read_files_requested,
            tool_error_results = self.error_results,
            skipped_after_prior_error = self.skipped_after_prior_error,
            "agent tool efficiency summary"
        );
    }
}
