//! Starter catalogue: categories and curated prompt examples.

/// One prompt the user can drop into the composer.
pub struct Starter {
    pub(crate) title_key: &'static str,
    pub(crate) prompt_key: &'static str,
}

/// The five starter buckets, each owning four curated prompt examples.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Website,
    App,
    Game,
    Docs,
    Data,
}

impl Category {
    pub(crate) const ALL: [Self; 5] =
        [Self::Website, Self::App, Self::Game, Self::Docs, Self::Data];

    pub(crate) const fn title_key(self) -> &'static str {
        match self {
            Self::Website => "home.starter.group.website",
            Self::App => "home.starter.group.app",
            Self::Game => "home.starter.group.game",
            Self::Docs => "home.starter.group.docs",
            Self::Data => "home.starter.group.data",
        }
    }

    pub(crate) const fn suggested_skill_id(self) -> &'static str {
        match self {
            Self::Data => "excel",
            Self::Docs => "research",
            Self::Website | Self::App | Self::Game => "script",
        }
    }

    pub(crate) const fn examples(self) -> &'static [Starter] {
        match self {
            Self::Website => WEBSITE,
            Self::App => APP,
            Self::Game => GAME,
            Self::Docs => DOCS,
            Self::Data => DATA,
        }
    }
}

const WEBSITE: &[Starter] = &[
    Starter {
        title_key: "home.starter.website.portfolio.title",
        prompt_key: "home.starter.website.portfolio.prompt",
    },
    Starter {
        title_key: "home.starter.website.cafe.title",
        prompt_key: "home.starter.website.cafe.prompt",
    },
    Starter {
        title_key: "home.starter.website.launch.title",
        prompt_key: "home.starter.website.launch.prompt",
    },
    Starter {
        title_key: "home.starter.website.docs.title",
        prompt_key: "home.starter.website.docs.prompt",
    },
];

const APP: &[Starter] = &[
    Starter {
        title_key: "home.starter.app.todo.title",
        prompt_key: "home.starter.app.todo.prompt",
    },
    Starter {
        title_key: "home.starter.app.ledger.title",
        prompt_key: "home.starter.app.ledger.prompt",
    },
    Starter {
        title_key: "home.starter.app.study.title",
        prompt_key: "home.starter.app.study.prompt",
    },
    Starter {
        title_key: "home.starter.app.habit.title",
        prompt_key: "home.starter.app.habit.prompt",
    },
];

const GAME: &[Starter] = &[
    Starter {
        title_key: "home.starter.game.quiz.title",
        prompt_key: "home.starter.game.quiz.prompt",
    },
    Starter {
        title_key: "home.starter.game.memory.title",
        prompt_key: "home.starter.game.memory.prompt",
    },
    Starter {
        title_key: "home.starter.game.snake.title",
        prompt_key: "home.starter.game.snake.prompt",
    },
    Starter {
        title_key: "home.starter.game.typing.title",
        prompt_key: "home.starter.game.typing.prompt",
    },
];

const DOCS: &[Starter] = &[
    Starter {
        title_key: "home.starter.docs.minutes.title",
        prompt_key: "home.starter.docs.minutes.prompt",
    },
    Starter {
        title_key: "home.starter.docs.readme.title",
        prompt_key: "home.starter.docs.readme.prompt",
    },
    Starter {
        title_key: "home.starter.docs.prd.title",
        prompt_key: "home.starter.docs.prd.prompt",
    },
    Starter {
        title_key: "home.starter.docs.weekly.title",
        prompt_key: "home.starter.docs.weekly.prompt",
    },
];

const DATA: &[Starter] = &[
    Starter {
        title_key: "home.starter.data.csv.title",
        prompt_key: "home.starter.data.csv.prompt",
    },
    Starter {
        title_key: "home.starter.data.clean.title",
        prompt_key: "home.starter.data.clean.prompt",
    },
    Starter {
        title_key: "home.starter.data.compare.title",
        prompt_key: "home.starter.data.compare.prompt",
    },
    Starter {
        title_key: "home.starter.data.script.title",
        prompt_key: "home.starter.data.script.prompt",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_examples_and_a_title() {
        for category in Category::ALL {
            assert!(
                !category.examples().is_empty(),
                "{category:?} has no examples"
            );
            assert_ne!(category.title_key().len(), 0);
        }
    }

    #[test]
    fn every_example_has_both_keys() {
        for category in Category::ALL {
            for starter in category.examples() {
                assert_ne!(starter.title_key.len(), 0);
                assert_ne!(starter.prompt_key.len(), 0);
            }
        }
    }

    #[test]
    fn titles_are_unique_across_categories() {
        let mut seen = std::collections::HashSet::new();
        for category in Category::ALL {
            for starter in category.examples() {
                assert!(
                    seen.insert(starter.title_key),
                    "duplicate title key {}",
                    starter.title_key
                );
            }
        }
    }
}
