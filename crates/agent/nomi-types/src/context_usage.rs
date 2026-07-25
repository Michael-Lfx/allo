use serde::{Deserialize, Serialize};

use crate::compact::CompactTrigger;

/// Optional metadata for the most recent conversation compaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SummarizedConversationProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<CompactTrigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_compact_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages_summarized: Option<usize>,
}

/// Cursor-style context occupancy breakdown for one provider request.
///
/// Category values are calibrated local estimates that sum to the engine's
/// context occupancy gauge (`context_tokens`). Providers do not report
/// per-category tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextUsageBreakdown {
    #[serde(default)]
    pub system_prompt: u64,
    #[serde(default)]
    pub tool_definitions: u64,
    #[serde(default)]
    pub rules: u64,
    #[serde(default)]
    pub skills: u64,
    #[serde(default)]
    pub mcp_and_dynamic_tools: u64,
    #[serde(default)]
    pub subagent_definitions: u64,
    #[serde(default)]
    pub summarized_conversation: u64,
    #[serde(default)]
    pub conversation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarized: Option<SummarizedConversationProperties>,
}

impl ContextUsageBreakdown {
    pub fn total(&self) -> u64 {
        self.system_prompt
            .saturating_add(self.tool_definitions)
            .saturating_add(self.rules)
            .saturating_add(self.skills)
            .saturating_add(self.mcp_and_dynamic_tools)
            .saturating_add(self.subagent_definitions)
            .saturating_add(self.summarized_conversation)
            .saturating_add(self.conversation)
    }

    /// Scale category totals so they sum exactly to `target`.
    ///
    /// Remainder after integer scaling is applied to the largest category so the
    /// panel occupancy matches the provider/engine gauge.
    pub fn calibrate_to(&mut self, target: u64) {
        let summarized = self.summarized.clone();
        let sum = self.total();
        if target == 0 {
            *self = Self {
                summarized,
                ..Self::default()
            };
            return;
        }
        if sum == 0 {
            self.conversation = target;
            return;
        }
        if sum == target {
            return;
        }

        let originals = [
            self.system_prompt,
            self.tool_definitions,
            self.rules,
            self.skills,
            self.mcp_and_dynamic_tools,
            self.subagent_definitions,
            self.summarized_conversation,
            self.conversation,
        ];
        let mut scaled = [0u64; 8];
        let mut scaled_sum = 0u64;
        for (idx, original) in originals.iter().enumerate() {
            let value = ((*original as u128) * (target as u128) / (sum as u128)) as u64;
            scaled[idx] = value;
            scaled_sum = scaled_sum.saturating_add(value);
        }

        let mut largest_idx = 0usize;
        let mut largest_val = 0u64;
        for (idx, original) in originals.iter().enumerate() {
            if *original >= largest_val {
                largest_val = *original;
                largest_idx = idx;
            }
        }
        if scaled_sum < target {
            scaled[largest_idx] = scaled[largest_idx].saturating_add(target - scaled_sum);
        } else if scaled_sum > target {
            scaled[largest_idx] = scaled[largest_idx].saturating_sub(scaled_sum - target);
        }

        self.system_prompt = scaled[0];
        self.tool_definitions = scaled[1];
        self.rules = scaled[2];
        self.skills = scaled[3];
        self.mcp_and_dynamic_tools = scaled[4];
        self.subagent_definitions = scaled[5];
        self.summarized_conversation = scaled[6];
        self.conversation = scaled[7];
        self.summarized = summarized;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrate_preserves_exact_total() {
        let mut breakdown = ContextUsageBreakdown {
            system_prompt: 100,
            tool_definitions: 200,
            rules: 50,
            skills: 25,
            mcp_and_dynamic_tools: 25,
            subagent_definitions: 10,
            summarized_conversation: 90,
            conversation: 500,
            summarized: None,
        };
        assert_eq!(breakdown.total(), 1000);
        breakdown.calibrate_to(272_000);
        assert_eq!(breakdown.total(), 272_000);
    }

    #[test]
    fn calibrate_zero_estimate_puts_all_in_conversation() {
        let mut breakdown = ContextUsageBreakdown::default();
        breakdown.calibrate_to(42);
        assert_eq!(breakdown.conversation, 42);
        assert_eq!(breakdown.total(), 42);
    }
}
