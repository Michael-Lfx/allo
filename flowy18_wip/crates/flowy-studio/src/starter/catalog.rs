//! Starter catalogue: categories, examples and their cover art.
//!
//! Pure data plus cover loaders. Layout lives in `starter::view`, interaction
//! state in `starter`. Keeping them apart lets the visuals be replaced without
//! touching the prompt inventory.

use std::sync::{Arc, OnceLock};

use gpui::{Image, ImageFormat, Img, img};

/// One prompt the user can drop into the composer.
pub struct Starter {
    pub(crate) title_key: &'static str,
    pub(crate) prompt_key: &'static str,
    cover: fn() -> Img,
}

impl Starter {
    pub(crate) fn cover(&self) -> Img {
        (self.cover)()
    }
}

/// The five starter buckets. Each owns four examples and reuses its first
/// example's cover as its own.
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

/// Full-colour cover art, decoded as an image so the authored palette survives
/// instead of becoming a text-colour mask.
macro_rules! covers {
    ($($name:ident => $file:literal),+ $(,)?) => {
        $(
            fn $name() -> Img {
                static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
                img(IMAGE.get_or_init(|| {
                    Arc::new(Image::from_bytes(
                        ImageFormat::Webp,
                        include_bytes!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/assets/starters/",
                            $file
                        ))
                        .to_vec(),
                    ))
                })
                .clone())
            }
        )+
    };
}

covers! {
    cover_website_portfolio => "website-portfolio.webp",
    cover_website_cafe => "website-cafe.webp",
    cover_website_launch => "website-launch.webp",
    cover_website_docs => "website-docs.webp",
    cover_app_todo => "app-todo.webp",
    cover_app_ledger => "app-ledger.webp",
    cover_app_study => "app-study.webp",
    cover_app_habit => "app-habit.webp",
    cover_game_quiz => "game-quiz.webp",
    cover_game_memory => "game-memory.webp",
    cover_game_snake => "game-snake.webp",
    cover_game_typing => "game-typing.webp",
    cover_docs_minutes => "docs-minutes.webp",
    cover_docs_readme => "docs-readme.webp",
    cover_docs_prd => "docs-prd.webp",
    cover_docs_weekly => "docs-weekly.webp",
    cover_data_csv => "data-csv.webp",
    cover_data_clean => "data-clean.webp",
    cover_data_compare => "data-compare.webp",
    cover_data_script => "data-script.webp",
}

const WEBSITE: &[Starter] = &[
    Starter {
        title_key: "home.starter.website.portfolio.title",
        prompt_key: "home.starter.website.portfolio.prompt",
        cover: cover_website_portfolio,
    },
    Starter {
        title_key: "home.starter.website.cafe.title",
        prompt_key: "home.starter.website.cafe.prompt",
        cover: cover_website_cafe,
    },
    Starter {
        title_key: "home.starter.website.launch.title",
        prompt_key: "home.starter.website.launch.prompt",
        cover: cover_website_launch,
    },
    Starter {
        title_key: "home.starter.website.docs.title",
        prompt_key: "home.starter.website.docs.prompt",
        cover: cover_website_docs,
    },
];

const APP: &[Starter] = &[
    Starter {
        title_key: "home.starter.app.todo.title",
        prompt_key: "home.starter.app.todo.prompt",
        cover: cover_app_todo,
    },
    Starter {
        title_key: "home.starter.app.ledger.title",
        prompt_key: "home.starter.app.ledger.prompt",
        cover: cover_app_ledger,
    },
    Starter {
        title_key: "home.starter.app.study.title",
        prompt_key: "home.starter.app.study.prompt",
        cover: cover_app_study,
    },
    Starter {
        title_key: "home.starter.app.habit.title",
        prompt_key: "home.starter.app.habit.prompt",
        cover: cover_app_habit,
    },
];

const GAME: &[Starter] = &[
    Starter {
        title_key: "home.starter.game.quiz.title",
        prompt_key: "home.starter.game.quiz.prompt",
        cover: cover_game_quiz,
    },
    Starter {
        title_key: "home.starter.game.memory.title",
        prompt_key: "home.starter.game.memory.prompt",
        cover: cover_game_memory,
    },
    Starter {
        title_key: "home.starter.game.snake.title",
        prompt_key: "home.starter.game.snake.prompt",
        cover: cover_game_snake,
    },
    Starter {
        title_key: "home.starter.game.typing.title",
        prompt_key: "home.starter.game.typing.prompt",
        cover: cover_game_typing,
    },
];

const DOCS: &[Starter] = &[
    Starter {
        title_key: "home.starter.docs.minutes.title",
        prompt_key: "home.starter.docs.minutes.prompt",
        cover: cover_docs_minutes,
    },
    Starter {
        title_key: "home.starter.docs.readme.title",
        prompt_key: "home.starter.docs.readme.prompt",
        cover: cover_docs_readme,
    },
    Starter {
        title_key: "home.starter.docs.prd.title",
        prompt_key: "home.starter.docs.prd.prompt",
        cover: cover_docs_prd,
    },
    Starter {
        title_key: "home.starter.docs.weekly.title",
        prompt_key: "home.starter.docs.weekly.prompt",
        cover: cover_docs_weekly,
    },
];

const DATA: &[Starter] = &[
    Starter {
        title_key: "home.starter.data.csv.title",
        prompt_key: "home.starter.data.csv.prompt",
        cover: cover_data_csv,
    },
    Starter {
        title_key: "home.starter.data.clean.title",
        prompt_key: "home.starter.data.clean.prompt",
        cover: cover_data_clean,
    },
    Starter {
        title_key: "home.starter.data.compare.title",
        prompt_key: "home.starter.data.compare.prompt",
        cover: cover_data_compare,
    },
    Starter {
        title_key: "home.starter.data.script.title",
        prompt_key: "home.starter.data.script.prompt",
        cover: cover_data_script,
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
