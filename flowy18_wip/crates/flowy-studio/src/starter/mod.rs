//! Home starter browsing.
//!
//! One selector plus one content row. A centred slogan anchors the stage, a
//! row of intent chips picks the category, and the four examples below swap in
//! place. Picking an example overwrites the composer so the prompt can be
//! previewed without sending it.

pub(crate) mod catalog;



use crate::panel::StudioPanel;
use catalog::{Category, Starter};
use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, Context, IntoElement, ObjectFit, SharedString, div,
    prelude::*, px,
};

/// Width of the whole stage, shared by the slogan, the chips and the cards.
const STAGE_W: f32 = 880.;
const COVER_H: f32 = 132.;

impl StudioPanel {
    /// Slogan, category chips and the current category's four examples.
    pub fn starter_section(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let category = self.starter_category();
        let title = self.t("home.title");
        let mut strip = div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap(self.theme.space(12.));
        for (index, starter) in category.examples().iter().enumerate() {
            strip = strip.child(self.example_card(starter, index as u64, cx));
        }
        div()
            .id("home-starters")
            .w_full()
            .max_w(px(STAGE_W))
            .flex()
            .flex_col()
            .items_center()
            .gap(self.theme.space(18.))
            .child(
                div()
                    .id("home-goal")
                    .role(gpui::Role::Heading)
                    .aria_level(1)
                    .aria_label(title.clone())
                    .text_2xl()
                    .text_center()
                    .child(title),
            )
            .child(self.category_chips(cx))
            .child(
                strip
                    .with_animation(
                        // Keyed by category so switching replays the entrance.
                        ("home-starter-cards", category as u32 as u64),
                        Animation::new(crate::motion::MODERATE)
                            .with_easing(crate::motion::entrance()),
                        |strip, delta| {
                            // Movement carries the motion; the fade alone was
                            // invisible on the dark surface.
                            strip.opacity(delta).relative().top(px(
                                crate::motion::ENTER_OFFSET * (1.0 - delta),
                            ))
                        },
                    )
                    .into_any_element(),
            )
    }

    fn category_chips(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let mut row = div()
            .w_full()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(self.theme.space(6.));
        for category in Category::ALL {
            row = row.child(self.category_chip(category, cx));
        }
        row
    }

    /// Geometry stays put across hover and focus; only colours change.
    fn category_chip(
        &self,
        category: Category,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let title = self.t(category.title_key());
        let selected = category == self.starter_category();
        let mut chip = theme::interactive(
            ("home-starter-chip", category as u32 as u64),
            SharedString::from(title.clone()),
            &self.theme,
        )
        .px(self.theme.space(12.))
        .py(self.theme.space(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .child(
            // Same weight for every chip: a bold selected label changes the
            // row's width and shifts the centred slogan.
            div().text_sm().child(title),
        );
        let soft = self.theme.soft();
        chip = if selected {
            chip.bg(soft).text_color(self.theme.accent())
        } else {
            chip.text_color(self.theme.muted())
                .hover(move |style| style.bg(soft))
        };
        chip.on_click(cx.listener(move |panel, _, _, cx| {
            panel.set_starter_category(category, cx);
        }))
    }

    fn example_card(
        &self,
        starter: &Starter,
        index: u64,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let title = self.t(starter.title_key);
        let prompt = self.t(starter.prompt_key);
        let cover = starter.cover();
        theme::interactive(
            ("home-starter-example", index),
            SharedString::from(title.clone()),
            &self.theme,
        )
        .flex_1()
        // Below this the card becomes unreadable, so wrap instead of squeezing.
        .min_w(px(180.))
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(self.theme.line())
        .bg(self.theme.card())
        .overflow_hidden()
        .hover(|style| style.bg(self.theme.soft()))
        .child(
            // Media sits flush against the rounded top, so it carries the
            // corner itself (GPUI clips rectangles, not curves).
            div()
                .w_full()
                .h(px(COVER_H))
                .rounded_t_xl()
                .overflow_hidden()
                .child(
                    cover
                        .w_full()
                        .h(px(COVER_H))
                        .object_fit(ObjectFit::Cover),
                ),
        )
        .child(
            div()
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .text_sm()
                .child(title),
        )
        .on_click(cx.listener(move |panel, _, _, cx| {
            // Picking previews the prompt; it never sends.
            panel.request_fill_prompt(prompt.clone(), cx);
        }))
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
    fn picking_an_example_overwrites_the_draft(cx: &mut TestAppContext) {
        let (panel, root) = test_panel(cx, "overwrite");
        panel.update(cx, |panel, cx| {
            panel.fill_prompt("my draft", cx);
            panel.request_fill_prompt("starter copy".into(), cx);
        });
        cx.read_entity(&panel, |panel, cx| {
            // The click means "show me this prompt", so the draft is replaced
            // without asking.
            assert_eq!(panel.prompt.read(cx).as_str(), "starter copy");
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    fn switching_category_keeps_the_stage_in_sync(cx: &mut TestAppContext) {
        let (panel, root) = test_panel(cx, "switch");
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.starter_category(), Category::Website);
        });
        panel.update(cx, |panel, cx| {
            panel.set_starter_category(Category::Docs, cx);
        });
        cx.read_entity(&panel, |panel, _| {
            assert_eq!(panel.starter_category(), Category::Docs);
        });
        drop(panel);
        let _ = std::fs::remove_dir_all(root);
    }
}
