//! Per-round continuation prompt. Intentionally **not** byte-identical across
//! turns: a cache-stable "keep going" is what produced empty Goal loops.
//! Goal *context* on the turn tail stays cache-stable; only the continuation
//! user message carries the delta.

const CONTINUATION_DELTA_TEMPLATE: &str = include_str!("../goal/templates/continuation_delta.md");

#[derive(Debug, Clone, Default)]
pub struct ContinuationDelta {
    pub objective: String,
    pub criteria: String,
    pub world_delta: String,
    pub remaining: usize,
    pub missing_verification: String,
}

pub fn render_continuation_delta(delta: &ContinuationDelta) -> String {
    CONTINUATION_DELTA_TEMPLATE
        .replace("{{objective}}", &delta.objective)
        .replace("{{criteria}}", &delta.criteria)
        .replace("{{delta}}", &delta.world_delta)
        .replace("{{remaining}}", &delta.remaining.to_string())
        .replace("{{missing_verification}}", &delta.missing_verification)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_every_placeholder() {
        let text = render_continuation_delta(&ContinuationDelta {
            objective: "ship csv export".into(),
            criteria: "- Verification: bun test".into(),
            world_delta: "- mutation this request: no".into(),
            remaining: 2,
            missing_verification: "No successful verification command yet.".into(),
        });
        assert!(text.contains("ship csv export"));
        assert!(text.contains("bun test"));
        assert!(text.contains("mutation this request: no"));
        assert!(text.contains('2'));
        assert!(text.contains("No successful verification"));
        assert!(!text.contains("{{"));
    }
}
