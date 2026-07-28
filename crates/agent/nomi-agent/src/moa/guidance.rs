//! Formatting of reference advice into a single turn-tail guidance block.

use super::MoaAdvice;

/// Format advisor responses into one guidance block appended to the turn
/// tail of the aggregator request. Failed slots carry a `"[failed: …]"`
/// sentinel in `text`; the caller filters an all-failed round out entirely
/// (returns `None` from the runner) so this never wraps pure failure noise.
pub fn format_guidance(advices: &[MoaAdvice]) -> String {
    let labels = advices
        .iter()
        .map(|a| a.label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = String::new();
    out.push_str("[Mixture of Agents reference context]\n");
    out.push_str(&format!("References: {labels}\n\n"));
    out.push_str(
        "Use the reference responses below as private context. You are the \
         aggregator and acting model: answer the user directly or call tools \
         as needed.",
    );
    for (idx, advice) in advices.iter().enumerate() {
        out.push_str(&format!(
            "\n\nReference {} — {}:\n{}",
            idx + 1,
            advice.label,
            advice.text
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_header_labels_and_numbered_blocks() {
        let advices = vec![
            MoaAdvice {
                label: "openai/gpt-x".into(),
                text: "Try A first.".into(),
            },
            MoaAdvice {
                label: "anthropic/claude-y".into(),
                text: "Watch out for B.".into(),
            },
        ];
        let out = format_guidance(&advices);
        assert!(out.starts_with("[Mixture of Agents reference context]\n"));
        assert!(out.contains("References: openai/gpt-x, anthropic/claude-y"));
        assert!(out.contains("Reference 1 — openai/gpt-x:\nTry A first."));
        assert!(out.contains("Reference 2 — anthropic/claude-y:\nWatch out for B."));
    }

    #[test]
    fn failed_slot_sentinel_is_passed_through() {
        let advices = vec![MoaAdvice {
            label: "p/m".into(),
            text: "[failed: timeout after 120s]".into(),
        }];
        let out = format_guidance(&advices);
        assert!(out.contains("Reference 1 — p/m:\n[failed: timeout after 120s]"));
    }
}
