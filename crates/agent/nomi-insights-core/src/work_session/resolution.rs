//! Resolution Verdict Engine — local POI solve-quality labeling.

use std::sync::Arc;

use nomi_config::InsightsContributionConfig;
use crate::types::ResolutionPayload;
use nomi_auxiliary::AuxiliaryClient;
use nomi_poi::{is_poi_synthetic_user_text, message_text_from_value};
use tracing::{debug, warn};

use super::resolution_llm::{
    format_session_transcript_for_resolution, infer_resolution_from_transcript_llm,
};

#[derive(Debug, Clone)]
pub struct SessionSignals {
    pub user_turns: u32,
    pub tool_failures: u32,
    pub tool_successes: u32,
    pub skill_patched: bool,
    pub skill_created: bool,
    pub explicit_positive: bool,
    pub explicit_negative: bool,
    pub correction_loops: u32,
    pub closure_without_followup: bool,
}

pub fn analyze_session(
    messages: &[serde_json::Value],
    skill_summary: &crate::SessionSkillSummary,
) -> SessionSignals {
    let mut user_turns = 0u32;
    let mut user_messages: Vec<String> = Vec::new();
    let mut tool_failures = 0u32;
    let mut tool_successes = 0u32;

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role.eq_ignore_ascii_case("user") {
            let text = message_text_from_value(msg);
            let trimmed = text.trim();
            if trimmed.is_empty() || is_poi_synthetic_user_text(trimmed) {
                continue;
            }
            user_turns += 1;
            user_messages.push(trimmed.to_string());
        } else if role.eq_ignore_ascii_case("tool") {
            let content = message_text_from_value(msg);
            if tool_outcome_is_failure(&content) {
                tool_failures += 1;
            } else if tool_outcome_is_success(&content) {
                tool_successes += 1;
            }
        }
    }

    let explicit_positive = user_messages
        .iter()
        .any(|m| matches_feedback(m, POSITIVE_PATTERNS));
    let explicit_negative = user_messages
        .iter()
        .any(|m| matches_feedback(m, NEGATIVE_PATTERNS));
    let correction_loops = count_correction_loops(&user_messages);
    let closure_without_followup = user_turns >= 2
        && !explicit_negative
        && user_messages
            .last()
            .is_some_and(|m| m.chars().count() < 120);

    SessionSignals {
        user_turns,
        tool_failures,
        tool_successes,
        skill_patched: skill_summary.patch_count > 0,
        skill_created: skill_summary.skill_created,
        explicit_positive,
        explicit_negative,
        correction_loops,
        closure_without_followup,
    }
}

pub fn fuse_verdict(signals: &SessionSignals) -> ResolutionPayload {
    let mut codes = Vec::new();

    if signals.user_turns < 2 {
        codes.push("insufficient_turns".to_string());
        return resolution_payload("abandoned", "low", "D", "unknown", None, codes, false, signals.correction_loops);
    }

    if signals.explicit_negative {
        codes.push("user_explicit_negative".to_string());
        if signals.correction_loops > 0 {
            codes.push("user_correction_loop".to_string());
        }
        let objective = objective_band(signals);
        return resolution_payload(
            "failed",
            "high",
            if signals.tool_failures > 0 { "B" } else { "A" },
            "explicit_negative",
            objective,
            codes,
            signals.skill_patched,
            signals.correction_loops,
        );
    }

    if signals.explicit_positive {
        codes.push("user_explicit_positive".to_string());
        if signals.closure_without_followup {
            codes.push("closure_without_followup".to_string());
        }
        let objective = objective_band(signals);
        if objective.as_deref() == Some("pass") {
            codes.push("objective_test_pass".to_string());
        }
        if signals.skill_patched {
            codes.push("skill_patched_this_session".to_string());
        }
        if signals.skill_created {
            codes.push("skill_created_this_session".to_string());
        }
        return resolution_payload(
            "solved_confirmed",
            "high",
            "A",
            "explicit_positive",
            objective,
            codes,
            false,
            signals.correction_loops,
        );
    }

    if signals.tool_failures > 0 && signals.tool_successes == 0 {
        codes.push("objective_test_fail".to_string());
        return resolution_payload(
            "partial",
            "medium",
            "C",
            "neutral",
            Some("fail".to_string()),
            codes,
            signals.skill_patched,
            signals.correction_loops,
        );
    }

    if signals.tool_successes > 0 && signals.correction_loops == 0 {
        codes.push("objective_test_pass".to_string());
        if signals.closure_without_followup {
            codes.push("closure_without_followup".to_string());
        }
        if signals.skill_patched {
            codes.push("skill_patched_this_session".to_string());
        }
        return resolution_payload(
            "solved_inferred",
            "medium",
            "B",
            "neutral",
            Some("pass".to_string()),
            codes,
            signals.skill_patched,
            signals.correction_loops,
        );
    }

    if signals.correction_loops > 0 {
        codes.push("user_correction_loop".to_string());
        return resolution_payload(
            "partial",
            "medium",
            "C",
            "neutral",
            objective_band(signals),
            codes,
            signals.skill_patched,
            signals.correction_loops,
        );
    }

    if (signals.skill_patched || signals.skill_created)
        && !signals.explicit_negative
        && signals.correction_loops == 0
    {
        if signals.skill_patched {
            codes.push("skill_patched_this_session".to_string());
        }
        if signals.skill_created {
            codes.push("skill_created_this_session".to_string());
        }
        if signals.closure_without_followup {
            codes.push("closure_without_followup".to_string());
        }
        return resolution_payload(
            "solved_inferred",
            "medium",
            "C",
            if signals.explicit_positive {
                "explicit_positive"
            } else {
                "neutral"
            },
            Some("not_applicable".to_string()),
            codes,
            signals.skill_patched,
            signals.correction_loops,
        );
    }

    codes.push("closure_without_followup".to_string());
    resolution_payload(
        "unresolved",
        "low",
        "D",
        "unknown",
        Some("not_applicable".to_string()),
        codes,
        false,
        signals.correction_loops,
    )
}

/// Resolve session verdict: rules baseline, optional LLM supplement, hybrid merge.
pub async fn resolve_session_verdict(
    insights_cfg: &InsightsContributionConfig,
    auxiliary: Option<&Arc<AuxiliaryClient>>,
    messages: &[serde_json::Value],
    signals: &SessionSignals,
) -> ResolutionPayload {
    let rules = fuse_verdict(signals);

    if !insights_cfg.session_end_resolution_llm_enabled() {
        return rules;
    }

    let mode = insights_cfg.resolution_mode_normalized();
    if mode == "rules" {
        return rules;
    }

    let Some(aux) = auxiliary else {
        warn!("resolution: session-end LLM enabled but auxiliary client unavailable — using rules");
        return rules;
    };

    let transcript = format_session_transcript_for_resolution(messages);
    if transcript.trim().is_empty() {
        debug!("resolution: session-end LLM skipped — empty transcript");
        return rules;
    }

    let llm_payload = infer_resolution_from_transcript_llm(aux, &transcript, signals).await;
    match mode.as_str() {
        "llm" => llm_payload.unwrap_or(rules),
        _ => merge_resolution_hybrid(signals, &rules, llm_payload.as_ref()),
    }
}

pub fn merge_resolution_hybrid(
    signals: &SessionSignals,
    rules: &ResolutionPayload,
    llm: Option<&ResolutionPayload>,
) -> ResolutionPayload {
    let Some(llm) = llm else {
        return rules.clone();
    };

    if rules.user_feedback_band == "explicit_negative" {
        return rules.clone();
    }
    if rules.verdict == "solved_confirmed" && rules.evidence_tier == "A" {
        return rules.clone();
    }

    let merged = merge_payloads(rules, llm);
    apply_skill_signal_boost(signals, merged)
}

fn merge_payloads(rules: &ResolutionPayload, llm: &ResolutionPayload) -> ResolutionPayload {
    let rules_rank = tier_rank(&rules.evidence_tier);
    let llm_rank = tier_rank(&llm.evidence_tier);
    let pick_llm = llm_rank > rules_rank
        || (llm_rank == rules_rank && verdict_rank(&llm.verdict) > verdict_rank(&rules.verdict));

    let base = if pick_llm { llm } else { rules };
    let mut signal_codes = base.signal_codes.clone();
    for code in &rules.signal_codes {
        if !signal_codes.contains(code) {
            signal_codes.push(code.clone());
        }
    }
    for code in &llm.signal_codes {
        if !signal_codes.contains(code) {
            signal_codes.push(code.clone());
        }
    }

    ResolutionPayload {
        verdict: base.verdict.clone(),
        confidence_band: base.confidence_band.clone(),
        evidence_tier: if llm_rank > rules_rank {
            llm.evidence_tier.clone()
        } else {
            rules.evidence_tier.clone()
        },
        user_feedback_band: prefer_feedback_band(
            &rules.user_feedback_band,
            &llm.user_feedback_band,
        ),
        objective_check_band: base
            .objective_check_band
            .clone()
            .or_else(|| rules.objective_check_band.clone()),
        signal_codes,
        recovery_attempted: rules.recovery_attempted || llm.recovery_attempted,
        correction_loops: rules.correction_loops.max(llm.correction_loops),
    }
}

fn apply_skill_signal_boost(
    signals: &SessionSignals,
    mut res: ResolutionPayload,
) -> ResolutionPayload {
    if !(signals.skill_patched || signals.skill_created) {
        return res;
    }
    if tier_rank(&res.evidence_tier) >= tier_rank("C") {
        return res;
    }
    if signals.skill_patched
        && !res
            .signal_codes
            .iter()
            .any(|c| c == "skill_patched_this_session")
    {
        res.signal_codes
            .push("skill_patched_this_session".to_string());
    }
    if signals.skill_created
        && !res
            .signal_codes
            .iter()
            .any(|c| c == "skill_created_this_session")
    {
        res.signal_codes
            .push("skill_created_this_session".to_string());
    }
    if res.verdict == "unresolved" || res.verdict == "abandoned" {
        res.verdict = "solved_inferred".to_string();
        res.evidence_tier = "C".to_string();
        res.confidence_band = "medium".to_string();
    }
    res
}

fn tier_rank(tier: &str) -> u8 {
    match tier.trim().to_ascii_uppercase().as_str() {
        "A" => 4,
        "B" => 3,
        "C" => 2,
        "D" => 1,
        _ => 0,
    }
}

fn verdict_rank(verdict: &str) -> u8 {
    match verdict {
        "solved_confirmed" => 6,
        "solved_inferred" => 5,
        "partial" => 4,
        "unresolved" => 3,
        "failed" => 2,
        "abandoned" => 1,
        _ => 0,
    }
}

fn prefer_feedback_band(rules: &str, llm: &str) -> String {
    let rank = |band: &str| match band {
        "explicit_positive" | "explicit_negative" => 3,
        "neutral" => 2,
        _ => 1,
    };
    if rank(rules) >= rank(llm) {
        rules.to_string()
    } else {
        llm.to_string()
    }
}

fn tool_outcome_is_success(content: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(success) = v.get("success").and_then(|b| b.as_bool()) {
            return success;
        }
    }
    let lower = content.to_ascii_lowercase();
    if lower.contains("\"success\": false") || lower.contains("'success': false") {
        return false;
    }
    lower.contains("\"success\": true") || lower.contains("'success': true")
}

fn tool_outcome_is_failure(content: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(success) = v.get("success").and_then(|b| b.as_bool()) {
            return !success;
        }
    }
    let lower = content.to_ascii_lowercase();
    lower.contains("\"success\": false")
        || lower.contains("'success': false")
        || (lower.contains("error") && !lower.contains("\"success\": true"))
}

fn resolution_payload(
    verdict: &str,
    confidence_band: &str,
    evidence_tier: &str,
    user_feedback_band: &str,
    objective_check_band: Option<String>,
    signal_codes: Vec<String>,
    recovery_attempted: bool,
    correction_loops: u32,
) -> ResolutionPayload {
    ResolutionPayload {
        verdict: verdict.to_string(),
        confidence_band: confidence_band.to_string(),
        evidence_tier: evidence_tier.to_string(),
        user_feedback_band: user_feedback_band.to_string(),
        objective_check_band,
        signal_codes,
        recovery_attempted,
        correction_loops,
    }
}

fn objective_band(signals: &SessionSignals) -> Option<String> {
    if signals.tool_successes == 0 && signals.tool_failures == 0 {
        return Some("not_applicable".to_string());
    }
    if signals.tool_failures > signals.tool_successes {
        Some("fail".to_string())
    } else {
        Some("pass".to_string())
    }
}

fn matches_feedback(text: &str, patterns: &[&str]) -> bool {
    let lower = text.to_ascii_lowercase();
    patterns.iter().any(|p| lower.contains(p))
}

fn count_correction_loops(messages: &[String]) -> u32 {
    messages
        .iter()
        .filter(|m| matches_feedback(m, CORRECTION_PATTERNS))
        .count() as u32
}

const POSITIVE_PATTERNS: &[&str] = &[
    "解决了",
    "可以了",
    "好的",
    "谢谢",
    "感谢",
    "perfect",
    "works now",
    "that worked",
    "looks good",
    "great job",
    "ok thanks",
    "没问题",
];

const NEGATIVE_PATTERNS: &[&str] = &[
    "不对",
    "不行",
    "还是错",
    "没用",
    "不要这样",
    "wrong",
    "not working",
    "doesn't work",
    "still broken",
    "try again",
    "incorrect",
];

const CORRECTION_PATTERNS: &[&str] = &[
    "不对",
    "错了",
    "应该是",
    "别这样",
    "instead",
    "don't do",
    "stop doing",
    "not like that",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionSkillSummary;

    #[test]
    fn explicit_positive_yields_solved_confirmed() {
        let messages = vec![
            serde_json::json!({"role":"user","content":"help with ledger reconciliation"}),
            serde_json::json!({"role":"user","content":"可以了，谢谢"}),
        ];
        let signals = analyze_session(&messages, &SessionSkillSummary::default());
        let resolution = fuse_verdict(&signals);
        assert_eq!(resolution.verdict, "solved_confirmed");
        assert_eq!(resolution.evidence_tier, "A");
    }

    #[test]
    fn skill_patched_yields_tier_c_without_explicit_thanks() {
        let messages = vec![
            serde_json::json!({"role":"user","content":"help me build a douyin skill"}),
            serde_json::json!({"role":"user","content":"continue with the outline"}),
        ];
        let summary = SessionSkillSummary {
            slugs: vec!["douyin-content".to_string()],
            patch_count: 1,
            skill_created: false,
            ..Default::default()
        };
        let signals = analyze_session(&messages, &summary);
        let resolution = fuse_verdict(&signals);
        assert_eq!(resolution.verdict, "solved_inferred");
        assert_eq!(resolution.evidence_tier, "C");
        assert!(
            resolution
                .signal_codes
                .contains(&"skill_patched_this_session".to_string())
        );
    }

    #[test]
    fn hybrid_merge_upgrades_unresolved_with_llm() {
        let signals = SessionSignals {
            user_turns: 3,
            tool_failures: 0,
            tool_successes: 0,
            skill_patched: true,
            skill_created: false,
            explicit_positive: false,
            explicit_negative: false,
            correction_loops: 0,
            closure_without_followup: true,
        };
        let rules = fuse_verdict(&signals);
        assert_eq!(rules.evidence_tier, "C");
        let llm = ResolutionPayload {
            verdict: "solved_inferred".to_string(),
            confidence_band: "medium".to_string(),
            evidence_tier: "B".to_string(),
            user_feedback_band: "neutral".to_string(),
            objective_check_band: Some("not_applicable".to_string()),
            signal_codes: vec!["closure_without_followup".to_string()],
            recovery_attempted: false,
            correction_loops: 0,
        };
        let merged = merge_resolution_hybrid(&signals, &rules, Some(&llm));
        assert_eq!(merged.evidence_tier, "B");
    }
}
