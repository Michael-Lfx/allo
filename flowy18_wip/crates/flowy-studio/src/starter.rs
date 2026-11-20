//! Home starter prompts: a few clickable tasks above the composer.

use flowy_ui as theme;
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px};

/// One selectable starter; both keys live in the locale files.
struct Starter {
    title_key: &'static str,
    prompt_key: &'static str,
}

struct StarterGroup {
    title_key: &'static str,
    items: &'static [Starter],
}

const GROUPS: &[StarterGroup] = &[
    StarterGroup {
        title_key: "home.starter.group.website",
        items: &[
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
        ],
    },
    StarterGroup {
        title_key: "home.starter.group.app",
        items: &[
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
        ],
    },
    StarterGroup {
        title_key: "home.starter.group.game",
        items: &[
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
        ],
    },
    StarterGroup {
        title_key: "home.starter.group.docs",
        items: &[
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
        ],
    },
    StarterGroup {
        title_key: "home.starter.group.data",
        items: &[
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
        ],
    },
];

const STARTER_HEADING_KEY: &str = "auto.suggestions";
const CONFIRM_LABEL_KEY: &str = "home.starter.replace";
const CONFIRM_HINT_KEY: &str = "home.starter.replace_hint";

impl crate::panel::StudioPanel {
    pub fn starter_section(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let heading = self.t(STARTER_HEADING_KEY);
        let mut column = div()
            .w_full()
            .max_w(px(760.))
            .flex()
            .flex_col()
            .gap(self.theme.space(12.))
            .child(
                div()
                    .id("home-starters-heading")
                    .role(gpui::Role::Heading)
                    .aria_level(2)
                    .aria_label(heading.clone())
                    .text_sm()
                    .text_color(self.theme.muted())
                    .child(heading),
            );
        for group in GROUPS {
            let group_heading = self.t(group.title_key);
            let mut section = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(self.theme.space(6.))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(self.theme.muted())
                        .child(group_heading),
                );
            for starter in group.items {
                section = section.child(self.starter_row(starter, cx));
            }
            column = column.child(section);
        }
        div()
            .id("home-starters")
            .w_full()
            .px(self.theme.space(32.))
            .flex()
            .justify_center()
            .child(
                // Five groups are taller than the empty home shows at once.
                div()
                    .id("home-starters-scroll")
                    .w_full()
                    .max_w(px(760.))
                    .max_h(px(320.))
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(column),
            )
    }

    fn starter_row(
        &self,
        starter: &Starter,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let title = self.t(starter.title_key);
        let prompt = self.t(starter.prompt_key);
        theme::interactive(
            SharedString::from(format!("home-starter-{}", starter.title_key)),
            title.clone(),
            &self.theme,
        )
        .w_full()
        .px(self.theme.space(16.))
        .py(self.theme.space(12.))
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .flex()
        .flex_col()
        .gap(self.theme.space(2.))
        .hover(|style| style.bg(self.theme.soft()))
        .child(div().text_sm().child(title))
        .child(
            div()
                .text_xs()
                .text_color(self.theme.muted())
                .child(prompt.clone()),
        )
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.request_fill_prompt(prompt.clone(), cx);
        }))
    }

    /// Replace confirmation shown below the starters while a draft is kept.
    pub(crate) fn starter_confirm_row(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("home-starter-confirm")
            .w_full()
            .px(self.theme.space(32.))
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(self.theme.muted())
                            .child(self.t(CONFIRM_HINT_KEY)),
                    )
                    .child(
                        theme::Button::new(
                            "home-starter-cancel",
                            self.t("thread.cancel"),
                        )
                        .variant(theme::ControlVariant::Secondary)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.cancel_pending_starter(cx);
                            }),
                        ),
                    )
                    .child(
                        theme::Button::new(
                            "home-starter-confirm",
                            self.t(CONFIRM_LABEL_KEY),
                        )
                        .variant(theme::ControlVariant::Primary)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.confirm_pending_starter(cx);
                            }),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{StudioConfig, StudioPanel};
    use flowy_agent::{LlmConfig, Model, Plan};
    use flowy_i18n::I18n;
    use flowy_ui::UiContext;
    use gpui::{Entity, TestAppContext};
    use std::path::PathBuf;

    fn test_panel(
        cx: &TestAppContext,
        name: &str,
    ) -> (Entity<StudioPanel>, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-starter-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let panel = cx.update(|cx| {
            let ui =
                UiContext::new(flowy_ui::ThemeOptions::default(), false, cx);
            cx.new(|cx| {
                StudioPanel::new(
                    StudioConfig {
                        product: "Flowy",
                        i18n: I18n::embedded("Flowy"),
                        model: Model::Lite,
                        plan: Plan::Free,
                        model_info_version: String::new(),
                        model_info_open: false,
                        store_root: root.clone(),
                        llm: LlmConfig {
                            api_key: None,
                            base_url: String::new(),
                            model: String::new(),
                            provider: String::new(),
                            ..LlmConfig::default()
                        },
                    },
                    ui,
                    cx,
                )
            })
        });
        (panel, root)
    }

    #[gpui::test]
    fn empty_prompt_is_filled_directly(cx: &mut TestAppContext) {
        let (panel, root) = test_panel(cx, "empty");
        panel.update(cx, |panel, cx| {
            panel.request_fill_prompt("starter copy".into(), cx);
        });
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).as_str(), "starter copy");
            assert!(panel.pending_starter.is_none());
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn draft_is_kept_and_cancel_leaves_it_untouched(cx: &mut TestAppContext) {
        let (panel, root) = test_panel(cx, "draft");
        panel.update(cx, |panel, cx| {
            panel.fill_prompt("my draft", cx);
            panel.request_fill_prompt("starter copy".into(), cx);
        });
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).as_str(), "my draft");
            assert_eq!(panel.pending_starter.as_deref(), Some("starter copy"));
        });
        panel.update(cx, |panel, cx| panel.cancel_pending_starter(cx));
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).as_str(), "my draft");
            assert!(panel.pending_starter.is_none());
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn confirm_replaces_the_draft_with_the_starter(cx: &mut TestAppContext) {
        let (panel, root) = test_panel(cx, "confirm");
        panel.update(cx, |panel, cx| {
            panel.fill_prompt("my draft", cx);
            panel.request_fill_prompt("starter copy".into(), cx);
            panel.confirm_pending_starter(cx);
        });
        cx.read_entity(&panel, |panel, cx| {
            assert_eq!(panel.prompt.read(cx).as_str(), "starter copy");
            assert!(panel.pending_starter.is_none());
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }
}
