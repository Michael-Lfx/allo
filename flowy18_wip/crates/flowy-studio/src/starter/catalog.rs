//! Starter catalogue: four calm one-click prompts for the Home stage.

/// One prompt the user can drop into the composer.
pub struct Starter {
    pub(crate) title_key: &'static str,
    pub(crate) prompt_key: &'static str,
}

/// Curated everyday prompts shown as quiet pills below the Home composer.
pub const CURATED: &[Starter] = &[
    Starter {
        title_key: "home.starter.docs.minutes.title",
        prompt_key: "home.starter.docs.minutes.prompt",
    },
    Starter {
        title_key: "home.starter.data.csv.title",
        prompt_key: "home.starter.data.csv.prompt",
    },
    Starter {
        title_key: "home.starter.docs.weekly.title",
        prompt_key: "home.starter.docs.weekly.prompt",
    },
    Starter {
        title_key: "home.starter.website.launch.title",
        prompt_key: "home.starter.website.launch.prompt",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_starters_have_four_unique_examples_with_both_keys() {
        assert_eq!(CURATED.len(), 4);
        let mut seen = std::collections::HashSet::new();
        for starter in CURATED {
            assert_ne!(starter.title_key.len(), 0);
            assert_ne!(starter.prompt_key.len(), 0);
            assert!(
                seen.insert(starter.title_key),
                "duplicate starter title key {}",
                starter.title_key
            );
        }
    }
}
