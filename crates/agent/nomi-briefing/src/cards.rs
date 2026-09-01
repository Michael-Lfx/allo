//! Original news-card catalog. IDs must match `packaging/briefing-compositor`.
//! Distinctive ink-desk visual language. Do not copy third-party card source.

pub const CARD_CATALOG: [&str; 8] = [
    "title_desk",
    "evidence_tour",
    "highlighter",
    "number_roll",
    "source_bar",
    "yield_shrink",
    "transition_wipe",
    "subtitle_plain",
];

pub fn card_exists(id: &str) -> bool {
    CARD_CATALOG.contains(&id)
}

pub fn default_card_for_visual(visual_kind: &str) -> &'static str {
    match visual_kind {
        "evidence_screenshot" => "evidence_tour",
        "generated_infographic" => "number_roll",
        "licensed_broll" | "user_asset" => "yield_shrink",
        _ => "subtitle_plain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_news_scenes() {
        assert!(card_exists("title_desk"));
        assert!(card_exists("evidence_tour"));
        assert!(!card_exists("talkcraft_apple_card"));
    }
}
